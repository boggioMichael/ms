//! The companion: what MapleSyrup says, and when.
//!
//! It is fed one [`Observation`] per frame and whatever the phone sends
//! (sentences it heard, buttons pressed), and answers with [`Say`] lines and
//! the odd action. It has no clock, no window and no voice of its own —
//! time is passed in as seconds — so every rule here is tested by playing a
//! session through it.
//!
//! It speaks up on its own for little: HP or MP running low, a level-up, a
//! death, and the game window coming and going. Everything else waits to be
//! asked.

pub mod attitude;
pub mod chat;
pub mod commands;
pub mod exp;
pub mod instant;
pub mod observation;

pub use attitude::Attitude;

use serde::Serialize;

pub use commands::{Command, Heard};
pub use exp::{ExpTracker, spoken_duration};
pub use observation::{GameView, Gauge, Observation};

/// Thresholds and pacing.
#[derive(Debug, Clone)]
pub struct Settings {
    /// Warn when HP falls below this percent…
    pub hp_low: f32,
    /// …and warn again only after it has recovered above this.
    pub hp_rearm: f32,
    pub mp_low: f32,
    pub mp_rearm: f32,
    /// A warning is not repeated sooner than this, in seconds.
    pub warning_cooldown: f64,
    /// How long the game must be gone before saying so, in seconds.
    pub lost_after: f64,
    /// After the wake word alone, how long the next sentence counts as
    /// addressed, in seconds.
    pub listen_for: f64,
    /// Answer everything said to it (true), or only sentences with the wake
    /// word "syrup" in them (for streams, where most talk is to the chat).
    pub always_listen: bool,
    /// How its own lines sound (warnings, deaths, level-ups).
    pub attitude: Attitude,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            hp_low: 30.0,
            hp_rearm: 45.0,
            mp_low: 15.0,
            mp_rearm: 30.0,
            warning_cooldown: 20.0,
            lost_after: 5.0,
            listen_for: 8.0,
            always_listen: true,
            attitude: Attitude::Friendly,
        }
    }
}

/// Why a line is said: the phone and the console colour them differently.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// The companion noticed something on its own (low HP, a level-up).
    Alert,
    /// An answer to something the player asked.
    Reply,
    /// News about the companion itself (phone connected, muted).
    Info,
    /// What the phone heard the player say, shown for reference.
    Heard,
}

/// One line for the player.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Say {
    pub kind: Kind,
    pub text: String,
    /// Whether it should be spoken aloud (when not muted), or only shown.
    pub speak: bool,
}

impl Say {
    fn alert(text: impl Into<String>) -> Self {
        Self {
            kind: Kind::Alert,
            text: text.into(),
            speak: true,
        }
    }
    fn reply(text: impl Into<String>) -> Self {
        Self {
            kind: Kind::Reply,
            text: text.into(),
            speak: true,
        }
    }
    fn info(text: impl Into<String>, speak: bool) -> Self {
        Self {
            kind: Kind::Info,
            text: text.into(),
            speak,
        }
    }
}

/// What the companion wants done.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Say(Say),
    /// Save this moment: the current frame and a line in markers.csv.
    Mark,
    /// Stop (true) or resume (false) speaking aloud.
    SetMuted(bool),
}

/// Session figures the phone and the console show.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Progress {
    pub seconds: f64,
    pub exp_per_hour: Option<f64>,
    pub seconds_to_level: Option<f64>,
    pub levels_gained: u32,
    pub marks: u32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Warning {
    /// Below the threshold, warned at this time.
    Warned(f64),
    /// Recovered (or never low): the next fall warns.
    Armed,
}

pub struct Companion {
    pub settings: Settings,
    last: Option<Observation>,
    /// When the game was last seen, and whether its loss was announced.
    seen_at: Option<f64>,
    announced_lost: bool,
    ever_seen: bool,
    hp_warning: Warning,
    mp_warning: Warning,
    /// Consecutive frames below the HP / MP threshold, and since when (a
    /// frame or two can be a bar half-covered by a dialog, or misread).
    hp_low_frames: u32,
    mp_low_frames: u32,
    hp_low_since: f64,
    mp_low_since: f64,
    /// Counts warnings, to vary how they are said.
    warnings: u32,
    zero_hp_frames: u32,
    dead: bool,
    /// HP lately (when, percent), to tell a death a sooner warning could
    /// have helped with from a sudden one, and a beating as it happens.
    hp_lately: std::collections::VecDeque<(f64, f32)>,
    /// Frames in a row in which HP has fallen fast, and when that was last
    /// said.
    falling_frames: u32,
    fall_told: f64,
    listening_until: f64,
    muted: bool,
    exp: ExpTracker,
    last_level: Option<u32>,
    /// The character's name when the level was last taken: a level that
    /// comes with another name is another character's.
    last_name: Option<String>,
    /// A level reading waiting to hold: the level, and since when.
    level_candidate: Option<(u32, f64)>,
    announced_level_up: f64,
    marks: u32,
    now: f64,
    /// Lines spoken lately (when, and normalised), to tell the phone hearing
    /// MapleSyrup's own voice from the player.
    spoken: std::collections::VecDeque<(f64, String)>,
    /// Counts small-talk answers, to vary them.
    turn: u32,
    /// Whether the HP and MP bars are being read or guessed at.
    hp_steady: Steadiness,
    mp_steady: Steadiness,
    /// Since when HP has read as zero.
    zero_hp_since: f64,
    /// Alerts said with no sign of life from the player since, and the
    /// state when the last was said: when, and HP, EXP and the level then,
    /// with the highest HP and EXP seen since.
    unanswered: u32,
    alert_at: f64,
    hp_at_alert: Option<f32>,
    hp_peak: Option<f32>,
    exp_at_alert: Option<f32>,
    exp_peak: Option<f32>,
    level_at_alert: Option<u32>,
    /// When the player last spoke.
    spoke_at: f64,
    /// Until when alerts are held for want of an answer, and when that was
    /// last said.
    hold_until: f64,
    hold_told: f64,
}

/// A bar's readings lately, to tell a bar that is being read from one
/// that is being guessed at: readings that swing back and forth — three
/// turns of [`SWING`] points or more within [`SWINGS_WINDOW`] seconds —
/// are a guess (HP does not go 3%, 46%, 9%, 11%, 3% in a few seconds while
/// the character stands), and nothing is said from them until they have
/// held steady for [`SETTLE_SECS`].
struct Steadiness {
    lately: std::collections::VecDeque<(f64, f32)>,
    unsteady_until: f64,
    /// How long the current hold is.
    settle: f64,
    /// When the player was last told the readings are jumping around.
    noted: f64,
}

const SWING: f32 = 20.0;
const SWINGS_WINDOW: f64 = 6.0;
const SWINGS_UNSTEADY: u32 = 3;
/// How long a bar found unsteady is held, the first time; it doubles each
/// time it is found unsteady again as soon as the hold ends, up to a cap.
const SETTLE_SECS: f64 = 20.0;
const SETTLE_MAX_SECS: f64 = 600.0;
/// How often the player is told that a bar's readings are jumping around.
const UNSTEADY_NOTE_EVERY: f64 = 600.0;

impl Steadiness {
    fn new() -> Self {
        Self {
            lately: std::collections::VecDeque::new(),
            unsteady_until: f64::NEG_INFINITY,
            settle: SETTLE_SECS,
            noted: f64::NEG_INFINITY,
        }
    }

    /// Takes a reading; whether the bar is being guessed at right now.
    fn unsteady(&mut self, now: f64, percent: f32) -> bool {
        self.lately.push_back((now, percent));
        while self
            .lately
            .front()
            .is_some_and(|(t, _)| now - t > SWINGS_WINDOW)
        {
            self.lately.pop_front();
        }
        // While held, one more turn keeps it held (the hold ends a settle
        // after the last turn); found unsteady again as soon as the hold
        // ends, it is held twice as long, so a bar misread all night is
        // not warned from every half minute; steady for a while, a new
        // bout starts over.
        let turns = reversals(self.lately.iter().map(|(_, p)| *p), SWING);
        let held = now < self.unsteady_until;
        let just_ended = !held && now < self.unsteady_until + SWINGS_WINDOW;
        let unsteady = if held || just_ended {
            turns >= 1
        } else {
            turns >= SWINGS_UNSTEADY
        };
        if unsteady {
            if just_ended {
                self.settle = (self.settle * 2.0).min(SETTLE_MAX_SECS);
            } else if !held {
                self.settle = SETTLE_SECS;
            }
            self.unsteady_until = now + self.settle;
            self.lately.clear();
        }
        now < self.unsteady_until
    }
}

/// How many times a series of readings turns around by `swing` points or
/// more: up then down is one turn, down, up and down again two.
fn reversals(readings: impl IntoIterator<Item = f32>, swing: f32) -> u32 {
    let mut readings = readings.into_iter();
    let Some(mut pivot) = readings.next() else {
        return 0;
    };
    let mut direction = 0i8;
    let mut turns = 0;
    for value in readings {
        let change = value - pivot;
        if change.abs() >= swing {
            let heading = if change > 0.0 { 1 } else { -1 };
            if direction != 0 && heading != direction {
                turns += 1;
            }
            direction = heading;
            pivot = value;
        } else if (direction > 0 && value > pivot) || (direction < 0 && value < pivot) {
            // The move goes on.
            pivot = value;
        }
    }
    turns
}

/// A death read from the bar's fill (not the printed number) must last
/// this long: a dialog over the bar reads as an empty bar too.
const ZERO_HOLD_SECS: f64 = 2.0;

