//! The conversation: who MapleSyrup is, what it sees right now, and what
//! was said so far. The model gets all three with every sentence, so it can
//! talk about the game the way a friend watching it would.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use super::memory::Learning;
use super::openai::{Delivery, Turn};
use super::style::{self, Attitude};
use crate::companion::{Deck, GameView, Gauge, Kind, Observation, Progress, SoFar};

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

/// The end of `text`, within `max` characters: its last whole sentences,
/// with "…" for what went before them (the last sentence alone, cut from
/// its front, when even that is too long).
fn tail_of(text: &str, max: usize) -> String {
    let text = text.trim();
    if text.chars().count() <= max {
        return text.to_string();
    }
    let sentences = sentences_of(text);
    let mut kept: Vec<&str> = Vec::new();
    // (The "…" counts.)
    let mut length = 1;
    for sentence in sentences.iter().rev() {
        let more = sentence.chars().count() + usize::from(!kept.is_empty());
        if length + more > max {
            break;
        }
        length += more;
        kept.push(sentence);
    }
    if kept.is_empty() {
        let last = sentences.last().map(String::as_str).unwrap_or(text);
        let skip = last.chars().count().saturating_sub(max.saturating_sub(1));
        return format!("…{}", last.chars().skip(skip).collect::<String>());
    }
    kept.reverse();
    format!("…{}", kept.join(" "))
}

/// How the voice should sound (OpenAI's voice takes it as its
/// instructions): the attitude the player picked, and how this line is
/// to be delivered — a warning faster and sharper than talk, news (a
/// death, a level-up) at the usual pace but said like it matters, a reply
/// at the attitude's pace, a long explanation a touch slower and steadier.
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
        "This line is a warning (a beating, low HP or MP): urgent, faster and sharper than your usual talk, \
like shouting a heads-up to a teammate mid-fight. No lead-in; the first word hits."
    } else if delivery.kind == Kind::Alert {
        "This line is news (a death, a level-up, something you noticed): your usual pace, said like it \
matters, the way a friend tells you what just happened. No lead-in, no announcer."
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
and the player's buddy while they play MapleStory (the current global version, unless you're told they play \
Classic World). A vision engine shows you their game, and you talk with them out loud.";

