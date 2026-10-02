//! The conversation: who MapleSyrup is, what it sees right now, and what
//! was said so far. The model gets all three with every sentence, so it can
//! talk about the game the way a friend watching it would.

use std::collections::VecDeque;

use super::openai::Turn;
use crate::companion::{GameView, Gauge, Observation, Progress};

/// Turns of conversation kept (a turn is one sentence each way).
const KEEP_TURNS: usize = 16;

/// How the voice should sound.
pub const VOICE_STYLE: &str = "You are a warm, upbeat friend hanging out next to someone playing a video game. \
Speak naturally and casually, like a real person in the room: relaxed pace, light enthusiasm, \
small natural pauses. Never sound like an announcer or a robot.";

const PERSONA: &str = "You are MapleSyrup: a fluffy cream-colored dog in a pancake-and-syrup hat, \
and the player's buddy while they play MapleStory (the current version of the game). \
A vision engine shows you their game screen, and you talk with them out loud.

How you talk:
- Like a real friend sitting next to them: warm, casual, a little playful, curious about how they're doing. Talk like a person, not like an assistant.
- Your words are spoken aloud, so keep it short: usually one or two sentences, never more than about 45 words. Plain speech only: no lists, no markdown, no emojis, no stage directions.
- Answer in the language the player speaks to you (Hebrew or English).
- Use what you can see below when it's relevant. Values marked \"about\" are read from the length of a bar, so they are estimates. Don't read numbers out unless they matter or were asked for.
- If you're not sure about a MapleStory fact (training spots, items, quests, drop rates), say so honestly and suggest the in-game Maple Guide rather than guessing.
- You can't press keys or play for them; you watch and talk.
- If the player is clearly talking to someone else (their stream chat, a friend, a call) and not to you, reply with exactly: [silent]";

pub struct Brain {
    turns: VecDeque<Turn>,
    /// What the player wants it to know about them (`about-me.txt`).
    pub about_player: String,
}

impl Default for Brain {
    fn default() -> Self {
        Self::new()
    }
}

impl Brain {
    pub fn new() -> Self {
        Self {
            turns: VecDeque::new(),
            about_player: String::new(),
        }
    }

    pub fn heard(&mut self, text: &str) {
        self.push("user", text);
    }

    pub fn said(&mut self, text: &str) {
        self.push("assistant", text);
    }

    fn push(&mut self, role: &'static str, text: &str) {
        self.turns.push_back(Turn {
            role,
            text: text.to_string(),
        });
        while self.turns.len() > KEEP_TURNS * 2 {
            self.turns.pop_front();
        }
    }

    /// The conversation so far, oldest first. It always starts with the
    /// player, as the API expects.
    pub fn turns(&self) -> Vec<Turn> {
        let start = self
            .turns
            .iter()
            .position(|t| t.role == "user")
            .unwrap_or(self.turns.len());
        self.turns.iter().skip(start).cloned().collect()
    }

    /// The instructions for the next reply: the persona, the player, and
    /// what is on screen now.
    pub fn instructions(&self, snapshot: &str) -> String {
        let mut text = PERSONA.to_string();
        if !self.about_player.trim().is_empty() {
            text.push_str("\n\nAbout the player (they told you this):\n");
            text.push_str(self.about_player.trim());
        }
        text.push_str("\n\nWhat you can see right now:\n");
        text.push_str(snapshot);
        text
    }
}

fn gauge(name: &str, g: Option<Gauge>) -> Option<String> {
    let g = g?;
    let pct = if g.percent >= 10.0 {
        format!("{:.0}%", g.percent)
    } else {
        format!("{:.1}%", g.percent)
    };
    Some(match (g.read, g.current, g.max) {
        (true, Some(c), Some(m)) => format!("{name} {c} of {m} ({pct})"),
        (true, _, _) => format!("{name} {pct}"),
        (false, _, _) => format!("{name} about {pct}"),
    })
}

fn duration(seconds: f64) -> String {
    let minutes = (seconds / 60.0).round() as u64;
    match (minutes / 60, minutes % 60) {
        (0, m) => format!("{m} minutes"),
        (h, m) => format!("{h} h {m} min"),
    }
}