/// Alerts said with no sign of life from the player — a word, HP going
/// back up (a potion), EXP gained, a level — before the rest are held…
const UNANSWERED_MAX: u32 = 6;
/// …and for how long, unless the player turns up sooner. When the hold
/// ends, a couple more come through before the next.
const HOLD_SECS: f64 = 600.0;
const AFTER_HOLD: u32 = 2;
/// How often the player is told their warnings are being held.
const HOLD_TOLD_EVERY: f64 = 1800.0;
/// HP back up by this much since the last alert is a potion: a sign of life.
const POTTED: f32 = 10.0;
/// EXP up by this much since the last alert is play going on.
const EXP_GAINED: f32 = 0.2;

/// How long a new level reading must hold before it is believed, in seconds.
const LEVEL_HOLD_SECS: f64 = 3.0;

/// How long HP or MP must stay low before it is said, in seconds (and at
/// least three frames): a moment's misread is not worth a warning.
const LOW_HOLD_SECS: f64 = 0.6;

/// HP warnings move sooner after deaths no warning came before, up to here.
const SOONEST_WARNING: f32 = 50.0;

/// HP down this many points within [`FALL_SECS`] is a beating, said at
/// once (after a second frame says so: a dialog half over the bar does not
/// last) — before it is low, while backing off still helps.
const FALL_POINTS: f32 = 25.0;
const FALL_SECS: f64 = 3.0;
/// A beating is not said again sooner than this, in seconds.
const FALL_COOLDOWN: f64 = 12.0;

/// How long after a line was said the phone may still hand it back as heard,
/// in seconds: the line, its playing, and the phone's recognition finishing.
const ECHO_WINDOW: f64 = 30.0;

/// Words that stop MapleSyrup on their own when the player says them over it.
const STOP_WORDS: &[&str] = &[
    "stop",
    "wait",
    "mute",
    "השתק",
    "hold",
    "shush",
    "shh",
    "quiet",
    "enough",
    "cancel",
    "nevermind",
    "רגע",
    "די",
    "עצור",
    "תפסיק",
    "שקט",
    "חכה",
    "סטופ",
    "para",
    "espera",
    "basta",
    "chega",
    "attends",
    "arrête",
    "arrete",
    "stopp",
    "warte",
    "halt",
    "잠깐",
    "그만",
    "멈춰",
    "待って",
    "ストップ",
    "やめて",
    "等等",
    "停",
    "别说了",
    "別說了",
    "стоп",
    "подожди",
    "хватит",
];

/// Sounds that are not words.
const FILLERS: &[&str] = &[
    "uh", "um", "uhm", "umm", "hmm", "mm", "mmm", "ah", "oh", "eh", "er", "אה", "אמ", "הממ", "אממ",
];

/// How numbers sound: the phone may write "sixty percent" for MapleSyrup's
/// "60%".
const NUMBER_WORDS: &[&str] = &[
    "zero",
    "one",
    "two",
    "three",
    "four",
    "five",
    "six",
    "seven",
    "eight",
    "nine",
    "ten",
    "eleven",
    "twelve",
    "thirteen",
    "fourteen",
    "fifteen",
    "sixteen",
    "seventeen",
    "eighteen",
    "nineteen",
    "twenty",
    "thirty",
    "forty",
    "fifty",
    "sixty",
    "seventy",
    "eighty",
    "ninety",
    "hundred",
    "thousand",
    "million",
    "point",
    "percent",
    "per",
    "cent",
];

/// Writing without spaces between words (Chinese, Japanese, Thai): a run of
/// three letters or more counts as two words.
fn unspaced(word: &str) -> bool {
    word.chars()
        .filter(|c| {
            ('\u{3040}'..='\u{30FF}').contains(c)
                || ('\u{4E00}'..='\u{9FFF}').contains(c)
                || ('\u{0E00}'..='\u{0E7F}').contains(c)
        })
        .count()
        >= 3
}

impl Companion {
    pub fn new(settings: Settings) -> Self {
        Self {
            settings,
            last: None,
            seen_at: None,
            announced_lost: false,
            ever_seen: false,
            hp_warning: Warning::Armed,
            mp_warning: Warning::Armed,
            hp_low_frames: 0,
            mp_low_frames: 0,
            hp_low_since: 0.0,
            mp_low_since: 0.0,
            warnings: 0,
            zero_hp_frames: 0,
            dead: false,
            hp_lately: std::collections::VecDeque::new(),
            falling_frames: 0,
            fall_told: f64::NEG_INFINITY,
            listening_until: f64::NEG_INFINITY,
            muted: false,
            exp: ExpTracker::new(),
            last_level: None,
            last_name: None,
            level_candidate: None,
            announced_level_up: f64::NEG_INFINITY,
            marks: 0,
            now: 0.0,
            spoken: std::collections::VecDeque::new(),
            turn: 0,
            hp_steady: Steadiness::new(),
            mp_steady: Steadiness::new(),
            zero_hp_since: 0.0,
            unanswered: 0,
            alert_at: f64::NEG_INFINITY,
            hp_at_alert: None,
            hp_peak: None,
            exp_at_alert: None,
            exp_peak: None,
            level_at_alert: None,
            spoke_at: f64::NEG_INFINITY,
            hold_until: f64::NEG_INFINITY,
            hold_told: f64::NEG_INFINITY,
        }
    }

    /// The player said something (to MapleSyrup or near it): a sign of
    /// life, and the end of any hold on the alerts.
    pub fn player_spoke(&mut self, now: f64) {
        self.spoke_at = now;
        self.unanswered = 0;
        self.hold_until = f64::NEG_INFINITY;
    }

    /// Whether alerts are being held because nothing the player does
    /// answers them (they are away, or the readings are wrong).
    pub fn alerts_held(&self, now: f64) -> bool {
        now < self.hold_until
    }

    /// Note a line that was spoken aloud (by the PC or the phone).
    pub fn remember_spoken(&mut self, now: f64, text: &str) {
        self.spoken.push_back((now, commands::normalize(text)));
        while self.spoken.front().is_some_and(|(t, _)| now - t > 45.0) {
            self.spoken.pop_front();
        }
    }

    /// Whether `heard` is only MapleSyrup's own voice coming back through the
    /// phone's microphone, with nothing of the player's in it.
    pub fn is_echo(&self, now: f64, heard: &str) -> bool {
        self.strip_echo(now, heard).is_none()
    }

    /// The player's words in `heard`, with MapleSyrup's own voice taken out:
    /// runs of words it said lately that the phone heard back, as when its
    /// last words and the player's answer end up in one sentence ("how are
    /// you doing — let's play"). `None` when nothing of the player's is left.
    ///
    /// A run counts as an echo when its words were said lately and mostly in
    /// that order: three words or more, or two within a few seconds. A word
    /// or two the player repeats from MapleSyrup's line stays theirs.
    pub fn strip_echo(&self, now: f64, heard: &str) -> Option<String> {
        let heard = heard.trim();
        // Each word of the sentence, normalised, with the token it came from.
        let tokens: Vec<&str> = heard.split_whitespace().collect();
        let mut words: Vec<(String, usize)> = Vec::new();
        for (i, token) in tokens.iter().enumerate() {
            for word in commands::normalize(token)
                .split(' ')
                .filter(|w| !w.is_empty())
            {
                words.push((word.to_string(), i));
            }
        }
        if words.is_empty() {
            return None;
        }
        let recent: Vec<(f64, Vec<&str>)> = self
            .spoken
            .iter()
            .map(|(t, said)| (now - t, said.split(' ').collect::<Vec<_>>()))
            .filter(|(age, _)| (-1.0..=ECHO_WINDOW).contains(age))
            .collect();
        if recent.is_empty() {
            return Some(heard.to_string());
        }
        let said = |w: &str| recent.iter().any(|(_, line)| line.contains(&w));
        let mut echo = vec![false; words.len()];
        let mut start = 0;
        while start < words.len() {
            if !said(&words[start].0) {
                start += 1;
                continue;
            }
            let mut end = start + 1;
            while end < words.len() && said(&words[end].0) {
                end += 1;
            }
            let run = &words[start..end];
            let length = run.len();
            // The player quoting MapleSyrup's words in the middle of their
            // own sentence ("I am not in the Gate of the Future, check
            // again") is not its voice coming back: a run with words of
            // the player's on both sides of it is theirs.
            let own = |range: &[(String, usize)]| range.iter().filter(|(w, _)| !said(w)).count();
            let quoted = own(&words[..start]) >= 2 && own(&words[end..]) >= 2;
            let is_echo = length >= 2
                && !quoted
                && recent.iter().any(|(age, line)| {
                    let in_order = run
                        .windows(2)
                        .filter(|pair| {
                            line.windows(2)
                                .any(|l| l[0] == pair[0].0.as_str() && l[1] == pair[1].0.as_str())
                        })
                        .count();
                    (length >= 3 && in_order * 2 >= length - 1)
                        || (length == 2 && in_order == 1 && *age <= 12.0)
                });
            if is_echo {
                echo[start..end].iter_mut().for_each(|e| *e = true);
            }
            start = end;
        }
        if !echo.contains(&true) {
            return Some(heard.to_string());
        }
        // Tokens with a word of the player's in them, in order.
        let mut kept: Vec<usize> = words
            .iter()
            .zip(&echo)
            .filter(|(_, e)| !**e)
            .map(|((_, token), _)| *token)
            .collect();
        kept.dedup();
        let left = echo.iter().filter(|e| !**e).count();
        if left < 2 {
            return None;
        }
        let rest = kept
            .iter()
            .map(|&i| tokens[i])
            .collect::<Vec<_>>()
            .join(" ");
        // Mostly its own voice: the few words left are its own too, written
        // differently ("Monster פארק שקד" for "Monster Park Shuttle"), unless
        // they stop it or ask for something.
        let mostly_echo = left <= 3 && left * 10 < words.len() * 3;
        if mostly_echo {
            let stops = commands::normalize(&rest)
                .split(' ')
                .any(|w| STOP_WORDS.contains(&w));
            if !stops && commands::local_command(&rest).is_none() {
                return None;
            }
        }
        Some(rest)
    }