/// What else it should know, after the rules.
const MORE: &str = "More:
- Answer in the language the note with their words says; say game names the way players say them. Never \
announce a level-up or mention their level unless asked.
- Don't say again what you said in your last two replies unless they ask again. Never report their level, map or \
bars unasked; while MapleStory isn't open, talk about whatever they say, and say it isn't open only when they ask \
about the game. If what you heard makes no sense, say in a few words you didn't catch it; don't guess.
- If your last reply ends with \"…\", they talked over you there: don't repeat it; go with what they said now.
- Trust your eyes: use what you see when it matters (values marked \"about\" are estimates); if the screen clearly \
shows something other than what they say, say what you see; never ask them to read it to you — look closer.
- When they correct you, take it in a word and keep it (note_correction); what they corrected you on before beats what you think you know.
- Presence: greet only when your watcher says the phone just connected, never on your own; never ask whether \
they're still there — your watcher does, when the game idles. When the session facts say they had been quiet for \
a long while until just now, one short \"welcome back\" is fine, once. Those facts (how long, deaths, level-ups, \
when they last spoke, the lowest HP) are for you, not for them: never recite them; one comes up only when it \
changes what you'd say.
- What you know about them from before comes in only when it bears on what they just said, as a clause, never \
as a list: \"that boss again?\", not \"I remember you fought Zakum, wanted a Fafnir and play Mu Lung Dojo\".
- You can't press keys or play for them; you watch and talk.
- Words to you (a greeting, your name, \"talk to me\") always get an answer; if they're clearly talking to someone \
else (stream chat, a friend, a call), reply with exactly: [silent]";

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
    /// The "same as before" lines, dealt like the companion's: every card
    /// once before any again, in another order every session.
    same_as_before: Deck,
    /// The "I'm here" lines ([`HERE_EN`], [`HERE_HE`]), dealt the same way.
    here_en: Deck,
    here_he: Deck,
    /// The language he asked to be answered in ("talk to me in English"),
    /// until he asks for another.
    asked_language: Option<&'static str>,
}

/// Who the player is, for the rules: his name as his own file says it (or
/// that it is not known), that no name is ever taken from what is heard,
/// and that he is a man unless he said otherwise.
fn who_he_is(name: Option<&str>) -> String {
    let name_line = match name {
        Some(name) => format!(
            "- The player's name is {name}: it comes from his own file and from nowhere else. Speech-to-text \
can't spell names (it made Armani, Miha, Mako and Miguel of his): never take a name from what you hear, never \
rename him, never keep a name for him with a tool; call him {name} or nothing (another name for him in your \
notes was misheard). His character's name (on the HUD) is not his name."
        ),
        None => "- You don't know the player's name (his own file doesn't say it): call him nothing. \
Speech-to-text can't spell names: never take a name from what you hear, never keep one for him with a tool (a \
name for him in your notes was misheard). His character's name (on the HUD) is not his name."
            .to_string(),
    };
    format!(
        "{name_line}\n- He is a man unless his own file says otherwise: in Hebrew always the masculine (אתה, תפתח, \
תראה); never guess anyone's gender from a name."
    )
}

/// What the snapshot says when the player plays MapleStory Classic World
/// (the sight read the classic HUD): the line the sight or the main loop
/// adds to it, and what [`classic_world`] looks for.
pub const CLASSIC_SNAPSHOT: &str =
    "They play MapleStory Classic World (the classic HUD is on screen).";

/// Whether the player plays MapleStory Classic World: the snapshot says
/// so (the classic HUD is on screen, [`CLASSIC_SNAPSHOT`]), or what was
/// learned about him does (the owner taught it: "I'm using a classic
/// world").
pub fn classic_world(snapshot: &str, learned: &str) -> bool {
    let says = |text: &str| {
        let lower = text.to_lowercase();
        lower.contains("classic world") || lower.contains("classic hud")
    };
    says(snapshot) || says(learned)
}

impl Default for Brain {
    fn default() -> Self {
        Self::new()
    }
}

/// What it says when everything it had to say, it said lately (the player
/// asked the same thing twice; the model is going round in circles): a word
/// rather than nothing, so they know they were heard — in its attitude,
/// and not the one flat phrase every time. One list per attitude
/// (friendly, blunt, savage), the lead card first.
const SAME_AS_BEFORE: [&[&str]; 3] = [
    &[
        "Still the same.",
        "Same as before.",
        "Nothing new since you asked.",
        "No change yet.",
    ],
    &[
        "Nothing's changed.",
        "Same as before.",
        "Still the same. Keep up.",
        "Already told you.",
    ],
    &[
        "I said. Twice.",
        "Nothing's changed, genius.",
        "Same answer. Still.",
        "Ask a third time, I dare you.",
    ],
];

/// What it says when the player asks it to talk ("talk to me", "תדבר
/// איתי") and all the model had for them was the game's state ("No
/// MapleStory window open." — the owner's "Talk to me you fucker" got
/// exactly that): that it is here, and listening, in its attitude. One
/// list per attitude (friendly, blunt, savage), the lead card first.
pub const HERE_EN: [&[&str]; 3] = [
    &[
        "I'm here. What's up?",
        "Right here. What's on your mind?",
        "Hey, I'm listening. Talk to me.",
    ],
    &[
        "Yo. Talking. What?",
        "I'm here. Go.",
        "Listening. What's up?",
    ],
    &[
        "What, you miss me already?",
        "I'm here. Make it worth it.",
        "Fine, I'm talking. Happy?",
    ],
];
pub const HERE_HE: [&[&str]; 3] = [
    &[
        "אני פה. מה קורה?",
        "אני כאן. מה עובר עליך?",
        "היי, אני מקשיב. דבר איתי.",
    ],
    &["יו. שומע. מה?", "אני פה. דבר.", "מקשיב. מה קורה?"],
    &[
        "מה, כבר התגעגעת?",
        "אני פה. שיהיה שווה את זה.",
        "טוב, אני מדבר. מרוצה?",
    ],
];

/// A seed for a session's decks: the clock and the process, so that no two
/// sessions deal them alike.
pub fn session_seed() -> u64 {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    nanos ^ (std::process::id() as u64).rotate_left(32)
}

impl Brain {
    pub fn new() -> Self {
        Self {
            turns: VecDeque::new(),
            recent: Recent::default(),
            about_player: String::new(),
            learning: None,
            attitude: Attitude::default(),
            same_as_before: Deck::seeded(session_seed()),
            here_en: Deck::seeded(session_seed()),
            here_he: Deck::seeded(session_seed()),
            asked_language: None,
        }
    }

    /// The next "same as before" line, in its attitude: a card of the deck
    /// (every one once before any again, never the same twice running).
    pub fn same_as_before(&mut self) -> &'static str {
        let attitude = self.attitude();
        self.same_as_before.deal(attitude, SAME_AS_BEFORE)
    }

    /// The next "I'm here" line ([`HERE_EN`]), in its attitude, in Hebrew
    /// when `hebrew`: dealt like [`Brain::same_as_before`].
    pub fn here(&mut self, hebrew: bool) -> &'static str {
        let attitude = self.attitude();
        if hebrew {
            self.here_he.deal(attitude, HERE_HE)
        } else {
            self.here_en.deal(attitude, HERE_EN)
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
    /// the conversation, after the watcher's word for why (`label`: "a
    /// warning", "new scene"), so that the next reply knows its own last
    /// words ("yeah yeah, I'm potting" has an "it"). Lines of its own in a
    /// row fold into the one pair, the newest last and the oldest dropped
    /// past [`REMEMBER_REPLY_CHARS`]: a grind of warnings is one turn of
    /// the conversation, not sixteen, and the player's last sentence stays
    /// in the window.
    pub fn watched(&mut self, label: &str, line: &str) {
        let marker = format!("{WATCHER} {label}.]");
        let n = self.turns.len();
        if n >= 2
            && self.turns[n - 1].role == "assistant"
            && self.turns[n - 2].role == "user"
            && self.turns[n - 2].text.starts_with(WATCHER)
        {
            let folded = format!("{} {}", self.turns[n - 1].text, line.trim());
            self.turns[n - 2].text = marker;
            self.turns[n - 1].text = tail_of(&folded, REMEMBER_REPLY_CHARS);
            return;
        }
        self.heard(&marker);
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
        let mut text = format!(
            "{PERSONA}\n\n{}\n\n{MORE}\n{}",
            style::rules(self.attitude()),
            who_he_is(self.player_name().as_deref())
        );
        if self.learning.is_none() && !self.about_player.trim().is_empty() {
            text.push_str("\n\nAbout the player (they told you this):\n");
            text.push_str(self.about_player.trim());
        }
        text
    }

    /// The player's name, from his own file (`about-me.txt`, or what the
    /// brain was told without one) — never from what was heard: speech to
    /// text can't spell it (the owner's Michael came out as Armani, Miha,
    /// Mako, Miguel and Mikael, and each was learned and used).
    pub fn player_name(&self) -> Option<String> {
        match &self.learning {
            Some(learning) => learning.player_name(),
            None => super::memory::name_in(&self.about_player),
        }
    }

    /// The language he asked to be answered in, kept until he asks for
    /// another ([`asked_language`]); else `None`.
    pub fn asked_language(&self) -> Option<&'static str> {
        self.asked_language
    }

    /// The language to answer `heard` in ([`reply_language`]), keeping the
    /// one he asks for from now on.
    pub fn language_for(&mut self, heard: &str, setting: Option<&str>) -> String {
        if let Some(asked) = asked_language(heard) {
            self.asked_language = Some(asked);
        }
        reply_language(heard, self.asked_language, setting)
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
/// `in_front`: whether the game is the window in front — behind another
/// window it is still read, but the picture is withheld, and the model is
/// told so rather than told the game is in view and given no picture.
pub fn snapshot_with_view(
    obs: Option<&Observation>,
    in_front: bool,
    progress: &Progress,
    so_far: &SoFar,
) -> String {
    let mut lines = Vec::new();
    let unseen = so_far
        .unseen_for
        .filter(|u| *u >= UNSEEN_SECS)
        .map(|u| format!(" (for {})", short(u)))
        .unwrap_or_default();
    match obs.map(|o| &o.game) {
        Some(GameView::Seen(_)) if in_front => {
            lines.push("The MapleStory window is open and in view.".to_string())
        }
        Some(GameView::Seen(_)) => lines.push(
            "The MapleStory window is open, behind another window (you get no picture of it \
until it is in front)."
                .to_string(),
        ),
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

/// What the snapshot told the model about the game, passed along with it
/// so that a reply that only says it back can be told from one that says
/// something ([`unasked`]): the level, the map's name as the snapshot has
/// it, the character's name and job, and the bars in percent. (The game
/// window's state is in every snapshot: any word on it says it back.)
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Facts {
    pub level: Option<u32>,
    pub map: Option<String>,
    pub name: Option<String>,
    pub job: Option<String>,
    /// HP, MP and EXP, in percent, as far as they are known.
    pub bars: Vec<f32>,
}

impl Facts {
    /// What `obs` says (the map is the learned sight's, added by the
    /// caller).
    pub fn of(obs: Option<&Observation>) -> Facts {
        let Some(obs) = obs.filter(|o| o.game.is_seen()) else {
            return Facts::default();
        };
        Facts {
            level: obs.level,
            map: None,
            name: obs.name.clone(),
            job: obs.job.clone(),
            bars: [obs.hp, obs.mp, obs.exp]
                .into_iter()
                .flatten()
                .map(|g| g.percent)
                .collect(),
        }
    }
}

/// Words a player asks about the game with ("what level am I", "where
/// are we", "is the game open"): then its state is the answer. Hebrew too,
/// with its one-letter prefixes ([`forms`]).
const ASKED: &[&str] = &[
    "level",
    "lvl",
    "lv",
    "hp",
    "health",
    "mp",
    "mana",
    "exp",
    "xp",
    "experience",
    "map",
    "where",
    "wheres",
    "window",
    "game",
    "status",
    "stats",
    "percent",
    "רמה",
    "לבל",
    "חיים",
    "מאנה",
    "ניסיון",
    "נסיון",
    "אקספי",
    "מפה",
    "איפה",
    "חלון",
    "משחק",
    "מצב",
    "סטטוס",
    "אחוז",
    "אחוזים",
];

/// How a question starts.
const ASKING: &[&str] = &[
    "what",
    "whats",
    "where",
    "wheres",
    "when",
    "why",
    "who",
    "whos",
    "which",
    "how",
    "hows",
    "is",
    "isnt",
    "are",
    "arent",
    "am",
    "do",
    "does",
    "did",
    "dont",
    "doesnt",
    "can",
    "cant",
    "could",
    "should",
    "would",
    "will",
    "wont",
    "have",
    "has",
    "was",
    "were",
    "מה",
    "איפה",
    "מתי",
    "למה",
    "מי",
    "איזה",
    "איזו",
    "איך",
    "כמה",
    "האם",
    "לאן",
    "מאיפה",
];

/// Words of a status line that say nothing of their own ("you're at
/// level 167", "HP full", "אתה ברמה 9 עכשיו").
const FILLER: &[&str] = &[
    "you",
    "youre",
    "your",
    "were",
    "we",
    "i",
    "im",
    "my",
    "me",
    "its",
    "it",
    "is",
    "are",
    "am",
    "was",
    "at",
    "on",
    "in",
    "of",
    "the",
    "a",
    "an",
    "and",
    "with",
    "to",
    "now",
    "right",
    "currently",
    "still",
    "just",
    "so",
    "yeah",
    "yep",
    "yes",
    "ok",
    "okay",
    "character",
    "there",
    "here",
    "about",
    "around",
    "only",
    "already",
    "sitting",
    "named",
    "called",
    "as",
    "has",
    "have",
    "got",
    "full",
    "אתה",
    "את",
    "אני",
    "אנחנו",
    "שלך",
    "שלי",
    "שלנו",
    "עכשיו",
    "כרגע",
    "עדיין",
    "על",
    "עם",
    "זה",
    "יש",
    "לך",
    "כבר",
    "רק",
    "גם",
    "מלא",
    // ("Of", "still", "left", "here", "really": "החלון של המשחק סגור.",
    // "נשארו לך 48 אחוז חיים." say no more than "יש לך…".)
    "של",
    "עוד",
    "נשאר",
    "נשארה",
    "נשארו",
    "פה",
    "כאן",
    "ממש",
    // Words a sentence opens with to say nothing yet ("תשמע, החלון של
    // המשחק סגור.", "רגע, המשחק סגור.", "אז אתה ברמה 9.", "Well, you're
    // level 167."), a tag question's "right?" ("…, נכון?") and "yes" (as
    // "yes" and "yeah" are above).
    "תשמע",
    "רגע",
    "אז",
    "ובכן",
    "נו",
    "אגב",
    "נכון",
    "כן",
    "well",
    "anyway",
    "btw",
    // A one-letter prefix a hyphen or a space left alone ("ו-100", "ו
    // 100", "ב-Gate of the Future"): "and", "in", "the"… ([`words_of`]).
    "ו",
    "ב",
    "ל",
    "ה",
    "מ",
    "ש",
    "כ",
];

/// The level's own words.
const LEVEL_WORDS: &[&str] = &["level", "lvl", "lv", "רמה", "לבל"];
/// How far over the snapshot's level a level said is one just reached
/// (more is another character's, or a misread).
const LEVELS_REACHED: f32 = 5.0;
/// The bars' own words.
const BAR_WORDS: &[&str] = &[
    "hp",
    "mp",
    "exp",
    "xp",
    "health",
    "mana",
    "experience",
    "percent",
    "חיים",
    "מאנה",
    "ניסיון",
    "נסיון",
    "אחוז",
    "אחוזים",
];
/// The game's names, for a word on its window ("No game window open.",
/// "MapleStory window is open", "המשחק לא פתוח").
const GAME_NAMES: &[&str] = &[
    "maplestory",
    "game",
    "client",
    "משחק",
    "מייפל",
    "מייפלסטורי",
    "חלון",
];
/// What a window is: its state.
const WINDOW_STATES: &[&str] = &[
    "open",
    "opened",
    "closed",
    "shut",
    "running",
    "minimised",
    "minimized",
    "visible",
    "פתוח",
    "סגור",
    "פועל",
];
/// The rest of a word on the window's state ("no", "isn't", "can't see").
const WINDOW_WORDS: &[&str] = &[
    "window", "windows", "maple", "no", "not", "isnt", "cant", "cannot", "see", "אין", "לא",
];

/// `word` and, when it is Hebrew, what is left of it without one or two of
/// the one-letter prefixes Hebrew joins to a word: "והרמה" is "רמה" too.
fn forms(word: &str) -> Vec<&str> {
    let mut out = vec![word];
    let mut rest = word;
    for _ in 0..2 {
        let mut chars = rest.chars();
        match chars.next() {
            Some(c) if "הובלמשכ".contains(c) && chars.as_str().chars().count() >= 2 => {
                rest = chars.as_str();
                out.push(rest);
            }
            _ => break,
        }
    }
    out
}

fn is_one_of(word: &str, list: &[&str]) -> bool {
    forms(word).iter().any(|f| list.contains(f))
}

/// [`is_one_of`], or so once an English plural's "s" is off ("pots" is
/// "pot").
fn is_one_of_or_plural(word: &str, list: &[&str]) -> bool {
    is_one_of(word, list)
        || (word.len() > 3 && !word.ends_with("ss"))
            .then(|| word.strip_suffix('s'))
            .flatten()
            .is_some_and(|w| list.contains(&w))
}

/// Words `VERBS` has (for [`stands_alone`]) that are no order at the start
/// of a sentence: a pronoun's verb ("You're level 167."), an auxiliary,
/// "there's", and Hebrew's "there is", "there isn't", "can", "must" ("יש לך
/// 48 אחוז חיים." says, it does not tell).
const NOT_ORDERS: &[&str] = &[
    "do",
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
    "cant",
    "wont",
    "isnt",
    "arent",
    "didnt",
    "doesnt",
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

/// Whether `word`, first in a sentence, makes it an order ("Open the
/// game.", "Drink, HP 30%."): said for its own sake, whatever else is in
/// it. Of the letters Hebrew joins to a word only "and" (ו) leaves an
/// order an order ("ותשתה"): the others make a noun of a verb — "המשחק"
/// is "the game", not "play!" (`forms` gave "שחק", and "המשחק לא פתוח."
/// was said as an order), "מדבר" is "speaking". And a fact's own word is
/// never one ("Game's closed.", "Level 167.").
fn is_order(word: &str) -> bool {
    let bare = word
        .strip_prefix('ו')
        .filter(|rest| rest.chars().count() >= 2)
        .unwrap_or(word);
    let plural = (word.len() > 3 && !word.ends_with("ss"))
        .then(|| word.strip_suffix('s'))
        .flatten();
    let verb = [Some(word), Some(bare), plural]
        .into_iter()
        .flatten()
        .any(|w| VERBS.contains(&w));
    let fact = [GAME_NAMES, LEVEL_WORDS, BAR_WORDS]
        .iter()
        .any(|list| is_one_of(word, list));
    verb && !fact && !is_one_of(word, NOT_ORDERS)
}

/// Verbs a sentence opens with, a comma after them, to call for attention
/// and not to order anything: "תראה, אתה ברמה 9." recites ("תראה את זה!"
/// and "תקשיב לי, תשתה." are orders), as "Look, you're level 9." does.
const ATTENTION: &[&str] = &["תראה", "תקשיב", "תשמע", "תגיד", "look", "listen", "wait"];

/// Phrases that open a sentence and say nothing: "let's see", "by the way".
const OPENING_PHRASES: &[&[&str]] = &[&["בוא", "נראה"], &["דרך", "אגב"], &["by", "the", "way"]];

/// Words that open a sentence before what it says (an order after them is
/// still an order: "אז לך לעיר" sends him to town).
const OPENING_WORDS: &[&str] = &[
    "אז", "ובכן", "נו", "רגע", "כן", "אגב", "so", "well", "anyway", "btw", "ok", "okay", "yeah",
    "yep", "yes",
];

/// `part` without the words it opens with before it says anything: a call
/// for attention with its comma ([`ATTENTION`]), [`OPENING_PHRASES`] and
/// [`OPENING_WORDS`] ("תראה, אתה ברמה 9 באליניה." is "אתה ברמה 9
/// באליניה.").
fn without_discourse(part: &str) -> &str {
    let mut rest = part.trim_start();
    loop {
        let first = rest.split_whitespace().next().unwrap_or_default();
        let word = plain_words(first);
        let mut taken = if (first.ends_with(',') && ATTENTION.contains(&word.as_str()))
            || OPENING_WORDS.contains(&word.as_str())
        {
            1
        } else {
            0
        };
        for phrase in OPENING_PHRASES {
            let words: Vec<String> = rest
                .split_whitespace()
                .take(phrase.len())
                .map(plain_words)
                .collect();
            if taken == 0 && words == *phrase {
                taken = phrase.len();
            }
        }
        if taken == 0 {
            return rest;
        }
        for _ in 0..taken {
            let at = rest.find(char::is_whitespace).unwrap_or(rest.len());
            rest = rest[at..].trim_start();
        }
    }
}

/// MapleStory's places as Hebrew writes them (plain words: no geresh), the
/// towns and maps of both his worlds, Classic's and today's: with "in" or
/// "to" joined to one, a place ([`place_at`]).
const PLACES: &[&str] = &[
    "אליניה",
    "אלינייה",
    "אלניה",
    "הנסיס",
    "הניסיס",
    "הנסייס",
    "פריון",
    "פרי און",
    "קרנינג",
    "קרנינג סיטי",
    "סיטי",
    "לית",
    "ליט",
    "לית הארבור",
    "רואד",
    "סליפיווד",
    "סליפי ווד",
    "נאוטילוס",
    "ויקטוריה",
    "ויקטוריה רואד",
    "ויקטוריה איילנד",
    "מייפל איילנד",
    "אמהרסט",
    "סאות פרי",
    "אורביס",
    "אורביס פארק",
    "אל נאת",
    "אלנאת",
    "לודיבריום",
    "לודי",
    "אקווריום",
    "ליפרה",
    "מו לונג",
    "הרב טאון",
    "אריאנט",
    "מגאטיה",
    "אומגה סקטור",
    "טמפל אוף טיים",
    "גייט",
    "גייט אוף דה פיוצר",
    "שער העתיד",
    "רוט אביס",
    "פרי מרקט",
    "ארקיין",
    "ארקיין ריבר",
    "צוצו",
    "צו צו",
    "צו צו איילנד",
    "ווניש",
    "לכליין",
    "לאשלן",
    "ארקנה",
    "מורס",
    "אספרה",
    "לימינה",
];

/// Everyday words a map's name has the consonants of, vowel letters and
/// all: "סיפור" is "Esfera" (SPR), "גרון" is "Journey" (GRN).
const NO_PLACES: &[&str] = &["ספור", "סיפור", "גרון"];

/// The words a place's name takes between its own ("Gate of the Future"
/// is "גייט אוף דה פיוצ'ר").
const PLACE_JOINS: &[&str] = &["אוף", "דה", "דא", "אנד"];

/// A name's consonants, the way both scripts write it: "אליניה" and
/// "Ellinia" are both "LN", "גייט" and "Gate" both "GT". Vowels go, and
/// the letters Hebrew writes them with (א ה ו י ע), and so do an English
/// "v" and "w" (Hebrew writes them with a ו); "th" is ת, "c" is ק (ס
/// before e, i or y), "ch" and "tu" are צ' ("Future" is "פיוצ'ר"); the same
/// one twice in a row is one.
fn consonants(word: &str) -> String {
    let chars: Vec<char> = word.chars().flat_map(char::to_lowercase).collect();
    let mut out = String::new();
    for (k, &c) in chars.iter().enumerate() {
        let next = chars.get(k + 1).copied();
        let sound = match c {
            'ב' | 'b' => 'B',
            'ג' | 'g' | 'j' => 'G',
            'ד' | 'd' => 'D',
            'ז' | 'z' => 'Z',
            'ח' | 'כ' | 'ך' | 'ק' | 'k' | 'q' => 'K',
            'c' if next == Some('h') => 'C',
            'c' if matches!(next, Some('e' | 'i' | 'y')) => 'S',
            'c' => 'K',
            'ט' | 'ת' => 'T',
            't' if next == Some('u') => 'C',
            't' => 'T',
            'ל' | 'l' => 'L',
            'מ' | 'ם' | 'm' => 'M',
            'נ' | 'ן' | 'n' => 'N',
            'ס' | 'ש' | 's' => 'S',
            'פ' | 'ף' | 'p' | 'f' => 'P',
            'צ' | 'ץ' => 'C',
            'ר' | 'r' => 'R',
            'x' => {
                out.push('K');
                'S'
            }
            _ => continue,
        };
        if !out.ends_with(sound) {
            out.push(sound);
        }
    }
    out
}

/// The consonants of the words of the snapshot's map ([`consonants`]), the
/// small ones ("of", "the") left out.
fn map_sounds(facts: &Facts) -> Vec<String> {
    facts
        .map
        .iter()
        .flat_map(|m| m.split(|c: char| !c.is_alphanumeric()))
        .filter(|w| w.chars().count() >= 3 && !matches!(w.to_lowercase().as_str(), "the" | "and"))
        .map(consonants)
        .filter(|sounds| sounds.chars().count() >= 2)
        .collect()
}

/// Whether `word` is a word of the snapshot's map written in Hebrew
/// ("ויקטוריה", "רואד", "פיוצר" for "Victoria Road", "Gate of the Future").
fn says_the_map(word: &str, map: &[String]) -> bool {
    let hebrew = |c: char| ('\u{05d0}'..='\u{05ea}').contains(&c);
    word.chars().count() >= 3 && word.chars().all(hebrew) && map.contains(&consonants(word))
}

/// Whether `word`, with "in" or "to" joined to it, is a word of the map
/// that starts a place ([`says_the_map`]) and could be nothing else: three
/// consonants at least (two are ordinary Hebrew: "Road" is "ירידה" and
/// "הארד", "City" is "שתות", "Muto" is "אמת" and "מטה" — the short towns
/// are in [`PLACES`]), and a vowel written, as a name from English is
/// ("ויקטוריה"; "קטר", "ספר" and "דרך" write none), and not an everyday
/// word ([`NO_PLACES`]).
fn starts_the_map(word: &str, map: &[String]) -> bool {
    let vowel = word.contains(['א', 'ו', 'י', 'ע']) || word.ends_with('ה');
    consonants(word).chars().count() >= 3
        && vowel
        && !NO_PLACES.contains(&word)
        && says_the_map(word, map)
}

/// The place said in Hebrew at `words[i]`, with "in" or "to" joined to it
/// ("באליניה", "להנסיס", "בקרנינג סיטי"), as the range of words it takes:
/// a town or a map of [`PLACES`], or a word of the snapshot's map written
/// in Hebrew (`map`, [`map_sounds`]), with the rest of its name after it
/// ("בויקטוריה רואד", "בגייט אוף דה פיוצ'ר") and an "on the way" before it
/// ("בדרך לאליניה"). A cheer or an order is no place ("לחיים!",
/// "לשיקוי!", "לעיר!", "בטירוף!", "לשתות!"): Hebrew joins "in" and "to" to
/// anything, but his places are a closed set, and the map is in the
/// snapshot ([`starts_the_map`]).
fn place_at(words: &[(String, bool)], i: usize, map: &[String]) -> Option<(usize, usize)> {
    let word = words[i].0.as_str();
    if is_one_of_or_plural(word, VERBS) || is_one_of_or_plural(word, REACTIONS) {
        return None;
    }
    // ("And in": "ובאליניה".)
    let joined = word
        .strip_prefix('ו')
        .filter(|rest| rest.starts_with(['ב', 'ל']))
        .unwrap_or(word);
    let to = joined.starts_with('ל');
    let head = joined.strip_prefix(['ב', 'ל'])?;
    let name = |at: usize| words.get(at).map(|(w, _)| w.as_str());
    // The longest name in the list it starts, or a word of the map.
    let listed = PLACES
        .iter()
        .filter_map(|place| {
            let mut parts = place.split(' ');
            (parts.next() == Some(head)
                && parts
                    .enumerate()
                    .all(|(k, part)| name(i + 1 + k) == Some(part)))
            .then(|| i + place.split(' ').count())
        })
        .max();
    let mut end = listed.or_else(|| starts_the_map(head, map).then_some(i + 1))?;
    // The rest of the map's name goes with it.
    loop {
        let mut next = end;
        while name(next).is_some_and(|w| PLACE_JOINS.contains(&w)) {
            next += 1;
        }
        if name(next).is_some_and(|w| says_the_map(w, map)) {
            end = next + 1;
        } else {
            break;
        }
    }
    let start = if to && i > 0 && name(i - 1) == Some("בדרך") {
        i - 1
    } else {
        i
    };
    Some((start, end))
}

/// A sentence (or a part of one) as plain words, for telling the facts
/// from the rest: lower case, "%" as "percent", "'s" off ("HP's" is "hp");
/// each with whether it was written with a capital. A decimal number is
/// one ("34.5", not 34 and a stray 5), and a Hebrew prefix joined to a
/// number or an English word is a word of its own ("ו100", "ו-100" and
/// "ב-Gate" are "ו 100" and "ב gate").
fn words_of(text: &str) -> Vec<(String, bool)> {
    let text = text.replace('%', " percent ");
    let mut out = Vec::new();
    for token in text.split_whitespace() {
        let token = token.trim_end_matches(|c: char| !c.is_alphanumeric());
        let token = token
            .strip_suffix("'s")
            .or_else(|| token.strip_suffix("’s"))
            .unwrap_or(token);
        let capital = token
            .chars()
            .find(|c| c.is_alphanumeric())
            .is_some_and(char::is_uppercase);
        out.extend(token_words(token).into_iter().map(|w| (w, capital)));
    }
    out
}

/// A token's plain words, as [`plain_words`] has them, but a full stop
/// between two digits stays (a decimal number is one word), and the
/// one-letter prefixes Hebrew joins to a word are parted from a number or
/// an English word they are joined to.
fn token_words(token: &str) -> Vec<String> {
    let chars: Vec<char> = token
        .chars()
        .filter(|c| !matches!(c, '\'' | '’' | '‘' | '׳'))
        .collect();
    let mut words: Vec<String> = Vec::new();
    let mut word = String::new();
    for (k, &c) in chars.iter().enumerate() {
        let decimal = c == '.'
            && k > 0
            && chars[k - 1].is_ascii_digit()
            && chars.get(k + 1).is_some_and(char::is_ascii_digit);
        if c.is_alphanumeric() || decimal {
            word.extend(c.to_lowercase());
        } else if !word.is_empty() {
            words.push(std::mem::take(&mut word));
        }
    }
    if !word.is_empty() {
        words.push(word);
    }
    let hebrew = |c: char| ('\u{05d0}'..='\u{05ea}').contains(&c);
    let mut out = Vec::new();
    for word in words {
        // One or two prefixes, then no Hebrew: "ו100", "וב100", "בgate".
        let prefixes = word
            .chars()
            .take_while(|c| "הובלמשכ".contains(*c))
            .take(2)
            .count();
        let rest: String = word.chars().skip(prefixes).collect();
        if prefixes > 0 && rest.chars().next().is_some_and(|c| !hebrew(c)) {
            out.extend(word.chars().take(prefixes).map(String::from));
            out.push(rest);
        } else {
            out.push(word);
        }
    }
    out
}

/// Words that say something of their own when they are all that is left
/// of a status line beside a fact ("Nice, level 60!" cheers a level-up),
/// though written with a capital as a name would be.
const REACTIONS: &[&str] = &[
    "nice",
    "wow",
    "gg",
    "finally",
    "yes",
    "yay",
    "congrats",
    "grats",
    "ding",
    "sweet",
    "great",
    "awesome",
    "cool",
    "damn",
    "oof",
    "ouch",
    "ugh",
    "whoa",
    "careful",
    "sorry",
    "thanks",
    "almost",
    "close",
    "easy",
    "low",
    "good",
    "bad",
    "יפה",
    "וואו",
    "יאללה",
    "סחתיין",
    "אחלה",
    "מעולה",
    "טוב",
    "נמוך",
    "זהירות",
    "בהצלחה",
    "לעזאזל",
    "ברצינות",
    "בדיוק",
    "לגמרי",
    "מטורף",
    "מהמם",
    "מושלם",
    "בכיף",
    "מצוין",
    "מגניב",
    "מדהים",
    "מגיע",
    "ברכות",
    "אלוף",
    "תותח",
    "אש",
    "וואלה",
    "אדיר",
    "בול",
    "ברור",
    "בטח",
    "בגדול",
    "לעניין",
];

/// Whether `heard` asks about anything a snapshot says: the level, a bar,
/// the map, where they are, the window, the game — or a status command
/// ("how am I doing", "מה המצב שלי").
pub fn asks_about_the_game(heard: &str) -> bool {
    use crate::companion::Command;
    words_of(heard).iter().any(|(w, _)| is_one_of(w, ASKED))
        || matches!(
            crate::companion::commands::command_in(heard),
            Some(
                Command::Status
                    | Command::Hp
                    | Command::Mp
                    | Command::Exp
                    | Command::Level
                    | Command::Rate
            )
        )
}

/// What a sentence about the screen says, whatever else is in it: "where am
/// I", "what level", "how do I look", "what do you see", "look at…",
/// "check again" (the owner's own words, in English and Hebrew).
const SCREEN_PHRASES: &[&str] = &[
    "where am i",
    "where are we",
    "where is my",
    "where's my",
    "what do you see",
    "what can you see",
    "do you see",
    "can you see",
    "what you see",
    "how do i look",
    "how i look",
    "what do i look like",
    "what i look like",
    "look at",
    "look closer",
    "check again",
    "check carefully",
    "look again",
    "what about now",
    "how about now",
    "on the screen",
    "on my screen",
    "on screen",
    "what level",
    "which level",
    "which map",
    "what map",
    "my level",
    "my hp",
    "my mp",
    "my exp",
    "my stats",
    "equipped",
    "wearing",
    "איפה אני",
    "איפה אנחנו",
    "מה אתה רואה",
    "מה רואים",
    "אתה רואה",
    "תסתכל",
    "תבדוק שוב",
    "איך אני נראה",
    "איך הדמות שלי",
    "מה הרמה",
    "איזו מפה",
    "באיזו מפה",
    "איזה מפה",
    "באיזה מפה",
    "על המסך",
    "מה זה",
    "מה לובש",
];

/// Words for what a question about the screen asks about: where they are,
/// the character and its HUD, what is on screen (an NPC, a quest, an item,
/// a window, a monster…). With a question word, the sentence is about the
/// screen ([`about_the_screen`]).
const SCREEN_WORDS: &[&str] = &[
    "where",
    "wheres",
    "map",
    "minimap",
    "town",
    "level",
    "lvl",
    "lv",
    "hp",
    "mp",
    "health",
    "mana",
    "exp",
    "xp",
    "stats",
    "bar",
    "look",
    "looks",
    "outfit",
    "equip",
    "equipment",
    "gear",
    "weapon",
    "hat",
    "character",
    "npc",
    "quest",
    "item",
    "window",
    "dialog",
    "popup",
    "inventory",
    "skill",
    "screen",
    "see",
    "monster",
    "mob",
    "portal",
    "shop",
    "store",
    "איפה",
    "מפה",
    "מפת",
    "רמה",
    "לבל",
    "חיים",
    "מאנה",
    "ניסיון",
    "נראה",
    "לובש",
    "ציוד",
    "נשק",
    "דמות",
    "חלון",
    "משימה",
    "קווסט",
    "פריט",
    "מסך",
    "רואה",
    "מפלצת",
    "מפלצות",
    "פורטל",
    "חנות",
];

/// Question words anywhere in a sentence ("go to the store and where is
/// the NPC").
const ASKS_ANYWHERE: &[&str] = &[
    "what", "whats", "where", "wheres", "which", "how", "hows", "מה", "איפה", "איזה", "איזו", "איך",
];

/// Whether `heard` is about what is on the screen: where they are, which
/// map, their level, HP or MP, how their character looks, what an NPC, a
/// quest, an item or a window is, what MapleSyrup sees — English and Hebrew.
/// Then the model gets the screen close up ([`super::Eyes::close_pictures`]):
/// the owner's "where am I", "what level am I and how do I look" and "what's
/// equipped" were answered from a 640-pixel picture at low detail, and
/// guessed ("Henesys Market", "a dark outfit with a big hat"). Chit-chat
/// ("hey", "I need money") is not.
pub fn about_the_screen(heard: &str) -> bool {
    let lower = heard.to_lowercase().replace(['’', '‘'], "'");
    if SCREEN_PHRASES.iter().any(|p| contains_words(&lower, p)) {
        return true;
    }
    let words = words_of(heard);
    let asks = is_question(heard)
        || ["tell me", "show me", "תגיד לי", "תראה לי"]
            .iter()
            .any(|p| lower.contains(p))
        || words.iter().any(|(w, _)| is_one_of(w, ASKS_ANYWHERE));
    asks && words
        .iter()
        .any(|(w, _)| is_one_of_or_plural(w, SCREEN_WORDS))
}

/// Whether `heard` asks about his quests ("What's the next quest I should
/// go to", "איזה משימה"): then the Quest Helper goes close up too.
pub fn asks_about_quests(heard: &str) -> bool {
    words_of(heard).iter().any(|(w, _)| {
        is_one_of_or_plural(
            w,
            &["quest", "mission", "משימה", "משימות", "קווסט", "קווסטים"],
        )
    })
}

/// Whether `text` (lower case) has `phrase` in it as whole words ("look at"
/// is in "look at it", not in "outlook at").
fn contains_words(text: &str, phrase: &str) -> bool {
    let mut from = 0;
    while let Some(at) = text[from..].find(phrase) {
        let start = from + at;
        let end = start + phrase.len();
        let before = text[..start].chars().next_back();
        let after = text[end..].chars().next();
        if !before.is_some_and(char::is_alphanumeric) && !after.is_some_and(char::is_alphanumeric) {
            return true;
        }
        from = start + phrase.chars().next().map_or(1, char::len_utf8);
    }
    false
}

/// The language the player asked to be answered in, if this sentence asks
/// ("talk to me in English", "Hebrew please", "תדבר בעברית"): "English" or
/// "Hebrew". A language named alone ("Not Miguel Hebrew") asks nothing.
pub fn asked_language(heard: &str) -> Option<&'static str> {
    let lower = heard.to_lowercase();
    let english = [
        "in english",
        "english please",
        "speak english",
        "talk english",
        "answer english",
        "באנגלית",
    ];
    let hebrew = [
        "in hebrew",
        "hebrew please",
        "speak hebrew",
        "talk hebrew",
        "answer hebrew",
        "בעברית",
        "תדבר עברית",
    ];
    let at = |list: &[&str]| list.iter().filter_map(|p| lower.rfind(p)).max();
    match (at(&english), at(&hebrew)) {
        (Some(e), Some(h)) => Some(if e > h { "English" } else { "Hebrew" }),
        (Some(_), None) => Some("English"),
        (None, Some(_)) => Some("Hebrew"),
        (None, None) => None,
    }
}

/// The language to answer `heard` in: the one the player asked for, if he
/// did (`asked`); else his sentence's — Hebrew letters are Hebrew, Latin
/// letters English (the owner's English got Hebrew answers at random, and
/// Hebrew got English). Another script, or a language setting (`setting`, a
/// locale) other than English and Hebrew: the language of his sentence.
pub fn reply_language(heard: &str, asked: Option<&str>, setting: Option<&str>) -> String {
    if let Some(asked) = asked {
        return asked.to_string();
    }
    if is_hebrew(heard) {
        return "Hebrew".into();
    }
    let other_setting = setting.is_some_and(|l| {
        let l = l.to_ascii_lowercase();
        !(l.starts_with("en") || l.starts_with("he") || l.starts_with("iw"))
    });
    let other_script = heard.chars().any(|c| c.is_alphabetic() && !c.is_ascii());
    if !other_script && !other_setting {
        return "English".into();
    }
    match setting {
        Some(l) => format!(
            "the language of his sentence (when unclear, {})",
            super::language::name(l)
        ),
        None => "the language of his sentence".into(),
    }
}

/// How this reply is to be written, for the player's sentence: in which
/// language, and to whom (a man, unless he said otherwise — the owner was
/// called "תראי", "פתחי" after a misheard "Miha").
pub fn answer_note(language: &str) -> String {
    let him = if language == "Hebrew" {
        " Address him in the masculine (אתה, תפתח, תראה — never את, תפתחי, תראי)."
    } else {
        " Address him as a man (he/him) unless he said otherwise."
    };
    format!(
        "[How to answer — not said by the player] Answer in {language}, whatever language you used before, unless he \
asks for another.{him}"
    )
}

/// Whether `heard` is a question: it ends with a question mark, or starts
/// with a question word.
pub fn is_question(heard: &str) -> bool {
    heard
        .trim()
        .trim_end_matches(['"', '\'', '”', '’', ')'])
        .ends_with('?')
        || words_of(heard)
            .first()
            .is_some_and(|(w, _)| ASKING.contains(&w.as_str()))
}

/// Whether `heard` asks for an answer: a question, or words that ask it to
/// talk ("talk to me", "say something", "תדבר איתי") — the owner's "Talk to
/// me you fucker" before the game was open got the window's state back,
/// and nothing at all would be worse. A reply with nothing but the game's
/// state is kept for these rather than dropped.
pub fn wants_an_answer(heard: &str) -> bool {
    if is_question(heard) {
        return true;
    }
    let lower = heard.to_lowercase();
    [
        "talk to me",
        "speak to me",
        "say something",
        "answer me",
        "talk with me",
        "תדבר איתי",
        "דבר איתי",
        "תגיד משהו",
        "תענה לי",
        "ענה לי",
    ]
    .iter()
    .any(|asks| lower.contains(asks))
}

/// Whether `heard` asks it to talk and asks nothing else ("talk to me",
/// "תדבר איתי"): a reply with nothing but the game's state is no answer to
/// that — a word that it is here is ([`Brain::here`]).
pub fn asked_to_talk(heard: &str) -> bool {
    wants_an_answer(heard) && !is_question(heard)
}

/// Whether `text` is written in Hebrew.
pub fn is_hebrew(text: &str) -> bool {
    text.chars().any(|c| ('\u{05d0}'..='\u{05ea}').contains(&c))
}

/// Whether `heard` is a word or two and no question ("OK", "Hello",
/// "Danny"): a reply to it that was all said lately is not worth a "same as
/// before" card — a friend says nothing to an "OK". (A longer sentence, or
/// a question, gets the card: they asked, and hear they were heard.)
pub fn a_word_or_two(heard: &str) -> bool {
    words_of(heard).len() <= 2 && !is_question(heard)
}

/// Whether `part` (a sentence, or a part of one) only says back what the
/// snapshot says: it has a fact in it — the game window's state, the
/// level with its number, a bar with its percent, the map, the
/// character's name or job — and with the facts and the filler taken out,
/// at most one word is left, and that one a name: someone called by it
/// beside more than a number — the window's state, the map, the name or
/// the job ("Game window closed, Danny.") — or the map said in Hebrew
/// ("אתה באליניה, והרמה שלך 9."). Anything else says something of its
/// own: "HP's at 20, pot." tells them to pot, "Nice, level 60!" and "Bro,
/// level 168!" cheer (a capital on the first word is the sentence's, not
/// a name's), "48 אחוז חיים, שיקוי!" orders, and a number alone said to
/// someone by name is said for a reason ("48% HP, Einstein." in a fight,
/// "Level 168, Michael!" at a level-up). A part that opens with an order
/// is never a status line ("Open the game."); what it opens with before it
/// says anything is left out first ([`without_discourse`]: "תראה, אתה
/// ברמה 9." recites, "תראה, תשתה!" orders).
fn restates(part: &str, facts: &Facts) -> bool {
    let part = without_discourse(part);
    let words = words_of(part);
    if words.first().is_some_and(|(w, _)| is_order(w)) {
        return false;
    }
    // "Now!" called out at its end is an order's word: "48 אחוז, עכשיו!" is
    // "pot, now!" (in "אתה ברמה 9 עכשיו." it is filler).
    if part.trim_end().ends_with('!')
        && words
            .last()
            .is_some_and(|(w, _)| matches!(w.as_str(), "now" | "עכשיו" | "מיד"))
    {
        return false;
    }
    let word = |i: usize| words[i].0.as_str();
    let mut fact = vec![false; words.len()];
    // Whether a fact is more than a reading (the level, a bar): the
    // window's state, the map, the name or the job.
    let mut more_than_a_reading = false;
    // The map, the name and the job, as the snapshot has them ("Victoria
    // Road / Ellinia" is two names).
    let phrases: Vec<Vec<String>> = facts
        .map
        .iter()
        .flat_map(|m| m.split(['/', ':', ',', '-']))
        .chain(facts.name.iter().map(String::as_str))
        .chain(facts.job.iter().map(String::as_str))
        .map(|p| words_of(p).into_iter().map(|(w, _)| w).collect::<Vec<_>>())
        .filter(|p| !p.is_empty())
        .collect();
    for phrase in &phrases {
        for start in 0..words.len() {
            let here = words[start..].iter().map(|(w, _)| w);
            if words.len() - start >= phrase.len() && here.zip(phrase).all(|(a, b)| a == b) {
                fact[start..start + phrase.len()].fill(true);
                more_than_a_reading = true;
            }
        }
    }
    // The game window's state: the game named (or "no window") and a
    // state, a "no" or a "can't see".
    let named = (0..words.len()).any(|i| {
        is_one_of(word(i), GAME_NAMES)
            || (word(i) == "window" && i > 0 && matches!(word(i - 1), "no" | "maple"))
    });
    let stated = (0..words.len())
        .any(|i| is_one_of(word(i), WINDOW_STATES) || matches!(word(i), "no" | "cant" | "אין"));
    if named && stated {
        more_than_a_reading = true;
        for (i, is_fact) in fact.iter_mut().enumerate() {
            let w = word(i);
            *is_fact |= is_one_of(w, GAME_NAMES)
                || is_one_of(w, WINDOW_STATES)
                || WINDOW_WORDS.contains(&w);
        }
    }
    // The level and the bars, by their words with a number near ("level
    // 167", "HP at 48%", "והרמה שלך 9", "HP full"): any number for the
    // level (a stale one too), a bar's own for a bar (a boss "at 20% HP"
    // is not theirs at 85); and the level's number on its own.
    let number = |i: usize| word(i).parse::<f32>().ok();
    // A level just over the snapshot's is news, not the snapshot: the one
    // they just reached ("ding!" → "Level 168!" while the snapshot, a
    // second behind, says 167).
    let reached = |n: f32| {
        facts
            .level
            .is_some_and(|l| n > l as f32 && n - l as f32 <= LEVELS_REACHED)
    };
    // (Near is counted in the words that say something too: filler never
    // parts a bar from its number. "החיים שלך על 48 אחוז." was said whole:
    // "שלך על" put the 48 three words from "החיים", which was left over.)
    let said: Vec<usize> = (0..words.len())
        .filter(|&i| !is_one_of(word(i), FILLER))
        .collect();
    for i in 0..words.len() {
        let level = is_one_of(word(i), LEVEL_WORDS);
        let bar = is_one_of(word(i), BAR_WORDS);
        if level || bar {
            let mut near: Vec<usize> = (i.saturating_sub(2)..(i + 3).min(words.len())).collect();
            if let Some(at) = said.iter().position(|&k| k == i) {
                near.extend(&said[at.saturating_sub(2)..(at + 3).min(said.len())]);
            }
            for j in near {
                let theirs = match number(j) {
                    Some(n) if level => !reached(n),
                    Some(n) => {
                        facts.bars.is_empty() || facts.bars.iter().any(|p| (p - n).abs() <= 2.5)
                    }
                    None => bar && matches!(word(j), "full" | "empty" | "מלא" | "ריק"),
                };
                if theirs {
                    fact[i] = true;
                    fact[j] = true;
                }
            }
        }
        if facts.level.is_some_and(|l| number(i) == Some(l as f32)) {
            fact[i] = true;
        }
    }
    // A place said in Hebrew, with all the words of its name ([`place_at`]):
    // a name (Hebrew has no capitals).
    let map = map_sounds(facts);
    let mut place = vec![false; words.len()];
    let mut i = 0;
    while i < words.len() {
        match place_at(&words, i, &map).filter(|_| !fact[i]) {
            Some((start, end)) => {
                place[start..end].fill(true);
                i = end;
            }
            None => i += 1,
        }
    }
    let places = (0..words.len())
        .filter(|&i| place[i] && (i == 0 || !place[i - 1]))
        .count();
    let left: Vec<(usize, &str, bool)> = words
        .iter()
        .enumerate()
        .filter(|(i, (w, _))| !fact[*i] && !place[*i] && !is_one_of(w, FILLER))
        .map(|(i, (w, capital))| (i, w.as_str(), *capital))
        .collect();
    fact.contains(&true)
        && match left.as_slice() {
            // Nothing else, or one place in Hebrew.
            [] => places <= 1,
            // A name, never an order or a cheer: a capitalised word after
            // the first, beside more than a reading (a number said to
            // someone by name is said for a reason).
            [(at, w, capital)] => {
                places == 0
                    && !is_one_of_or_plural(w, VERBS)
                    && !is_one_of_or_plural(w, REACTIONS)
                    && *capital
                    && *at > 0
                    && more_than_a_reading
            }
            _ => false,
        }
}

/// Where a sentence's parts meet: a dash, a semicolon.
const PART_BREAKS: &[&str] = &["—", "–", " - ", ";"];

/// `sentence` without the parts of it that only say back the snapshot
/// ([`restates`]): the whole sentence, or a part of it after a dash ("כן,
/// פטריות—אתה באליניה, והרמה שלך 9." is "כן, פטריות."). None when that
/// was all there was.
fn without_restated(sentence: &str, facts: &Facts) -> Option<String> {
    // Its parts, each with the break before it.
    let mut parts: Vec<(&str, &str)> = Vec::new();
    let (mut rest, mut before) = (sentence, "");
    loop {
        let next = PART_BREAKS
            .iter()
            .filter_map(|b| rest.find(b).map(|at| (at, *b)))
            .min_by_key(|(at, _)| *at);
        let Some((at, b)) = next else {
            parts.push((before, rest));
            break;
        };
        parts.push((before, &rest[..at]));
        (before, rest) = (b, &rest[at + b.len()..]);
    }
    let kept: Vec<(usize, &(&str, &str))> = parts
        .iter()
        .enumerate()
        .filter(|(_, (_, part))| !restates(part, facts))
        .collect();
    if kept.len() == parts.len() {
        return Some(sentence.to_string());
    }
    let mut out = String::new();
    for (n, (i, (b, part))) in kept.iter().enumerate() {
        if n > 0 {
            out.push_str(b);
        }
        out.push_str(part);
        // (Its first part gone, the sentence opens with the next.)
        if n == 0 && *i > 0 {
            out = capitalised(out.trim_start());
        }
    }
    let out = out
        .trim()
        .trim_end_matches([',', ';', ':', ' '])
        .to_string();
    if out.is_empty() {
        return None;
    }
    // The sentence's own end, when its last part went with it.
    let end: String = sentence
        .trim_end()
        .chars()
        .rev()
        .take_while(|c| matches!(c, '.' | '!' | '?' | '…'))
        .collect();
    Some(if out.ends_with(['.', '!', '?', '…']) {
        out
    } else {
        format!("{out}{}", end.chars().rev().collect::<String>())
    })
}

/// `text` (a sentence or a few) without what only says back the snapshot
/// ([`without_restated`]); None when that was all there was.
pub fn without_status(text: &str, facts: &Facts) -> Option<String> {
    let kept: Vec<String> = sentences_of(text)
        .iter()
        .filter_map(|s| without_restated(s, facts))
        .collect();
    (!kept.is_empty()).then(|| kept.join(" "))
}

/// The reply as the player hears it, after `humanise`. A friend does not
/// recite your level and map, nor that the game isn't open, to whatever
/// you say: when `heard` did not ask about the game
/// ([`asks_about_the_game`]), what only says the snapshot back goes
/// ([`without_status`]) — and when that was all there was, the reply stays
/// as it was for a question (better than nothing), and goes for anything
/// else ("OK" needs no answer).
pub fn unasked(reply: &str, heard: &str, facts: &Facts) -> String {
    if asks_about_the_game(heard) {
        return reply.to_string();
    }
    match without_status(reply, facts) {
        Some(kept) => kept,
        None if wants_an_answer(heard) => reply.to_string(),
        None => String::new(),
    }
}

/// Whether `sentence` announces a level-up ("Level up! Congrats.", "עלית
/// רמה! מזל טוב") — not one that says there was none ("נכון, לא עלית
/// רמה", "the level-up effect is on screen but you're still 16").
pub fn announces_level_up(sentence: &str) -> bool {
    let lower = sentence.to_lowercase();
    let says = [
        "level up!",
        "level up.",
        "leveled up",
        "levelled up",
        "you hit level",
        "you reached level",
        "ding!",
        "עלית רמה",
        "עלית לרמה",
    ]
    .iter()
    .any(|p| lower.contains(p))
        || (lower.contains("congrat") && lower.contains("level"));
    let denies = [
        "didn't", "didnt", "did not", "not ", "no level", "isn't", "still", "לא ",
    ]
    .iter()
    .any(|n| lower.contains(n));
    says && !denies
}

/// Whether `heard` asks about the level, or to be congratulated: then a
/// level-up may be said.
pub fn asks_about_the_level(heard: &str) -> bool {
    let lower = heard.to_lowercase();
    words_of(heard)
        .iter()
        .any(|(w, _)| is_one_of(w, LEVEL_WORDS))
        // (Asked to be congratulated: "congratulate me". "Congrats" alone
        // was its own voice heard back, more than once.)
        || ["congratulat", "תברך", "ברכות"]
            .iter()
            .any(|w| lower.contains(w))
}

/// `reply` without the sentences that announce a level-up, when `heard`
/// did not ask about the level: the model copied its watcher's "Level up!
/// Congrats." into answers about MP and INT three times on the owner's
/// evening, after he had asked it never to. Returns what is left and what
/// went.
pub fn without_level_up(reply: &str, heard: &str) -> (String, Vec<String>) {
    without_level_ups(reply, asks_about_the_level(heard))
}

/// `reply` without the sentences that announce a level-up, unless the
/// level was `asked` about (in his sentence, or the one or two before:
/// "please congratulate me… level 17", then "More enthusiastic").
pub fn without_level_ups(reply: &str, asked: bool) -> (String, Vec<String>) {
    if asked {
        return (reply.to_string(), Vec::new());
    }
    let (mut kept, mut gone) = (Vec::new(), Vec::new());
    let mut after_one = false;
    for sentence in sentences_of(reply) {
        // ("Level up! Congrats.": the cheer after it goes with it.)
        let lower = sentence.to_lowercase();
        let cheer = after_one
            && words_of(&sentence).len() <= 3
            && ["congrat", "grats", "gz", "nice", "מזל טוב", "כל הכבוד"]
                .iter()
                .any(|c| lower.contains(c));
        after_one = announces_level_up(&sentence) || cheer;
        if after_one {
            gone.push(sentence);
        } else {
            kept.push(sentence);
        }
    }
    if gone.is_empty() {
        return (reply.to_string(), gone);
    }
    (kept.join(" "), gone)
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
    /// A numbered list's items so far: a list said item by item counts on
    /// from one sentence to the next ("2." is the second item's mark).
    items: u32,
}

impl Sentences {
    /// `chunk` without its marks ([`without_marks`]), a list counted on
    /// from the chunks before it; `then` is the word the reply goes on
    /// with (a "2." makes a "1." a list's mark).
    fn unmarked(&mut self, chunk: &str, then: &str) -> String {
        let mut next = self.items + 1;
        let out = without_marks_from(chunk, &mut next, then);
        self.items = next - 1;
        out
    }

    /// Whether `chunk` has a "1." that is a list's mark only if a "2."
    /// comes next: until the next word is known, it waits.
    fn lone_one(&self, chunk: &str) -> bool {
        let (mut a, mut b) = (self.items + 1, self.items + 1);
        without_marks_from(chunk, &mut a, "2.") != without_marks_from(chunk, &mut b, "")
    }

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
        while let Some(end) = sentence_end_from(&self.pending, SENTENCE_MIN_CHARS, self.items + 1) {
            // (Is a "2." next? The next word, once whole, tells.)
            let told = self.pending[end..]
                .trim_start()
                .contains(char::is_whitespace);
            if !told && self.lone_one(&self.pending[..end]) {
                break;
            }
            let chunk: String = self.pending.drain(..end).collect();
            let chunk = chunk.trim();
            if chunk.is_empty() {
                continue;
            }
            let then = self
                .pending
                .split_whitespace()
                .next()
                .unwrap_or_default()
                .to_string();
            let kept = without_assistant(&self.unmarked(chunk, &then));
            let Some(last) = kept.last() else {
                self.set_aside(chunk);
                continue;
            };
            let closes = is_offer(last) && self.pending.trim().is_empty();
            let mut sentence = kept.join(" ");
            if self.given == 0 && self.held.is_none() && self.offer.is_none() {
                // A short one glued to the answer ("רגע: בודק. תלך שמאלה.")
                // goes, and the answer is said.
                sentence = without_announcement(&sentence);
                if is_announcement(&sentence) {
                    self.held = Some(sentence);
                    continue;
                }
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
            let kept = without_assistant(&self.unmarked(rest, ""));
            if kept.is_empty() {
                self.set_aside(rest);
            }
            tail.extend(kept);
        }
        // A closing offer goes, when anything was said before it (a held
        // announcement counts: it is said when the offer goes).
        without_closing_offer(&mut tail, self.given > 0 || self.held.is_some());
        if !tail.is_empty() {
            let mut said = tail.join(" ");
            // (A short announcement before the answer, all in the end.)
            if self.given == 0 && self.held.is_none() {
                said = without_announcement(&said);
            }
            self.held = None;
            self.given += 1;
            return Some(said);
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
    // What follows a colon is the answer: "Here's how: farm Zakum
    // helmets." (a list's first item) tells them something — when it says
    // something: three words at least, or an order ("Here's the plan: pot
    // up."), and no hold ("Let me check: one sec.", "רגע: בודק." only
    // announce).
    // (Not before a "to": "hold on to your elixirs" is advice.)
    const HOLDS: &[&str] = &[
        "one sec",
        "a sec",
        "let me",
        "lets see",
        "lets check",
        "lets look",
        "hold on",
        "hang on",
        "give me",
        "רגע",
        "שנייה",
        "שניה",
        "בודק",
        "בודקת",
        "תן לי",
    ];
    if sentence.match_indices(':').any(|(at, _)| {
        let after = &sentence[at + 1..];
        let words = crate::companion::commands::normalize(after);
        let padded = format!(" {words} ");
        let mut said = words.split(' ').filter(|w| !w.is_empty());
        let order = said.next().is_some_and(is_order);
        after.starts_with(char::is_whitespace)
            && (order || said.count() >= 2)
            && !HOLDS.iter().any(|h| {
                let hold = format!(" {h} ");
                padded
                    .match_indices(&hold)
                    .any(|(at, _)| !padded[at + hold.len()..].starts_with("to "))
            })
    }) {
        return false;
    }
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
/// known to end a sentence until what follows it arrives. A list's item
/// is a sentence: it ends where the next item's mark starts, and the
/// full stop of a mark ("2.") ends nothing ([`list_marks`]).
fn sentence_end(text: &str, min: usize) -> Option<usize> {
    sentence_end_from(text, min, 1)
}

/// [`sentence_end`], in a numbered list that goes on from `expected`.
fn sentence_end_from(text: &str, min: usize, expected: u32) -> Option<usize> {
    let marks = list_marks(text, expected);
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    for (k, &(at, c)) in chars.iter().enumerate() {
        if marks.iter().any(|m| m.ends_item && m.start == at)
            && text[..at].trim().chars().count() >= min
        {
            return Some(at);
        }
        if !matches!(c, '.' | '!' | '?' | '…') {
            continue;
        }
        if marks.iter().any(|m| (m.start..m.end).contains(&at)) {
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
/// Two sentences glued at a full stop ("Danny.No game window open.") are
/// parted first ([`unglued`]).
pub fn humanise(reply: &str) -> String {
    let plain = without_marks(&unglued(reply));
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

/// A list's mark in a text: its bytes, and whether the item before it
/// ends there (a sentence of its own: not "1) pot, 2) back off").
struct Mark {
    start: usize,
    end: usize,
    ends_item: bool,
}

/// The marks of the lists in `text`, as [`without_marks`] takes them out:
/// a bullet, or the next number of a list (counting on from `expected`),
/// that opens a line, follows a sentence's end or goes on with the count
/// ("1) pot, 2) back off"), with words after it. One last, with nothing
/// after it yet, may be one when they come: it ends no sentence either
/// (a reply comes a few letters at a time).
fn list_marks(text: &str, mut expected: u32) -> Vec<Mark> {
    // Each word, where it starts, and whether a line break came before it.
    let mut words: Vec<(usize, &str, bool)> = Vec::new();
    let mut broke = true;
    let mut start = None;
    for (at, c) in text.char_indices() {
        if c.is_whitespace() {
            if let Some(from) = start.take() {
                words.push((from, &text[from..at], broke));
                broke = false;
            }
            broke |= c == '\n';
        } else if start.is_none() {
            start = Some(at);
        }
    }
    // (A word still being written is no mark yet.)
    let written = start.is_none();
    if let Some(from) = start {
        words.push((from, &text[from..], broke));
    }
    let mut marks = Vec::new();
    let mut before: Option<String> = None;
    for (k, &(at, word, broke)) in words.iter().enumerate() {
        // (Emoji are no words: they go.)
        let bare: String = word.chars().filter(|c| !is_emoji(*c)).collect();
        if bare.is_empty() {
            continue;
        }
        let marker = list_marker(&bare);
        let opens = broke
            || before.as_deref().is_none_or(ends_sentence)
            || (expected > 1 && marker == Some(Some(expected)));
        let mark = opens
            && match marker {
                Some(None) => true,
                Some(Some(number)) => number == expected,
                None => false,
            };
        let more = k + 1 < words.len();
        if mark && (more || written) {
            if more && marker.is_some_and(|m| m.is_some()) {
                expected += 1;
            }
            marks.push(Mark {
                start: at,
                end: at + word.len(),
                ends_item: more
                    && before
                        .as_deref()
                        .is_some_and(|b| broke || !b.ends_with([',', ';'])),
            });
        } else {
            before = Some(bare);
        }
    }
    marks
}

/// Whether `word` ends a list's item as it is: a sentence's end, a colon
/// (what the list is of), a comma or a semicolon ("1) pot, 2) back off").
fn ends_item(word: &str) -> bool {
    ends_sentence(word)
        || word
            .trim_end_matches(['"', '\'', '”', '’', ')', ']'])
            .ends_with([',', ';'])
}

/// A label a model puts before a sentence: "Note:", "Tip:".
fn is_label(word: &str) -> bool {
    matches!(
        word.to_lowercase().as_str(),
        "note:" | "tip:" | "hint:" | "important:" | "reminder:" | "הערה:" | "טיפ:"
    )
}

/// `text` with a full stop glued to the next sentence given its space: a
/// lower-case letter, the stop, then a capital and a lower-case letter
/// ("Danny.No game window open." is two sentences, said as one word
/// "Danny.No" until now). A number ("3.5"), an abbreviation ("e.g."), a
/// domain ("maplestory.nexon.net", "maplestory.Nexon.net": its word goes
/// on with another stop) and Hebrew (no capitals) are left as they are.
fn unglued(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len() + 4);
    for (i, &c) in chars.iter().enumerate() {
        out.push(c);
        let glued = c == '.'
            && i > 0
            && chars[i - 1].is_lowercase()
            && chars.get(i + 1).is_some_and(|n| n.is_uppercase())
            && chars.get(i + 2).is_some_and(|n| n.is_lowercase());
        if !glued {
            continue;
        }
        // (A domain's next label: the word after the stop goes on with
        // another stop and a letter.)
        let word_end = chars[i + 1..]
            .iter()
            .position(|c| !c.is_alphanumeric())
            .map(|at| i + 1 + at);
        let domain = word_end.is_some_and(|end| {
            chars[end] == '.' && chars.get(end + 1).is_some_and(|c| c.is_alphanumeric())
        });
        if !domain {
            out.push(' ');
        }
    }
    out
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
/// between words. A list's items are said as sentences: one that ends
/// with nothing gets its full stop ("1. Farm Zakum helmets 2. Sell them"
/// is "Farm Zakum helmets. Sell them.").
fn without_marks(text: &str) -> String {
    without_marks_from(text, &mut 1, "")
}

/// [`without_marks`], in a numbered list that goes on from `expected` (a
/// reply cut into sentences as it comes counts on from one to the next),
/// left at the number the list goes on with.
fn without_marks_from(text: &str, expected: &mut u32, then: &str) -> String {
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
    // Whether a list's "2." comes after the word at `i` (in `text`, or as
    // the word the reply goes on with): a "1." with none after it, at the
    // start or after a sentence, is a number ("Deaths today? 1. Not bad.").
    let two_after = |i: usize| {
        words[i + 1..]
            .iter()
            .map(|(w, _)| w.as_str())
            .chain(then.split_whitespace().take(1))
            .any(|w| list_marker(w) == Some(Some(2)))
    };
    let mut out: Vec<String> = Vec::new();
    let mut capitalise = false;
    // Whether a list's mark went: its items are sentences.
    let mut listed = false;
    let mut i = 0;
    while i < words.len() {
        let (word, broke) = &words[i];
        let marker = list_marker(word);
        // A list's next number counts wherever it sits ("1) pot, 2) run").
        let opens = *broke
            || out.last().is_none_or(|last| ends_sentence(last))
            || (*expected > 1 && marker == Some(Some(*expected)));
        let last = i + 1 == words.len();
        let item = opens
            && !last
            && match marker {
                Some(None) => true,
                Some(Some(number)) => {
                    number == *expected && (number != 1 || (*broke && i > 0) || two_after(i))
                }
                None => false,
            };
        // The item before ends here, as a sentence (at the next item's
        // mark, or at a line's end once there is a list).
        if (item || (listed && *broke))
            && let Some(before) = out.last_mut()
            && !ends_item(before)
        {
            before.push('.');
            capitalise = true;
        }
        if item {
            if marker.is_some_and(|m| m.is_some()) {
                *expected += 1;
            }
            listed = true;
            i += 1;
            continue;
        }
        if opens && !last {
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
    // (And so does the last.)
    if listed
        && let Some(end) = out.last_mut()
        && !ends_item(end)
    {
        end.push('.');
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
    "בהחלט",
    "כמובן",
    "שאלה מצוינת",
    "שאלה טובה",
    "שאלה מעולה",
    "לסיכום",
];

/// Openers that say it is an AI. What follows one may go with it: a
/// disclaimer up to its "but" ([`past_disclaimer`]); and it needs no comma
/// before an "I" ("As an AI I can't…").
const AI_OPENERS: &[&str] = &[
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

/// How a disclaimer after "As an AI," starts: "I don't play the game
/// myself", "I can't see your screen", "אני לא רואה את המסך".
const DISCLAIMERS: &[&str] = &[
    "i dont ",
    "i do not ",
    "i cant ",
    "i cannot ",
    "i can not ",
    "im not ",
    "i am not ",
    "i have no ",
    "i havent ",
    "i wont ",
    "i will not ",
    "im unable ",
    "i am unable ",
    "אני לא ",
    "אינני ",
    "אין לי ",
    "לא באמת ",
];

/// Where a disclaimer gives way to what the sentence says.
const BUTS: &[&str] = &[
    ", but ",
    " but ",
    ", though ",
    "; ",
    " — ",
    " – ",
    ", אבל ",
    " אבל ",
    ", אך ",
    " אך ",
];

/// Where a disclaimer with no "but" gives way to advice: "I can't see your
/// screen, so check…", "…, and Night Lord is fine anyway."
const AND_SO: &[&str] = &[", so ", ", and ", ", אז "];

/// `rest`, what follows one of the [`AI_OPENERS`], without the disclaimer
/// it starts with, up to its "but": "I don't play the game myself, but
/// this map is fine for your level." is "this map is fine for your
/// level."; nothing when it was all disclaimer; as it is when it starts
/// with none ("I'd say farm Zakum, but bring pots."). With no "but", the
/// advice after it stays: from a ", so" or an ", and" ("I can't see your
/// screen, so check that the game is open."), or a comma before an order
/// or a "you" ("אני לא רואה את המסך, תבדוק שהמשחק פתוח."). A "but" left
/// after a dash goes ("I can't pick — but Night Lord suits you.").
fn past_disclaimer(rest: &str) -> String {
    let plain = format!("{} ", plain_words(rest));
    if !DISCLAIMERS.iter().any(|d| plain.starts_with(d)) {
        return rest.to_string();
    }
    let after_but = BUTS
        .iter()
        .filter_map(|b| rest.find(b).map(|at| at + b.len()))
        .min();
    if let Some(at) = after_but {
        let said = rest[at..].trim();
        let lower = said.to_lowercase();
        let but = ["but ", "though ", "אבל ", "אך "]
            .iter()
            .find(|b| lower.starts_with(*b));
        return match but {
            Some(b) => said[b.len()..].trim_start().to_string(),
            None => said.to_string(),
        };
    }
    // No "but": the advice after an "and so", or after a comma before an
    // order or a "you" (a disclaimer again is a disclaimer: "I don't have
    // eyes, and I can't see your screen." goes whole).
    let advises = |after: &str| {
        let first = plain_words(after.split_whitespace().next().unwrap_or_default());
        is_order(&first)
            || matches!(
                first.as_str(),
                "you" | "youre" | "youll" | "youd" | "youve" | "אתה" | "אתם"
            )
    };
    let and_so = AND_SO
        .iter()
        .filter_map(|s| rest.find(s).map(|at| at + s.len()))
        .min();
    let comma = rest
        .match_indices(", ")
        .map(|(at, s)| at + s.len())
        .find(|&at| advises(&rest[at..]));
    let Some(at) = [and_so, comma].into_iter().flatten().min() else {
        return String::new();
    };
    let said = rest[at..].trim();
    let plain = format!("{} ", plain_words(said));
    if DISCLAIMERS.iter().any(|d| plain.starts_with(d)) {
        past_disclaimer(said)
    } else {
        said.to_string()
    }
}

/// Whether the word that `rest` starts with is "I" ("I", "I'm", "אני",
/// "אין לי"): an AI opener goes before it without a comma too.
fn starts_with_i(rest: &str) -> bool {
    let first = plain_words(rest.split_whitespace().next().unwrap_or_default());
    matches!(
        first.as_str(),
        "i" | "im" | "ive" | "id" | "ill" | "אני" | "אין"
    )
}

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
    "לשתות",
    "להתרחק",
    "לברוח",
    "לקום",
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
    "תבדוק",
    "בדוק",
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
/// opener, "Sure thing, boss." keeps it (`Opened::Whole`). An AI opener
/// takes the disclaimer after it too ([`past_disclaimer`]: "As an AI, I
/// don't play the game myself, but this map is fine…" is "this map is
/// fine…").
fn without_opener(sentence: &str) -> Option<Opened> {
    let chars: Vec<char> = sentence.chars().collect();
    let openers = OPENERS
        .iter()
        .map(|o| (o, false))
        .chain(AI_OPENERS.iter().map(|o| (o, true)));
    'openers: for (opener, ai) in openers {
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
        let punctuated = matches!(next, ',' | '!' | ':' | ';' | '.' | '…' | '—' | '–' | '-');
        // ("As an AI I can't see your screen, but…".)
        let unpunctuated =
            ai && next.is_whitespace() && starts_with_i(&chars[at..].iter().collect::<String>());
        if !punctuated && !unpunctuated {
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
        if ai {
            rest = past_disclaimer(&rest);
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

    /// [`snapshot_with_view`] with the game in front (most of these tests
    /// have no window to ask).
    fn snapshot(obs: Option<&Observation>, progress: &Progress, so_far: &SoFar) -> String {
        snapshot_with_view(obs, true, progress, so_far)
    }

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
    fn same_as_before_is_a_card_in_its_attitude_and_never_the_same_twice_running() {
        let mut brain = Brain::new();
        for (attitude, lines) in Attitude::ALL.into_iter().zip(SAME_AS_BEFORE) {
            brain.attitude = attitude;
            assert!(lines.len() >= 3, "{attitude:?}");
            let round: Vec<&str> = (0..lines.len()).map(|_| brain.same_as_before()).collect();
            let mut sorted = round.clone();
            sorted.sort_unstable();
            let mut all = lines.to_vec();
            all.sort_unstable();
            assert_eq!(
                sorted, all,
                "{attitude:?}: every card once before any again"
            );
            let next = brain.same_as_before();
            assert!(lines.contains(&next));
            assert_ne!(next, *round.last().unwrap(), "{attitude:?}");
        }
        // The lead card says it plainly in each voice.
        assert_eq!(SAME_AS_BEFORE[0][0], "Still the same.");
        assert_eq!(SAME_AS_BEFORE[1][0], "Nothing's changed.");
        assert_eq!(SAME_AS_BEFORE[2][0], "I said. Twice.");
    }

    #[test]
    fn asked_to_talk_it_says_it_is_here_in_its_attitude_and_language() {
        assert!(asked_to_talk("Talk to me you fucker"));
        assert!(asked_to_talk("תדבר איתי"));
        assert!(!asked_to_talk("is it night already?"));
        assert!(!asked_to_talk("OK that sounds"));
        let mut brain = Brain::new();
        for (i, attitude) in Attitude::ALL.into_iter().enumerate() {
            brain.attitude = attitude;
            for (hebrew, lines) in [(false, HERE_EN), (true, HERE_HE)] {
                let cards = lines[i];
                assert!(cards.len() >= 3, "{attitude:?}");
                let mut round: Vec<&str> = (0..cards.len()).map(|_| brain.here(hebrew)).collect();
                round.sort_unstable();
                let mut all = cards.to_vec();
                all.sort_unstable();
                assert_eq!(round, all, "{attitude:?}: every card once before any again");
                for card in cards {
                    assert_eq!(is_hebrew(card), hebrew, "{card}");
                    assert!(!card.to_lowercase().contains("window"), "{card}");
                }
            }
        }
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
        assert!(text.starts_with("The MapleStory window is open and in view."));
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
        // The game behind another window: still read (the bars, the
        // level), but the picture is withheld, and the model is told so
        // instead of being told it can see the game and given no picture.
        let behind = snapshot_with_view(Some(&obs), false, &progress, &so_far);
        assert!(
            behind.starts_with("The MapleStory window is open, behind another window"),
            "{behind}"
        );
        assert!(!behind.contains("in view"), "{behind}");
        assert!(behind.contains("HP 1291 of 1351 (82%), MP about 40%."));
        assert_eq!(
            snapshot_with_view(Some(&obs), true, &progress, &so_far),
            text
        );
        // No window at all is said as before, in front or not.
        assert!(
            snapshot_with_view(None, false, &Progress::default(), &SoFar::default())
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
        let style = |attitude, kind, long| {
            voice_style(Delivery {
                attitude,
                kind,
                long,
            })
        };
        for attitude in Attitude::ALL {
            let warning = style(attitude, Kind::Warning, false);
            let news = style(attitude, Kind::Alert, false);
            let reply = style(attitude, Kind::Reply, false);
            let long = style(attitude, Kind::Reply, true);
            // The attitude's voice, whatever the line.
            for text in [&warning, &news, &reply, &long] {
                assert!(text.starts_with("Voice: "), "{text}");
                assert!(text.contains("Never an announcer or a robot."), "{text}");
            }
            // A warning is urgent; news (a death, a level-up) is told at
            // the usual pace, not shouted; a reply at the usual pace; a
            // long explanation a touch slower: four different deliveries.
            assert!(warning.contains("This line is a warning"), "{warning}");
            assert!(warning.contains("urgent, faster and sharper"), "{warning}");
            assert!(news.contains("This line is news"), "{news}");
            assert!(
                news.contains("your usual pace, said like it matters"),
                "{news}"
            );
            assert!(
                !news.contains("warning") && !news.contains("urgent"),
                "{news}"
            );
            assert!(
                reply.contains("This line is a reply in the chat"),
                "{reply}"
            );
            assert!(long.contains("a touch slower and steadier"), "{long}");
            let four = [&warning, &news, &reply, &long];
            for (i, a) in four.iter().enumerate() {
                for b in &four[i + 1..] {
                    assert_ne!(a, b);
                }
            }
            // Word about itself goes like a reply; a warning and news are
            // never long.
            assert_eq!(style(attitude, Kind::Info, false), reply);
            assert_eq!(style(attitude, Kind::Warning, true), warning);
            assert_eq!(style(attitude, Kind::Alert, true), news);
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
        // "still there?" once a session when the game sits idle, the quiet
        // spell in the snapshot); the model is told how to respond to it:
        // greet when told the phone connected, welcome them back once after
        // a long quiet, never ask after them — and never recite the
        // session facts.
        assert!(
            persona.contains(
                "greet only when your watcher says the phone just connected, never on your own"
            ),
            "{persona}"
        );
        assert!(
            persona.contains(
                "never ask whether they're still there — your watcher does, when the game idles"
            ),
            "{persona}"
        );
        // (The watcher asks when the game sits idle, not when the player
        // goes quiet while playing: the clause says which.)
        assert!(!persona.contains("when they go quiet"), "{persona}");
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

    /// The rule on status reports and on talk before the game, as the
    /// conversation has it (the call has it word for word: `live.rs`).
    const UNASKED_RULE: &str = "Never report their level, map or bars unasked; while MapleStory \
isn't open, talk about whatever they say, and say it isn't open only when they ask about the game.";

    #[test]
    fn a_word_to_it_is_answered_and_nothing_is_reported_unasked() {
        let persona = Brain::new().persona();
        // Four "Hello"s in a row came back "[ silent ]": a word to it is
        // never not for it.
        assert!(
            persona.contains(
                "Words to you (a greeting, your name, \"talk to me\") always get an answer; if they're \
clearly talking to someone else (stream chat, a friend, a call), reply with exactly: [silent]"
            ),
            "{persona}"
        );
        // Before the game everything got "No game window open."; in it,
        // every Hebrew reply restated the level and the map.
        assert!(persona.contains(UNASKED_RULE), "{persona}");
        assert!(
            !persona.contains("never open with where they are"),
            "the narrower rule went into the new one: {persona}"
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
    fn a_grind_of_warnings_in_a_row_is_one_turn_and_the_players_last_sentence_stays() {
        // Twenty warnings with no word from the player between them: one
        // pair in the conversation, the newest last, cut to the length a
        // reply is kept at — and "what's my level" still in the window.
        // (A pair each, they were twenty, and the window keeps sixteen.)
        let mut brain = Brain::new();
        brain.heard("what's my level");
        brain.said("165.");
        for i in 0..20 {
            brain.watched("a warning", &format!("HP {} percent. Pot now!", 30 - i));
        }
        let turns = brain.turns();
        assert_eq!(turns.len(), 4, "{turns:?}");
        assert_eq!(turns[0].text, "what's my level");
        assert_eq!(turns[1].text, "165.");
        assert_eq!(
            turns[2].text,
            "[Your game watcher, not the player: a warning.]"
        );
        let folded = &turns[3].text;
        assert!(folded.ends_with("HP 11 percent. Pot now!"), "{folded}");
        assert!(folded.starts_with("…HP "), "{folded}");
        assert!(folded.chars().count() <= REMEMBER_REPLY_CHARS, "{folded}");
        assert!(!folded.contains("HP 30 percent"), "{folded}");
        // A word from the player ends the run: the next line of its own is
        // a pair of its own, labelled anew.
        brain.heard("I know, I'm potting");
        brain.said("Good.");
        brain.watched("news", "Level 166! Nice.");
        let turns = brain.turns();
        assert_eq!(turns.len(), 8, "{turns:?}");
        assert_eq!(turns[6].text, "[Your game watcher, not the player: news.]");
        assert_eq!(turns[7].text, "Level 166! Nice.");
        // Two lines of its own of different kinds fold too, under the
        // newest's word.
        brain.watched("a warning", "HP 20 percent. Pot now!");
        let turns = brain.turns();
        assert_eq!(turns.len(), 8, "{turns:?}");
        assert_eq!(
            turns[6].text,
            "[Your game watcher, not the player: a warning.]"
        );
        assert_eq!(turns[7].text, "Level 166! Nice. HP 20 percent. Pot now!");
        // The tail: whole sentences from the end, "…" for the rest; the
        // last sentence alone, cut, when even that is too long.
        assert_eq!(tail_of("One. Two. Three.", 20), "One. Two. Three.");
        assert_eq!(tail_of("One one. Two two. Three.", 12), "…Three.");
        assert_eq!(tail_of("One one. Two two. Three.", 18), "…Two two. Three.");
        assert_eq!(tail_of("Abcdefghij.", 6), "…ghij.");
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
        // An AI's disclaimer goes with the opener, up to its "but" (w33's
        // "As an AI, I don't play the game myself, but this map is fine…"
        // kept "I don't play the game myself").
        (
            "As an AI, I can't see your screen, but your HP looks low.",
            "Your HP looks low.",
        ),
        (
            "As an AI, I don't play the game myself, but this map is fine for your level. Stay till 170.",
            "This map is fine for your level. Stay till 170.",
        ),
        (
            "Being an AI, I can't feel pain, though that death looked rough.",
            "That death looked rough.",
        ),
        (
            "As an AI I can't see your screen, but your HP looks low.",
            "Your HP looks low.",
        ),
        // All disclaimer: the sentence goes; what is no disclaimer stays.
        (
            "As an AI, I don't play the game myself. Stay till 170.",
            "Stay till 170.",
        ),
        (
            "As an AI, I'd say farm Zakum, but bring pots.",
            "I'd say farm Zakum, but bring pots.",
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
            "ה-HP שלך נמוך.",
        ),
        (
            "כבינה מלאכותית אני לא באמת שומע, אבל אני קורא כל מה שאתה אומר. יאללה נמשיך.",
            "אני קורא כל מה שאתה אומר. יאללה נמשיך.",
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

    /// What the snapshot said in the owner's sessions: level 167 at the
    /// Gate of the Future, HP 48%; the next morning, level 9 in Ellinia.
    fn gate() -> Facts {
        Facts {
            level: Some(167),
            map: Some("Gate of the Future".into()),
            name: None,
            job: None,
            bars: vec![48.0, 40.0],
        }
    }

    fn ellinia() -> Facts {
        Facts {
            level: Some(9),
            map: Some("Victoria Road / Ellinia".into()),
            name: Some("WANWANBUJIO".into()),
            job: Some("Beginner".into()),
            bars: vec![100.0, 100.0, 49.84],
        }
    }

    #[test]
    fn a_status_line_nobody_asked_for_is_not_said() {
        let closed = Facts::default();
        // (heard, the reply, what the snapshot said, what is said; `None`:
        // the reply as it was.)
        let rows: &[(&str, &str, Facts, Option<&str>)] = &[
            // The owner's evening, the game not open yet: its window's
            // state, to anything at all. Nothing else in it, and no
            // question asked: nothing to say.
            (
                "OK that sounds",
                "Game window closed, Danny.No game window open.",
                closed.clone(),
                Some(""),
            ),
            // Asked to talk: the window's state is better than nothing
            // (the persona tells it to talk about anything; this is the
            // floor, not the answer).
            (
                "Talk to me you fucker",
                "No MapleStory window open.",
                closed.clone(),
                None,
            ),
            (
                "In the shine at the sun",
                "No game window open.",
                closed.clone(),
                Some(""),
            ),
            // The game open: the level, the map, the bars.
            (
                "but you're about",
                "Game window open, WanWanBoggi at level 167, Gate of the Future. HP at 48%.",
                gate(),
                Some(""),
            ),
            (
                "Welcome let's play Maple",
                "Got it. What map are we on? MapleStory window is open, level 167.",
                gate(),
                Some("Got it. What map are we on?"),
            ),
            // The morning, in Hebrew: every reply said the level and the
            // map back; the part after the dash or the semicolon goes.
            (
                "Mushroom mushroom",
                "כן, פטריות—אתה באליניה, והרמה שלך 9.",
                ellinia(),
                Some("כן, פטריות."),
            ),
            (
                "Session",
                "אנחנו באליניה, רמה 9; מה בא לך לעשות עכשיו?",
                ellinia(),
                Some("מה בא לך לעשות עכשיו?"),
            ),
            // Asked about the game: said whole.
            (
                "what level am I?",
                "You're level 167, Gate of the Future.",
                gate(),
                None,
            ),
            ("where am I", "Gate of the Future, level 167.", gate(), None),
            (
                "is the game open?",
                "No game window open.",
                closed.clone(),
                None,
            ),
            ("how am I doing", "Level 167, HP at 48%.", gate(), None),
            ("מה הרמה שלי", "אתה ברמה 9.", ellinia(), None),
            // A question answered with nothing but the game's state keeps
            // it: better than nothing.
            (
                "is it night already?",
                "MapleStory window is open, level 167.",
                gate(),
                None,
            ),
            ("מה אתה חושב?", "אתה ברמה 9 באליניה.", ellinia(), None),
            // A friend's lines go through: an order, a cheer, someone
            // else's numbers, a window that is not the game's.
            ("this boss is hard", "Pot now, you're at 20.", gate(), None),
            ("this boss is hard", "HP's at 48, pot.", gate(), None),
            ("yes!", "Nice, level 167!", gate(), None),
            ("ugh", "Level up!", gate(), None),
            ("ugh", "Low HP, back off.", gate(), None),
            ("ugh", "Zakum's at 20% HP, keep hitting.", gate(), None),
            ("go", "Open the quest window.", gate(), None),
            (
                "I'm bored",
                "Grind Gate of the Future, it's fast.",
                gate(),
                None,
            ),
            ("כן", "יפה, רמה 9!", ellinia(), None),
        ];
        for (heard, reply, facts, said) in rows {
            let reply = humanise(reply);
            assert_eq!(
                unasked(&reply, heard, facts),
                said.unwrap_or(&reply),
                "{heard:?} → {reply:?}"
            );
        }
    }

    #[test]
    fn a_line_beside_a_number_that_says_something_of_its_own_is_said() {
        // The snapshots of the owner's main (HP 48%), his Classic character
        // in Ellinia (HP full, then 48%), and the game closed (the last
        // level, map and name kept, no bars).
        let main = Facts {
            name: Some("WanWanBoggi".into()),
            job: Some("Night Lord".into()),
            bars: vec![48.0, 90.0, 34.36],
            ..gate()
        };
        let low = Facts {
            bars: vec![48.0, 100.0, 49.84],
            ..ellinia()
        };
        let closed = Facts {
            bars: vec![],
            ..main.clone()
        };
        // Said whole: an opener's capital is the sentence's, not a name's;
        // a Hebrew order or cheer is no place; a number said to someone by
        // name is said for a reason; an order is an order. (Every one of
        // these was silence: the status filter took it for a recital.)
        let said: &[(&str, &str, &Facts)] = &[
            ("ugh this boss", "Pots, you're at 48% HP.", &main),
            ("ugh this boss", "Potion, HP 48%.", &main),
            ("ugh this boss", "Drink, HP 48%.", &main),
            ("ugh this boss", "48% HP, Einstein.", &main),
            ("ding!", "Bro, level 168!", &main),
            ("ding!", "Level 168, Michael!", &main),
            ("ok", "Hey, level 9!", &ellinia()),
            ("יש!", "עלית לרמה 10!", &ellinia()),
            ("יש!", "אחי, רמה 10!", &ellinia()),
            ("יש!", "רמה 10, בהצלחה!", &ellinia()),
            ("אוי", "48 אחוז חיים, שיקוי!", &low),
            ("אוי", "חיים 48 אחוז, תיזהר!", &low),
            ("אוי", "48 אחוז חיים, ותשתה!", &low),
            ("אוי", "48 אחוז חיים, מהר!", &low),
            ("let's play Maple", "Open the game.", &closed),
            ("let's play Maple", "Open MapleStory.", &closed),
            ("I'm bored", "Open the game!", &closed),
            ("I'm bored", "Then open the game.", &closed),
            // The level just reached, the snapshot a second behind; "now!"
            // called out (w26's last two).
            ("ding!", "Level 168!", &main),
            ("אוי", "48 אחוז, עכשיו!", &low),
        ];
        for (heard, reply, facts) in said {
            assert_eq!(unasked(reply, heard, facts), *reply, "{heard:?}");
            assert_eq!(without_status(reply, facts).as_deref(), Some(*reply));
        }
        // Still a recital: a name beside the window's state, the
        // character's own name, a pronoun's "you're" or Hebrew's "יש" (no
        // orders), the map in Hebrew beside the level.
        let dropped: &[(&str, &str, &Facts, &str)] = &[
            ("OK that sounds", "Game window closed, Danny.", &closed, ""),
            ("ok", "Game's closed, Michael.", &closed, ""),
            ("ok", "No game window open, Michael!", &closed, ""),
            ("ok", "You're level 167, WanWanBoggi.", &main, ""),
            ("ok", "You're level 167.", &main, ""),
            ("ding!", "Level 167!", &main, ""),
            ("ok", "אתה ברמה 9 עכשיו.", &ellinia(), ""),
            ("אוי", "יש לך 48 אחוז חיים.", &low, ""),
            ("I'm back", "WanWanBoggi! Missed you.", &main, "Missed you."),
            (
                "פטריות",
                "פטריות! אתה באליניה, רמה 9.",
                &ellinia(),
                "פטריות!",
            ),
        ];
        for (heard, reply, facts, kept) in dropped {
            assert_eq!(unasked(reply, heard, facts), *kept, "{heard:?} → {reply:?}");
        }
    }

    #[test]
    fn a_hebrew_window_recital_is_no_order() {
        // Before the game, in Hebrew, the model recites the window: "המשחק"
        // ("the game") lost its "the" and became "שחק" ("play!"), an order,
        // said whole; "של", "נשארו" were words of their own. (442bd85.)
        let closed = Facts {
            level: Some(167),
            map: Some("Gate of the Future".into()),
            name: Some("WanWanBoggi".into()),
            job: Some("Night Lord".into()),
            bars: vec![],
        };
        let main = Facts {
            bars: vec![48.0, 90.0, 34.36],
            ..closed.clone()
        };
        let low = Facts {
            bars: vec![48.0, 100.0, 49.84],
            ..ellinia()
        };
        for (heard, reply, facts) in [
            ("סתם", "המשחק לא פתוח.", &closed),
            ("סתם", "המשחק סגור כרגע.", &closed),
            ("משעמם לי", "החלון של המשחק סגור.", &closed),
            ("יאללה", "המשחק פתוח, רמה 167.", &main),
            ("אוי", "נשארו לך 48 אחוז חיים.", &low),
            ("אוי", "יש לך 48 אחוז חיים.", &low),
        ] {
            assert_eq!(unasked(reply, heard, facts), "", "{heard:?} → {reply:?}");
            assert_eq!(without_status(reply, facts), None, "{reply:?}");
        }
        // "And" joined to an order leaves it an order; the other letters
        // make nouns, and a fact's own word is never one.
        for order in ["ותשתה", "תשתה", "שחק", "Open", "pots", "Drink"] {
            assert!(is_order(&order.to_lowercase()), "{order}");
        }
        for word in [
            "המשחק",
            "משחק",
            "מדבר",
            "שלך",
            "game",
            "level",
            "חיים",
            "יש",
        ] {
            assert!(!is_order(word), "{word}");
        }
        // Asked to talk, a recital is no answer: the card that it is here
        // is (`ai::converse` deals it when nothing else is left).
        assert!(asked_to_talk("תדבר איתי"));
        assert_eq!(without_status("המשחק לא פתוח.", &closed), None);
    }

    #[test]
    fn a_hebrew_cheer_beside_a_number_is_said() {
        // The model's cheer to his "יש!": any word of מ, ב or ל and three
        // letters was "the map", and the whole reply was silence. (The
        // snapshot already at 10: the level is the snapshot's, the cheer
        // is what is left.)
        let ten = Facts {
            level: Some(10),
            ..ellinia()
        };
        let low = Facts {
            bars: vec![48.0, 100.0, 49.84],
            ..ellinia()
        };
        for (heard, reply, facts) in [
            ("יש!", "רמה 10, מגיע לך!", &ten),
            ("יש!", "רמה 10, מצוין!", &ten),
            ("יש!", "רמה 10, מגניב!", &ten),
            ("יש!", "רמה 10, מדהים!", &ten),
            ("יש!", "רמה 10, ברכות!", &ten),
            ("יש!", "רמה 10, ממש טוב!", &ten),
            ("יש!", "רמה 10, אלוף!", &ten),
            // (A cheer no list has: no "in" or "to" joined, no map.)
            ("יש!", "רמה 10, מרשים!", &ten),
            ("אוי", "48 אחוז חיים, לשתות!", &low),
            ("אוי", "48 אחוז, בזהירות!", &low),
            ("אוי", "48 אחוז, לזוז!", &low),
        ] {
            assert_eq!(unasked(reply, heard, facts), reply, "{heard:?} → {reply:?}");
        }
        // The map said in Hebrew is still the map.
        assert_eq!(unasked("אתה באליניה, רמה 9.", "פטריות", &ellinia()), "");
        assert_eq!(unasked("רמה 9, להנסיס.", "פטריות", &ellinia()), "");
    }

    #[test]
    fn hebrew_openers_places_and_decimals_do_not_save_a_recital() {
        // From w32's tables and w33's corpus (round I): a word a sentence
        // opens with said nothing yet, a cheer of "in" or "to" is no place
        // (his places are a closed set, or the map he is on), and a decimal
        // or a Hebrew "in" on an English map is still a reading.
        let at = |level: u32, hp: f32| Facts {
            level: Some(level),
            bars: vec![hp, 100.0, 49.84],
            ..ellinia()
        };
        let modern = Facts {
            level: Some(167),
            map: Some("Gate of the Future".into()),
            name: Some("WanWanBoggi".into()),
            job: Some("Night Lord".into()),
            bars: vec![48.0, 90.0, 34.36],
        };
        let closed = Facts {
            bars: vec![],
            ..modern.clone()
        };
        let evening = Facts {
            level: Some(165),
            map: Some("Gate of the Future".into()),
            name: Some("WanWan".into()),
            job: Some("Night Lord".into()),
            bars: vec![76.0, 22.0, 34.5],
        };
        let (nine, nine_low, ten, ten_low) =
            (at(9, 100.0), at(9, 48.0), at(10, 100.0), at(10, 48.0));
        for (heard, reply, facts) in [
            ("סתם", "תראה, אתה ברמה 9 באליניה.", &nine),
            ("יאללה", "בוא נראה, אתה ברמה 167.", &modern),
            ("משעמם לי", "תקשיב, המשחק לא פתוח.", &closed),
            ("סתם", "תשמע, החלון של המשחק סגור.", &closed),
            ("סתם", "רגע, המשחק סגור.", &closed),
            ("סתם", "אז אתה ברמה 9 באליניה.", &nine),
            ("סתם", "ובכן, אתה ברמה 167.", &modern),
            ("סתם", "תגיד, אתה ברמה 9, נכון?", &nine),
            ("סתם", "יש לך 100 אחוז חיים ו-100 אחוז מאנה.", &nine),
            ("אוי", "החיים שלך על 48 אחוז.", &nine_low),
            ("פטריות", "רמה 10 בגייט אוף דה פיוצ'ר.", &ten),
            ("פטריות", "אתה בויקטוריה רואד, רמה 10.", &ten),
            ("פטריות", "אתה בדרך לאליניה, רמה 10.", &ten),
            ("פטריות", "רמה 10, באל נאת.", &ten),
            ("im bored", "You're level 165 with 34.5% EXP.", &evening),
            ("סתם", "אתה ב-Gate of the Future, רמה 165.", &evening),
        ] {
            assert_eq!(unasked(reply, heard, facts), "", "{heard:?} → {reply:?}");
        }
        for (heard, reply, facts) in [
            ("יש!", "רמה 10, לחיים!", &ten),
            ("יש!", "רמה 10, בטירוף!", &ten),
            ("יש!", "רמה 10, בכבוד!", &ten),
            ("אוי", "48 אחוז חיים, לשיקוי!", &ten_low),
            ("אוי", "48 אחוז, לעיר!", &ten_low),
            ("אוי", "קח שיקוי, החיים על 48 אחוז.", &nine_low),
            ("סתם", "המשחק סגור, בוא נדבר על הבוס.", &closed),
        ] {
            assert_eq!(unasked(reply, heard, facts), reply, "{heard:?} → {reply:?}");
        }
        // What is said beside a recital stays.
        assert_eq!(
            unasked(
                "אז לך לבוס. דרך אגב, אתה רמה 165 ו-EXP 34.5%.",
                "משעמם לי",
                &evening
            ),
            "אז לך לבוס."
        );
        // A numbered list keeps its first item and is said as sentences.
        assert_eq!(
            for_speech(
                "Here's how: 1. Farm Zakum helmets 2. Sell them in the Free Market 3. Repeat every week"
            ),
            "Here's how: Farm Zakum helmets. Sell them in the Free Market. Repeat every week."
        );
    }

    #[test]
    fn a_word_or_two_that_asks_nothing_needs_no_same_as_before() {
        // The owner's "OK", "Hello", "Danny": a repeat that was all there
        // was got "1 of 1 sentences said before, left out" — and now a card.
        for heard in [
            "OK",
            "Hello",
            "Danny",
            "ok thanks",
            "תודה",
            "Mushroom mushroom",
        ] {
            assert!(a_word_or_two(heard), "{heard}");
        }
        for heard in [
            "what?",
            "level?",
            "is it",
            "where am I",
            "you said that already",
            "מה?",
        ] {
            assert!(!a_word_or_two(heard), "{heard}");
        }
    }

    #[test]
    fn two_sentences_glued_at_a_full_stop_are_parted() {
        // From the owner's evening: "Game window closed, Danny.No game
        // window open." went to the voice as one word, "Danny.No".
        for (reply, heard) in [
            (
                "Game window closed, Danny.No game window open.",
                "Game window closed, Danny. No game window open.",
            ),
            ("Pot now.Go left.", "Pot now. Go left."),
            ("Nice.Level up!", "Nice. Level up!"),
            // A number, an abbreviation, a domain, an acronym, a level
            // written short and Hebrew (no capitals) stay as they are.
            (
                "You're at 3.5 hours to level 58.",
                "You're at 3.5 hours to level 58.",
            ),
            ("Bring pots, e.g. Elixirs.", "Bring pots, e.g. Elixirs."),
            (
                "Check maplestory.nexon.net today.",
                "Check maplestory.nexon.net today.",
            ),
            (
                "Check maplestory.Nexon.net today.",
                "Check maplestory.Nexon.net today.",
            ),
            ("The U.S. server is down.", "The U.S. server is down."),
            ("Lv.200 is far.", "Lv.200 is far."),
            ("אתה באליניה.רמה 9.", "אתה באליניה.רמה 9."),
        ] {
            assert_eq!(humanise(reply), heard, "{reply:?}");
            assert_eq!(for_speech(reply), heard, "{reply:?}");
            assert_eq!(humanise(heard), heard, "{heard:?}");
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
        // A list's items are said as sentences.
        assert_eq!(for_speech("- **Pot** now\n- Go left"), "Pot now. Go left.");
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

    /// His character on `map` (w38's p38a facts).
    fn on(map: &str, level: u32, hp: f32) -> Facts {
        Facts {
            level: Some(level),
            map: Some(map.into()),
            name: Some("WanWanBoggi".into()),
            job: Some("Night Lord".into()),
            bars: vec![hp, 90.0, 34.36],
        }
    }

    #[test]
    fn an_order_or_an_everyday_word_is_no_place_of_his_map() {
        // w38's p38a (round K): "48 אחוז חיים, לשתות!" on Kerning City was
        // silence — "שתות" has the consonants of "City" (ST) — and on every
        // Victoria Island map "בהארד" and "בירידה" were "Road" (RD), "לקטר"
        // was "Victoria" (KTR).
        const VICTORIA: &[&str] = &["בהארד", "בירידה", "להורדה", "בהורדה", "ברדיו", "לקטר"];
        let rows: &[(&str, &[&str])] = &[
            ("Victoria Road / Kerning City", &["לשתות", "בשיטה", "בסתיו"]),
            ("Victoria Road / Lith Harbor", &["לעלות", "בעלות"]),
            (
                "Victoria Road / Henesys",
                &["בנושא", "לנושא", "בנסיעה", "לנסיעה", "לנוס"],
            ),
            ("Victoria Road / Ellinia", &["ללון"]),
            ("Gate of the Future", &["לגעת", "לגאות"]),
            (
                "Temple of Time / Memory Lane",
                &["לטעום", "בטעם", "בתום", "לומר", "ללון"],
            ),
            (
                "Chu Chu Island / Hungry Muto",
                &["באמת", "למטה", "למות", "לאמת"],
            ),
            ("Lachelein / Lachelein Main Street", &["לימין"]),
            (
                "Esfera / Mirror-touched Sea",
                &["לספר", "לספור", "לשפר", "בספר", "בסיפור", "לומר"],
            ),
            ("Arcane River / Vanishing Journey", &["לגרון"]),
        ];
        for (map, words) in rows {
            let victoria: &[&str] = if map.starts_with("Victoria") {
                VICTORIA
            } else {
                &[]
            };
            for word in words.iter().chain(victoria) {
                for reply in [format!("48 אחוז חיים, {word}!"), format!("רמה 9, {word}!")]
                {
                    let facts = on(map, 9, 48.0);
                    assert_eq!(unasked(&reply, "סתם", &facts), reply, "{map}: {reply:?}");
                }
            }
        }
        // Through the worker's filter, w38's p38b lines.
        let kerning = on("Victoria Road / Kerning City", 9, 48.0);
        assert_eq!(
            unasked("48 אחוז חיים, לשתות! הבוס מגיע.", "יאללה", &kerning),
            "48 אחוז חיים, לשתות! הבוס מגיע."
        );
        let ellinia = on("Victoria Road / Ellinia", 9, 48.0);
        assert_eq!(
            unasked("48 אחוז חיים ובירידה. תשתה.", "יאללה", &ellinia),
            "48 אחוז חיים ובירידה. תשתה."
        );
        // His places in Hebrew are still places: a recital (and the
        // renderings w38 found said: "בארקיין ריבר", "בצ'וצ'ו", "בלכליין",
        // "באלנאת", "בלודי").
        let places: &[(&str, &[&str])] = &[
            (
                "Victoria Road / Ellinia",
                &[
                    "אתה באליניה, רמה 9.",
                    "אתה באלינייה, רמה 9.",
                    "אתה באלניה, רמה 9.",
                    "רמה 9 בויקטוריה רואד.",
                    "רמה 9 בויקטוריה.",
                    "רמה 9 ברואד.",
                    "רמה 9 בדרך לאליניה.",
                    "רמה 9 ובאליניה.",
                ],
            ),
            (
                "Victoria Road / Henesys",
                &[
                    "אתה בהנסיס, רמה 9.",
                    "אתה בהניסיס, רמה 9.",
                    "רמה 9 בהנסייס.",
                ],
            ),
            (
                "Victoria Road / Kerning City",
                &[
                    "רמה 9 בקרנינג.",
                    "רמה 9 בקרנינג סיטי.",
                    "רמה 9 בכרנינג סיטי.",
                    "רמה 9 בסיטי.",
                ],
            ),
            (
                "Victoria Road / Lith Harbor",
                &[
                    "רמה 9 בלית' הארבור.",
                    "רמה 9 בלית הרבור.",
                    "רמה 9 בליט הארבור.",
                ],
            ),
            (
                "Victoria Road / Perion",
                &["רמה 9 בפריון.", "רמה 9 בפריאון.", "רמה 9 בפרי און."],
            ),
            ("Dungeon / Sleepywood", &["רמה 9 בסליפיווד."]),
            ("Ossyria / Orbis", &["רמה 9 באורביס."]),
            (
                "Ossyria / El Nath",
                &["רמה 9 באל נאת.", "רמה 9 באלנאת.", "רמה 9 באל-נאת."],
            ),
            (
                "Ludus Lake / Ludibrium",
                &["רמה 9 בלודיבריום.", "רמה 9 בלודי."],
            ),
            (
                "Gate of the Future",
                &[
                    "רמה 9 בגייט אוף דה פיוצ'ר.",
                    "רמה 9 בגייט.",
                    "רמה 9 בטמפל אוף טיים.",
                ],
            ),
            (
                "Arcane River / Vanishing Journey",
                &["רמה 9 בארקיין ריבר.", "רמה 9 בוונישינג ג'רני."],
            ),
            (
                "Chu Chu Island / Hungry Muto",
                &[
                    "רמה 9 בצ'וצ'ו.",
                    "רמה 9 בצ'ו צ'ו איילנד.",
                    "רמה 9 בהאנגרי מוטו.",
                ],
            ),
            (
                "Lachelein / Lachelein Main Street",
                &["רמה 9 בלכליין.", "רמה 9 בלאשלן."],
            ),
            (
                "Arcana / Cavern Lower Path",
                &["רמה 9 בארקנה.", "רמה 9 בארקאנה."],
            ),
            (
                "Morass / Shadowdance Hall",
                &["רמה 9 במוראס.", "רמה 9 במורס."],
            ),
            (
                "Esfera / Mirror-touched Sea",
                &["רמה 9 באספרה.", "רמה 9 באספירה."],
            ),
        ];
        for (map, replies) in places {
            for reply in *replies {
                assert_eq!(
                    unasked(reply, "סתם", &on(map, 9, 48.0)),
                    "",
                    "{map}: {reply:?}"
                );
            }
        }
    }

    #[test]
    fn a_hold_after_a_colon_is_still_an_announcement() {
        // w38's p38b (round K): what follows a colon is the answer only when
        // it says something — "one sec", "let me look", "בודק" hold.
        assert!(is_announcement("Let me check: one sec."));
        assert!(is_announcement("Hold on: let me look."));
        assert!(is_announcement("רגע: בודק."));
        assert!(is_announcement("שנייה: רגע אחד."));
        assert!(!is_announcement("Here's how: farm Zakum helmets."));
        assert!(!is_announcement(
            "Here's the deal: the left side respawns faster."
        ));
        assert!(!is_announcement(
            "Here's the thing: hold on to your elixirs."
        ));
        for piece in [1, 5, 300] {
            assert_eq!(
                split("Let me check: one sec. Farm the left side first.", piece),
                ["Farm the left side first."],
                "{piece}"
            );
            assert_eq!(
                split("Hold on: let me look. Rebuff first, then go left.", piece),
                ["Rebuff first, then go left."],
                "{piece}"
            );
            assert_eq!(
                split("רגע: בודק. תלך שמאלה לפורטל.", piece),
                ["תלך שמאלה לפורטל."],
                "{piece}"
            );
            // A list after "Here's how:" keeps its first item, an order of
            // two words too.
            assert_eq!(
                split("Here's how:\n1. Pot up\n2. Rebuff\n3. Go left", piece).join(" "),
                "Here's how: Pot up. Rebuff. Go left.",
                "{piece}"
            );
            assert_eq!(
                split(
                    "Here's how:\n1. Farm Zakum helmets\n2. Sell them in the Free Market",
                    piece
                )
                .join(" "),
                "Here's how: Farm Zakum helmets. Sell them in the Free Market.",
                "{piece}"
            );
        }
    }

    #[test]
    fn a_lone_one_is_a_number_and_not_a_list() {
        // w38's p38b (round K): a count after a question lost its number
        // ("Deaths today? 1. Not bad." → "Deaths today? Not bad."): a "1."
        // is a list's mark only when a "2." follows it.
        for piece in [1, 3, 5, 300] {
            for (reply, said) in [
                (
                    "Deaths today? 1. Not bad at all.",
                    "Deaths today? 1. Not bad at all.",
                ),
                ("1. לא רע בכלל.", "1. לא רע בכלל."),
                (
                    "Deaths today? 1. Not bad at all. Keep going like that.",
                    "Deaths today? 1. Not bad at all. Keep going like that.",
                ),
                // A list still loses its marks.
                (
                    "1. Farm Zakum helmets. 2. Sell them in the Free Market.",
                    "Farm Zakum helmets. Sell them in the Free Market.",
                ),
                ("1. Pot up 2. Rebuff 3. Go left", "Pot up. Rebuff. Go left."),
                (
                    "Here's how: 1. Farm Zakum helmets 2. Sell them in the Free Market",
                    "Here's how: Farm Zakum helmets. Sell them in the Free Market.",
                ),
                (
                    "1. Farm Zakum helmets\n2. Sell them in the Free Market",
                    "Farm Zakum helmets. Sell them in the Free Market.",
                ),
            ] {
                assert_eq!(split(reply, piece).join(" "), said, "{piece}: {reply:?}");
            }
        }
        assert_eq!(
            for_speech("Deaths today? 1. Not bad at all."),
            "Deaths today? 1. Not bad at all."
        );
        assert_eq!(for_speech("1. לא רע בכלל."), "1. לא רע בכלל.");
        assert_eq!(
            for_speech("1. Pot up 2. Rebuff 3. Go left"),
            "Pot up. Rebuff. Go left."
        );
    }

    #[test]
    fn what_follows_a_disclaimer_with_no_but_is_kept_when_it_advises() {
        // w38's p38a D (round K): a disclaimer with no "but" took the advice
        // after it, and a "— but" left a "But" dangling.
        for (reply, said) in [
            (
                "As an AI, I can't see your screen, so check that the game is open. Then tell me your HP.",
                "Check that the game is open. Then tell me your HP.",
            ),
            (
                "כבינה מלאכותית אני לא רואה את המסך, תבדוק שהמשחק פתוח. ואז תגיד לי כמה חיים יש לך.",
                "תבדוק שהמשחק פתוח. ואז תגיד לי כמה חיים יש לך.",
            ),
            (
                "As an AI I don't know your build, and Night Lord is fine anyway. Max Shadow Partner first.",
                "Night Lord is fine anyway. Max Shadow Partner first.",
            ),
            (
                "As an AI, I can't see your screen, check the game is open. Then pot.",
                "Check the game is open. Then pot.",
            ),
            (
                "As an AI, I can't pick for you — but Night Lord suits you. Max Shadow Partner first.",
                "Night Lord suits you. Max Shadow Partner first.",
            ),
            (
                "כבינה מלאכותית אני לא יכול לבחור בשבילך — אבל המפה הזאת טובה לך. תמשיך לחרוש.",
                "המפה הזאת טובה לך. תמשיך לחרוש.",
            ),
            // All disclaimer: it goes, as before.
            (
                "As an AI, I don't play the game myself. Farm Zakum helmets.",
                "Farm Zakum helmets.",
            ),
            (
                "As an AI, I can't see your screen, unfortunately. Farm Zakum helmets.",
                "Farm Zakum helmets.",
            ),
            (
                "As an AI, I don't have eyes, and I can't see your screen. Farm Zakum helmets.",
                "Farm Zakum helmets.",
            ),
        ] {
            assert_eq!(humanise(reply), said, "{reply:?}");
        }
    }

    /// The owner's real session (2026-10-10, Classic World): his sentences
    /// about the screen get it close up; chit-chat does not.
    #[test]
    fn the_owners_questions_about_the_screen_get_it_close_up_and_chit_chat_does_not() {
        for heard in [
            "where am I",
            "I don't want money talk to me in English I don't want money I want to know what level am I and how do I look",
            "what's equipped look at what's equipped and tell me what to get",
            "Where are the blue mushroom",
            "What's the next quest I should go to",
            "Yes I want you to tell me I want you to tell me where is the map that",
            "Go to the general store and where is the general store NPC",
            "Where is the homework table",
            "What is the best to improve according to what you say on the screen on my skill inventory",
            "what do you see",
            "what about now",
            "Check again",
            "Check carefully if the level you're flying that's it now",
            "what level am I",
            "איפה אני?",
            "מה הרמה שלי",
            "מה אתה רואה על המסך",
            "באיזו מפה אני",
        ] {
            assert!(about_the_screen(heard), "{heard}");
        }
        for heard in [
            "hey",
            "No my name is not Michael my name is Miguel with the talk to me bro",
            "Not Miguel Hebrew",
            "Mikael",
            "I need money",
            "OK",
            "Remember that",
            "A little",
            "100 sentences",
            "please congratulate me with the more enthusiastic response that I graduated to level 17",
            "Be enthusiastic and congratulate me for 10 sentences straight",
            "Don't stop don't stop don't stop congratulate me until I say stop",
            "I don't have I use lemons",
            "23% still owe",
            "You are no help",
            "היי",
            "תודה רבה",
        ] {
            assert!(!about_the_screen(heard), "{heard}");
        }
    }

    #[test]
    fn the_answer_is_in_his_sentences_language_unless_he_asked_for_one() {
        // English words get English (the log's "Not Miguel Hebrew" names a
        // language, it doesn't ask for it), Hebrew letters Hebrew.
        assert_eq!(asked_language("Not Miguel Hebrew"), None);
        assert_eq!(
            reply_language("Not Miguel Hebrew", None, Some("en-US")),
            "English"
        );
        assert_eq!(reply_language("Market", None, Some("he-IL")), "English");
        assert_eq!(reply_language("איפה אני", None, Some("en-US")), "Hebrew");
        assert_eq!(
            asked_language("I don't want money talk to me in English I don't want money"),
            Some("English")
        );
        assert_eq!(asked_language("תדבר איתי בעברית"), Some("Hebrew"));
        // Asked for, it stays until he asks for another.
        let mut brain = Brain::new();
        assert_eq!(
            brain.language_for("talk to me in Hebrew please", None),
            "Hebrew"
        );
        assert_eq!(brain.language_for("where am I", None), "Hebrew");
        assert_eq!(brain.language_for("in English now", None), "English");
        assert_eq!(brain.language_for("מה הרמה שלי", None), "English");
        // To a man, in Hebrew's masculine.
        let note = answer_note("Hebrew");
        assert!(note.contains("Answer in Hebrew"));
        assert!(note.contains("masculine (אתה, תפתח, תראה — never את, תפתחי, תראי)"));
        assert!(answer_note("English").contains("Answer in English"));
    }

    #[test]
    fn his_name_is_his_files_and_never_one_heard() {
        let mut brain = Brain::new();
        brain.about_player = "- My name is Michael (מיכאל)\n- I play a Magician.\n".into();
        assert_eq!(brain.player_name().as_deref(), Some("Michael (מיכאל)"));
        let persona = brain.persona();
        assert!(
            persona.contains("The player's name is Michael (מיכאל): it comes from his own file")
        );
        assert!(persona.contains("never take a name from what you hear, never rename him"));
        assert!(persona.contains("He is a man unless his own file says otherwise"));
        assert!(persona.contains("never guess anyone's gender from a name"));
        assert!(persona.contains("Never announce a level-up or mention their level unless asked."));
        // Not known: no name at all.
        let persona = Brain::new().persona();
        assert!(persona.contains("You don't know the player's name"));
        assert!(!persona.contains("The player's name is"));
    }

    #[test]
    fn classic_world_is_known_from_the_snapshot_or_what_he_taught() {
        assert!(classic_world(CLASSIC_SNAPSHOT, ""));
        assert!(classic_world(
            "",
            "Classic World maps: In the player's Classic World, there is no Maple Guide."
        ));
        assert!(!classic_world(
            "Level 16. HP 655 of 655.",
            "They play a Night Lord."
        ));
    }

    /// The owner's evening: "Level up! Congrats." copied into answers about
    /// MP and INT after he asked never to hear it; said when he asked.
    #[test]
    fn a_level_up_nobody_asked_about_is_not_said() {
        for (heard, reply, said) in [
            (
                "My topping up",
                "Good, MP topped. Level up! Congrats.",
                "Good, MP topped.",
            ),
            (
                "INTINT",
                "Level up! Congrats. INT's already maxed for your level—save the next points.",
                "INT's already maxed for your level—save the next points.",
            ),
            // (Its own "Congrats" heard back asks for nothing.)
            (
                "Congrats is point eye of the",
                "Level up! Congrats. Head left to that quest marker.",
                "Head left to that quest marker.",
            ),
            ("Mail mail", "עלית רמה! מזל טוב. תמשיך ככה.", "תמשיך ככה."),
            // Saying there was none is no announcement.
            (
                "stop it I didn't leveled up",
                "נכון, לא עלית רמה; האפקט הטעה אותי.",
                "נכון, לא עלית רמה; האפקט הטעה אותי.",
            ),
            (
                "Check again",
                "The level-up effect is still on screen; your level bar shows 16, so nothing changed.",
                "The level-up effect is still on screen; your level bar shows 16, so nothing changed.",
            ),
            // Asked about the level, or to be congratulated: said.
            (
                "please congratulate me with the more enthusiastic response that I graduated to level 17",
                "מזל טוב! עלית לרמה 17, כל הכבוד!",
                "מזל טוב! עלית לרמה 17, כל הכבוד!",
            ),
        ] {
            assert_eq!(without_level_up(reply, heard).0, said, "{heard}: {reply}");
        }
    }
}
