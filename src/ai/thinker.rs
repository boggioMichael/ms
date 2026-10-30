//! The thinker behind the live call. A question the call's voice model
//! hands over because it needs working out — exact numbers, a plan or a
//! route, a comparison — is thought through by Claude while a Claude Code
//! session listens, else by MapleSyrup's own model at more effort than the
//! call's. The call says it will think a moment first, like a person, and
//! says the answer when it comes; the talk goes on meanwhile.
//!
//! ```text
//!   call (voice model) ── think_it_through ──▶ PC ──▶ Claude (the board's question)
//!          ▲                                    │        └─ no answer in CLAUDE_WAIT ─┐
//!          └──────────── the answer ◀───────────┴──── MapleSyrup's own model ◀───────┘
//! ```

use std::time::Duration;

use serde_json::json;

use super::openai::{AiError, Ask, OpenAi};

/// How long the call waits for Claude's answer before MapleSyrup's own
/// model thinks it through instead (Claude's brief says so).
pub const CLAUDE_WAIT: Duration = Duration::from_secs(20);
/// How hard MapleSyrup's own model thinks.
pub const EFFORT: &str = "medium";
/// The most its answer may take, its thinking included.
const MAX_OUTPUT_TOKENS: u32 = 4000;
/// How long it may take.
const TIMEOUT: Duration = Duration::from_secs(30);

/// What MapleSyrup's own model is told when it is the thinker.
pub const INSTRUCTIONS: &str = "You are the thinker behind MapleSyrup, a companion for MapleStory. Its voice is on a \
live call with the player and hands you a question that needs working out: exact numbers (EXP per hour, time to a \
level, potions, mesos), a plan or a route (where to train, a quest's steps, a build), or a comparison. You get the \
question and the game as MapleSyrup's vision engine reads it right now (level, HP, MP, EXP, the map, the session's \
rates). Work it out carefully and compute numbers exactly from the reading. Never invent maps, monsters, quests or \
drops: when you're not sure, say so. Answer in the player's language, for the voice to say: the answer first, in one \
to three short sentences, with the numbers that matter. No lists, no markdown.";

/// The question as it goes to Claude's board: what the call asked, for
/// him (the reading goes with it there).
pub fn headline(question: &str) -> String {
    format!("MapleSyrup's voice asks you to think this through for him: {question}")
}

/// What MapleSyrup's own model is given: the game as read now, then the
/// question.
fn input(question: &str, reading: &str) -> String {
    format!(
        "[The game right now, read by MapleSyrup's vision engine]\n{reading}\n\n[The question]\n{question}"
    )
}

/// MapleSyrup's own model thinks `question` through, with the game as
/// `reading` has it.
pub fn ask_own(chat: &OpenAi, question: &str, reading: &str) -> Result<String, AiError> {
    let ask = Ask {
        instructions: INSTRUCTIONS.to_string(),
        input: vec![json!({"role": "user", "content": input(question, reading)})],
        max_output_tokens: MAX_OUTPUT_TOKENS,
        timeout: TIMEOUT,
        effort: Some(EFFORT.to_string()),
        ..Default::default()
    };
    chat.ask(&ask, None)
        .map(|answer| answer.text.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_question_goes_with_the_game_as_read_and_names_who_asks() {
        let text = input("how long to level 30?", "level 27, EXP 41%");
        assert!(text.find("level 27").unwrap() < text.find("how long").unwrap());
        assert!(headline("how long to level 30?").ends_with("how long to level 30?"));
        assert!(INSTRUCTIONS.contains("compute numbers exactly"));
        assert!(INSTRUCTIONS.contains("Never invent"));
    }
}