    /// What the phone is hearing right now, while MapleSyrup may be
    /// talking: the player's own words in it, when there are enough to stop
    /// talking for — two or more among the last few words, or a word such as
    /// "stop" or "wait". `None` when it is only MapleSyrup's own voice
    /// coming back (or a sound or two).
    pub fn barge_in(&self, now: f64, hearing: &str) -> Option<String> {
        self.players_words(now, hearing, 2)
    }

    /// Whether the player is still talking (`hearing`, while their last
    /// sentence is being answered and nothing has been said yet): one word
    /// of theirs is enough.
    pub fn still_talking(&self, now: f64, hearing: &str) -> bool {
        self.players_words(now, hearing, 1).is_some()
    }

    /// The player's words in `hearing` when there are at least `enough`
    /// of them among the last few (or a stop word).
    fn players_words(&self, now: f64, hearing: &str, enough: usize) -> Option<String> {
        let tokens: Vec<&str> = hearing.split_whitespace().collect();
        let mut words: Vec<(String, usize)> = Vec::new();
        for (i, token) in tokens.iter().enumerate() {
            for word in commands::normalize(token)
                .split(' ')
                .filter(|w| !w.is_empty())
            {
                words.push((word.to_string(), i));
            }
        }
        if words.is_empty() {
            return None;
        }
        let lines: Vec<&str> = self
            .spoken
            .iter()
            .filter(|(t, _)| (-1.0..=ECHO_WINDOW).contains(&(now - t)))
            .map(|(_, said)| said.as_str())
            .collect();
        let said: std::collections::HashSet<&str> =
            lines.iter().flat_map(|line| line.split(' ')).collect();
        let numbers = lines
            .iter()
            .any(|line| line.chars().any(|c| c.is_ascii_digit()));
        let theirs: Vec<bool> = words
            .iter()
            .map(|(w, _)| {
                w.chars().count() >= 2
                    && !said.contains(w.as_str())
                    && !FILLERS.contains(&w.as_str())
                    && !(numbers
                        && (w.chars().all(|c| c.is_ascii_digit())
                            || NUMBER_WORDS.contains(&w.as_str())))
            })
            .collect();
        let stop_word = |w: &str| {
            STOP_WORDS.contains(&w) || (unspaced(w) && STOP_WORDS.iter().any(|s| w.contains(s)))
        };
        let tail = words.len().saturating_sub(3);
        let stop = words[tail..]
            .iter()
            .zip(&theirs[tail..])
            .any(|((w, _), &t)| t && stop_word(w));
        let recent = words.len().saturating_sub(6);
        let count: usize = words[recent..]
            .iter()
            .zip(&theirs[recent..])
            .filter(|(_, t)| **t)
            .map(|((w, _), _)| if unspaced(w) { 2 } else { 1 })
            .sum();
        if !stop && count < enough.max(1) {
            return None;
        }
        // From the player's first word to their last (a word of theirs that
        // MapleSyrup also used stays in).
        let theirs_at: Vec<usize> = words
            .iter()
            .zip(&theirs)
            .filter(|(_, t)| **t)
            .map(|((_, token), _)| *token)
            .collect();
        let (first, last) = (*theirs_at.first()?, *theirs_at.last()?);
        Some(tokens[first..=last].join(" "))
    }

    /// The player's words in a sentence the phone heard, with MapleSyrup's
    /// own voice taken out (`strip_echo`). Heard while MapleSyrup talks or
    /// just after, what is left must also have at least `need` words it did
    /// not just say (or a stop word): a word or two of its own voice that the
    /// phone wrote differently is not the player's. (Two while it talks;
    /// one just after, so a quick "yes" to its question still counts.)
    pub fn own_words(&self, now: f64, heard: &str, need: usize) -> Option<String> {
        let rest = self.strip_echo(now, heard)?;
        if need == 0 {
            return Some(rest);
        }
        self.players_words(now, &rest, need).map(|_| rest)
    }

    pub fn set_always_listen(&mut self, on: bool) {
        self.settings.always_listen = on;
    }

    pub fn muted(&self) -> bool {
        self.muted
    }

    /// The character is dead (HP at zero), until HP comes back.
    pub fn dead(&self) -> bool {
        self.dead
    }

    /// The most recent frame's observation.
    pub fn last(&self) -> Option<&Observation> {
        self.last.as_ref()
    }

    pub fn progress(&self) -> Progress {
        Progress {
            seconds: self.now,
            exp_per_hour: self.exp.per_hour(),
            seconds_to_level: self.exp.seconds_to_level(),
            levels_gained: self.exp.levels_gained(),
            marks: self.marks,
        }
    }

    /// Whether a sentence now would count as addressed without the wake word.
    /// When a level-up was last announced (never: minus infinity).
    pub fn last_level_up(&self) -> f64 {
        self.announced_level_up
    }

    /// The level, as last believed.
    pub fn level(&self) -> Option<u32> {
        self.last_level
    }

    pub fn listening(&self, now: f64) -> bool {
        now <= self.listening_until
    }

    /// The first words, when MapleSyrup starts.
    pub fn hello(&self) -> Vec<Action> {
        vec![Action::Say(Say::info(
            "Maple companion is on. Open MapleStory and I'll keep an eye on it.",
            true,
        ))]
    }

    /// One frame.
    pub fn observe(&mut self, now: f64, obs: Observation) -> Vec<Action> {
        self.now = now;
        let mut out = Vec::new();
        self.track_window(now, &obs, &mut out);
        if obs.game.is_seen() {
            self.watch_hp(now, &obs, &mut out);
            self.watch_mp(now, &obs, &mut out);
            self.watch_progress(now, &obs, &mut out);
        }
        self.pace(now, &obs, &mut out);
        self.last = Some(obs);
        out
    }

    /// Alerts that nothing answers are held: after [`UNANSWERED_MAX`] of
    /// them with no sign of life from the player — not a word, no potion,
    /// no EXP gained, no level — the rest wait [`HOLD_SECS`] (and the
    /// player is told once why), unless the player turns up sooner. One
    /// night of a misread bar ran to 3,400 warnings said to an empty room.
    fn pace(&mut self, now: f64, obs: &Observation, out: &mut Vec<Action>) {
        let hp = obs.hp.map(|g| g.percent);
        let exp = obs.exp.map(|g| g.percent);
        let higher = |peak: Option<f32>, value: Option<f32>| match (peak, value) {
            (Some(p), Some(v)) => Some(p.max(v)),
            (p, v) => p.or(v),
        };
        if obs.game.is_seen() {
            self.hp_peak = higher(self.hp_peak, hp);
            self.exp_peak = higher(self.exp_peak, exp);
        }
        let is_alert = |a: &Action| matches!(a, Action::Say(s) if s.kind == Kind::Alert);
        let alerts = out.iter().filter(|a| is_alert(a)).count() as u32;
        if alerts == 0 {
            return;
        }
        let above = |peak: Option<f32>, then: Option<f32>, by: f32| matches!((peak, then), (Some(peak), Some(then)) if peak >= then + by);
        let potted = above(self.hp_peak, self.hp_at_alert, POTTED);
        let gained = above(self.exp_peak, self.exp_at_alert, EXP_GAINED);
        let leveled = obs.level.is_some() && obs.level != self.level_at_alert;
        if self.spoke_at > self.alert_at || potted || gained || leveled {
            self.unanswered = 0;
            self.hold_until = f64::NEG_INFINITY;
        }
        if now < self.hold_until {
            out.retain(|a| !is_alert(a));
            return;
        }
        if self.hold_until.is_finite() {
            // The hold ended with nothing answering: a couple come through.
            self.hold_until = f64::NEG_INFINITY;
            self.unanswered = UNANSWERED_MAX - AFTER_HOLD;
        }
        self.unanswered += alerts;
        self.alert_at = now;
        self.hp_at_alert = hp;
        self.hp_peak = hp;
        self.exp_at_alert = exp;
        self.exp_peak = exp;
        self.level_at_alert = obs.level;
        if self.unanswered > UNANSWERED_MAX {
            out.retain(|a| !is_alert(a));
            self.hold_until = now + HOLD_SECS;
            if now - self.hold_told >= HOLD_TOLD_EVERY {
                self.hold_told = now;
                out.push(Action::Say(Say::info(
                    "You're not answering, so I'll hold my warnings until you say something.",
                    true,
                )));
            }
        }
    }

    fn track_window(&mut self, now: f64, obs: &Observation, out: &mut Vec<Action>) {
        if let GameView::Seen(title) = &obs.game {
            if !self.ever_seen {
                self.ever_seen = true;
                // The window's title, when it is not plainly the game's: a
                // player once heard "I can see MapleStory" with the game
                // closed, and the log did not say what had been taken for it.
                let line = if title.trim().eq_ignore_ascii_case("maplestory") {
                    "I can see MapleStory.".to_string()
                } else {
                    format!("I can see MapleStory (the window \"{}\").", title.trim())
                };
                out.push(Action::Say(Say::info(line, true)));
            } else if self.announced_lost {
                out.push(Action::Say(Say::info("I can see the game again.", true)));
            }
            self.announced_lost = false;
            self.seen_at = Some(now);
            return;
        }
        if let Some(seen) = self.seen_at
            && !self.announced_lost
            && now - seen >= self.settings.lost_after
        {
            self.announced_lost = true;
            let why = match &obs.game {
                GameView::Unavailable(reason) => format!("I lost sight of the game: {reason}."),
                _ => "I lost sight of the game window.".to_string(),
            };
            out.push(Action::Say(Say::info(why, true)));
        }
    }

