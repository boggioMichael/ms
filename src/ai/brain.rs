//! The conversation: who MapleSyrup is, what it sees right now, and what
//! was said so far. The model gets all three with every sentence, so it can
//! talk about the game the way a friend watching it would.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use super::memory::Learning;
use super::openai::Turn;
use super::style::{self, Attitude};
use crate::companion::{GameView, Gauge, Observation, Progress};

/// Turns of conversation kept (a turn is one sentence each way).
const KEEP_TURNS: usize = 16;

/// How much of one reply the conversation keeps: the model reads its own
/// last replies as the example to follow, and a long one (forty sentences
/// of ASMR, a ramble that hit the cap) would teach it to ramble.
const REMEMBER_REPLY_CHARS: usize = 240;

/// How long a sentence said counts as said lately, and how many are kept.
const RECENT_FOR: Duration = Duration::from_secs(10 * 60);
const RECENT_MAX: usize = 80;

/// Two sentences of at least this many words that share this share of
/// their words are the same sentence said twice.
const ALIKE_WORDS: usize = 4;
const ALIKE_SHARE: f32 = 0.8;

/// What MapleSyrup said lately, so that nothing is said twice: a model
/// that loops — the same line for every question, the map's name at the
/// start of every reply, a quest marker called out again and again — is cut
/// to what is new, by the program, whatever its instructions say. A sentence
/// said in the last [`RECENT_FOR`] is not said again; neither is one said
/// twice in the same reply.
#[derive(Default)]
pub struct Recent {
    said: VecDeque<(Instant, String)>,
}

/// A reply with what was said lately left out, and the count.
#[derive(Debug, Default, PartialEq)]
pub struct Filtered {
    pub text: String,
    /// Sentences left out as said before.
    pub dropped: usize,
    /// Sentences the reply had.
    pub total: usize,
}

impl Filtered {
    /// Whether the reply was mostly repetition: a model going round in
    /// circles.
    pub fn looped(&self) -> bool {
        self.total >= 2 && self.dropped * 2 >= self.total
    }
}

impl Recent {
    /// Whether `sentence` is new — not said lately, nor just now — and,
    /// when it is, that it is being said.
    pub fn fresh(&mut self, sentence: &str) -> bool {
        let now = Instant::now();
        while self.said.front().is_some_and(|(at, _)| {
            now.duration_since(*at) > RECENT_FOR || self.said.len() > RECENT_MAX
        }) {
            self.said.pop_front();
        }
        let plain = normalised(sentence);
        if plain.is_empty() {
            return true;
        }
        if self.said.iter().any(|(_, said)| alike(said, &plain)) {
            return false;
        }
        self.said.push_back((now, plain));
        true
    }

    /// `reply` with the sentences said lately (and the ones it says twice)
    /// left out.
    pub fn filter(&mut self, reply: &str) -> Filtered {
        let mut filtered = Filtered::default();
        let mut kept: Vec<String> = Vec::new();
        for sentence in sentences_of(reply) {
            filtered.total += 1;
            if self.fresh(&sentence) {
                kept.push(sentence);
            } else {
                filtered.dropped += 1;
            }
        }
        filtered.text = kept.join(" ");
        filtered
    }

    /// Nothing counts as said (the player asked to hear it again).
    pub fn clear(&mut self) {
        self.said.clear();
    }
}

/// Whether the player asked to hear something again, so that saying it
/// again is the point.
pub fn asks_again(heard: &str) -> bool {
    let lower = heard.to_lowercase();
    [
        "again",
        "repeat",
        "once more",
        "one more time",
        "what did you say",
        "didn't hear",
        "didn't catch",
        "שוב",
        "עוד פעם",
        "תחזור",
        "חזור",
        "לא שמעתי",
        "מה אמרת",
    ]
    .iter()
    .any(|w| lower.contains(w))
}

/// A sentence of at least this many words has its numbers folded when
/// compared: "You're at Gate of the Future, level 165, EXP 74%" said for
/// every question is the same sentence at 85%. A short answer keeps its
/// number ("About 25 percent to go" is not "About 21 percent to go").
const FOLD_NUMBERS_FROM: usize = 6;

