//! Looking things up without making the player wait: MapleSyrup answers at
//! once with what it knows, and the check runs in the background. It speaks
//! again only when it was wrong (or when the player asked for the look-up),
//! and what was found is kept for next time.

use std::time::Duration;

use serde_json::json;

use super::openai::{AiError, Ask, OpenAi};

/// What to say after a look-up: `None` when the quick answer was right.
pub fn check(
    openai: &OpenAi,
    question: &str,
    said: &str,
    asked: bool,
    language: Option<&str>,
) -> Result<Option<String>, AiError> {
    let tongue = match language {
        Some(l) => format!(
            "in {} (or the language of the question)",
            super::language::name(l)
        ),
        None => "in the language of the question".to_string(),
    };
    let task = if asked {
        "They asked for it to be looked up: give the answer in one short spoken sentence."
    } else {
        "If the buddy's answer is right or close enough, reply with exactly OK. If it's wrong or misses something \
that matters, reply with the right answer in one short spoken sentence, starting the way a friend would \
(\"Actually...\")."
    };
    let ask = Ask {
        instructions: format!(
            "You check a MapleStory gaming buddy's quick answer on the web (the current global version, GMS; \
prefer maplestorywiki.net and maplestory.nexon.net). {task} Speak {tongue}, casual and short, no links, no sources."
        ),
        input: vec![json!({"role": "user", "content": format!(
            "The player asked: {question}\nThe buddy answered: {said}"
        )})],
        tools: vec![json!({"type": "web_search", "search_context_size": "low"})],
        max_output_tokens: 200,
        timeout: Duration::from_secs(60),
        ..Default::default()
    };
    let answer = super::brain::for_speech(&openai.ask(&ask, None)?.text);
    let trimmed = answer
        .trim()
        .trim_matches(|c: char| c.is_ascii_punctuation() || c.is_whitespace());
    if trimmed.is_empty() || (!asked && trimmed.eq_ignore_ascii_case("ok")) {
        return Ok(None);
    }
    Ok(Some(answer))
}

/// What a background look-up came back with.
#[derive(Debug, Clone, PartialEq)]
pub enum Found {
    /// Something to say: a correction, or the answer they asked for.
    Say(String),
    /// The quick answer was right: nothing to say.
    Right,
    /// It couldn't be looked up (for the log).
    Failed(String),
}

/// Runs look-ups in the background, one thread each, and keeps what they
/// find (`knowledge`, as looked up).
pub struct Lookups {
    openai: std::sync::Arc<OpenAi>,
    learning: Option<super::memory::Learning>,
    tx: std::sync::mpsc::Sender<Found>,
}

impl Lookups {
    pub fn new(
        openai: std::sync::Arc<OpenAi>,
        learning: Option<super::memory::Learning>,
    ) -> (Lookups, std::sync::mpsc::Receiver<Found>) {
        let (tx, rx) = std::sync::mpsc::channel();
        (
            Lookups {
                openai,
                learning,
                tx,
            },
            rx,
        )
    }

    /// Check `said` (the quick answer to `question`) in the background.
    pub fn start(&self, question: &str, said: &str, asked: bool, language: Option<&str>) {
        let (openai, learning, tx) = (
            std::sync::Arc::clone(&self.openai),
            self.learning.clone(),
            self.tx.clone(),
        );
        let (question, said, language) = (
            question.to_string(),
            said.to_string(),
            language.map(String::from),
        );
        let _ = std::thread::Builder::new()
            .name("look-up".into())
            .spawn(move || {
                let found = match check(&openai, &question, &said, asked, language.as_deref()) {
                    Ok(Some(text)) => {
                        if let Some(learning) = &learning {
                            learning.knowledge().add(
                                &question,
                                &text,
                                super::knowledge::Source::Web,
                            );
                        }
                        Found::Say(text)
                    }
                    Ok(None) => {
                        if let Some(learning) = &learning
                            && !said.trim().is_empty()
                        {
                            learning.knowledge().add(
                                &question,
                                &said,
                                super::knowledge::Source::Web,
                            );
                        }
                        Found::Right
                    }
                    Err(e) => Found::Failed(format!("couldn't look up \"{question}\": {e}")),
                };
                let _ = tx.send(found);
            });
    }
}