    fn watch_hp(&mut self, now: f64, obs: &Observation, out: &mut Vec<Action>) {
        let Some(hp) = obs.hp else {
            return;
        };
        // Readings swinging back and forth are a bar being guessed at, not
        // read: nothing is said from them.
        if self.hp_steady.unsteady(now, hp.percent) {
            self.falling_frames = 0;
            self.hp_low_frames = 0;
            self.zero_hp_frames = 0;
            if now - self.hp_steady.noted >= UNSTEADY_NOTE_EVERY {
                self.hp_steady.noted = now;
                out.push(Action::Say(Say::info(
                    "My HP readings are jumping around, so I'm holding the HP warnings until they settle.",
                    false,
                )));
            }
            return;
        }
        // A death: HP at zero for a moment (longer when that is the bar's
        // fill rather than the printed number: a dialog over the bar reads
        // as empty too).
        if hp.percent <= 0.5 {
            if self.zero_hp_frames == 0 {
                self.zero_hp_since = now;
            }
            self.zero_hp_frames += 1;
            let held = if hp.read { 0.0 } else { ZERO_HOLD_SECS };
            if self.zero_hp_frames >= 3 && now - self.zero_hp_since >= held && !self.dead {
                self.dead = true;
                let mut line = self
                    .settings
                    .attitude
                    .pick(
                        [
                            &["Your HP hit zero. Time to revive and head back."],
                            &["You died. Revive and get back in there."],
                            &[
                                "Dead. Wow. Revive and try not to suck this time.",
                                "You died, genius. Revive and get back in.",
                            ],
                        ],
                        self.warnings,
                    )
                    .to_string();
                if let Some(sooner) = self.sooner_warning(now) {
                    self.settings.hp_low = sooner;
                    self.settings.hp_rearm = (sooner + 15.0).min(95.0);
                    line.push_str(&format!(
                        " I'll warn you sooner from now on, under {sooner:.0}%."
                    ));
                }
                out.push(Action::Say(Say::alert(line)));
            }
            return;
        }
        self.zero_hp_frames = 0;
        self.hp_lately.push_back((now, hp.percent));
        while self.hp_lately.front().is_some_and(|(t, _)| now - t > 10.0) {
            self.hp_lately.pop_front();
        }
        if self.dead && hp.percent > 10.0 {
            self.dead = false;
            self.hp_warning = Warning::Armed;
        }
        // A beating: HP falling fast, said as it happens — the one thing
        // worth interrupting for, and never worth a model's wait.
        let highest_lately = self
            .hp_lately
            .iter()
            .filter(|(t, _)| now - t <= FALL_SECS)
            .map(|(_, p)| *p)
            .fold(0.0, f32::max);
        if highest_lately - hp.percent >= FALL_POINTS && hp.percent < 70.0 {
            self.falling_frames += 1;
        } else {
            self.falling_frames = 0;
        }
        if self.falling_frames >= 2 && now - self.fall_told >= FALL_COOLDOWN && !self.dead {
            self.fall_told = now;
            let line = self
                .settings
                .attitude
                .pick(
                    [
                        &[
                            "Whoa, you're taking a beating. Back off and pot!",
                            "Careful, your HP's dropping fast. Back off!",
                        ],
                        &[
                            "Back off, you're getting shredded.",
                            "Get out of there. Pot.",
                        ],
                        &[
                            "Back off, idiot, you're getting shredded.",
                            "Move! You're melting, genius.",
                        ],
                    ],
                    self.warnings,
                )
                .to_string();
            self.warnings += 1;
            // It said to pot: the low warning would only say so again.
            if hp.percent < self.settings.hp_low {
                self.hp_warning = Warning::Warned(now);
            }
            out.push(Action::Say(Say::alert(line)));
        }
        if hp.percent < self.settings.hp_low {
            if self.hp_low_frames == 0 {
                self.hp_low_since = now;
            }
            self.hp_low_frames += 1;
        } else {
            self.hp_low_frames = 0;
        }
        if hp.percent >= self.settings.hp_rearm {
            self.hp_warning = Warning::Armed;
        }
        let due = match self.hp_warning {
            Warning::Armed => true,
            Warning::Warned(at) => now - at >= self.settings.warning_cooldown * 3.0,
        };
        // (Not right after the beating was called: that said to pot.)
        let due = due && now - self.fall_told >= 5.0;
        if self.hp_low_frames >= 3 && now - self.hp_low_since >= LOW_HOLD_SECS && due {
            self.hp_warning = Warning::Warned(now);
            let amount = low_words(hp);
            let line = self
                .settings
                .attitude
                .pick(
                    [
                        &[
                            "Careful, your HP's down to {}. Drink a potion!",
                            "HP's at {}, potion time!",
                            "Whoa, {} HP. Drink something!",
                        ],
                        &["HP {}. Pot now!", "Pot! You're at {}.", "{} HP. Drink!"],
                        &[
                            "{} HP. Drink, you idiot!",
                            "Pot NOW, you're at {}, genius.",
                            "{} HP. Are you trying to die?",
                        ],
                    ],
                    self.warnings,
                )
                .replace("{}", &amount);
            self.warnings += 1;
            out.push(Action::Say(Say::alert(line)));
        }
    }

    /// Died without a warning, though HP went down through where a sooner
    /// one would have come: warn sooner from now on (five points, up to
    /// half the bar). Not when warnings are off, nor after a sudden death
    /// (no warning would have helped).
    fn sooner_warning(&self, now: f64) -> Option<f32> {
        let low = self.settings.hp_low;
        if low <= 0.0 || low >= SOONEST_WARNING {
            return None;
        }
        let warned = matches!(self.hp_warning, Warning::Warned(at) if now - at <= 15.0);
        let lowest = self
            .hp_lately
            .iter()
            .filter(|(t, _)| now - t <= 10.0)
            .map(|(_, p)| *p)
            .fold(f32::INFINITY, f32::min);
        (!warned && lowest < low + 20.0).then(|| (low + 5.0).min(SOONEST_WARNING))
    }

    fn watch_mp(&mut self, now: f64, obs: &Observation, out: &mut Vec<Action>) {
        let Some(mp) = obs.mp else {
            return;
        };
        if self.mp_steady.unsteady(now, mp.percent) {
            self.mp_low_frames = 0;
            if now - self.mp_steady.noted >= UNSTEADY_NOTE_EVERY {
                self.mp_steady.noted = now;
                out.push(Action::Say(Say::info(
                    "My MP readings are jumping around, so I'm holding the MP warnings until they settle.",
                    false,
                )));
            }
            return;
        }
        if mp.percent < self.settings.mp_low {
            if self.mp_low_frames == 0 {
                self.mp_low_since = now;
            }
            self.mp_low_frames += 1;
        } else {
            self.mp_low_frames = 0;
        }
        if mp.percent >= self.settings.mp_rearm {
            self.mp_warning = Warning::Armed;
        }
        let due = match self.mp_warning {
            Warning::Armed => true,
            Warning::Warned(at) => now - at >= self.settings.warning_cooldown * 3.0,
        };
        if self.mp_low_frames >= 3 && now - self.mp_low_since >= LOW_HOLD_SECS && due && !self.dead
        {
            self.mp_warning = Warning::Warned(now);
            let amount = low_words(mp);
            let line = self
                .settings
                .attitude
                .pick(
                    [
                        &[
                            "Your MP's down to {}.",
                            "MP's at {}, might want a potion.",
                            "Heads up, only {} MP left.",
                        ],
                        &["MP {}. Pot.", "Mana's at {}. Drink.", "{} MP left. Drink."],
                        &[
                            "{} MP. Drink before you're useless.",
                            "Out of mana again? {}. Pot, genius.",
                            "{} MP. Drink something, clown.",
                        ],
                    ],
                    self.warnings,
                )
                .replace("{}", &amount);
            self.warnings += 1;
            out.push(Action::Say(Say::alert(line)));
        }
    }

    /// The level, from the number read at the bottom left of the screen
    /// ("Lv. 165"), and the EXP bar for the pace. A level-up is that number
    /// going up by one, for the same character, once the new reading has
    /// held for a moment — then, and only then, is it said. The EXP bar
    /// wrapping says nothing on its own (it has the sight read the number
    /// again, and the number says); a number that jumps, falls, or comes
    /// with another name is another character or a misread: taken, not
    /// celebrated.
    fn watch_progress(&mut self, now: f64, obs: &Observation, out: &mut Vec<Action>) {
        if let Some(level) = obs.level {
            match self.level_candidate {
                Some((candidate, since)) if candidate == level => {
                    if now - since >= LEVEL_HOLD_SECS && self.last_level != Some(level) {
                        let before = self.last_level;
                        let same_character = match (&self.last_name, &obs.name) {
                            (Some(then), Some(now)) => then == now,
                            _ => true,
                        };
                        self.last_level = Some(level);
                        if obs.name.is_some() {
                            self.last_name = obs.name.clone();
                        }
                        let rose_by_one = before.is_some_and(|b| level == b + 1);
                        if rose_by_one && same_character {
                            // (The EXP bar may have counted this one already.)
                            self.exp.level_rose(now);
                            if now - self.announced_level_up >= 30.0 {
                                self.announced_level_up = now;
                                let text = self
                                    .settings
                                    .attitude
                                    .pick(
                                        [
                                            &["Level up! You're level {}."],
                                            &["Level {}! Nice."],
                                            &[
                                                "Level {}. Took you long enough.",
                                                "Level {}. Finally.",
                                            ],
                                        ],
                                        self.warnings,
                                    )
                                    .replace("{}", &level.to_string());
                                out.push(Action::Say(Say::alert(text)));
                            }
                        } else if let Some(before) = before {
                            let why = if !same_character {
                                "another character"
                            } else if level > before {
                                "not one level up: not celebrated"
                            } else {
                                "a lower level: another character, or misread"
                            };
                            out.push(Action::Say(Say::info(
                                format!("Level {level} now, from {before} ({why})."),
                                false,
                            )));
                        }
                    }
                }
                _ => self.level_candidate = Some((level, now)),
            }
        } else if self.last_name.is_none() && obs.name.is_some() {
            self.last_name = obs.name.clone();
        }
        // The EXP bar: for the pace and the time to the next level. Its
        // wrapping counts a level for the total, and says nothing.
        if let Some(exp) = obs.exp {
            self.exp.add(now, exp.percent as f64);
        }
    }