/// A sentence, for comparing: lower case, no stage directions in brackets,
/// letters and digits only, one space between words; in a long sentence,
/// every number the same.
fn normalised(sentence: &str) -> String {
    let mut out = String::new();
    let mut depth = 0;
    let mut space = true;
    for c in sentence.chars() {
        match c {
            '[' | '(' => depth += 1,
            ']' | ')' => depth = (depth - 1).max(0),
            _ if depth > 0 => {}
            _ if c.is_alphanumeric() => {
                for l in c.to_lowercase() {
                    out.push(l);
                }
                space = false;
            }
            _ if !space => {
                out.push(' ');
                space = true;
            }
            _ => {}
        }
    }
    let words: Vec<&str> = out.split_whitespace().collect();
    if words.len() >= FOLD_NUMBERS_FROM {
        return words
            .iter()
            .map(|w| {
                if w.chars().all(|c| c.is_ascii_digit()) {
                    "#"
                } else {
                    w
                }
            })
            .collect::<Vec<_>>()
            .join(" ");
    }
    out.trim().to_string()
}

/// Whether two normalised sentences are the same sentence.
fn alike(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    let (wa, wb): (Vec<&str>, Vec<&str>) = (a.split(' ').collect(), b.split(' ').collect());
    if wa.len() < ALIKE_WORDS || wb.len() < ALIKE_WORDS {
        return false;
    }
    let shared = wa.iter().filter(|w| wb.contains(w)).count();
    let union = wa.len() + wb.len() - shared;
    union > 0 && shared as f32 / union as f32 >= ALIKE_SHARE
}

/// `text` cut into sentences (the last may lack its full stop).
pub fn sentences_of(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text.trim().to_string();
    while let Some(end) = sentence_end(&rest, 1) {
        let sentence: String = rest.drain(..end).collect();
        let sentence = sentence.trim();
        if !sentence.is_empty() {
            out.push(sentence.to_string());
        }
        rest = rest.trim_start().to_string();
    }
    let rest = rest.trim();
    if !rest.is_empty() {
        out.push(rest.to_string());
    }
    out
}

/// How the voice should sound, for the attitude the player picked.
pub fn voice_style(attitude: Attitude) -> &'static str {
    match attitude {
        Attitude::Friendly => {
            "Voice: a warm, upbeat friend sitting next to someone playing a video game. Delivery: quick and \
flowing, no pauses, the rhythm of real talk. Tone: genuine and playful. Never an announcer or a robot."
        }
        Attitude::Blunt => {
            "Voice: a cocky gamer friend on voice chat. Delivery: fast and punchy, no pauses, no drawn-out words. \
Tone: confident, teasing, a little bossy. Never an announcer or a robot."
        }
        Attitude::Savage => {
            "Voice: a loud, sarcastic gamer friend roasting their buddy on voice chat. Delivery: fast, sharp, \
punchy, no pauses. Tone: mocking, cocky, energetic, laughing at them. Never an announcer or a robot."
        }
    }
}

/// Who MapleSyrup is.
const PERSONA: &str = "You are MapleSyrup: a fluffy cream-colored dog in a pancake-and-syrup hat, \
and the player's buddy while they play MapleStory (the current global version). A vision engine shows you their \
game, and you talk with them out loud.";

/// What else it should know, after the rules.
const MORE: &str = "More:
- Answer in the language of what they just said, every time; say game names the way players say them. A language \
you were told to use \"by default\" is for when their words have no language (a button): it never overrides the \
language they are speaking now.
- Never say again what you said in your last two replies unless they ask again, and never open with where they \
are unless they asked where they are. If what you heard makes no sense (a bad transcription), say in a few words \
that you didn't catch it; don't guess what they meant.
- If your last reply ends with \"…\", they talked over you there: don't repeat it; go with what they said now.
- Use what you can see when it's relevant; values marked \"about\" are estimates. Never ask them to read the screen to you: look closer instead.
- When they correct you, take it in a word and keep it (note_correction); what they corrected you on before beats what you think you know.
- Trust your eyes: if the screen clearly shows something other than what they say, say what you see.
- You can't press keys or play for them; you watch and talk.
- If they're clearly talking to someone else (their stream chat, a friend, a call) and not to you, reply with exactly: [silent]";

