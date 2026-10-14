//! Looking things up without making the player wait: MapleSyrup answers at
//! once with what it knows, and the check runs in the background. What it
//! finds when the quick answer was wrong (or when the player asked for the
//! look-up) is shown on the phone, never said — one answer, not two — and
//! kept for next time.

use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::Duration;

use serde_json::json;

use super::openai::{AiError, Ask, OpenAi};

/// How a question says it is about MapleStory Classic World (the
/// conversation adds it when the player plays it): checked for that world,
/// not today's game.
pub const CLASSIC: &str = "MapleStory Classic World";

/// What the look-ups found lately, to be shown and never said: the player
/// got one answer, and a second spoken over the next thing ("Actually…",
/// "Your buddy's answer is basically right…") was a second answer —
/// wrong, more than once, for his world. The main loop hands a finding to
/// the worker as one of MapleSyrup's own lines (`Job::Say`); the worker
/// asks [`shown_only`] and shows it on the phone without a word.
static FINDINGS: Mutex<VecDeque<String>> = Mutex::new(VecDeque::new());

/// Keep `text` as a look-up's finding (see [`FINDINGS`]).
fn found(text: &str) {
    let mut findings = FINDINGS.lock().unwrap_or_else(|e| e.into_inner());
    findings.push_back(text.trim().to_string());
    while findings.len() > 16 {
        findings.pop_front();
    }
}

/// Whether `text` is what a look-up found (and so is shown, never said):
/// taken off the list as it is asked about.
pub fn shown_only(text: &str) -> bool {
    let mut findings = FINDINGS.lock().unwrap_or_else(|e| e.into_inner());
    match findings.iter().position(|f| f == text.trim()) {
        Some(at) => {
            findings.remove(at);
            true
        }
        None => false,
    }
}

/// What to show after a look-up: `None` when the quick answer was right.
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
        "They asked for it to be looked up: give the answer in one short sentence."
    } else {
        "If the buddy's answer is right or close enough, reply with exactly OK. If it's wrong or misses something \
that matters, reply with the right answer in one short sentence."
    };
    // The version they play: Classic World is not today's game.
    let version = if question.to_lowercase().contains("classic world") {
        "MapleStory Classic World — the game as it was long ago, not today's GMS: none of today's systems \
(no Maple Guide, no world-map search, no Arcane River, no Drop Coupons); only what holds in Classic World counts"
    } else {
        "the current global version, GMS"
    };
    let ask = Ask {
        instructions: format!(
            "You check a MapleStory gaming buddy's quick answer on the web ({version}; prefer maplestorywiki.net \
and maplestory.nexon.net). {task} If you can't find it for that version, reply with exactly OK: never a guess, \
never a map, NPC, recipe or route you didn't find. Write {tongue}, casual and short, no links, no sources; it is \
shown on their phone, never said."
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
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("ok") {
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
                let result = match check(&openai, &question, &said, asked, language.as_deref()) {
                    Ok(Some(text)) => {
                        if let Some(learning) = &learning {
                            learning.knowledge().add(
                                &question,
                                &text,
                                super::knowledge::Source::Web,
                            );
                        }
                        // Shown, never said: one answer, not two.
                        found(&text);
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
                let _ = tx.send(result);
            });
    }
}
