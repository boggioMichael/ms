//! The conversation: who MapleSyrup is, what it sees right now, and what
//! was said so far. The model gets all three with every sentence, so it can
//! talk about the game the way a friend watching it would.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use super::memory::Learning;
use super::openai::{Delivery, Turn};
use super::style::{self, Attitude};
use crate::companion::{GameView, Gauge, Observation, Progress, SoFar};

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
///
/// A reply's sentences go in as they are written, before the voice says
/// them (that is when they are checked). What the voice never got to say
/// (the player talked over it) must not count as said: once the voice is
/// done, the reply's entries are settled against what it did say
/// (`settle`).
#[derive(Default)]
pub struct Recent {
    /// Each with when it was said, and its number (the `n`th recorded), so
    /// that the entries made since a point can be found again.
    said: VecDeque<(Instant, u64, String)>,
    /// How many sentences were recorded so far.
    taken: u64,
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
        while self.said.front().is_some_and(|(at, _, _)| {
            now.duration_since(*at) > RECENT_FOR || self.said.len() > RECENT_MAX
        }) {
            self.said.pop_front();
        }
        let plain = normalised(sentence);
        if plain.is_empty() {
            return true;
        }
        if self.said.iter().any(|(_, _, said)| alike(said, &plain)) {
            return false;
        }
        self.said.push_back((now, self.taken, plain));
        self.taken += 1;
        true
    }

    /// Where the record stands: how many sentences it took so far. A reply
    /// notes it before its first sentence, to `settle` by.
    pub fn taken(&self) -> u64 {
        self.taken
    }

    /// A reply's sentences went in as they were written (from the `taken`th
    /// on), before the voice said them. Now the voice is done: what it did
    /// not say (the player talked over it; it was called off) is not said
    /// lately, and goes — and the sentences of `spoken`, what it did say,
    /// are recorded in its place.
    pub fn settle(&mut self, taken: u64, spoken: &str) {
        while self.said.back().is_some_and(|(_, n, _)| *n >= taken) {
            self.said.pop_back();
        }
        for sentence in sentences_of(spoken) {
            self.fresh(&sentence);
        }
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

/// How the voice should sound (OpenAI's voice takes it as its
/// instructions): the attitude the player picked, and how this line is
/// to be delivered — a warning faster and sharper than talk, a reply at
/// the attitude's pace, a long explanation a touch slower and steadier.
pub fn voice_style(delivery: Delivery) -> String {
    let voice = match delivery.attitude {
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
    };
    let pace = if delivery.urgent() {
        "This line is a warning (a beating, low HP, a death): urgent, faster and sharper than your usual talk, \
like shouting a heads-up to a teammate mid-fight. No lead-in; the first word hits."
    } else if delivery.long {
        "This line is a longer explanation: a touch slower and steadier than your usual chat, so every word \
lands, still without pauses."
    } else {
        "This line is a reply in the chat: your usual pace."
    };
    format!("{voice} {pace}")
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
- Don't say again what you said in your last two replies unless they ask again, and never open with where they \
are unless they asked. If what you heard makes no sense (a bad transcription), say in a few words that you didn't \
catch it; don't guess what they meant.
- If your last reply ends with \"…\", they talked over you there: don't repeat it; go with what they said now.
- Trust your eyes: use what you see when it bears on the moment (values marked \"about\" are estimates); if the \
screen clearly shows something other than what they say, say what you see; never ask them to read the screen to \
you — look closer instead.
- When they correct you, take it in a word and keep it (note_correction); what they corrected you on before beats what you think you know.
- Presence: greet only when your watcher says the phone just connected, never on your own; never ask whether \
they're still there — your watcher does, when they go quiet. When the session facts say they had been quiet for \
a long while until just now, one short \"welcome back\" is fine, once. Those facts (how long, deaths, level-ups, \
when they last spoke, the lowest HP) are for you, not for them: never recite them; one comes up only when it \
changes what you'd say.
- What you know about them from before comes in only when it bears on what they just said, as a clause, never \
as a list: \"that boss again?\", not \"I remember you fought Zakum, wanted a Fafnir and play Mu Lung Dojo\".
- You can't press keys or play for them; you watch and talk.
- If they're clearly talking to someone else (their stream chat, a friend, a call) and not to you, reply with exactly: [silent]";

/// How a turn of the conversation that is the watcher's, not the player's,
/// starts (what follows says why it spoke: "an alert", "new scene").
pub const WATCHER: &str = "[Your game watcher, not the player:";

/// How it sounds, for the voice: the attitude the player picked, and the
/// ElevenLabs voice (both kept in `Learning`, shared with the phone that
/// sets them). Read through here where the brain is not — the lane that
/// says its own lines.
#[derive(Clone)]
pub struct Tuning {
    learning: Option<Learning>,
    /// The attitude when there is no `learning` to keep it.
    attitude: Attitude,
}

impl Tuning {
    /// How it talks now.
    pub fn attitude(&self) -> Attitude {
        attitude_of(self.learning.as_ref(), self.attitude)
    }

    /// The ElevenLabs voice the player picked, if they did.
    pub fn voice_id(&self) -> Option<String> {
        voice_of(self.learning.as_ref())
    }
}

/// The attitude the player picked on the phone, or `fallback` when nothing
/// keeps it.
fn attitude_of(learning: Option<&Learning>, fallback: Attitude) -> Attitude {
    learning.map(|l| l.memory().attitude).unwrap_or(fallback)
}

/// The ElevenLabs voice the player picked, if they did.
fn voice_of(learning: Option<&Learning>) -> Option<String> {
    learning
        .and_then(|l| l.memory().voice.clone())
        .filter(|v| !v.is_empty() && v != "openai")
}

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

    /// A line it said on its own — a warning, the coach's callout — into
    /// the conversation, after the watcher's word for why (`label`: "an
    /// alert", "new scene"), so that the next reply knows its own last
    /// words ("yeah yeah, I'm potting" has an "it").
    pub fn watched(&mut self, label: &str, line: &str) {
        self.heard(&format!("{WATCHER} {label}.]"));
        self.said(line);
    }

    /// The last reply was talked over: only `heard` of it was heard (cut
    /// off there). A line of its own said since (`watched`) is not the
    /// reply, and is skipped.
    pub fn cut_short(&mut self, heard: &str) {
        let mut end = self.turns.len();
        while end >= 2
            && self.turns[end - 1].role == "assistant"
            && self.turns[end - 2].role == "user"
            && self.turns[end - 2].text.starts_with(WATCHER)
        {
            end -= 2;
        }
        let Some(last) = end
            .checked_sub(1)
            .map(|at| &mut self.turns[at])
            .filter(|t| t.role == "assistant")
        else {
            return;
        };
        let heard = heard.trim().trim_end_matches(['.', ' ']);
        last.text = if heard.is_empty() {
            "…".to_string()
        } else {
            format!("{heard}…")
        };
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

    /// How it sounds, to read where the brain is not: it follows the
    /// player's choices on the phone, as the brain does.
    pub fn tuning(&self) -> Tuning {
        Tuning {
            learning: self.learning.clone(),
            attitude: self.attitude,
        }
    }

    /// How it talks now (the player picks it on the phone).
    pub fn attitude(&self) -> Attitude {
        attitude_of(self.learning.as_ref(), self.attitude)
    }

    /// The ElevenLabs voice the player picked, if they did.
    pub fn voice_id(&self) -> Option<String> {
        voice_of(self.learning.as_ref())
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

/// A duration the short way: "under a minute", "25 min", "1 h 12 min",
/// "2 h".
fn short(seconds: f64) -> String {
    let minutes = (seconds / 60.0).round() as u64;
    match (minutes / 60, minutes % 60) {
        (0, 0) => "under a minute".to_string(),
        (0, m) => format!("{m} min"),
        (h, 0) => format!("{h} h"),
        (h, m) => format!("{h} h {m} min"),
    }
}

/// How long ago, rounded: "just now" within the minute.
fn ago(seconds: f64) -> String {
    if seconds < 60.0 {
        "just now".to_string()
    } else {
        format!("{} ago", short(seconds))
    }
}

/// Silence from the player long enough to be worth a word, in seconds.
const QUIET_PLAYER_SECS: f64 = 60.0;
/// The game unmoving (no HP or EXP change) for this long is worth a word.
const QUIET_GAME_SECS: f64 = 120.0;
/// The game out of sight for this long is worth a word.
const UNSEEN_SECS: f64 = 60.0;
/// The lowest HP lately is worth a word under this.
const LOW_HP_WORTH_A_WORD: f32 = 50.0;

/// The session so far, as a friend in the room would know it, in one line
/// or none: what has something to say is said, rounded; the rest is left
/// out. (How long the session has run goes with the EXP rate.)
fn presence(so_far: &SoFar) -> Option<String> {
    let mut parts = Vec::new();
    // (What they just said ended a long silence: the reply to it is the
    // one that may welcome them back; `since_player_spoke` is 0 by now.)
    match (so_far.quiet_before, so_far.since_player_spoke) {
        (Some(quiet), _) => parts.push(format!(
            "They had been quiet for {} until just now.",
            short(quiet)
        )),
        (None, None) if so_far.seconds >= QUIET_PLAYER_SECS => {
            parts.push("They haven't said anything yet this session.".to_string())
        }
        (None, Some(since)) if since >= QUIET_PLAYER_SECS => {
            parts.push(format!("They last spoke {}.", ago(since)))
        }
        _ => {}
    }
    if so_far.deaths > 0 {
        let last = so_far
            .since_last_death
            .map(|s| format!(" (last one {})", ago(s)))
            .unwrap_or_default();
        parts.push(format!("Deaths: {}{last}.", so_far.deaths));
    }
    if so_far.level_ups > 0 {
        let last = so_far
            .since_last_level_up
            .map(|s| format!(" (last one {})", ago(s)))
            .unwrap_or_default();
        parts.push(format!("Level-ups: {}{last}.", so_far.level_ups));
    }
    if let Some(lowest) = so_far.lowest_hp_lately.filter(|l| *l < LOW_HP_WORTH_A_WORD) {
        parts.push(format!("Lowest HP in the last minute: {:.0}%.", lowest));
    }
    if let Some(quiet) = so_far.quiet_for.filter(|q| *q >= QUIET_GAME_SECS) {
        parts.push(format!(
            "The game has been quiet for {} (no HP or EXP change).",
            short(quiet)
        ));
    }
    (!parts.is_empty()).then(|| parts.join(" "))
}

/// What the companion sees, as a few plain lines for the model, and what
/// a friend in the room would know of the session so far (`so_far`).
pub fn snapshot(obs: Option<&Observation>, progress: &Progress, so_far: &SoFar) -> String {
    let mut lines = Vec::new();
    let unseen = so_far
        .unseen_for
        .filter(|u| *u >= UNSEEN_SECS)
        .map(|u| format!(" (for {})", short(u)))
        .unwrap_or_default();
    match obs.map(|o| &o.game) {
        Some(GameView::Seen(_)) => {
            lines.push("The MapleStory window is open and in view.".to_string())
        }
        Some(GameView::Unavailable(why)) => lines.push(format!(
            "MapleStory can't be seen right now{unseen}: {why}."
        )),
        _ => lines.push(format!("No MapleStory window is open right now{unseen}.")),
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
    let mut session = vec![format!("Session: {}", short(progress.seconds))];
    if let Some(rate) = progress.exp_per_hour {
        session.push(format!("EXP rate about {rate:+.1}% per hour"));
    }
    if let Some(eta) = progress.seconds_to_level {
        session.push(format!("next level in about {}", duration(eta)));
    }
    lines.push(format!("{}.", session.join("; ")));
    if let Some(line) = presence(so_far) {
        lines.push(line);
    }
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
/// when it turns out to be the whole reply. The assistant goes the same
/// way ([`humanise`], sentence by sentence as they complete): assistant-
/// speak is not handed out, and a sentence that offers help is held until
/// it is known whether the reply ends there (then it goes) or not.
#[derive(Default)]
pub struct Sentences {
    pending: String,
    /// Sentences handed out so far.
    given: usize,
    /// A first sentence held back as an announcement.
    held: Option<String>,
    /// A sentence (or a few) ending on a question that offers help, held
    /// until more of the reply comes, or its end.
    offer: Option<String>,
    /// What went as assistant-speak, in case it is all there was.
    dropped: String,
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
        // More came after the offer: it did not close the reply, so it goes
        // out like any sentence.
        if !self.pending.trim().is_empty()
            && let Some(offer) = self.offer.take()
        {
            self.held = None;
            self.given += 1;
            out.push(offer);
        }
        while let Some(end) = sentence_end(&self.pending, SENTENCE_MIN_CHARS) {
            let chunk: String = self.pending.drain(..end).collect();
            let chunk = chunk.trim();
            if chunk.is_empty() {
                continue;
            }
            let kept = without_assistant(&without_marks(chunk));
            let Some(last) = kept.last() else {
                self.set_aside(chunk);
                continue;
            };
            let closes = is_offer(last) && self.pending.trim().is_empty();
            let sentence = kept.join(" ");
            if self.given == 0
                && self.held.is_none()
                && self.offer.is_none()
                && is_announcement(&sentence)
            {
                self.held = Some(sentence);
                continue;
            }
            // (An announcement stays held under an offer: the offer may go
            // at the end, and the announcement be all there was.)
            if closes {
                self.offer = Some(sentence);
                continue;
            }
            // The answer came: the announcement before it is not said.
            self.held = None;
            self.given += 1;
            out.push(sentence);
        }
        out
    }

    /// What is left at the end of the reply.
    pub fn finish(&mut self) -> Option<String> {
        let rest = std::mem::take(&mut self.pending);
        let rest = rest.trim();
        let mut tail: Vec<String> = self
            .offer
            .take()
            .map(|offer| sentences_of(&offer))
            .unwrap_or_default();
        if !rest.is_empty() {
            let kept = without_assistant(&without_marks(rest));
            if kept.is_empty() {
                self.set_aside(rest);
            }
            tail.extend(kept);
        }
        // A closing offer goes, when anything was said before it (a held
        // announcement counts: it is said when the offer goes).
        without_closing_offer(&mut tail, self.given > 0 || self.held.is_some());
        if !tail.is_empty() {
            self.held = None;
            self.given += 1;
            return Some(tail.join(" "));
        }
        if self.given > 0 {
            return None;
        }
        // The announcement, or the assistant-speak, was all there was:
        // better than nothing.
        self.held.take().or_else(|| {
            let dropped = std::mem::take(&mut self.dropped);
            (!dropped.is_empty()).then_some(dropped)
        })
    }

    /// Assistant-speak that went: said at the end only when nothing else was.
    fn set_aside(&mut self, chunk: &str) {
        if !self.dropped.is_empty() {
            self.dropped.push(' ');
        }
        self.dropped.push_str(chunk);
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
/// emoji — and no assistant in it ([`humanise`]).
pub fn for_speech(reply: &str) -> String {
    let plain: String = reply
        .chars()
        .filter(|c| !matches!(c, '*' | '#' | '`' | '_' | '~' | '>'))
        .collect();
    humanise(&without_links(&without_marks(&plain)))
}

/// A reply with the assistant taken out of it. MapleSyrup is a friend on
/// voice chat, and a friend never says "as an AI", never offers help and
/// never signs off with "let me know if…"; a model does, now and then,
/// whatever it is told. What goes — and what is left means what it meant:
/// - sentences that are assistant-speak: "I'm just an AI", "I don't have
///   feelings", "I'm here to help", "Happy to help", "Let me know if…",
///   "Feel free to…", "Hope this helps", "Is there anything else…", "Great
///   question", and the Hebrew ("אני רק בינה מלאכותית", "אני כאן כדי
///   לעזור", "אשמח לעזור", "תרגיש חופשי", "מקווה שזה עוזר", "שאלה מצוינת");
/// - the openers "Certainly!", "Of course,", "Absolutely!", "Sure thing,",
///   "As mentioned,", "In summary,", "As an AI," ("בהחלט!", "כמובן!",
///   "לסיכום,", "כבינה מלאכותית,") at the start of a sentence, which keeps
///   the rest of it;
/// - a closing question that offers help ("Want me to…?", "Should I…?",
///   "רוצה ש…?") when anything was said before it;
/// - a sentence that only restates the question ("You asked where you
///   are."; "You asked where you are: Henesys." keeps "Henesys.");
/// - list bullets and numbers, "Note:" and "Tip:" labels, and emoji.
///
/// When every sentence would go, they all stay (bar the marks): a reply of
/// nothing but "Happy to help!" is still a reply, and better than silence.
pub fn humanise(reply: &str) -> String {
    let plain = without_marks(reply);
    let mut kept = without_assistant(&plain);
    without_closing_offer(&mut kept, false);
    if kept.is_empty() {
        plain
    } else {
        kept.join(" ")
    }
}

/// Whether `c` is an emoji (or one of the marks that ride along with
/// them: variation selectors, the joiner, keycaps) or a bullet.
fn is_emoji(c: char) -> bool {
    let u = c as u32;
    u >= 0x1F000
        || (0x2300..=0x23FF).contains(&u)
        || (0x2600..=0x27BF).contains(&u)
        || (0x2B00..=0x2BFF).contains(&u)
        || matches!(u, 0x200D | 0x20E3 | 0x2022 | 0xFE0F)
}

/// Whether `word` ends a sentence (a closing quote or bracket after the
/// stop counts).
fn ends_sentence(word: &str) -> bool {
    word.trim_end_matches(['"', '\'', '”', '’', ')', ']'])
        .ends_with(['.', '!', '?', ':', '…'])
}

/// `word` as a list marker: a bullet (`None`) or an item's number.
fn list_marker(word: &str) -> Option<Option<u32>> {
    if matches!(word, "-" | "–" | "—" | "*" | "•") {
        return Some(None);
    }
    let bare = word.strip_prefix('(').unwrap_or(word);
    let digits = bare.strip_suffix(['.', ')'])?;
    if digits.is_empty() || digits.len() > 2 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok().map(Some)
}

/// A label a model puts before a sentence: "Note:", "Tip:".
fn is_label(word: &str) -> bool {
    matches!(
        word.to_lowercase().as_str(),
        "note:" | "tip:" | "hint:" | "important:" | "reminder:" | "הערה:" | "טיפ:"
    )
}

/// `text` with its first letter in upper case.
fn capitalised(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) if first.is_lowercase() => first.to_uppercase().chain(chars).collect(),
        _ => text.to_string(),
    }
}

/// `text` without emoji, list bullets and numbers (a numbered list counts
/// from 1; "Level? 20." is an answer) or "Note:"/"Tip:" labels, one space
/// between words.
fn without_marks(text: &str) -> String {
    // Each word, and whether a line break came before it.
    let mut words: Vec<(String, bool)> = Vec::new();
    let mut word = String::new();
    let mut broke = true;
    for c in text.chars().filter(|c| !is_emoji(*c)) {
        if c.is_whitespace() {
            if !word.is_empty() {
                words.push((std::mem::take(&mut word), broke));
                broke = false;
            }
            broke |= c == '\n';
        } else {
            word.push(c);
        }
    }
    if !word.is_empty() {
        words.push((word, broke));
    }
    let mut out: Vec<String> = Vec::new();
    let mut expected = 1;
    let mut capitalise = false;
    let mut i = 0;
    while i < words.len() {
        let (word, broke) = &words[i];
        let marker = list_marker(word);
        // A list's next number counts wherever it sits ("1) pot, 2) run").
        let opens = *broke
            || out.last().is_none_or(|last| ends_sentence(last))
            || (expected > 1 && marker == Some(Some(expected)));
        let last = i + 1 == words.len();
        if opens && !last {
            match marker {
                Some(None) => {
                    i += 1;
                    continue;
                }
                Some(Some(number)) if number == expected => {
                    expected += 1;
                    i += 1;
                    continue;
                }
                _ => {}
            }
            if is_label(word) {
                capitalise = true;
                i += 1;
                continue;
            }
            if word.eq_ignore_ascii_case("pro")
                && words
                    .get(i + 1)
                    .is_some_and(|(next, _)| next.eq_ignore_ascii_case("tip:"))
            {
                capitalise = true;
                i += 2;
                continue;
            }
        }
        out.push(if capitalise {
            capitalised(word)
        } else {
            word.clone()
        });
        capitalise = false;
        i += 1;
    }
    out.join(" ")
}

/// `text` for matching phrases: lower case, apostrophes out ("I'm" and
/// "im" alike), letters and digits only, one space between words.
fn plain_words(text: &str) -> String {
    let mut out = String::new();
    let mut space = true;
    for c in text.chars() {
        if matches!(c, '\'' | '’' | '‘' | '׳') {
            continue;
        }
        if c.is_alphanumeric() {
            out.extend(c.to_lowercase());
            space = false;
        } else if !space {
            out.push(' ');
            space = true;
        }
    }
    out.trim_end().to_string()
}

/// Phrases a sentence goes for, in either language.
const ASSISTANT: &[&str] = &[
    "im just an ai",
    "im an ai",
    "i am an ai",
    "i am just an ai",
    "as an ai",
    "as a language model",
    "im a language model",
    "im just a language model",
    "im a bot",
    "im just a bot",
    "im an artificial intelligence",
    "im a virtual assistant",
    "im an assistant",
    "im just a program",
    "im a computer program",
    "i dont have feelings",
    "i dont have real feelings",
    "i dont have emotions",
    "i dont have eyes",
    "i dont have a body",
    "i dont have personal",
    "i dont have the ability to feel",
    "im here to help",
    "here to help",
    "happy to help",
    "glad to help",
    "glad i could help",
    "happy to assist",
    "here to assist",
    "let me know if",
    "let me know when",
    "let me know whether",
    "let me know what",
    "let me know which",
    "just let me know",
    "feel free to",
    "i hope this helps",
    "hope this helps",
    "hope that helps",
    "hope it helps",
    "hope this helped",
    "is there anything else",
    "anything else i can",
    "anything else you need",
    "anything else youd like",
    "if you have any other questions",
    "if you have any questions",
    "if you need anything else",
    "if you need any more",
    "dont hesitate to",
    "great question",
    "good question",
    "excellent question",
    "אני רק בינה מלאכותית",
    "אני בינה מלאכותית",
    "כבינה מלאכותית",
    "אני מודל שפה",
    "כמודל שפה",
    "אני רק בוט",
    "אני בוט",
    "אני רק תוכנה",
    "אני כאן כדי לעזור",
    "אני כאן בשביל לעזור",
    "כאן כדי לעזור",
    "אשמח לעזור",
    "שמח לעזור",
    "תרגיש חופשי",
    "תרגישי חופשייה",
    "אל תהסס",
    "אל תהססי",
    "מקווה שזה עוזר",
    "מקווה שזה עזר",
    "מקווה שעזרתי",
    "שאלה מצוינת",
    "שאלה טובה",
    "שאלה מעולה",
    "יש עוד משהו",
    "אם יש לך שאלות נוספות",
    "אם יש לך עוד שאלות",
    "אם תצטרך עוד משהו",
    "תגיד לי אם",
    "תגידי לי אם",
    "אין לי רגשות",
    "אין לי עיניים",
    "אין לי גוף",
    "אין לי תחושות",
];

/// What a sentence may open with and lose: the opener, when a comma, a
/// stop or a dash follows it.
const OPENERS: &[&str] = &[
    "certainly",
    "of course",
    "absolutely",
    "sure thing",
    "great question",
    "good question",
    "excellent question",
    "as mentioned",
    "as mentioned before",
    "as mentioned earlier",
    "as mentioned above",
    "as i mentioned",
    "as i mentioned before",
    "as i mentioned earlier",
    "as i mentioned above",
    "in summary",
    "to summarize",
    "to summarise",
    "to sum up",
    "as an ai",
    "as an ai language model",
    "as a language model",
    "as a bot",
    "as an assistant",
    "as a virtual assistant",
    "being an ai",
    "im just an ai",
    "im an ai",
    "i am an ai",
    "im just a bot",
    "im a bot",
    "im just a language model",
    "im a language model",
    "i dont have feelings",
    "i dont have real feelings",
    "i dont have emotions",
    "i dont have eyes",
    "i dont have a body",
    "בהחלט",
    "כמובן",
    "שאלה מצוינת",
    "שאלה טובה",
    "שאלה מעולה",
    "לסיכום",
    "כבינה מלאכותית",
    "כמודל שפה",
    "אני רק בינה מלאכותית",
    "אני בינה מלאכותית",
    "אני רק בוט",
    "אין לי רגשות",
    "אין לי עיניים",
    "אין לי גוף",
];

/// A conjunction that may follow an opener ("I'm just an AI, but…").
const CONJUNCTIONS: &[&str] = &["but ", "and ", "so ", "אבל ", "אז "];

/// How a sentence that restates the question starts, and whether it must
/// go on with a question word to count ("You asked for it." is a taunt).
const RESTATES: &[(&str, bool)] = &[
    ("you asked", true),
    ("youre asking", true),
    ("you are asking", true),
    ("so you asked", true),
    ("so youre asking", true),
    ("if youre asking", true),
    ("you want to know", false),
    ("you wanted to know", false),
    ("so you want to know", false),
    ("your question is", false),
    ("your question was", false),
    ("youre wondering", false),
    ("you are wondering", false),
    ("so youre wondering", false),
    ("שאלת", true),
    ("אז שאלת", true),
    ("אתה שואל", true),
    ("את שואלת", true),
    ("אז אתה שואל", true),
    ("אתה רוצה לדעת", false),
    ("את רוצה לדעת", false),
    ("השאלה שלך", false),
];

const QUESTION_WORDS: &[&str] = &[
    "where",
    "what",
    "whats",
    "how",
    "when",
    "why",
    "which",
    "who",
    "about",
    "if",
    "whether",
    "איפה",
    "מה",
    "כמה",
    "איך",
    "מתי",
    "למה",
    "איזה",
    "איזו",
    "מי",
    "על",
    "אם",
    "האם",
    "לאן",
    "מאיפה",
];

/// How a question that offers help starts (a Hebrew form without a
/// trailing space goes on into the next word: "רוצה שאסמן").
const OFFERS: &[&str] = &[
    "want me to ",
    "do you want me to ",
    "would you like me to ",
    "should i ",
    "shall i ",
    "need me to ",
    "do you need me to ",
    "you want me to ",
    "can i help ",
    "how can i help ",
    "anything else ",
    "need anything else ",
    "want a hand ",
    "want help ",
    "need help ",
    "do you want help ",
    "do you need help ",
    "רוצה ש",
    "אתה רוצה ש",
    "את רוצה ש",
    "תרצה ש",
    "תרצי ש",
    "שאני ",
    "צריך שאני ",
    "צריכה שאני ",
    "לעזור לך",
    "רוצה עזרה",
    "צריך עזרה",
    "יש עוד משהו",
];

/// Whether `sentence` is a question that offers help ("Want me to mark
/// this spot?").
pub fn is_offer(sentence: &str) -> bool {
    let text = sentence.trim().trim_end_matches(['"', '\'', '”', '’', ')']);
    if !text.ends_with('?') {
        return false;
    }
    let padded = format!("{} ", plain_words(text));
    OFFERS.iter().any(|o| padded.starts_with(o))
}

/// What is left of a sentence that starts with an opener.
enum Opened {
    /// The rest of it (empty when the opener was the whole sentence).
    Rest(String),
    /// Nothing goes: what follows the opener is a word or two that is no
    /// sentence of its own — a name, a vocative ("Sure thing, boss.", "Of
    /// course, genius.") — so the opener is the joke, and the sentence
    /// stays whole.
    Whole,
}

/// Words that make a word or two after an opener a sentence of its own:
/// orders ("go left", "pot"), the verbs a pronoun takes ("it's Henesys",
/// "you're bad", "I know"), and their Hebrew; a Hebrew pronoun with
/// anything after it is a sentence too ("אתה גרוע": there is no "are").
const VERBS: &[&str] = &[
    "go",
    "pot",
    "run",
    "move",
    "back",
    "rebuff",
    "buff",
    "drink",
    "heal",
    "revive",
    "respawn",
    "stop",
    "wait",
    "hold",
    "hang",
    "jump",
    "use",
    "buy",
    "sell",
    "talk",
    "head",
    "get",
    "keep",
    "stay",
    "look",
    "check",
    "try",
    "kill",
    "hit",
    "attack",
    "dodge",
    "climb",
    "enter",
    "leave",
    "come",
    "take",
    "grab",
    "pick",
    "farm",
    "grind",
    "train",
    "press",
    "switch",
    "change",
    "open",
    "close",
    "turn",
    "watch",
    "listen",
    "focus",
    "relax",
    "chill",
    "calm",
    "breathe",
    "hurry",
    "follow",
    "read",
    "ask",
    "tell",
    "say",
    "answer",
    "play",
    "fight",
    "retreat",
    "flee",
    "escape",
    "teleport",
    "port",
    "warp",
    "hop",
    "cast",
    "equip",
    "wear",
    "craft",
    "trade",
    "swap",
    "return",
    "continue",
    "repeat",
    "start",
    "finish",
    "win",
    "lose",
    "die",
    "pay",
    "click",
    "type",
    "mute",
    "save",
    "record",
    "pause",
    "resume",
    "see",
    "do",
    "be",
    "is",
    "are",
    "am",
    "was",
    "were",
    "know",
    "think",
    "mean",
    "want",
    "need",
    "have",
    "has",
    "can",
    "will",
    "should",
    "must",
    "dont",
    "cant",
    "wont",
    "isnt",
    "arent",
    "didnt",
    "doesnt",
    "lets",
    "let",
    "im",
    "youre",
    "its",
    "thats",
    "theres",
    "heres",
    "hes",
    "shes",
    "theyre",
    "ill",
    "youll",
    "ive",
    "youve",
    "id",
    "youd",
    "לך",
    "לכי",
    "תלך",
    "רוץ",
    "תרוץ",
    "זוז",
    "תזוז",
    "תשתה",
    "שתה",
    "תתרחק",
    "תברח",
    "ברח",
    "חכה",
    "תחכה",
    "עצור",
    "תעצור",
    "תחזור",
    "חזור",
    "תקנה",
    "קנה",
    "תדבר",
    "דבר",
    "תלחץ",
    "לחץ",
    "תסתכל",
    "תראה",
    "תנסה",
    "נסה",
    "קח",
    "תיקח",
    "בוא",
    "בואי",
    "תעלה",
    "עלה",
    "תרד",
    "רד",
    "תקפוץ",
    "קפוץ",
    "תמשיך",
    "תפסיק",
    "תירגע",
    "תתחדש",
    "תילחם",
    "תהרוג",
    "תתקוף",
    "תשמור",
    "תעשה",
    "עשה",
    "תן",
    "תני",
    "תחשוב",
    "תקשיב",
    "תענה",
    "שחק",
    "תשחק",
    "תפתח",
    "תסגור",
    "יכול",
    "יכולה",
    "צריך",
    "צריכה",
    "רוצה",
    "חייב",
    "חייבת",
    "יש",
    "אין",
];

/// Hebrew pronouns (and the "it's" of "זה הנסיס"): with a word after them,
/// a sentence.
const HEBREW_PRONOUNS: &[&str] = &[
    "אני",
    "אתה",
    "את",
    "הוא",
    "היא",
    "אנחנו",
    "אתם",
    "אתן",
    "הם",
    "הן",
    "זה",
    "זאת",
    "זו",
];

/// Whether `rest`, what follows an opener, is a sentence of its own: three
/// words or more, or a word or two with a verb in it (`VERBS`), or a
/// Hebrew pronoun with a word after it. A bare name or vocative ("boss",
/// "genius", "my friend", "captain obvious") is not: the opener before it
/// was the point.
fn stands_alone(rest: &str) -> bool {
    let plain = plain_words(rest);
    let words: Vec<&str> = plain.split(' ').filter(|w| !w.is_empty()).collect();
    words.len() >= 3
        || words.iter().any(|w| VERBS.contains(w))
        || (words.len() == 2 && HEBREW_PRONOUNS.contains(&words[0]))
}

/// `sentence` without the opener it starts with (and the comma or stop
/// after it, and a "but"), or `None` when it starts with none. The opener
/// goes only when what follows it stands as a sentence of its own
/// (`stands_alone`): "Sure thing, I'll keep an eye on it." loses its
/// opener, "Sure thing, boss." keeps it (`Opened::Whole`).
fn without_opener(sentence: &str) -> Option<Opened> {
    let chars: Vec<char> = sentence.chars().collect();
    'openers: for opener in OPENERS {
        let mut at = 0;
        for wanted in opener.chars() {
            // Apostrophes in the sentence are not in the opener.
            while chars
                .get(at)
                .is_some_and(|c| matches!(c, '\'' | '’' | '‘' | '׳'))
            {
                at += 1;
            }
            let Some(&c) = chars.get(at) else {
                continue 'openers;
            };
            if c.to_lowercase().next() != Some(wanted) {
                continue 'openers;
            }
            at += 1;
        }
        let Some(&next) = chars.get(at) else {
            // The opener was the whole sentence.
            return Some(Opened::Rest(String::new()));
        };
        if !matches!(next, ',' | '!' | ':' | ';' | '.' | '…' | '—' | '–' | '-') {
            continue;
        }
        while chars.get(at).is_some_and(|c| {
            matches!(c, ',' | '!' | ':' | ';' | '.' | '…' | '—' | '–' | '-') || c.is_whitespace()
        }) {
            at += 1;
        }
        let mut rest: String = chars[at..].iter().collect();
        let lower = rest.to_lowercase();
        if let Some(conjunction) = CONJUNCTIONS.iter().find(|c| lower.starts_with(*c)) {
            rest = rest[conjunction.len()..].trim_start().to_string();
        }
        if !rest.trim().is_empty() && !stands_alone(&rest) {
            return Some(Opened::Whole);
        }
        return Some(Opened::Rest(rest));
    }
    None
}

/// `sentence` with the assistant taken out, or `None` when nothing else
/// was in it.
fn human_sentence(sentence: &str) -> Option<String> {
    let mut text = sentence.trim().to_string();
    let mut opened = false;
    loop {
        match without_opener(&text) {
            Some(Opened::Rest(rest)) => {
                text = rest;
                opened = true;
            }
            // The opener is the joke ("Sure thing, boss."): the sentence
            // is a friend's, whatever else it contains.
            Some(Opened::Whole) => {
                return Some(if opened { capitalised(&text) } else { text });
            }
            None => break,
        }
    }
    if text.is_empty() {
        return None;
    }
    let plain = plain_words(&text);
    let padded = format!(" {plain} ");
    if ASSISTANT.iter().any(|p| padded.contains(&format!(" {p} ")))
        || padded.ends_with(" let me know ")
    {
        return None;
    }
    // Only the question again: what comes after a colon or a dash is the
    // answer, and stays.
    let restates = RESTATES.iter().any(|(start, needs_question)| {
        padded.starts_with(&format!(" {start} "))
            && (!needs_question
                || QUESTION_WORDS
                    .iter()
                    .any(|q| padded[start.len() + 1..].contains(&format!(" {q} "))))
    });
    if restates {
        let answer = [":", " — ", " – ", " - ", ", and ", ", so ", "; "]
            .iter()
            .filter_map(|sep| text.find(sep).map(|at| at + sep.len()))
            .min()
            .map(|at| text[at..].trim().to_string())
            .filter(|rest| !rest.is_empty())?;
        return Some(capitalised(&answer));
    }
    Some(if opened { capitalised(&text) } else { text })
}

/// The sentences of `text` with the assistant taken out (a closing offer
/// is `without_closing_offer`'s business: it needs the whole reply).
fn without_assistant(text: &str) -> Vec<String> {
    sentences_of(text)
        .iter()
        .filter_map(|s| human_sentence(s))
        .collect()
}

/// Takes a closing question that offers help out of `kept`, when
/// anything was said before it (in `kept`, or earlier in the reply).
fn without_closing_offer(kept: &mut Vec<String>, said_before: bool) {
    while (kept.len() >= 2 || (said_before && !kept.is_empty()))
        && kept.last().is_some_and(|s| is_offer(s))
    {
        kept.pop();
    }
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
    fn what_the_voice_never_said_is_not_said_lately() {
        let mut recent = Recent::default();
        assert!(recent.fresh("Pot now, you're at 20."));
        // A reply of two sentences, written in full; the player talks over
        // the first, so the voice never says the second.
        let taken = recent.taken();
        assert!(recent.fresh("You're at the Gate of the Future, level 165, EXP 74%."));
        assert!(recent.fresh("The quest marker is four maps to the left."));
        recent.settle(
            taken,
            "You're at the Gate of the Future, level 165, EXP 74%.",
        );
        // Asked again: the first sentence was said (at any EXP), the
        // second was not; what came before the reply still counts.
        assert!(!recent.fresh("You're at the Gate of the Future, level 165, EXP 75%."));
        assert!(recent.fresh("The quest marker is four maps to the left."));
        assert!(!recent.fresh("Pot now, you're at 20."));
        // Nothing of it said: nothing of it counts.
        let taken = recent.taken();
        assert!(recent.fresh("Rebuff, you're naked."));
        recent.settle(taken, "");
        assert!(recent.fresh("Rebuff, you're naked."));
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
        let so_far = SoFar {
            seconds: 42.0 * 60.0,
            since_player_spoke: Some(12.0),
            ..Default::default()
        };
        let text = snapshot(Some(&obs), &progress, &so_far);
        assert!(text.contains("Character: level 57, Assassin."));
        assert!(text.contains("HP 1291 of 1351 (82%), MP about 40%."));
        assert!(text.contains(
            "Session: 42 min; EXP rate about +9.1% per hour; next level in about 3 h 0 min."
        ));
        // Nothing to say of the session so far (they just spoke, no
        // deaths, no level-ups, HP fine): no line about it.
        assert_eq!(text.lines().count(), 4, "{text}");
        assert!(
            snapshot(None, &Progress::default(), &SoFar::default())
                .starts_with("No MapleStory window is open right now.")
        );
    }

    #[test]
    fn the_snapshot_says_what_a_friend_in_the_room_would_know() {
        let obs = Observation {
            game: GameView::Seen("MapleStory".into()),
            hp: Some(Gauge {
                percent: 100.0,
                current: None,
                max: None,
                read: false,
            }),
            mp: None,
            exp: None,
            level: Some(165),
            name: None,
            job: None,
        };
        let progress = Progress {
            seconds: 72.0 * 60.0 + 20.0,
            levels_gained: 1,
            ..Default::default()
        };
        let so_far = SoFar {
            seconds: 72.0 * 60.0 + 20.0,
            since_player_spoke: Some(25.0 * 60.0 + 10.0),
            quiet_before: None,
            deaths: 3,
            since_last_death: Some(4.0 * 60.0 + 5.0),
            level_ups: 1,
            since_last_level_up: Some(40.0),
            lowest_hp_lately: Some(8.4),
            quiet_for: Some(30.0),
            unseen_for: None,
        };
        let text = snapshot(Some(&obs), &progress, &so_far);
        assert!(text.contains("Session: 1 h 12 min."), "{text}");
        assert!(
            text.contains(
                "They last spoke 25 min ago. Deaths: 3 (last one 4 min ago). Level-ups: 1 (last one \
just now). Lowest HP in the last minute: 8%."
            ),
            "{text}"
        );
        // Half a minute without a change is not quiet; six minutes is.
        assert!(!text.contains("quiet"), "{text}");
        let quiet = SoFar {
            quiet_for: Some(6.0 * 60.0),
            lowest_hp_lately: Some(95.0),
            since_player_spoke: Some(3.0 * 3600.0 + 5.0 * 60.0),
            ..so_far.clone()
        };
        let text = snapshot(Some(&obs), &progress, &quiet);
        assert!(
            text.contains("The game has been quiet for 6 min (no HP or EXP change)."),
            "{text}"
        );
        assert!(text.contains("They last spoke 3 h 5 min ago."), "{text}");
        // HP that never went low lately is nothing to say.
        assert!(!text.contains("Lowest HP"), "{text}");
        // Not a word from them yet, a minute in; and the game out of sight.
        let silent = SoFar {
            seconds: 90.0,
            since_player_spoke: None,
            ..Default::default()
        };
        let text = snapshot(Some(&obs), &progress, &silent);
        assert!(
            text.contains("They haven't said anything yet this session."),
            "{text}"
        );
        let gone = SoFar {
            unseen_for: Some(12.0 * 60.0),
            ..Default::default()
        };
        let text = snapshot(None, &Progress::default(), &gone);
        assert!(
            text.starts_with("No MapleStory window is open right now (for 12 min)."),
            "{text}"
        );
        let hidden = Observation::unseen(GameView::Unavailable("minimised".into()));
        let text = snapshot(Some(&hidden), &Progress::default(), &gone);
        assert!(
            text.starts_with("MapleStory can't be seen right now (for 12 min): minimised."),
            "{text}"
        );
    }

    #[test]
    fn the_voice_is_told_the_attitude_and_how_this_line_is_delivered() {
        use crate::companion::Kind;
        let style = |attitude, kind, long| {
            voice_style(Delivery {
                attitude,
                kind,
                long,
            })
        };
        for attitude in Attitude::ALL {
            let alert = style(attitude, Kind::Alert, false);
            let reply = style(attitude, Kind::Reply, false);
            let long = style(attitude, Kind::Reply, true);
            // The attitude's voice, whatever the line.
            for text in [&alert, &reply, &long] {
                assert!(text.starts_with("Voice: "), "{text}");
                assert!(text.contains("Never an announcer or a robot."), "{text}");
            }
            // A warning is urgent, a reply at the usual pace, a long
            // explanation a touch slower: three different deliveries.
            assert!(alert.contains("This line is a warning"), "{alert}");
            assert!(alert.contains("urgent, faster and sharper"), "{alert}");
            assert!(
                reply.contains("This line is a reply in the chat"),
                "{reply}"
            );
            assert!(long.contains("a touch slower and steadier"), "{long}");
            assert!(alert != reply && reply != long && alert != long);
            // News about itself goes like a reply; a warning is never long.
            assert_eq!(style(attitude, Kind::Info, false), reply);
            assert_eq!(style(attitude, Kind::Alert, true), alert);
        }
        assert!(style(Attitude::Friendly, Kind::Reply, false).contains("a warm, upbeat friend"));
        assert!(style(Attitude::Blunt, Kind::Reply, false).contains("a cocky gamer friend"));
        assert!(
            style(Attitude::Savage, Kind::Reply, false).contains("a loud, sarcastic gamer friend")
        );
    }

    #[test]
    fn the_persona_says_how_to_be_present_and_how_to_use_what_it_knows() {
        let persona = Brain::new().persona();
        // Presence is the watcher's to manage (the hello once per phone,
        // "still there?" once per session, the quiet spell in the
        // snapshot); the model is told how to respond to it: greet when
        // told the phone connected, welcome them back once after a long
        // quiet, never ask after them — and never recite the session facts.
        assert!(
            persona.contains(
                "greet only when your watcher says the phone just connected, never on your own"
            ),
            "{persona}"
        );
        assert!(
            persona.contains(
                "never ask whether they're still there — your watcher does, when they go quiet"
            ),
            "{persona}"
        );
        assert!(
            persona.contains(
                "they had been quiet for a long while until just now, one short \"welcome back\" \
is fine, once"
            ),
            "{persona}"
        );
        for managing in [
            "greet once per session",
            "at most once, after 20 minutes",
            "still there once at most",
        ] {
            assert!(!persona.contains(managing), "{managing}: {persona}");
        }
        assert!(
            persona.contains("never recite them; one comes up only when it changes what you'd say"),
            "{persona}"
        );
        // Memory in talk: a clause when it bears on what they said, never
        // a list.
        assert!(
            persona.contains(
                "only when it bears on what they just said, as a clause, never as a list: \"that \
boss again?\""
            ),
            "{persona}"
        );
        // Kept short: the two rules came with two merged, not on top.
        assert!(MORE.lines().count() == 10, "{}", MORE.lines().count());
        assert!(
            MORE.split_whitespace().count() < 360,
            "{}",
            MORE.split_whitespace().count()
        );
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
    fn its_own_lines_join_the_conversation_after_the_watchers_word() {
        let mut brain = Brain::new();
        brain.heard("where am I");
        brain.said("Gate of the Future.");
        brain.watched("an alert", "HP 20 percent. Pot now!");
        let turns = brain.turns();
        assert_eq!(turns.len(), 4);
        assert_eq!(turns[2].role, "user");
        assert_eq!(
            turns[2].text,
            "[Your game watcher, not the player: an alert.]"
        );
        assert_eq!(turns[3].role, "assistant");
        assert_eq!(turns[3].text, "HP 20 percent. Pot now!");
        // The reply talked over after that is the reply, not the warning
        // said meanwhile.
        brain.cut_short("Gate of the");
        let turns = brain.turns();
        assert_eq!(turns[1].text, "Gate of the…");
        assert_eq!(turns[3].text, "HP 20 percent. Pot now!");
        // With nothing of its own since, the last reply is the one cut.
        brain.heard("and now?");
        brain.said("Same map. Go left.");
        brain.cut_short("Same map");
        assert_eq!(brain.turns().last().unwrap().text, "Same map…");
        // A line of its own with no reply before it: nothing to cut.
        let mut brain = Brain::new();
        brain.watched("new scene", "Rebuff.");
        brain.cut_short("Reb");
        assert_eq!(brain.turns()[1].text, "Rebuff.");
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

    /// What models say now and then, whatever they are told, and what the
    /// player hears instead.
    const ASSISTANT_SPEAK: &[(&str, &str)] = &[
        (
            "As an AI, I can't see your screen, but your HP looks low.",
            "I can't see your screen, but your HP looks low.",
        ),
        (
            "I'm just an AI, but your HP's at 20. Pot.",
            "Your HP's at 20. Pot.",
        ),
        (
            "I don't have feelings, but that death hurt to watch.",
            "That death hurt to watch.",
        ),
        (
            "I don't have eyes, but the minimap shows a portal left.",
            "The minimap shows a portal left.",
        ),
        (
            "I'm a language model and I can't see colours. The boss is at 10%.",
            "The boss is at 10%.",
        ),
        ("I'm here to help! Your HP is 20.", "Your HP is 20."),
        ("Happy to help. Pot now.", "Pot now."),
        ("Glad I could help! Go left.", "Go left."),
        (
            "Pot now. Let me know if you need anything else.",
            "Pot now.",
        ),
        ("Pot now. Feel free to ask for the long route.", "Pot now."),
        (
            "Go left at the portal. I hope this helps!",
            "Go left at the portal.",
        ),
        (
            "Pot now. Is there anything else I can help with?",
            "Pot now.",
        ),
        (
            "If you want the long route, let me know.",
            "If you want the long route, let me know.",
        ),
        (
            "Great question! The boss is level 110.",
            "The boss is level 110.",
        ),
        (
            "Good question. Zakum's at level 110.",
            "Zakum's at level 110.",
        ),
        (
            "That's a great question. Zakum is level 110.",
            "Zakum is level 110.",
        ),
        ("Certainly! Pot now.", "Pot now."),
        ("Of course, go left.", "Go left."),
        ("Absolutely! You're at 20, pot.", "You're at 20, pot."),
        ("Sure thing, head to Henesys.", "Head to Henesys."),
        // The opener goes when a sentence of its own follows it…
        (
            "Sure thing, I'll keep an eye on it.",
            "I'll keep an eye on it.",
        ),
        (
            "Great question, the answer is Henesys.",
            "The answer is Henesys.",
        ),
        ("Absolutely, go left.", "Go left."),
        ("Of course, pot.", "Pot."),
        ("In summary, you're bad.", "You're bad."),
        ("Certainly, it's Henesys.", "It's Henesys."),
        ("בהחלט, לך שמאלה.", "לך שמאלה."),
        ("לסיכום, אתה גרוע.", "אתה גרוע."),
        // …and stays when a word or two of address does: it was the joke.
        ("Sure thing, boss.", "Sure thing, boss."),
        ("Of course, genius. Pot.", "Of course, genius. Pot."),
        (
            "Good question, dumbass. It's Henesys.",
            "Good question, dumbass. It's Henesys.",
        ),
        ("Of course, you idiot.", "Of course, you idiot."),
        ("Certainly, captain obvious.", "Certainly, captain obvious."),
        ("Absolutely, my friend.", "Absolutely, my friend."),
        ("כמובן, גאון. תשתה.", "כמובן, גאון. תשתה."),
        (
            "As mentioned, the boss spawns at the top.",
            "The boss spawns at the top.",
        ),
        ("As I mentioned earlier, rebuff first.", "Rebuff first."),
        ("In summary, pot and back off.", "Pot and back off."),
        ("You're at 20. Want me to mark this spot?", "You're at 20."),
        ("Go left. Should I keep warning you?", "Go left."),
        (
            "Pot now. Want me to mark it? Should I warn you at 40?",
            "Pot now.",
        ),
        (
            "You asked where you are. Henesys, by the potion shop.",
            "Henesys, by the potion shop.",
        ),
        (
            "You're asking about your HP. It's at 20, pot.",
            "It's at 20, pot.",
        ),
        ("You asked where you are: Henesys.", "Henesys."),
        ("Note: the boss spawns at 8.", "The boss spawns at 8."),
        (
            "Tip: pot before the second phase.",
            "Pot before the second phase.",
        ),
        (
            "Pro tip: rebuff before the door.",
            "Rebuff before the door.",
        ),
        ("1. Pot now. 2. Back off.", "Pot now. Back off."),
        ("- Pot now.\n- Go left.", "Pot now. Go left."),
        ("Do this: 1. Pot. 2. Go left.", "Do this: Pot. Go left."),
        (
            "Okay, listen: 1) pot, 2) back off, 3) rebuff.",
            "Okay, listen: pot, back off, rebuff.",
        ),
        ("Pot now! ✅ You're fine. 🎉", "Pot now! You're fine."),
        ("Nice ⭐ ding!", "Nice ding!"),
        // Nothing but the assistant: better than silence.
        ("Happy to help!", "Happy to help!"),
        (
            "Let me know if you need anything else.",
            "Let me know if you need anything else.",
        ),
        ("Happy to help! Want me to mark it?", "Want me to mark it?"),
        // Hebrew.
        (
            "כבינה מלאכותית, אני לא רואה את המסך, אבל ה-HP שלך נמוך.",
            "אני לא רואה את המסך, אבל ה-HP שלך נמוך.",
        ),
        (
            "אני רק בינה מלאכותית, אבל ה-HP שלך ב-20. תשתה.",
            "ה-HP שלך ב-20. תשתה.",
        ),
        ("אני כאן כדי לעזור! ה-HP שלך ב-20.", "ה-HP שלך ב-20."),
        ("אשמח לעזור. תשתה עכשיו.", "תשתה עכשיו."),
        ("תשתה עכשיו. תרגיש חופשי לשאול עוד.", "תשתה עכשיו."),
        ("לך שמאלה. מקווה שזה עוזר!", "לך שמאלה."),
        ("שאלה מצוינת! הבוס ברמה 110.", "הבוס ברמה 110."),
        ("בהחלט! תשתה עכשיו.", "תשתה עכשיו."),
        ("כמובן! לך שמאלה.", "לך שמאלה."),
        ("לסיכום, תשתה ותתרחק.", "תשתה ותתרחק."),
        ("אתה ב-20. רוצה שאסמן את המקום?", "אתה ב-20."),
        (
            "שאלת איפה אתה. הנסיס, ליד חנות השיקויים.",
            "הנסיס, ליד חנות השיקויים.",
        ),
        ("תשתה עכשיו. יש עוד משהו שאוכל לעזור בו?", "תשתה עכשיו."),
        ("אין לי רגשות, אבל המוות הזה כאב.", "המוות הזה כאב."),
    ];

    /// What a friend on voice chat says, and says just so.
    const GAMER_TALK: &[&str] = &[
        "Pot now.",
        "I don't have a potion for you, buy some in Henesys.",
        "Sure, that'll help with the boss.",
        "You'll need help with Zakum, bring a party.",
        "Go left, the portal's there.",
        "HP's at 20, drink.",
        "Why are you still standing in that?",
        "That's the wrong map, you want Ellinia.",
        "Nice, level 60!",
        "Back off, you're getting shredded.",
        "I can't see the game right now.",
        "I'm here! You're level 57.",
        "Help me understand why you didn't pot.",
        "Of course you can solo it at 165.",
        "Note that the boss hits hard.",
        "Want a potion? Buy some.",
        "Want me to mark it?",
        "You're at 2.5 hours to level 58.",
        "Zakum? Easy. Go.",
        "It's 20 percent, not 40.",
        "Level 61 (nice) [silent]",
        "What do you mean, absolutely not.",
        "You asked for it. Zakum, now.",
        "Level? 20. Pot now.",
        "Just pot. Seriously.",
        "1v1 the boss? Bold.",
        "Lv. 165, EXP 74%.",
        "Sure thing, boss.",
        "Great question, Einstein.",
        "Of course, genius. Pot.",
        "אין לי שיקוי בשבילך, תקנה בהנסיס.",
        "בטח, זה יעזור נגד הבוס.",
        "לך שמאלה, הפורטל שם.",
        "כמובן שאתה יכול לעשות סולו לזקום.",
    ];

    #[test]
    fn the_assistant_is_taken_out_of_a_reply() {
        for (reply, expected) in ASSISTANT_SPEAK {
            assert_eq!(humanise(reply), *expected, "{reply:?}");
        }
    }

    #[test]
    fn a_friends_lines_pass_through_untouched() {
        for line in GAMER_TALK {
            assert_eq!(humanise(line), *line);
            assert_eq!(for_speech(line), *line);
        }
    }

    #[test]
    fn humanising_is_done_in_one_pass() {
        for (reply, expected) in ASSISTANT_SPEAK {
            assert_eq!(humanise(expected), *expected, "{reply:?}");
            assert_eq!(for_speech(expected), *expected, "{reply:?}");
        }
        assert_eq!(humanise(""), "");
        assert_eq!(humanise("  \n "), "");
        assert_eq!(for_speech("🎉"), "");
    }

    #[test]
    fn speech_cleanup_and_humanising_go_together() {
        assert_eq!(
            for_speech("**Great question!** You're at *20%*. Pot now 🎉"),
            "You're at 20%. Pot now"
        );
        assert_eq!(
            for_speech("Note: 1. Pot now.\n2. Go left.\n\nLet me know if you need more! 😊"),
            "Pot now. Go left."
        );
        assert_eq!(
            for_speech(
                "Certainly! Talk to Gardin. ([maplestorywiki.net](https://maplestorywiki.net/w/Azwan)) Then go right."
            ),
            "Talk to Gardin. Then go right."
        );
        assert_eq!(for_speech("- **Pot** now\n- Go left"), "Pot now Go left");
        assert!(is_offer("Want me to mark this spot?"));
        assert!(is_offer("רוצה שאסמן את המקום?"));
        assert!(!is_offer("Want me to mark this spot."));
        assert!(!is_offer("Why would I mark that?"));
    }

    #[test]
    fn the_stream_drops_the_assistant_too() {
        for piece in [1, 4, 300] {
            // A closing sign-off is never handed to the voice…
            assert_eq!(
                split(
                    "Great question! You're at 20, pot. Let me know if you need anything else.",
                    piece
                ),
                ["You're at 20, pot."],
                "{piece}"
            );
            // …nor a closing offer, which is held until the reply ends…
            assert_eq!(
                split("Certainly! Pot now. Should I keep warning you?", piece),
                ["Pot now."],
                "{piece}"
            );
            assert_eq!(
                split(
                    "You're at the Gate of the Future. Want me to mark this spot?",
                    piece
                ),
                ["You're at the Gate of the Future."],
                "{piece}"
            );
            // …and said when more follows it.
            assert_eq!(
                split(
                    "You're at 20, pot. Want me to mark this spot? It's a good one.",
                    piece
                ),
                [
                    "You're at 20, pot.",
                    "Want me to mark this spot?",
                    "It's a good one."
                ],
                "{piece}"
            );
            // Nothing but the assistant: said, rather than nothing.
            assert_eq!(
                split("I'm here to help! Let me know if you need anything.", piece),
                ["I'm here to help! Let me know if you need anything."],
                "{piece}"
            );
            assert_eq!(
                split("Want me to mark this spot?", piece),
                ["Want me to mark this spot?"]
            );
            // An announcement after an opener is still an announcement.
            assert_eq!(
                split(
                    "Sure thing, let me give you the quickest route. Farm Root Abyss.",
                    piece
                ),
                ["Farm Root Abyss."],
                "{piece}"
            );
            // An opener that was the joke is not cut to a bare "Boss.":
            // the first spoken chunk is the whole taunt.
            assert_eq!(
                split("Sure thing, boss. Pot now, you're at 20.", piece),
                ["Sure thing, boss.", "Pot now, you're at 20."],
                "{piece}"
            );
            // An announcement and an offer: the offer goes, as it would
            // from the whole reply, and the announcement is what is left.
            assert_eq!(
                split("Let me give you the route. Want me to mark it?", piece),
                ["Let me give you the route."],
                "{piece}"
            );
            assert_eq!(
                humanise("Let me give you the route. Want me to mark it?"),
                "Let me give you the route."
            );
        }
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