    /// A sentence the phone heard. Returns what to do about it, starting
    /// with the sentence itself for the log.
    pub fn heard(&mut self, now: f64, sentence: &str) -> Vec<Action> {
        self.now = now.max(self.now);
        let sentence = sentence.trim();
        if sentence.is_empty() {
            return Vec::new();
        }
        let mut out = vec![Action::Say(Say {
            kind: Kind::Heard,
            text: sentence.to_string(),
            speak: false,
        })];
        let Some(sentence) = self.strip_echo(now, sentence) else {
            return out;
        };
        let sentence = sentence.as_str();
        let addressed = self.settings.always_listen || self.listening(now);
        match commands::interpret(sentence, addressed) {
            Heard::NotForUs => {}
            Heard::WakeOnly => {
                self.listening_until = now + self.settings.listen_for;
                out.push(Action::Say(Say::reply("Yes?")));
            }
            Heard::Command(command) => {
                self.listening_until = f64::NEG_INFINITY;
                out.extend(self.command(now, command));
            }
            Heard::Unclear(rest) => {
                self.listening_until = f64::NEG_INFINITY;
                self.turn += 1;
                let answer = match chat::small_talk(&rest).or_else(|| chat::small_talk(sentence)) {
                    Some(talk) => Some(chat::answer(talk, self.last.as_ref(), self.turn)),
                    None => chat::fallback(&rest, self.turn),
                };
                if let Some(answer) = answer {
                    out.push(Action::Say(Say::reply(answer)));
                }
            }
        }
        out
    }

    /// A command, from a sentence or a button.
    pub fn command(&mut self, now: f64, command: Command) -> Vec<Action> {
        self.now = now.max(self.now);
        let obs = self.last.clone();
        let seen = obs.as_ref().is_some_and(|o| o.game.is_seen());
        let reply = |text: String| vec![Action::Say(Say::reply(text))];
        if !seen
            && matches!(
                command,
                Command::Status | Command::Hp | Command::Mp | Command::Exp | Command::Level
            )
        {
            return reply("I can't see the game right now.".to_string());
        }
        let obs = obs.unwrap_or_else(|| Observation::unseen(GameView::NotFound));
        match command {
            Command::Status => reply(status_line(&obs)),
            Command::Hp => reply(gauge_line("HP", obs.hp)),
            Command::Mp => reply(gauge_line("MP", obs.mp)),
            Command::Exp => reply(gauge_line("EXP", obs.exp)),
            Command::Level => reply(match obs.level {
                Some(level) => format!("You're level {level}."),
                None => "I can't read your level right now.".to_string(),
            }),
            Command::Rate => reply(self.rate_line()),
            Command::Session => reply(format!(
                "This session has been running for {}.",
                spoken_duration(now)
            )),
            Command::Mark => {
                self.marks += 1;
                vec![
                    Action::Mark,
                    Action::Say(Say::reply(format!("Marked. That's mark {}.", self.marks))),
                ]
            }
            Command::Mute => {
                self.muted = true;
                vec![
                    Action::Say(Say::info("Muted. I'll keep writing to your phone.", false)),
                    Action::SetMuted(true),
                ]
            }
            Command::Unmute => {
                self.muted = false;
                vec![
                    Action::SetMuted(false),
                    Action::Say(Say::info("I'm back.", true)),
                ]
            }
            Command::Help => reply(if self.settings.always_listen {
                "Just talk to me. Ask how you're doing, about your HP, MP, EXP or level, or how long until you level. Say mark to save a moment, or mute to quiet me."
                    .to_string()
            } else {
                "Say syrup, then: status, HP, MP, EXP, rate, level, time, mark, mute or unmute."
                    .to_string()
            }),
        }
    }

    fn rate_line(&self) -> String {
        match (self.exp.per_hour(), self.exp.seconds_to_level()) {
            (Some(rate), Some(eta)) => format!(
                "About {} EXP an hour. At this pace you level up in {}.",
                percent_amount(rate),
                spoken_duration(eta)
            ),
            (Some(rate), None) => format!(
                "About {} EXP an hour, so no level-up at this pace.",
                percent_amount(rate)
            ),
            _ => "Give me a couple of minutes of play to measure your EXP rate.".to_string(),
        }
    }
}

/// "82 percent", or "about 82 percent" for a bar estimate.
/// A low bar the way a person says it: "about 12 percent" (whole numbers;
/// "about" when it was measured from the bar rather than read).
fn low_words(gauge: Gauge) -> String {
    let amount = format!("{} percent", (gauge.percent.round() as i64).max(1));
    if gauge.read {
        amount
    } else {
        format!("about {amount}")
    }
}

fn percent_words(gauge: Gauge) -> String {
    let amount = percent_amount(gauge.percent as f64);
    if gauge.read {
        amount
    } else {
        format!("about {amount}")
    }
}

/// "82 percent", "4.5 percent", "0.25 percent".
fn percent_amount(value: f64) -> String {
    let magnitude = value.abs();
    let number = if magnitude >= 10.0 {
        format!("{value:.0}")
    } else if magnitude >= 1.0 {
        format!("{value:.1}")
    } else {
        format!("{value:.2}")
    };
    let number = if number.contains('.') {
        number
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string()
    } else {
        number
    };
    format!("{number} percent")
}

fn gauge_line(name: &str, gauge: Option<Gauge>) -> String {
    match gauge {
        Some(g) => match (g.current, g.max) {
            (Some(current), Some(max)) if g.read => {
                format!("{name} {current} of {max}, {}.", percent_words(g))
            }
            _ => format!("{name} {}.", percent_words(g)),
        },
        None => format!("I can't see your {name} bar right now."),
    }
}