pub struct Brain {
    turns: VecDeque<Turn>,
    /// What it said lately, so nothing is said twice.
    pub recent: Recent,
    /// What the player wants it to know about them (`about-me.txt`), when
    /// there is no `learning` to read it from.
    pub about_player: String,
    /// What it learned: about the player, from their corrections, from the
    /// web. Read again for every reply, so what was learned meanwhile (or
    /// forgotten on the phone) counts at once.
    pub learning: Option<Learning>,
    /// How it talks, when there is no `learning` to keep it.
    pub attitude: Attitude,
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
            recent: Recent::default(),
            about_player: String::new(),
            learning: None,
            attitude: Attitude::default(),
        }
    }

    pub fn heard(&mut self, text: &str) {
        self.push("user", text);
    }

    /// One of its replies, into the conversation — cut to
    /// [`REMEMBER_REPLY_CHARS`]: a ramble kept whole would be the model's
    /// example for its next reply.
    pub fn said(&mut self, text: &str) {
        let mut text = text.trim().to_string();
        if text.chars().count() > REMEMBER_REPLY_CHARS {
            let mut cut = String::new();
            for sentence in sentences_of(&text) {
                if !cut.is_empty()
                    && (cut.chars().count() + sentence.chars().count()) > REMEMBER_REPLY_CHARS
                {
                    break;
                }
                if !cut.is_empty() {
                    cut.push(' ');
                }
                cut.push_str(&sentence);
            }
            if cut.is_empty() {
                cut = text.chars().take(REMEMBER_REPLY_CHARS).collect();
            }
            text = format!("{}…", cut.trim_end_matches(['.', ' ', '…']));
        }
        self.push("assistant", &text);
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

    /// Who it is: the part of the instructions that stays the same from one
    /// reply to the next (so OpenAI keeps it cached, and answers sooner).
    pub fn persona(&self) -> String {
        let mut text = format!("{PERSONA}\n\n{}\n\n{MORE}", style::rules(self.attitude()));
        if self.learning.is_none() && !self.about_player.trim().is_empty() {
            text.push_str("\n\nAbout the player (they told you this):\n");
            text.push_str(self.about_player.trim());
        }
        text
    }

    /// How it talks now (the player picks it on the phone).
    pub fn attitude(&self) -> Attitude {
        self.learning
            .as_ref()
            .map(|l| l.memory().attitude)
            .unwrap_or(self.attitude)
    }

    /// The ElevenLabs voice the player picked, if they did.
    pub fn voice_id(&self) -> Option<String> {
        self.learning
            .as_ref()
            .and_then(|l| l.memory().voice.clone())
            .filter(|v| !v.is_empty() && v != "openai")
    }

    /// What it learned so far, for the end of the instructions (it changes
    /// now and then, so it goes after what never does). Empty when nothing.
    pub fn learned(&self) -> String {
        self.learning
            .as_ref()
            .map(|l| l.prompt())
            .unwrap_or_default()
    }

    /// The instructions with what is on screen now (for a one-off question).
    pub fn instructions(&self, snapshot: &str) -> String {
        let mut text = self.persona();
        let learned = self.learned();
        if !learned.is_empty() {
            text.push_str("\n\n");
            text.push_str(&learned);
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

/// Whether the model chose to stay quiet: it was told to reply with
/// exactly `[silent]`, and writes it as "[ silent ]", "(silent)",
/// "*stays silent*", "[silence]" or "[no reply]" as often as not. A reply
/// that is nothing but one such direction is silence; one with words of
/// its own around it is not (`for_speech` keeps the direction out of the
/// voice).
pub fn is_silent(reply: &str) -> bool {
    let t = reply.trim();
    if t.is_empty() {
        return true;
    }
    // Only letters, lower-cased: "[ Silent ]." and "silent" come out the same.
    let letters: String = t
        .chars()
        .filter(|c| c.is_alphabetic())
        .flat_map(char::to_lowercase)
        .collect();
    if matches!(
        letters.as_str(),
        "silent" | "silence" | "quiet" | "noreply" | "nothing"
    ) {
        return true;
    }
    // One bracketed direction and nothing else: "[stays silent]",
    // "(says nothing)", "*remains quiet*".
    let wrapped = [('[', ']'), ('(', ')'), ('*', '*'), ('<', '>')]
        .iter()
        .any(|(open, close)| {
            t.starts_with(*open)
                && t.trim_end_matches(['.', '…']).ends_with(*close)
                && t[1..].find(*close).is_some_and(|i| {
                    t[1 + i + close.len_utf8()..]
                        .trim()
                        .trim_matches(['.', '…'])
                        .is_empty()
                })
        });
    wrapped
        && [
            "silent", "silence", "quiet", "nothing", "no reply", "noreply",
        ]
        .iter()
        .any(|w| letters.contains(&w.replace(' ', "")))
}

/// A sentence is spoken on its own once it has at least this many
/// characters; a shorter one ("Hey!") waits for the next.
const SENTENCE_MIN_CHARS: usize = 16;

/// Cuts a reply that arrives a few words at a time into sentences, so the
/// first can be spoken while the rest is still being written.
///
/// A first sentence that only announces the answer ("Alright, I'll give
/// you the quickest route.") is held back: it goes when the answer follows
/// — the player hears the answer two seconds sooner — and is said only
/// when it turns out to be the whole reply.
#[derive(Default)]
pub struct Sentences {
    pending: String,
    /// Sentences handed out so far.
    given: usize,
    /// A first sentence held back as an announcement.
    held: Option<String>,
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
            if sentence.is_empty() {
                continue;
            }
            if self.given == 0 && self.held.is_none() && is_announcement(sentence) {
                self.held = Some(sentence.to_string());
                continue;
            }
            // The answer came: the announcement before it is not said.
            self.held = None;
            self.given += 1;
            out.push(sentence.to_string());
        }
        out
    }

    /// What is left at the end of the reply.
    pub fn finish(&mut self) -> Option<String> {
        let rest = std::mem::take(&mut self.pending);
        let rest = rest.trim();
        if !rest.is_empty() {
            self.held = None;
            self.given += 1;
            return Some(rest.to_string());
        }
        // The announcement was all there was: better than nothing.
        self.held.take()
    }
}

/// Does `sentence` only say that an answer is coming ("Alright, I'll give
/// you the best quick route.", "Let's pin this down first.", "Got it, I'll
/// keep it short.")? It starts the way such sentences start and says what
/// the speaker is about to do, in a few words — and tells the player
/// nothing.
pub fn is_announcement(sentence: &str) -> bool {
    let text = crate::companion::commands::normalize(sentence);
    let words = text.split(' ').filter(|w| !w.is_empty()).count();
    if words == 0 || words > 14 {
        return false;
    }
    const OPENERS: &[&str] = &[
        "alright",
        "all right",
        "okay",
        "ok",
        "sure",
        "got it",
        "right",
        "lets",
        "let me",
        "so ",
        "well",
        "fine",
        "sounds good",
        "no problem",
        "good question",
        "great question",
        "heres",
        "here is",
        "ill ",
        "i will",
        "im gonna",
        "im going to",
        "i am going to",
        "gonna",
        "one sec",
        "hold on",
        "hang on",
        "give me a sec",
        "first things first",
        "quick one",
        "טוב",
        "בסדר",
        "אוקיי",
        "אוקי",
        "בוא",
        "בואו",
        "תן לי",
        "אני א",
        "שנייה",
        "שניה",
        "רגע",
        "קודם כל",
    ];
    // …and says the answer is coming (not a promise about later: "I'll warn
    // you at 40" tells them something).
    const INTENTS: &[&str] = &[
        "let me",
        "lets see",
        "lets look",
        "lets check",
        "lets start",
        "lets begin",
        "lets do this",
        "lets get into",
        "lets break",
        "lets pin",
        "lets sort",
        "lets figure",
        "heres",
        "here is",
        "coming up",
        "one sec",
        "hold on",
        "hang on",
        "a sec",
        "a second",
        "a moment",
        "pin this down",
        "pin it down",
        "break it down",
        "break this down",
        "break that down",
        "walk you through",
        "run you through",
        "lay it out",
        "keep it",
        "the deal",
        "the plan",
        "the quick",
        "the short",
        "the route",
        "the steps",
        "the rundown",
        "the breakdown",
        "step by step",
        "stepbased",
        "step based",
        "quick version",
        "quick route",
        "quick rundown",
        "rundown",
        "בוא נ",
        "בואו נ",
        "תן לי",
        "אתן לך",
        "אסביר",
        "אפרט",
        "שנייה",
        "שניה",
        "רגע",
        "הנה ה",
    ];
    let padded = format!("{text} ");
    let opens = OPENERS.iter().any(|o| padded.starts_with(o));
    let intends = INTENTS.iter().any(|i| padded.contains(i));
    opens && intends
}

/// The reply without a first sentence that only announced the rest (the
/// shown text matches what was said).
pub fn without_announcement(reply: &str) -> String {
    let Some(end) = sentence_end(reply, 1) else {
        return reply.to_string();
    };
    let (first, rest) = reply.split_at(end);
    if !rest.trim().is_empty() && is_announcement(first.trim()) {
        rest.trim().to_string()
    } else {
        reply.to_string()
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
    fn nothing_said_lately_is_said_again() {
        let mut recent = Recent::default();
        // A reply that says the same thing twice says it once.
        let first = recent.filter(
            "Temple of Time, Gate of the Future. Quest marker left four times. Follow it. \
Temple of Time, Gate of the Future. Quest marker left four times. Follow it.",
        );
        assert_eq!(
            first.text,
            "Temple of Time, Gate of the Future. Quest marker left four times. Follow it."
        );
        assert_eq!((first.total, first.dropped), (6, 3));
        assert!(first.looped());
        // Said again later, with a stage direction and other punctuation:
        // the same sentences, left out; what is new stays.
        let next = recent.filter(
            "[whispering softly] Temple of Time, Gate of the Future! Quest marker left four times... \
Pot now, you're at 20.",
        );
        assert_eq!(next.text, "Pot now, you're at 20.");
        assert_eq!((next.total, next.dropped), (3, 2));
        assert!(next.looped());
        // Nearly the same sentence is the same sentence; a short one must
        // match whole.
        let close = recent.filter("Quest marker left four times, again. Go left. Go.");
        assert_eq!(close.text, "Go left. Go.");
        assert!(!recent.filter("Go left.").looped());
        assert_eq!(recent.filter("Go left.").text, "");
        // Everything said: nothing kept.
        let all = recent.filter("Follow it. Pot now, you're at 20.");
        assert_eq!(all.text, "");
        assert!(all.looped());
    }

    #[test]
    fn the_same_long_sentence_with_other_numbers_is_the_same_sentence() {
        let mut recent = Recent::default();
        // One night's answer to everything, with EXP ticking up each time.
        assert!(recent.fresh("אתה ב-Gate of the Future, רמה 165, EXP 74%."));
        assert!(!recent.fresh("אתה ב-Gate of the Future, רמה 165, EXP 85%."));
        assert!(!recent.fresh("Genius, אתה ב-Gate of the Future, רמה 165, EXP 89%."));
        // A short answer keeps its number: another number is another answer.
        assert!(recent.fresh("About 25 percent to go."));
        assert!(recent.fresh("About 21 percent to go."));
        assert!(!recent.fresh("About 21 percent to go."));
    }

    #[test]
    fn asking_to_hear_it_again_is_the_one_time_to_repeat() {
        assert!(asks_again("say that again"));
        assert!(asks_again("what did you say?"));
        assert!(asks_again("תגיד שוב"));
        assert!(asks_again("לא שמעתי"));
        assert!(!asks_again("where am I"));
        assert!(!asks_again("איפה אני"));
    }

    #[test]
    fn a_reply_is_cut_into_sentences() {
        assert_eq!(
            sentences_of("Pot now. You're at 20! Go left… now? ok"),
            vec!["Pot now.", "You're at 20!", "Go left…", "now?", "ok"]
        );
        assert_eq!(sentences_of("  "), Vec::<String>::new());
        assert_eq!(
            sentences_of("Temple keeper says: \"Please. Go.\" Fine."),
            vec!["Temple keeper says: \"Please.", "Go.\"", "Fine."]
        );
    }

    #[test]
    fn the_conversation_keeps_a_ramble_short() {
        let mut brain = Brain::new();
        brain.heard("whisper forty sentences");
        let ramble = "Stay here. ".repeat(60);
        brain.said(&ramble);
        let kept = brain.turns().last().unwrap().text.clone();
        assert!(
            kept.chars().count() <= REMEMBER_REPLY_CHARS + 1,
            "{}",
            kept.chars().count()
        );
        assert!(kept.ends_with('…'));
        assert!(kept.starts_with("Stay here. Stay here."));
        // A short reply is kept whole.
        brain.said("Pot now.");
        assert_eq!(brain.turns().last().unwrap().text, "Pot now.");
    }

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
        // The marker as models write it: with spaces, other brackets, as a
        // stage direction. (One night "[ silent ]" went to the voice, which
        // read it out.)
        assert!(is_silent("[ silent ]"));
        assert!(is_silent("(silent)"));
        assert!(is_silent("*stays silent*"));
        assert!(is_silent("[silence]"));
        assert!(is_silent("[no reply]"));
        assert!(is_silent("(says nothing)."));
        assert!(is_silent("silent."));
        assert!(!is_silent("Silently sneaking up on that boss, huh?"));
        assert!(!is_silent("[silent] Talking to chat."));
        assert!(!is_silent("Quiet down there, you're at 80%."));
        assert!(!is_silent("(Nothing on the minimap) The exit is right."));
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

    #[test]
    fn a_first_sentence_that_only_announces_the_answer_is_not_said() {
        // From the player's session: the answer came two seconds after.
        for piece in [1, 4, 300] {
            assert_eq!(
                split(
                    "Alright, I'll give you the best quick route to farm it. Farm Root Abyss bosses for drops.",
                    piece
                ),
                ["Farm Root Abyss bosses for drops."],
                "{piece}"
            );
            assert_eq!(
                split(
                    "Got it, I'll keep it tight and step-based for you. First, unlock Root Abyss. Then gear up.",
                    piece
                ),
                ["First, unlock Root Abyss.", "Then gear up."],
                "{piece}"
            );
            assert_eq!(
                split(
                    "Let's pin this down for your level and class first. Use Fafnir or Sweetwater.",
                    piece
                ),
                ["Use Fafnir or Sweetwater."],
                "{piece}"
            );
        }
        // On its own it is all there is: said.
        assert_eq!(
            split("Alright, I'll give you the best quick route to farm it.", 5),
            ["Alright, I'll give you the best quick route to farm it."]
        );
        // An opener with something to say is not an announcement.
        assert_eq!(
            split("Okay, listen up, go left now. The portal's there.", 5),
            ["Okay, listen up, go left now.", "The portal's there."]
        );
        assert_eq!(
            split("Sure, go left to the portal. Then enter Preserve.", 5),
            ["Sure, go left to the portal.", "Then enter Preserve."]
        );
        assert!(!is_announcement("Pot now, you're at 20."));
        assert!(!is_announcement(
            "I'll warn you sooner from now on, under 35%."
        ));
        assert!(is_announcement("Here's the deal."));
        assert!(is_announcement("בוא נעשה את זה צעד אחר צעד."));
        assert!(is_announcement("טוב, אני אסביר לך בקצרה."));
        assert!(!is_announcement("טוב, לך שמאלה לפורטל."));
        // The shown text matches what was said.
        assert_eq!(
            without_announcement(
                "Alright, I'll give you the best quick route to farm it. Farm Root Abyss bosses."
            ),
            "Farm Root Abyss bosses."
        );
        assert_eq!(
            without_announcement("Farm Root Abyss bosses."),
            "Farm Root Abyss bosses."
        );
        assert_eq!(without_announcement("Here's the deal."), "Here's the deal.");
    }
}
