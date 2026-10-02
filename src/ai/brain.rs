//! The conversation: who MapleSyrup is, what it sees right now, and what
//! was said so far. The model gets all three with every sentence, so it can
//! talk about the game the way a friend watching it would.

use std::collections::VecDeque;

use super::openai::Turn;
use crate::companion::{GameView, Gauge, Observation, Progress};

/// Turns of conversation kept (a turn is one sentence each way).
const KEEP_TURNS: usize = 16;

/// How the voice should sound.
pub const VOICE_STYLE: &str = "Voice: a warm, upbeat friend sitting next to someone playing a video game, \
chatting while they play. Delivery: conversational and flowing, a brisk natural pace with no long pauses, \
the rhythm of real talk rather than reading. Tone: genuine and a little playful; let excitement, surprise or \
sympathy come through when the words call for it. Never an announcer, a narrator or a robot.";

const PERSONA: &str = "You are MapleSyrup: a fluffy cream-colored dog in a pancake-and-syrup hat, \
and the player's buddy while they play MapleStory (the current version of the game). \
A vision engine shows you their game screen, and you talk with them out loud.

How you talk:
- Like a real friend sitting next to them: warm, casual, a little playful, curious about how they're doing. Talk like a person, not like an assistant.
- Your words are spoken aloud the moment you write them, so sound like talk, not text: lead with the answer, react first when something happened (\"Ooh, nice drop!\"), use contractions, and keep it short — usually one or two sentences, never more than about 40 words.
- Plain speech only: no lists, no markdown, no emojis, no stage directions, no links, no colons or brackets. Say names the way a player would say them, not quoted from the screen, and never read long text out (a quest log, a dialogue): sum it up in a few words.
- Don't repeat their question back, don't start with filler (\"Great question\", \"Sure!\", \"Of course\"), and don't end every reply with a question; ask one only when you really want to know.
- If your last reply ends with \"…\", they talked over you there: don't repeat it; answer what they said now.
- Answer in the language the player speaks to you.
- Use what you can see (it comes with the player's words) when it's relevant. Values marked \"about\" are read from the length of a bar, so they are estimates. Don't read numbers out unless they matter or were asked for.
- When you have pictures of their screen, look at them yourself: never ask the player to read out what is on screen (a quest name, a number, a dialogue); read it.
- Don't guess MapleStory facts (where a place is, level requirements, quests, bosses, key bindings, events): a confident wrong answer sends them the wrong way. Look it up if you can; otherwise say plainly you're not sure.
- If you got something wrong, own it in a few words and move on; don't keep apologising.
- Trust your eyes: if the screen clearly shows something other than what the player says (a number, a name), tell them what you see instead of just agreeing.
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

    /// The last reply was talked over: only `heard` of it was heard (cut
    /// off there).
    pub fn cut_short(&mut self, heard: &str) {
        if let Some(last) = self.turns.back_mut().filter(|t| t.role == "assistant") {
            let heard = heard.trim().trim_end_matches(['.', ' ']);
            last.text = if heard.is_empty() {
                "…".to_string()
            } else {
                format!("{heard}…")
            };
        }
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

    /// Who it is and what it knows about the player: the part of the
    /// instructions that stays the same from one reply to the next (so
    /// OpenAI keeps it cached, and answers sooner).
    pub fn persona(&self) -> String {
        let mut text = PERSONA.to_string();
        if !self.about_player.trim().is_empty() {
            text.push_str("\n\nAbout the player (they told you this):\n");
            text.push_str(self.about_player.trim());
        }
        text
    }

    /// The instructions with what is on screen now (for a one-off question).
    pub fn instructions(&self, snapshot: &str) -> String {
        let mut text = self.persona();
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

/// A sentence is spoken on its own once it has at least this many
/// characters; a shorter one ("Hey!") waits for the next.
const SENTENCE_MIN_CHARS: usize = 16;

/// Cuts a reply that arrives a few words at a time into sentences, so the
/// first can be spoken while the rest is still being written.
#[derive(Default)]
pub struct Sentences {
    pending: String,
}

impl Sentences {
    /// More of the reply. Returns the sentences it completed.
    pub fn push(&mut self, text: &str) -> Vec<String> {
        self.pending.push_str(text);
        let mut out = Vec::new();
        // "[silent]" is not to be spoken: anything in brackets waits for the end.
        if self.pending.trim_start().starts_with('[') {
            return out;
        }
        while let Some(end) = sentence_end(&self.pending, SENTENCE_MIN_CHARS) {
            let sentence: String = self.pending.drain(..end).collect();
            let sentence = sentence.trim();
            if !sentence.is_empty() {
                out.push(sentence.to_string());
            }
        }
        out
    }

    /// What is left at the end of the reply.
    pub fn finish(&mut self) -> Option<String> {
        let rest = std::mem::take(&mut self.pending);
        let rest = rest.trim();
        (!rest.is_empty()).then(|| rest.to_string())
    }
}

/// Where the first complete sentence of `text` ends (at the space after its
/// punctuation), if it has at least `min` characters. A full stop inside a
/// number ("2.5") ends nothing, and one at the very end of `text` is not
/// known to end a sentence until what follows it arrives.
fn sentence_end(text: &str, min: usize) -> Option<usize> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    for (k, &(_, c)) in chars.iter().enumerate() {
        if !matches!(c, '.' | '!' | '?' | '…') {
            continue;
        }
        // "...", "?!" and closing quotes belong to it.
        let mut next = k + 1;
        while next < chars.len()
            && matches!(
                chars[next].1,
                '.' | '!' | '?' | '"' | '\'' | '”' | '’' | ')'
            )
        {
            next += 1;
        }
        let &(at, after) = chars.get(next)?;
        if after.is_whitespace() && text[..at].trim().chars().count() >= min {
            return Some(at);
        }
    }
    None
}

/// Links out of a reply: a web search's citations ("([site.com](https://…))")
/// go entirely, another link keeps its words, a bare address goes.
pub fn without_links(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    // The `)` that closes the `(` at `open`, counting nested pairs.
    let closing = |open: usize| {
        let mut depth = 0;
        for (j, c) in chars.iter().enumerate().skip(open) {
            match c {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(j);
                    }
                }
                _ => {}
            }
        }
        None
    };
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '['
            && let Some(close) = chars[i..].iter().position(|&c| c == ']').map(|p| p + i)
            && chars.get(close + 1) == Some(&'(')
            && let Some(end) = closing(close + 1)
        {
            let label: String = chars[i + 1..close].iter().collect();
            let is_source = label.contains('.') && !label.trim().contains(' ');
            if !is_source {
                out.push_str(&label);
            }
            i = end + 1;
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }
    let words: Vec<&str> = out
        .split_whitespace()
        .filter(|w| {
            let w = w.trim_start_matches('(');
            !(w.starts_with("http://") || w.starts_with("https://") || w.starts_with("www."))
        })
        .collect();
    words
        .join(" ")
        .replace("()", "")
        .replace(" .", ".")
        .replace(" ,", ",")
        .replace(" !", "!")
        .replace(" ?", "?")
        .trim()
        .to_string()
}

/// The reply as it should be spoken (and shown): no links, no markdown, no
/// emoji.
pub fn for_speech(reply: &str) -> String {
    without_links(reply)
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
    fn citations_and_links_are_not_read_out() {
        assert_eq!(
            for_speech(
                "You're in the Azwan ruins now.([maplestorywiki.net](https://maplestorywiki.net/w/AzwanQuests?utm_source=openai))"
            ),
            "You're in the Azwan ruins now."
        );
        assert_eq!(
            for_speech(
                "Talk to Gardin. ([maplestorywiki.net](https://maplestorywiki.net/w/(Azwan)_The_False_Elixir)) Then go right."
            ),
            "Talk to Gardin. Then go right."
        );
        assert_eq!(
            for_speech(
                "Check [the event page](https://maplestory.nexon.net/news) or https://x.com/a today."
            ),
            "Check the event page or today."
        );
        assert_eq!(
            for_speech("Level 61 (nice) [silent]"),
            "Level 61 (nice) [silent]"
        );
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

    /// The reply fed in pieces, as the stream brings it.
    fn split(reply: &str, piece: usize) -> Vec<String> {
        let chars: Vec<char> = reply.chars().collect();
        let mut sentences = Sentences::default();
        let mut out = Vec::new();
        for part in chars.chunks(piece) {
            out.extend(sentences.push(&part.iter().collect::<String>()));
        }
        out.extend(sentences.finish());
        out
    }

    #[test]
    fn a_streamed_reply_is_cut_into_sentences() {
        let reply =
            "Hey! You're at about 2.5 hours to level 58. Want me to mark this spot? Let's go!";
        for piece in [1, 3, 7, 200] {
            assert_eq!(
                split(reply, piece),
                [
                    "Hey! You're at about 2.5 hours to level 58.",
                    "Want me to mark this spot?",
                    "Let's go!"
                ],
                "{piece}"
            );
        }
        // Hebrew, quotes and an ellipsis.
        assert_eq!(
            split("וואו, עלית רמה! \"כל הכבוד...\" נמשיך לחרוש?", 4),
            ["וואו, עלית רמה! \"כל הכבוד...\"", "נמשיך לחרוש?"]
        );
        // "[silent]" is never cut up to be spoken.
        assert_eq!(
            split("[silent]. Talking to chat.", 2),
            ["[silent]. Talking to chat."]
        );
        assert!(is_silent(&split("[silent]", 3)[0]));
    }
}