fn status_line(obs: &Observation) -> String {
    let mut parts = Vec::new();
    if let Some(level) = obs.level {
        parts.push(format!("Level {level}"));
    }
    for (name, gauge) in [("HP", obs.hp), ("MP", obs.mp), ("EXP", obs.exp)] {
        if let Some(g) = gauge {
            parts.push(format!("{name} {}", percent_words(g)));
        }
    }
    if parts.is_empty() {
        "I can see the game, but not your HP, MP or EXP bars.".to_string()
    } else {
        format!("{}.", parts.join(", "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gauge(percent: f32, read: bool) -> Option<Gauge> {
        Some(Gauge {
            percent,
            current: None,
            max: None,
            read,
        })
    }

    fn frame(hp: f32, mp: f32, exp: f32) -> Observation {
        Observation {
            game: GameView::Seen("MapleStory".into()),
            hp: gauge(hp, false),
            mp: gauge(mp, false),
            exp: gauge(exp, true),
            level: Some(57),
            name: None,
            job: None,
        }
    }

    fn said(actions: &[Action]) -> Vec<String> {
        actions
            .iter()
            .filter_map(|a| match a {
                Action::Say(s) if s.kind != Kind::Heard => Some(s.text.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn announces_the_game_once_and_its_loss_after_a_while() {
        let mut c = Companion::new(Settings::default());
        assert_eq!(
            said(&c.observe(0.0, frame(90.0, 90.0, 10.0))),
            ["I can see MapleStory."]
        );
        assert!(said(&c.observe(0.1, frame(90.0, 90.0, 10.0))).is_empty());
        assert!(said(&c.observe(2.0, Observation::unseen(GameView::NotFound))).is_empty());
        assert_eq!(
            said(&c.observe(5.5, Observation::unseen(GameView::NotFound))),
            ["I lost sight of the game window."]
        );
        assert!(said(&c.observe(9.0, Observation::unseen(GameView::NotFound))).is_empty());
        assert_eq!(
            said(&c.observe(10.0, frame(90.0, 90.0, 10.0))),
            ["I can see the game again."]
        );
    }

    #[test]
    fn low_hp_is_said_once_until_it_recovers() {
        let mut c = Companion::new(Settings::default());
        // Worn down slowly (a beating is called sooner, and otherwise).
        c.observe(0.0, frame(90.0, 90.0, 10.0));
        c.observe(5.0, frame(70.0, 90.0, 10.0));
        c.observe(10.0, frame(50.0, 90.0, 10.0));
        c.observe(15.0, frame(35.0, 90.0, 10.0));
        // A moment low is not enough (a dialog over the bar, a misread).
        assert!(said(&c.observe(21.0, frame(20.0, 90.0, 10.0))).is_empty());
        assert!(said(&c.observe(21.1, frame(20.0, 90.0, 10.0))).is_empty());
        assert!(said(&c.observe(21.3, frame(20.0, 90.0, 10.0))).is_empty());
        assert_eq!(
            said(&c.observe(21.7, frame(20.0, 90.0, 10.0))),
            ["Careful, your HP's down to about 20 percent. Drink a potion!"]
        );
        for i in 0..20 {
            assert!(said(&c.observe(21.8 + i as f64, frame(18.0, 90.0, 10.0))).is_empty());
        }
        // Recovered, then worn down again: warned again, put another way.
        c.observe(50.0, frame(80.0, 90.0, 10.0));
        c.observe(54.0, frame(60.0, 90.0, 10.0));
        c.observe(58.0, frame(40.0, 90.0, 10.0));
        c.observe(62.0, frame(20.0, 90.0, 10.0));
        c.observe(62.3, frame(20.0, 90.0, 10.0));
        assert_eq!(
            said(&c.observe(62.7, frame(20.0, 90.0, 10.0))),
            ["HP's at about 20 percent, potion time!"]
        );
    }

    #[test]
    fn a_beating_is_called_as_it_happens_and_the_low_warning_waits_its_turn() {
        let mut c = Companion::new(Settings::default());
        c.observe(0.0, frame(95.0, 90.0, 10.0));
        for i in 1..10 {
            assert!(said(&c.observe(i as f64 * 0.1, frame(95.0, 90.0, 10.0))).is_empty());
        }
        // Down 30 points in a second: called on the second frame that says
        // so, well before HP is low.
        assert!(said(&c.observe(1.5, frame(65.0, 90.0, 10.0))).is_empty());
        assert_eq!(
            said(&c.observe(1.6, frame(64.0, 90.0, 10.0))),
            ["Whoa, you're taking a beating. Back off and pot!"]
        );
        // Still falling: not said again for a while…
        assert!(said(&c.observe(2.0, frame(40.0, 90.0, 10.0))).is_empty());
        assert!(said(&c.observe(2.2, frame(35.0, 90.0, 10.0))).is_empty());
        // …and the low warning, which would only say to pot again, waits a
        // few seconds, then comes.
        assert!(said(&c.observe(3.0, frame(25.0, 90.0, 10.0))).is_empty());
        assert!(said(&c.observe(3.3, frame(25.0, 90.0, 10.0))).is_empty());
        assert!(said(&c.observe(3.7, frame(25.0, 90.0, 10.0))).is_empty());
        assert!(said(&c.observe(6.5, frame(25.0, 90.0, 10.0))).is_empty());
        assert_eq!(
            said(&c.observe(6.7, frame(25.0, 90.0, 10.0))),
            ["HP's at about 25 percent, potion time!"]
        );
        // A frame's misreading (a dialog over the bar) is not a beating.
        let mut quiet = Companion::new(Settings::default());
        quiet.observe(0.0, frame(95.0, 90.0, 10.0));
        assert!(said(&quiet.observe(0.5, frame(95.0, 90.0, 10.0))).is_empty());
        assert!(said(&quiet.observe(0.6, frame(50.0, 90.0, 10.0))).is_empty());
        assert!(said(&quiet.observe(0.7, frame(95.0, 90.0, 10.0))).is_empty());
        assert!(said(&quiet.observe(1.0, frame(95.0, 90.0, 10.0))).is_empty());
        // Slow attrition is the low warning's business, not a beating.
        let mut slow = Companion::new(Settings::default());
        slow.observe(0.0, frame(95.0, 90.0, 10.0));
        for i in 1..60 {
            let hp = 95.0 - i as f32;
            let lines = said(&slow.observe(i as f64 * 0.5, frame(hp, 90.0, 10.0)));
            assert!(lines.is_empty() || hp < 30.0, "{i}: {lines:?}");
        }
    }

    #[test]
    fn staying_low_is_repeated_only_after_a_long_while() {
        let mut c = Companion::new(Settings::default());
        c.observe(0.0, frame(20.0, 90.0, 10.0));
        c.observe(0.3, frame(20.0, 90.0, 10.0));
        assert_eq!(said(&c.observe(0.7, frame(20.0, 90.0, 10.0))).len(), 1);
        assert!(said(&c.observe(30.0, frame(20.0, 90.0, 10.0))).is_empty());
        assert_eq!(said(&c.observe(61.0, frame(20.0, 90.0, 10.0))).len(), 1);
    }

    #[test]
    fn low_mp_is_its_own_warning() {
        let mut c = Companion::new(Settings::default());
        c.observe(0.0, frame(90.0, 3.1, 10.0));
        c.observe(0.3, frame(90.0, 3.1, 10.0));
        assert_eq!(
            said(&c.observe(0.7, frame(90.0, 3.1, 10.0))),
            ["Your MP's down to about 3 percent."]
        );
    }

    #[test]
    fn talking_over_it_is_told_from_its_own_voice_coming_back() {
        let mut c = Companion::new(Settings::default());
        c.remember_spoken(
            10.0,
            "You're at 60.23% EXP, about twenty minutes to the next level.",
        );
        // Only its own voice, as the phone writes it (numbers its own way).
        assert_eq!(
            c.barge_in(11.0, "you're at sixty point two three percent EXP"),
            None
        );
        assert_eq!(c.barge_in(11.0, "about twenty minutes to the"), None);
        // A sound or a single word is not enough…
        assert_eq!(c.barge_in(11.5, "minutes to the next level um"), None);
        assert_eq!(c.barge_in(11.5, "minutes to the next level okay"), None);
        // …two words of the player's are, and so is "wait".
        assert_eq!(
            c.barge_in(11.5, "to the next level what about my HP"),
            Some("what about my HP".to_string())
        );
        assert_eq!(
            c.barge_in(11.5, "twenty minutes wait"),
            Some("wait".to_string())
        );
        assert_eq!(c.barge_in(11.5, "רגע"), Some("רגע".to_string()));
        assert_eq!(
            c.barge_in(11.5, "等等，我想问"),
            Some("等等，我想问".to_string())
        );
        // Nothing said lately: any two words are the player's.
        let quiet = Companion::new(Settings::default());
        assert_eq!(
            quiet.barge_in(5.0, "with the quest"),
            Some("with the quest".to_string())
        );
        assert_eq!(quiet.barge_in(5.0, "uh"), None);
        // Still talking while the answer is being made: one word will do.
        assert!(quiet.still_talking(5.0, "now"));
        assert!(!quiet.still_talking(5.0, "um"));
        assert!(!c.still_talking(11.5, "next level"));
    }

    #[test]
    fn what_was_heard_over_it_needs_enough_of_the_players_own_words() {
        let mut c = Companion::new(Settings::default());
        c.remember_spoken(10.0, "Take the Strange Bottle of Water to the blue pillar.");
        // Its own words, garbled a little: not the player.
        assert_eq!(
            c.own_words(
                12.0,
                "take the strange bottles of water to the blue pillar",
                2
            ),
            None
        );
        // The player's question after its words: theirs.
        assert_eq!(
            c.own_words(12.0, "to the blue pillar where is that pillar", 2),
            Some("where is that pillar".to_string())
        );
        // Just after it spoke, a quick answer is the player's…
        assert_eq!(c.own_words(13.0, "yes", 1), Some("yes".to_string()));
        // …but not the tail of its own line coming back late.
        assert_eq!(c.own_words(13.0, "the blue pillars", 1), None);
        // Not over it: a short answer stands.
        assert_eq!(
            c.own_words(20.0, "okay thanks", 0),
            Some("okay thanks".to_string())
        );
    }

    #[test]
    fn a_death_is_said_once() {
        let mut c = Companion::new(Settings::default());
        c.observe(0.0, frame(50.0, 50.0, 10.0));
        let mut lines = Vec::new();
        // From the bar's fill, a death must last two seconds (a dialog
        // over the bar reads as empty too).
        for i in 0..15 {
            lines.extend(said(
                &c.observe(1.0 + i as f64 * 0.1, frame(0.0, 50.0, 10.0)),
            ));
        }
        assert!(lines.is_empty(), "{lines:?}");
        for i in 15..40 {
            lines.extend(said(
                &c.observe(1.0 + i as f64 * 0.1, frame(0.0, 50.0, 10.0)),
            ));
        }
        assert_eq!(lines, ["Your HP hit zero. Time to revive and head back."]);
        // From the printed number, at once.
        let mut c = Companion::new(Settings::default());
        c.observe(0.0, frame(50.0, 50.0, 10.0));
        let mut lines = Vec::new();
        for i in 0..5 {
            let mut dead = frame(0.0, 50.0, 10.0);
            dead.hp = Some(Gauge {
                percent: 0.0,
                current: Some(0),
                max: Some(9795),
                read: true,
            });
            lines.extend(said(&c.observe(1.0 + i as f64 * 0.1, dead)));
        }
        assert_eq!(lines, ["Your HP hit zero. Time to revive and head back."]);
    }

    #[test]
    fn a_bar_whose_readings_jump_around_is_not_warned_from() {
        // What a misread HP bar gave one night: 3%, 46%, 9%, 11%, 3%, 40%…
        // ten times a second, with HP in truth full. Not one warning.
        let mut c = Companion::new(Settings::default());
        c.observe(0.0, frame(95.0, 90.0, 10.0));
        let noise = [3.0, 46.0, 9.0, 11.0, 3.0, 40.0, 5.0, 28.0, 46.0, 9.0];
        let mut lines: Vec<(f64, String)> = Vec::new();
        for i in 1..6000 {
            let t = i as f64 * 0.1;
            let hp = noise[i % noise.len()];
            for line in said(&c.observe(t, frame(hp, 90.0, 10.0))) {
                lines.push((t, line));
            }
        }
        // The very first swing can pass for a beating (nothing is known
        // yet); after that, not one warning in ten minutes.
        let alerts: Vec<&(f64, String)> = lines
            .iter()
            .filter(|(_, l)| !l.starts_with("My HP readings"))
            .collect();
        assert!(alerts.len() <= 1, "{alerts:?}");
        assert!(alerts.iter().all(|(t, _)| *t < 1.0), "{alerts:?}");
        // The player is told why it is quiet, on the phone, not aloud, and
        // not more than once in ten minutes.
        let notes = lines.len() - alerts.len();
        assert!((1..=2).contains(&notes), "{lines:?}");
        assert_eq!(
            lines
                .iter()
                .find(|(_, l)| l.starts_with("My HP"))
                .map(|(_, l)| l.as_str()),
            Some(
                "My HP readings are jumping around, so I'm holding the HP warnings until they settle."
            )
        );
        // Steady again for a while (the hold has grown long by now: the
        // bar must stay put for it to end): a real fall is warned about.
        let mut t = 600.0;
        while t < 1300.0 {
            let lines = said(&c.observe(t, frame(95.0, 90.0, 10.0)));
            assert!(
                lines.iter().all(|l| l.starts_with("My HP readings")),
                "{t}: {lines:?}"
            );
            t += 0.1;
        }
        c.observe(t, frame(60.0, 90.0, 10.0));
        let beating = said(&c.observe(t + 0.1, frame(58.0, 90.0, 10.0)));
        assert_eq!(beating.len(), 1, "{beating:?}");
        assert!(beating[0].contains("Back off"), "{beating:?}");
        // A fight — hit, potion, hit, potion — is not a misread.
        let mut c = Companion::new(Settings::default());
        c.observe(0.0, frame(95.0, 90.0, 10.0));
        c.observe(1.0, frame(55.0, 90.0, 10.0));
        c.observe(1.1, frame(55.0, 90.0, 10.0));
        c.observe(3.0, frame(95.0, 90.0, 10.0));
        c.observe(5.0, frame(60.0, 90.0, 10.0));
        assert!(!c.hp_steady.unsteady(5.1, 60.0));
    }

    #[test]
    fn reversals_are_counted_by_the_turn() {
        assert_eq!(reversals([90.0, 60.0, 30.0, 10.0], 20.0), 0);
        assert_eq!(reversals([90.0, 60.0, 95.0], 20.0), 1);
        // Up, down (through 9, 11 and 3: one move), up, down.
        assert_eq!(reversals([3.0, 46.0, 9.0, 11.0, 3.0, 40.0], 20.0), 2);
        assert_eq!(reversals([3.0, 46.0, 9.0, 11.0, 3.0, 40.0, 5.0], 20.0), 3);
        // Small wobbles are not turns.
        assert_eq!(reversals([50.0, 55.0, 48.0, 56.0, 47.0, 58.0], 20.0), 0);
        assert_eq!(reversals([], 20.0), 0);
    }

    #[test]
    fn warnings_nobody_answers_are_held_and_a_word_lets_them_through_again() {
        // HP at 20% for half an hour and nothing done about it (the player
        // is away, or the bar is misread): the low warning comes every
        // minute six times, then one line saying the rest will wait, then
        // nothing for ten minutes; then two more, and quiet again.
        let mut c = Companion::new(Settings::default());
        let mut lines: Vec<(f64, String)> = Vec::new();
        for i in 0..18_600 {
            let t = i as f64 * 0.1;
            for line in said(&c.observe(t, frame(20.0, 90.0, 10.0))) {
                if line != "I can see MapleStory." {
                    lines.push((t, line));
                }
            }
        }
        let texts: Vec<&str> = lines.iter().map(|(_, l)| l.as_str()).collect();
        assert_eq!(texts.len(), 11, "{lines:?}");
        assert!(texts[..6].iter().all(|l| l.contains("HP")), "{texts:?}");
        assert_eq!(
            texts[6],
            "You're not answering, so I'll hold my warnings until you say something."
        );
        // Six warnings a minute apart, then the hold ends ten minutes later.
        assert!((lines[5].0 - lines[0].0 - 300.0).abs() < 2.0, "{lines:?}");
        assert!(lines[7].0 - lines[6].0 >= 600.0, "{lines:?}");
        assert!(texts[7].contains("HP") && texts[8].contains("HP"));
        // (The hold is not announced again so soon.)
        assert!(texts[9].contains("HP"), "{texts:?}");
        assert!(c.alerts_held(1860.0));
        // The player says something: the warnings come again.
        c.player_spoke(1860.0);
        assert!(!c.alerts_held(1860.0));
        let mut after = Vec::new();
        for i in 0..500 {
            after.extend(said(
                &c.observe(1860.0 + i as f64 * 0.1, frame(20.0, 90.0, 10.0)),
            ));
        }
        assert_eq!(after.len(), 1, "{after:?}");
    }

    #[test]
    fn a_potion_after_a_warning_is_an_answer() {
        // Low, warned, potted, low again, warned… twelve times: the player
        // is plainly there, and every warning is said.
        let mut c = Companion::new(Settings::default());
        let mut t = 0.0;
        let mut count = 0;
        let alerts = |actions: &[Action]| {
            actions
                .iter()
                .filter(|a| matches!(a, Action::Say(s) if s.kind == Kind::Alert))
                .count()
        };
        for _ in 0..12 {
            for _ in 0..20 {
                t += 0.1;
                count += alerts(&c.observe(t, frame(95.0, 90.0, 10.0)));
            }
            for _ in 0..20 {
                t += 0.1;
                count += alerts(&c.observe(t, frame(20.0, 90.0, 10.0)));
            }
        }
        assert_eq!(count, 12);
        assert!(!c.alerts_held(t));
    }

    #[test]
    fn a_death_no_warning_came_before_moves_the_warning_sooner() {
        let mut c = Companion::new(Settings::default());
        // Down fast through 40% and 32%: above 30%, so no warning came.
        let mut t = 0.0;
        let mut step = |c: &mut Companion, hp: f32, frames: usize| {
            let mut lines = Vec::new();
            for _ in 0..frames {
                t += 0.1;
                lines.extend(said(&c.observe(t, frame(hp, 50.0, 10.0))));
            }
            lines
        };
        step(&mut c, 80.0, 3);
        // (A fall that fast is called as a beating; the low warning, at
        // 30%, never came.)
        assert_eq!(
            step(&mut c, 40.0, 3),
            ["Whoa, you're taking a beating. Back off and pot!"]
        );
        assert!(step(&mut c, 32.0, 3).is_empty());
        // (A death read from the bar takes two seconds to believe.)
        assert_eq!(
            step(&mut c, 0.0, 25),
            [
                "Your HP hit zero. Time to revive and head back. I'll warn you sooner from now on, under 35%."
            ]
        );
        assert_eq!((c.settings.hp_low, c.settings.hp_rearm), (35.0, 50.0));
        // A sudden death from full HP: no warning would have helped.
        step(&mut c, 100.0, 120);
        assert_eq!(
            step(&mut c, 0.0, 25),
            ["Your HP hit zero. Time to revive and head back."]
        );
        assert_eq!(c.settings.hp_low, 35.0);
        // Warned on the way down: the warning came, nothing to change.
        step(&mut c, 100.0, 120);
        assert_eq!(step(&mut c, 20.0, 8).len(), 1);
        assert_eq!(
            step(&mut c, 0.0, 25),
            ["Your HP hit zero. Time to revive and head back."]
        );
        assert_eq!(c.settings.hp_low, 35.0);
        // Never past half the bar, and never when warnings are off.
        c.settings.hp_low = 0.0;
        step(&mut c, 100.0, 120);
        step(&mut c, 20.0, 8);
        step(&mut c, 0.0, 25);
        assert_eq!(c.settings.hp_low, 0.0);
        c.settings.hp_low = 48.0;
        step(&mut c, 100.0, 300);
        step(&mut c, 60.0, 3);
        step(&mut c, 0.0, 25);
        assert_eq!(c.settings.hp_low, 50.0);
    }

    #[test]
    fn a_level_up_is_celebrated_once() {
        let mut c = Companion::new(Settings::default());
        for i in 0..40 {
            c.observe(i as f64 * 0.1, frame(90.0, 90.0, 99.0));
        }
        let mut lines = Vec::new();
        for i in 0..60 {
            let mut next = frame(90.0, 90.0, 0.5);
            next.level = Some(58);
            lines.extend(said(&c.observe(4.0 + i as f64 * 0.1, next)));
        }
        // The number at the bottom left went from 57 to 58 and held for
        // three seconds: celebrated once, with the number.
        assert_eq!(lines, ["Level up! You're level 58."]);
        // The level itself was taken once it held.
        assert_eq!(said(&c.command(20.0, Command::Level)), ["You're level 58."]);
        // The EXP bar's wrap alone says nothing: only the number does.
        let mut c = Companion::new(Settings::default());
        for i in 0..40 {
            let mut full = frame(90.0, 90.0, 99.0);
            full.level = None;
            c.observe(i as f64 * 0.1, full);
        }
        let mut lines = Vec::new();
        for i in 0..100 {
            let mut next = frame(90.0, 90.0, 0.5);
            next.level = None;
            lines.extend(said(&c.observe(4.0 + i as f64 * 0.1, next)));
        }
        assert!(lines.is_empty(), "{lines:?}");
        // (It still counts for the session's total.)
        assert_eq!(c.progress().levels_gained, 1);
    }

    #[test]
    fn only_the_number_going_up_by_one_for_the_same_character_is_a_level_up() {
        let named = |level: u32, name: &str| {
            let mut f = frame(90.0, 90.0, 40.0);
            f.level = Some(level);
            f.name = Some(name.to_string());
            f
        };
        let hold = |c: &mut Companion, from: f64, obs: Observation| -> Vec<String> {
            let mut lines = Vec::new();
            for i in 0..40 {
                lines.extend(said(&c.observe(from + i as f64 * 0.1, obs.clone())));
            }
            lines.retain(|l| l != "I can see MapleStory.");
            lines
        };
        // Up by one, the same name: said.
        let mut c = Companion::new(Settings::default());
        assert!(hold(&mut c, 0.0, named(165, "WanWanBoggio")).is_empty());
        assert_eq!(
            hold(&mut c, 10.0, named(166, "WanWanBoggio")),
            ["Level up! You're level 166."]
        );
        // A jump of five: taken, not celebrated (and noted, not spoken).
        let lines = hold(&mut c, 60.0, named(171, "WanWanBoggio"));
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert!(lines[0].starts_with("Level 171 now, from 166"), "{lines:?}");
        assert_eq!(
            said(&c.command(70.0, Command::Level)),
            ["You're level 171."]
        );
        // Another character's level, one higher: not a level-up.
        let lines = hold(&mut c, 120.0, named(172, "Mule"));
        assert!(lines[0].contains("another character"), "{lines:?}");
        // Back to the first character: another character again, quietly…
        let lines = hold(&mut c, 180.0, named(171, "WanWanBoggio"));
        assert!(lines[0].contains("another character"), "{lines:?}");
        // …and the same character read lower is a misread (or a rebirth).
        let lines = hold(&mut c, 240.0, named(160, "WanWanBoggio"));
        assert!(lines[0].contains("lower level"), "{lines:?}");
        // A name on neither side: the number alone decides.
        let mut c = Companion::new(Settings::default());
        let mut unnamed = frame(90.0, 90.0, 40.0);
        unnamed.level = Some(57);
        assert!(hold(&mut c, 0.0, unnamed.clone()).is_empty());
        unnamed.level = Some(58);
        assert_eq!(hold(&mut c, 10.0, unnamed), ["Level up! You're level 58."]);
    }

    #[test]
    fn a_level_misread_for_a_moment_is_not_believed() {
        let mut c = Companion::new(Settings::default());
        for i in 0..40 {
            c.observe(i as f64 * 0.1, frame(90.0, 90.0, 50.0));
        }
        let mut lines = Vec::new();
        for i in 0..10 {
            let mut misread = frame(90.0, 90.0, 50.0);
            misread.level = Some(58);
            lines.extend(said(&c.observe(4.0 + i as f64 * 0.1, misread)));
        }
        for i in 0..40 {
            lines.extend(said(
                &c.observe(5.0 + i as f64 * 0.1, frame(90.0, 90.0, 50.0)),
            ));
        }
        assert!(lines.is_empty(), "{lines:?}");
    }

    #[test]
    fn a_level_read_rising_without_an_exp_fall_is_celebrated() {
        let mut c = Companion::new(Settings::default());
        for i in 0..40 {
            c.observe(i as f64 * 0.1, frame(90.0, 90.0, 30.0));
        }
        let mut lines = Vec::new();
        for i in 0..50 {
            let mut next = frame(90.0, 90.0, 40.0);
            next.level = Some(58);
            lines.extend(said(&c.observe(4.0 + i as f64 * 0.1, next)));
        }
        assert_eq!(lines, ["Level up! You're level 58."]);
    }

    #[test]
    fn with_the_wake_word_required_talk_is_ignored_and_addressed_commands_answered() {
        let mut c = Companion::new(Settings {
            always_listen: false,
            ..Settings::default()
        });
        c.observe(0.0, frame(82.0, 40.0, 13.25));
        assert!(said(&c.heard(1.0, "my hp is fine chat")).is_empty());
        assert_eq!(
            said(&c.heard(2.0, "syrup status")),
            ["Level 57, HP about 82 percent, MP about 40 percent, EXP 13 percent."]
        );
        assert_eq!(said(&c.heard(3.0, "Syrup?")), ["Yes?"]);
        assert_eq!(
            said(&c.heard(4.0, "what's my mana")),
            ["MP about 40 percent."]
        );
        // The follow-up window closes after one sentence.
        assert!(said(&c.heard(5.0, "what's my mana")).is_empty());
    }

    #[test]
    fn everything_said_is_answered_like_a_conversation() {
        let mut c = Companion::new(Settings::default());
        c.observe(0.0, frame(82.0, 40.0, 13.25));
        // What was said on the first real try, word for word.
        let hello = &said(&c.heard(1.0, "Hello"))[0];
        assert!(
            hello.contains("You're level 57, HP about 82 percent"),
            "{hello}"
        );
        assert!(
            said(&c.heard(2.0, "Can you see my maple"))[0].starts_with("Yes, I can see your game.")
        );
        assert!(said(&c.heard(3.0, "Why don't you answer me"))[0].starts_with("I'm here!"));
        assert_eq!(
            said(&c.heard(4.0, "what's my mana")),
            ["MP about 40 percent."]
        );
        // A long sentence to someone else gets nothing.
        assert!(
            said(&c.heard(
                5.0,
                "ok chat so today we are farming the monkey forest until sixty"
            ))
            .is_empty()
        );
    }

    #[test]
    fn its_own_voice_coming_back_is_not_answered() {
        let mut c = Companion::new(Settings::default());
        c.observe(0.0, frame(82.0, 40.0, 13.25));
        c.remember_spoken(1.0, "HP about 82 percent, MP about 40 percent.");
        // The phone heard the PC say it.
        assert!(c.is_echo(3.0, "HP about 82% MP about 40%"));
        assert!(said(&c.heard(3.0, "HP about 82% MP about 40%")).is_empty());
        // The player saying a word that was in it is not an echo.
        assert!(!c.is_echo(4.0, "HP"));
        assert!(!c.is_echo(4.0, "how about my exp"));
        // Long after, the same words are the player's.
        assert!(!c.is_echo(100.0, "HP about 82% MP about 40%"));
    }

    #[test]
    fn the_players_answer_is_kept_when_the_echo_runs_into_it() {
        let mut c = Companion::new(Settings::default());
        c.remember_spoken(
            1.0,
            "I’m doing great, just lounging in my pancake hat and keeping you company. How are you doing?",
        );
        // Its last words and the player's answer, heard as one sentence.
        assert_eq!(
            c.strip_echo(9.0, "How are you doing let's play").as_deref(),
            Some("let's play")
        );
        // Only its own words, even misheard in places.
        assert_eq!(c.strip_echo(4.0, "How are you"), None);
        assert_eq!(
            c.strip_echo(5.0, "I'm doing great just longing in my pancake hat"),
            None
        );
        // Nothing of it.
        assert_eq!(
            c.strip_echo(9.0, "let's go to the forest").as_deref(),
            Some("let's go to the forest")
        );
        // A word or two of it, said by the player, is the player's.
        assert_eq!(
            c.strip_echo(9.0, "my hat is great").as_deref(),
            Some("my hat is great")
        );
        assert_eq!(
            c.strip_echo(30.0, "you doing ok").as_deref(),
            Some("you doing ok")
        );
        // With AI off, the simple answers get the player's part only.
        assert!(!said(&c.heard(9.5, "How are you doing what's my hp")).is_empty());
    }

    #[test]
    fn the_player_quoting_it_to_correct_it_keeps_their_whole_sentence() {
        let mut c = Companion::new(Settings::default());
        c.remember_spoken(
            10.0,
            "You're at the Gate of the Future, level 165, EXP 74%.",
        );
        // One night this came back as "I am not in be super accurate…".
        let heard =
            "I am not in the gate of the future be super accurate and check everything you say";
        assert_eq!(c.strip_echo(14.0, heard).as_deref(), Some(heard));
        // Its own tail with the answer after it is still cut to the answer.
        assert_eq!(
            c.strip_echo(14.0, "gate of the future level 165 where should I go")
                .as_deref(),
            Some("where should I go")
        );
    }

    #[test]
    fn a_long_line_heard_back_with_a_few_words_written_differently_is_all_its_own() {
        let mut c = Companion::new(Settings::default());
        c.remember_spoken(
            1.0,
            "אני לא מצליח לקרוא את שם המפה מהמסך כרגע; נראה שאתה עדיין באזור של Monster Park Shuttle, אבל אני לא בטוח.",
        );
        assert_eq!(
            c.strip_echo(
                8.0,
                "אני לא מצליח לקרוא את שם המפה מהמסך כרגע נראה שאתה עדיין באזור של Monster פארק שקד אבל אני לא בטוח"
            ),
            None
        );
        // "Wait" over it still stops it.
        assert_eq!(
            c.strip_echo(
                8.0,
                "אני לא מצליח לקרוא את שם המפה מהמסך כרגע נראה שאתה עדיין באזור של Monster רגע עצור"
            )
            .as_deref(),
            Some("רגע עצור")
        );
    }

    #[test]
    fn printed_numbers_are_quoted_exactly() {
        let mut c = Companion::new(Settings::default());
        let mut obs = frame(0.0, 0.0, 0.0);
        obs.hp = Some(Gauge {
            percent: 95.5,
            current: Some(1291),
            max: Some(1351),
            read: true,
        });
        c.observe(0.0, obs);
        assert_eq!(
            said(&c.command(1.0, Command::Hp)),
            ["HP 1291 of 1351, 96 percent."]
        );
    }

    #[test]
    fn mute_and_mark() {
        let mut c = Companion::new(Settings::default());
        c.observe(0.0, frame(90.0, 90.0, 10.0));
        let actions = c.command(1.0, Command::Mute);
        assert!(actions.contains(&Action::SetMuted(true)));
        assert!(c.muted());
        let actions = c.command(2.0, Command::Mark);
        assert_eq!(actions[0], Action::Mark);
        assert_eq!(c.progress().marks, 1);
        let actions = c.command(3.0, Command::Unmute);
        assert!(actions.contains(&Action::SetMuted(false)));
        assert!(!c.muted());
    }

    #[test]
    fn questions_about_an_unseen_game_say_so() {
        let mut c = Companion::new(Settings::default());
        assert_eq!(
            said(&c.command(0.0, Command::Status)),
            ["I can't see the game right now."]
        );
        assert!(said(&c.command(0.0, Command::Session))[0].starts_with("This session"));
    }

    #[test]
    fn the_rate_needs_some_play_first() {
        let mut c = Companion::new(Settings::default());
        c.observe(0.0, frame(90.0, 90.0, 10.0));
        assert!(said(&c.command(1.0, Command::Rate))[0].starts_with("Give me"));
        for i in 0..=120 {
            let t = i as f64 * 5.0;
            c.observe(t, frame(90.0, 90.0, 10.0 + (t / 60.0 * 0.5) as f32));
        }
        let line = &said(&c.command(601.0, Command::Rate))[0];
        assert!(line.starts_with("About 30 percent EXP an hour."), "{line}");
        assert!(line.contains("level up in 2 hours"), "{line}");
    }

    #[test]
    fn percents_are_said_with_sensible_precision() {
        assert_eq!(percent_amount(82.4), "82 percent");
        assert_eq!(percent_amount(4.56), "4.6 percent");
        assert_eq!(percent_amount(4.0), "4 percent");
        assert_eq!(percent_amount(0.25), "0.25 percent");
    }
}