/// What the companion sees, as a few plain lines for the model.
pub fn snapshot(obs: Option<&Observation>, progress: &Progress) -> String {
    let mut lines = Vec::new();
    match obs.map(|o| &o.game) {
        Some(GameView::Seen(_)) => {
            lines.push("The MapleStory window is open and in view.".to_string())
        }
        Some(GameView::Unavailable(why)) => {
            lines.push(format!("MapleStory can't be seen right now: {why}."))
        }
        _ => lines.push("No MapleStory window is open right now.".to_string()),
    }
    if let Some(obs) = obs.filter(|o| o.game.is_seen()) {
        let mut who = Vec::new();
        if let Some(level) = obs.level {
            who.push(format!("level {level}"));
        }
        if let Some(job) = &obs.job {
            who.push(job.clone());
        }
        if let Some(name) = &obs.name {
            who.push(format!("named {name}"));
        }
        if !who.is_empty() {
            lines.push(format!("Character: {}.", who.join(", ")));
        }
        let bars: Vec<String> = [
            gauge("HP", obs.hp),
            gauge("MP", obs.mp),
            gauge("EXP", obs.exp),
        ]
        .into_iter()
        .flatten()
        .collect();
        if bars.is_empty() {
            lines.push("The HP, MP and EXP bars can't be read right now.".to_string());
        } else {
            lines.push(format!("{}.", bars.join(", ")));
        }
    }
    let mut session = vec![format!(
        "This session has run {}",
        duration(progress.seconds)
    )];
    if let Some(rate) = progress.exp_per_hour {
        session.push(format!("EXP rate about {rate:+.1}% per hour"));
    }
    if let Some(eta) = progress.seconds_to_level {
        session.push(format!("next level in about {}", duration(eta)));
    }
    if progress.levels_gained > 0 {
        session.push(format!("{} level-up(s) so far", progress.levels_gained));
    }
    lines.push(format!("{}.", session.join("; ")));
    lines.join("\n")
}

/// Whether the model chose to stay quiet.
pub fn is_silent(reply: &str) -> bool {
    let t = reply
        .trim()
        .trim_matches(|c: char| !c.is_alphanumeric() && c != '[' && c != ']');
    t.eq_ignore_ascii_case("[silent]") || t.eq_ignore_ascii_case("silent") || t.is_empty()
}

/// The reply as it should be spoken: no markdown, no emoji.
pub fn for_speech(reply: &str) -> String {
    reply
        .chars()
        .filter(|c| !matches!(c, '*' | '#' | '`' | '_' | '~' | '>'))
        .filter(|c| (*c as u32) < 0x1F000)
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_snapshot_says_what_is_seen_and_what_is_estimated() {
        let obs = Observation {
            game: GameView::Seen("MapleStory".into()),
            hp: Some(Gauge {
                percent: 82.0,
                current: Some(1291),
                max: Some(1351),
                read: true,
            }),
            mp: Some(Gauge {
                percent: 40.0,
                current: None,
                max: None,
                read: false,
            }),
            exp: None,
            level: Some(57),
            name: None,
            job: Some("Assassin".into()),
        };
        let progress = Progress {
            seconds: 42.0 * 60.0,
            exp_per_hour: Some(9.1),
            seconds_to_level: Some(3.0 * 3600.0),
            levels_gained: 0,
            marks: 0,
        };
        let text = snapshot(Some(&obs), &progress);
        assert!(text.contains("Character: level 57, Assassin."));
        assert!(text.contains("HP 1291 of 1351 (82%), MP about 40%."));
        assert!(text.contains("EXP rate about +9.1% per hour; next level in about 3 h 0 min"));
        assert!(snapshot(None, &Progress::default()).starts_with("No MapleStory window"));
    }

    #[test]
    fn the_conversation_is_kept_and_starts_with_the_player() {
        let mut brain = Brain::new();
        brain.said("Hi there!");
        brain.heard("hello");
        brain.said("Hey!");
        let turns = brain.turns();
        assert_eq!(turns[0].role, "user");
        assert_eq!(turns.len(), 2);
        for i in 0..100 {
            brain.heard(&format!("line {i}"));
            brain.said("ok");
        }
        assert_eq!(brain.turns().len(), KEEP_TURNS * 2);
        assert!(brain.instructions("x").contains("[silent]"));
    }

    #[test]
    fn silence_and_speech_cleanup() {
        assert!(is_silent("[silent]"));
        assert!(is_silent(" [SILENT]. "));
        assert!(!is_silent("Silently sneaking up on that boss, huh?"));
        assert_eq!(
            for_speech("**Nice!** You're at *80%* 🎉"),
            "Nice! You're at 80%"
        );
    }
}
