//! The companion: what MapleSyrup says, and when.
//!
//! It is fed one [`Observation`] per frame and whatever the phone sends
//! (sentences it heard, buttons pressed), and answers with [`Say`] lines and
//! the odd action. It has no clock, no window and no voice of its own —
//! time is passed in as seconds — so every rule here is tested by playing a
//! session through it.
//!
//! It speaks up on its own for little: HP or MP running low, a level-up, a
//! death, the game window coming and going, and — once a night — to ask
//! whether the player is still there when the game has sat idle for twenty
//! minutes without a word from them. Everything else waits to be asked.

pub mod attitude;
pub mod chat;
pub mod commands;
pub mod exp;
pub mod instant;
pub mod observation;

pub use attitude::{Attitude, Deck};

use serde::Serialize;

pub use commands::{Command, Heard};
pub use exp::{ExpTracker, spoken_duration};
pub use observation::{GameView, Gauge, Observation};

/// Thresholds and pacing. (How often a low bar is warned of is no
/// setting: once per fight, again only unanswered or lower — see
/// [`Low`].)
#[derive(Debug, Clone)]
pub struct Settings {
    /// Warn when HP falls below this percent (0: never).
    pub hp_low: f32,
    /// Warn when MP falls below this percent (0: never — the default: only
    /// a mark the player set himself, by asking, warns of MP. A Magician's
    /// MP goes up and down all the time: 25 MP lines in his two hours were
    /// noise).
    pub mp_low: f32,
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
            mp_low: 0.0,
            lost_after: 5.0,
            listen_for: 8.0,
            always_listen: true,
            attitude: Attitude::Friendly,
        }
    }
}

/// Why a line is said: the phone and the console colour them differently,
/// and the voice says a warning sharper than news. "Aw, you died" in the
/// voice of "POT NOW" is a machine with one setting for important.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// Danger right now — a beating, a low bar, a thing the player asked
    /// to be warned of past its mark: shouted (said sharper and sooner),
    /// and on a call handed over with the reading behind it.
    Warning,
    /// News the companion noticed on its own — a death, a level-up, a
    /// thing seen, the coach's word, "still there?": told, at the usual
    /// pace.
    Alert,
    /// An answer to something the player asked.
    Reply,
    /// News about the companion itself (phone connected, muted).
    Info,
    /// What the phone heard the player say, shown for reference.
    Heard,
}

impl Kind {
    /// Whether a line of this kind is still true after the player talks
    /// over it — a warning, news — and so is not called off with the rest:
    /// the worker keeps its job, the main loop keeps its clip and lets its
    /// voice play out. Chat and a note are not.
    pub fn kept(self) -> bool {
        matches!(self, Kind::Warning | Kind::Alert)
    }
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
    fn warning(text: impl Into<String>) -> Self {
        Self {
            kind: Kind::Warning,
            text: text.into(),
            speak: true,
        }
    }
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

/// What a friend in the room would know of the session so far, for the
/// model (`Companion::so_far`): all in seconds, `None` where there is
/// nothing to say yet.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SoFar {
    /// How long the session has run.
    pub seconds: f64,
    /// Since the player last said anything (`None`: not yet this session).
    pub since_player_spoke: Option<f64>,
    /// How long the player had been silent before what they just said,
    /// when it was a minute or more — for the one reply to it (the main
    /// loop notes their words before it builds the reply's snapshot, so
    /// `since_player_spoke` is 0 by then, and "welcome back" had nothing
    /// to go on). `None` once 10 s have passed, or they speak again.
    pub quiet_before: Option<f64>,
    /// Deaths so far, and since the last.
    pub deaths: u32,
    pub since_last_death: Option<f64>,
    /// Level-ups so far, and since the last one celebrated.
    pub level_ups: u32,
    pub since_last_level_up: Option<f64>,
    /// The lowest HP read in the last minute, in percent (while alive and
    /// the bar was being read, not guessed at).
    pub lowest_hp_lately: Option<f32>,
    /// How long neither HP nor EXP has moved (nor MP been potted), with
    /// the game in view.
    pub quiet_for: Option<f64>,
    /// How long the game has been out of sight (`None`: it is in view, or
    /// was never seen).
    pub unseen_for: Option<f64>,
}

/// HP or EXP moving by less than this is the bar's flicker, not something
/// happening: a hit, a potion or a kill moves more.
const QUIET_HP_POINTS: f32 = 2.0;
const QUIET_EXP_READ: f32 = 0.01;
const QUIET_EXP_BAR: f32 = 0.5;
/// HP readings are kept this long for the lowest lately, in seconds.
const LOWEST_HP_SECS: f64 = 60.0;
/// A silence of the player's this long is worth a word when they end it,
/// and is reported for this long after, in seconds: one reply's worth —
/// the reply's snapshot is taken within a second or two of the sentence,
/// and a call fetches it as the player starts speaking. (Kept 30 s, the
/// next sentence's reply carried "they had been quiet for 32 min until
/// just now" again.)
const QUIET_BEFORE_SECS: f64 = 60.0;
const QUIET_BEFORE_KEPT_SECS: f64 = 10.0;
/// The game idle — HP and EXP unmoved, in view — for this long, and not
/// a word from the player in that time: it asks whether they are still
/// there, once a session…
const STILL_THERE_SECS: f64 = 1200.0;
/// …and never this soon after a warning of its own (one the player
/// answered with a potion was a sign of life two minutes ago).
const STILL_THERE_AFTER_WARNING_SECS: f64 = 300.0;

/// A bar's low warning, bound to the fight the way the beating is. A
/// fight begins when the bar goes under the mark and is over
/// [`FIGHT_OVER_SECS`] after it was last under (for HP, after the last fast
/// fall too). Within it the warning is said once; a potion answers it (the
/// bar back up by [`POTTED`] over where it was said), and then it is not
/// said again until the fight is over — unless the bar goes lower than it
/// was at the last line: then once more, at once. Unanswered, it is said
/// again only after a new fall — the bar [`NEW_FALL`] points under where it
/// was at the last line (damage taken) — and not before
/// [`FALL_COOLDOWNS`], longer each time; a bar that stays where it was is
/// said once ("46 percent HP" eleven times in fifteen minutes to the owner
/// standing still, 2026-10-10). A fight is over only once the bar has been
/// read over the mark: frames with no reading — the HUD lost and found
/// again — start no new fight, so no new first line. A grind of hit, pot,
/// hit, every 8 s, had the line at every hit: 75 "pot now" in ten minutes,
/// in seven wordings — the beating's nag, one rule down. (HP and MP alike:
/// MP's is this rule too.)
///
/// A bar that reads low for minutes while EXP comes in is a bar read
/// wrong — a character does not live at 20% through ten minutes of
/// killing things — and after [`DOUBT_AFTER_LINES`] unanswered lines of
/// one fight with EXP gained since the first, it is not believed: the
/// player is told so once, its warnings are held, and it is believed
/// again once it has read above the mark for [`BELIEVE_AGAIN_SECS`]. (The
/// hold in `pace` is for the case where nothing moves; a misread bar on a
/// night of grinding got past it on the EXP, one line every two minutes
/// for hours.)
#[derive(Debug, Clone, Copy)]
struct Low {
    /// Frames in a row under the mark, and since when (a frame or two can
    /// be a bar half-covered by a dialog, or misread).
    frames: u32,
    since: f64,
    /// When the bar was last under the mark, and whether it has been read
    /// at or over the mark since (only then can the fight be over).
    low_at: f64,
    seen_above: bool,
    /// When the line was last said, the reading then, how many lines this
    /// fight (and how many of the last in a row repeated an unanswered
    /// one), and whether a potion has answered the last — frames in a row
    /// with the bar back up by a potion's worth: one is a misread (see
    /// [`HELD_FRAMES`]).
    told: f64,
    told_at: f32,
    lines: u32,
    repeats: u32,
    answered: bool,
    up_frames: u32,
    /// EXP when the fight's first line was said.
    exp_at_first: Option<f32>,
    /// The bar is not believed, and since when it has read above the mark
    /// (never: minus infinity).
    doubted: bool,
    above_since: f64,
    /// Lines of this bar so far this session, and how many of them a
    /// potion answered (for the session's stats: [`Companion::tally`]).
    said_total: u32,
    potted: u32,
}

/// What the companion's own warnings came to this session, for the
/// session's stats ([`crate::metrics`]): how many of each were said, the
/// low-bar lines (HP or MP, said or held) and how many of them a potion
/// answered, and the deaths.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Tally {
    pub beating: u32,
    pub hp_low: u32,
    pub mp_low: u32,
    pub low_lines: u32,
    pub potted: u32,
    pub deaths: u32,
}

/// What a reading of a low bar calls for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Due {
    Nothing,
    /// The warning.
    Line,
    /// The bar is not to be believed, as of now: say so, once.
    Misread,
}

/// Which line of a fight a low line is, for the deck it is dealt from. A
/// friend says it, says it again, then asks why: four orders in 47 s at a
/// bar that had not moved was a machine with one sentence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Cadence {
    /// The fight's first line, or one that stands on its own (the bar
    /// lower than at the last line, after a potion answered it).
    First,
    /// A repeat of an unanswered line: the card may say "still".
    Again,
    /// The second repeat running: a question ("Out of pots?").
    Ask,
}

impl Low {
    fn new() -> Self {
        Self {
            frames: 0,
            since: 0.0,
            low_at: f64::NEG_INFINITY,
            seen_above: false,
            told: f64::NEG_INFINITY,
            told_at: f32::INFINITY,
            lines: 0,
            repeats: 0,
            answered: false,
            up_frames: 0,
            exp_at_first: None,
            doubted: false,
            above_since: f64::NEG_INFINITY,
            said_total: 0,
            potted: 0,
        }
    }

    /// Takes a reading (and EXP as read with it); what it calls for.
    /// `fought_at` is when the fight was last seen going on some other way
    /// (HP falling fast; never, for MP).
    fn due(&mut self, now: f64, percent: f32, mark: f32, fought_at: f64, exp: Option<f32>) -> Due {
        // (Over a minute since it was last low — and read over the mark
        // since: a minute of no readings is no minute of the bar up.)
        if now - self.low_at.max(fought_at) > FIGHT_OVER_SECS && self.seen_above {
            self.lines = 0;
        }
        if percent < mark {
            if self.frames == 0 {
                self.since = now;
            }
            self.frames += 1;
            self.low_at = now;
            self.seen_above = false;
        } else {
            self.frames = 0;
            self.seen_above = true;
        }
        if percent >= self.told_at + POTTED {
            self.up_frames += 1;
            if self.up_frames >= HELD_FRAMES {
                // (A line of this fight answered: counted once.)
                if !self.answered && self.lines > 0 {
                    self.potted += 1;
                }
                self.answered = true;
            }
        } else {
            self.up_frames = 0;
        }
        if self.doubted {
            // Believed again once it has read above the mark for a while;
            // the next time it is low is a new fight.
            if percent >= mark {
                if !self.above_since.is_finite() {
                    self.above_since = now;
                }
                if now - self.above_since >= BELIEVE_AGAIN_SECS {
                    self.doubted = false;
                    self.lines = 0;
                    self.answered = false;
                }
            } else {
                self.above_since = f64::NEG_INFINITY;
            }
            return Due::Nothing;
        }
        let held = self.frames >= 3 && now - self.since >= LOW_HOLD_SECS;
        if !held {
            return Due::Nothing;
        }
        match self.lines {
            0 => Due::Line,
            _ if self.answered => {
                if percent < self.told_at - QUIET_HP_POINTS {
                    Due::Line
                } else {
                    Due::Nothing
                }
            }
            said => {
                let wait = FALL_COOLDOWNS[(said as usize - 1).min(FALL_COOLDOWNS.len() - 1)];
                // Unanswered: again only after a new fall (and the wait).
                if now - self.told < wait || percent > self.told_at - NEW_FALL {
                    Due::Nothing
                } else if said >= DOUBT_AFTER_LINES && exp_gained(self.exp_at_first, exp) {
                    self.doubted = true;
                    self.above_since = f64::NEG_INFINITY;
                    Due::Misread
                } else {
                    Due::Line
                }
            }
        }
    }

    /// The line is said now, at `percent`, with EXP read at `exp`.
    fn said(&mut self, now: f64, percent: f32, exp: Option<f32>) {
        if self.lines == 0 {
            self.exp_at_first = exp;
        }
        self.repeats = if self.cadence() == Cadence::First {
            0
        } else {
            self.repeats + 1
        };
        self.told = now;
        self.told_at = percent;
        self.lines += 1;
        self.said_total += 1;
        self.answered = false;
        self.up_frames = 0;
    }

    /// Which line a line due now would be: one that repeats one of this
    /// fight's that went unanswered (the card may say "still") — the
    /// second of those running asks instead — or one that stands on its
    /// own: a first line, or one more because the bar went lower after a
    /// potion.
    fn cadence(&self) -> Cadence {
        if self.lines == 0 || self.answered {
            Cadence::First
        } else if self.repeats == 1 {
            Cadence::Ask
        } else {
            Cadence::Again
        }
    }

    /// Whether the line was said in this fight — the one still going on
    /// at `now` (see `due` for when a fight is over; `fought_at` as there).
    /// The death line goes by this: "told you to pot" when it did, this
    /// fight, however long ago; a window of seconds had it deny a warning
    /// said 17 s before, the second line of the cadence being 30 s off.
    fn warned_this_fight(&self, now: f64, fought_at: f64) -> bool {
        self.lines > 0 && now - self.low_at.max(fought_at) <= FIGHT_OVER_SECS
    }
}

pub struct Companion {
    pub settings: Settings,
    last: Option<Observation>,
    /// When the game was last seen, and whether its loss was announced.
    seen_at: Option<f64>,
    announced_lost: bool,
    ever_seen: bool,
    /// The HP and MP low warnings, each bound to its fight.
    low_hp: Low,
    low_mp: Low,
    /// Its own lines, dealt so that none is heard twice in a row.
    decks: Decks,
    zero_hp_frames: u32,
    dead: bool,
    /// HP lately (when, percent), to tell a death a sooner warning could
    /// have helped with from a sudden one, and a beating as it happens.
    hp_lately: std::collections::VecDeque<(f64, f32)>,
    /// Frames in a row in which HP has fallen fast, and when that was last
    /// said.
    falling_frames: u32,
    fall_told: f64,
    /// The fight: when HP last fell fast (it is over a minute after), how
    /// many times the beating has been said in it, HP when it last was,
    /// and whether a potion has answered it since (frames in a row with
    /// HP back up by a potion's worth: one is a misread).
    fall_at: f64,
    fight_lines: u32,
    fall_hp: f32,
    fall_answered: bool,
    fall_up_frames: u32,
    /// Trust in the player with a beating: fights in a row they handled
    /// (a potion within [`TRUST_POT_SECS`]), the lowest HP of those
    /// (infinite until one), the lowest of the fight under way, and a
    /// fall being watched instead of shouted — since when.
    trust: u32,
    trust_floor: f32,
    fight_low: f32,
    watch: Option<f64>,
    listening_until: f64,
    muted: bool,
    exp: ExpTracker,
    last_level: Option<u32>,
    /// The highest level seen for this character: a reading below it is a
    /// misread (the sight's reader can hold a wrong number for a while),
    /// and only one above it is a level-up — a reading that flips 165, 166,
    /// 165, 166 is not four of them.
    top_level: Option<u32>,
    /// The character's name when the level was last taken: a level that
    /// comes with another name is another character's.
    last_name: Option<String>,
    /// A level reading waiting to hold: the level, and since when.
    level_candidate: Option<(u32, f64)>,
    /// Level-ups of the characters played before this one, this session
    /// (`exp` starts over for each); when falls in EXP were counted as
    /// level-ups lately (wraps); and since when a level reading that would
    /// be another character's has waited to hold (see
    /// [`Companion::levels_gained`]).
    levels_before: u32,
    wraps: std::collections::VecDeque<f64>,
    switch_since: Option<f64>,
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
    /// MP read lately (when, percent; not a sliver), and whether the
    /// sliver being read now is a reading (`None`: MP is no sliver; see
    /// [`Companion::mp_reading`]).
    mp_lately: std::collections::VecDeque<(f64, f32)>,
    mp_empty: Option<bool>,
    /// Alerts said with no sign of life from the player since, and the
    /// state when the last was said: when, and HP, MP, EXP and the level
    /// then, with the highest EXP seen since, and whether HP or MP has been
    /// back up by a potion's worth since — frames in a row with it so: one
    /// is a misread (see [`HELD_FRAMES`]).
    unanswered: u32,
    alert_at: f64,
    hp_at_alert: Option<f32>,
    hp_up_frames: u32,
    mp_at_alert: Option<f32>,
    mp_up_frames: u32,
    potted: bool,
    exp_at_alert: Option<f32>,
    exp_peak: Option<f32>,
    level_at_alert: Option<u32>,
    /// When the player last spoke, and the silence that sentence ended
    /// (how long, and when), when it was long enough to be worth a word.
    spoke_at: f64,
    quiet_before: Option<(f64, f64)>,
    /// Until when alerts are held for want of an answer, and when that was
    /// last said.
    hold_until: f64,
    hold_told: f64,
    /// Deaths this session, and when the last was (for `so_far`; no rule
    /// reads them).
    deaths: u32,
    last_death: f64,
    /// HP read lately (when, percent), alive and steady, for the lowest in
    /// the last minute.
    hp_minute: std::collections::VecDeque<(f64, f32)>,
    /// When HP or EXP last moved noticeably, or MP jumped by a potion, and
    /// what HP and EXP were then; MP the frame before, and a jump waiting
    /// to hold (from what, and frames so far).
    changed_at: f64,
    hp_at_change: Option<f32>,
    exp_at_change: Option<f32>,
    mp_before: Option<f32>,
    mp_jump: Option<(f32, u32)>,
    /// HP's twin of `mp_before` / `mp_jump`.
    hp_before: Option<f32>,
    hp_jump: Option<(f32, u32)>,
    /// When a bar last jumped by a potion's worth from one frame to the
    /// next, held (`jumped`): a potion seen, not regen — what "There you
    /// go." may be said for.
    potion_at: f64,
    /// When the first frame came, since when the game has been in view
    /// (the last time it was found, or found again), and whether "still
    /// there?" was asked this session.
    started_at: Option<f64>,
    view_since: f64,
    asked_still_there: bool,
    /// Its own lines are said in Hebrew (see [`Companion::set_language`]).
    hebrew: bool,
    /// The warnings said so far, by kind (see [`Companion::tally`]).
    warned: Tally,
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
    /// When the swing being held began: its first reading, a few frames
    /// before it was found to swing (lines said from then on were said
    /// from a guess).
    swing_from: f64,
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
            swing_from: f64::INFINITY,
        }
    }

    /// Whether the readings are being held at `now`.
    fn holding(&self, now: f64) -> bool {
        now < self.unsteady_until
    }

    /// Whether a reading taken at `at` was the swing's: one of its first
    /// frames, before it was found to swing (they pass for readings), or
    /// one of the hold's.
    fn swung_at(&self, at: f64) -> bool {
        (self.swing_from..self.unsteady_until).contains(&at)
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
                // (A new bout: it began with its first jump.)
                self.swing_from = self
                    .lately
                    .iter()
                    .zip(self.lately.iter().skip(1))
                    .find(|(a, b)| (b.1 - a.1).abs() >= SWING)
                    .map_or(now, |(_, b)| b.0);
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

/// A potion seen on a bar: up by `POTTED` from one frame to the next and
/// still there `HELD_FRAMES` frames on (regen never jumps; one frame read
/// high does not hold). `jump` is the reading it rose from and the frames
/// it has held; true on the frame the jump is confirmed.
fn jumped(before: Option<f32>, jump: &mut Option<(f32, u32)>, value: f32) -> bool {
    let mut confirmed = false;
    *jump = match *jump {
        Some((from, frames)) if value >= from + POTTED => {
            confirmed = frames + 1 >= HELD_FRAMES;
            (!confirmed).then_some((from, frames + 1))
        }
        _ => before
            .filter(|before| value >= before + POTTED)
            .map(|before| (before, 1)),
    };
    confirmed
}

/// The readings among `readings` (when, percent; oldest first) that held:
/// all but a one-frame spike — a reading higher than both the frame
/// before and the frame after by more than the bar's flicker, which is a
/// misread (HP does not go 20, 60, 20 in three frames). A reading at
/// either end has one neighbour and is taken as it comes: the first frame
/// of a fall is where the fall is measured from.
fn held_readings(
    readings: &std::collections::VecDeque<(f64, f32)>,
) -> impl Iterator<Item = (f64, f32)> + '_ {
    let above = |a: f32, b: f32| a - b > QUIET_HP_POINTS;
    readings.iter().enumerate().filter_map(move |(i, &(t, p))| {
        let before = i.checked_sub(1).and_then(|j| readings.get(j));
        let after = readings.get(i + 1);
        let spike =
            before.is_some_and(|&(_, q)| above(p, q)) && after.is_some_and(|&(_, q)| above(p, q));
        (!spike).then_some((t, p))
    })
}

/// A bar's fill read at this or under, in percent, is no reading at all —
/// neither a value nor an empty bar — for HP and the close calls
/// ([`reading`]); MP's only after a way down (see
/// [`Companion::mp_reading`]). The learned sight gives no reading for an empty bar, so
/// a fill this low is a sliver, and the everyday sliver is the cursor's
/// tip on the bar's start: parked at 2 px then 1 px, or easing onto it
/// (a glide down the bar reads like HP draining), it was a death said to
/// a player at 85% — "אוי, אמרתי לשתות" after a false "פחות משני אחוז
/// חיים" (w38's p38c C1, C4, C6, C7) — and a close call counted. A living
/// character's 1–2 px of red lasts no time in play. A number read in the
/// game's font is believed as it comes, 0 and 1 too: the only zero that
/// is a death. (A death seen only in the fill is missed so — the old
/// detector's bar at 0 with EXP lost, or after a way down through the red
/// — as every death on the learned sight with the number unread already
/// was: its sign is a fall seen, then no reading held; not yet looked for.)
pub const SLIVER: f32 = 2.0;

/// `gauge` as a reading: none when it is the bar's fill at [`SLIVER`] or
/// under.
pub fn reading(gauge: Option<Gauge>) -> Option<Gauge> {
    gauge.filter(|g| g.read || g.percent > SLIVER)
}

/// MP's fill at [`SLIVER`] or under is a reading after the way down was
/// seen: this many readings over it and under the mark (not the swing's)
/// within this long before (see [`Companion::mp_reading`]).
const WAY_DOWN_READINGS: usize = 2;
const WAY_DOWN_SECS: f64 = 3.0;
/// "Under the mark" for that way down when MP warnings are off or set
/// lower: the old usual mark, 15%.
const MP_WAY_DOWN_MARK: f32 = 15.0;

/// Alerts said with no sign of life from the player — a word, HP going
/// back up (a potion), EXP gained, a level — before the rest are held…
/// (Four: a beating and the low line's first three — at 0, 5, 17 and 47 s,
/// the third a question ([`Cadence::Ask`]) — and the hold where the next
/// would come, a minute after; six ran a pet's death to six lines in six
/// minutes to a room that may be empty. The same count as
/// [`DOUBT_AFTER_LINES`], which is about a bar that lies while EXP comes
/// in; this one is about nothing moving at all.)
const UNANSWERED_MAX: u32 = 4;
/// …and for how long, unless the player turns up sooner. When the hold
/// ends, a couple more come through before the next.
const HOLD_SECS: f64 = 600.0;
const AFTER_HOLD: u32 = 2;
/// How often the player is told their warnings are being held…
const HOLD_TOLD_EVERY: f64 = 1800.0;
/// …in words for someone who is there when they spoke within this long,
/// in seconds ([`lines::HOLD_HERE`]); for an empty room when they did not,
/// or never did this session ([`lines::HOLD_AWAY`]).
const HOLD_AWAY_SECS: f64 = 600.0;
/// A potion that ends a hold gets one word ([`lines::POTTED_AT_LAST`])
/// when the hold was told this recently, in seconds.
const POTTED_AT_LAST_SECS: f64 = 600.0;
/// A potion that ends a hold is the one just seen as a jump when it came
/// within this long.
const SEEN_POTION_SECS: f64 = 2.0;
/// HP back up by this much since the last alert is a potion: a sign of life.
const POTTED: f32 = 10.0;
/// EXP up by this much since the last alert is play going on.
const EXP_GAINED: f32 = 0.2;
/// A bar's unanswered lines in one fight (more than 100 s under the mark)
/// after which, with EXP gained since the first, the bar is not believed
/// ([`Low`])…
const DOUBT_AFTER_LINES: u32 = 4;
/// …until it has read above the mark for this long, in seconds.
const BELIEVE_AGAIN_SECS: f64 = 3.0;

/// Whether EXP went from `then` to `now` by [`EXP_GAINED`] or more (a
/// level-up wraps it: 99 to 1 is a gain).
fn exp_gained(then: Option<f32>, now: Option<f32>) -> bool {
    let (Some(then), Some(now)) = (then, now) else {
        return false;
    };
    let gained = if now < then - 50.0 {
        now + 100.0 - then
    } else {
        now - then
    };
    gained >= EXP_GAINED
}

/// How long a new level reading must hold before it is believed, in
/// seconds; one below the highest seen for the character must hold ten
/// times as long, a misread by the sight's reader lasting minutes.
const LEVEL_HOLD_SECS: f64 = 3.0;
const LOWER_LEVEL_HOLDS: f64 = 10.0;
/// Another character's EXP bar coming into view lower is no level-up: a
/// fall in EXP counted as one (the bar's wrap) from this long before its
/// level was first read (the level's reader can lag the bar's)…
const SWITCH_WRAP_SECS: f64 = 10.0;
/// …and no longer ago than this (a lower level holds 30 s; one counted
/// before was the character's own).
const SWITCH_SECS: f64 = LEVEL_HOLD_SECS * LOWER_LEVEL_HOLDS + SWITCH_WRAP_SECS;

/// How long HP or MP must stay low before it is said, in seconds (and at
/// least three frames): a moment's misread is not worth a warning.
const LOW_HOLD_SECS: f64 = 0.6;

/// HP warnings move sooner after deaths no warning came before, up to here.
/// (A warning came before a death when a low line was said in the fight
/// the death ends — [`Low::warned_this_fight`] — or the beating was: the
/// warning moves sooner only when neither was, and the death line may say
/// "told you" only when one was.)
const SOONEST_WARNING: f32 = 50.0;
/// …and only when HP spent this long, in seconds, in the band this many
/// points over the mark on its way down: a sooner line would have had
/// time to help. A fall through it in a second and a half is a sudden
/// death, and moving the mark for it promises a shout that would have
/// come too late all the same.
const SOONER_BAND_SECS: f64 = 2.0;
const SOONER_BAND: f32 = 20.0;

/// HP down this many points within [`FALL_SECS`] is a beating, said at
/// once (after a second frame says so: a dialog half over the bar does not
/// last) — before it is low, while backing off still helps.
const FALL_POINTS: f32 = 25.0;
const FALL_SECS: f64 = 3.0;
/// A reading counts only when it holds: this many frames in a row (one
/// frame is a misread as often as not). The fall is measured from
/// readings that held ([`held_readings`]), and a potion's worth of HP
/// back answers a warning only once it has held — a bar read 20, 20, 60,
/// 20, 20 shouted a beating for a bar that did not move, and the one
/// frame of 60 counted as the potion that answered the fight's warnings.
const HELD_FRAMES: u32 = 2;
/// A beating is said once per fight — a fight begins at the first fall
/// and is over after this long without one — unless no potion answers it
/// (HP not back up by [`POTTED`] since): then it is said again, each time
/// after a longer wait. A grind (hit, pot, hit) had it said every 12 s
/// for an hour, and "Shut up" was the answer.
const FIGHT_OVER_SECS: f64 = 60.0;
const FALL_COOLDOWNS: [f64; 4] = [12.0, 30.0, 60.0, 120.0];
/// A low bar's unanswered line is said again only when the bar is this many
/// points under where it was at the last line: a new fall, damage taken
/// (see [`Low`]).
const NEW_FALL: f32 = 5.0;
/// A player who handles the beating is trusted with it: after this many
/// fights in a row with a potion within [`TRUST_POT_SECS`] of the line,
/// the next fall is watched instead of shouted, and shouted after all
/// when HP goes under their usual floor (the lowest of the fights they
/// handled) by [`TRUST_MARGIN`], or under the floor's bottom
/// ([`TRUST_BOTTOM`]) — the depth deserves it; potted in time, it counts
/// like the rest and the floor follows it — or when no potion comes
/// within the same time of the fall. Only a late potion, none, or a
/// death ends the trust; a death during a fall it watched does not move
/// the HP mark (the silence was the trust's, not the mark's). A friend
/// stops saying "back off" the fourth time you handle it (34 beatings in
/// 65 min for a player who potted within 3 s every time; 9 in 10 handled
/// fights when a deeper one started the count over).
const TRUST_FIGHTS: u32 = 3;
const TRUST_POT_SECS: f64 = 3.0;
const TRUST_MARGIN: f32 = 5.0;
/// The floor's bottom, as a fraction of the HP mark: however low the
/// falls they handled, one under this is shouted (half: 15% at the default
/// 30). With no bottom the floor followed them down a margin at a time —
/// or in one step, a deep dip potted in time — and a dip to 3% went
/// unsaid, the death after it too; a friend stops saying "back off" at
/// 60% when you handle it, and nobody stays quiet at 3%.
const TRUST_BOTTOM: f32 = 0.5;

/// How long after a line was said the phone may still hand it back as heard,
/// in seconds: the line, its playing, and the phone's recognition finishing.
const ECHO_WINDOW: f64 = 30.0;

/// Its own voice is never the player ([`Companion::own_words`]): a heard
/// sentence whose words are mostly — this share or more — words it said in
/// the last [`OWN_WINDOW`] s is its own voice, dropped whole, unless what is
/// left is [`OWN_REST_WORDS`] words or more that it did not say (nor a
/// near-spelling of one), and never when what is left carries a name. The
/// owner's evening (2026-10-10): 19 "kept" remainders of 85 echoes, every one
/// but one its own voice written differently — "my name is Armani my name is
/// Miha" learned as his name, "question mark" made a mark, "23% still owe"
/// set his MP warnings at 23%.
const OWN_SHARE: f32 = 0.6;
const OWN_WINDOW: f64 = 8.0;
const OWN_REST_WORDS: usize = 3;
/// A line is being said from when it is noted for about this long — a lead
/// and so much a word — and "said in the last 8 s" counts from its end: the
/// log's echoes came 4–12 s after a long line was noted, 1–7 s after it
/// ended.
const OWN_LEAD_SECS: f64 = 0.5;
const OWN_SECS_PER_WORD: f64 = 0.4;
/// The words a question starts with: the player's few words after its own,
/// starting so, ask about what it said.
const QUESTION_WORDS: &[&str] = &[
    "where", "wheres", "what", "whats", "how", "hows", "why", "which", "who", "whos", "when",
];

/// Edits (insertions, deletions, substitutions) from `a` to `b`, by letter.
fn edit_distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut diagonal = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let above = row[j + 1];
            row[j + 1] = (above + 1)
                .min(row[j] + 1)
                .min(diagonal + usize::from(ca != *cb));
            diagonal = above;
        }
    }
    row[b.len()]
}

/// Whether `word` (normalised) is one of `said`, or the phone's spelling of
/// one: its plural or possessive ("mp" for "mps", "point" for "points"), or,
/// for words of six letters or more, two letters off (three from nine) —
/// "genesis" for "henesys", "racoon" for "raccoon", "equivalent" for
/// "equipment". (Not "tell" for "tall": short words are words.)
fn said_or_near(word: &str, said: &[&str]) -> bool {
    let n = word.chars().count();
    said.iter().any(|s| {
        if *s == word {
            return true;
        }
        let m = s.chars().count();
        let (short, long) = if m <= n { (*s, word) } else { (word, *s) };
        if short.chars().count() >= 2
            && long
                .strip_prefix(short)
                .is_some_and(|tail| tail == "s" || tail == "es")
        {
            return true;
        }
        n.min(m) >= 6 && edit_distance(word, s) <= if n.max(m) >= 9 { 3 } else { 2 }
    })
}

/// Whether `token`, a word as the phone wrote it, is a name: capitalised
/// within the sentence ("Miha", "Hennessey") — not its first word, nor "I"
/// or an all-capitals "OK", "MP".
fn looks_like_a_name(token: &str, first: bool) -> bool {
    let core = token.trim_matches(|c: char| !c.is_alphanumeric());
    let mut chars = core.chars();
    !first
        && chars.next().is_some_and(char::is_uppercase)
        && chars.any(char::is_lowercase)
        && !core.starts_with("I'")
        && !core.starts_with("I’")
}

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

/// Words a correction starts with: a sentence that begins with one is the
/// player's, however much of MapleSyrup's own line it goes on to quote
/// ("no, I'm not at the Gate of the Future").
const DENIALS: &[&str] = &[
    "no", "not", "nope", "nah", "wrong", "לא", "non", "nein", "нет", "아니", "いや", "不",
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

/// MapleSyrup's own lines, the way a friend on voice chat would say them.
/// One list per attitude (friendly, blunt, savage); the first line of each
/// list is the lead — the most informative, the one with the number — and
/// opens a session; the rest are dealt from a [`Deck`], every line once
/// before any again. `{}` is the amount or the level.
///
/// Every deck has a Hebrew twin (`…_HE`), said when the player speaks
/// Hebrew ([`super::Companion::set_language`]) — at once, where an English
/// line waited for a model to translate it (and showed in English): the
/// same number of cards per attitude, card for card the same in meaning,
/// with the same `{}`. One [`Deck`] deals both by place
/// ([`Deck::deal_index`]), so a switch of language mid-session goes on with
/// the deck where it was. They are written as an Israeli gamer says them
/// on voice chat to a friend, in the masculine; HP is "חיים", MP "מאנה",
/// EXP "אקספי", an amount "20 אחוז" or "בערך 20 אחוז" — and no card has a
/// Latin letter in it: a line with one is sent to the model to translate
/// (`ai::translate` lets a line through only when it has none).
///
/// Savage stays within the policy in `ai::style`: the play is insulted,
/// never who they are.
pub mod lines {
    /// HP falling fast, said as it happens. Every card stands alone — it
    /// is a fight's first shout as often as not, so none says "still" or
    /// "again" — and none puts a number on the fall the rule does not
    /// check: a fall of a third of the bar was called "half your bar".
    pub const BEATING: [&[&str]; 3] = [
        &[
            "Whoa, you're taking a beating, HP's at {}. Back off and pot!",
            "Careful, your HP's dropping fast. Back off!",
            "Hey hey hey, that's a lot of damage. Step back a sec.",
            "You're melting! Pot, then come back.",
            "That thing hits hard. Give it some space.",
            "Oof. Back up and drink something before the next one lands.",
            "Big hits coming in, you're dropping fast. Pot now, fight later.",
        ],
        &[
            "Back off, you're getting shredded. {} and falling.",
            "Get out of there. Pot.",
            "You're eating every hit. Move.",
            "Get out of whatever that is. Back up.",
            "Pot. Now. Not after the next hit.",
            "That's a chunk of your bar in two seconds. Step off.",
            "Disengage. You can't trade with that thing.",
        ],
        &[
            "Move! You're melting, genius. {} and dropping.",
            "Back off, idiot, you're getting shredded.",
            "Are you tanking that on purpose? Pot, clown.",
            "Stop face-tanking everything and hit the damn potion.",
            "Your HP bar's doing a speedrun. Get out.",
            "Dodging's free, you know. Try it.",
            "Wow, you just stood there and took that. Move, dumbass.",
        ],
    ];

    /// [`BEATING`] in Hebrew, card for card.
    pub const BEATING_HE: [&[&str]; 3] = [
        &[
            "וואו, אתה חוטף מכות, נשארו לך {} חיים. תתרחק ותשתה!",
            "זהירות, החיים שלך יורדים מהר. תתרחק!",
            "היי היי היי, זה הרבה נזק. תתרחק שנייה.",
            "אתה נמס! תשתה, ואז תחזור.",
            "הדבר הזה מרביץ חזק. תתרחק ממנו קצת.",
            "אאוץ'. תתרחק ותשתה משהו לפני המכה הבאה.",
            "אתה חוטף חזק ויורד מהר. קודם שותים, אחר כך נלחמים.",
        ],
        &[
            "תתרחק, קורעים אותך. {} וממשיך לרדת.",
            "תצא משם. תשתה.",
            "אתה אוכל כל מכה. זוז.",
            "לא משנה מה זה, תצא משם. אחורה.",
            "שיקוי. עכשיו. לא אחרי המכה הבאה.",
            "חתיכה מהבר שלך נעלמה בבת אחת. תזוז משם.",
            "תצא מהקרב. אתה לא יכול להחליף מכות עם הדבר הזה.",
        ],
        &[
            "זוז! אתה נמס, גאון. {} ויורד.",
            "תתרחק, אידיוט, קורעים אותך.",
            "אתה סופג את זה בכוונה? תשתה, ליצן.",
            "תפסיק לספוג הכול בפרצוף ותלחץ על השיקוי המחורבן.",
            "בר החיים שלך עושה ספידראן. תצא משם.",
            "להתחמק לא עולה כסף, אתה יודע. תנסה.",
            "וואו, פשוט עמדת שם וספגת את זה. זוז, אהבל.",
        ],
    ];

    /// HP under the threshold: the first line of a fight. Every card
    /// stands alone — nothing here says "still", "again" or "twice", which
    /// presume a line before it; those are [`HP_LOW_AGAIN`]'s, for the
    /// unanswered cadence. (One line per fight makes nearly every line a
    /// first; a revive 6 s before had "Still at 25 percent? Pot already."
    /// open the new life.)
    pub const HP_LOW: [&[&str]; 3] = [
        &[
            "Careful, your HP's down to {}. Drink a potion!",
            "HP's at {}, potion time!",
            "Whoa, {} HP. Drink something!",
            "Pot up, you're at {}.",
            "You're low, {}. Don't push it, drink.",
            "Hey, HP's getting scary. Potion, please!",
            "{} left on the bar. Top it up before the next hit.",
        ],
        &[
            "HP {}. Pot now!",
            "Pot! You're at {}.",
            "{} HP. Drink!",
            "You're at {}. That's not a lot. Pot.",
            "Low HP, {}. Fix it.",
            "Drink. You're at {} and swinging like it's full.",
        ],
        &[
            "{} HP. Drink, you idiot!",
            "Pot NOW, you're at {}, genius.",
            "{} HP. Are you trying to die?",
            "Your HP's at {} and you're attacking. Bold. Stupid, but bold.",
            "Is {} a flex? Drink the damn potion.",
            "{} HP. Your gravestone's loading.",
            "Red bar, {}, no potion. Incredible. Pot.",
        ],
    ];

    /// [`HP_LOW`] in Hebrew, card for card.
    pub const HP_LOW_HE: [&[&str]; 3] = [
        &[
            "זהירות, נשארו לך {} חיים. תשתה שיקוי!",
            "{} חיים, זמן לשיקוי!",
            "וואו, {} חיים. תשתה משהו!",
            "תשתה, אתה על {}.",
            "החיים שלך נמוכים, {}. אל תגזים, תשתה.",
            "היי, בר החיים נהיה מפחיד. שיקוי, בבקשה!",
            "נשארו לך {} חיים. תשתה לפני המכה הבאה.",
        ],
        &[
            "{} חיים. תשתה עכשיו!",
            "שיקוי! אתה על {}.",
            "{} חיים. תשתה!",
            "אתה על {}. זה לא הרבה. תשתה.",
            "חיים נמוכים, {}. תסדר את זה.",
            "תשתה. אתה על {} ומרביץ כאילו הבר מלא.",
        ],
        &[
            "{} חיים. תשתה, אידיוט!",
            "תשתה עכשיו, אתה על {}, גאון.",
            "{} חיים. אתה מנסה למות?",
            "יש לך {} חיים ואתה תוקף. אמיץ. מטומטם, אבל אמיץ.",
            "{} חיים, מה, אתה עושה פוזות? תשתה את השיקוי המחורבן.",
            "{} חיים. המצבה שלך בטעינה.",
            "בר אדום, {}, בלי שיקוי. מדהים. תשתה.",
        ],
    ];

    /// HP still under the threshold, the last line of this fight
    /// unanswered: said again, and the card may say so. Not "still at
    /// {}": the bar may have gone lower since.
    pub const HP_LOW_AGAIN: [&[&str]; 3] = [
        &[
            "Still low, {}. Please drink something.",
            "I said potion! {} HP. Drink.",
            "Hey, {} HP is still scary. Pot!",
            "That bar's still red, {}. Drink up.",
            "Didn't you hear me? {} HP. Potion, now.",
            "Calling it again: {} HP. Drink before the next hit.",
        ],
        &[
            "Still low, {}. Pot already.",
            "I said pot. {}.",
            "{} HP. Didn't I just say drink?",
            "Again: {} HP. Drink.",
            "You heard me. {}. Pot.",
            "{}. Same bar, same answer: pot.",
        ],
        &[
            "Still low, {}. Was I unclear? Pot.",
            "I said drink, genius. {}.",
            "{} HP, still. Your ears work? Pot.",
            "Again? {} HP. Drink, clown.",
            "I keep saying it: {}. Pot.",
            "{}. Pot, or die and prove me right.",
        ],
    ];

    /// [`HP_LOW_AGAIN`] in Hebrew, card for card.
    pub const HP_LOW_AGAIN_HE: [&[&str]; 3] = [
        &[
            "עדיין נמוך, {}. תשתה משהו, בבקשה.",
            "אמרתי שיקוי! {} חיים. תשתה.",
            "היי, {} חיים זה עדיין מפחיד. תשתה!",
            "הבר עדיין אדום, {}. תשתה.",
            "לא שמעת אותי? {} חיים. שיקוי, עכשיו.",
            "אני אומר את זה שוב: {} חיים. תשתה לפני המכה הבאה.",
        ],
        &[
            "עדיין נמוך, {}. תשתה כבר.",
            "אמרתי לשתות. {}.",
            "{} חיים. לא אמרתי הרגע לשתות?",
            "שוב: {} חיים. תשתה.",
            "שמעת אותי. {}. תשתה.",
            "{}. אותו בר, אותה תשובה: שיקוי.",
        ],
        &[
            "עדיין נמוך, {}. לא הייתי ברור? תשתה.",
            "אמרתי לשתות, גאון. {}.",
            "{} חיים, עדיין. האוזניים שלך עובדות? תשתה.",
            "שוב? {} חיים. תשתה, ליצן.",
            "אני חוזר על עצמי: {}. תשתה.",
            "{}. תשתה, או שתמות ותוכיח שצדקתי.",
        ],
    ];

    /// HP still under the threshold after a line and its repeat, neither
    /// answered: the third line of the fight is a question, as a friend on
    /// voice chat would ask it. Every card asks; none says "still" or
    /// "again", nor how long it has been — all it knows is two lines went
    /// unanswered and the bar is where it is.
    pub const HP_LOW_ASK: [&[&str]; 3] = [
        &[
            "Out of pots? You're at {}.",
            "Can you get to a potion? HP's at {}.",
            "Is something stopping you from potting? {} HP.",
            "Everything okay? You're on {} and not drinking.",
            "Did the potions run out? {} left.",
            "Want to back off and pot? You're at {}.",
        ],
        &[
            "Out of pots, or just not pressing it? {}.",
            "Do you have potions or not? {} HP.",
            "Is your potion key broken? {}.",
            "{} HP. Are you going to pot or not?",
            "What are you waiting for? {}. Pot.",
            "Out of potions? Then back off. {}.",
        ],
        &[
            "Lost the potion key? {}.",
            "Is this a no-potion run? {} HP.",
            "Do your fingers know where the potion key is? {}.",
            "{}. Are you trying to see how low it goes?",
            "Out of pots, or out of ideas? {}.",
            "Did you sell all your potions? {} HP.",
        ],
    ];

    /// [`HP_LOW_ASK`] in Hebrew, card for card: every card asks.
    pub const HP_LOW_ASK_HE: [&[&str]; 3] = [
        &[
            "נגמרו השיקויים? אתה על {}.",
            "אתה מצליח להגיע לשיקוי? יש לך {} חיים.",
            "משהו מפריע לך לשתות? {} חיים.",
            "הכול בסדר? אתה על {} ולא שותה.",
            "נגמרו לך השיקויים? נשארו {}.",
            "רוצה להתרחק ולשתות? אתה על {}.",
        ],
        &[
            "נגמרו השיקויים, או שאתה פשוט לא לוחץ? {}.",
            "יש לך שיקויים או לא? {} חיים.",
            "המקש של השיקוי שבור? {}.",
            "{} חיים. אתה הולך לשתות או לא?",
            "למה אתה מחכה? {}. תשתה.",
            "נגמרו השיקויים? אז תתרחק. {}.",
        ],
        &[
            "איבדת את המקש של השיקוי? {}.",
            "זה אתגר בלי שיקויים? {} חיים.",
            "האצבעות שלך יודעות איפה המקש של השיקוי? {}.",
            "{}. אתה בודק כמה נמוך זה יכול לרדת?",
            "נגמרו השיקויים, או נגמר השכל? {}.",
            "מכרת את כל השיקויים? {} חיים.",
        ],
    ];

    /// MP under the threshold: the first line of a fight (see [`HP_LOW`]).
    pub const MP_LOW: [&[&str]; 3] = [
        &[
            "Your MP's down to {}.",
            "MP's at {}, might want a potion.",
            "Heads up, only {} MP left.",
            "Mana's getting low, {}. Drink a blue one.",
            "You're almost out of MP. Pot before the skills stop.",
            "{} MP. Top it up when you get a second.",
            "Mana check: {}. Time for a potion.",
        ],
        &[
            "MP {}. Pot.",
            "Mana's at {}. Drink.",
            "{} MP left. Drink.",
            "You're about to run dry, {}. Blue pot.",
            "No mana, no skills. {}. Drink.",
            "Mana's at {}. Skills need it. Drink.",
        ],
        &[
            "{} MP. Drink before you're useless.",
            "{} MP. Drink something, clown.",
            "{} MP. Gonna auto-attack the boss to death, are we?",
            "Mana's at {}. Your skills are about to be decorative.",
            "{} MP. Even the mage mules manage better than this.",
            "{} MP. Drink the blue one, genius.",
        ],
    ];

    /// [`MP_LOW`] in Hebrew, card for card.
    pub const MP_LOW_HE: [&[&str]; 3] = [
        &[
            "נשארו לך {} מאנה.",
            "{} מאנה, אולי כדאי שיקוי.",
            "שים לב, נשארו רק {} מאנה.",
            "המאנה יורדת, {}. תשתה כחול.",
            "המאנה כמעט נגמרת לך. תשתה לפני שתישאר בלי סקילים.",
            "{} מאנה. תמלא אותה כשיש לך רגע.",
            "בדיקת מאנה: {}. זמן לשיקוי.",
        ],
        &[
            "{} מאנה. תשתה.",
            "המאנה על {}. תשתה.",
            "נשארו {} מאנה. תשתה.",
            "המאנה עומדת להיגמר, {}. שיקוי כחול.",
            "אין מאנה, אין סקילים. {}. תשתה.",
            "המאנה על {}. הסקילים צריכים אותה. תשתה.",
        ],
        &[
            "{} מאנה. תשתה לפני שאתה נהיה חסר תועלת.",
            "{} מאנה. תשתה משהו, ליצן.",
            "{} מאנה. מה, תהרוג את הבוס במכות רגילות?",
            "המאנה על {}. הסקילים שלך עומדים להיות קישוט.",
            "{} מאנה. אפילו מאג' מתחיל מסתדר יותר טוב מזה.",
            "{} מאנה. תשתה את הכחול, גאון.",
        ],
    ];

    /// MP still under the threshold, the last line unanswered (see
    /// [`HP_LOW_AGAIN`]).
    pub const MP_LOW_AGAIN: [&[&str]; 3] = [
        &[
            "Still low on mana, {}. Blue potion!",
            "MP's still low, {}. Drink a blue one.",
            "Mana check again: {}. Top it up.",
            "I did say mana, {}. Pot when you can.",
            "Only {} MP, still. Drink.",
            "Mana again: {}. Blue pot, please.",
        ],
        &[
            "Still low on mana, {}. Drink.",
            "{} MP. I'm saying it twice now. Drink.",
            "I said mana. {}.",
            "Mana still low, {}. Why is it always mana with you?",
            "Again: {} MP. Blue pot.",
            "{} MP. You heard me the first time. Drink.",
        ],
        &[
            "Still out of mana? {}. Pot, genius.",
            "You're still sitting at {} mana. Learn. Drink.",
            "I said blue pot. {} MP. Clown.",
            "Still {} MP. Your skills are decorative and so are your ears.",
            "Mana, again. {}. Drink.",
            "{} MP. I said it once already. Pot.",
        ],
    ];

    /// [`MP_LOW_AGAIN`] in Hebrew, card for card.
    pub const MP_LOW_AGAIN_HE: [&[&str]; 3] = [
        &[
            "המאנה עדיין נמוכה, {}. שיקוי כחול!",
            "עדיין מעט מאנה, {}. תשתה כחול.",
            "שוב בדיקת מאנה: {}. תמלא אותה.",
            "אמרתי מאנה, {}. תשתה כשאתה יכול.",
            "רק {} מאנה, עדיין. תשתה.",
            "שוב מאנה: {}. שיקוי כחול, בבקשה.",
        ],
        &[
            "עדיין חסרה לך מאנה, {}. תשתה.",
            "{} מאנה. אני אומר את זה כבר פעמיים. תשתה.",
            "אמרתי מאנה. {}.",
            "המאנה עדיין נמוכה, {}. למה זה תמיד מאנה אצלך?",
            "שוב: {} מאנה. שיקוי כחול.",
            "{} מאנה. שמעת אותי בפעם הראשונה. תשתה.",
        ],
        &[
            "עדיין בלי מאנה? {}. תשתה, גאון.",
            "אתה עדיין תקוע עם {} מאנה. תלמד. תשתה.",
            "אמרתי שיקוי כחול. {} מאנה. ליצן.",
            "עדיין {} מאנה. הסקילים שלך קישוט, וגם האוזניים.",
            "מאנה, שוב. {}. תשתה.",
            "{} מאנה. אמרתי את זה כבר פעם אחת. תשתה.",
        ],
    ];

    /// MP's third unanswered line of a fight: a question (see
    /// [`HP_LOW_ASK`]).
    pub const MP_LOW_ASK: [&[&str]; 3] = [
        &[
            "Out of mana pots? You're at {} MP.",
            "Can you get to a blue potion? Mana's at {}.",
            "Is the mana potion key stuck? {} MP.",
            "Everything okay? {} MP and no blue pot.",
            "Did the blue pots run out? Mana's on {}.",
            "Want to grab a mana potion? You're at {} MP.",
        ],
        &[
            "Out of blue pots, or just not pressing it? {} MP.",
            "Got any mana potions or not? {} MP.",
            "Is your mana key broken? {} MP.",
            "Mana's at {}. Are you going to drink or not?",
            "How are you going to cast on {} MP?",
            "What are you waiting for? {} mana. Drink.",
        ],
        &[
            "Lost the blue pot key? {} MP.",
            "Is this a no-mana challenge? {} MP.",
            "Saving the blue potions for a special occasion? {} MP.",
            "Do you know mana potions exist? {} MP.",
            "Planning to auto-attack your way out? {} MP.",
            "Out of blue pots, or out of ideas? {} MP.",
        ],
    ];

    /// [`MP_LOW_ASK`] in Hebrew, card for card: every card asks.
    pub const MP_LOW_ASK_HE: [&[&str]; 3] = [
        &[
            "נגמרו השיקויים הכחולים? אתה על {} מאנה.",
            "אתה מצליח להגיע לשיקוי כחול? המאנה על {}.",
            "המקש של שיקוי המאנה תקוע? {} מאנה.",
            "הכול בסדר? {} מאנה ובלי שיקוי כחול.",
            "נגמרו הכחולים? המאנה על {}.",
            "רוצה לשתות שיקוי מאנה? אתה על {} מאנה.",
        ],
        &[
            "נגמרו הכחולים, או שאתה פשוט לא לוחץ? {} מאנה.",
            "יש לך שיקויי מאנה או לא? {} מאנה.",
            "המקש של המאנה שבור? {} מאנה.",
            "המאנה על {}. אתה הולך לשתות או לא?",
            "איך תזרוק סקילים עם {} מאנה?",
            "למה אתה מחכה? {} מאנה. תשתה.",
        ],
        &[
            "איבדת את המקש של הכחול? {} מאנה.",
            "זה אתגר בלי מאנה? {} מאנה.",
            "שומר את הכחולים לאירוע מיוחד? {} מאנה.",
            "אתה יודע ששיקויי מאנה קיימים? {} מאנה.",
            "מתכנן לצאת מזה במכות רגילות? {} מאנה.",
            "נגמרו הכחולים, או נגמר השכל? {} מאנה.",
        ],
    ];

    /// HP at zero, with no warning said just before: nothing here claims
    /// one came (see [`DEATH_WARNED`]), and nothing speaks of an earlier
    /// death ("Dead again"): for a player who knows it, any card may open
    /// a night's deaths (see [`super::Companion::settled`]).
    pub const DEATH: [&[&str]; 3] = [
        &[
            "Your HP hit zero. Time to revive and head back.",
            "Aw, you died. Revive and get back in there, you've got this.",
            "Down! Okay, respawn, pot up, try again.",
            "That one got you. Revive, no big deal.",
            "Rest in pieces. Grab your stuff and head back.",
            "Ouch. Back to town for you. Revive and we go again.",
            "Dead. Shake it off, revive and get back in.",
        ],
        &[
            "You died. Revive and get back in there.",
            "Well, that's a death. Revive, go again.",
            "Aaand you're down. Respawn.",
            "You stood in it and died. Learn something from that. Revive.",
            "Flat on the floor. Revive, and watch the bar this time.",
            "Dead. Up you get. Revive, and don't stand in that again.",
        ],
        &[
            "Dead. Wow. Revive and try not to suck this time.",
            "You died, genius. Revive and get back in.",
            "Congrats, you found the floor. Respawn.",
            "Dead. The mobs are starting to feel bad for you.",
            "HP zero. Skill zero. Revive, idiot.",
            "That was embarrassing. Revive before anyone sees.",
        ],
    ];

    /// [`DEATH`] in Hebrew, card for card: no card claims a warning came, or
    /// speaks of an earlier death.
    pub const DEATH_HE: [&[&str]; 3] = [
        &[
            "החיים שלך הגיעו לאפס. זמן לקום ולחזור.",
            "אוי, מתת. תקום ותחזור לשם, אתה יכול.",
            "נפלת! אוקיי, תקום, תשתה, ועוד ניסיון.",
            "זה גמר אותך. תקום, לא נורא.",
            "ריפ. תאסוף את הדברים שלך ותחזור לשם.",
            "אאוץ'. חוזרים לעיר. תקום ויוצאים לעוד סיבוב.",
            "מת. קורה. תקום, ממשיכים.",
        ],
        &[
            "מתת. קום, חוזרים לזה.",
            "נו, מתת. תקום, עוד סיבוב.",
            "וזהו, נפלת. ריספאון.",
            "עמדת בתוך זה ומתת. תלמד מזה משהו. תקום.",
            "שטוח על הרצפה. תקום, והפעם תסתכל על הבר.",
            "מת. יאללה, קום. ובפעם הבאה אל תעמוד בתוך זה.",
        ],
        &[
            "מת. וואו. תקום ותנסה לא לפשל הפעם.",
            "מתת, גאון. תקום ותחזור לשם.",
            "מזל טוב, מצאת את הרצפה. ריספאון.",
            "מת. המפלצות מתחילות לרחם עליך.",
            "אפס חיים. אפס כישרון. תקום, אידיוט.",
            "זה היה מביך. תקום לפני שמישהו רואה.",
        ],
    ];

    /// HP at zero in a fight a low warning or the beating was said in: it
    /// did say to pot (or to back off), and the line may say so (how long
    /// ago it said it, it may not: the warning can be most of a minute
    /// old).
    pub const DEATH_WARNED: [&[&str]; 3] = [
        &[
            "Aw, I did say pot. Revive and we go again.",
            "That's the one I warned about. Back to town, shake it off.",
            "I called it and it still got you. Revive, and pot a bit sooner.",
            "Told you it was getting low! Revive, and keep an eye on the bar.",
            "Next time, when I say drink, drink. Revive, you've got this.",
            "You heard me say potion, right? Okay, respawn, no harm done.",
        ],
        &[
            "Dead. Told you to pot.",
            "Dead. Next time pot when I say pot.",
            "I said pot. You didn't. Revive.",
            "Called it. Respawn, and listen next time.",
            "I warned you. Revive and do better.",
            "You heard me and died anyway. Go again.",
        ],
        &[
            "You pressed every key except the potion one. Respawn.",
            "I said pot, you said die. Revive, genius.",
            "Warned, ignored, dead. Classic. Respawn.",
            "I literally told you. Revive and pretend you listen.",
            "Dead with a potion in your bag and my voice in your ear. Respawn.",
            "Next time I say pot, try pressing it instead of dying. Revive.",
        ],
    ];

    /// [`DEATH_WARNED`] in Hebrew, card for card.
    pub const DEATH_WARNED_HE: [&[&str]; 3] = [
        &[
            "אוי, אמרתי לשתות. תקום ויוצאים לעוד סיבוב.",
            "זה בדיוק מה שהזהרתי ממנו. חוזרים לעיר, לא נורא.",
            "אמרתי שזה יקרה, ובכל זאת מתת. תקום, ובפעם הבאה תשתה קצת יותר מוקדם.",
            "אמרתי לך שזה נהיה נמוך! תקום, ותשים עין על הבר.",
            "בפעם הבאה, כשאני אומר לשתות, תשתה. תקום, אתה יכול.",
            "שמעת אותי אומר שיקוי, נכון? אוקיי, תקום, לא קרה כלום.",
        ],
        &[
            "מת. אמרתי לך לשתות.",
            "מת. בפעם הבאה תשתה כשאני אומר לשתות.",
            "אמרתי לשתות. לא שתית. תקום.",
            "ידעתי. ריספאון, ובפעם הבאה תקשיב.",
            "הזהרתי אותך. תקום ותשתפר.",
            "שמעת אותי ומתת בכל מקרה. עוד סיבוב.",
        ],
        &[
            "לחצת על כל מקש חוץ מזה של השיקוי. ריספאון.",
            "אני אמרתי לשתות, אתה אמרת למות. תקום, גאון.",
            "הוזהרת, התעלמת, מתת. קלאסי. ריספאון.",
            "ממש אמרתי לך. תקום ותעמיד פנים שאתה מקשיב.",
            "מת עם שיקוי בתיק והקול שלי באוזן. ריספאון.",
            "בפעם הבאה שאני אומר שיקוי, תנסה ללחוץ עליו במקום למות. תקום.",
        ],
    ];

    /// Said after a death no warning came before, when the HP warning
    /// moves sooner: `{}` is the new mark, in percent. One sentence after
    /// the death line, in the same voice — not a settings dialog spliced
    /// onto it ("…try not to suck this time. I'll warn you sooner from now
    /// on, under 35%.").
    pub const SOONER: [&[&str]; 3] = [
        &[
            "I'll shout earlier next time, from {}.",
            "Next time I'll say something sooner, at {} percent.",
            "I'll give you a heads-up earlier from now on, at {}.",
        ],
        &[
            "New rule: I yell at {}.",
            "I'll call it sooner next time. {} percent.",
            "From now on I shout at {}. Listen for it.",
        ],
        &[
            "Fine. I scream at {} from now on. Try to hear it.",
            "I'll start yelling at {} percent next time. Not that you'll listen.",
            "New rule: {} and I'm screaming. Maybe that gets through.",
        ],
    ];

    /// [`SOONER`] in Hebrew, card for card (`{}` is the new mark).
    pub const SOONER_HE: [&[&str]; 3] = [
        &[
            "בפעם הבאה אצעק מוקדם יותר, מ-{} אחוז.",
            "בפעם הבאה אגיד משהו מוקדם יותר, ב-{} אחוז.",
            "מעכשיו אזהיר אותך מוקדם יותר, ב-{} אחוז.",
        ],
        &[
            "חוק חדש: אני צועק ב-{} אחוז.",
            "בפעם הבאה אגיד את זה מוקדם יותר. {} אחוז.",
            "מעכשיו אני צועק ב-{} אחוז. תקשיב.",
        ],
        &[
            "יופי. מעכשיו אני צורח ב-{} אחוז. תנסה לשמוע.",
            "בפעם הבאה אתחיל לצעוק ב-{} אחוז. לא שתקשיב.",
            "חוק חדש: {} אחוז ואני צורח. אולי ככה זה ייכנס לך.",
        ],
    ];

    /// The level read one above the highest seen.
    pub const LEVEL_UP: [&[&str]; 3] = [
        &[
            "Level up! You're level {}.",
            "Yes! Level {}! Nice work.",
            "Ding! {}. Let's go!",
            "Level {}, look at you go!",
            "Woohoo, {}! Keep that pace up.",
            "There it is, level {}. Earned.",
            "{} already? You're flying.",
        ],
        &[
            "Level {}! Nice.",
            "Level {}. About time.",
            "{}. Good. Don't slow down now.",
            "Ding, {}. Keep grinding.",
            "Level {}. Put the points somewhere useful.",
            "That's {}. Took a while, but you got there.",
            "{}, ding. Back to work.",
        ],
        &[
            "Level {}. Took you long enough.",
            "Level {}. Finally.",
            "{}. Only took you the whole evening.",
            "Ding, {}. A snail with a keyboard could've done it faster.",
            "Level {}. I was starting to think it was broken.",
            "Oh look, {}. Don't get cocky, you still can't dodge.",
            "{}? Great. Now play like it.",
        ],
    ];

    /// [`LEVEL_UP`] in Hebrew, card for card.
    pub const LEVEL_UP_HE: [&[&str]; 3] = [
        &[
            "עלית רמה! אתה ברמה {}.",
            "יש! רמה {}! כל הכבוד.",
            "דינג! {}. יאללה!",
            "רמה {}, תראה אותך!",
            "יוהו, {}! תשמור על הקצב.",
            "הנה, רמה {}. מגיע לך.",
            "{} כבר? אתה טס.",
        ],
        &[
            "רמה {}! יפה.",
            "רמה {}. הגיע הזמן.",
            "{}. טוב. אל תאט עכשיו.",
            "דינג, {}. תמשיך לטחון.",
            "רמה {}. תשים את הנקודות במקום שימושי.",
            "זה {}. לקח זמן, אבל הגעת.",
            "{}, דינג. חזרה לעבודה.",
        ],
        &[
            "רמה {}. לקח לך נצח.",
            "רמה {}. סוף סוף.",
            "{}. לקח לך רק את כל הערב.",
            "דינג, {}. חילזון עם מקלדת היה עושה את זה יותר מהר.",
            "רמה {}. כבר התחלתי לחשוב שזה שבור.",
            "אוי תראו, {}. אל תתלהב, אתה עדיין לא יודע להתחמק.",
            "{}? מעולה. עכשיו תשחק בהתאם.",
        ],
    ];

    /// The game window seen for the first time.
    pub const SEEN: [&[&str]; 3] = [
        &[
            "I can see MapleStory.",
            "Got the game on screen. Let's play!",
            "There it is, MapleStory's up. I'm watching.",
            "Okay, I see the game. Go on, I've got your back.",
            "Game's on my screen now. Have fun!",
            "I've got eyes on MapleStory. Let's do this.",
            "MapleStory's up and I can see it. Ready when you are.",
        ],
        &[
            "I can see MapleStory.",
            "Game's up. I'm watching.",
            "There's the game. Don't embarrass me.",
            "Got it on screen. Go.",
            "MapleStory's up. Let's see what you've got.",
            "I see the game. Play properly.",
            "Window's up. I'm in.",
        ],
        &[
            "I can see MapleStory.",
            "Game's up. Let's watch you die.",
            "There's the game. Try to last five minutes.",
            "I see it. Oh boy, here we go.",
            "MapleStory's on. Time to get carried by my advice.",
            "Window's up. Pot before I have to tell you.",
            "Got the game. I'll be right here, judging.",
        ],
    ];

    /// [`SEEN`] in Hebrew, card for card.
    pub const SEEN_HE: [&[&str]; 3] = [
        &[
            "אני רואה את מייפלסטורי.",
            "המשחק על המסך. בוא נשחק!",
            "הנה, מייפלסטורי פתוח. אני צופה.",
            "אוקיי, אני רואה את המשחק. קדימה, אני מגבה אותך.",
            "המשחק אצלי על המסך. תהנה!",
            "אני איתך במייפלסטורי. יאללה, מתחילים.",
            "מייפלסטורי פתוח ואני רואה אותו. מוכן כשאתה מוכן.",
        ],
        &[
            "אני רואה את מייפלסטורי.",
            "המשחק פתוח. אני צופה.",
            "הנה המשחק. אל תבייש אותי.",
            "יש לי אותו על המסך. קדימה.",
            "מייפלסטורי פתוח. בוא נראה מה אתה שווה.",
            "אני רואה את המשחק. תשחק כמו שצריך.",
            "החלון פתוח. אני בפנים.",
        ],
        &[
            "אני רואה את מייפלסטורי.",
            "המשחק פתוח. בוא נראה אותך מת.",
            "הנה המשחק. תנסה להחזיק מעמד חמש דקות.",
            "אני רואה אותו. אוי ואבוי, מתחילים.",
            "מייפלסטורי פתוח. העצות שלי הולכות לסחוב אותך.",
            "החלון פתוח. תשתה לפני שאני צריך להגיד לך.",
            "רואה את המשחק. אני פה, שופט אותך.",
        ],
    ];

    /// The game window gone for a while (and no reason known).
    pub const LOST: [&[&str]; 3] = [
        &[
            "I lost sight of the game window.",
            "Hm, the game's gone from my view. Alt-tabbed?",
            "I can't see MapleStory anymore. Bring it back when you're ready.",
            "Game window's gone. I'll wait.",
            "Lost the game. Did you minimise it?",
            "MapleStory dropped out of view. Taking a break?",
            "I can't see the game right now. I'm still here though.",
        ],
        &[
            "I lost sight of the game window.",
            "Game's gone. Where'd you go?",
            "Can't see MapleStory. Bring it back.",
            "You minimised it. I'm blind now.",
            "Lost the window. Alt-tab back when you're done.",
            "No game on screen. I'll wait.",
            "MapleStory's off my screen. Hurry up.",
        ],
        &[
            "I lost sight of the game window.",
            "Game's gone. Rage quit already?",
            "Can't see MapleStory. Checking your socials mid-grind, classic.",
            "You hid the game from me. Coward.",
            "Window's gone. Taking a break from losing?",
            "No game. Nothing to roast. Hurry back.",
            "MapleStory vanished. So did your EXP rate.",
        ],
    ];

    /// [`LOST`] in Hebrew, card for card.
    pub const LOST_HE: [&[&str]; 3] = [
        &[
            "איבדתי את חלון המשחק.",
            "הממ, המשחק נעלם לי. עשית אלט-טאב?",
            "אני לא רואה יותר את מייפלסטורי. תחזיר אותו כשאתה מוכן.",
            "חלון המשחק נעלם. אני מחכה.",
            "איבדתי את המשחק. הקטנת אותו?",
            "מייפלסטורי נעלם לי מהעין. לוקח הפסקה?",
            "אני לא רואה את המשחק כרגע. אבל אני עדיין פה.",
        ],
        &[
            "איבדתי את חלון המשחק.",
            "המשחק נעלם. לאן הלכת?",
            "לא רואה את מייפלסטורי. תחזיר אותו.",
            "הקטנת אותו. אני עיוור עכשיו.",
            "איבדתי את החלון. תעשה אלט-טאב בחזרה כשתסיים.",
            "אין משחק על המסך. אני מחכה.",
            "מייפלסטורי ירד לי מהמסך. תזדרז.",
        ],
        &[
            "איבדתי את חלון המשחק.",
            "המשחק נעלם. כבר עשית רייג' קוויט?",
            "לא רואה את מייפלסטורי. בודק את הרשתות באמצע הגריינד, קלאסי.",
            "החבאת ממני את המשחק. פחדן.",
            "החלון נעלם. לוקח הפסקה מלהפסיד?",
            "אין משחק, אין על מה לרדת. תחזור מהר.",
            "מייפלסטורי נעלם. וגם קצב האקספי שלך.",
        ],
    ];

    /// The game window back after it was lost.
    pub const AGAIN: [&[&str]; 3] = [
        &[
            "I can see the game again.",
            "There we go, game's back.",
            "Welcome back! Game's on screen.",
            "Got it again. Let's keep going.",
            "Game's back in view. I'm watching.",
            "And we're back. Hi!",
            "I see MapleStory again. Carry on.",
        ],
        &[
            "I can see the game again.",
            "Back. Good.",
            "Game's back. Let's go.",
            "There you are. Keep playing.",
            "Window's back. I'm watching again.",
            "Took you long enough. Game's up.",
            "Game's back. Where were we?",
        ],
        &[
            "I can see the game again.",
            "Oh, you're back. Thrilling.",
            "Game's back. Let's see how fast you die this time.",
            "There it is. Try not to tab out mid-boss again.",
            "Welcome back. Your HP missed you, apparently.",
            "Game's up. Resume the clown show.",
            "Back already? I was enjoying the quiet.",
        ],
    ];

    /// [`AGAIN`] in Hebrew, card for card.
    pub const AGAIN_HE: [&[&str]; 3] = [
        &[
            "אני רואה שוב את המשחק.",
            "הנה, המשחק חזר.",
            "ברוך השב! המשחק על המסך.",
            "תפסתי אותו שוב. בוא נמשיך.",
            "המשחק שוב מול העיניים. אני צופה.",
            "וחזרנו. היי!",
            "אני רואה שוב את מייפלסטורי. תמשיך.",
        ],
        &[
            "אני רואה שוב את המשחק.",
            "חזר. טוב.",
            "המשחק חזר. יאללה.",
            "הנה אתה. תמשיך לשחק.",
            "החלון חזר. אני שוב צופה.",
            "לקח לך נצח. המשחק פתוח.",
            "המשחק חזר. איפה היינו?",
        ],
        &[
            "אני רואה שוב את המשחק.",
            "אה, חזרת. מרגש.",
            "המשחק חזר. בוא נראה כמה מהר תמות הפעם.",
            "הנה הוא. תנסה לא לצאת שוב באמצע בוס.",
            "ברוך השב. החיים שלך התגעגעו אליך, כנראה.",
            "המשחק פתוח. ממשיכים בהופעת הליצנים.",
            "כבר חזרת? נהניתי מהשקט.",
        ],
    ];

    /// Told to be quiet (shown on the phone, not said).
    pub const MUTED: [&[&str]; 3] = [
        &[
            "Muted. I'll keep writing to your phone.",
            "Okay, going quiet. I'll still write here.",
            "Shh, got it. Text only from now.",
            "Muted! Say unmute when you want me back.",
            "Quiet mode on. Still watching, just not talking.",
            "Lips sealed. You'll see me on the phone.",
            "No voice, just text. Right here if you want me back.",
        ],
        &[
            "Muted. I'll keep writing to your phone.",
            "Fine, quiet. I'll type.",
            "Muted. You'll still get it in writing.",
            "Okay, mouth shut. Phone's still on.",
            "Quiet. Read your phone, then.",
            "Muted. Don't die while I'm not talking.",
            "No voice. Text only till you say unmute.",
        ],
        &[
            "Muted. I'll keep writing to your phone.",
            "Muted. Enjoy dying in silence.",
            "Fine, I'll type my insults instead.",
            "Quiet mode. You'll still read what you did wrong.",
            "Muted. The phone still sees everything.",
            "Shutting up. Not because you're right.",
            "No voice. The roast continues in writing.",
        ],
    ];

    /// [`MUTED`] in Hebrew, card for card.
    pub const MUTED_HE: [&[&str]; 3] = [
        &[
            "מושתק. אמשיך לכתוב לך בטלפון.",
            "אוקיי, אני שותק. עדיין אכתוב פה.",
            "ששש, הבנתי. רק טקסט מעכשיו.",
            "מושתק! תגיד לי לחזור לדבר כשתרצה אותי בחזרה.",
            "מצב שקט. עדיין צופה, רק לא מדבר.",
            "הפה סגור. תראה אותי בטלפון.",
            "בלי קול, רק טקסט. אני פה אם תרצה אותי בחזרה.",
        ],
        &[
            "מושתק. אמשיך לכתוב לך בטלפון.",
            "בסדר, שקט. אני אקליד.",
            "מושתק. עדיין תקבל הכול בכתב.",
            "אוקיי, פה סגור. הטלפון עדיין פועל.",
            "שקט. אז תקרא בטלפון.",
            "מושתק. אל תמות בזמן שאני לא מדבר.",
            "בלי קול. רק טקסט עד שתגיד לי לחזור לדבר.",
        ],
        &[
            "מושתק. אמשיך לכתוב לך בטלפון.",
            "מושתק. תהנה למות בשקט.",
            "יופי, אז אקליד את העלבונות במקום.",
            "מצב שקט. עדיין תקרא מה עשית לא בסדר.",
            "מושתק. הטלפון עדיין רואה הכול.",
            "סותם. לא כי אתה צודק.",
            "בלי קול. הירידות ממשיכות בכתב.",
        ],
    ];

    /// Told to talk again.
    pub const UNMUTED: [&[&str]; 3] = [
        &[
            "I'm back.",
            "Unmuted! Hi again.",
            "Voice is back on. Missed you.",
            "Okay, talking again.",
            "And I'm back. What'd I miss?",
            "Unmuted. Let's go!",
            "Back on the mic.",
        ],
        &[
            "I'm back.",
            "Unmuted. Behave.",
            "Talking again. Did you pot while I was gone?",
            "Back on. Let's see the damage.",
            "Voice on. Listen up.",
            "Okay, I can talk. What'd you break?",
            "Mic's back. Keep playing.",
        ],
        &[
            "I'm back.",
            "Unmuted. Did you survive without me?",
            "Back. Let's see what you ruined in the quiet.",
            "Oh good, I can roast you again.",
            "Voice is on. Your HP had better be too.",
            "Miss me? Didn't think so. Pot anyway.",
            "Unmuted, and just in time to watch you mess up.",
        ],
    ];

    /// [`UNMUTED`] in Hebrew, card for card.
    pub const UNMUTED_HE: [&[&str]; 3] = [
        &[
            "הנה אני.",
            "חזרתי לדבר! היי שוב.",
            "הקול חזר. התגעגעתי.",
            "אוקיי, מדבר שוב.",
            "ואני חזרתי. מה פספסתי?",
            "מדבר שוב. יאללה!",
            "חזרתי למיקרופון.",
        ],
        &[
            "חזרתי.",
            "חזרתי לדבר. תתנהג יפה.",
            "מדבר שוב. שתית משהו בזמן שלא הייתי?",
            "שוב איתך. בוא נראה את הנזק.",
            "הקול פועל. תקשיב.",
            "אוקיי, אני יכול לדבר. מה שברת?",
            "המיקרופון חזר. תמשיך לשחק.",
        ],
        &[
            "חזרתי.",
            "חזרתי. שרדת בלעדיי?",
            "נו, מה הרסת בשקט?",
            "יופי, אני יכול לרדת עליך שוב.",
            "הקול חזר. כדאי שגם בר החיים שלך יהיה למעלה.",
            "התגעגעת? חשבתי שלא. תשתה בכל מקרה.",
            "חזרתי לדבר, בדיוק בזמן לראות אותך מפשל.",
        ],
    ];

    /// Alerts held for want of an answer, said to a player who has said
    /// nothing for a while ([`super::HOLD_AWAY_SECS`]), or nothing this
    /// session: as far as it knows the room is empty. A card that counts
    /// the warnings counts [`super::UNANSWERED_MAX`] of them.
    pub const HOLD_AWAY: [&[&str]; 3] = [
        &[
            "You're not answering, so I'll hold my warnings until you say something.",
            "No sign of you, so I'll keep quiet for a bit. Say anything and I'm back on it.",
            "I'll stop nagging for a while. Just talk to me when you're back.",
            "Nobody's potting, nobody's talking. I'll pipe down till you say something.",
            "Going quiet on the warnings for now. A word from you and they're back.",
            "You're away, or the bar's lying to me. Either way, I'll hush until you speak.",
            "Holding the warnings for a bit. Say something when you're back and I'll carry on.",
        ],
        &[
            "You're not answering, so I'll hold my warnings until you say something.",
            "Four warnings, zero answers. I'm done until you talk.",
            "Fine, I'll shut up about it. Say something when you're back.",
            "Nobody home? Warnings on hold till you speak.",
            "I've said it enough. Holding the rest until I hear from you.",
            "No pot, no word, no point. I'll wait.",
            "Warnings paused. Talk to me when you're actually here.",
        ],
        &[
            "You're not answering, so I'll hold my warnings until you say something.",
            "Talking to a wall here. I'll stop until the wall says something.",
            "Four warnings and nothing. Lose your HP in peace, I'll wait.",
            "You're either AFK or ignoring me. Either way, I'm done until you speak.",
            "Fine. Get wrecked quietly. Say something and I'll start caring again.",
            "No answer, no pot, no respect. Warnings on hold.",
            "I'll stop wasting my breath. Speak up when you're back from wherever.",
        ],
    ];

    /// [`HOLD_AWAY`] in Hebrew, card for card.
    pub const HOLD_AWAY_HE: [&[&str]; 3] = [
        &[
            "אתה לא עונה, אז אני עוצר את האזהרות עד שתגיד משהו.",
            "אין סימן ממך, אז אני אשתוק קצת. תגיד משהו ואני חוזר לזה.",
            "אני מפסיק לנדנד בינתיים. פשוט תדבר איתי כשתחזור.",
            "אף אחד לא שותה, אף אחד לא מדבר. אני משתתק עד שתגיד משהו.",
            "עוצר עם האזהרות בינתיים. מילה ממך והן חוזרות.",
            "או שאתה לא פה, או שהבר משקר לי. בכל מקרה, אני שותק עד שתדבר.",
            "עוצר את האזהרות לרגע. תגיד משהו כשתחזור ואני אמשיך.",
        ],
        &[
            "אתה לא עונה, אז אני עוצר את האזהרות עד שתגיד משהו.",
            "ארבע אזהרות, אפס תשובות. סיימתי עד שתדבר.",
            "בסדר, אני מפסיק עם זה. תגיד משהו כשתחזור.",
            "אין אף אחד בבית? האזהרות בהמתנה עד שתדבר.",
            "אמרתי את זה מספיק. שומר את השאר עד שאשמע ממך.",
            "אין שיקוי, אין מילה, אין טעם. אני מחכה.",
            "האזהרות בהפסקה. תדבר איתי כשאתה באמת פה.",
        ],
        &[
            "אתה לא עונה, אז אני עוצר את האזהרות עד שתגיד משהו.",
            "אני מדבר פה אל הקיר. אפסיק עד שהקיר יגיד משהו.",
            "ארבע אזהרות וכלום. תמשיך לחטוף בשקט, אני מחכה.",
            "או שאתה לא ליד המחשב או שאתה מתעלם ממני. בכל מקרה, סיימתי עד שתדבר.",
            "יופי. תתרסק בשקט. תגיד משהו ואני אתחיל שוב לדאוג.",
            "אין תשובה, אין שיקוי, אין כבוד. האזהרות בהמתנה.",
            "חבל על המילים. תדבר כשתחזור, איפה שלא היית.",
        ],
    ];

    /// Alerts held for want of an answer, said to a player who spoke
    /// lately: someone is there, and not potting. A friend stops nagging —
    /// no card says they are away, or asks them to come back ("Talk to me
    /// when you're back" three minutes after "ok, I'm back"). A card that
    /// counts the warnings counts [`super::UNANSWERED_MAX`] of them.
    pub const HOLD_HERE: [&[&str]; 3] = [
        &[
            "Okay, I'll stop nagging. Pots are yours to press.",
            "I've said my bit. I'll leave the potions to you.",
            "Alright, no more potion talk from me. Your call.",
            "Four warnings is plenty. I'll let you play.",
            "Fair enough, you know where the pots are. I'll hush.",
            "I'll drop it. Pot when it suits you.",
        ],
        &[
            "Fine. I'll shut up about it.",
            "Four warnings. I'm done nagging.",
            "Your bar, your call. I'm done.",
            "Okay. No more pot talk from me.",
            "I've said my piece. Pot or don't.",
            "Not my bar. I'll stop.",
        ],
        &[
            "Fine. Ignore me. I'm done.",
            "Cool, I'm talking to myself. I'll stop.",
            "Four warnings ignored. Enjoy the floor.",
            "Fine, do it your way. I'll watch.",
            "Noted: potions are beneath you. I'll shut up.",
            "Okay, I give up. Pot whenever, or never.",
        ],
    ];

    /// [`HOLD_HERE`] in Hebrew, card for card: no card says they are away, or
    /// asks them back.
    pub const HOLD_HERE_HE: [&[&str]; 3] = [
        &[
            "אוקיי, אני מפסיק לנדנד. השיקויים בידיים שלך.",
            "אמרתי את שלי. משאיר לך את השיקויים.",
            "טוב, לא עוד דיבורים על שיקויים ממני. ההחלטה שלך.",
            "ארבע אזהרות זה מספיק. אני נותן לך לשחק.",
            "בסדר גמור, אתה יודע איפה השיקויים. אני שותק.",
            "אני עוזב את זה. תשתה כשמתאים לך.",
        ],
        &[
            "בסדר. אני סותם.",
            "ארבע אזהרות. סיימתי לנדנד.",
            "הבר שלך, ההחלטה שלך. סיימתי.",
            "אוקיי. ממני לא תשמע יותר על שיקויים.",
            "אמרתי את מה שהיה לי. תשתה או אל תשתה.",
            "לא הבר שלי. אני מפסיק.",
        ],
        &[
            "יופי. תתעלם ממני. סיימתי.",
            "מגניב, אני מדבר לעצמי. אני מפסיק.",
            "ארבע אזהרות שהתעלמת מהן. תהנה מהרצפה.",
            "יופי, תעשה את זה בדרך שלך. אני אצפה.",
            "רשמתי: שיקויים זה מתחת לכבוד שלך. אני סותם.",
            "אוקיי, אני מוותר. תשתה מתי שבא לך, או אף פעם.",
        ],
    ];

    /// A potion at last, while the warnings were held and the player had
    /// been told so lately: one short word that it saw.
    pub const POTTED_AT_LAST: [&[&str]; 3] = [
        &[
            "There you go.",
            "That's better.",
            "Phew. There we go.",
            "Nice, that's more like it.",
        ],
        &[
            "Finally.",
            "There it is.",
            "Better.",
            "Good. Keep it there.",
        ],
        &[
            "Oh look, it found the pot key.",
            "Wow, a potion. Character development.",
            "Finally. Was that so hard?",
            "It lives. And it pots.",
        ],
    ];

    /// [`POTTED_AT_LAST`] in Hebrew, card for card.
    pub const POTTED_AT_LAST_HE: [&[&str]; 3] = [
        &[
            "הנה, ככה.",
            "זה יותר טוב.",
            "פיו. הנה.",
            "יפה, ככה זה צריך להיות.",
        ],
        &["סוף סוף.", "הנה.", "יותר טוב.", "טוב. תשאיר את זה ככה."],
        &[
            "אוי תראו, הוא מצא את המקש של השיקוי.",
            "וואו, שיקוי. התפתחות אישית.",
            "סוף סוף. זה היה כל כך קשה?",
            "הוא חי. והוא שותה.",
        ],
    ];

    /// The game idle for twenty minutes with not a word from the player:
    /// what a friend in the room says, once. All it knows is that nothing
    /// has moved on the screen: no card says they are grinding, or asleep
    /// at a keyboard that is moving, or how long it has been.
    pub const STILL_THERE: [&[&str]; 3] = [
        &[
            "You still there? It's gone quiet over here.",
            "Still with me? Nothing's moved in a while.",
            "Hey, you around? The game's been sitting still for a bit.",
            "Just checking in. You there?",
            "Went quiet in here. Everything okay?",
            "Still there? Say something if you're back.",
        ],
        &[
            "You still there?",
            "Went quiet over here. Still with me?",
            "Nothing's moved in a while. You there?",
            "Oi. Still around?",
            "Game's just sitting there. You still here?",
            "A while now with nothing moving. You there?",
        ],
        &[
            "Hello? The mobs are getting bored.",
            "You still there, or did the chair take over? Nothing's moving.",
            "Your character's been standing there a while. You alive?",
            "Nothing's moved in ages. Blink if you're there.",
            "Standing still this long is a skill. You there?",
            "Did you leave? The game's just sitting there.",
        ],
    ];

    /// [`STILL_THERE`] in Hebrew, card for card.
    pub const STILL_THERE_HE: [&[&str]; 3] = [
        &[
            "אתה עדיין פה? נהיה ממש שקט.",
            "עדיין איתי? שום דבר לא זז כבר הרבה זמן.",
            "היי, אתה בסביבה? המשחק עומד במקום כבר קצת.",
            "רק בודק. אתה שם?",
            "נהיה פה שקט. הכול בסדר?",
            "עדיין שם? תגיד משהו אם חזרת.",
        ],
        &[
            "אתה עדיין שם?",
            "נהיה שקט פה. עדיין איתי?",
            "שום דבר לא זז כבר הרבה זמן. אתה שם?",
            "הלו? עדיין בסביבה?",
            "המשחק סתם עומד שם. אתה עדיין פה?",
            "כבר הרבה זמן ששום דבר לא זז. אתה שם?",
        ],
        &[
            "הלו? המפלצות מתחילות להשתעמם.",
            "אתה עדיין שם, או שהכיסא השתלט? שום דבר לא זז.",
            "הדמות שלך עומדת שם כבר הרבה זמן. אתה חי?",
            "שום דבר לא זז כבר נצח. תמצמץ אם אתה שם.",
            "לעמוד במקום כל כך הרבה זמן זה כישרון. אתה שם?",
            "הלכת? המשחק סתם עומד שם.",
        ],
    ];

    /// A bar read low for minutes while EXP came in: it is not believed,
    /// and its warnings are held (see [`super::Low`]). `{}` is the bar.
    pub const MISREAD: [&[&str]; 3] = [
        &[
            "I think I'm reading your {} wrong — holding the {} warnings for now.",
            "Your {} bar's read low for ages while you keep leveling: I'm probably misreading it. Holding the {} warnings.",
            "That {} reading can't be right. I'll hold the {} warnings until it looks sane again.",
        ],
        &[
            "I'm reading your {} wrong. Holding the {} warnings.",
            "{} says low, EXP says you're fine. I'll shut up about {} until it reads right.",
            "That {} bar's lying to me. No more {} warnings till it comes back up.",
        ],
        &[
            "Either your {} bar's broken or you're immortal. Holding the {} warnings.",
            "Four warnings and you kept killing things. I'm reading your {} wrong, so I'll shut up about it.",
            "Your {} reads dead and you're leveling. Fine, I'll stop trusting it.",
        ],
    ];

    /// [`MISREAD`] in Hebrew, card for card. `{}` is the bar's Hebrew name
    /// (חיים, מאנה), which takes ה before it and nothing that agrees with
    /// it: the one is plural, the other feminine.
    pub const MISREAD_HE: [&[&str]; 3] = [
        &[
            "נראה לי שאני קורא לא נכון את בר ה{} שלך. עוצר את אזהרות ה{} בינתיים.",
            "הבר של ה{} נמוך כבר נצח ואתה ממשיך לצבור אקספי: כנראה אני קורא אותו לא נכון. עוצר את אזהרות ה{}.",
            "הקריאה הזאת של ה{} לא יכולה להיות נכונה. אעצור את אזהרות ה{} עד שזה ייראה הגיוני.",
        ],
        &[
            "אני קורא את בר ה{} שלך לא נכון. עוצר את אזהרות ה{}.",
            "הבר של ה{} אומר נמוך, האקספי אומר שאתה בסדר. אני מפסיק עם אזהרות ה{} עד שזה ייקרא נכון.",
            "הבר של ה{} משקר לי. אין יותר אזהרות {} עד שהוא יעלה בחזרה.",
        ],
        &[
            "או שהבר של ה{} שלך שבור או שאתה בן אלמוות. עוצר את אזהרות ה{}.",
            "ארבע אזהרות ואתה ממשיך להרוג דברים. אני קורא את בר ה{} שלך לא נכון, אז אני שותק.",
            "הבר של ה{} שלך אומר שאתה מת ואתה עולה רמות. יופי, אני מפסיק לסמוך עליו.",
        ],
    ];

    /// A deck: one list per attitude (friendly, blunt, savage).
    pub type Cards = [&'static [&'static str]; 3];

    /// Every deck, by name, with its Hebrew twin: for tests and tools.
    pub const ALL: &[(&str, Cards, Cards)] = &[
        ("beating", BEATING, BEATING_HE),
        ("HP low", HP_LOW, HP_LOW_HE),
        ("HP low, again", HP_LOW_AGAIN, HP_LOW_AGAIN_HE),
        ("HP low, ask", HP_LOW_ASK, HP_LOW_ASK_HE),
        ("MP low", MP_LOW, MP_LOW_HE),
        ("MP low, again", MP_LOW_AGAIN, MP_LOW_AGAIN_HE),
        ("MP low, ask", MP_LOW_ASK, MP_LOW_ASK_HE),
        ("death", DEATH, DEATH_HE),
        ("death, warned", DEATH_WARNED, DEATH_WARNED_HE),
        ("sooner", SOONER, SOONER_HE),
        ("level up", LEVEL_UP, LEVEL_UP_HE),
        ("game seen", SEEN, SEEN_HE),
        ("game lost", LOST, LOST_HE),
        ("game seen again", AGAIN, AGAIN_HE),
        ("muted", MUTED, MUTED_HE),
        ("unmuted", UNMUTED, UNMUTED_HE),
        ("warnings held, away", HOLD_AWAY, HOLD_AWAY_HE),
        ("warnings held, here", HOLD_HERE, HOLD_HERE_HE),
        ("potted at last", POTTED_AT_LAST, POTTED_AT_LAST_HE),
        ("still there", STILL_THERE, STILL_THERE_HE),
        ("misread", MISREAD, MISREAD_HE),
    ];

    /// A bar's readings swinging (a bar guessed at, not read): its warnings
    /// are held, and the player is told so on the phone (not aloud). `{}`
    /// is the bar.
    pub const UNSTEADY: &str =
        "My {} readings are jumping around, so I'm holding the {} warnings until they settle.";
    /// [`UNSTEADY`] in Hebrew (`{}` as in [`MISREAD_HE`]).
    pub const UNSTEADY_HE: &str =
        "הקריאות של בר ה{} קופצות, אז אני עוצר את אזהרות ה{} עד שזה יירגע.";

    /// A moment marked: `{}` is how many this session.
    pub const MARKED: &str = "Marked. That's mark {}.";
    pub const MARKED_HE: &str = "סימנתי. זה סימון מספר {}.";

    /// The word that a language asked for out loud is taken ("talk to me
    /// in Hebrew"), in that language, said by the PC when it switches
    /// (`ask_language`): Hebrew's in Hebrew; the rest in English, `{lang}`
    /// its name, translated into it as its own lines are. (No twins: only
    /// Hebrew has a list of its own. Not among [`ALL`].)
    pub const LANGUAGE_TAKEN_EN: [&[&str]; 3] = [
        &[
            "Sure — {lang} from now on.",
            "You got it: {lang} it is.",
            "Love it. {lang} from here on.",
        ],
        &[
            "Fine, {lang} from now on.",
            "{lang} it is.",
            "Done. {lang} now.",
        ],
        &[
            "{lang}? Fine, genius.",
            "Ugh, fine. {lang} it is.",
            "Whatever you say. {lang} now.",
        ],
    ];
    pub const LANGUAGE_TAKEN_HE: [&[&str]; 3] = [
        &[
            "בטח, מעכשיו בעברית.",
            "בכיף, עוברים לעברית.",
            "סגור, מדברים עברית.",
        ],
        &["סבבה, עברית מעכשיו.", "עברית. סגור.", "יאללה, בעברית."],
        &[
            "עברית? סבבה, גאון.",
            "אוף, טוב. עברית.",
            "מה שתגיד. עברית מעכשיו.",
        ],
    ];

    /// The decks in [`ALL`] that are dealt a few times a night at most
    /// (the mark moves up four times and no more; a bar is doubted once a
    /// fight, after four lines; a potion ends a hold once): three ways to
    /// say it is plenty, where the others have six.
    pub const SHORT: &[&str] = &["sooner", "misread", "potted at last"];
}

/// Something worth speaking up about, seen in a frame; its line is dealt
/// once it is known to be said (`pace` holds alerts nobody answers).
#[derive(Debug, Clone, Copy, PartialEq)]
enum Alert {
    Beating(Gauge),
    /// A low bar, and which line of the fight it is (see [`Cadence`]).
    HpLow {
        hp: Gauge,
        cadence: Cadence,
    },
    MpLow {
        mp: Gauge,
        cadence: Cadence,
    },
    /// Whether a warning came just before (a card may say "told you" only
    /// then), and the threshold the HP warning moves to, when it does.
    Death {
        warned: bool,
        sooner: Option<f32>,
    },
    LevelUp(u32),
}

impl Alert {
    /// News (a death, a level-up) rather than a warning: said whatever
    /// answered the warnings, counted against nothing, and told rather
    /// than shouted ([`Kind::Alert`], not [`Kind::Warning`]).
    fn is_news(&self) -> bool {
        matches!(self, Alert::Death { .. } | Alert::LevelUp(_))
    }
}

impl Tally {
    /// A warning said (news is counted elsewhere: deaths, level-ups).
    fn count(&mut self, alert: &Alert) {
        match alert {
            Alert::Beating(_) => self.beating += 1,
            Alert::HpLow { .. } => self.hp_low += 1,
            Alert::MpLow { .. } => self.mp_low += 1,
            Alert::Death { .. } | Alert::LevelUp(_) => {}
        }
    }
}

/// A [`Deck`] per situation, so that each is dealt on its own.
#[derive(Debug)]
struct Decks {
    beating: Deck,
    hp_low: Deck,
    hp_low_again: Deck,
    hp_low_ask: Deck,
    mp_low: Deck,
    mp_low_again: Deck,
    mp_low_ask: Deck,
    death: Deck,
    death_warned: Deck,
    sooner: Deck,
    level_up: Deck,
    seen: Deck,
    lost: Deck,
    again: Deck,
    muted: Deck,
    unmuted: Deck,
    hold_away: Deck,
    hold_here: Deck,
    potted_at_last: Deck,
    still_there: Deck,
    misread: Deck,
    /// The instant answers ("HP 76%."): a deck per kind of answer.
    instant: instant::Decks,
}

impl Decks {
    /// Every deck shuffled by the session's `seed` (each by its own lines
    /// too, so none are dealt in step).
    fn seeded(seed: u64) -> Self {
        Self {
            beating: Deck::seeded(seed),
            hp_low: Deck::seeded(seed),
            hp_low_again: Deck::seeded(seed),
            hp_low_ask: Deck::seeded(seed),
            mp_low: Deck::seeded(seed),
            mp_low_again: Deck::seeded(seed),
            mp_low_ask: Deck::seeded(seed),
            death: Deck::seeded(seed),
            death_warned: Deck::seeded(seed),
            sooner: Deck::seeded(seed),
            level_up: Deck::seeded(seed),
            seen: Deck::seeded(seed),
            lost: Deck::seeded(seed),
            again: Deck::seeded(seed),
            muted: Deck::seeded(seed),
            unmuted: Deck::seeded(seed),
            hold_away: Deck::seeded(seed),
            hold_here: Deck::seeded(seed),
            potted_at_last: Deck::seeded(seed),
            still_there: Deck::seeded(seed),
            misread: Deck::seeded(seed),
            instant: instant::Decks::seeded(seed),
        }
    }

    /// Whether each deck's first round opens with its lead (see
    /// [`Deck::lead_first`]). The instant answers are not among them: an
    /// answer is an answer, the plain one is the right first one, and
    /// their lists keep cards that presume an earlier answer ("Still
    /// level 165.") off the lead.
    fn lead_first(&mut self, lead_first: bool) {
        for deck in [
            &mut self.beating,
            &mut self.hp_low,
            &mut self.hp_low_again,
            &mut self.hp_low_ask,
            &mut self.mp_low,
            &mut self.mp_low_again,
            &mut self.mp_low_ask,
            &mut self.death,
            &mut self.death_warned,
            &mut self.sooner,
            &mut self.level_up,
            &mut self.seen,
            &mut self.lost,
            &mut self.again,
            &mut self.muted,
            &mut self.unmuted,
            &mut self.hold_away,
            &mut self.hold_here,
            &mut self.potted_at_last,
            &mut self.still_there,
            &mut self.misread,
        ] {
            deck.lead_first(lead_first);
        }
    }
}

/// A bar, as its own lines name it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Bar {
    Hp,
    Mp,
}

impl Bar {
    /// "HP", "MP" — or in Hebrew "חיים", "מאנה" (with no Latin letter, a
    /// Hebrew line is said as it is: see [`lines`]).
    fn name(self, hebrew: bool) -> &'static str {
        match (self, hebrew) {
            (Bar::Hp, false) => "HP",
            (Bar::Mp, false) => "MP",
            (Bar::Hp, true) => "חיים",
            (Bar::Mp, true) => "מאנה",
        }
    }
}

/// The next card of a deck (`en`, and its Hebrew twin `he`) in `attitude`,
/// in Hebrew or English: dealt by place in the English list's order, then
/// read off the list of the language it speaks — so the two languages are
/// one deck, and a switch between them goes on where it was.
fn deal(
    deck: &mut Deck,
    attitude: Attitude,
    hebrew: bool,
    en: lines::Cards,
    he: lines::Cards,
) -> &'static str {
    let at = deck.deal_index(attitude, en);
    let card = |cards: lines::Cards| attitude.lines(cards).get(at).copied();
    (if hebrew { card(he) } else { None })
        .or_else(|| card(en))
        .unwrap_or("")
}

/// A seed for the session's decks: the clock and the process, so that no
/// two nights deal them alike.
fn session_seed() -> u64 {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    nanos ^ (std::process::id() as u64).rotate_left(32)
}

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
    /// A companion for a session: its decks are shuffled for this night.
    pub fn new(settings: Settings) -> Self {
        Self::seeded(settings, session_seed())
    }

    /// A companion whose decks are shuffled by `seed` (the same seed
    /// replays a session's lines exactly: for tests).
    pub fn seeded(settings: Settings, seed: u64) -> Self {
        Self {
            settings,
            last: None,
            seen_at: None,
            announced_lost: false,
            ever_seen: false,
            low_hp: Low::new(),
            low_mp: Low::new(),
            decks: Decks::seeded(seed),
            zero_hp_frames: 0,
            dead: false,
            hp_lately: std::collections::VecDeque::new(),
            falling_frames: 0,
            fall_told: f64::NEG_INFINITY,
            fall_at: f64::NEG_INFINITY,
            fight_lines: 0,
            fall_hp: f32::INFINITY,
            fall_answered: false,
            fall_up_frames: 0,
            trust: 0,
            trust_floor: f32::INFINITY,
            fight_low: f32::INFINITY,
            watch: None,
            listening_until: f64::NEG_INFINITY,
            muted: false,
            exp: ExpTracker::new(),
            last_level: None,
            top_level: None,
            last_name: None,
            level_candidate: None,
            levels_before: 0,
            wraps: std::collections::VecDeque::new(),
            switch_since: None,
            announced_level_up: f64::NEG_INFINITY,
            marks: 0,
            now: 0.0,
            spoken: std::collections::VecDeque::new(),
            turn: 0,
            hp_steady: Steadiness::new(),
            mp_steady: Steadiness::new(),
            zero_hp_since: 0.0,
            mp_lately: std::collections::VecDeque::new(),
            mp_empty: None,
            unanswered: 0,
            alert_at: f64::NEG_INFINITY,
            hp_at_alert: None,
            hp_up_frames: 0,
            mp_at_alert: None,
            mp_up_frames: 0,
            potted: false,
            exp_at_alert: None,
            exp_peak: None,
            level_at_alert: None,
            spoke_at: f64::NEG_INFINITY,
            quiet_before: None,
            hold_until: f64::NEG_INFINITY,
            hold_told: f64::NEG_INFINITY,
            deaths: 0,
            last_death: f64::NEG_INFINITY,
            hp_minute: std::collections::VecDeque::new(),
            changed_at: f64::NEG_INFINITY,
            hp_at_change: None,
            exp_at_change: None,
            mp_before: None,
            mp_jump: None,
            hp_before: None,
            hp_jump: None,
            potion_at: f64::NEG_INFINITY,
            started_at: None,
            view_since: f64::NEG_INFINITY,
            asked_still_there: false,
            hebrew: false,
            warned: Tally::default(),
        }
    }

    /// The player's language is `locale` now (the phone's, or one they
    /// asked for out loud; `None`: not known yet). Hebrew (`he…`, or the
    /// old `iw…`) has its own lines dealt from the decks' Hebrew twins
    /// ([`lines`]), said at once — no model to translate them first, and
    /// the phone shows them in Hebrew too. Any other language gets the
    /// English cards (translated on their way to the voice, as before).
    /// One deck deals both languages, by place: a switch mid-session goes
    /// on with each deck where it was.
    pub fn set_language(&mut self, locale: Option<&str>) {
        self.hebrew = locale.is_some_and(|locale| {
            let lower = locale.trim().to_ascii_lowercase();
            matches!(lower.split(['-', '_']).next(), Some("he" | "iw"))
        });
    }

    /// Whether its own lines are said in Hebrew.
    pub fn speaks_hebrew(&self) -> bool {
        self.hebrew
    }

    /// Whether the player is `known` — not on their first sessions with
    /// it. A new player hears each deck's lead first (the most informative
    /// line, the one with the number); a known one has heard the leads
    /// enough, and gets the first beating, death and level-up of the
    /// night in another order every night, like the rest (the lead-first
    /// rule had them the same three lines night after night). To be
    /// called before the first line is dealt: a deck in play keeps its
    /// order.
    pub fn settled(&mut self, known: bool) {
        self.decks.lead_first(!known);
    }

    /// The player said something (to MapleSyrup or near it): a sign of
    /// life, and the end of any hold on the alerts. The silence it ended,
    /// when it was long, is kept for the reply (`so_far`).
    pub fn player_spoke(&mut self, now: f64) {
        let quiet = now - self.spoke_at;
        self.quiet_before =
            (self.spoke_at.is_finite() && quiet >= QUIET_BEFORE_SECS).then_some((quiet, now));
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
    /// or two the player repeats from MapleSyrup's line stays theirs, and so
    /// does a run they quote after words of their own, or a sentence that
    /// starts with "no" (a correction, whatever it quotes).
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
        // A sentence that starts with "no", "not", "wrong" (or "stop") — a
        // word MapleSyrup did not just use itself — is a correction, the
        // player's whatever it goes on to quote: what is left of it after
        // the echo is never thrown away.
        let denial = {
            let first = words[0].0.as_str();
            let denies = |w: &&str| first == *w || (unspaced(first) && first.starts_with(w));
            !said(first) && (DENIALS.iter().any(denies) || STOP_WORDS.iter().any(denies))
        };
        let theirs: Vec<bool> = words.iter().map(|(w, _)| !said(w)).collect();
        let mut echo = vec![false; words.len()];
        let mut start = 0;
        while start < words.len() {
            if theirs[start] {
                start += 1;
                continue;
            }
            let mut end = start + 1;
            while end < words.len() && !theirs[end] {
                end += 1;
            }
            let run = &words[start..end];
            let length = run.len();
            // The player quoting MapleSyrup's words in their own sentence
            // ("I am not in the Gate of the Future") is not its voice
            // coming back: the phone's echo never has the player's words
            // before it (the clip plays first), so a run with two words of
            // theirs before it, and none of its own voice, is theirs
            // whatever follows. (After an echo, two words are as likely its
            // own written differently.)
            let own = theirs[..start].iter().filter(|t| **t).count();
            let quoted = own >= 2 && !echo[..start].contains(&true);
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
        if left < 2 && !denial {
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
        if mostly_echo && !denial {
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
    ///
    /// Its own voice is never the player: a sentence of two words or more
    /// whose words are mostly ([`OWN_SHARE`]) its own — said in the last
    /// [`OWN_WINDOW`] s, or the phone's near-spelling of them — is dropped
    /// whole, with no "kept" remainder, unless the rest is
    /// [`OWN_REST_WORDS`] words or more (of three letters or more) that it
    /// did not say, a question about it ("where is that pillar") or a word
    /// that stops it; and a remainder never carries a name. A sentence that
    /// starts with two words of the player's, or with "no" or "stop", is
    /// theirs whatever it quotes (its echo starts with its own voice: the
    /// clip plays first). The same for the PC's voice, a clip on the phone
    /// and a call's transcript: each is a line noted as said.
    pub fn own_words(&self, now: f64, heard: &str, need: usize) -> Option<String> {
        let rest = self.strip_echo(now, heard)?;
        let rest = self.not_mostly_own(now, heard, rest)?;
        if need == 0 {
            return Some(rest);
        }
        self.players_words(now, &rest, need).map(|_| rest)
    }

    /// Its own words said lately, for [`Companion::own_words`]: those of
    /// every line still being said, or said within [`OWN_WINDOW`] s before
    /// `now` (see [`OWN_SECS_PER_WORD`]).
    fn said_lately(&self, now: f64) -> Vec<&str> {
        self.spoken
            .iter()
            .filter(|(at, said)| {
                let words = said.split(' ').filter(|w| !w.is_empty()).count();
                let end = at + OWN_LEAD_SECS + OWN_SECS_PER_WORD * words as f64;
                *at <= now + 1.0 && end >= now - OWN_WINDOW
            })
            .flat_map(|(_, said)| said.split(' ').filter(|w| !w.is_empty()))
            .collect()
    }

    /// `rest`, what `strip_echo` left of `heard`, unless `heard` is mostly
    /// its own voice with too little of the player's in it, or `rest` is a
    /// remainder with a name in it (see [`Companion::own_words`]): then
    /// `None`.
    fn not_mostly_own(&self, now: f64, heard: &str, rest: String) -> Option<String> {
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
        let said = self.said_lately(now);
        if words.len() >= 2 && !said.is_empty() {
            let own: Vec<bool> = words.iter().map(|(w, _)| said_or_near(w, &said)).collect();
            let first = words[0].0.as_str();
            let leads = !own[0] && !own[1];
            let denial =
                !said.contains(&first) && (DENIALS.contains(&first) || STOP_WORDS.contains(&first));
            let share = own.iter().filter(|o| **o).count() as f32 / words.len() as f32;
            if share >= OWN_SHARE && !leads && !denial {
                let theirs: Vec<&(String, usize)> = words
                    .iter()
                    .zip(&own)
                    .filter(|(_, own)| !**own)
                    .map(|(word, _)| word)
                    .collect();
                let strong: std::collections::HashSet<&str> = theirs
                    .iter()
                    .map(|(w, _)| w.as_str())
                    .filter(|w| w.chars().count() >= 3)
                    .collect();
                // (A question about what it said — "…to the blue pillar,
                // where is that pillar?" — is theirs: two words or more of
                // theirs, the first a question word.)
                let asks = theirs.len() >= 2 && QUESTION_WORDS.contains(&theirs[0].0.as_str());
                // (A word that stops it, said over it — "mute", "stop" — is
                // theirs: the main loop still acts on it.)
                let stops = theirs.iter().any(|(w, _)| STOP_WORDS.contains(&w.as_str()));
                let asks = asks || stops;
                let named = theirs
                    .iter()
                    .any(|(_, i)| looks_like_a_name(tokens[*i], *i == 0));
                if (strong.len() < OWN_REST_WORDS && !asks) || named {
                    return None;
                }
            }
        }
        // A remainder never carries a name ("my name is Armani my name is
        // Miha" was learned as his).
        if rest != heard.trim() {
            let named = rest.split_whitespace().enumerate().any(|(k, token)| {
                looks_like_a_name(token, k == 0 && tokens.first() == Some(&token))
            });
            if named {
                return None;
            }
        }
        Some(rest)
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
            levels_gained: self.levels_gained(),
            marks: self.marks,
        }
    }

    /// Level-ups this session: the characters' played before this one,
    /// and this one's — less, while a level reading that would be another
    /// character's waits to hold, the falls in EXP counted as level-ups
    /// that came with it ([`Companion::switch_wraps`]): another character's
    /// bar coming into view lower is no level-up, nor said as one while it
    /// may be. (Taken, they are taken back: see `watch_progress`.)
    fn levels_gained(&self) -> u32 {
        let held = self
            .switch_since
            .map_or(0, |since| self.switch_wraps(since));
        self.levels_before + self.exp.levels_gained().saturating_sub(held)
    }

    /// The falls in EXP counted as level-ups (wraps, kept [`SWITCH_SECS`])
    /// that came with a level reading first read at `since`: from
    /// [`SWITCH_WRAP_SECS`] before it on.
    fn switch_wraps(&self, since: f64) -> u32 {
        self.wraps
            .iter()
            .filter(|&&at| at >= since - SWITCH_WRAP_SECS)
            .count() as u32
    }

    /// The session so far, as a friend in the room would know it, as of
    /// the last frame: how long, when the player last spoke, deaths and
    /// level-ups and when the last of each was, the lowest HP in the last
    /// minute, how long the game has been quiet, or out of sight.
    pub fn so_far(&self) -> SoFar {
        let now = self.now;
        let seen = self.last.as_ref().is_some_and(|o| o.game.is_seen());
        let since = |at: f64| at.is_finite().then(|| (now - at).max(0.0));
        SoFar {
            seconds: now,
            since_player_spoke: since(self.spoke_at),
            quiet_before: self
                .quiet_before
                .filter(|(_, at)| now - at <= QUIET_BEFORE_KEPT_SECS)
                .map(|(quiet, _)| quiet),
            deaths: self.deaths,
            since_last_death: since(self.last_death),
            level_ups: self.levels_gained(),
            since_last_level_up: since(self.announced_level_up),
            lowest_hp_lately: self
                .hp_minute
                .iter()
                .filter(|(t, _)| now - t <= LOWEST_HP_SECS)
                .map(|(_, p)| *p)
                .reduce(f32::min),
            quiet_for: since(self.changed_at).filter(|_| seen),
            unseen_for: match (seen, self.seen_at) {
                (false, Some(at)) => Some((now - at).max(0.0)),
                _ => None,
            },
        }
    }

    /// What its own warnings came to this session, for the session's
    /// stats: the warnings said by kind, the low-bar lines and how many a
    /// potion answered, and the deaths. Counts only: no line, no reading.
    pub fn tally(&self) -> Tally {
        Tally {
            low_lines: self.low_hp.said_total + self.low_mp.said_total,
            potted: self.low_hp.potted + self.low_mp.potted,
            deaths: self.deaths,
            ..self.warned
        }
    }

    /// When HP or EXP last moved by more than the bar's flicker, or MP was
    /// potted: the game is quiet while none does. (MP by a jump — a
    /// potion's worth from one frame to the next, held — not by its slow
    /// regen; a ranged class potting MP through a boss, HP untouched and
    /// EXP flat until the kill, was asked "You still there?")
    fn track_change(&mut self, now: f64, obs: &Observation) {
        let hp = obs.hp.map(|g| g.percent);
        let exp = obs.exp;
        let hp_moved = match (self.hp_at_change, hp) {
            (Some(then), Some(v)) => (v - then).abs() >= QUIET_HP_POINTS,
            (None, Some(_)) => true,
            _ => false,
        };
        let exp_moved = match (self.exp_at_change, exp) {
            (Some(then), Some(g)) => {
                let flicker = if g.read {
                    QUIET_EXP_READ
                } else {
                    QUIET_EXP_BAR
                };
                (g.percent - then).abs() >= flicker
            }
            (None, Some(_)) => true,
            _ => false,
        };
        let mut mp_potted = false;
        if let Some(mp) = obs.mp.map(|g| g.percent) {
            mp_potted = jumped(self.mp_before, &mut self.mp_jump, mp);
            self.mp_before = Some(mp);
        }
        let mut hp_potted = false;
        if let Some(hp) = hp {
            hp_potted = jumped(self.hp_before, &mut self.hp_jump, hp);
            self.hp_before = Some(hp);
        }
        if hp_potted || mp_potted {
            self.potion_at = now;
        }
        if hp_moved || exp_moved || mp_potted {
            self.changed_at = now;
            self.hp_at_change = hp.or(self.hp_at_change);
            self.exp_at_change = exp.map(|g| g.percent).or(self.exp_at_change);
        }
    }

    /// MP as this frame reads it; `None` where it is no reading. A number
    /// read in the game's font is believed as it comes. The bar's fill at
    /// [`SLIVER`] or under is a sliver, and the everyday sliver is the
    /// cursor's tip on the bar's start: it is a reading — low MP, for the
    /// warning and its hold — only when the way down was seen:
    /// [`WAY_DOWN_READINGS`] readings over the sliver and under the mark
    /// (the default's when the warnings are set lower, or off), none of
    /// them the swing's, within [`WAY_DOWN_SECS`] before the first sliver.
    /// A drain goes so, fast ones too (80 to empty in 4 s is under the mark
    /// for less than the warning's hold before the bar reads empty).
    /// Otherwise it is no reading for as long as the bar reads a sliver: no
    /// warning ("less than 2% MP" four times in two minutes to a player at
    /// 80, then the hold), and the cursor moving off is no potion ("There
    /// you go."). (HP's sliver is never a reading: see [`SLIVER`].)
    fn mp_reading(&mut self, now: f64, mp: Option<Gauge>) -> Option<Gauge> {
        let mp = mp?;
        if mp.read || mp.percent > SLIVER {
            self.mp_empty = None;
            self.mp_lately.push_back((now, mp.percent));
            while self
                .mp_lately
                .front()
                .is_some_and(|(t, _)| now - t > WAY_DOWN_SECS)
            {
                self.mp_lately.pop_front();
            }
            return Some(mp);
        }
        // (Decided as it first reads a sliver, from the readings before.)
        let believed = match self.mp_empty {
            Some(believed) => believed,
            None => {
                let mark = self.settings.mp_low.max(MP_WAY_DOWN_MARK);
                let way_down = self
                    .mp_lately
                    .iter()
                    .filter(|&&(t, p)| {
                        p > SLIVER
                            && p < mark
                            && now - t <= WAY_DOWN_SECS
                            && !self.mp_steady.swung_at(t)
                    })
                    .count();
                way_down >= WAY_DOWN_READINGS
            }
        };
        self.mp_empty = Some(believed);
        believed.then_some(mp)
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
    pub fn observe(&mut self, now: f64, mut obs: Observation) -> Vec<Action> {
        self.now = now;
        self.started_at.get_or_insert(now);
        let mut out = Vec::new();
        let mut alerts = Vec::new();
        self.track_window(now, &obs, &mut out);
        if obs.game.is_seen() {
            // (A fill at 2% or under is no reading, to everything after —
            // and to the chat, which is given this frame: see `SLIVER`; MP's
            // is one after a way down: see `mp_reading`.)
            obs.hp = reading(obs.hp);
            obs.mp = self.mp_reading(now, obs.mp);
            self.track_change(now, &obs);
            self.watch_hp(now, &obs, &mut out, &mut alerts);
            self.watch_mp(now, &obs, &mut out, &mut alerts);
            self.watch_progress(now, &obs, &mut out, &mut alerts);
        }
        // The line is dealt only for an alert that is said: one held is no
        // line, and the next the player hears is still the next of the deck.
        // A warning (a beating, a low bar) is shouted; news (a death, a
        // level-up) is told.
        for alert in self.pace(now, &obs, alerts, &mut out) {
            self.warned.count(&alert);
            let news = alert.is_news();
            let line = self.line(alert);
            out.push(Action::Say(if news {
                Say::alert(line)
            } else {
                Say::warning(line)
            }));
        }
        if obs.game.is_seen() {
            self.still_there(now, &mut out);
        }
        self.last = Some(obs);
        out
    }

    /// The game idle for twenty minutes — nothing moved on a screen it can
    /// see, not a word from the player — "you still there?", once a
    /// session: what a friend in the room says to someone who has gone
    /// quiet, and never to someone they are watching play (it was asked
    /// of a player grinding in silence, two minutes after a beating they
    /// potted: the opposite of when a friend would ask). Not while dead,
    /// not during a hold (the warnings are held because nothing answers
    /// them: this would be one more line to the same empty room), not
    /// within minutes of a warning of its own; a word from the player, or
    /// anything moving, starts the twenty minutes over.
    fn still_there(&mut self, now: f64, out: &mut Vec<Action>) {
        if self.asked_still_there || self.dead || now < self.hold_until {
            return;
        }
        // (The first frame's readings count as a change to `track_change`:
        // a session that starts idle is idle from its first frame.)
        let idle_since = self
            .changed_at
            .max(self.spoke_at)
            .max(self.view_since)
            .max(self.started_at.unwrap_or(now));
        if now - idle_since >= STILL_THERE_SECS
            && now - self.alert_at >= STILL_THERE_AFTER_WARNING_SECS
        {
            self.asked_still_there = true;
            let line = deal(
                &mut self.decks.still_there,
                self.settings.attitude,
                self.hebrew,
                lines::STILL_THERE,
                lines::STILL_THERE_HE,
            );
            out.push(Action::Say(Say::alert(line)));
        }
    }

    /// The line for an alert, dealt from its deck, in the language it
    /// speaks.
    fn line(&mut self, alert: Alert) -> String {
        let attitude = self.settings.attitude;
        let he = self.hebrew;
        let decks = &mut self.decks;
        match alert {
            Alert::Beating(hp) => deal(
                &mut decks.beating,
                attitude,
                he,
                lines::BEATING,
                lines::BEATING_HE,
            )
            .replace("{}", &low_words(hp, he)),
            // A fight's first line stands alone; one that repeats an
            // unanswered line may say "still"; the second repeat asks.
            Alert::HpLow { hp, cadence } => {
                let (deck, en, twin) = match cadence {
                    Cadence::First => (&mut decks.hp_low, lines::HP_LOW, lines::HP_LOW_HE),
                    Cadence::Again => (
                        &mut decks.hp_low_again,
                        lines::HP_LOW_AGAIN,
                        lines::HP_LOW_AGAIN_HE,
                    ),
                    Cadence::Ask => (
                        &mut decks.hp_low_ask,
                        lines::HP_LOW_ASK,
                        lines::HP_LOW_ASK_HE,
                    ),
                };
                deal(deck, attitude, he, en, twin).replace("{}", &low_words(hp, he))
            }
            Alert::MpLow { mp, cadence } => {
                let (deck, en, twin) = match cadence {
                    Cadence::First => (&mut decks.mp_low, lines::MP_LOW, lines::MP_LOW_HE),
                    Cadence::Again => (
                        &mut decks.mp_low_again,
                        lines::MP_LOW_AGAIN,
                        lines::MP_LOW_AGAIN_HE,
                    ),
                    Cadence::Ask => (
                        &mut decks.mp_low_ask,
                        lines::MP_LOW_ASK,
                        lines::MP_LOW_ASK_HE,
                    ),
                };
                deal(deck, attitude, he, en, twin).replace("{}", &low_words(mp, he))
            }
            Alert::Death { warned, sooner } => {
                // "Told you to pot" only when it did: a death with no
                // warning before it is dealt from the deck that makes no
                // such claim (one night a one-shot from 80% got "Dead. Told
                // you to pot. I'll warn you sooner from now on").
                let mut line = if warned {
                    deal(
                        &mut decks.death_warned,
                        attitude,
                        he,
                        lines::DEATH_WARNED,
                        lines::DEATH_WARNED_HE,
                    )
                } else {
                    deal(
                        &mut decks.death,
                        attitude,
                        he,
                        lines::DEATH,
                        lines::DEATH_HE,
                    )
                }
                .to_string();
                // The mark moving, in the same voice.
                if let Some(sooner) = sooner {
                    let card = deal(
                        &mut decks.sooner,
                        attitude,
                        he,
                        lines::SOONER,
                        lines::SOONER_HE,
                    )
                    .replace("{}", &format!("{sooner:.0}"));
                    line.push(' ');
                    line.push_str(&card);
                }
                line
            }
            Alert::LevelUp(level) => deal(
                &mut decks.level_up,
                attitude,
                he,
                lines::LEVEL_UP,
                lines::LEVEL_UP_HE,
            )
            .replace("{}", &level.to_string()),
        }
    }

    /// Warnings that nothing answers are held: after [`UNANSWERED_MAX`] of
    /// them with no sign of life from the player — not a word, no potion
    /// (HP or MP, a reading that held, not a frame), no EXP gained, no
    /// level — the rest wait [`HOLD_SECS`] (and the player is told once
    /// why, in words for whoever is there), unless the player turns up
    /// sooner: a word, or a potion (which gets a word back). One night of a
    /// misread bar ran to 3,400 warnings said to an empty room.
    /// A death and a level-up are news, not a nag: they pass through a
    /// hold (the coach counts the death either way, and would speak of a
    /// third death nobody heard of). Returns the alerts of this frame that
    /// are to be said.
    fn pace(
        &mut self,
        now: f64,
        obs: &Observation,
        alerts: Vec<Alert>,
        out: &mut Vec<Action>,
    ) -> Vec<Alert> {
        let hp = obs.hp.map(|g| g.percent);
        let mp = obs.mp.map(|g| g.percent);
        let exp = obs.exp.map(|g| g.percent);
        let above = |value: Option<f32>, then: Option<f32>, by: f32| matches!((value, then), (Some(value), Some(then)) if value >= then + by);
        if obs.game.is_seen() {
            // (A potion's worth back is a sign of life once it has held,
            // as it answers the warning itself: one frame read high is a
            // misread, and it lifted the hold. HP's or MP's: a player
            // potting every MP warning through a boss — EXP flat until the
            // kill, HP untouched — was told "No pot, no word, no point".)
            for (value, then, frames) in [
                (hp, self.hp_at_alert, &mut self.hp_up_frames),
                (mp, self.mp_at_alert, &mut self.mp_up_frames),
            ] {
                if above(value, then, POTTED) {
                    *frames += 1;
                    if *frames >= HELD_FRAMES {
                        self.potted = true;
                    }
                } else {
                    *frames = 0;
                }
            }
            // A potion while the warnings are held: someone is there after
            // all. The hold is over — and when they were told of it lately,
            // one word that it saw ("There you go."), once: for a potion it
            // saw as one (a jump, `potion_at`), never for HP coming back on
            // its own over the minutes of a hold.
            if self.potted && now < self.hold_until {
                self.hold_until = f64::NEG_INFINITY;
                self.unanswered = 0;
                if now - self.hold_told <= POTTED_AT_LAST_SECS
                    && now - self.potion_at <= SEEN_POTION_SECS
                {
                    let line = deal(
                        &mut self.decks.potted_at_last,
                        self.settings.attitude,
                        self.hebrew,
                        lines::POTTED_AT_LAST,
                        lines::POTTED_AT_LAST_HE,
                    );
                    out.push(Action::Say(Say::info(line, true)));
                }
            }
            self.exp_peak = match (self.exp_peak, exp) {
                (Some(p), Some(v)) => Some(p.max(v)),
                (p, v) => p.or(v),
            };
        }
        let warnings = alerts.iter().filter(|a| !a.is_news()).count() as u32;
        if warnings == 0 {
            return alerts;
        }
        let news = |alerts: Vec<Alert>| -> Vec<Alert> {
            alerts.into_iter().filter(Alert::is_news).collect()
        };
        let gained = above(self.exp_peak, self.exp_at_alert, EXP_GAINED);
        // (The level as believed, not this frame's: a level read, lost and
        // read again — the HUD dropping out — is no level gained.)
        let leveled = matches!(
            (self.last_level, self.level_at_alert),
            (Some(level), Some(then)) if level != then
        );
        if self.spoke_at > self.alert_at || self.potted || gained || leveled {
            self.unanswered = 0;
            self.hold_until = f64::NEG_INFINITY;
        }
        if now < self.hold_until {
            return news(alerts);
        }
        if self.hold_until.is_finite() {
            // The hold ended with nothing answering: a couple come through.
            self.hold_until = f64::NEG_INFINITY;
            self.unanswered = UNANSWERED_MAX - AFTER_HOLD;
        }
        self.unanswered += warnings;
        self.alert_at = now;
        self.hp_at_alert = hp;
        self.hp_up_frames = 0;
        self.mp_at_alert = mp;
        self.mp_up_frames = 0;
        self.potted = false;
        self.exp_at_alert = exp;
        self.exp_peak = exp;
        self.level_at_alert = self.last_level;
        if self.unanswered > UNANSWERED_MAX {
            self.hold_until = now + HOLD_SECS;
            if now - self.hold_told >= HOLD_TOLD_EVERY {
                self.hold_told = now;
                // To someone who spoke lately, not to an empty room ("Just
                // talk to me when you're back" came three minutes after
                // "ok, I'm back", to a player being hit).
                let (attitude, he) = (self.settings.attitude, self.hebrew);
                let line = if now - self.spoke_at <= HOLD_AWAY_SECS {
                    deal(
                        &mut self.decks.hold_here,
                        attitude,
                        he,
                        lines::HOLD_HERE,
                        lines::HOLD_HERE_HE,
                    )
                } else {
                    deal(
                        &mut self.decks.hold_away,
                        attitude,
                        he,
                        lines::HOLD_AWAY,
                        lines::HOLD_AWAY_HE,
                    )
                };
                out.push(Action::Say(Say::info(line, true)));
            }
            return news(alerts);
        }
        alerts
    }

    fn track_window(&mut self, now: f64, obs: &Observation, out: &mut Vec<Action>) {
        let (attitude, he) = (self.settings.attitude, self.hebrew);
        if let GameView::Seen(title) = &obs.game {
            if !self.ever_seen || self.announced_lost {
                self.view_since = now;
            }
            if !self.ever_seen {
                self.ever_seen = true;
                // The window's title, when it is not plainly the game's: a
                // player once heard "I can see MapleStory" with the game
                // closed, and the log did not say what had been taken for it.
                let line = if title.trim().eq_ignore_ascii_case("maplestory") {
                    deal(
                        &mut self.decks.seen,
                        attitude,
                        he,
                        lines::SEEN,
                        lines::SEEN_HE,
                    )
                    .to_string()
                } else {
                    format!("I can see MapleStory (the window \"{}\").", title.trim())
                };
                out.push(Action::Say(Say::info(line, true)));
            } else if self.announced_lost {
                let line = deal(
                    &mut self.decks.again,
                    attitude,
                    he,
                    lines::AGAIN,
                    lines::AGAIN_HE,
                );
                out.push(Action::Say(Say::info(line, true)));
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
                _ => deal(
                    &mut self.decks.lost,
                    attitude,
                    he,
                    lines::LOST,
                    lines::LOST_HE,
                )
                .to_string(),
            };
            out.push(Action::Say(Say::info(why, true)));
        }
    }

    fn watch_hp(
        &mut self,
        now: f64,
        obs: &Observation,
        out: &mut Vec<Action>,
        alerts: &mut Vec<Alert>,
    ) {
        let Some(hp) = obs.hp else {
            return;
        };
        // Readings swinging back and forth are a bar being guessed at, not
        // read: nothing is said from them. A number read in the game's own
        // font is no guess, and in a fight it does swing — a hit, a potion,
        // a hit — so it is trusted as it comes, deaths and all.
        let unsteady = !hp.read && self.hp_steady.unsteady(now, hp.percent);
        if unsteady {
            self.falling_frames = 0;
            self.low_hp.frames = 0;
            if now - self.hp_steady.noted >= UNSTEADY_NOTE_EVERY {
                self.hp_steady.noted = now;
                out.push(Action::Say(Say::info(self.unsteady_note(Bar::Hp), false)));
            }
            // (A swing is the fill's, and a fill is never at zero: see
            // `SLIVER`.)
            self.zero_hp_frames = 0;
            return;
        }
        // A death: HP at zero for a moment — the number read 0 in the
        // game's font (a fill at zero is no reading: see `SLIVER`).
        if hp.percent <= 0.5 {
            if self.zero_hp_frames == 0 {
                self.zero_hp_since = now;
            }
            self.zero_hp_frames += 1;
            if self.zero_hp_frames >= 3 && !self.dead {
                self.dead = true;
                self.deaths += 1;
                self.last_death = now;
                // (A death is the end of trusting them with a beating.)
                self.trust = 0;
                self.trust_floor = f32::INFINITY;
                // A fall it chose to watch rather than shout: the silence
                // was the trust's, not the mark's — no sooner card.
                let watched = self.watch.take().is_some();
                // Whether it told them to pot (or to back off) in this
                // fight: a low line, or the beating — above the mark too.
                // ("You're eating every hit. Move." and a second later
                // "New rule: I yell at 35." — "I yell" one second after it
                // yelled.)
                // (During a hold, not the lines said from the swing — its
                // first frames, before it was found to swing, can pass for
                // a beating: those warned of nothing.)
                let holding = self.hp_steady.holding(now);
                let before_swing = |at: f64| !holding || at < self.hp_steady.swing_from;
                let beaten = self.fight_lines > 0
                    && now - self.fall_at <= FIGHT_OVER_SECS
                    && before_swing(self.fall_told);
                let warned = beaten
                    || (self.low_hp.warned_this_fight(now, self.fall_at)
                        && before_swing(self.low_hp.told));
                // (Nor for a death after a swinging bar: the way down was a
                // guess, not seen.)
                let sooner = if watched || holding {
                    None
                } else {
                    self.sooner_warning(warned)
                };
                if let Some(sooner) = sooner {
                    self.settings.hp_low = sooner;
                }
                alerts.push(Alert::Death { warned, sooner });
            }
            return;
        }
        self.zero_hp_frames = 0;
        self.hp_lately.push_back((now, hp.percent));
        while self.hp_lately.front().is_some_and(|(t, _)| now - t > 10.0) {
            self.hp_lately.pop_front();
        }
        // (The same readings, kept a minute, for `so_far`.)
        self.hp_minute.push_back((now, hp.percent));
        while self
            .hp_minute
            .front()
            .is_some_and(|(t, _)| now - t > LOWEST_HP_SECS)
        {
            self.hp_minute.pop_front();
        }
        if self.dead && hp.percent > 10.0 {
            self.dead = false;
            // A new life is a new fight: the last life's beating warned of
            // nothing in this one.
            self.low_hp = Low::new();
            self.fight_lines = 0;
        }
        // A beating: HP falling fast, said as it happens — the one thing
        // worth interrupting for, and never worth a model's wait. Once per
        // fight, though: a player who pots after it has handled it, and
        // the next hit is their business (the low warning still comes);
        // one who does not hears it again, after longer and longer waits.
        // Not at all when they have asked for no HP warnings. The fall is
        // measured from readings that held (`HELD_FRAMES`): one frame of
        // 60 in a run of 20s is a misread, not a potion to fall from.
        let highest_lately = held_readings(&self.hp_lately)
            .filter(|(t, _)| now - t <= FALL_SECS)
            .map(|(_, p)| p)
            .fold(0.0, f32::max);
        if highest_lately - hp.percent >= FALL_POINTS && hp.percent < 70.0 {
            self.falling_frames += 1;
        } else {
            self.falling_frames = 0;
        }
        if now - self.fall_at > FIGHT_OVER_SECS {
            // (A fight over with its beating unanswered: no trust.)
            if self.fight_lines > 0 && !self.fall_answered {
                self.trust = 0;
                self.trust_floor = f32::INFINITY;
            }
            self.fight_lines = 0;
        }
        // (A potion's worth back, once it holds, answers the beating — or
        // the fall being watched in its place.)
        let was_answered = self.fall_answered;
        if hp.percent >= self.fall_hp + POTTED {
            self.fall_up_frames += 1;
            if self.fall_up_frames >= HELD_FRAMES {
                self.fall_answered = true;
            }
        } else {
            self.fall_up_frames = 0;
        }
        if !self.fall_answered && (self.fight_lines > 0 || self.watch.is_some()) {
            self.fight_low = self.fight_low.min(hp.percent);
        }
        if self.fall_answered && !was_answered {
            // Handled: a potion within seconds of the beating (or of the
            // fall watched in its place). Every handled fight counts, a
            // deeper one too — depth is shouted, not distrusted — and the
            // floor follows the lowest of them; a late potion ends the
            // trust (so does none, when the fight is over, and a death).
            // Three in a row and the player is trusted with it. (A deeper
            // fight started the streak over: two dips under the mark,
            // both potted in two seconds, had the two fights after them
            // shouted again — nine shouts in ten handled fights.)
            let since = self.watch.take().unwrap_or(self.fall_told);
            if now - since <= TRUST_POT_SECS {
                self.trust += 1;
                self.trust_floor = self.trust_floor.min(self.fight_low);
            } else {
                self.trust = 0;
                self.trust_floor = f32::INFINITY;
            }
        }
        let falling = self.falling_frames >= 2;
        if falling {
            self.fall_at = now;
        }
        let due = match self.fight_lines {
            0 => true,
            said => {
                let wait = FALL_COOLDOWNS[(said as usize - 1).min(FALL_COOLDOWNS.len() - 1)];
                !self.fall_answered && now - self.fall_told >= wait
            }
        };
        let exp = obs.exp.map(|g| g.percent);
        // (Nor while the bar is not believed: see `Low`.)
        let wanted = !self.dead && self.settings.hp_low > 0.0 && !self.low_hp.doubted;
        if falling && due && wanted && self.watch.is_none() {
            if self.trust >= TRUST_FIGHTS {
                // A player who handles it: watched, not shouted.
                self.watch = Some(now);
                self.fall_hp = hp.percent;
                self.fall_answered = false;
                self.fall_up_frames = 0;
                self.fight_low = hp.percent;
            } else {
                self.shout_beating(now, hp, exp, alerts);
            }
        }
        // A fall watched: shouted after all when HP goes under their usual
        // floor (the depth deserves it; potted in time, it counts like the
        // rest, and the floor follows it), or when no potion has come in
        // the time they usually take — and then the trust is gone.
        if let Some(since) = self.watch {
            // (Never trusted under the bottom: a fall into low HP is said.)
            let floor = (self.trust_floor - TRUST_MARGIN).max(self.settings.hp_low * TRUST_BOTTOM);
            let under_floor = hp.percent < floor;
            let late = now - since > TRUST_POT_SECS;
            if wanted && (under_floor || late) {
                self.watch = None;
                if late {
                    self.trust = 0;
                    self.trust_floor = f32::INFINITY;
                }
                self.shout_beating(now, hp, exp, alerts);
            }
        }
        // The low warning: once per fight (see `Low`), and not right after
        // the beating was called (that said to pot), nor while a fall is
        // being watched in its place.
        match self
            .low_hp
            .due(now, hp.percent, self.settings.hp_low, self.fall_at, exp)
        {
            Due::Line if now - self.fall_told >= 5.0 && self.watch.is_none() => {
                let cadence = self.low_hp.cadence();
                self.low_hp.said(now, hp.percent, exp);
                alerts.push(Alert::HpLow { hp, cadence });
            }
            Due::Misread => out.push(Action::Say(self.misread(Bar::Hp))),
            _ => {}
        }
    }

    /// The beating, said now at `hp`: the fight's bookkeeping, and the
    /// low warning counted as said when HP is already under the mark (it
    /// would only say to pot again).
    fn shout_beating(&mut self, now: f64, hp: Gauge, exp: Option<f32>, alerts: &mut Vec<Alert>) {
        self.fall_told = now;
        self.fight_lines += 1;
        self.fall_hp = hp.percent;
        self.fall_answered = false;
        self.fall_up_frames = 0;
        self.fight_low = hp.percent;
        if hp.percent < self.settings.hp_low {
            self.low_hp.said(now, hp.percent, exp);
        }
        alerts.push(Alert::Beating(hp));
    }

    /// The bar is not believed from now on (see [`Low`]): said once, in
    /// the attitude's voice, as a note about itself.
    fn misread(&mut self, bar: Bar) -> Say {
        let line = deal(
            &mut self.decks.misread,
            self.settings.attitude,
            self.hebrew,
            lines::MISREAD,
            lines::MISREAD_HE,
        )
        .replace("{}", bar.name(self.hebrew));
        Say::info(line, true)
    }

    /// The bar's readings are swinging, so its warnings are held: the note
    /// for the phone.
    fn unsteady_note(&self, bar: Bar) -> String {
        let note = if self.hebrew {
            lines::UNSTEADY_HE
        } else {
            lines::UNSTEADY
        };
        note.replace("{}", bar.name(self.hebrew))
    }

    /// Died without a warning, though HP went down through where a sooner
    /// one would have come, slowly enough for it to help: warn sooner from
    /// now on (five points, up to half the bar). Not when warnings are
    /// off, nor after a sudden death — HP through the band over the mark
    /// ([`SOONER_BAND`]) in less than [`SOONER_BAND_SECS`] on the way down
    /// (no warning would have helped).
    fn sooner_warning(&self, warned: bool) -> Option<f32> {
        let low = self.settings.hp_low;
        if warned || low <= 0.0 || low >= SOONEST_WARNING {
            return None;
        }
        // The way down: the readings (that held) since HP was last over
        // the band, each lasting until the next, the last until the death.
        let readings: Vec<(f64, f32)> = held_readings(&self.hp_lately).collect();
        let from = readings
            .iter()
            .rposition(|&(_, p)| p >= low + SOONER_BAND)
            .map_or(0, |i| i + 1);
        let way_down = &readings[from..];
        let in_band: f64 = way_down
            .iter()
            .enumerate()
            .filter(|(_, (_, p))| *p >= low)
            .map(|(i, &(t, _))| {
                let until = way_down
                    .get(i + 1)
                    .map_or(self.zero_hp_since, |&(next, _)| next);
                until - t
            })
            .sum();
        (in_band >= SOONER_BAND_SECS).then(|| (low + 5.0).min(SOONEST_WARNING))
    }

    fn watch_mp(
        &mut self,
        now: f64,
        obs: &Observation,
        out: &mut Vec<Action>,
        alerts: &mut Vec<Alert>,
    ) {
        let Some(mp) = obs.mp else {
            return;
        };
        // (A read number is trusted; see the HP bar.)
        if !mp.read && self.mp_steady.unsteady(now, mp.percent) {
            self.low_mp.frames = 0;
            if now - self.mp_steady.noted >= UNSTEADY_NOTE_EVERY {
                self.mp_steady.noted = now;
                out.push(Action::Say(Say::info(self.unsteady_note(Bar::Mp), false)));
            }
            return;
        }
        // Once per fight, as the HP one (a pet pots MP too), and not
        // believed past four unanswered lines with EXP coming in, as it.
        let exp = obs.exp.map(|g| g.percent);
        match self.low_mp.due(
            now,
            mp.percent,
            self.settings.mp_low,
            f64::NEG_INFINITY,
            exp,
        ) {
            Due::Line if !self.dead => {
                let cadence = self.low_mp.cadence();
                self.low_mp.said(now, mp.percent, exp);
                alerts.push(Alert::MpLow { mp, cadence });
            }
            Due::Misread => out.push(Action::Say(self.misread(Bar::Mp))),
            _ => {}
        }
    }

    /// The level, from the number read at the bottom left of the screen
    /// ("Lv. 165"), and the EXP bar for the pace. A level-up is that number
    /// going one above the highest seen for the same character, once the
    /// new reading has held for a moment — then, and only then, is it
    /// said. The EXP bar wrapping says nothing on its own (it has the sight
    /// read the number again, and the number says); a number that jumps,
    /// or comes with another name, is another character or a misread:
    /// taken, not celebrated. One below the highest seen is a misread
    /// until it has held much longer, and taken quietly then — else a
    /// reading that flips 165, 166, 165, 166 celebrates 166 every time.
    fn watch_progress(
        &mut self,
        now: f64,
        obs: &Observation,
        out: &mut Vec<Action>,
        alerts: &mut Vec<Alert>,
    ) {
        if let Some(level) = obs.level {
            match self.level_candidate {
                Some((candidate, since)) if candidate == level => {
                    let same_character = match (&self.last_name, &obs.name) {
                        (Some(then), Some(now)) => then == now,
                        _ => true,
                    };
                    let top = self.top_level.filter(|_| same_character);
                    let lower = top.is_some_and(|top| level < top);
                    // Another character's, it would be (another name, or
                    // lower: the note's two cases below), waiting to hold:
                    // its EXP is not this one's (see `levels_gained`).
                    let another = self.last_level.is_some_and(|taken| taken != level)
                        && (!same_character || lower);
                    self.switch_since = another.then_some(since);
                    let hold = if lower {
                        LEVEL_HOLD_SECS * LOWER_LEVEL_HOLDS
                    } else {
                        LEVEL_HOLD_SECS
                    };
                    if now - since >= hold && self.last_level != Some(level) {
                        let before = self.last_level;
                        self.last_level = Some(level);
                        if obs.name.is_some() {
                            self.last_name = obs.name.clone();
                        }
                        if !lower {
                            self.top_level = Some(level);
                        }
                        if another {
                            // Taken: the falls in EXP counted as level-ups
                            // as its bar came into view are taken back (a
                            // switch from 85% to a bar at 20% is the EXP
                            // tracker's wrap), and its pace starts over.
                            self.levels_before += self
                                .exp
                                .levels_gained()
                                .saturating_sub(self.switch_wraps(since));
                            self.exp = ExpTracker::new();
                            self.wraps.clear();
                            self.switch_since = None;
                        }
                        let rose_by_one = top.is_some_and(|top| level == top + 1);
                        if rose_by_one {
                            // (The EXP bar may have counted this one already.)
                            self.exp.level_rose(now);
                            if now - self.announced_level_up >= 30.0 {
                                self.announced_level_up = now;
                                alerts.push(Alert::LevelUp(level));
                            }
                        } else if let Some(before) = before {
                            let why = if !same_character {
                                ["another character", "דמות אחרת"]
                            } else if lower {
                                [
                                    "a lower level: another character, or misread",
                                    "רמה נמוכה יותר: דמות אחרת, או קריאה שגויה",
                                ]
                            } else if top == Some(level) {
                                [
                                    "back to a level seen before: not celebrated again",
                                    "חזרה לרמה שכבר הייתה: לא חוגגים אותה שוב",
                                ]
                            } else {
                                [
                                    "not one level up: not celebrated",
                                    "לא עלייה של רמה אחת: לא חוגגים",
                                ]
                            };
                            let note = if self.hebrew {
                                format!("רמה {level} עכשיו, אחרי {before} ({}).", why[1])
                            } else {
                                format!("Level {level} now, from {before} ({}).", why[0])
                            };
                            out.push(Action::Say(Say::info(note, false)));
                        }
                    }
                }
                _ => {
                    self.level_candidate = Some((level, now));
                    self.switch_since = None;
                }
            }
        } else if self.last_name.is_none() && obs.name.is_some() {
            self.last_name = obs.name.clone();
        }
        // The EXP bar: for the pace and the time to the next level. Its
        // wrapping counts a level for the total, and says nothing.
        if let Some(exp) = obs.exp
            && self.exp.add(now, exp.percent as f64)
        {
            self.wraps.push_back(now);
        }
        while self.wraps.front().is_some_and(|at| now - at > SWITCH_SECS) {
            self.wraps.pop_front();
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

    /// An answer it knows without a model, when `sentence` asks for one of
    /// the player's own numbers ([`instant::asks`]): dealt from the
    /// session's decks (one per kind of answer), like its other lines.
    /// `None` when it does not, or the number isn't known right now.
    pub fn instant(&mut self, sentence: &str) -> Option<String> {
        let ask = instant::asks(sentence)?;
        instant::answer_in(
            ask,
            sentence,
            self.hebrew,
            self.last.as_ref(),
            &self.progress(),
            self.settings.attitude,
            &mut self.decks.instant,
        )
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
                    Action::Say(Say::reply(
                        if self.hebrew {
                            lines::MARKED_HE
                        } else {
                            lines::MARKED
                        }
                        .replace("{}", &self.marks.to_string()),
                    )),
                ]
            }
            Command::Mute => {
                self.muted = true;
                let line = deal(
                    &mut self.decks.muted,
                    self.settings.attitude,
                    self.hebrew,
                    lines::MUTED,
                    lines::MUTED_HE,
                );
                vec![Action::Say(Say::info(line, false)), Action::SetMuted(true)]
            }
            Command::Unmute => {
                self.muted = false;
                let line = deal(
                    &mut self.decks.unmuted,
                    self.settings.attitude,
                    self.hebrew,
                    lines::UNMUTED,
                    lines::UNMUTED_HE,
                );
                vec![Action::SetMuted(false), Action::Say(Say::info(line, true))]
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
/// "about" when it was measured from the bar rather than read) — in
/// Hebrew "12 אחוז", "בערך 12 אחוז", and one [`LESS_THAN_TWO`].
fn low_words(gauge: Gauge, hebrew: bool) -> String {
    let number = (gauge.percent.round() as i64).max(1);
    match (gauge.read, hebrew) {
        (true, false) => format!("{number} percent"),
        (false, false) => format!("about {number} percent"),
        (_, true) if number == 1 => LESS_THAN_TWO.to_string(),
        (true, true) => format!("{number} אחוז"),
        (false, true) => format!("בערך {number} אחוז"),
    }
}

/// One percent in Hebrew, read or measured: "less than two percent". It
/// is plural, as every amount the cards take is ("נשארו לך {} חיים",
/// "{} ויורד"), where "אחוז אחד" is singular ("נשארו לך אחוז אחד חיים"
/// is not said) and "1 אחוז" is read "אחד אחוז"; and it says "about"
/// already ("בערך פחות משני אחוז" is not said either).
pub(crate) const LESS_THAN_TWO: &str = "פחות משני אחוז";

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

    /// The tests' session seed: a session's decks are shuffled by it, and
    /// a test that pins a line replays exactly with it. (The rules the
    /// tests assert — the lead first, every card once a round, none twice
    /// running — hold for every seed.)
    const SEED: u64 = 7;

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

    /// A frame whose HP was read from the printed number (of 10,000), not
    /// measured from the bar.
    fn read(hp: f32) -> Observation {
        let mut obs = frame(hp, 80.0, 10.0);
        obs.hp = Some(Gauge {
            percent: hp,
            current: Some((hp * 100.0) as u64),
            max: Some(10_000),
            read: true,
        });
        obs.level = Some(165);
        obs.name = Some("WanWan".into());
        obs
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

    /// The lines it spoke up with on its own: warnings and news both.
    fn alerts(actions: &[Action]) -> Vec<String> {
        actions
            .iter()
            .filter_map(|a| match a {
                Action::Say(s) if matches!(s.kind, Kind::Warning | Kind::Alert) => {
                    Some(s.text.clone())
                }
                _ => None,
            })
            .collect()
    }

    /// Which line of `deck` `line` is, in the friendly voice (the tests'
    /// default), `{}` standing for the amount or the level; `None` for a
    /// line from elsewhere.
    fn variant(deck: [&[&str]; 3], line: &str) -> Option<usize> {
        place(Attitude::Friendly, deck, line)
    }

    /// Which card of `attitude`'s list of `deck` `line` is (`{}` standing
    /// for the amount or the level); `None` for a line from elsewhere.
    fn place(attitude: Attitude, deck: [&[&str]; 3], line: &str) -> Option<usize> {
        attitude
            .lines(deck)
            .iter()
            .position(|pattern| match pattern.split_once("{}") {
                Some((head, tail)) => {
                    line.len() >= head.len() + tail.len()
                        && line.starts_with(head)
                        && line.ends_with(tail)
                }
                None => line == *pattern,
            })
    }

    /// Whether `phrase` (Hebrew, a word or more) is in `card` as whole
    /// words, the first also with a prefix of a letter or two (ו, ה, ב, ל,
    /// מ, ש, כ): "ושוב" says "שוב", "חשוב" does not.
    fn says_he(card: &str, phrase: &str) -> bool {
        let words: Vec<&str> = card
            .split(|c: char| !c.is_alphabetic() && c != '\'')
            .filter(|w| !w.is_empty())
            .collect();
        let want: Vec<&str> = phrase.split(' ').collect();
        words.windows(want.len()).any(|run| {
            run.iter().zip(&want).enumerate().all(|(i, (have, want))| {
                have == want
                    || (i == 0
                        && have.strip_suffix(want).is_some_and(|prefix| {
                            (1..=2).contains(&prefix.chars().count())
                                && prefix.chars().all(|c| "והבלמשכ".contains(c))
                        }))
            })
        })
    }

    /// Whether `line` is one of `deck`'s (the situation, not the words).
    fn from(deck: [&[&str]; 3], line: &str) -> bool {
        variant(deck, line).is_some()
    }

    /// Whether `line` says the warnings are held, in any voice, to a room
    /// it takes for empty or to someone there.
    fn hold_note(line: &str) -> bool {
        Attitude::ALL.iter().any(|a| {
            a.lines(lines::HOLD_AWAY).contains(&line) || a.lines(lines::HOLD_HERE).contains(&line)
        })
    }

    /// The alerts among `actions`, less the half hour's "still there?" (a
    /// long test session with no word from the player gets one; it is not
    /// the deck under test).
    fn dealt(actions: &[Action]) -> Vec<String> {
        let still_there = |line: &str| {
            Attitude::ALL
                .iter()
                .any(|a| a.lines(lines::STILL_THERE).contains(&line))
        };
        alerts(actions)
            .into_iter()
            .filter(|l| !still_there(l))
            .collect()
    }

    /// `line` with the "I'll warn you sooner" card for `mark` taken off
    /// its end (in any voice), or `None` when it ends with no such card.
    fn without_sooner(line: &str, mark: u32) -> Option<&str> {
        Attitude::ALL.iter().find_map(|a| {
            a.lines(lines::SOONER).iter().find_map(|card| {
                let tail = format!(" {}", card.replace("{}", &mark.to_string()));
                line.strip_suffix(tail.as_str())
            })
        })
    }

    /// Whether `line` is one of `deck`'s and, when it is one with the
    /// amount in it, says `amount`.
    fn says(deck: [&[&str]; 3], line: &str, amount: &str) -> bool {
        from(deck, line) && (!line.contains("percent") || line.contains(amount))
    }

    /// `lines` were dealt from `deck` in order: the lead (the one with the
    /// number) first, every line once per round of the deck's size before
    /// any comes again, and never the same line twice in a row.
    fn dealt_like_a_deck(deck: [&[&str]; 3], lines: &[String]) {
        let size = Attitude::Friendly.lines(deck).len();
        let which: Vec<usize> = lines
            .iter()
            .map(|l| variant(deck, l).unwrap_or_else(|| panic!("not from the deck: {l:?}")))
            .collect();
        assert_eq!(which[0], 0, "the lead first: {lines:?}");
        for (r, round) in which.chunks(size).enumerate() {
            let mut seen = round.to_vec();
            seen.sort_unstable();
            seen.dedup();
            assert_eq!(seen.len(), round.len(), "round {r} repeats: {round:?}");
        }
        for (i, pair) in which.windows(2).enumerate() {
            assert_ne!(pair[0], pair[1], "twice in a row at {i}: {lines:?}");
        }
    }

    #[test]
    fn announces_the_game_once_and_its_loss_after_a_while() {
        let mut c = Companion::seeded(Settings::default(), SEED);
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
    fn low_hp_is_said_once_per_fight_and_again_only_unanswered_or_lower() {
        let mut c = Companion::seeded(Settings::default(), SEED);
        // Worn down slowly (a beating is called sooner, and otherwise).
        c.observe(0.0, frame(90.0, 90.0, 10.0));
        c.observe(5.0, frame(70.0, 90.0, 10.0));
        c.observe(10.0, frame(50.0, 90.0, 10.0));
        c.observe(15.0, frame(35.0, 90.0, 10.0));
        // A moment low is not enough (a dialog over the bar, a misread).
        assert!(said(&c.observe(21.0, frame(20.0, 90.0, 10.0))).is_empty());
        assert!(said(&c.observe(21.1, frame(20.0, 90.0, 10.0))).is_empty());
        assert!(said(&c.observe(21.3, frame(20.0, 90.0, 10.0))).is_empty());
        // The first warning of a session is the one with the number in full.
        let first = said(&c.observe(21.7, frame(20.0, 90.0, 10.0)));
        assert_eq!(first.len(), 1, "{first:?}");
        assert_eq!(variant(lines::HP_LOW, &first[0]), Some(0), "{first:?}");
        assert!(first[0].contains("about 20 percent"), "{first:?}");
        // Still low, nothing done about it, and no new fall (18 is no
        // fall from 20): nothing, the wait over or not…
        for i in 0..13 {
            assert!(said(&c.observe(21.8 + i as f64, frame(18.0, 90.0, 10.0))).is_empty());
        }
        // …then it falls again (damage taken, no potion): said again, put
        // another way — from the deck that may say "still" (and again
        // after longer waits: see the beating's).
        let nagged = said(&c.observe(34.8, frame(12.0, 90.0, 10.0)));
        assert_eq!(nagged.len(), 1, "{nagged:?}");
        assert!(from(lines::HP_LOW_AGAIN, &nagged[0]), "{nagged:?}");
        assert_ne!(nagged, first);
        // A potion answers it. Worn down again in the same fight (a minute
        // since HP was last low has not passed): the player handled it, and
        // the next hit is their business.
        c.observe(40.0, frame(80.0, 90.0, 10.0));
        c.observe(44.0, frame(60.0, 90.0, 10.0));
        c.observe(48.0, frame(40.0, 90.0, 10.0));
        c.observe(52.0, frame(20.0, 90.0, 10.0));
        c.observe(52.3, frame(20.0, 90.0, 10.0));
        assert!(said(&c.observe(52.7, frame(20.0, 90.0, 10.0))).is_empty());
        assert!(said(&c.observe(55.0, frame(20.0, 90.0, 10.0))).is_empty());
        // Lower than it was at the last line, though: once more, at once
        // (the bar has been low a while: no wait for a misread to pass) —
        // a line that stands on its own: the last one was answered.
        let lower = said(&c.observe(56.0, frame(9.0, 90.0, 10.0)));
        assert_eq!(lower.len(), 1, "{lower:?}");
        assert!(
            says(lines::HP_LOW, &lower[0], "about 9 percent"),
            "{lower:?}"
        );
        assert!(said(&c.observe(56.3, frame(9.0, 90.0, 10.0))).is_empty());
        assert!(said(&c.observe(58.0, frame(9.0, 90.0, 10.0))).is_empty());
        // Potted, and a minute without HP low: the fight is over, and the
        // next time it is low is a new one.
        c.observe(60.0, frame(90.0, 90.0, 10.0));
        c.observe(120.0, frame(80.0, 90.0, 10.0));
        c.observe(124.0, frame(60.0, 90.0, 10.0));
        c.observe(128.0, frame(40.0, 90.0, 10.0));
        c.observe(132.0, frame(20.0, 90.0, 10.0));
        c.observe(132.3, frame(20.0, 90.0, 10.0));
        let again = said(&c.observe(132.7, frame(20.0, 90.0, 10.0)));
        assert_eq!(again.len(), 1, "{again:?}");
        assert!(from(lines::HP_LOW, &again[0]), "{again:?}");
    }

    #[test]
    fn a_beating_is_called_as_it_happens_and_the_low_warning_waits_its_turn() {
        let mut c = Companion::seeded(Settings::default(), SEED);
        c.observe(0.0, frame(95.0, 90.0, 10.0));
        for i in 1..10 {
            assert!(said(&c.observe(i as f64 * 0.1, frame(95.0, 90.0, 10.0))).is_empty());
        }
        // Down 30 points in a second: called on the second frame that says
        // so, well before HP is low — the first time with the reading.
        assert!(said(&c.observe(1.5, frame(65.0, 90.0, 10.0))).is_empty());
        let beating = said(&c.observe(1.6, frame(64.0, 90.0, 10.0)));
        assert_eq!(beating.len(), 1, "{beating:?}");
        assert_eq!(variant(lines::BEATING, &beating[0]), Some(0), "{beating:?}");
        assert!(beating[0].contains("about 64 percent"), "{beating:?}");
        // Still falling: not said again for a while…
        assert!(said(&c.observe(2.0, frame(40.0, 90.0, 10.0))).is_empty());
        assert!(said(&c.observe(2.2, frame(35.0, 90.0, 10.0))).is_empty());
        // …and the low warning, which would only say to pot again, waits a
        // few seconds, then comes.
        assert!(said(&c.observe(3.0, frame(25.0, 90.0, 10.0))).is_empty());
        assert!(said(&c.observe(3.3, frame(25.0, 90.0, 10.0))).is_empty());
        assert!(said(&c.observe(3.7, frame(25.0, 90.0, 10.0))).is_empty());
        assert!(said(&c.observe(6.5, frame(25.0, 90.0, 10.0))).is_empty());
        let low = said(&c.observe(6.7, frame(25.0, 90.0, 10.0)));
        assert_eq!(low.len(), 1, "{low:?}");
        assert!(from(lines::HP_LOW, &low[0]), "{low:?}");
        assert!(low[0].contains("about 25 percent"), "{low:?}");
        // A frame's misreading (a dialog over the bar) is not a beating.
        let mut quiet = Companion::seeded(Settings::default(), SEED);
        quiet.observe(0.0, frame(95.0, 90.0, 10.0));
        assert!(said(&quiet.observe(0.5, frame(95.0, 90.0, 10.0))).is_empty());
        assert!(said(&quiet.observe(0.6, frame(50.0, 90.0, 10.0))).is_empty());
        assert!(said(&quiet.observe(0.7, frame(95.0, 90.0, 10.0))).is_empty());
        assert!(said(&quiet.observe(1.0, frame(95.0, 90.0, 10.0))).is_empty());
        // Slow attrition is the low warning's business, not a beating.
        let mut slow = Companion::seeded(Settings::default(), SEED);
        slow.observe(0.0, frame(95.0, 90.0, 10.0));
        for i in 1..60 {
            let hp = 95.0 - i as f32;
            let lines = said(&slow.observe(i as f64 * 0.5, frame(hp, 90.0, 10.0)));
            assert!(lines.is_empty() || hp < 30.0, "{i}: {lines:?}");
        }
    }

    /// A grind at level 165, the numbers read off the HUD: every 6 s a hit
    /// takes HP from 100 to 55 over a second and a half, and a potion puts
    /// it back.
    fn grind(t: f64) -> f32 {
        let phase = t % 6.0;
        if phase < 1.5 {
            100.0 - (phase / 1.5 * 45.0) as f32
        } else if phase < 3.0 {
            55.0
        } else {
            100.0
        }
    }

    #[test]
    fn a_beating_is_said_once_per_fight() {
        // Ten minutes of the grind: one night this was said every 12 s for
        // an hour ("Shut up"). Once, at the first hit; the potions answer it.
        let mut c = Companion::seeded(Settings::default(), SEED);
        let mut lines: Vec<(f64, String)> = Vec::new();
        for i in 0..6000 {
            let t = i as f64 * 0.1;
            for line in alerts(&c.observe(t, read(grind(t)))) {
                lines.push((t, line));
            }
        }
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert!(
            lines[0].0 < 2.0 && from(lines::BEATING, &lines[0].1),
            "{lines:?}"
        );
        assert!(!c.alerts_held(600.0));
        // Two quiet minutes end the fight: the next hit is called at once.
        for i in 0..1200 {
            let lines = alerts(&c.observe(600.0 + i as f64 * 0.1, read(100.0)));
            assert!(lines.is_empty(), "{lines:?}");
        }
        assert!(alerts(&c.observe(720.0, read(60.0))).is_empty());
        let again = alerts(&c.observe(720.1, read(58.0)));
        assert_eq!(again.len(), 1, "{again:?}");
        assert!(from(lines::BEATING, &again[0]), "{again:?}");
        // "No more HP warnings" means this one too.
        let mut quiet = Companion::seeded(
            Settings {
                hp_low: 0.0,
                ..Settings::default()
            },
            SEED,
        );
        for i in 0..6000 {
            let t = i as f64 * 0.1;
            let lines = alerts(&quiet.observe(t, read(grind(t))));
            assert!(lines.is_empty(), "{t}: {lines:?}");
        }
    }

    #[test]
    fn a_beating_nobody_pots_after_is_said_again_after_longer_and_longer_waits() {
        // Hit from 100 to 68, and from then on hit for 27 every so often
        // with never a potion's worth of HP back above where it was when
        // the line was said: said again 12 s after the first, 30 s after
        // the second, 60 s after the third — and not for the hits between.
        let hp = |tenth: usize| -> f32 {
            match tenth {
                0..=9 => 100.0,
                10 => 69.0,
                11..=29 => 68.0,
                30..=69 | 72..=129 => 77.0,
                70..=71 | 130..=131 => 50.0,
                132..=249 | 252..=429 => 59.0,
                250..=251 | 430..=431 => 32.0,
                432..=729 | 732..=1029 => 41.0,
                730..=731 | 1030..=1031 => 14.0,
                1032..=1099 => 23.0,
                // At last a potion, and the next hit is their business.
                1100..=1249 | 1252..=1300 => 100.0,
                1250..=1251 => 60.0,
                _ => unreachable!(),
            }
        };
        let mut c = Companion::seeded(Settings::default(), SEED);
        let mut beatings = Vec::new();
        for tenth in 0..=1300 {
            let t = tenth as f64 * 0.1;
            for line in alerts(&c.observe(t, read(hp(tenth)))) {
                if from(lines::BEATING, &line) {
                    beatings.push(t);
                }
            }
        }
        let expected = [1.1, 13.1, 43.1, 103.1];
        assert_eq!(beatings.len(), expected.len(), "{beatings:?}");
        for (at, want) in beatings.iter().zip(expected) {
            assert!((at - want).abs() < 0.05, "{beatings:?}");
        }
    }

    #[test]
    fn one_frame_read_high_is_neither_a_potion_nor_a_bar_to_fall_from() {
        // HP read at 20 for five minutes with one frame of 60 at 20 s (a
        // misread): no beating — the bar did not move — and the one frame
        // is no potion either: the line is said once, as for any bar that
        // stays put, and nothing after. One night this was "Back off,
        // you're getting shredded. 20 percent and falling" at 20.2 s. The
        // same with a frame of 52 every 15 s at 25%, which had the fight
        // go quiet for 285 s after the beating.
        type Lines = Vec<(f64, String)>;
        let cadence = |glitch: &dyn Fn(f64) -> Option<f32>, low: f32| -> (Lines, Lines) {
            let mut c = Companion::seeded(Settings::default(), SEED);
            let mut lines: Lines = Vec::new();
            let mut holds: Lines = Vec::new();
            for i in 0..3000 {
                let t = i as f64 * 0.1;
                let hp = glitch(t).unwrap_or(low);
                for action in c.observe(t, read(hp)) {
                    let Action::Say(say) = action else { continue };
                    match say.kind {
                        Kind::Warning | Kind::Alert => lines.push((t, say.text)),
                        Kind::Info if hold_note(&say.text) => holds.push((t, say.text)),
                        _ => {}
                    }
                }
            }
            (lines, holds)
        };
        // (Steady: said once — no new fall — and nothing to hold.)
        let (steady, held) = cadence(&|_| None, 20.0);
        let expected = [0.6];
        assert_eq!(steady.len(), expected.len(), "{steady:?}");
        assert!(held.is_empty(), "{held:?}");
        for (shape, (lines, holds)) in [
            (
                "one frame of 60 at 20 s",
                cadence(&|t| ((t - 20.0).abs() < 0.05).then_some(60.0), 20.0),
            ),
            (
                "a frame of 52 every 15 s",
                cadence(&|t| (t > 1.0 && t % 15.0 < 0.05).then_some(52.0), 25.0),
            ),
        ] {
            assert_eq!(lines.len(), expected.len(), "{shape}: {lines:?}");
            for ((at, line), want) in lines.iter().zip(expected) {
                assert!((at - want).abs() < 0.15, "{shape}: {lines:?}");
                assert!(!from(lines::BEATING, line), "{shape}: {lines:?}");
            }
            assert!(holds.is_empty(), "{shape}: {holds:?}");
        }
        // A potion that holds (HP read at 100 from the second frame on)
        // answers at once: worn down slowly to 20 again in the same
        // fight, nothing more is said until the bar goes lower than at
        // the line. One frame at 100 answers nothing: the cadence goes on.
        for answered in [true, false] {
            let mut c = Companion::seeded(Settings::default(), SEED);
            let mut lines: Vec<(f64, String)> = Vec::new();
            for i in 0..600 {
                let t = i as f64 * 0.1;
                // (Unanswered, it falls on to 14: the cadence goes on.)
                let hp = match i {
                    0..50 => 20.0,
                    50 => 100.0,
                    51..110 if answered => 100.0,
                    110..210 if answered => 100.0 - (i - 110) as f32 * 0.8,
                    _ if answered => 20.0,
                    _ => 14.0,
                };
                for line in alerts(&c.observe(t, read(hp))) {
                    lines.push((t, line));
                }
            }
            if answered {
                assert_eq!(lines.len(), 1, "{lines:?}");
                let lower = alerts(&c.observe(60.0, read(15.0)));
                assert_eq!(lower.len(), 1, "{lower:?}");
                assert!(says(lines::HP_LOW, &lower[0], "15 percent"), "{lower:?}");
            } else {
                // (A repeat of an unanswered line, not a first: the one
                // frame at 100 answered nothing.)
                assert_eq!(lines.len(), 2, "{lines:?}");
                assert!((lines[1].0 - 12.6).abs() < 0.15, "{lines:?}");
                assert!(from(lines::HP_LOW_AGAIN, &lines[1].1), "{lines:?}");
            }
        }
        // The beating's answer is the same. Hit from 100 to 60 at 1 s,
        // and the next hit 13 s on: after a potion that held, the next
        // hit is their business; after one frame of 100, the beating is
        // unanswered and the next hit has it said again.
        for (frames, answered) in [(2, true), (1, false)] {
            let mut c = Companion::seeded(Settings::default(), SEED);
            let mut beatings = Vec::new();
            for i in 0..300 {
                let t = i as f64 * 0.1;
                let hp = if i < 10 {
                    100.0
                } else if i < 50 {
                    60.0
                } else if i < 50 + frames || (i < 140 && answered) {
                    100.0
                } else if i < 140 || answered {
                    60.0
                } else {
                    32.0
                };
                for line in alerts(&c.observe(t, read(hp))) {
                    if from(lines::BEATING, &line) {
                        beatings.push(t);
                    }
                }
            }
            assert_eq!(beatings.len(), if answered { 1 } else { 2 }, "{beatings:?}");
        }
    }

    #[test]
    fn a_player_who_always_pots_is_not_shouted_at_for_every_beating() {
        // A fight every two minutes for 65 min: hit from 100 to 25 over
        // 1.5 s, 1.5 s there, and potted — within 3 s of the beating,
        // every time. Three beatings, then the player is trusted with it:
        // nothing for the thirty fights after (no low line either, in the
        // seconds the potion takes). One night this was 34 beatings in 65
        // min, one per fight. Only a late potion, none, or a death ends
        // the trust; a deeper fall is shouted for its depth and, potted in
        // time, counts like the rest (the floor follows it).
        let fight = |phase: f64, floor: f32| -> f32 {
            if phase < 1.5 {
                100.0 - (phase / 1.5) as f32 * (100.0 - floor)
            } else if phase < 3.0 {
                floor
            } else {
                100.0
            }
        };
        let mut c = Companion::seeded(Settings::default(), SEED);
        let mut lines: Vec<(f64, String)> = Vec::new();
        for i in 0..39_000 {
            let t = i as f64 * 0.1;
            for line in dealt(&c.observe(t, read(fight(t % 120.0, 25.0)))) {
                lines.push((t, line));
            }
        }
        assert_eq!(lines.len(), 3, "{lines:?}");
        for (n, (at, line)) in lines.iter().enumerate() {
            assert!(from(lines::BEATING, line), "{lines:?}");
            assert!((at - (n as f64 * 120.0 + 0.8)).abs() < 0.15, "{lines:?}");
        }
        // They stop potting: the fall is watched for the time they usually
        // take, then shouted after all — in that fight, not a later one —
        // and the next fights are shouted again until trusted anew.
        let mut lines: Vec<(f64, String)> = Vec::new();
        for i in 0..6000 {
            let t = i as f64 * 0.1;
            let phase = t % 120.0;
            let hp = if t < 120.0 {
                // (The first fight of this stretch: no potion for 20 s.)
                if phase < 1.5 {
                    fight(phase, 25.0)
                } else if phase < 20.0 {
                    25.0
                } else {
                    100.0
                }
            } else {
                fight(phase, 25.0)
            };
            for line in dealt(&c.observe(3900.0 + t, read(hp))) {
                lines.push((t, line));
            }
        }
        assert!(from(lines::BEATING, &lines[0].1), "{lines:?}");
        assert!((lines[0].0 - 3.9).abs() < 0.15, "{lines:?}");
        // (Unanswered, HP staying at 25: no new fall, so no low line after
        // the beating's.)
        assert_eq!(lines.len(), 4, "{lines:?}");
        let later: Vec<&(f64, String)> = lines.iter().filter(|(t, _)| *t >= 120.0).collect();
        assert_eq!(later.len(), 3, "{lines:?}");
        for (n, (at, line)) in later.iter().enumerate() {
            assert!(from(lines::BEATING, line), "{lines:?}");
            assert!(
                (at - ((n + 1) as f64 * 120.0 + 0.8)).abs() < 0.15,
                "{lines:?}"
            );
        }
        // Trusted again (three more handled); then a fight deeper than
        // their usual floor by more than the margin: shouted the moment
        // HP goes under it — the depth deserves that — and, potted in
        // time, handled like the rest: the trust holds, the floor follows
        // it down, and the next fight to that depth is watched and silent.
        // (A deeper handled fall started the streak over: the fight after
        // it was shouted.) One within the margin of the new floor but
        // under the floor's bottom — half the mark, 15 — is shouted all
        // the same: the floor went no lower than that, and a fall to 12 is
        // said however well they handle it.
        let mut lines: Vec<(f64, String)> = Vec::new();
        for i in 0..4800 {
            let t = i as f64 * 0.1;
            let floor = match (t / 120.0) as u32 {
                0 => 25.0,
                1 | 2 => 15.0,
                _ => 12.0,
            };
            for line in dealt(&c.observe(9900.0 + t, read(fight(t % 120.0, floor)))) {
                lines.push((t, line));
            }
        }
        assert_eq!(lines.len(), 2, "{lines:?}");
        assert!(from(lines::BEATING, &lines[0].1), "{lines:?}");
        // (Under 20 at 1.4 s into the fall: 100 to 15 over 1.5 s.)
        assert!((lines[0].0 - 121.5).abs() < 0.15, "{lines:?}");
        // (Said with the reading, when the card carries one: under 20.)
        assert!(
            !lines[0].1.contains("percent")
                || (15..20).any(|n| lines[0].1.contains(&format!("{n} percent"))),
            "{lines:?}"
        );
        // (The fight to 15 at 240 s quiet; the one to 12 shouted at 12,
        // the first reading under 15.)
        assert!(from(lines::BEATING, &lines[1].1), "{lines:?}");
        assert!((lines[1].0 - 361.5).abs() < 0.15, "{lines:?}");
        assert!(
            !lines[1].1.contains("percent") || lines[1].1.contains("12 percent"),
            "{lines:?}"
        );
        // While the trust is built, the same: a fight to 60, then dips to
        // 25 and to 12, each potted in two seconds — three shouts, the
        // dips for their depth — and the player is trusted: the fights to
        // 60 after them are silent. (One evening's grind: the dips started
        // the count over, and 60, 25, 12, 60, 60 were five shouts.)
        let mut c = Companion::seeded(Settings::default(), SEED);
        let mut lines: Vec<(f64, String)> = Vec::new();
        for i in 0..6000 {
            let t = i as f64 * 0.1;
            let floor = match (t / 120.0) as u32 {
                0 => 60.0,
                1 => 25.0,
                2 => 12.0,
                _ => 60.0,
            };
            for line in dealt(&c.observe(t, read(fight(t % 120.0, floor)))) {
                lines.push((t, line));
            }
        }
        assert_eq!(lines.len(), 3, "{lines:?}");
        for ((at, line), want) in lines.iter().zip([1.3, 120.8, 240.7]) {
            assert!(from(lines::BEATING, line), "{lines:?}");
            assert!((at - want).abs() < 0.15, "{lines:?}");
        }
        // The trust outlives a shout for depth, not the fight's answer.
        // Trusted (fights to 60), a fall to 30 is shouted under the floor;
        // no potion answers it, and the next hit lands thirteen seconds
        // on: shouted again at once (the beating's second wait) — it is
        // under the floor too, as a fall from an unanswered one must be —
        // not watched for three seconds first.
        let mut c = Companion::seeded(Settings::default(), SEED);
        for i in 0..4800 {
            let t = i as f64 * 0.1;
            c.observe(t, read(fight(t % 120.0, 60.0)));
        }
        let mut lines: Vec<(f64, String)> = Vec::new();
        for i in 0..200 {
            let t = i as f64 * 0.1;
            let hp = if t < 1.5 {
                fight(t, 30.0)
            } else if !(13.0..14.0).contains(&t) {
                30.0
            } else {
                // (Nine points back: not a potion's worth.)
                62.0
            };
            for line in dealt(&c.observe(480.0 + t, read(hp))) {
                lines.push((t, line));
            }
        }
        let beatings: Vec<f64> = lines
            .iter()
            .filter(|(_, l)| from(lines::BEATING, l))
            .map(|(t, _)| *t)
            .collect();
        assert_eq!(beatings.len(), 2, "{lines:?}");
        assert!((beatings[0] - 1.0).abs() < 0.15, "{beatings:?}");
        assert!((beatings[1] - 14.1).abs() < 0.15, "{beatings:?}");
        // A fight within the margin of the floor is handled like the rest.
        let mut c = Companion::seeded(Settings::default(), SEED);
        let mut lines: Vec<(f64, String)> = Vec::new();
        for i in 0..6000 {
            let t = i as f64 * 0.1;
            let floor = if t < 360.0 { 25.0 } else { 22.0 };
            for line in dealt(&c.observe(t, read(fight(t % 120.0, floor)))) {
                lines.push((t, line));
            }
        }
        assert_eq!(lines.len(), 3, "{lines:?}");
        // A death ends the trust: the next fight is shouted.
        let mut c = Companion::seeded(Settings::default(), SEED);
        let mut lines: Vec<(f64, String)> = Vec::new();
        for i in 0..6000 {
            let t = i as f64 * 0.1;
            let hp = if (400.0..405.0).contains(&t) {
                0.0
            } else {
                fight(t % 120.0, 25.0)
            };
            for line in dealt(&c.observe(t, read(hp))) {
                lines.push((t, line));
            }
        }
        let beatings: Vec<f64> = lines
            .iter()
            .filter(|(_, l)| from(lines::BEATING, l))
            .map(|(t, _)| *t)
            .collect();
        // (Three shouted, the fourth fight watched, the death, and the
        // fifth fight shouted.)
        assert_eq!(beatings.len(), 4, "{lines:?}");
        assert!((beatings[3] - 480.8).abs() < 0.15, "{beatings:?}");
        assert!(
            lines.iter().any(|(_, l)| from(lines::DEATH, l)),
            "{lines:?}"
        );
    }

    #[test]
    fn a_trusted_player_is_never_left_unwarned_under_half_the_mark() {
        // The trust floor has a bottom: half the mark (15 at 30). Fights
        // two minutes apart from minute 1, HP from 100 to the dip over
        // 1.5 s, held there and potted 3 s after the fight began (`None`:
        // never); killed outright at `kill`; dead, it stays at zero for
        // 30 s. (With no bottom the floor followed the dips down: 26
        // minutes of silence through dips to 12, 8 and 4, then a death read
        // as unwarned — "New rule: I yell at 35." — for a silence the
        // trust had chosen.)
        type Fight = (f64, f32, Option<f64>);
        type Lines = Vec<(f64, f32, String)>;
        let play = |fights: &[Fight], kill: f64, end: f64| -> (Lines, f32) {
            let mut c = Companion::seeded(Settings::default(), SEED);
            let mut lines: Lines = Vec::new();
            let mut dead_since = f64::NEG_INFINITY;
            for i in 0..(end * 10.0) as usize {
                let t = i as f64 * 0.1;
                let mut hp = 100.0;
                for &(t0, dip, pot) in fights {
                    let s = t - t0;
                    if s >= 0.0 && pot.is_none_or(|p| s < p) {
                        hp = if s < 1.5 {
                            100.0 - (s / 1.5) as f32 * (100.0 - dip)
                        } else {
                            dip
                        };
                    }
                }
                if t >= kill {
                    hp = 0.0;
                }
                if hp <= 0.5 && t - dead_since >= 30.0 {
                    dead_since = t;
                }
                if t - dead_since < 30.0 {
                    hp = 0.0;
                }
                for line in dealt(&c.observe(t, read(hp))) {
                    lines.push((t, hp, line));
                }
            }
            (lines, c.settings.hp_low)
        };
        let during = |lines: &Lines, t0: f64| -> Lines {
            lines
                .iter()
                .filter(|(t, _, _)| (t0..t0 + 120.0).contains(t))
                .cloned()
                .collect()
        };
        // A death after a fall it shouted: warned, and the mark stays.
        let warned_death = |lines: &Lines, mark: f32| {
            let (_, _, death) = lines.last().unwrap();
            assert!(from(lines::DEATH_WARNED, death), "{lines:?}");
            assert!(without_sooner(death, 35).is_none(), "{death:?}");
            assert_eq!(mark, 30.0);
        };
        // A: each fight 4 points deeper, 60, 56, … 4, and the one to 0
        // kills. Three shouts build the trust; the dips to 48 … 16 are
        // watched and quiet (each within the margin of the last); every
        // dip under 15 is shouted, under 15.
        let a: Vec<Fight> = (0..=15)
            .map(|k| (60.0 + 120.0 * k as f64, 60.0 - 4.0 * k as f32, Some(3.0)))
            .collect();
        let (lines, mark) = play(&a, f64::INFINITY, 1950.0);
        for &(t0, dip, _) in &a {
            let said = during(&lines, t0);
            let beatings: Vec<_> = said
                .iter()
                .filter(|(_, _, l)| from(lines::BEATING, l))
                .collect();
            if dip >= 52.0 {
                assert_eq!(beatings.len(), 1, "{dip}: {said:?}");
            } else if dip >= 15.0 {
                assert!(said.is_empty(), "{dip}: {said:?}");
            } else {
                assert_eq!(beatings.len(), 1, "{dip}: {said:?}");
                assert!(beatings[0].1 < 15.0, "{dip}: {said:?}");
            }
        }
        warned_death(&lines, mark);
        // B: the floor walked down to 8 (said at 12 and 8 now), then a
        // fall to 3 with no potion and dead 3.5 s later: shouted first.
        let mut b: Vec<Fight> = (0..=13)
            .map(|k| (60.0 + 120.0 * k as f64, 60.0 - 4.0 * k as f32, Some(3.0)))
            .collect();
        b.push((1740.0, 3.0, None));
        let (lines, mark) = play(&b, 1743.5, 1800.0);
        let last = during(&lines, 1740.0);
        assert_eq!(last.len(), 2, "{last:?}");
        assert!(from(lines::BEATING, &last[0].2), "{last:?}");
        assert!(last[0].0 < 1743.5 && last[0].1 < 15.0, "{last:?}");
        warned_death(&lines, mark);
        // D: trusted at about 57, then a dip to 8 potted in time — the
        // floor is 8 now — and dips to 5 and 2, potted in time: all three
        // shouted, the last two under 15.
        let d: Vec<Fight> = [60.0, 60.0, 60.0, 57.0, 8.0, 5.0, 2.0]
            .into_iter()
            .enumerate()
            .map(|(k, dip)| (60.0 + 120.0 * k as f64, dip, Some(3.0)))
            .collect();
        let (lines, mark) = play(&d, f64::INFINITY, 900.0);
        assert!(during(&lines, 420.0).is_empty(), "{lines:?}");
        for (t0, under) in [(540.0, 52.0), (660.0, 15.0), (780.0, 15.0)] {
            let said = during(&lines, t0);
            assert_eq!(said.len(), 1, "{t0}: {lines:?}");
            assert!(from(lines::BEATING, &said[0].2), "{t0}: {lines:?}");
            assert!(said[0].1 < under, "{t0}: {lines:?}");
        }
        assert_eq!(mark, 30.0);
    }

    #[test]
    fn staying_low_is_said_once_and_falling_on_again_after_longer_and_longer_waits() {
        // HP at 20% and nothing done about it, the player grumbling at
        // minutes 2.5 and 5 but drinking nothing: said once. (It was again
        // 12 s on, 30 s after that, then 60, then 120: "46 percent HP"
        // eleven times in fifteen minutes to the owner standing still.)
        let mut c = Companion::seeded(Settings::default(), SEED);
        let mut told = Vec::new();
        for i in 0..4000 {
            let t = i as f64 * 0.1;
            if i == 1500 || i == 3000 {
                c.player_spoke(t);
            }
            for line in alerts(&c.observe(t, frame(20.0, 90.0, 10.0))) {
                told.push((t, line));
            }
        }
        assert_eq!(told.len(), 1, "{told:?}");
        assert!((told[0].0 - 0.6).abs() < 0.15, "{told:?}");
        assert!(from(lines::HP_LOW, &told[0].1), "{told:?}");
        // Falling on, no potion — a point every two seconds from 29: said
        // again at each new fall of five points or more, after the waits
        // (12, 30, 60 s), not for the frames between; the first stands
        // alone, the second may say "still", the third asks why.
        let mut c = Companion::seeded(Settings::default(), SEED);
        let mut told: Vec<(f64, f32, String)> = Vec::new();
        for i in 0..2000 {
            let t = i as f64 * 0.1;
            let hp = (29.0 - (t / 2.0).floor() as f32).max(1.0);
            for line in alerts(&c.observe(t, read(hp))) {
                told.push((t, hp, line));
            }
        }
        let at: Vec<f64> = told.iter().map(|(t, _, _)| *t).collect();
        assert_eq!(told.len(), 4, "{told:?}");
        for (at, want) in at.iter().zip([0.6, 12.6, 42.6, 102.6]) {
            assert!((at - want).abs() < 0.15, "{told:?}");
        }
        for pair in told.windows(2) {
            assert!(pair[1].1 <= pair[0].1 - NEW_FALL, "{told:?}");
        }
        for ((_, _, line), deck) in told.iter().zip([
            lines::HP_LOW,
            lines::HP_LOW_AGAIN,
            lines::HP_LOW_ASK,
            lines::HP_LOW_AGAIN,
        ]) {
            assert!(from(deck, line), "{told:?}");
        }
    }

    /// The owner's 13:08–13:23 (2026-10-10), replayed: standing, not
    /// fighting, HP read at 46% the whole time ("46 percent" eleven times
    /// in fifteen minutes, every wait of the cadence), MP coming back by
    /// its regen, EXP flat, the HUD lost now and then — a few frames with
    /// no level, and twice for over a minute no reading at all. A steady
    /// low bar is said once; a gap in the readings is no new fight.
    #[test]
    fn a_steady_low_hp_is_said_once_through_the_huds_gaps() {
        // (His mark had moved to 50 after deaths with no warning before.)
        let mut c = Companion::seeded(
            Settings {
                hp_low: 50.0,
                ..Settings::default()
            },
            SEED,
        );
        let mut lines: Vec<(f64, String)> = Vec::new();
        // 13:08:10 is 0 s; to 13:24:30, two frames a second.
        for i in 0..1960 {
            let t = i as f64 * 0.5;
            let mut obs = read(46.4);
            obs.level = Some(17);
            obs.mp = gauge((32.0 + t as f32 * 0.25).min(100.0), true);
            obs.exp = gauge(44.63, true);
            // The HUD lost: no level for a few frames every minute, and no
            // reading at all for 70 s at 4:00 and at 9:30.
            if t % 60.0 < 2.0 {
                obs.level = None;
            }
            if (240.0..310.0).contains(&t) || (570.0..640.0).contains(&t) {
                obs.hp = None;
                obs.mp = None;
                obs.exp = None;
                obs.level = None;
            }
            for line in alerts(&c.observe(t, obs)) {
                lines.push((t, line));
            }
        }
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert!(says(lines::HP_LOW, &lines[0].1, "46 percent"), "{lines:?}");
        assert!(lines[0].0 < 2.0, "{lines:?}");
    }

    /// A low bar that keeps falling — damage taken, no potion — is said
    /// again, after the waits; one that stays where it was is not. (The
    /// owner's 13:35–13:38: HP draining from 14% to 7% while he was away,
    /// then dead.)
    #[test]
    fn a_low_hp_said_again_only_after_a_new_fall() {
        let mut c = Companion::seeded(Settings::default(), SEED);
        let mut lines: Vec<(f64, String)> = Vec::new();
        for i in 0..3000 {
            let t = i as f64 * 0.1;
            // 25% for a minute, 14% for a minute (a new fall), 12% for a
            // minute (not one: two points), then 7%.
            let hp = match t {
                t if t < 60.0 => 25.0,
                t if t < 120.0 => 14.0,
                t if t < 180.0 => 12.0,
                _ => 7.0,
            };
            for line in alerts(&c.observe(t, read(hp))) {
                lines.push((t, line));
            }
        }
        let at: Vec<f64> = lines.iter().map(|(t, _)| *t).collect();
        assert_eq!(lines.len(), 3, "{lines:?}");
        assert!(says(lines::HP_LOW, &lines[0].1, "25 percent"), "{lines:?}");
        assert!((at[1] - 60.0).abs() < 1.0, "{lines:?}");
        assert!(lines[1].1.contains("14"), "{lines:?}");
        assert!((at[2] - 180.0).abs() < 1.0, "{lines:?}");
        assert!(lines[2].1.contains('7'), "{lines:?}");
    }

    /// The owner's evening: 25 MP lines in two hours to a Magician, whose
    /// MP goes up and down all the time — noise. No MP line unless he set a
    /// mark himself (`mp_low`, from his plain request); HP stays on.
    #[test]
    fn mp_warnings_are_off_unless_the_player_set_a_mark() {
        // MP spent and coming back, again and again: down to 5% each time.
        let evening = |settings: Settings| -> (Vec<String>, Tally) {
            let mut c = Companion::seeded(settings, SEED);
            let mut lines = Vec::new();
            for i in 0..6000 {
                let t = i as f64 * 0.1;
                let mut obs = read(90.0);
                obs.mp = gauge((95.0 - (t % 120.0) as f32).max(5.0), true);
                lines.extend(alerts(&c.observe(t, obs)));
            }
            (lines, c.tally())
        };
        assert_eq!(Settings::default().mp_low, 0.0);
        let (lines, tally) = evening(Settings::default());
        assert!(lines.is_empty(), "{lines:?}");
        assert_eq!(tally.mp_low, 0);
        // He asked to be warned under 20%: each of the five dips is said
        // (and, falling on, once more at most).
        let (lines, tally) = evening(Settings {
            mp_low: 20.0,
            ..Settings::default()
        });
        assert!((5..=10).contains(&lines.len()), "{lines:?}");
        assert_eq!(tally.mp_low as usize, lines.len());
        let mp_card = |l: &String| {
            [lines::MP_LOW, lines::MP_LOW_AGAIN, lines::MP_LOW_ASK]
                .iter()
                .any(|deck| from(*deck, l))
        };
        assert!(lines.iter().all(mp_card), "{lines:?}");
        // HP is warned of by default.
        let mut c = Companion::seeded(Settings::default(), SEED);
        let mut hp = Vec::new();
        for i in 0..10 {
            hp.extend(alerts(&c.observe(i as f64 * 0.1, read(20.0))));
        }
        assert_eq!(hp.len(), 1, "{hp:?}");
    }

    #[test]
    fn low_mp_is_its_own_warning() {
        // (When the player asked for MP warnings: none by default.)
        let mut c = Companion::seeded(
            Settings {
                mp_low: 15.0,
                ..Settings::default()
            },
            SEED,
        );
        c.observe(0.0, frame(90.0, 3.1, 10.0));
        c.observe(0.3, frame(90.0, 3.1, 10.0));
        assert_eq!(
            said(&c.observe(0.7, frame(90.0, 3.1, 10.0))),
            ["Your MP's down to about 3 percent."]
        );
    }

    #[test]
    fn a_beating_and_a_low_bar_are_warnings_and_a_death_and_a_level_up_are_news() {
        // A warning is shouted; news is told. The kind says which, and the
        // voice, the phone and the call all go by it: "Aw, you died" in the
        // voice of "POT NOW" was a machine with one setting for important.
        let kinds = |actions: &[Action]| -> Vec<Kind> {
            actions
                .iter()
                .filter_map(|a| match a {
                    Action::Say(s) if s.speak => Some(s.kind),
                    _ => None,
                })
                .collect()
        };
        // (MP warnings asked for: there are none by default.)
        let mut c = Companion::seeded(
            Settings {
                mp_low: 15.0,
                ..Settings::default()
            },
            SEED,
        );
        c.observe(0.0, frame(95.0, 90.0, 10.0));
        for i in 1..10 {
            c.observe(i as f64 * 0.1, frame(95.0, 90.0, 10.0));
        }
        // A beating, then the low warning behind it, then MP: warnings.
        c.observe(1.5, frame(65.0, 90.0, 10.0));
        assert_eq!(
            kinds(&c.observe(1.6, frame(64.0, 90.0, 10.0))),
            [Kind::Warning]
        );
        for t in [3.0, 3.3, 3.7, 6.5] {
            c.observe(t, frame(25.0, 90.0, 10.0));
        }
        assert_eq!(
            kinds(&c.observe(6.7, frame(25.0, 90.0, 10.0))),
            [Kind::Warning]
        );
        c.observe(7.0, frame(25.0, 3.0, 10.0));
        c.observe(7.3, frame(25.0, 3.0, 10.0));
        assert_eq!(
            kinds(&c.observe(7.7, frame(25.0, 3.0, 10.0))),
            [Kind::Warning]
        );
        // A death (the number read as zero): news.
        let mut dead = frame(0.0, 3.0, 10.0);
        dead.hp = Some(Gauge {
            percent: 0.0,
            current: Some(0),
            max: Some(9795),
            read: true,
        });
        let mut told = Vec::new();
        for i in 0..5 {
            told.extend(kinds(&c.observe(8.0 + i as f64 * 0.1, dead.clone())));
        }
        assert_eq!(told, [Kind::Alert]);
        // Revived, then a level (the number held three seconds): news.
        c.observe(12.0, frame(90.0, 90.0, 99.0));
        let mut told = Vec::new();
        for i in 0..40 {
            let mut next = frame(90.0, 90.0, 0.5);
            next.level = Some(58);
            told.extend(kinds(&c.observe(13.0 + i as f64 * 0.1, next)));
        }
        assert_eq!(told, [Kind::Alert]);
        // Twenty minutes with nothing moving and not a word: news too.
        assert_eq!(
            kinds(&c.observe(1700.0, frame(90.0, 90.0, 0.5))),
            [Kind::Alert]
        );
    }

    #[test]
    fn talking_over_it_is_told_from_its_own_voice_coming_back() {
        let mut c = Companion::seeded(Settings::default(), SEED);
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
        let quiet = Companion::seeded(Settings::default(), SEED);
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
    fn a_sentence_mostly_of_its_own_words_is_its_own_voice_whole() {
        // The owner's 12:21: its two lines, then the phone's "23% still owe
        // about 10% potion" — kept "23% still owe" at f9b6047, and the model
        // set his MP warnings at 23%. Mostly its words: dropped whole.
        let mut c = Companion::seeded(Settings::default(), SEED);
        c.remember_spoken(0.0, "Got it—MP's at 23%.");
        c.remember_spoken(4.0, "Still low on mana, about 10 percent. Blue potion!");
        assert_eq!(c.own_words(10.0, "23% still owe about 10% potion", 0), None);
        // A remainder never carries a name (11:49: "Miha" learned as his).
        let mut c = Companion::seeded(Settings::default(), SEED);
        c.remember_spoken(0.0, "Got it—that bunny-ear hat is you, Armani.");
        assert_eq!(
            c.own_words(
                10.0,
                "Got it that bunny hat is my name is Armani my name is Miha",
                0
            ),
            None
        );
        // Three words and more of the player's are theirs (12:01: walking
        // to the NPC); so is a question about what it said, and a word
        // that stops it.
        let mut c = Companion::seeded(Settings::default(), SEED);
        c.remember_spoken(
            0.0,
            "General Store NPC is the tall raccoon on the right side of the map.",
        );
        assert_eq!(
            c.own_words(
                11.0,
                "NPC is the racoon on the right side of the map OK tell me when to stop",
                0
            )
            .as_deref(),
            Some("racoon OK tell me when to stop")
        );
        assert!(
            c.own_words(11.0, "the tall raccoon on the right side where is it", 0)
                .is_some()
        );
        assert_eq!(
            c.own_words(11.0, "the tall raccoon on the right side mute please", 0)
                .as_deref(),
            Some("mute please")
        );
        // Two words of the player's, the rest its own: its own (12:10,
        // "Join the cannabis" kept at f9b6047).
        let mut c = Companion::seeded(Settings::default(), SEED);
        c.remember_spoken(
            0.0,
            "You need an item drop rate buff: equip a drop coupon or join a party with a Kanna or Bishop for Holy Symbol.",
        );
        let heard = "Join the party with a cannabis";
        assert_eq!(
            c.strip_echo(9.0, heard).as_deref(),
            Some("Join the cannabis")
        );
        assert_eq!(c.own_words(9.0, heard, 0), None);
        // "The last 8 s" count from the end of its speaking: a long line
        // noted at 0 is still being said at 8 s, and heard back at 12 s.
        let mut c = Companion::seeded(Settings::default(), SEED);
        c.remember_spoken(
            0.0,
            "To reach Blue Mushroom Forest 2, follow the world map's Henesys route south of town; select the nearby mushroom-forest map.",
        );
        let heard = "route south of town for two";
        assert_eq!(c.strip_echo(12.0, heard).as_deref(), Some("for two"));
        assert_eq!(c.own_words(12.0, heard, 0), None);
        // Long after, the player saying its words is the player.
        assert_eq!(c.own_words(40.0, heard, 0).as_deref(), Some(heard));
    }

    #[test]
    fn what_was_heard_over_it_needs_enough_of_the_players_own_words() {
        let mut c = Companion::seeded(Settings::default(), SEED);
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
        let mut c = Companion::seeded(Settings::default(), SEED);
        c.observe(0.0, frame(50.0, 50.0, 10.0));
        let mut lines = Vec::new();
        // From the bar's fill, none: a fill at zero is no reading (see
        // `SLIVER`), EXP penalty (10% to 9%) or not. (At 03303a9 this was
        // a death after two seconds: a death missed now.)
        for i in 0..40 {
            lines.extend(said(
                &c.observe(1.0 + i as f64 * 0.1, frame(0.0, 50.0, 9.0)),
            ));
        }
        assert!(lines.is_empty(), "{lines:?}");
        assert_eq!(c.deaths, 0);
        // From the printed number, at once, and once.
        let mut c = Companion::seeded(Settings::default(), SEED);
        c.observe(0.0, frame(50.0, 50.0, 10.0));
        let mut lines = Vec::new();
        for i in 0..40 {
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
        let mut c = Companion::seeded(Settings::default(), SEED);
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
        assert!(from(lines::BEATING, &beating[0]), "{beating:?}");
        // A fight — hit, potion, hit, potion — is not a misread.
        let mut c = Companion::seeded(Settings::default(), SEED);
        c.observe(0.0, frame(95.0, 90.0, 10.0));
        c.observe(1.0, frame(55.0, 90.0, 10.0));
        c.observe(1.1, frame(55.0, 90.0, 10.0));
        c.observe(3.0, frame(95.0, 90.0, 10.0));
        c.observe(5.0, frame(60.0, 90.0, 10.0));
        assert!(!c.hp_steady.unsteady(5.1, 60.0));
    }

    #[test]
    fn a_death_is_seen_through_a_bar_that_swings() {
        // His cursor over the start of the HP bar (Classic, the number
        // unread, the bar's fill guessed at): 100, 3, 100, 46, 9… ten
        // times a second — a swing, held — and now and then an empty bar
        // for a frame or two. No death in that. Then he dies for real,
        // inside the hold: the number reads 0 in the game's font for 3 s.
        // The death is said, once, at once — and the mark does not move for
        // it (the way down was not seen). At a095e2b the hold reset the
        // count of empty frames every frame: the death waited out the hold,
        // 20 s that doubles up to ten minutes while the cursor stays. (The
        // bar's fill at 0 with EXP lost was a death too, through the hold,
        // until a fill at 2% or under was made no reading: see `SLIVER`.)
        let swing = [
            100.0, 3.0, 100.0, 46.0, 9.0, 0.0, 100.0, 28.0, 0.0, 0.0, 3.0, 100.0,
        ];
        let mut c = Companion::seeded(Settings::default(), SEED);
        c.observe(0.0, frame(100.0, 90.0, 10.0));
        let mut lines: Vec<(f64, String)> = Vec::new();
        let deaths = |lines: &[(f64, String)]| -> Vec<(f64, String)> {
            lines
                .iter()
                .filter(|(_, l)| from(lines::DEATH, l) || from(lines::DEATH_WARNED, l))
                .cloned()
                .collect()
        };
        for i in 1..300 {
            let t = i as f64 * 0.1;
            let hp = swing[i % swing.len()];
            for line in dealt(&c.observe(t, frame(hp, 90.0, 10.0))) {
                lines.push((t, line));
            }
        }
        assert!(deaths(&lines).is_empty(), "{lines:?}");
        assert!(!c.dead());
        // (Held: the swing is still being guessed at as it ends.)
        assert!(c.hp_steady.unsteady_until > 30.0, "{lines:?}");
        let zero_read = |exp: f32| {
            let mut obs = frame(0.0, 90.0, exp);
            obs.hp = gauge(0.0, true);
            obs
        };
        for i in 300..330 {
            let t = i as f64 * 0.1;
            for line in dealt(&c.observe(t, zero_read(9.0))) {
                lines.push((t, line));
            }
        }
        let died = deaths(&lines);
        assert_eq!(died.len(), 1, "{lines:?}");
        assert!((died[0].0 - 30.2).abs() < 0.05, "{died:?}");
        assert!(without_sooner(&died[0].1, 35).is_none(), "{died:?}");
        assert!(c.dead());
        assert_eq!(c.settings.hp_low, 30.0);
        // Revived, the cursor gone: the bar steady at full, a new life once
        // the hold is over — and the death is not said again.
        for i in 330..1200 {
            let t = i as f64 * 0.1;
            for line in dealt(&c.observe(t, frame(100.0, 90.0, 9.0))) {
                lines.push((t, line));
            }
        }
        assert_eq!(deaths(&lines).len(), 1, "{lines:?}");
        assert!(!c.dead());
        // The bar read steady at 40% — over the mark, in the band where a
        // sooner warning would have come — then the cursor on it (40, 10,
        // 40, 10…), then the death (read in the font): no warning came, but
        // the way down was a guess, and the mark stays where it is.
        let mut c = Companion::seeded(Settings::default(), SEED);
        let mut lines: Vec<(f64, String)> = Vec::new();
        for i in 0..400 {
            let t = i as f64 * 0.1;
            let obs = match i {
                0..50 => frame(40.0, 90.0, 10.0),
                50..372 if i % 2 == 1 => frame(10.0, 90.0, 10.0),
                50..372 => frame(40.0, 90.0, 10.0),
                _ => zero_read(9.0),
            };
            for line in dealt(&c.observe(t, obs)) {
                lines.push((t, line));
            }
        }
        let died = deaths(&lines);
        assert_eq!(died.len(), 1, "{lines:?}");
        assert!((died[0].0 - 37.4).abs() < 0.05, "{died:?}");
        assert!(from(lines::DEATH, &died[0].1), "{died:?}");
        assert_eq!(c.settings.hp_low, 30.0, "{died:?}");
    }

    #[test]
    fn a_cursor_parked_on_the_bars_start_is_no_death() {
        // w29's script: the cursor swings the bar's estimate for 10 s, then
        // rests on the bar's start — the fill read empty for 2.5 s — then
        // moves off. At 3851c5e that was a death, said ("Oh, I said to pot.
        // Get up…"), counted and told to the coach — and claiming a warning:
        // the swing's first frames, before it was found to swing, passed
        // for a beating.
        let swing = [
            100.0, 3.0, 100.0, 46.0, 9.0, 0.0, 100.0, 28.0, 0.0, 0.0, 3.0, 100.0,
        ];
        let is_death = |l: &str| from(lines::DEATH, l) || from(lines::DEATH_WARNED, l);
        // (`before`: HP steady for 5 s before the swing, or not; `read`:
        // the zero read in the game's font — a real death — or the fill.)
        let run = |before: Option<f32>, exp_at_zero: f32, read: bool| {
            let mut c = Companion::seeded(Settings::default(), SEED);
            let mut lines = Vec::new();
            let start = if before.is_some() { 0 } else { 50 };
            for i in start..300 {
                let t = i as f64 * 0.1;
                let (hp, exp) = match i {
                    0..50 => (before.unwrap_or(100.0), 10.0),
                    50..150 => (swing[i % swing.len()], 10.0),
                    150..175 => (0.0, exp_at_zero),
                    _ => (80.0, exp_at_zero),
                };
                let mut obs = frame(hp, 90.0, exp);
                if (150..175).contains(&i) {
                    obs.hp = gauge(0.0, read);
                }
                for line in dealt(&c.observe(t, obs)) {
                    lines.push((t, line));
                }
            }
            (lines, c.deaths > 0)
        };
        // Parked: no death.
        let (parked, died) = run(None, 10.0, false);
        assert!(!died, "{parked:?}");
        assert!(parked.iter().all(|(_, l)| !is_death(l)), "{parked:?}");
        // (The swing's first frames did pass for a beating: nothing is
        // known yet.)
        assert!(
            parked
                .iter()
                .any(|(t, l)| *t < 6.0 && from(lines::BEATING, l)),
            "{parked:?}"
        );
        // A death through the hold (read 0 in the font): said — and not as
        // one it warned of: that beating was the swing's.
        let (lines, died) = run(None, 9.0, true);
        let deaths: Vec<&(f64, String)> = lines.iter().filter(|(_, l)| is_death(l)).collect();
        assert!(died);
        assert_eq!(deaths.len(), 1, "{lines:?}");
        assert!(from(lines::DEATH, &deaths[0].1), "{deaths:?}");
        // HP read steady under the mark before the swing (and warned of
        // then): a warned death.
        let (lines, died) = run(Some(20.0), 10.0, true);
        let deaths: Vec<&(f64, String)> = lines.iter().filter(|(_, l)| is_death(l)).collect();
        assert!(died);
        assert_eq!(deaths.len(), 1, "{lines:?}");
        assert!(from(lines::DEATH_WARNED, &deaths[0].1), "{deaths:?}");
        // The same two from the fill's 0 (EXP lost; steady under the mark
        // before): deaths at 03303a9, none now — a fill at 2% or under is
        // no reading (see `SLIVER`). Missed, as on the learned sight.
        assert!(!run(None, 9.0, false).1);
        assert!(!run(Some(20.0), 10.0, false).1);
        // Steady over the mark before it: parked, no death.
        let (lines, died) = run(Some(80.0), 10.0, false);
        assert!(!died, "{lines:?}");
    }

    #[test]
    fn a_fill_at_two_percent_or_under_is_no_reading_and_a_number_read_is_believed() {
        // w38's p38c, through `observe` in Hebrew, each script at 10 fps
        // and at 4 fps (the main loop's tick). The cursor's tip on a bar's
        // start reads a sliver — 0.3, or 0.67 then 0.33 (2 px, 1 px) — and
        // a cursor easing onto it reads like HP draining. At 03303a9: C1
        // (10 fps), C4, C6 and C7 were deaths said to a player at 85%, C6
        // and C7 after a false "פחות משני אחוז חיים"; and the MP tip was
        // "פחות משני אחוז מאנה". A fill at 2% or under is no reading now —
        // neither a value nor an empty bar — while a number read in the
        // game's font is believed as it comes, 0 and 1 too. `script(t)`:
        // HP and MP (None: none), whether HP was read in the font, EXP.
        type Script = dyn Fn(f64) -> (Option<f32>, f32, bool, f32);
        fn lerp(t: f64, t0: f64, t1: f64, a: f32, b: f32) -> f32 {
            a + (b - a) * ((t - t0) / (t1 - t0)).clamp(0.0, 1.0) as f32
        }
        let run = |fps: f64, secs: f64, script: &Script| {
            let mut c = Companion::seeded(Settings::default(), SEED);
            c.set_language(Some("he-IL"));
            c.settled(false);
            let mut lines: Vec<(f64, String)> = Vec::new();
            for f in 0..(secs * fps) as usize {
                let t = f as f64 / fps;
                let (hp, mp, read, exp) = script(t);
                let obs = Observation {
                    game: GameView::Seen("MapleStory".into()),
                    hp: hp.and_then(|p| gauge(p, read)),
                    mp: gauge(mp, false),
                    exp: gauge(exp, false),
                    level: Some(9),
                    name: None,
                    job: None,
                };
                for line in said(&c.observe(t, obs)) {
                    lines.push((t, line));
                }
            }
            (c.deaths, lines)
        };
        let sliver = |lines: &[(f64, String)]| lines.iter().any(|(_, l)| l.contains(LESS_THAN_TWO));
        // The cursor, HP really 85 all along (MP 80): no death, and no
        // "less than two percent".
        let cursor: [(&str, Box<Script>); 5] = [
            (
                "C1 slow sweep onto the start, parked",
                Box::new(|t| {
                    let hp = match t {
                        t if t < 5.0 => 85.0,
                        t if t < 6.0 => lerp(t, 5.0, 6.0, 85.0, 0.3),
                        t if t < 12.0 => 0.3,
                        _ => 85.0,
                    };
                    (Some(hp), 80.0, false, 40.0)
                }),
            ),
            (
                "C4 eases in: fast to 30, slow over the last 30, parked",
                Box::new(|t| {
                    let hp = match t {
                        t if t < 5.0 => 85.0,
                        t if t < 5.1 => 30.0,
                        t if t < 5.7 => lerp(t, 5.1, 5.7, 30.0, 0.3),
                        t if t < 11.7 => 0.3,
                        _ => 85.0,
                    };
                    (Some(hp), 80.0, false, 40.0)
                }),
            ),
            (
                "C6 tip 0.67 then 0.33, twice",
                Box::new(|t| {
                    let hp = match t {
                        t if t < 5.0 => 85.0,
                        t if t < 5.6 => 0.67,
                        t if t < 8.6 => 0.33,
                        t if t < 9.2 => 0.67,
                        t if t < 12.2 => 0.33,
                        _ => 85.0,
                    };
                    (Some(hp), 80.0, false, 40.0)
                }),
            ),
            (
                "C7 tip 0.67 for 6 s, then 0.33",
                Box::new(|t| {
                    let hp = match t {
                        t if t < 5.0 => 85.0,
                        t if t < 11.0 => 0.67,
                        t if t < 15.0 => 0.33,
                        _ => 85.0,
                    };
                    (Some(hp), 80.0, false, 40.0)
                }),
            ),
            (
                "MP: the tip on the MP bar, 0.67 for 6 s, then 1.5",
                Box::new(|t| {
                    let mp = match t {
                        t if t < 5.0 => 80.0,
                        t if t < 11.0 => 0.67,
                        t if t < 15.0 => 1.5,
                        _ => 80.0,
                    };
                    (Some(85.0), mp, false, 40.0)
                }),
            ),
        ];
        for (label, script) in &cursor {
            for fps in [10.0, 4.0] {
                let (deaths, lines) = run(fps, 20.0, script.as_ref());
                assert_eq!(deaths, 0, "{label} at {fps} fps: {lines:?}");
                assert!(!sliver(&lines), "{label} at {fps} fps: {lines:?}");
            }
        }
        // A number read in the game's font is believed as it comes: 0 is
        // a death at once (R6), 1 is "less than two percent".
        for fps in [10.0, 4.0] {
            let (deaths, lines) = run(fps, 20.0, &|t| {
                let hp = if t < 5.0 {
                    80.0
                } else if t < 13.0 {
                    0.0
                } else {
                    100.0
                };
                (Some(hp), 80.0, true, 40.0)
            });
            assert_eq!(deaths, 1, "R6 at {fps} fps: {lines:?}");
            let (deaths, lines) = run(fps, 20.0, &|t| {
                let hp = if t < 5.0 {
                    80.0
                } else if t < 13.0 {
                    1.0
                } else {
                    80.0
                };
                (Some(hp), 80.0, true, 40.0)
            });
            assert_eq!(deaths, 0, "read 1 at {fps} fps: {lines:?}");
            assert!(sliver(&lines), "read 1 at {fps} fps: {lines:?}");
        }
        // What this costs: a death seen only in the fill — the bar's 0, with
        // EXP lost (R3), the way down through the red (R4) or HP steady
        // under the mark before it (R7) — is no longer counted (each was at
        // 03303a9). A fill at zero is the cursor's tip as often as a death,
        // and the learned sight gives no reading at all for an empty bar.
        // (The next round's sign: a fall seen, then no reading held ≥ 2 s.)
        let fill_deaths: [(&str, Box<Script>); 3] = [
            (
                "R3 one-shot from 80, EXP 40 -> 39",
                Box::new(|t| {
                    let hp = if t < 5.0 {
                        80.0
                    } else if t < 13.0 {
                        0.0
                    } else {
                        100.0
                    };
                    (Some(hp), 80.0, false, if t < 5.0 { 40.0 } else { 39.0 })
                }),
            ),
            (
                "R4 through the red over 1.5 s",
                Box::new(|t| {
                    let hp = match t {
                        t if t < 5.0 => 80.0,
                        t if t < 6.5 => lerp(t, 5.0, 6.5, 80.0, 0.0),
                        t if t < 14.5 => 0.0,
                        _ => 100.0,
                    };
                    (Some(hp), 80.0, false, 40.0)
                }),
            ),
            (
                "R7 steady 25, then 0",
                Box::new(|t| {
                    let hp = if t < 5.0 {
                        25.0
                    } else if t < 13.0 {
                        0.0
                    } else {
                        100.0
                    };
                    (Some(hp), 80.0, false, 40.0)
                }),
            ),
        ];
        for (label, script) in &fill_deaths {
            let (deaths, lines) = run(10.0, 20.0, script.as_ref());
            assert_eq!(deaths, 0, "{label}: {lines:?}");
            assert!(!sliver(&lines), "{label}: {lines:?}");
        }
    }

    #[test]
    fn an_empty_fill_is_no_death_and_a_zero_read_in_the_font_is() {
        // The learned sight gives no reading for an empty bar, so the fill
        // read at zero is a sliver, and the everyday sliver is his cursor
        // parked on the bar's start (w32's D1, p21 A6/A7). At 72c97ff: on a
        // steady bar, two seconds of it was a death — said ("Your HP hit
        // zero" at 85%), counted in the stats, told to the coach; and
        // through a hold, a cursor coming in rests a moment (12, 12; 10,
        // 10) before it is found to swing, and that "steady HP under the
        // mark" passed for the second sign. Ten frames a second; `script`
        // gives HP, whether it was read in the game's font, and EXP.
        type Script<'a> = &'a dyn Fn(usize) -> (f32, bool, f32);
        let swing = [
            100.0, 3.0, 100.0, 46.0, 9.0, 0.0, 100.0, 28.0, 0.0, 0.0, 3.0, 100.0,
        ];
        let is_death = |l: &str| from(lines::DEATH, l) || from(lines::DEATH_WARNED, l);
        let run = |settings: Settings, frames: usize, script: Script| {
            let mut c = Companion::seeded(settings, SEED);
            let mut lines: Vec<(f64, String)> = Vec::new();
            for i in 0..frames {
                let t = i as f64 * 0.1;
                let (hp, read, exp) = script(i);
                let mut obs = frame(hp, 90.0, exp);
                obs.hp = gauge(hp, read);
                for line in dealt(&c.observe(t, obs)) {
                    lines.push((t, line));
                }
            }
            let deaths: Vec<(f64, String)> =
                lines.iter().filter(|(_, l)| is_death(l)).cloned().collect();
            assert_eq!(deaths.len() as u32, c.deaths, "{lines:?}");
            (deaths, lines)
        };
        let run_with = run;
        let run = |frames: usize, script: Script| run_with(Settings::default(), frames, script);
        // Parked on a steady bar: 85%, the cursor's tip at the bar's start
        // for 3 s, then off. (Fails at 72c97ff: a death at 7.0 s.)
        let (deaths, lines) = run(200, &|i| match i {
            50..80 => (0.3, false, 40.0),
            _ => (85.0, false, 40.0),
        });
        assert!(deaths.is_empty(), "{lines:?}");
        // However long it stays (a mouse left there while he grinds on the
        // keyboard), and at 0.0 (a tip just before the start reads 0).
        let (deaths, lines) = run(600, &|i| match i {
            50..350 => (0.0, false, 40.0),
            _ => (80.0, false, 40.0),
        });
        assert!(deaths.is_empty(), "{lines:?}");
        // Through a hold, the cursor coming in resting at 12% and 10% — the
        // swing's own first frames, HP really at 85 — then parked. (Fails
        // at 72c97ff: a death at 16.9 s, from the steady 10.)
        let coming_in = [
            85.0, 60.0, 40.0, 20.0, 12.0, 12.0, 40.0, 70.0, 85.0, 50.0, 10.0, 10.0, 60.0, 85.0,
            30.0, 5.0, 60.0, 85.0,
        ];
        let (deaths, lines) = run(300, &|i| match i {
            0..50 => (85.0, false, 40.0),
            50..68 => (coming_in[i - 50], false, 40.0),
            68..150 => (swing[i % swing.len()], false, 40.0),
            150..180 => (0.3, false, 40.0),
            _ => (85.0, false, 40.0),
        });
        assert!(deaths.is_empty(), "{lines:?}");
        // The cursor coming in, found to swing, and parked at once: its
        // readings under the mark in the three seconds before the zero
        // were the swing's, no fall. (Fails at 72c97ff, as above.)
        let quick = [60.0, 20.0, 12.0, 12.0, 70.0, 10.0, 10.0, 80.0, 15.0];
        let (deaths, lines) = run(200, &|i| match i {
            50..59 => (quick[i - 50], false, 40.0),
            59..89 => (0.3, false, 40.0),
            _ => (85.0, false, 40.0),
        });
        assert!(deaths.is_empty(), "{lines:?}");
        // A death: the 0 read in the font, through a hold, at once.
        let (deaths, lines) = run(300, &|i| match i {
            0..50 => (70.0, false, 40.0),
            50..150 => (swing[i % swing.len()], false, 40.0),
            150..230 => (0.0, true, 40.0),
            _ => (100.0, false, 40.0),
        });
        assert_eq!(deaths.len(), 1, "{lines:?}");
        assert!(deaths[0].0 < 15.3, "{deaths:?}");
        // The fill's 0 with a second sign — EXP lost (40 → 39) through a
        // hold; HP read steady at 20 before the swing; the way down seen
        // on a steady bar (85, 45, 25, 12), the warnings on or off — was a
        // death at 03303a9, and is none now: a fill at 2% or under is no
        // reading (see `SLIVER`). Missed, as every death on the learned
        // sight with the number unread already was; and from 70% straight
        // to an empty fill, with no EXP to lose, as before (p21 A1).
        let exp_lost = |i: usize| -> (f32, bool, f32) {
            match i {
                0..50 => (70.0, false, 40.0),
                50..150 => (swing[i % swing.len()], false, 40.0),
                150..230 => (0.0, false, 39.0),
                _ => (100.0, false, 39.0),
            }
        };
        let steady_under = |i: usize| -> (f32, bool, f32) {
            match i {
                0..50 => (20.0, false, 40.0),
                50..150 => (swing[i % swing.len()], false, 40.0),
                150..230 => (0.0, false, 40.0),
                _ => (100.0, false, 40.0),
            }
        };
        let fall = |i: usize| -> (f32, bool, f32) {
            match i {
                0..50 => (85.0, false, 40.0),
                50 => (45.0, false, 40.0),
                51 => (25.0, false, 40.0),
                52 => (12.0, false, 40.0),
                53..133 => (0.0, false, 40.0),
                _ => (100.0, false, 40.0),
            }
        };
        let straight = |i: usize| -> (f32, bool, f32) {
            match i {
                50..130 => (0.0, false, 40.0),
                0..50 => (70.0, false, 40.0),
                _ => (100.0, false, 40.0),
            }
        };
        for script in [&exp_lost as Script, &steady_under, &fall, &straight] {
            let (deaths, lines) = run(300, script);
            assert!(deaths.is_empty(), "{lines:?}");
        }
        let off = Settings {
            hp_low: 0.0,
            ..Settings::default()
        };
        let (deaths, lines) = run_with(off, 200, &fall);
        assert!(deaths.is_empty(), "{lines:?}");
    }

    #[test]
    fn an_empty_mp_fill_is_no_reading_unless_the_way_down_was_seen() {
        // D1's MP twin (w34): his cursor parked on the MP bar's start reads
        // as an empty MP bar. At b28cdfb, 0.3 on a steady 80 for five
        // minutes was four MP warnings in two minutes, the hold note, and
        // "There you go." when the cursor moved off — taken for a potion
        // at last. An empty fill — any fill at 2% or under (see `SLIVER`;
        // 0.5 until round K) — counts only when the way down was seen —
        // two readings over 2% and under the mark in the 3 s before it,
        // not the swing's — else it is no reading. (HP's never counts.)
        // Ten frames a second; `script` gives MP and whether it
        // was read in the game's font. Returns what was said after the
        // first frame (when, the line), and the companion.
        type Script<'a> = &'a dyn Fn(usize) -> (f32, bool);
        let run = |settings: Settings, frames: usize, script: Script| {
            let mut c = Companion::seeded(settings, SEED);
            let mut lines: Vec<(f64, String)> = Vec::new();
            for i in 0..frames {
                let t = i as f64 * 0.1;
                let (mp, read) = script(i);
                let mut obs = frame(85.0, mp, 40.0);
                obs.mp = gauge(mp, read);
                let actions = c.observe(t, obs);
                if i > 0 {
                    lines.extend(said(&actions).into_iter().map(|l| (t, l)));
                }
            }
            (lines, c)
        };
        let off = Settings {
            mp_low: 0.0,
            ..Settings::default()
        };
        let run_off = |frames: usize, script: Script| run(off.clone(), frames, script);
        // (MP warnings asked for at the old usual 15%: none by default.)
        let asked = Settings {
            mp_low: 15.0,
            ..Settings::default()
        };
        let run = |frames: usize, script: Script| run(asked.clone(), frames, script);
        let mp_low = |lines: &[(f64, String)]| -> Vec<f64> {
            lines
                .iter()
                .filter(|(_, l)| {
                    [lines::MP_LOW, lines::MP_LOW_AGAIN, lines::MP_LOW_ASK]
                        .iter()
                        .any(|deck| from(*deck, l))
                })
                .map(|(t, _)| *t)
                .collect()
        };
        // Parked five minutes on a steady 80, then off: not a word, no
        // potion. (Fails at b28cdfb: the four warnings, the hold note and
        // "There you go." at 305.1 s.)
        let (lines, c) = run(3200, &|i| match i {
            50..3050 => (0.3, false),
            _ => (80.0, false),
        });
        assert!(lines.is_empty(), "{lines:?}");
        assert_eq!((c.tally().mp_low, c.tally().potted), (0, 0));
        assert_eq!(c.potion_at, f64::NEG_INFINITY, "moving off is no potion");
        // Parked 3 s, at 0.0 (a tip just before the bar's start), and with
        // the MP warnings off: the same.
        for (lines, c) in [
            run(200, &|i| match i {
                50..80 => (0.3, false),
                _ => (80.0, false),
            }),
            run(400, &|i| match i {
                50..350 => (0.0, false),
                _ => (80.0, false),
            }),
            run_off(400, &|i| match i {
                50..350 => (0.3, false),
                _ => (80.0, false),
            }),
        ] {
            assert!(lines.is_empty(), "{lines:?}");
            assert_eq!(c.potion_at, f64::NEG_INFINITY, "{lines:?}");
        }
        // The cursor coming in, found to swing, and parked at once: its
        // readings under the mark in the 3 s before the empty bar were the
        // swing's, no way down — not warned when the hold ends. (Fails at
        // b28cdfb: a warning 0.6 s after the hold.)
        let quick = [60.0, 20.0, 12.0, 12.0, 70.0, 10.0, 10.0, 80.0, 15.0];
        let (lines, _) = run(600, &|i| match i {
            50..59 => (quick[i - 50], false),
            59..559 => (0.3, false),
            _ => (80.0, false),
        });
        assert!(mp_low(&lines).is_empty(), "{lines:?}");
        // A real drain, 80 to empty over 4 s (20 points a second: under the
        // mark for less than the low line's hold before it reads 2% or
        // under), empty 30 s, then a potion: warned — the empty bar after
        // the way down is believed (and not said again: an empty bar that
        // stays empty is no new fall) — and the potion seen: it answers the
        // line. (w39's M4; not warned at all while every fill at 2% or
        // under was no reading.)
        let drain = |i: usize| match i {
            0..50 => (80.0, false),
            50..90 => ((80.0 - (i - 50) as f32 * 2.0).max(0.3), false),
            90..390 => (0.3, false),
            _ => (80.0, false),
        };
        let (lines, c) = run(500, &drain);
        let warned = mp_low(&lines);
        assert!((1..=2).contains(&warned.len()), "{lines:?}");
        assert!((8.8..9.1).contains(&warned[0]), "{lines:?}");
        assert!(warned.iter().all(|t| *t < 39.0), "{lines:?}");
        assert_eq!(c.tally().potted, 1, "{lines:?}");
        assert!((39.0..39.3).contains(&c.potion_at), "{}", c.potion_at);
        // With the MP warnings off, the same drain still makes the empty
        // bar a reading (the mark is the default's then): the potion seen.
        let (lines, c) = run_off(500, &drain);
        assert!(lines.is_empty(), "{lines:?}");
        assert!((39.0..39.3).contains(&c.potion_at), "{}", c.potion_at);
        // A fast one — 80, 40, 12, 6, empty — is a way down too (w39's M5).
        let (lines, _) = run(300, &|i| match i {
            0..50 => (80.0, false),
            50 => (40.0, false),
            51 => (12.0, false),
            52 => (6.0, false),
            53..153 => (0.3, false),
            _ => (80.0, false),
        });
        let warned = mp_low(&lines);
        assert!(!warned.is_empty() && warned[0] < 6.0, "{lines:?}");
        // A tip of 2 px then 1 px (0.67, then 0.33) on a steady 80 is no
        // way down: no reading, no line, and moving off is no potion.
        let (lines, c) = run(300, &|i| match i {
            50..110 => (0.67, false),
            110..150 => (0.33, false),
            _ => (80.0, false),
        });
        assert!(lines.is_empty(), "{lines:?}");
        assert_eq!(c.potion_at, f64::NEG_INFINITY, "{lines:?}");
        // A 0 read in the game's font is believed at once.
        let (lines, _) = run(300, &|i| match i {
            50..150 => (0.0, true),
            _ => (80.0, true),
        });
        let warned = mp_low(&lines);
        assert!(!warned.is_empty() && warned[0] < 5.8, "{lines:?}");
    }

    #[test]
    fn numbers_read_off_the_screen_are_trusted_through_a_fast_fight() {
        // HP read from the printed number: every 2.5 s a hit takes it from
        // 100 to 40 in under a second and a potion puts it back — three
        // turns of 60 points in 6 s, which from a bar's fill would be a
        // misread. Thirty seconds of it, then HP at 15: warned at once,
        // and never a word about the readings jumping around (one night
        // that held the HP warnings — the death too — through the fight).
        let fight = |t: f64| -> f32 {
            let phase = t % 2.5;
            if phase < 0.8 {
                100.0 - (phase / 0.8 * 60.0) as f32
            } else if phase < 1.4 {
                40.0
            } else {
                100.0
            }
        };
        let mut c = Companion::seeded(Settings::default(), SEED);
        let mut lines: Vec<(f64, String)> = Vec::new();
        for i in 0..300 {
            let t = i as f64 * 0.1;
            for line in said(&c.observe(t, read(fight(t)))) {
                lines.push((t, line));
            }
        }
        for i in 300..320 {
            let t = i as f64 * 0.1;
            for line in said(&c.observe(t, read(15.0))) {
                lines.push((t, line));
            }
        }
        lines.retain(|(_, l)| !from(lines::SEEN, l));
        assert_eq!(lines.len(), 2, "{lines:?}");
        assert!(
            lines[0].0 < 1.0 && from(lines::BEATING, &lines[0].1),
            "{lines:?}"
        );
        assert!(from(lines::HP_LOW, &lines[1].1), "{lines:?}");
        // (Read off the number: no "about".)
        assert!(lines[1].1.contains(" 15 percent"), "{lines:?}");
        assert!(lines[1].0 <= 30.0 + LOW_HOLD_SECS + 0.3, "{lines:?}");
        // A death read in the thick of it is announced at once.
        let mut c = Companion::seeded(Settings::default(), SEED);
        for i in 0..100 {
            let t = i as f64 * 0.1;
            c.observe(t, read(fight(t)));
        }
        let mut dead = Vec::new();
        for i in 100..103 {
            dead.extend(said(&c.observe(i as f64 * 0.1, read(0.0))));
        }
        assert_eq!(dead.len(), 1, "{dead:?}");
        // (One it warned of: the beating was shouted in this fight.)
        assert_eq!(
            dead[0],
            Attitude::Friendly.lines(lines::DEATH_WARNED)[0],
            "{dead:?}"
        );
        assert!(c.dead());
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
        // HP hovering at the mark for twenty-five minutes and nothing done
        // about it (the player is away): half a minute at 25%, a minute at
        // 31% (no potion: six points), again and again — a new fight each
        // time, each with its line (a bar that stays put is said once a
        // fight: see `a_steady_low_hp_is_said_once…`). Four lines with no
        // sign of life, then, where the fifth would come, one line saying
        // the rest will wait — to an empty room: not a word from the
        // player all session — then nothing for ten minutes; then two
        // more, and quiet again. (Six, and a pet's death was six lines in
        // six minutes to a room that may be empty.)
        let hp = |t: f64| if t % 91.0 < 30.0 { 25.0 } else { 31.0 };
        let mut c = Companion::seeded(Settings::default(), SEED);
        let mut lines: Vec<(f64, String)> = Vec::new();
        for i in 0..15_000 {
            let t = i as f64 * 0.1;
            for line in said(&c.observe(t, frame(hp(t), 90.0, 10.0))) {
                if !from(lines::SEEN, &line) {
                    lines.push((t, line));
                }
            }
        }
        let texts: Vec<&str> = lines.iter().map(|(_, l)| l.as_str()).collect();
        assert_eq!(texts.len(), 7, "{lines:?}");
        for text in &texts[..4] {
            assert!(from(lines::HP_LOW, text), "{texts:?}");
        }
        // The first time, the hold is explained in full.
        assert_eq!(variant(lines::HOLD_AWAY, texts[4]), Some(0), "{texts:?}");
        // Four warnings a fight apart, the hold where the fifth would come,
        // and it ends ten minutes later.
        assert!((lines[3].0 - lines[0].0 - 273.0).abs() < 0.5, "{lines:?}");
        assert!((lines[4].0 - lines[3].0 - 91.0).abs() < 0.5, "{lines:?}");
        assert!(lines[5].0 - lines[4].0 >= 600.0, "{lines:?}");
        assert!(from(lines::HP_LOW, texts[5]) && from(lines::HP_LOW, texts[6]));
        // (The hold is not announced again so soon.)
        assert!(c.alerts_held(1500.0));
        // The player says something: the warnings come again.
        c.player_spoke(1500.0);
        assert!(!c.alerts_held(1500.0));
        let mut after = Vec::new();
        for i in 0..500 {
            let t = 1500.0 + i as f64 * 0.1;
            after.extend(said(&c.observe(t, frame(hp(t), 90.0, 10.0))));
        }
        assert_eq!(after.len(), 1, "{after:?}");
        // A card that counts the warnings counts four.
        for attitude in Attitude::ALL {
            let cards = [lines::HOLD_AWAY, lines::HOLD_HERE].map(|deck| attitude.lines(deck));
            for card in cards.concat() {
                let lower = card.to_lowercase();
                let counts: Vec<&str> = [
                    "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten",
                ]
                .into_iter()
                .filter(|w| lower.split(|c: char| !c.is_alphabetic()).any(|x| x == *w))
                .collect();
                assert!(
                    counts.is_empty() || counts == ["four"],
                    "{}: {card:?} counts {counts:?}",
                    attitude.word()
                );
            }
        }
    }

    #[test]
    fn a_bar_read_low_while_exp_comes_in_is_not_believed() {
        // HP read low for an hour, a little lower at each line (29, 23,
        // 17, 11, then 5: a bar that stays put is said once, see
        // `a_steady_low_hp…`), while EXP rises 0.3% a minute: the player
        // is grinding and fine, and the bar is read wrong. Four unanswered
        // lines (0.6, 12.6, 42.6, 102.6 s), then, where the fifth would
        // come, one note in its own voice that it is holding the HP
        // warnings — and nothing more all hour. (The hold for warnings
        // nobody answers never engaged: EXP answered them, every minute.
        // 34 lines in the hour; six hours of it was ~200.)
        type Lines = Vec<(f64, String)>;
        let step = |t: f64| match t {
            t if t < 12.0 => 29.0,
            t if t < 42.0 => 23.0,
            t if t < 102.0 => 17.0,
            t if t < 222.0 => 11.0,
            _ => 5.0,
        };
        let hour = |_hp: f32, exp_per_min: f32| -> (Lines, Lines) {
            let mut c = Companion::seeded(Settings::default(), SEED);
            let mut warnings: Lines = Vec::new();
            let mut notes: Lines = Vec::new();
            for i in 0..36_000 {
                let t = i as f64 * 0.1;
                let mut obs = read(step(t));
                obs.exp = gauge(10.0 + (t / 60.0) as f32 * exp_per_min, true);
                for action in c.observe(t, obs) {
                    let Action::Say(say) = action else { continue };
                    match say.kind {
                        Kind::Warning => warnings.push((t, say.text)),
                        Kind::Info if !from(lines::SEEN, &say.text) => {
                            assert!(say.speak, "{say:?}");
                            notes.push((t, say.text));
                        }
                        _ => {}
                    }
                }
            }
            (warnings, notes)
        };
        let (warnings, notes) = hour(20.0, 0.3);
        assert_eq!(warnings.len(), 4, "{warnings:?}");
        for ((at, _), want) in warnings.iter().zip([0.6, 12.6, 42.6, 102.6]) {
            assert!((at - want).abs() < 0.15, "{warnings:?}");
        }
        assert_eq!(notes.len(), 1, "{notes:?}");
        assert!((notes[0].0 - 222.6).abs() < 0.15, "{notes:?}");
        assert_eq!(
            variant(lines::MISREAD, &notes[0].1.replace("HP", "{}")),
            Some(0),
            "{notes:?}"
        );
        assert!(!notes[0].1.contains("MP"), "{notes:?}");
        // EXP flat: nobody is playing (or nothing answers), and the old
        // rule stands — the cadence to four lines, then the hold where the
        // fifth would come (the two rules count to four, and part there:
        // EXP gained since the first line is a bar not believed, none is
        // a room not answering). (At 5% it falls no further: nothing more.)
        let (warnings, notes) = hour(20.0, 0.0);
        assert_eq!(warnings.len(), 4, "{warnings:?}");
        assert!((warnings[3].0 - 102.6).abs() < 0.15, "{warnings:?}");
        assert!((notes[0].0 - 222.6).abs() < 0.15, "{notes:?}");
        assert!(notes.iter().all(|(_, n)| hold_note(n)), "{notes:?}");
        // Believed again once it has read above the mark for three
        // seconds (2.9 s is not enough — and the drop back to 20 after
        // those is no beating either: the bar is not believed): worn down
        // slowly to 20 after that, it is a new fight, said at once.
        let mut c = Companion::seeded(Settings::default(), SEED);
        let mut lines: Vec<(f64, String)> = Vec::new();
        for i in 0..3300 {
            let t = i as f64 * 0.1;
            let hp = match i {
                0..3000 => step(t),
                3000..3029 => 90.0,
                3029..3100 => 20.0,
                3100..3131 => 90.0,
                _ => (90.0 - (i - 3131) as f32 * 0.7).max(20.0),
            };
            let mut obs = read(hp);
            obs.exp = gauge(10.0 + (t / 60.0) as f32 * 0.3, true);
            for line in dealt(&c.observe(t, obs)) {
                lines.push((t, line));
            }
        }
        assert_eq!(lines.len(), 5, "{lines:?}");
        assert!((lines[4].0 - 322.3).abs() < 0.15, "{lines:?}");
        assert!(from(lines::HP_LOW, &lines[4].1), "{lines:?}");
        // MP, the same shape (warned under 50%, asked for: none by
        // default).
        let mut c = Companion::seeded(
            Settings {
                mp_low: 50.0,
                ..Settings::default()
            },
            SEED,
        );
        let mut warnings = Vec::new();
        let mut notes = Vec::new();
        for i in 0..6000 {
            let t = i as f64 * 0.1;
            let mut obs = read(90.0);
            obs.mp = gauge(step(t) + 16.0, true);
            obs.exp = gauge(10.0 + (t / 60.0) as f32 * 0.3, true);
            for action in c.observe(t, obs) {
                let Action::Say(say) = action else { continue };
                match say.kind {
                    Kind::Warning => warnings.push(say.text),
                    Kind::Info if from(lines::MISREAD, &say.text.replace("MP", "{}")) => {
                        notes.push(say.text);
                    }
                    _ => {}
                }
            }
        }
        assert_eq!(warnings.len(), 4, "{warnings:?}");
        assert_eq!(notes.len(), 1, "{notes:?}");
    }

    #[test]
    fn a_death_and_a_level_up_pass_through_a_hold() {
        // HP read low for eight minutes, lower at each line, and nothing
        // done about it: four warnings, then the hold. A death in it is
        // said all the same (the coach counts it either way, and would
        // speak of a third death the player never heard of), and so is a
        // level-up; the warnings stay held.
        let step = |t: f64| match t {
            t if t < 12.0 => 29.0,
            t if t < 42.0 => 23.0,
            t if t < 102.0 => 17.0,
            t if t < 222.0 => 11.0,
            _ => 5.0,
        };
        let mut c = Companion::seeded(Settings::default(), SEED);
        let mut lines = Vec::new();
        for i in 0..4800 {
            let t = i as f64 * 0.1;
            lines.extend(alerts(&c.observe(t, read(step(t)))));
        }
        assert_eq!(lines.len(), 4, "{lines:?}");
        assert!(c.alerts_held(480.0));
        let mut death = Vec::new();
        for i in 0..3 {
            death.extend(alerts(&c.observe(500.0 + i as f64 * 0.1, read(0.0))));
        }
        assert_eq!(death.len(), 1, "{death:?}");
        // (A warned death: it said to pot four times in this fight.)
        assert!(from(lines::DEATH_WARNED, &death[0]), "{death:?}");
        assert_eq!(c.so_far().deaths, 1);
        assert!(c.alerts_held(501.0));
        // Revived, still low (12%: less than a potion's worth over the
        // last line's 5%): the warnings are still held…
        let mut after = Vec::new();
        for i in 0..600 {
            after.extend(alerts(&c.observe(505.0 + i as f64 * 0.1, read(12.0))));
        }
        assert!(after.is_empty(), "{after:?}");
        assert!(c.alerts_held(565.0));
        // …and a level-up is said through it.
        let mut up = read(12.0);
        up.level = Some(166);
        let mut cheered = Vec::new();
        for i in 0..40 {
            cheered.extend(alerts(&c.observe(570.0 + i as f64 * 0.1, up.clone())));
        }
        assert_eq!(cheered, ["Level up! You're level 166."]);
    }

    #[test]
    fn it_says_it_twice_then_asks_then_stops_in_words_for_whoever_is_there() {
        // The evening's 82:00: the pet dies a minute and a half after the
        // player said "ok, I'm back" — HP from 100 to 25 over 1.5 s, then
        // lower and lower (19, 13, 7: hits, no potion), EXP flat. The
        // beating; the low line 5 s on; said again at the next fall; then
        // asked about; and where the next would come it stops, in words for
        // someone who is there. (It was four orders in 47 s, then "Just
        // talk to me when you're back.") A potion at last, six minutes on:
        // one word that it saw, and nothing is held any more. (Had HP
        // stayed at 25 it was the beating and the low line, and quiet: see
        // `a_steady_low_hp_is_said_once…`.)
        type Lines = Vec<(f64, Kind, String)>;
        let falling = |t: f64| -> f32 {
            if t < 1001.5 {
                100.0 - (t - 1000.0) as f32 / 1.5 * 75.0
            } else if t < 1017.0 {
                25.0
            } else if t < 1047.0 {
                19.0
            } else if t < 1107.0 {
                13.0
            } else {
                7.0
            }
        };
        let evening = |word: f64, potion: f64, end: f64| -> (Lines, Companion) {
            let mut c = Companion::seeded(Settings::default(), SEED);
            let mut lines: Lines = Vec::new();
            for i in 0..(end * 10.0) as usize {
                let t = i as f64 * 0.1;
                if (t - word).abs() < 0.05 {
                    c.player_spoke(t);
                }
                let hp = if t < 1000.0 || t >= potion {
                    100.0
                } else {
                    falling(t)
                };
                let mut obs = read(hp);
                // (Grinding until the pet dies: EXP coming in, then flat.)
                obs.exp = gauge(10.0 + (t.min(1000.0) / 60.0) as f32 * 0.3, true);
                for action in c.observe(t, obs) {
                    if let Action::Say(say) = action
                        && say.kind != Kind::Heard
                        && t >= 1000.0
                    {
                        lines.push((t, say.kind, say.text));
                    }
                }
            }
            (lines, c)
        };
        let fight = |lines: &Lines| {
            let decks = [
                lines::BEATING,
                lines::HP_LOW,
                lines::HP_LOW_AGAIN,
                lines::HP_LOW_ASK,
            ];
            for ((at, kind, line), (deck, want)) in lines
                .iter()
                .zip(decks.into_iter().zip([1000.8, 1005.8, 1017.8, 1047.8]))
            {
                assert_eq!(*kind, Kind::Warning, "{lines:?}");
                assert!(from(deck, line), "{lines:?}");
                assert!((at - want).abs() < 0.15, "{lines:?}");
            }
            assert_eq!(lines[4].1, Kind::Info, "{lines:?}");
            assert!((lines[4].0 - 1107.8).abs() < 0.15, "{lines:?}");
        };
        let (lines, c) = evening(910.0, 1360.0, 1400.0);
        assert_eq!(lines.len(), 6, "{lines:?}");
        fight(&lines);
        assert!(from(lines::HOLD_HERE, &lines[4].2), "{lines:?}");
        let (at, kind, line) = &lines[5];
        assert!((at - 1360.1).abs() < 0.05, "{lines:?}");
        assert_eq!(*kind, Kind::Info, "{lines:?}");
        assert!(from(lines::POTTED_AT_LAST, line), "{lines:?}");
        assert!(!c.alerts_held(1361.0));
        // HP coming back on its own over the hold (regen, half a point a
        // second from 1300): the hold lifts once it is a potion's worth up,
        // without a word — no potion was seen, and "There it is." would
        // claim one.
        let mut c = Companion::seeded(Settings::default(), SEED);
        let mut regen: Lines = Vec::new();
        for i in 0..14_000usize {
            let t = i as f64 * 0.1;
            if (t - 910.0).abs() < 0.05 {
                c.player_spoke(t);
            }
            let hp = if t < 1000.0 {
                100.0
            } else if t < 1300.0 {
                falling(t)
            } else {
                (7.0 + (t - 1300.0) as f32 / 2.0).min(100.0)
            };
            let mut obs = read(hp);
            obs.exp = gauge(10.0 + (t.min(1000.0) / 60.0) as f32 * 0.3, true);
            for action in c.observe(t, obs) {
                if let Action::Say(say) = action
                    && say.kind != Kind::Heard
                    && t >= 1000.0
                {
                    regen.push((t, say.kind, say.text));
                }
            }
        }
        fight(&regen);
        assert_eq!(regen.len(), 5, "{regen:?}");
        assert!(!c.alerts_held(1330.0));
        // No word for fifteen minutes before: as far as it knows the room
        // is empty, and the hold says so. When it ends, nothing more: HP
        // stays where it is (no new fall). A potion then, the note sixteen
        // minutes old: not a word.
        let (lines, mut c) = evening(100.0, f64::INFINITY, 2100.0);
        fight(&lines);
        assert_eq!(lines.len(), 5, "{lines:?}");
        assert!(from(lines::HOLD_AWAY, &lines[4].2), "{lines:?}");
        assert_eq!(lines.iter().filter(|l| hold_note(&l.2)).count(), 1);
        let mut potted = Vec::new();
        for i in 0..20 {
            potted.extend(said(&c.observe(2100.0 + i as f64 * 0.1, read(100.0))));
        }
        assert!(potted.is_empty(), "{potted:?}");
        assert!(!c.alerts_held(2102.0));
        // The words: no card of the hold for someone there says they are
        // away or asks them back; the word at the potion is a word.
        for attitude in Attitude::ALL {
            for card in attitude.lines(lines::HOLD_HERE) {
                let lower = card.to_lowercase();
                for away in [
                    "away",
                    "back",
                    "afk",
                    "nobody",
                    "anyone",
                    "wall",
                    "where are you",
                    "wherever",
                    "say something",
                    "talk to me",
                    "speak",
                    "hear from you",
                ] {
                    assert!(!lower.contains(away), "{card:?}");
                }
            }
            for card in attitude.lines(lines::POTTED_AT_LAST) {
                assert!(card.split_whitespace().count() <= 8, "{card:?}");
            }
        }
    }

    #[test]
    fn a_potion_after_a_warning_is_an_answer() {
        // Low, warned, potted; a new fight a minute and a half on, low
        // again, warned… twelve times: the player is plainly there, and
        // every warning is said (six with no potion after would have the
        // rest held).
        let mut c = Companion::seeded(Settings::default(), SEED);
        let mut t = 0.0;
        let mut count = 0;
        let alerts = |actions: &[Action]| {
            actions
                .iter()
                .filter(|a| matches!(a, Action::Say(s) if s.kind == Kind::Warning))
                .count()
        };
        for fight in 0..12 {
            t = fight as f64 * 90.0;
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
    fn a_mana_potion_is_a_sign_of_life() {
        // A boss: EXP flat until the kill, HP untouched at 90 (a ranged
        // class), MP drained a point a second from 60 every two minutes
        // and potted to 90 two seconds after each MP line. Half an hour:
        // every line said, no hold, no "still there?" — the potions are
        // the player, there. (It was four lines, then "No pot, no word, no
        // point. I'll wait." and twenty minutes in "You still there?")
        let still_there = |line: &str| from(lines::STILL_THERE, line);
        // (MP warnings asked for: none by default.)
        let mut c = Companion::seeded(
            Settings {
                mp_low: 15.0,
                ..Settings::default()
            },
            SEED,
        );
        let (mut warnings, mut notes, mut asked) = (Vec::new(), Vec::new(), Vec::new());
        let mut potted_at = f64::NEG_INFINITY;
        let mut last_line = f64::NEG_INFINITY;
        for i in 0..18_000 {
            let t = i as f64 * 0.1;
            let cycle = t % 120.0;
            let mut mp = (60.0 - cycle as f32).max(5.0);
            if t >= potted_at && t - cycle <= potted_at {
                mp = 90.0;
            }
            let mut obs = read(90.0);
            obs.mp = gauge(mp, true);
            for action in c.observe(t, obs) {
                let Action::Say(say) = action else { continue };
                match say.kind {
                    Kind::Warning => {
                        last_line = t;
                        warnings.push((t, say.text));
                    }
                    Kind::Info if hold_note(&say.text) => notes.push((t, say.text)),
                    Kind::Alert if still_there(&say.text) => asked.push((t, say.text)),
                    _ => {}
                }
            }
            if (t - last_line - 2.0).abs() < 0.05 {
                potted_at = t;
            }
        }
        assert!(notes.is_empty(), "{notes:?}");
        assert!(asked.is_empty(), "{asked:?}");
        assert_eq!(warnings.len(), 15, "{warnings:?}");
        for (k, (at, line)) in warnings.iter().enumerate() {
            assert!(
                (at - (45.7 + 120.0 * k as f64)).abs() < 0.15,
                "{warnings:?}"
            );
            assert!(from(lines::MP_LOW, line), "{warnings:?}");
        }
        // With no warning at all — MP spent and potted back above the
        // mark every two minutes — the potions are the game going on…
        let idle = |mp: &dyn Fn(f64) -> f32| -> Vec<f64> {
            let mut c = Companion::seeded(Settings::default(), SEED);
            let mut asked = Vec::new();
            for i in 0..18_000 {
                let t = i as f64 * 0.1;
                let mut obs = read(90.0);
                obs.mp = gauge(mp(t), true);
                for line in alerts(&c.observe(t, obs)) {
                    assert!(still_there(&line), "{t}: {line:?}");
                    asked.push(t);
                }
            }
            asked
        };
        let potting = |t: f64| {
            if t % 120.0 < 30.0 {
                70.0 - (t % 120.0) as f32
            } else {
                90.0
            }
        };
        assert!(idle(&potting).is_empty());
        // …and MP coming back by its slow regen, a point every three
        // seconds, is not: asked twenty minutes in.
        let regen = |t: f64| (20.0 + (t / 3.0).floor() as f32).min(80.0);
        let asked = idle(&regen);
        assert_eq!(asked.len(), 1, "{asked:?}");
        assert!((asked[0] - 1200.0).abs() < 0.2, "{asked:?}");
    }

    /// A grind in which every hit dips under the mark and a potion answers
    /// it: HP from 100 to `floor` over a second and a half, a second and a
    /// half there, potted back to 100; every 8 s.
    fn dipping(t: f64, floor: f32) -> f32 {
        let phase = t % 8.0;
        if phase < 1.5 {
            100.0 - (phase / 1.5) as f32 * (100.0 - floor)
        } else if phase < 3.0 {
            floor
        } else {
            100.0
        }
    }

    #[test]
    fn a_low_warning_a_potion_answers_is_said_once_per_fight() {
        // Ten minutes of hit → 25% → pot, every 8 s, the numbers read: one
        // night this was 75 "pot now" lines in seven wordings — the nag
        // the beating fix was for, one rule down. The first hit is called
        // at once; the warning comes once; the potions answer it, and the
        // next hit is their business. The same at the 50% mark the
        // warning moves to after deaths, with dips to 45.
        for (low, floor) in [(30.0, 25.0), (50.0, 45.0)] {
            let mut c = Companion::seeded(
                Settings {
                    hp_low: low,
                    ..Settings::default()
                },
                SEED,
            );
            let mut lines: Vec<(f64, String)> = Vec::new();
            for i in 0..6000 {
                let t = i as f64 * 0.1;
                for line in alerts(&c.observe(t, read(dipping(t, floor)))) {
                    lines.push((t, line));
                }
            }
            assert!(lines.len() <= 3, "{low}: {lines:?}");
            assert!(lines[0].0 < 3.0, "{low}: {lines:?}");
            assert!(
                lines.iter().any(|(_, l)| from(lines::HP_LOW, l)),
                "{low}: {lines:?}"
            );
            assert!(!c.alerts_held(600.0));
            // Lower than it was at the last line — a hit to 12 — is one
            // more, at once.
            let mut lower = Vec::new();
            for i in 6000..6030 {
                let t = i as f64 * 0.1;
                let hp = if t < 601.0 { 100.0 } else { 12.0 };
                lower.extend(alerts(&c.observe(t, read(hp))));
            }
            assert_eq!(lower.len(), 1, "{low}: {lower:?}");
            assert!(says(lines::HP_LOW, &lower[0], "12 percent"), "{lower:?}");
        }
        // MP has the same shape (a pet pots MP too): the same rule (MP
        // warnings asked for: none by default).
        let mut c = Companion::seeded(
            Settings {
                mp_low: 15.0,
                ..Settings::default()
            },
            SEED,
        );
        let mut lines: Vec<(f64, String)> = Vec::new();
        for i in 0..6000 {
            let t = i as f64 * 0.1;
            let mut obs = read(90.0);
            obs.mp = Some(Gauge {
                percent: dipping(t, 10.0),
                current: None,
                max: None,
                read: true,
            });
            for line in alerts(&c.observe(t, obs)) {
                lines.push((t, line));
            }
        }
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert!(
            lines[0].0 < 3.0 && from(lines::MP_LOW, &lines[0].1),
            "{lines:?}"
        );
    }

    #[test]
    fn the_tally_counts_the_warnings_said_and_the_low_lines_a_potion_answered() {
        // For the session's stats, counts only: HP left at 20% gets its
        // line, then down to 14% a repeat 12 s on, and a potion answers the
        // repeat; then MP low (asked for), once, which nothing answers.
        let mut c = Companion::seeded(
            Settings {
                mp_low: 15.0,
                ..Settings::default()
            },
            SEED,
        );
        assert_eq!(c.tally(), Tally::default());
        let mut said = 0;
        for i in 0..130 {
            let hp = if i < 60 { 20.0 } else { 14.0 };
            said += alerts(&c.observe(i as f64 * 0.1, frame(hp, 90.0, 10.0))).len();
        }
        assert_eq!(said, 2);
        let tally = c.tally();
        assert_eq!((tally.hp_low, tally.low_lines, tally.potted), (2, 2, 0));
        for i in 130..150 {
            c.observe(i as f64 * 0.1, frame(90.0, 90.0, 10.0));
        }
        assert_eq!(c.tally().potted, 1, "{:?}", c.tally());
        for i in 150..170 {
            c.observe(i as f64 * 0.1, frame(90.0, 3.0, 10.0));
        }
        assert_eq!(
            c.tally(),
            Tally {
                beating: 0,
                hp_low: 2,
                mp_low: 1,
                low_lines: 3,
                potted: 1,
                deaths: c.so_far().deaths,
            }
        );
    }

    #[test]
    fn sixty_low_hp_warnings_in_a_night_are_never_the_same_line_twice_running() {
        // Two hours' grind: a fight every two minutes (a minute without HP
        // low ends one), worn down to 20% in each (slowly, so it is the
        // low warning and not a beating) and potted back (a potion answers
        // a warning, so none is held). One night this was the same line
        // 892 times. Sixty warnings, dealt like a deck: the one with the
        // number first, every line once before any comes again, none twice
        // in a row.
        let mut c = Companion::seeded(Settings::default(), SEED);
        let mut warnings = Vec::new();
        for fight in 0..60 {
            let t = fight as f64 * 120.0;
            for (dt, hp) in [
                (0.0, 90.0),
                (5.0, 70.0),
                (10.0, 50.0),
                (15.0, 35.0),
                (21.0, 20.0),
                (21.1, 20.0),
                (21.3, 20.0),
                (21.7, 20.0),
                (30.0, 90.0),
            ] {
                warnings.extend(dealt(&c.observe(t + dt, frame(hp, 90.0, 10.0))));
            }
        }
        assert_eq!(warnings.len(), 60, "{warnings:?}");
        dealt_like_a_deck(lines::HP_LOW, &warnings);
        assert!(warnings[0].contains("about 20 percent"), "{warnings:?}");
        // MP, the same (with a word from the player each fight too; the
        // mana potion alone would do, see `a_mana_potion_is_a_sign_of_life`;
        // MP warnings asked for: none by default).
        let mut c = Companion::seeded(
            Settings {
                mp_low: 15.0,
                ..Settings::default()
            },
            SEED,
        );
        let mut warnings = Vec::new();
        for fight in 0..60 {
            let t = fight as f64 * 120.0;
            for (dt, mp) in [
                (0.0, 90.0),
                (21.0, 10.0),
                (21.1, 10.0),
                (21.3, 10.0),
                (21.7, 10.0),
                (30.0, 90.0),
            ] {
                warnings.extend(dealt(&c.observe(t + dt, frame(90.0, mp, 10.0))));
            }
            c.player_spoke(t + 40.0);
        }
        assert_eq!(warnings.len(), 60, "{warnings:?}");
        dealt_like_a_deck(lines::MP_LOW, &warnings);
        assert!(warnings[0].contains("about 10 percent"), "{warnings:?}");
    }

    #[test]
    fn the_first_line_of_a_fight_never_says_still_or_again() {
        // Thirty fights two minutes apart, each worn down to 20% and
        // potted: one line each, every one the first of its fight, in
        // every voice — and not one says "still", "again" or "twice",
        // which presume a line before it. (A revive 6 s before had "Still
        // at 25 percent? Pot already." open the new life; one line per
        // fight makes nearly every line a first.) MP the same.
        let presumes = |line: &str| {
            let lower = line.to_lowercase();
            [
                "still",
                "again",
                "twice",
                "keep saying",
                "heard me",
                "i said",
                "always",
            ]
            .iter()
            .any(|w| lower.contains(w))
        };
        // Whether `line` is a card of `deck` in `attitude`'s voice.
        let card_of = |attitude: Attitude, deck: [&[&str]; 3], line: &str| {
            attitude.lines(deck).iter().any(|card| {
                let (head, tail) = card.split_once("{}").unwrap_or((card, ""));
                line.len() >= head.len() + tail.len()
                    && line.starts_with(head)
                    && line.ends_with(tail)
            })
        };
        for attitude in Attitude::ALL {
            // (MP warnings asked for: none by default.)
            let mut c = Companion::seeded(
                Settings {
                    attitude,
                    mp_low: 15.0,
                    ..Settings::default()
                },
                SEED,
            );
            let mut hp_lines = Vec::new();
            let mut mp_lines = Vec::new();
            for fight in 0..30 {
                let t = fight as f64 * 120.0;
                for (dt, hp, mp) in [
                    (0.0, 90.0, 90.0),
                    (5.0, 70.0, 90.0),
                    (10.0, 50.0, 90.0),
                    (15.0, 35.0, 90.0),
                    (21.0, 20.0, 10.0),
                    (21.1, 20.0, 10.0),
                    (21.3, 20.0, 10.0),
                    (21.7, 20.0, 10.0),
                    (30.0, 90.0, 90.0),
                ] {
                    for line in dealt(&c.observe(t + dt, frame(hp, mp, 10.0))) {
                        if card_of(attitude, lines::HP_LOW, &line) {
                            hp_lines.push(line);
                        } else if card_of(attitude, lines::MP_LOW, &line) {
                            mp_lines.push(line);
                        } else {
                            panic!("{}: not a first line: {line:?}", attitude.word());
                        }
                    }
                }
                c.player_spoke(t + 40.0);
            }
            assert_eq!(hp_lines.len(), 30, "{}: {hp_lines:?}", attitude.word());
            assert_eq!(mp_lines.len(), 30, "{}: {mp_lines:?}", attitude.word());
            for line in hp_lines.iter().chain(&mp_lines) {
                assert!(!presumes(line), "{}: {line:?}", attitude.word());
            }
            // An unanswered fight, the bar lower at each line (a bar that
            // stays put is said once): the second line repeats the first,
            // and may say so; the third asks why. MP the same (under the
            // 50% he asked for: 45, 39, 33).
            for (hp_bar, decks) in [
                (
                    true,
                    [lines::HP_LOW, lines::HP_LOW_AGAIN, lines::HP_LOW_ASK],
                ),
                (
                    false,
                    [lines::MP_LOW, lines::MP_LOW_AGAIN, lines::MP_LOW_ASK],
                ),
            ] {
                let mut c = Companion::seeded(
                    Settings {
                        attitude,
                        mp_low: 50.0,
                        ..Settings::default()
                    },
                    SEED,
                );
                let mut again = Vec::new();
                for i in 0..=430 {
                    let t = i as f64 * 0.1;
                    let low = if t < 12.0 {
                        20.0
                    } else if t < 42.0 {
                        14.0
                    } else {
                        8.0
                    };
                    let (hp, mp) = if hp_bar {
                        (low, 90.0)
                    } else {
                        (90.0, low + 25.0)
                    };
                    again.extend(dealt(&c.observe(t, frame(hp, mp, 10.0))));
                }
                assert_eq!(again.len(), 3, "{}: {again:?}", attitude.word());
                for (line, deck) in again.iter().zip(decks) {
                    assert!(
                        card_of(attitude, deck, line),
                        "{}: {again:?}",
                        attitude.word()
                    );
                }
            }
        }
        // Every repeat card is one that could follow an unanswered line,
        // and none says "still at" the number (the bar may have moved).
        for deck in [lines::HP_LOW_AGAIN, lines::MP_LOW_AGAIN] {
            for attitude in Attitude::ALL {
                for card in attitude.lines(deck) {
                    assert!(!card.to_lowercase().contains("still at {}"), "{card:?}");
                }
            }
        }
        // Every card that asks, asks, with the bar in it — and claims no
        // more than it knows: two lines unanswered, the bar where it is. No
        // "still" or "again", no fraction of the bar, no time.
        for deck in [lines::HP_LOW_ASK, lines::MP_LOW_ASK] {
            for attitude in Attitude::ALL {
                for card in attitude.lines(deck) {
                    let lower = card.to_lowercase();
                    assert!(card.contains('?') && card.contains("{}"), "{card:?}");
                    assert!(!presumes(card), "{}: {card:?}", attitude.word());
                    for claim in [
                        "half",
                        "third",
                        "quarter",
                        "most of",
                        "second",
                        "minute",
                        "hour",
                        "ago",
                        "earlier",
                        "all night",
                        "all day",
                    ] {
                        assert!(!lower.contains(claim), "{}: {card:?}", attitude.word());
                    }
                }
            }
        }
        // The beating is a fight's first shout as often as not, and it has
        // one deck: every card stands alone, and none puts a number on the
        // fall that the rule does not check (a 100→65 fall was "half your
        // bar"; a fight's first line was "Why are you still standing in
        // it?").
        for attitude in Attitude::ALL {
            for card in attitude.lines(lines::BEATING) {
                assert!(!presumes(card), "{}: {card:?}", attitude.word());
                for fraction in ["half", "third", "quarter", "most of"] {
                    assert!(
                        !card.to_lowercase().contains(fraction),
                        "{}: {card:?} sizes the fall",
                        attitude.word()
                    );
                }
            }
        }
    }

    #[test]
    fn sixty_beatings_across_fights_are_dealt_like_a_deck() {
        // A fight every two minutes (a minute's quiet ends one): hit from
        // 100 to 64 in a second, potted back. Each is called, each put
        // another way, the first with the reading.
        let mut c = Companion::seeded(Settings::default(), SEED);
        let mut beatings = Vec::new();
        for fight in 0..60 {
            let t = fight as f64 * 120.0;
            for (dt, hp) in [
                (0.0, 100.0),
                (0.5, 100.0),
                (1.0, 100.0),
                (1.5, 65.0),
                (1.6, 64.0),
                (2.0, 64.0),
                (5.0, 100.0),
                (10.0, 100.0),
            ] {
                beatings.extend(dealt(&c.observe(t + dt, read(hp))));
            }
        }
        assert_eq!(beatings.len(), 60, "{beatings:?}");
        dealt_like_a_deck(lines::BEATING, &beatings);
        assert!(beatings[0].contains("64 percent"), "{beatings:?}");
    }

    #[test]
    fn sixty_deaths_and_sixty_level_ups_are_dealt_like_decks() {
        // A death a minute (HP read at zero, revived at once), each said
        // another way; the first in full.
        let mut c = Companion::seeded(Settings::default(), SEED);
        let mut deaths = Vec::new();
        for minute in 0..60 {
            let t = minute as f64 * 60.0;
            for (dt, hp) in [
                (0.0, 100.0),
                (0.1, 100.0),
                (0.2, 100.0),
                (1.0, 0.0),
                (1.1, 0.0),
                (1.2, 0.0),
                (2.0, 100.0),
            ] {
                deaths.extend(dealt(&c.observe(t + dt, read(hp))));
            }
        }
        assert_eq!(deaths.len(), 60, "{deaths:?}");
        dealt_like_a_deck(lines::DEATH, &deaths);
        assert!(deaths[0].starts_with("Your HP hit zero."), "{deaths:?}");
        // A level a minute, from 57 to 117.
        let mut c = Companion::seeded(Settings::default(), SEED);
        let mut ups = Vec::new();
        for minute in 0..=60 {
            for second in 0..60 {
                let mut obs = frame(90.0, 90.0, 10.0);
                obs.level = Some(57 + minute);
                let t = minute as f64 * 60.0 + second as f64;
                ups.extend(dealt(&c.observe(t, obs)));
            }
        }
        assert_eq!(ups.len(), 60, "{ups:?}");
        dealt_like_a_deck(lines::LEVEL_UP, &ups);
        assert_eq!(ups[0], "Level up! You're level 58.");
        assert!(ups.iter().all(|l| l.contains(char::is_numeric)), "{ups:?}");
    }

    #[test]
    fn another_night_deals_the_decks_in_another_order() {
        // Two sessions, two seeds (checked to differ in what they deal, as
        // nearly any two do): seven deaths each, the lead first in both,
        // and then not the same script — one night it was death #1, #2
        // and #3 in the same three lines every night.
        let deaths = |seed: u64| -> Vec<String> {
            let mut c = Companion::seeded(Settings::default(), seed);
            let mut deaths = Vec::new();
            for minute in 0..7 {
                let t = minute as f64 * 60.0;
                for (dt, hp) in [
                    (0.0, 100.0),
                    (1.0, 0.0),
                    (1.1, 0.0),
                    (1.2, 0.0),
                    (2.0, 100.0),
                ] {
                    deaths.extend(alerts(&c.observe(t + dt, read(hp))));
                }
            }
            deaths
        };
        let (one, two) = (deaths(1), deaths(2));
        assert_eq!(one.len(), 7);
        assert_ne!(one, two);
        dealt_like_a_deck(lines::DEATH, &one);
        dealt_like_a_deck(lines::DEATH, &two);
        assert_eq!(deaths(1), one);
        // The instant answers ("HP 76%.") are dealt from the session's
        // deck too: another order each night, the plainest first — and
        // nothing for a question it has no number for.
        let answers = |seed: u64| -> Vec<String> {
            let mut c = Companion::seeded(Settings::default(), seed);
            c.observe(0.0, read(76.0));
            assert_eq!(c.instant("where am I"), None);
            (0..12)
                .map(|_| c.instant("what's my hp").unwrap())
                .collect()
        };
        let (one, two) = (answers(1), answers(2));
        assert_eq!(one[0], "Your HP's at 76%.");
        assert_eq!(two[0], one[0]);
        assert_ne!(one, two);
        assert_eq!(answers(1), one);
        let mut unseen = Companion::seeded(Settings::default(), 1);
        assert_eq!(unseen.instant("what's my hp"), None);
    }

    #[test]
    fn the_quieter_lines_are_dealt_like_decks_too() {
        // The game coming and going, mute and unmute: each time put another
        // way, in the attitude of the moment.
        let mut c = Companion::seeded(
            Settings {
                attitude: Attitude::Savage,
                ..Settings::default()
            },
            SEED,
        );
        let mut seen = Vec::new();
        let mut lost = Vec::new();
        let mut again = Vec::new();
        for k in 0..14 {
            let t = k as f64 * 100.0;
            let line = said(&c.observe(t, frame(90.0, 90.0, 10.0)));
            if k == 0 {
                seen.extend(line);
            } else {
                again.extend(line);
            }
            c.observe(t + 1.0, Observation::unseen(GameView::NotFound));
            lost.extend(said(
                &c.observe(t + 10.0, Observation::unseen(GameView::NotFound)),
            ));
        }
        let savage = |deck: [&[&str]; 3], line: &str| Attitude::Savage.lines(deck).contains(&line);
        assert_eq!(seen.len(), 1);
        assert!(savage(lines::SEEN, &seen[0]), "{seen:?}");
        assert_eq!((lost.len(), again.len()), (14, 13));
        for (deck, dealt) in [(lines::LOST, &lost), (lines::AGAIN, &again)] {
            assert!(dealt.iter().all(|l| savage(deck, l)), "{dealt:?}");
            assert_eq!(dealt[0], Attitude::Savage.lines(deck)[0]);
            for pair in dealt.windows(2) {
                assert_ne!(pair[0], pair[1], "{dealt:?}");
            }
            let mut first_round: Vec<&String> = dealt[..7].iter().collect();
            first_round.sort();
            first_round.dedup();
            assert_eq!(first_round.len(), 7, "{dealt:?}");
        }
        let mut muted = Vec::new();
        let mut unmuted = Vec::new();
        for k in 0..14 {
            muted.extend(said(&c.command(2000.0 + k as f64, Command::Mute)));
            unmuted.extend(said(&c.command(2000.5 + k as f64, Command::Unmute)));
        }
        for (deck, dealt) in [(lines::MUTED, &muted), (lines::UNMUTED, &unmuted)] {
            assert_eq!(dealt.len(), 14);
            assert!(dealt.iter().all(|l| savage(deck, l)), "{dealt:?}");
            for pair in dealt.windows(2) {
                assert_ne!(pair[0], pair[1], "{dealt:?}");
            }
        }
    }

    #[test]
    fn every_deck_has_six_ways_to_say_it_and_savage_roasts_the_play_only() {
        // Never said in any voice, in either language: these are about
        // who they are, not how they play (and the last ones are not a
        // joke).
        const BLOCKLIST: &[&str] = &[
            "retard",
            "spaz",
            "fag",
            "tranny",
            "nigg",
            "kike",
            "chink",
            "kys",
            "kill yourself",
            "מפגר",
            "הומו",
            "קוקסינל",
            "כושי",
            "ערבוש",
            "פרענק",
            "אשכנאצי",
            "שרמוט",
            "זונה",
            "נכה",
            "תתאבד",
            "לך תמות",
        ];
        for (name, en, he) in lines::ALL {
            // (A deck dealt a few times a night at most has three.)
            let least = if lines::SHORT.contains(name) { 3 } else { 6 };
            for (attitude, deck) in Attitude::ALL.into_iter().flat_map(|a| [(a, *en), (a, *he)]) {
                let list = attitude.lines(deck);
                assert!(
                    list.len() >= least,
                    "{name} ({}): {} lines",
                    attitude.word(),
                    list.len()
                );
                let mut sorted = list.to_vec();
                sorted.sort_unstable();
                sorted.dedup();
                assert_eq!(
                    sorted.len(),
                    list.len(),
                    "{name} ({}) repeats a line",
                    attitude.word()
                );
                for line in list {
                    let lower = line.to_lowercase();
                    assert!(!line.trim().is_empty(), "{name}");
                    // (The wake word in a spoken line: the phone would hear it.)
                    assert!(
                        !lower.contains("syrup") && !line.contains("סירופ"),
                        "{name}: {line:?}"
                    );
                    assert!(
                        BLOCKLIST.iter().all(|word| !lower.contains(word)),
                        "{name}: {line:?}"
                    );
                    // A line or two, as said on voice chat.
                    assert!(line.split_whitespace().count() <= 20, "{name}: {line:?}");
                }
            }
        }
        // Where there is a number, the lead says it (and every card of
        // the mark moving).
        for deck in [
            lines::BEATING,
            lines::HP_LOW,
            lines::MP_LOW,
            lines::LEVEL_UP,
            lines::SOONER,
            lines::BEATING_HE,
            lines::HP_LOW_HE,
            lines::MP_LOW_HE,
            lines::LEVEL_UP_HE,
            lines::SOONER_HE,
        ] {
            for attitude in Attitude::ALL {
                assert!(
                    attitude.lines(deck)[0].contains("{}"),
                    "{}",
                    attitude.word()
                );
            }
        }
        for attitude in Attitude::ALL {
            for card in attitude
                .lines(lines::SOONER)
                .iter()
                .chain(attitude.lines(lines::SOONER_HE))
            {
                assert!(card.contains("{}"), "{card:?}");
            }
        }
    }

    #[test]
    fn every_deck_has_a_hebrew_twin_card_for_card() {
        // One deck deals both languages by place, so the card at a place
        // must be the same line in both: the same number of cards per
        // attitude, the same `{}` card for card. No Latin letter anywhere
        // (`ai::translate` lets a line through as it is only when it has
        // none: one "HP" in a card and every warning waits for a model
        // again) — HP is חיים, MP מאנה, EXP אקספי. And an amount stands on
        // its own ("בערך 20 אחוז" glued to a ב- or ל- would not read).
        let latin = |line: &str| line.chars().any(|c| c.is_ascii_alphabetic());
        let hebrew = |line: &str| line.chars().any(|c| ('\u{05d0}'..='\u{05ea}').contains(&c));
        let amounts = [
            "beating",
            "HP low",
            "HP low, again",
            "HP low, ask",
            "MP low",
            "MP low, again",
            "MP low, ask",
        ];
        let mut cards = 0;
        for (name, en, he) in lines::ALL {
            for attitude in Attitude::ALL {
                let (en, he) = (attitude.lines(*en), attitude.lines(*he));
                assert_eq!(en.len(), he.len(), "{name} ({})", attitude.word());
                for (e, h) in en.iter().zip(he) {
                    cards += 1;
                    assert_eq!(
                        e.matches("{}").count(),
                        h.matches("{}").count(),
                        "{name}: {e:?} / {h:?}"
                    );
                    assert!(!latin(h) && hebrew(h), "{name}: {h:?}");
                    if amounts.contains(name) {
                        let parts: Vec<&str> = h.split("{}").collect();
                        for pair in parts.windows(2) {
                            assert!(
                                pair[0].is_empty() || pair[0].ends_with(' '),
                                "{name}: {h:?}"
                            );
                            assert!(
                                pair[1]
                                    .chars()
                                    .next()
                                    .is_none_or(|c| c == ' ' || c.is_ascii_punctuation()),
                                "{name}: {h:?}"
                            );
                        }
                    }
                }
            }
        }
        assert_eq!(cards, 382);
        // Its notes that are no deck, the same.
        for (e, h) in [
            (lines::UNSTEADY, lines::UNSTEADY_HE),
            (lines::MARKED, lines::MARKED_HE),
        ] {
            assert_eq!(e.matches("{}").count(), h.matches("{}").count());
            assert!(!latin(h) && hebrew(h), "{h:?}");
        }
        // Filled in as they are said — an amount read or estimated, a
        // bar's name — still not a Latin letter.
        for gauge in [gauge(20.0, true), gauge(19.6, false)] {
            let amount = low_words(gauge.unwrap(), true);
            assert!(!latin(&amount), "{amount:?}");
        }
        assert_eq!(low_words(gauge(20.0, true).unwrap(), true), "20 אחוז");
        assert_eq!(low_words(gauge(19.6, false).unwrap(), true), "בערך 20 אחוז");
        for bar in [Bar::Hp, Bar::Mp] {
            assert!(!latin(bar.name(true)));
            for attitude in Attitude::ALL {
                for card in attitude.lines(lines::MISREAD_HE) {
                    assert!(!latin(&card.replace("{}", bar.name(true))));
                }
            }
        }
    }

    #[test]
    fn the_hebrew_decks_keep_the_rules_of_the_english_ones() {
        // What a card may presume, claim or count, as the English tests
        // have it for their decks (`the_first_line_of_a_fight_never_says_
        // still_or_again`, `a_death_with_no_warning_before_it_never_claims_
        // one_came`, `no_death_card_speaks_of_an_earlier_death…`, `it_says_
        // it_twice_then_asks…`), in Hebrew words.
        const PRESUMES: &[&str] = &[
            "עדיין",
            "שוב",
            "עוד פעם",
            "פעמיים",
            "אמרתי",
            "שמעת",
            "תמיד",
            "כל הזמן",
            "כבר",
        ];
        const DURATIONS: &[&str] = &[
            "שנייה",
            "שניות",
            "דקה",
            "דקות",
            "שעה",
            "שעות",
            "לפני",
            "קודם",
            "זמן",
            "כל הלילה",
            "כל היום",
            "כל הערב",
        ];
        const FRACTIONS: &[&str] = &["חצי", "שליש", "רבע", "רוב"];
        const CLAIMS: &[&str] = &[
            "אמרתי",
            "הזהרתי",
            "הוזהרת",
            "כשאני אומר",
            "שמעת",
            "צעקתי",
            "ידעתי",
        ];
        const EARLIER_DEATH: &[&str] = &["שוב", "עוד פעם", "עוד מוות", "עדיין"];
        const AWAY: &[&str] = &[
            "לא פה",
            "לא כאן",
            "תחזור",
            "כשתחזור",
            "חזרת",
            "אף אחד",
            "קיר",
            "איפה אתה",
            "תגיד משהו",
            "תדבר",
            "לשמוע ממך",
            "ליד המחשב",
        ];
        let any = |card: &str, words: &[&'static str]| -> Option<&'static str> {
            words.iter().find(|w| says_he(card, w)).copied()
        };
        // (The helper itself: whole words, with a prefix or two.)
        assert!(says_he("ושוב: 20 אחוז.", "שוב"));
        assert!(!says_he("זה חשוב.", "שוב"));
        assert!(says_he("אני כל הזמן אומר.", "כל הזמן"));
        assert!(!says_he("ארבע אזהרות.", "רבע"));
        for attitude in Attitude::ALL {
            let cards = |deck: [&'static [&'static str]; 3]| attitude.lines(deck).iter().copied();
            let voice = attitude.word();
            // A fight's first line stands alone (and the beating sizes no
            // fall).
            for card in cards(lines::BEATING_HE)
                .chain(cards(lines::HP_LOW_HE))
                .chain(cards(lines::MP_LOW_HE))
            {
                assert_eq!(any(card, PRESUMES), None, "{voice}: {card:?}");
            }
            for card in cards(lines::BEATING_HE) {
                assert_eq!(any(card, FRACTIONS), None, "{voice}: {card:?}");
            }
            // A repeat may say "still", never "still at" the number.
            for card in cards(lines::HP_LOW_AGAIN_HE).chain(cards(lines::MP_LOW_AGAIN_HE)) {
                for still_at in ["עדיין על {}", "עדיין ב-{}", "עדיין ב{}"] {
                    assert!(!card.contains(still_at), "{voice}: {card:?}");
                }
            }
            // Every card that asks, asks, with the bar in it, and claims no
            // more than it knows.
            for card in cards(lines::HP_LOW_ASK_HE).chain(cards(lines::MP_LOW_ASK_HE)) {
                assert!(card.contains('?') && card.contains("{}"), "{card:?}");
                for words in [PRESUMES, DURATIONS, FRACTIONS] {
                    assert_eq!(any(card, words), None, "{voice}: {card:?}");
                }
            }
            // A death with no warning before it claims none, and no death
            // card speaks of an earlier one; a warned death may say it
            // warned (and some do), never how long ago.
            for card in cards(lines::DEATH_HE) {
                assert_eq!(any(card, CLAIMS), None, "{voice}: {card:?}");
            }
            for card in cards(lines::DEATH_HE).chain(cards(lines::DEATH_WARNED_HE)) {
                assert_eq!(any(card, EARLIER_DEATH), None, "{voice}: {card:?}");
            }
            for card in cards(lines::DEATH_WARNED_HE) {
                assert_eq!(any(card, DURATIONS), None, "{voice}: {card:?}");
            }
            assert!(
                cards(lines::DEATH_WARNED_HE).any(|c| any(c, CLAIMS).is_some()),
                "{voice}"
            );
            // The hold for someone who is there never takes them for away.
            for card in cards(lines::HOLD_HERE_HE) {
                assert_eq!(any(card, AWAY), None, "{voice}: {card:?}");
            }
            for card in cards(lines::POTTED_AT_LAST_HE) {
                assert!(card.split_whitespace().count() <= 8, "{card:?}");
            }
            // "Still there?" knows nothing moved, not for how long.
            for card in cards(lines::STILL_THERE_HE) {
                assert!(!card.chars().any(|c| c.is_ascii_digit()), "{card:?}");
                for words in [&["דקה", "דקות", "שעה", "שעות"][..], FRACTIONS] {
                    assert_eq!(any(card, words), None, "{voice}: {card:?}");
                }
            }
        }
    }

    #[test]
    fn the_hebrew_cards_say_it_the_way_it_is_said() {
        // What a native ear caught as translated (w29 §2): calques of
        // English idioms, a system message, "your life" for the HP bar, and
        // one card broken outright ("166 עכשיו. לקח לך מספיק.").
        const CALQUES: &[&str] = &[
            "לקח לך מספיק",
            "עבודה יפה",
            "יש לי את המשחק",
            "יש לי עין",
            "תנער את זה",
            "תחזור פנימה",
            "חזרה לעיר",
            "בשבילך",
            "בחינם",
            "ההשתקה בוטלה",
            "סותם על",
            "תפס אותך",
            "תן לו קצת מרחק",
            "ממה שזה לא יהיה",
            "זה מוות",
            "הרווחת את זה",
            "להשוויץ?",
            "להתייבש",
            "מיול",
            "להטיל סקילים",
            "הסקילים נעצרים",
            "{} על הבר",
            "משתתק עם",
            "מאיפה שלא תהיה",
            "זמן שהעצות",
            "על מה לרדת עליך",
            "את ה{} שלך",
            "הקריאות של ה{} שלך",
            "טיפוס יפה",
            "תמשיך לטפס",
            "עכשיו. למעלה.",
            "יאללה, למעלה",
            "ממש פה.",
            "יו. מדבר.",
            "זמן מה",
            "מזערת",
            "בהשהיה",
        ];
        let mut hebrew: Vec<&str> = Vec::new();
        for (_, _, he) in lines::ALL {
            for list in he.iter() {
                hebrew.extend(list.iter());
            }
        }
        hebrew.extend([lines::UNSTEADY_HE, lines::MARKED_HE]);
        for deck in [
            instant::lines::LEVEL_UP_HE,
            instant::lines::LEVEL_NOW_HE,
            instant::lines::LEVEL_HE,
            instant::lines::BAR_LOW_HE,
            instant::lines::BAR_OK_HE,
            instant::lines::EXP_HE,
            crate::ai::brain::HERE_HE,
        ] {
            for list in deck.iter() {
                hebrew.extend(list.iter());
            }
        }
        for card in &hebrew {
            for calque in CALQUES {
                assert!(!card.contains(calque), "{card:?} says {calque:?}");
            }
        }
        // The bar's name in the notes is the bar's: "בר החיים שלך".
        assert_eq!(
            lines::UNSTEADY_HE.replace("{}", Bar::Hp.name(true)),
            "הקריאות של בר החיים קופצות, אז אני עוצר את אזהרות החיים עד שזה יירגע."
        );
        // One is "פחות משני אחוז", read or measured: plural, as every card's
        // amount, and "about" already ("נשארו לך אחוז אחד חיים" broke the
        // agreement; "1 אחוז" is read "אחד אחוז").
        let one = |read| Gauge {
            percent: 0.6,
            current: None,
            max: None,
            read,
        };
        assert_eq!(low_words(one(true), true), "פחות משני אחוז");
        assert_eq!(low_words(one(false), true), "פחות משני אחוז");
        assert_eq!(low_words(one(true), false), "1 percent");
    }

    #[test]
    fn no_hebrew_card_says_what_a_native_ear_heard_as_translated() {
        // w32's §2: Hebrew a friend on voice chat would not say — calques
        // of the English ("נוחתת", "לבזבז אוויר", "ממש שם", "הנה זה"),
        // book Hebrew ("קשה לקריאה"), a word for word "not changing while
        // you ask", the wrong word ("הפיל" is a knockdown; "עוקב אחרי" is
        // "follows"; "מכשף" a translated wizard where gamers say "מאג'";
        // skills are thrown, not done) — and the tells of a machine: one
        // opening ("חזרתי") for most of a deck, one ending ("תחזור לשם")
        // for a quarter of one. Every Hebrew card, its own and the
        // instant answers'.
        const SAID_WRONG: &[&str] = &[
            "לפני שלא",
            "תמלא לפני",
            "תמלא.",
            "תמלא!",
            "תמלא כש",
            "הפיל אותך",
            "הנה זה",
            "עוקב אחרי",
            "תעשה סקילים",
            "מכשף",
            "נוחת",
            "לבזבז אוויר",
            "לא משתנה בזמן",
            "אין מה לראות",
            "ממש שם",
            "לשאול לא ימלא",
            "קשה לקריאה",
            "זה רמה",
            "כל הזמן אומר",
        ];
        let mut cards: Vec<&str> = Vec::new();
        for (_, _, he) in lines::ALL {
            for attitude in Attitude::ALL {
                cards.extend(attitude.lines(*he));
            }
        }
        for (_, list) in instant::lines::ALL {
            for attitude in Attitude::ALL {
                cards.extend(attitude.lines(*list));
            }
        }
        for card in &cards {
            for wrong in SAID_WRONG {
                assert!(!card.contains(wrong), "{card:?} says {wrong:?}");
            }
        }
        let count = |deck: lines::Cards, f: &dyn Fn(&str) -> bool| -> usize {
            Attitude::ALL
                .iter()
                .flat_map(|a| a.lines(deck).iter())
                .filter(|c| f(c))
                .count()
        };
        // (Was 11 of 21, and 5 of 19.)
        assert!(count(lines::UNMUTED_HE, &|c| c.starts_with("חזרתי")) <= 8);
        assert!(count(lines::DEATH_HE, &|c| c.ends_with("תחזור לשם.")) <= 3);
    }

    #[test]
    fn one_percent_agrees_with_every_hebrew_card() {
        // w32's D6: filled at 1%, the cards that open with the plural
        // "נשארו" said "נשארו לך אחוז אחד חיים" (a plural verb, a singular
        // noun, and "אחוז אחד חיים" is not said), "נשארו אחוז אחד בבר",
        // "נשארו לך אחוז אחד מאנה". Every card with an amount, at 1%, read
        // and measured, says "פחות משני אחוז" — plural like the rest — and
        // never "about" before it.
        let decks = [
            lines::BEATING_HE,
            lines::HP_LOW_HE,
            lines::HP_LOW_AGAIN_HE,
            lines::HP_LOW_ASK_HE,
            lines::MP_LOW_HE,
            lines::MP_LOW_AGAIN_HE,
            lines::MP_LOW_ASK_HE,
        ];
        let mut filled = 0;
        for deck in decks {
            for attitude in Attitude::ALL {
                for card in attitude.lines(deck).iter().filter(|c| c.contains("{}")) {
                    for read in [true, false] {
                        let one = gauge(1.3, read).unwrap();
                        let line = card.replace("{}", &low_words(one, true));
                        assert!(line.contains("פחות משני אחוז"), "{line}");
                        assert!(!line.contains("אחוז אחד"), "{line}");
                        assert!(!line.contains("בערך פחות"), "{line}");
                        filled += 1;
                    }
                }
            }
        }
        assert!(filled > 100, "{filled}");
        // As said: the beating's lead at 1%, then the low line's — read in
        // the game's font (the fill at 1.2 is no reading: see `SLIVER`).
        let mut c = Companion::seeded(Settings::default(), SEED);
        c.set_language(Some("he-IL"));
        c.observe(0.0, frame(80.0, 90.0, 10.0));
        c.observe(0.1, frame(80.0, 90.0, 10.0));
        let mut lines: Vec<String> = Vec::new();
        for i in 2..30 {
            let mut obs = frame(1.2, 90.0, 10.0);
            obs.hp = gauge(1.2, true);
            lines.extend(dealt(&c.observe(i as f64 * 0.1, obs)));
        }
        assert_eq!(
            lines.first().map(String::as_str),
            Some("וואו, אתה חוטף מכות, נשארו לך פחות משני אחוז חיים. תתרחק ותשתה!"),
            "{lines:?}"
        );
    }

    #[test]
    fn a_latin_hp_asked_in_a_hebrew_session_is_answered_in_hebrew() {
        // A Hebrew recogniser writes "HP?" in Latin letters: it got "76% HP.
        // You're fine, stop asking." — English, mid-Hebrew-session.
        let mut c = Companion::seeded(Settings::default(), SEED);
        c.observe(0.0, frame(76.0, 80.0, 10.0));
        let english = c.instant("HP?").unwrap();
        assert!(english.contains("HP"), "{english}");
        c.set_language(Some("he-IL"));
        let hebrew = c.instant("HP?").unwrap();
        assert!(
            !hebrew.chars().any(|ch| ch.is_ascii_alphabetic()) && hebrew.contains("76 אחוז"),
            "{hebrew}"
        );
        // Asked in English words, it is answered in English.
        let english = c.instant("what's my HP").unwrap();
        assert!(
            english.chars().any(|ch| ch.is_ascii_alphabetic()),
            "{english}"
        );
    }

    #[test]
    fn a_player_who_speaks_hebrew_hears_its_own_lines_in_hebrew() {
        // He said "talk to me in Hebrew": every line of its own came in
        // English and went to a model to be translated first — a warning
        // with its number missed the cache every time — and the phone
        // showed the English. Now the cards are Hebrew from the start, in
        // order: the game seen, a beating (HP read off the number), a low
        // line (the bar's estimate: "בערך"), a sudden death, a level-up.
        let mut c = Companion::seeded(Settings::default(), SEED);
        c.set_language(Some("he-IL"));
        assert!(c.speaks_hebrew());
        let at = |hp: f32, read: bool, level: u32| -> Observation {
            let mut obs = frame(hp, 80.0, 10.0);
            obs.hp = gauge(hp, read);
            obs.level = Some(level);
            obs
        };
        let mut seen = Vec::new();
        let mut lines = Vec::new();
        let mut play = |c: &mut Companion, frames: &[(f64, f32, bool, u32)]| {
            for &(t, hp, read, level) in frames {
                let actions = c.observe(t, at(hp, read, level));
                seen.extend(said(&actions));
                lines.extend(alerts(&actions));
            }
        };
        play(
            &mut c,
            &[
                (0.0, 100.0, true, 57),
                (0.5, 100.0, true, 57),
                (1.0, 100.0, true, 57),
                (1.5, 65.0, true, 57),
                (1.6, 64.0, true, 57),
                (2.0, 64.0, true, 57),
                (5.0, 100.0, true, 57),
            ],
        );
        play(
            &mut c,
            &[
                (120.0, 90.0, false, 57),
                (125.0, 70.0, false, 57),
                (130.0, 50.0, false, 57),
                (135.0, 35.0, false, 57),
                (141.0, 20.0, false, 57),
                (141.1, 20.0, false, 57),
                (141.3, 20.0, false, 57),
                (141.7, 20.0, false, 57),
                (150.0, 90.0, false, 57),
            ],
        );
        play(
            &mut c,
            &[
                (300.0, 100.0, true, 57),
                (301.0, 0.0, true, 57),
                (301.1, 0.0, true, 57),
                (301.2, 0.0, true, 57),
                (303.0, 100.0, true, 57),
            ],
        );
        play(
            &mut c,
            &[
                (400.0, 100.0, true, 58),
                (402.0, 100.0, true, 58),
                (403.1, 100.0, true, 58),
            ],
        );
        let lead = |deck: [&'static [&'static str]; 3]| Attitude::Friendly.lines(deck)[0];
        assert_eq!(seen[0], lead(lines::SEEN_HE), "{seen:?}");
        assert_eq!(
            lines,
            [
                lead(lines::BEATING_HE).replace("{}", "64 אחוז"),
                lead(lines::HP_LOW_HE).replace("{}", "בערך 20 אחוז"),
                lead(lines::DEATH_HE).to_string(),
                lead(lines::LEVEL_UP_HE).replace("{}", "58"),
            ]
        );
        assert_eq!(
            lines[0],
            "וואו, אתה חוטף מכות, נשארו לך 64 אחוז חיים. תתרחק ותשתה!"
        );
        for line in seen.iter().chain(&lines) {
            assert!(!line.chars().any(|c| c.is_ascii_alphabetic()), "{line:?}");
        }
        // A mark, a mute and back: Hebrew too.
        let marked = said(&c.command(500.0, Command::Mark));
        assert_eq!(marked, ["סימנתי. זה סימון מספר 1."]);
        let muted = said(&c.command(501.0, Command::Mute));
        assert!(
            Attitude::Friendly
                .lines(lines::MUTED_HE)
                .contains(&muted[0].as_str())
        );
        // Any other language: the English cards (translated on the way to
        // the voice); none known: English.
        for (locale, hebrew) in [
            (Some("he"), true),
            (Some("iw-IL"), true),
            (Some("HE_il"), true),
            (Some("en-US"), false),
            (Some("es-ES"), false),
            (Some("hebrew"), false),
            (None, false),
        ] {
            c.set_language(locale);
            assert_eq!(c.speaks_hebrew(), hebrew, "{locale:?}");
        }
    }

    #[test]
    fn a_switch_of_language_mid_session_goes_on_with_the_deck() {
        // One deck deals both languages by place. Fourteen deaths (two
        // rounds of the friendly deck), the language switched after the
        // fourth: the fifth is the card the deck had next, in the other
        // language — the same place a session in one language all along
        // deals — and no card of the round comes twice (a deck that
        // started over would deal its lead again).
        let deaths = |first: &str, then: &str| -> Vec<String> {
            let mut c = Companion::seeded(Settings::default(), SEED);
            c.set_language(Some(first));
            let mut deaths = Vec::new();
            for minute in 0..14 {
                if minute == 4 {
                    c.set_language(Some(then));
                }
                let t = minute as f64 * 60.0;
                for (dt, hp) in [
                    (0.0, 100.0),
                    (1.0, 0.0),
                    (1.1, 0.0),
                    (1.2, 0.0),
                    (2.0, 100.0),
                ] {
                    deaths.extend(dealt(&c.observe(t + dt, read(hp))));
                }
            }
            deaths
        };
        let places = |deaths: &[String]| -> Vec<usize> {
            deaths
                .iter()
                .map(|l| {
                    place(Attitude::Friendly, lines::DEATH, l)
                        .or_else(|| place(Attitude::Friendly, lines::DEATH_HE, l))
                        .unwrap_or_else(|| panic!("not a death card: {l:?}"))
                })
                .collect()
        };
        let english = deaths("en-US", "en-US");
        let hebrew = deaths("he-IL", "he-IL");
        assert_eq!(places(&english), places(&hebrew));
        for (first, then) in [("he-IL", "en-US"), ("en-US", "he-IL")] {
            let switched = deaths(first, then);
            assert_eq!(places(&switched), places(&english), "{first} → {then}");
            for (i, line) in switched.iter().enumerate() {
                let he = (i < 4) == (first == "he-IL");
                assert_eq!(
                    line.chars().any(|c| c.is_ascii_alphabetic()),
                    !he,
                    "{first} → {then}, {i}: {line:?}"
                );
            }
            let mut round = places(&switched)[..7].to_vec();
            round.sort_unstable();
            round.dedup();
            assert_eq!(round.len(), 7, "{switched:?}");
        }
    }

    #[test]
    fn a_death_no_warning_came_before_moves_the_warning_sooner() {
        let mut c = Companion::seeded(Settings::default(), SEED);
        // Worn down through 40% and 32% over a few seconds: above 30%, so
        // no warning came, and slowly enough for one at 35 to have helped.
        // Each death is the number read 0 in the game's font, and costs EXP
        // (10% to 9%, then 8%…). (The bar's fill at 0 is no reading: see
        // `SLIVER`.)
        let mut t = 0.0;
        let mut exp = 10.0;
        let mut step = |c: &mut Companion, hp: f32, frames: usize| {
            let mut lines = Vec::new();
            for i in 0..frames {
                t += 0.1;
                if hp <= 0.0 && i == 0 {
                    exp -= 1.0;
                }
                let mut obs = frame(hp, 50.0, exp);
                if hp <= 0.0 {
                    obs.hp = gauge(0.0, true);
                }
                lines.extend(said(&c.observe(t, obs)));
            }
            lines
        };
        step(&mut c, 80.0, 3);
        // (Twenty points at a time, three seconds apart, is no beating —
        // a beating is a warning, and the death after it a warned one.)
        assert!(step(&mut c, 60.0, 31).is_empty());
        assert!(step(&mut c, 40.0, 31).is_empty());
        assert!(step(&mut c, 32.0, 3).is_empty());
        // The death line, with the change after it.
        let death = step(&mut c, 0.0, 25);
        assert_eq!(death.len(), 1, "{death:?}");
        let line = without_sooner(&death[0], 35).unwrap_or_else(|| panic!("{death:?}"));
        assert!(from(lines::DEATH, line), "{death:?}");
        assert_eq!(c.settings.hp_low, 35.0);
        // A sudden death from full HP: no warning would have helped.
        step(&mut c, 100.0, 120);
        let death = step(&mut c, 0.0, 25);
        assert_eq!(death.len(), 1, "{death:?}");
        assert!(from(lines::DEATH, &death[0]), "{death:?}");
        assert_eq!(c.settings.hp_low, 35.0);
        // Warned on the way down: the warning came, nothing to change (and
        // the line may say it did).
        step(&mut c, 100.0, 120);
        assert_eq!(step(&mut c, 20.0, 8).len(), 1);
        let death = step(&mut c, 0.0, 25);
        assert_eq!(death.len(), 1, "{death:?}");
        assert!(from(lines::DEATH_WARNED, &death[0]), "{death:?}");
        assert_eq!(c.settings.hp_low, 35.0);
        // Never past half the bar, and never when warnings are off.
        c.settings.hp_low = 0.0;
        step(&mut c, 100.0, 120);
        step(&mut c, 20.0, 8);
        step(&mut c, 0.0, 25);
        assert_eq!(c.settings.hp_low, 0.0);
        c.settings.hp_low = 48.0;
        step(&mut c, 100.0, 300);
        step(&mut c, 80.0, 31);
        step(&mut c, 60.0, 31);
        step(&mut c, 0.0, 25);
        assert_eq!(c.settings.hp_low, 50.0);
    }

    #[test]
    fn no_death_card_speaks_of_an_earlier_death_and_a_known_player_gets_another_opening_each_night()
    {
        // No card of either death deck speaks of an earlier death ("Dead
        // again"): for a player who knows it (`settled`), any card may
        // open a night's deaths, so none may presume one before it. And
        // then, in every voice, for the first death of twenty nights,
        // sudden or warned, fresh or settled: a card of the right deck,
        // and never one that says "again".
        let again = |line: &str| {
            let lower = line.to_lowercase();
            ["dead again", "died again", "another death", "once again"]
                .iter()
                .any(|c| lower.contains(c))
        };
        for deck in [lines::DEATH, lines::DEATH_WARNED] {
            for attitude in Attitude::ALL {
                for card in attitude.lines(deck) {
                    assert!(!again(card), "{card:?}");
                }
            }
        }
        let sudden: [(f64, f32); 7] = [
            (0.0, 100.0),
            (0.1, 100.0),
            (0.2, 100.0),
            (1.0, 0.0),
            (1.1, 0.0),
            (1.2, 0.0),
            (2.0, 100.0),
        ];
        let warned: [(f64, f32); 8] = [
            (0.0, 100.0),
            (5.0, 100.0),
            (10.0, 20.0),
            (10.4, 20.0),
            (10.7, 20.0),
            (12.0, 0.0),
            (12.1, 0.0),
            (12.2, 0.0),
        ];
        for seed in 1..=20 {
            for attitude in Attitude::ALL {
                for known in [false, true] {
                    for (frames, deck) in [
                        (&sudden[..], lines::DEATH),
                        (&warned[..], lines::DEATH_WARNED),
                    ] {
                        let mut c = Companion::seeded(
                            Settings {
                                attitude,
                                ..Settings::default()
                            },
                            seed,
                        );
                        c.settled(known);
                        let mut lines = Vec::new();
                        for (t, hp) in frames {
                            lines.extend(dealt(&c.observe(*t, read(*hp))));
                        }
                        let death = lines.last().unwrap().as_str();
                        assert!(
                            attitude.lines(deck).contains(&death),
                            "seed {seed}, {}: {lines:?}",
                            attitude.word()
                        );
                        assert!(!again(death), "seed {seed}, {}: {death:?}", attitude.word());
                    }
                }
            }
        }
    }

    #[test]
    fn a_known_player_does_not_hear_the_same_opening_line_every_night() {
        // Twenty nights, two companions each (seeded apart, as two nights
        // are): for a player who knows it, the night's first death is not
        // the lead on most nights (one in seven, give or take), the
        // deaths are still every card once before any again and never the
        // same card twice running — and a new player still hears the lead
        // first, every night. (The lead-first rule had "You died. Revive
        // and get back in there." open the deaths of every night.)
        let deaths = |seed: u64, known: bool| -> Vec<String> {
            let mut c = Companion::seeded(Settings::default(), seed);
            c.settled(known);
            let mut deaths = Vec::new();
            for minute in 0..14 {
                let t = minute as f64 * 60.0;
                for (dt, hp) in [
                    (0.0, 100.0),
                    (1.0, 0.0),
                    (1.1, 0.0),
                    (1.2, 0.0),
                    (2.0, 100.0),
                ] {
                    deaths.extend(dealt(&c.observe(t + dt, read(hp))));
                }
            }
            deaths
        };
        let lead = Attitude::Friendly.lines(lines::DEATH)[0];
        let mut not_led = 0;
        for night in 1..=20u64 {
            let (one, two) = (
                deaths(night * 1000 + 1, true),
                deaths(night * 1000 + 2, true),
            );
            assert_ne!(one, two, "night {night}");
            for dealt in [&one, &two] {
                assert_eq!(dealt.len(), 14, "{dealt:?}");
                for round in dealt.chunks(7) {
                    let mut seen: Vec<&String> = round.iter().collect();
                    seen.sort();
                    seen.dedup();
                    assert_eq!(seen.len(), 7, "{dealt:?}");
                }
                for pair in dealt.windows(2) {
                    assert_ne!(pair[0], pair[1], "{dealt:?}");
                }
                not_led += usize::from(dealt[0] != lead);
            }
            // The same seeds, a new player: the lead, both nights.
            assert_eq!(deaths(night * 1000 + 1, false)[0], lead);
            assert_eq!(deaths(night * 1000 + 2, false)[0], lead);
        }
        assert!(
            not_led >= 24,
            "the lead opened {} of 40 nights",
            40 - not_led
        );
        // Settled after a line was dealt: the deck in play keeps its order.
        let mut c = Companion::seeded(Settings::default(), SEED);
        let mut d = Vec::new();
        for (t, hp) in [
            (0.0, 100.0),
            (1.0, 0.0),
            (1.1, 0.0),
            (1.2, 0.0),
            (2.0, 100.0),
        ] {
            d.extend(dealt(&c.observe(t, read(hp))));
        }
        assert_eq!(d, [lead]);
        c.settled(true);
        for minute in 1..7 {
            let t = minute as f64 * 60.0;
            for (dt, hp) in [
                (0.0, 100.0),
                (1.0, 0.0),
                (1.1, 0.0),
                (1.2, 0.0),
                (2.0, 100.0),
            ] {
                d.extend(dealt(&c.observe(t + dt, read(hp))));
            }
        }
        assert_eq!(d, deaths(SEED, false)[..7]);
    }

    #[test]
    fn a_death_with_no_warning_before_it_never_claims_one_came() {
        const CLAIMS: [&str; 3] = ["told you", "when i say", "potion one"];
        let claims = |line: &str| {
            let lower = line.to_lowercase();
            CLAIMS.iter().any(|c| lower.contains(c))
        };
        // Down to 40% for a couple of seconds and dead, with no warning
        // (the mark is 30): the warning moves sooner, and the line says so
        // — and nothing else. One night this was "Dead. Told you to pot.
        // I'll warn you sooner from now on, under 40%."
        let mut c = Companion::seeded(
            Settings {
                attitude: Attitude::Blunt,
                ..Settings::default()
            },
            SEED,
        );
        let mut death = Vec::new();
        for (t, hp) in [
            (0.0, 100.0),
            (0.1, 100.0),
            (5.0, 40.0),
            (5.1, 40.0),
            (5.2, 40.0),
            (7.0, 40.0),
            (7.5, 0.0),
            (7.6, 0.0),
            (7.7, 0.0),
        ] {
            death.extend(dealt(&c.observe(t, read(hp))));
        }
        assert_eq!(death.len(), 1, "{death:?}");
        let line = without_sooner(&death[0], 35).unwrap_or_else(|| panic!("{death:?}"));
        assert!(
            Attitude::Blunt.lines(lines::DEATH).contains(&line),
            "{death:?}"
        );
        assert!(!claims(line), "{death:?}");
        // A one-shot from full, sixty times, in every voice: never a claim.
        for attitude in Attitude::ALL {
            let mut c = Companion::seeded(
                Settings {
                    attitude,
                    ..Settings::default()
                },
                SEED,
            );
            let mut deaths = Vec::new();
            for minute in 0..60 {
                let t = minute as f64 * 60.0;
                for (dt, hp) in [
                    (0.0, 100.0),
                    (0.1, 100.0),
                    (0.2, 100.0),
                    (1.0, 0.0),
                    (1.1, 0.0),
                    (1.2, 0.0),
                    (2.0, 100.0),
                ] {
                    deaths.extend(dealt(&c.observe(t + dt, read(hp))));
                }
            }
            assert_eq!(deaths.len(), 60, "{}: {deaths:?}", attitude.word());
            for line in &deaths {
                assert!(
                    attitude.lines(lines::DEATH).contains(&line.as_str()),
                    "{}: {line:?}",
                    attitude.word()
                );
                assert!(!claims(line), "{}: {line:?}", attitude.word());
            }
            // Warned two seconds before (HP read at 20, the line said, then
            // zero): it did say to pot, and may say so — and never that it
            // will warn sooner.
            let mut c = Companion::seeded(
                Settings {
                    attitude,
                    ..Settings::default()
                },
                SEED,
            );
            let mut deaths = Vec::new();
            for minute in 0..60 {
                let t = minute as f64 * 60.0;
                let mut said = Vec::new();
                for (dt, hp) in [
                    (0.0, 100.0),
                    (5.0, 100.0),
                    (10.0, 20.0),
                    (10.1, 20.0),
                    (10.2, 20.0),
                    (10.3, 20.0),
                    (10.4, 20.0),
                    (10.5, 20.0),
                    (10.6, 20.0),
                    (10.7, 20.0),
                    (12.0, 0.0),
                    (12.1, 0.0),
                    (12.2, 0.0),
                    (13.0, 100.0),
                ] {
                    said.extend(dealt(&c.observe(t + dt, read(hp))));
                }
                assert_eq!(said.len(), 2, "{}: {said:?}", attitude.word());
                deaths.push(said.pop().unwrap());
            }
            for line in &deaths {
                assert!(
                    attitude.lines(lines::DEATH_WARNED).contains(&line.as_str()),
                    "{}: {line:?}",
                    attitude.word()
                );
                assert!(without_sooner(line, 35).is_none(), "{line:?}");
            }
            assert!(deaths.iter().any(|l| claims(l)), "{deaths:?}");
        }
    }

    #[test]
    fn the_mark_moving_is_said_in_the_attitudes_voice_from_a_small_deck() {
        // Four deaths no warning came before, each after a couple of
        // seconds where a sooner warning would have come (48%, over every
        // one of the marks): the mark moves 30, 35, 40, 45, 50, and
        // each death line ends with a card of the voice that says so,
        // with the new mark in it — a different card each time, and never
        // the one sentence in every voice ("…try not to suck this time.
        // I'll warn you sooner from now on, under 35%." was a settings
        // dialog spliced onto a savage card).
        for attitude in Attitude::ALL {
            let mut c = Companion::seeded(
                Settings {
                    attitude,
                    ..Settings::default()
                },
                SEED,
            );
            let mut cards = Vec::new();
            for (n, mark) in [35u32, 40, 45, 50].into_iter().enumerate() {
                let t = n as f64 * 120.0;
                let mut death = Vec::new();
                for (dt, hp) in [
                    (0.0, 100.0),
                    (0.1, 100.0),
                    (5.0, 48.0),
                    (5.1, 48.0),
                    (5.2, 48.0),
                    (7.0, 48.0),
                    (7.5, 0.0),
                    (7.6, 0.0),
                    (7.7, 0.0),
                ] {
                    death.extend(dealt(&c.observe(t + dt, read(hp))));
                }
                assert_eq!(death.len(), 1, "{}: {death:?}", attitude.word());
                assert_eq!(c.settings.hp_low, mark as f32);
                let head = without_sooner(&death[0], mark)
                    .unwrap_or_else(|| panic!("{}: {death:?}", attitude.word()));
                assert!(
                    attitude.lines(lines::DEATH).contains(&head),
                    "{}: {death:?}",
                    attitude.word()
                );
                let card = &death[0][head.len() + 1..];
                assert!(
                    attitude
                        .lines(lines::SOONER)
                        .iter()
                        .any(|c| c.replace("{}", &mark.to_string()) == card),
                    "{}: {card:?}",
                    attitude.word()
                );
                assert!(!death[0].contains("from now on, under"), "{death:?}");
                cards.push(card.to_string());
            }
            for pair in cards.windows(2) {
                assert_ne!(pair[0], pair[1], "{cards:?}");
            }
            // At the half-way mark the warning moves no further, and the
            // line says nothing about it.
            let mut death = Vec::new();
            for (dt, hp) in [
                (0.0, 100.0),
                (0.1, 100.0),
                (5.0, 60.0),
                (5.1, 60.0),
                (5.2, 60.0),
                (6.0, 0.0),
                (6.1, 0.0),
                (6.2, 0.0),
            ] {
                death.extend(dealt(&c.observe(600.0 + dt, read(hp))));
            }
            assert_eq!(death.len(), 1, "{death:?}");
            assert!(
                attitude.lines(lines::DEATH).contains(&death[0].as_str()),
                "{death:?}"
            );
            assert_eq!(c.settings.hp_low, 50.0);
        }
    }

    #[test]
    fn a_death_in_a_fight_it_warned_in_is_a_warned_death_however_long_ago_the_warning() {
        // Hit from 100 to 25 at 1 s (the beating, with HP already low:
        // it said to pot), HP staying at 25 (no new fall: the low line is
        // not said again), and dead at 30 s — 29 s after the line. It
        // warned in this fight: the death is a warned one, nothing about
        // warning sooner, and the mark stays. (A window of 15 s had it say
        // "I'll warn you sooner from now on, under 35%" here, and the mark
        // crept up a night at a time for deaths it had called.)
        let mut c = Companion::seeded(Settings::default(), SEED);
        let mut lines: Vec<(f64, String)> = Vec::new();
        for i in 0..=302 {
            let t = i as f64 * 0.1;
            let hp = if t < 1.0 {
                100.0
            } else if t < 30.0 {
                25.0
            } else {
                0.0
            };
            for line in dealt(&c.observe(t, read(hp))) {
                lines.push((t, line));
            }
        }
        assert_eq!(lines.len(), 2, "{lines:?}");
        assert!(from(lines::BEATING, &lines[0].1), "{lines:?}");
        let (at, death) = &lines[1];
        assert!((at - 30.2).abs() < 0.05, "{lines:?}");
        assert!(from(lines::DEATH_WARNED, death), "{death:?}");
        assert!(without_sooner(death, 35).is_none(), "{death:?}");
        assert_eq!(c.settings.hp_low, 30.0);
        // A minute on with HP back and never low again, then a sudden
        // death: that fight is over, and this death was not warned of.
        for i in 0..=700 {
            let t = 31.0 + i as f64 * 0.1;
            let hp = if t < 100.0 { 100.0 } else { 0.0 };
            for line in dealt(&c.observe(t, read(hp))) {
                lines.push((t, line));
            }
        }
        assert_eq!(lines.len(), 3, "{lines:?}");
        assert!(from(lines::DEATH, &lines[2].1), "{lines:?}");
        assert_eq!(c.settings.hp_low, 30.0);
    }

    #[test]
    fn a_death_right_after_a_beating_is_warned_and_a_sudden_one_moves_no_mark() {
        // Trusted (four fights to 60, each potted in time), then a fall
        // from full to zero in 1.5 s: the beating is shouted as HP goes
        // under the floor, at 53 — above the mark — and the death a second
        // later is one it warned of: the warned deck, no sooner card, the
        // mark where it was. (It was "You're eating every hit. Move." then
        // "Well, that's a death. Revive, go again. New rule: I yell at 35.")
        let mut c = Companion::seeded(Settings::default(), SEED);
        let mut lines: Vec<(f64, f32, String)> = Vec::new();
        for i in 0..6000 {
            let t = i as f64 * 0.1;
            // (Fights at 1, 3, 5, 7 and 9 min.)
            let start = 60.0 + ((t - 60.0) / 120.0).floor() * 120.0;
            let (dip, pot) = if t < 540.0 {
                (60.0, 3.0)
            } else {
                (0.0, f64::INFINITY)
            };
            let s = t - start;
            let hp = if t < 60.0 || s >= pot {
                100.0
            } else if s < 1.5 {
                100.0 - (s / 1.5) as f32 * (100.0 - dip)
            } else {
                dip
            };
            for line in dealt(&c.observe(t, read(hp))) {
                lines.push((t, hp, line));
            }
        }
        let last: Vec<_> = lines.iter().filter(|(t, _, _)| *t >= 540.0).collect();
        assert_eq!(last.len(), 2, "{lines:?}");
        assert!(from(lines::BEATING, &last[0].2), "{last:?}");
        assert!(last[0].1 > 30.0, "{last:?}");
        let (at, _, death) = last[1];
        assert!((at - 541.7).abs() < 0.05, "{last:?}");
        assert!(from(lines::DEATH_WARNED, death), "{death:?}");
        assert!(without_sooner(death, 35).is_none(), "{death:?}");
        assert_eq!(c.settings.hp_low, 30.0);
        // No beating and no low line: worn down to 52 (five points a
        // second is no beating), a hit to 45, and dead after `secs` there.
        // Two seconds and more in the band over the mark (30 to 50): a
        // warning at 35 would have had its time — the mark moves. Less: a
        // sudden death, and the mark stays (it moved for 1.5 s at 45).
        let death_after = |secs: f64| -> (String, f32) {
            let mut c = Companion::seeded(Settings::default(), SEED);
            let mut lines = Vec::new();
            for i in 0..400 {
                let t = i as f64 * 0.1;
                let hp = if t < 1.0 {
                    100.0
                } else if t < 10.6 {
                    100.0 - (t - 1.0) as f32 * 5.0
                } else if t < 14.0 {
                    52.0
                } else if t < 14.0 + secs {
                    45.0
                } else {
                    0.0
                };
                lines.extend(dealt(&c.observe(t, read(hp))));
            }
            assert_eq!(lines.len(), 1, "{secs}: {lines:?}");
            (lines.pop().unwrap(), c.settings.hp_low)
        };
        let (death, mark) = death_after(1.5);
        assert!(from(lines::DEATH, &death), "{death:?}");
        assert_eq!(mark, 30.0);
        let (death, mark) = death_after(2.5);
        let line = without_sooner(&death, 35).unwrap_or_else(|| panic!("{death:?}"));
        assert!(from(lines::DEATH, line), "{death:?}");
        assert_eq!(mark, 35.0);
    }

    #[test]
    fn a_level_up_is_celebrated_once() {
        let mut c = Companion::seeded(Settings::default(), SEED);
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
        let mut c = Companion::seeded(Settings::default(), SEED);
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
            lines.retain(|l| !self::from(lines::SEEN, l));
            lines
        };
        // Up by one, the same name: said.
        let mut c = Companion::seeded(Settings::default(), SEED);
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
        // …and the same character read lower is a misread (or a rebirth):
        // not taken for a good while, then quietly.
        assert!(hold(&mut c, 240.0, named(160, "WanWanBoggio")).is_empty());
        let mut lines = Vec::new();
        for i in 0..300 {
            lines.extend(said(
                &c.observe(244.0 + i as f64 * 0.1, named(160, "WanWanBoggio")),
            ));
        }
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert!(lines[0].contains("lower level"), "{lines:?}");
        assert_eq!(c.level(), Some(160));
        // A name on neither side: the number alone decides.
        let mut c = Companion::seeded(Settings::default(), SEED);
        let mut unnamed = frame(90.0, 90.0, 40.0);
        unnamed.level = Some(57);
        assert!(hold(&mut c, 0.0, unnamed.clone()).is_empty());
        unnamed.level = Some(58);
        assert_eq!(hold(&mut c, 10.0, unnamed), ["Level up! You're level 58."]);
    }

    #[test]
    fn a_level_misread_for_a_moment_is_not_believed() {
        let mut c = Companion::seeded(Settings::default(), SEED);
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
    fn a_level_reading_that_flips_celebrates_the_level_once() {
        // The sight's reader gives 165, then 166, then 165 again, every 40 s
        // for the same character (one of them a misread it holds for a
        // while): one level-up, not one every time 166 comes round.
        let at = |level: u32| {
            let mut obs = read(100.0);
            obs.level = Some(level);
            obs
        };
        let mut c = Companion::seeded(Settings::default(), SEED);
        let mut lines: Vec<(f64, String)> = Vec::new();
        for i in 0..4000 {
            let t = i as f64 * 0.1;
            let level = if (i / 400) % 2 == 0 { 165 } else { 166 };
            for line in alerts(&c.observe(t, at(level))) {
                lines.push((t, line));
            }
        }
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert_eq!(lines[0].1, "Level up! You're level 166.");
        assert!((40.0..45.0).contains(&lines[0].0), "{lines:?}");
        assert_eq!(c.level(), Some(166));
        // The lower reading is taken, quietly, once it has held a long
        // while — and the number is still celebrated only going up.
        let mut c = Companion::seeded(Settings::default(), SEED);
        for i in 0..100 {
            c.observe(i as f64 * 0.1, at(166));
        }
        let mut lower = Vec::new();
        for i in 100..400 {
            lower.extend(said(&c.observe(i as f64 * 0.1, at(165))));
        }
        assert_eq!(c.level(), Some(166), "{lower:?}");
        lower.extend(said(&c.observe(40.1, at(165))));
        assert_eq!(c.level(), Some(165));
        assert_eq!(
            lower,
            ["Level 165 now, from 166 (a lower level: another character, or misread)."]
        );
        // A real level-up after all that: said.
        let mut lines = Vec::new();
        for i in 450..500 {
            lines.extend(alerts(&c.observe(i as f64 * 0.1, at(167))));
        }
        assert_eq!(lines, ["Level up! You're level 167."]);
    }

    #[test]
    fn a_level_read_rising_without_an_exp_fall_is_celebrated() {
        let mut c = Companion::seeded(Settings::default(), SEED);
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
    fn another_character_is_no_level_up_though_its_exp_bar_is_lower() {
        // w36's note: the EXP tracker takes a fall from 80% or more to under
        // 30% for a level-up (the bar's wrap), and a character switch is
        // one — level 167 at 85%, the select screen, level 9 at 20%. At
        // b28cdfb it was counted (`levels_gained`, the model's "Level-ups:
        // 1", the stats' record) 4 s after the new bar was read: after the
        // switch was taken when the name changed (3 s), and 26 s before it
        // with no name read (a lower level holds 30 s). Ten frames a
        // second, EXP from the fill and creeping; the second character's
        // name and level.
        let run = |first: Option<&str>, then: (Option<&str>, u32)| {
            let mut c = Companion::seeded(Settings::default(), SEED);
            let (mut lines, mut counted) = (Vec::new(), Vec::new());
            for i in 0..2400 {
                let t = i as f64 * 0.1;
                let (name, level, exp) = match i {
                    0..600 => (first, Some(167), Some(85.0 + t as f32 * 0.01)),
                    // (The select screen: the window, no HUD.)
                    600..650 => (None, None, None),
                    _ => (then.0, Some(then.1), Some(20.0 + (t - 65.0) as f32 * 0.05)),
                };
                let mut obs = frame(90.0, 80.0, 0.0);
                if level.is_none() {
                    (obs.hp, obs.mp) = (None, None);
                }
                obs.name = name.map(String::from);
                obs.level = level;
                obs.exp = exp.and_then(|e| gauge(e, false));
                lines.extend(said(&c.observe(t, obs)).into_iter().map(|l| (t, l)));
                let count = (c.progress().levels_gained, c.so_far().level_ups);
                if count != (0, 0) {
                    counted.push((t, count));
                }
            }
            (lines, counted, c)
        };
        for then in [(Some("WanLittle"), 9), (None, 9), (Some("WanBig"), 170)] {
            let first = then.0.map(|_| "WanWanBoggio");
            let (lines, counted, c) = run(first, then);
            // Never counted, at any frame. (Fails at b28cdfb: (1, 1) from
            // 69.0 s.)
            assert!(counted.is_empty(), "{then:?}: {:?}", counted.first());
            assert!(
                lines.iter().all(|(_, l)| !from(lines::LEVEL_UP, l)),
                "{lines:?}"
            );
            // Taken quietly, as before: noted, not celebrated.
            assert_eq!(c.level(), Some(then.1));
            let note = format!("Level {} now, from 167 (", then.1);
            assert!(lines.iter().any(|(_, l)| l.starts_with(&note)), "{lines:?}");
        }
        // The new character's own level-up is one: 9 to 10, its bar from
        // 99.5% to 0.5% — said, and counted once.
        let (_, _, mut c) = run(Some("WanWanBoggio"), (Some("WanLittle"), 9));
        let mut lines = Vec::new();
        for i in 0..100 {
            let mut obs = frame(90.0, 80.0, 0.0);
            obs.name = Some("WanLittle".into());
            let (level, exp) = if i < 40 { (9, 99.5) } else { (10, 0.5) };
            obs.level = Some(level);
            obs.exp = gauge(exp, false);
            lines.extend(said(&c.observe(240.0 + i as f64 * 0.1, obs)));
        }
        assert_eq!(lines, ["Level up! You're level 10."]);
        assert_eq!((c.progress().levels_gained, c.so_far().level_ups), (1, 1));
    }

    #[test]
    fn with_the_wake_word_required_talk_is_ignored_and_addressed_commands_answered() {
        let mut c = Companion::seeded(
            Settings {
                always_listen: false,
                ..Settings::default()
            },
            SEED,
        );
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
        let mut c = Companion::seeded(Settings::default(), SEED);
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
        let mut c = Companion::seeded(Settings::default(), SEED);
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
        let mut c = Companion::seeded(Settings::default(), SEED);
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
        let mut c = Companion::seeded(Settings::default(), SEED);
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
    fn a_correction_that_ends_with_its_own_words_is_the_players_whole_sentence() {
        let mut c = Companion::seeded(Settings::default(), SEED);
        c.remember_spoken(
            10.0,
            "You're at the Gate of the Future, level 165, EXP 74%.",
        );
        // One night these came back as "No, I'm not", "I am not in", and
        // nothing at all.
        for heard in [
            "No, I'm not at the Gate of the Future",
            "I am not in the gate of the future",
            "wrong, not the gate of the future",
        ] {
            assert_eq!(c.strip_echo(14.0, heard).as_deref(), Some(heard));
        }
        // "No" and then only its own words: not thrown away as an echo.
        assert_eq!(
            c.strip_echo(14.0, "no the gate of the future").as_deref(),
            Some("no")
        );
        assert_eq!(
            c.strip_echo(14.0, "לא, gate of the future level 165")
                .as_deref(),
            Some("לא,")
        );
        // Unless "no" was its own first word: then it is its own voice.
        let mut c = Companion::seeded(Settings::default(), SEED);
        c.remember_spoken(10.0, "No, you're at the Gate of the Future.");
        assert_eq!(
            c.strip_echo(14.0, "no you're at the gate of the future"),
            None
        );
        assert_eq!(
            c.strip_echo(14.0, "no I'm not at the gate of the future")
                .as_deref(),
            Some("no I'm not at the gate of the future")
        );
    }

    #[test]
    fn a_long_line_heard_back_with_a_few_words_written_differently_is_all_its_own() {
        let mut c = Companion::seeded(Settings::default(), SEED);
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
        let mut c = Companion::seeded(Settings::default(), SEED);
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
        let mut c = Companion::seeded(Settings::default(), SEED);
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
        let mut c = Companion::seeded(Settings::default(), SEED);
        assert_eq!(
            said(&c.command(0.0, Command::Status)),
            ["I can't see the game right now."]
        );
        assert!(said(&c.command(0.0, Command::Session))[0].starts_with("This session"));
    }

    #[test]
    fn the_rate_needs_some_play_first() {
        let mut c = Companion::seeded(Settings::default(), SEED);
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

    #[test]
    fn the_session_so_far_is_what_a_friend_in_the_room_would_know() {
        let mut c = Companion::seeded(Settings::default(), SEED);
        // Nothing yet: no game, nobody spoke, nothing to say.
        assert_eq!(c.so_far(), SoFar::default());
        // Five minutes of play, ten frames a second: HP read, swinging
        // 90–94 (a hit, a potion), EXP still.
        for i in 0..3000 {
            c.observe(i as f64 * 0.1, read(90.0 + (i / 10 % 5) as f32));
        }
        let so_far = c.so_far();
        assert!((so_far.seconds - 299.9).abs() < 0.01, "{so_far:?}");
        assert_eq!(so_far.since_player_spoke, None);
        assert_eq!((so_far.deaths, so_far.since_last_death), (0, None));
        assert_eq!((so_far.level_ups, so_far.since_last_level_up), (0, None));
        assert_eq!(so_far.lowest_hp_lately, Some(90.0));
        // HP moving is the game going on: not quiet.
        assert!(so_far.quiet_for.unwrap() < 5.0, "{so_far:?}");
        assert_eq!(so_far.unseen_for, None);
        // They say something; a hit to 8% and a potion; four minutes at
        // 60; then a death, read from the number at once.
        c.player_spoke(300.0);
        c.observe(300.1, read(8.0));
        c.observe(300.2, read(8.0));
        for i in 3..2400 {
            c.observe(300.0 + i as f64 * 0.1, read(60.0));
        }
        for i in 0..3 {
            c.observe(540.0 + i as f64 * 0.1, read(0.0));
        }
        assert!(c.dead());
        for i in 3..50 {
            c.observe(540.0 + i as f64 * 0.1, read(0.0));
        }
        let so_far = c.so_far();
        assert_eq!(so_far.deaths, 1);
        assert!(
            (so_far.since_last_death.unwrap() - 4.7).abs() < 0.11,
            "{so_far:?}"
        );
        assert!(
            (so_far.since_player_spoke.unwrap() - 244.9).abs() < 0.11,
            "{so_far:?}"
        );
        // The 8% was more than a minute ago; the lowest reading of the
        // last minute is the 60 (a death is no reading).
        assert_eq!(so_far.lowest_hp_lately, Some(60.0));
        // HP sat at 60 and EXP at 10 for four minutes — but the death
        // moved HP, 4.7 s ago.
        assert!(so_far.quiet_for.unwrap() < 5.0, "{so_far:?}");
        // A revive, then a long stand in town: quiet for the duration.
        for i in 0..710 {
            c.observe(545.0 + i as f64 * 0.5, read(100.0));
        }
        let so_far = c.so_far();
        assert!(!c.dead());
        assert!(
            (so_far.quiet_for.unwrap() - 354.5).abs() < 0.01,
            "{so_far:?}"
        );
        assert_eq!(so_far.lowest_hp_lately, Some(100.0));
        // A level-up: counted, and when.
        let mut up = read(100.0);
        up.level = Some(166);
        for i in 0..40 {
            c.observe(900.0 + i as f64 * 0.1, up.clone());
        }
        let so_far = c.so_far();
        assert_eq!(so_far.level_ups, 1);
        assert!(so_far.since_last_level_up.unwrap() < 1.5, "{so_far:?}");
        // The game out of sight: for how long; nothing is quiet about it.
        for i in 0..100 {
            c.observe(
                904.0 + i as f64 * 0.1,
                Observation::unseen(GameView::NotFound),
            );
        }
        let so_far = c.so_far();
        assert!(
            (so_far.unseen_for.unwrap() - 10.0).abs() < 0.01,
            "{so_far:?}"
        );
        assert_eq!(so_far.quiet_for, None);
        // Back in view.
        c.observe(914.0, read(100.0));
        assert_eq!(c.so_far().unseen_for, None);
    }

    #[test]
    fn it_asks_whether_the_player_is_there_only_when_the_game_has_sat_idle() {
        // A player it can see playing is never asked: a fight every two
        // minutes answered by a potion, EXP creeping up, one word at
        // minute 2 and not another all evening — ninety minutes, not a
        // line. (It was asked at minute 32, two minutes after a beating
        // they potted: "You alive over there?" to someone it was watching
        // play.)
        let asked = |actions: &[Action]| -> Vec<String> {
            alerts(actions)
                .into_iter()
                .filter(|l| from(lines::STILL_THERE, l))
                .collect()
        };
        let fight = |t: f64| -> f32 {
            let phase = t % 120.0;
            if phase < 1.5 {
                100.0 - (phase / 1.5) as f32 * 75.0
            } else if phase < 3.0 {
                25.0
            } else {
                100.0
            }
        };
        let playing = |t: f64| {
            let mut obs = read(fight(t));
            obs.exp = gauge(10.0 + (t / 60.0) as f32 * 0.3, true);
            obs
        };
        let mut c = Companion::seeded(Settings::default(), SEED);
        let mut lines: Vec<(f64, String)> = Vec::new();
        for i in 0..54_000 {
            let t = i as f64 * 0.1;
            if i == 1200 {
                c.player_spoke(t);
            }
            for line in asked(&c.observe(t, playing(t))) {
                lines.push((t, line));
            }
        }
        assert!(lines.is_empty(), "{lines:?}");
        // Playing until minute 10 (HP moving every second), then nothing
        // moves and nobody speaks: asked once, twenty minutes on, at
        // minute 30 — and not again in the hour after.
        let grind = |i: usize| read(90.0 + (i / 10 % 5) as f32);
        let idle_from = |i: usize, from: usize| if i < from { grind(i) } else { read(90.0) };
        let mut c = Companion::seeded(Settings::default(), SEED);
        let mut lines: Vec<(f64, String)> = Vec::new();
        for i in 0..54_000 {
            let t = i as f64 * 0.1;
            for line in asked(&c.observe(t, idle_from(i, 6000))) {
                lines.push((t, line));
            }
        }
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert!((lines[0].0 - 1800.0).abs() < 0.2, "{lines:?}");
        assert_eq!(variant(lines::STILL_THERE, &lines[0].1), Some(0));
        // A word at minute 25: not before minute 45.
        let mut c = Companion::seeded(Settings::default(), SEED);
        let mut lines: Vec<(f64, String)> = Vec::new();
        for i in 0..36_000 {
            let t = i as f64 * 0.1;
            if i == 15_000 {
                c.player_spoke(t);
            }
            for line in asked(&c.observe(t, idle_from(i, 6000))) {
                lines.push((t, line));
            }
        }
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert!((lines[0].0 - 2700.0).abs() < 0.2, "{lines:?}");
        // A session that starts idle is idle from its first frame.
        let mut c = Companion::seeded(Settings::default(), SEED);
        let mut lines: Vec<(f64, String)> = Vec::new();
        for i in 0..15_000 {
            let t = i as f64 * 0.1;
            for line in asked(&c.observe(t, read(90.0))) {
                lines.push((t, line));
            }
        }
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert!((lines[0].0 - 1200.0).abs() < 0.2, "{lines:?}");
        // The game out of view from 100 s and back at 1000 s: the twenty
        // minutes count from when it is back on screen.
        let mut c = Companion::seeded(Settings::default(), SEED);
        let mut lines: Vec<(f64, String)> = Vec::new();
        for i in 0..25_000 {
            let t = i as f64 * 0.1;
            let obs = if (1000..10_000).contains(&i) {
                Observation::unseen(GameView::NotFound)
            } else {
                read(90.0)
            };
            for line in asked(&c.observe(t, obs)) {
                lines.push((t, line));
            }
        }
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert!((lines[0].0 - 2200.0).abs() < 0.2, "{lines:?}");
        // Not within five minutes of a warning of its own: MP low at
        // 1000 s, coming back by its slow regen (a point a second: no
        // potion, and not the game going on), puts it off to 1300. (MP
        // warnings asked for: none by default.)
        let mut c = Companion::seeded(
            Settings {
                mp_low: 15.0,
                ..Settings::default()
            },
            SEED,
        );
        let mut lines: Vec<(f64, String)> = Vec::new();
        for i in 0..15_000 {
            let t = i as f64 * 0.1;
            let mut obs = read(90.0);
            obs.mp = gauge(
                if i < 10_000 {
                    80.0
                } else {
                    (10.0 + (i - 10_000) as f32 * 0.1).min(80.0)
                },
                true,
            );
            for line in asked(&c.observe(t, obs)) {
                lines.push((t, line));
            }
        }
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert!((lines[0].0 - 1300.6).abs() < 0.2, "{lines:?}");
        // Dead at the mark: not then (a death moves HP, and so does the
        // revive: twenty minutes from that).
        let mut c = Companion::seeded(Settings::default(), SEED);
        let mut lines: Vec<(f64, String)> = Vec::new();
        for i in 0..31_000 {
            let t = i as f64 * 0.1;
            let obs = if (11_950..12_100).contains(&i) {
                read(0.0)
            } else {
                read(90.0)
            };
            for line in asked(&c.observe(t, obs)) {
                lines.push((t, line));
            }
        }
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert!((lines[0].0 - 2410.0).abs() < 0.2, "{lines:?}");
        // A low bar nobody answers (flat under the mark): said once and
        // then quiet — no new fall, nothing to hold — so twenty minutes of
        // nothing moving and not a word is asked about, once, like any.
        let mut c = Companion::seeded(Settings::default(), SEED);
        let mut lines: Vec<(f64, String)> = Vec::new();
        for i in 0..24_000 {
            let t = i as f64 * 0.1;
            for line in asked(&c.observe(t, read(20.0))) {
                lines.push((t, line));
            }
        }
        assert!(!c.alerts_held(2400.0));
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert!((lines[0].0 - 1200.0).abs() < 0.2, "{lines:?}");
        // No card claims to know what they are doing: not that they are
        // grinding, not that they are asleep, not how long it has been.
        for attitude in Attitude::ALL {
            for card in attitude.lines(lines::STILL_THERE) {
                let lower = card.to_lowercase();
                for claim in [
                    "grind",
                    "asleep",
                    "keyboard",
                    "minute",
                    "half an hour",
                    "afk",
                ] {
                    assert!(!lower.contains(claim), "{card:?}");
                }
            }
        }
    }

    #[test]
    fn the_silence_a_sentence_ended_is_kept_for_the_reply_to_it() {
        // A word at a minute in, then 32 silent minutes of play; then "ok,
        // I'm back". The main loop notes the sentence before it builds the
        // reply's snapshot: `since_player_spoke` is 0 by then, and the
        // silence has to be remembered for it — for one reply's worth.
        let mut c = Companion::seeded(Settings::default(), SEED);
        let mut t = 0.0;
        while t < 60.0 {
            c.observe(t, read(90.0));
            t += 0.1;
        }
        c.player_spoke(60.0);
        assert_eq!(c.so_far().quiet_before, None);
        while t < 60.0 + 32.0 * 60.0 {
            c.observe(t, read(90.0));
            t += 0.1;
        }
        c.player_spoke(t);
        let so_far = c.so_far();
        assert!(
            (so_far.quiet_before.unwrap() - 1920.0).abs() < 0.2,
            "{so_far:?}"
        );
        assert!(so_far.since_player_spoke.unwrap() < 0.2, "{so_far:?}");
        // The snapshot the model gets says so, once, in place of when they
        // last spoke (the coach reads the same `so_far`: reading it does
        // not use it up).
        let text = crate::ai::brain::snapshot_with_view(c.last(), true, &c.progress(), &c.so_far());
        assert_eq!(
            text.matches("They had been quiet for 32 min until just now.")
                .count(),
            1,
            "{text}"
        );
        assert!(!text.contains("They last spoke"), "{text}");
        assert_eq!(c.so_far().quiet_before, so_far.quiet_before);
        // A second sentence five seconds on ended no silence…
        let from = t;
        while t < from + 5.0 {
            c.observe(t, read(90.0));
            t += 0.1;
        }
        c.player_spoke(t);
        assert_eq!(c.so_far().quiet_before, None);
        let text = crate::ai::brain::snapshot_with_view(c.last(), true, &c.progress(), &c.so_far());
        assert!(!text.contains("had been quiet"), "{text}");
        // …and ten seconds after a sentence that did, it is old news: the
        // reply to it has its snapshot by then, and the next sentence's
        // reply is not to say "welcome back" again (kept 30 s, it could).
        let from = t;
        while t < from + 120.0 {
            c.observe(t, read(90.0));
            t += 0.1;
        }
        c.player_spoke(t);
        assert!(c.so_far().quiet_before.is_some());
        let from = t;
        while t < from + 9.0 {
            c.observe(t, read(90.0));
            t += 0.1;
        }
        assert!(c.so_far().quiet_before.is_some());
        while t < from + 11.0 {
            c.observe(t, read(90.0));
            t += 0.1;
        }
        assert_eq!(c.so_far().quiet_before, None);
        // The first sentence of a session ends no silence of theirs: the
        // greeting is for that.
        let mut fresh = Companion::seeded(Settings::default(), SEED);
        for i in 0..6000 {
            fresh.observe(i as f64 * 0.1, read(90.0));
        }
        fresh.player_spoke(600.0);
        assert_eq!(fresh.so_far().quiet_before, None);
    }

    /// The owner's evening (2026-10-10), every line it said and every
    /// sentence the phone heard, as his log has them (`[reply]`,
    /// `[warning]`, `[alert]`: its own; `[echo]`: dropped as its own voice,
    /// with the part kept as his, if any; `[heard]`: taken as his).
    const OWNER_EVENING: &str = r##"
11:44:59  [reply] Hey Mako, back in the game?
11:45:11  [heard] No my name is not Michael my name is Miguel with the talk to me bro
11:45:12  [reply] [ silent ]Got it Miguel, sorry for the mix-up. Talk to me.
11:45:21  [heard] Not Miguel Hebrew
11:45:23  [reply] היי מיגל. תפתח את החלון של המשחק.
11:45:29  [heard] Mi Mi
11:45:30  [reply] [ silent ]היי מיגל, החלון של המשחק פתוח מאחורי חלון אחר.
11:45:36  [heard] Mika Mika Mika L
11:45:37  [reply] [ silent ]היי מיגל, תביא את חלון המשחק לקדימה.
11:45:48  [heard] Mikael
11:45:49  [reply] היי מיכאל, אתה במפה "East Forest Trail".
11:46:01  [heard] Market
11:46:02  [reply] היי מיכאל, המפה פתוחה אבל השחקן לא נראה על המסך.
11:46:12  [heard] Can you get
11:46:14  [reply] היי מיכאל, אתה ב־Perion, ליד שער הכפר.
11:46:37  [heard] Royalty
11:46:38  [reply] היי מיכאל, המפה היא "Henesys Market".
11:46:43  [echo] Market
11:47:25  [heard] Mail mail
11:47:26  [reply] יש לך 5 אפקטי עליית רמה על המסך.
11:47:35  [heard] Armani
11:47:41  [reply] אַרמני? זה השם שמופיע במשחק, או שאתה מתכוון למשהו אחר? ארמני—קלטתי. I'll call you Armani.
11:47:47  [heard] I need money
11:47:55  [reply] בשביל כסף, תמכור ציוד ופריטים מיותרים ל־NPC; אל תמכור ציוד שאת משתמשת בו.
11:47:57  [echo] Kiss
11:48:09  [heard] I don't want money talk to me in English I don't want money I want to know what level am I and how do I look
11:48:11  [reply] You’re level 16, and you look like a Magician in a dark outfit with a big hat.
11:48:18  [echo] Your level 16 and you look like a magician in a dog outfit with a big hat
11:48:22  [echo] I look like
11:48:24  [heard] Melo wheat
11:48:26  [heard] Atua and
11:48:27  [heard] and with
11:48:32  [heard] A hat that's like a bunny ears
11:48:34  [reply] Your character’s hidden behind the window, so I can’t judge the outfit—move it aside.
11:48:39  [echo] Outfit
11:48:44  [heard] My character is next to the cursor
11:48:46  [reply] I see a bunny-ear hat next to the cursor.
11:48:53  [echo] I see a bunny hat next to the cursor yeah that's that's
11:48:54  [heard] me that's one
11:48:55  [reply] I can’t make out what you mean by “that’s one”—say it again.
11:49:00  [echo] What do you mean by that  (kept: What do that)
11:49:00  [heard] What do that
11:49:01  [reply] That’s the Monster Collection registration popup; it’s asking you to confirm the entry.
11:49:05  [echo] that's me that's the monster  (kept: that's me)
11:49:05  [heard] that's me
11:49:08  [reply] Got it—that bunny-ear hat is you, Armani.
11:49:18  [echo] Got it that bunny hat is my name is Armani my name is Miha  (kept: my name is Armani my name is Miha)
11:49:18  [heard] my name is Armani my name is Miha
11:49:21  [reply] Got it, Miha—I’ll call you Miha.
11:49:27  [heard] I'll Miha
11:49:30  [reply] Got it, Miha.
11:49:35  [echo] Got it no
11:49:35  [heard] me
11:49:37  [reply] כן, את מיה—הדמות שלך היא הקוסמת עם אוזני הארנב.
11:49:44  [heard] My name is
11:49:46  [heard] Mikael
11:49:49  [reply] Got it, Mikael.
11:49:58  [heard] Let's
11:50:01  [heard] let's get some stuff for my magician
11:50:06  [warning] Wow, you just stood there and took that. Move, dumbass.
11:50:09  [heard] I need like a really good magician equipment how do I get it wow
11:50:12  [echo] Dumbass
11:50:18  [reply] At level 16, use gear drops or the weapon shop; better Magician gear unlocks as you level.
11:50:26  [echo] At level 16 used eardrops or the weapon shop
11:50:26  [reply] Actually, at level 16 don’t spend time farming gear—use quest rewards or whatever drops, then replace it with level-
11:50:37  [echo] Actually at level 16 don't spend time farming use rewards or whatever drops then replace it with
11:50:37  [echo] with level
11:50:40  [heard] OK
11:50:44  [heard] so what should I do now what should I do now
11:50:46  [reply] Open the equipment menu and check your weapon slot; your level-16 gear can wait until we see what you have.
11:50:56  [echo] The equipment and check your button slot level 16 gear from wait until we see what you have here
11:51:28  [heard] I opened it
11:51:28  [alert] Level up! Congrats.
11:51:29  [reply] Your inventory’s open; check the weapon slot and tell me what’s equipped.
11:51:33  [echo] Congrats
11:51:40  [heard] Equipped
11:51:44  [heard] what's equipped look at what's equipped and tell me what to get
11:51:50  [reply] Your weapon slot has a wand equipped; get a level-20 wand next. You’ve got a Sapphire Staff equipped, plus a hat, cape, gloves, and shoes. Keep them for now; replace pieces with higher-level Magician gear from drops or shops.
11:51:56  [echo] Weapon equipped sapphire equipped
11:52:06  [heard] What's the next quest I should go to
11:52:08  [reply] Follow the yellow quest markers to your left, and check the quest tracker for the objective name.
11:52:13  [echo] The yellow to your left and
11:52:15  [alert] עלית רמה! מזל טוב, וונוואן בוג׳יו!
11:52:17  [alert] Level up! Congrats.
11:52:20  [echo] A little
11:52:23  [echo] Congrats
11:52:48  [heard] How do I get to level 20 from quest fastest
11:52:54  [reply] Follow the active quest marker and clear nearby mobs; turn in each quest immediately for the fastest XP.
11:52:59  [echo] Follow the active question mark and clear  (kept: question mark)
11:52:59  [heard] question mark
11:52:59  [reply] Marked. That's mark 1.
11:53:00  [reply] Actually, that marker’s in Henesys—follow its available quest chain, but there’s no single fastest Mag
11:53:10  [echo] That's actually in Hennessey but there's no single  (kept: That's actually in Hennessey)
11:53:10  [heard] That's actually in Hennessey
11:53:12  [heard] search
11:53:16  [heard] quest that I can do in level 16
11:53:19  [reply] In Henesys, check the lightbulb quests for level 16; take nearby quests that send you to kill monsters.
11:53:23  [reply] Your buddy’s answer is basically right: there’s no reliable Henesys quest chain for levels 16–20 in current GMS, so do any nearby-level lightbulb quests you already have, but training is usually quicker.
11:53:32  [echo] Lightbulb to kill monster buddies is basically right there's no
11:53:47  [heard] Nothing specifically Hennessy
11:53:50  [reply] נכון, אין כאן הוכחה שזה הנסי—תראי לי את חלון המשימות ואזהה בדיוק מה זמין.נכון, אי אפשר לזהות מכאן משימה של הנסי; פתחי את חלון המשימות כדי לראות מה זמין.
11:54:17  [heard] Where are the blue mushroom
11:54:18  [reply] The blue mushroom is on the small floating platform above-left of you, near the map’s center-left.
11:54:30  [echo] The mushroom is on the floating platform above of near the center no no the map is the mushroom on  (kept: no no the map is the mushroom on)
11:54:30  [heard] no no the map is the mushroom on
11:54:30  [heard] is the mushroom on what
11:54:32  [heard] Basic
11:54:34  [heard] one
11:54:36  [reply] נכון, זו לא הנסי—אני לא רואה מפה או פטרייה כחולה כרגע; מה שם המפה שמופיע למעלה?
11:54:48  [heard] Yes I want you to tell me I want you to tell me where is the map that
11:54:50  [heard] blue mushroom spawn at
11:54:52  [reply] Blue Mushrooms spawn in Henesys Hunting Ground I and II, just outside Henesys.
11:54:58  [echo] Spawn in ground one and two  (kept: one and two)
11:54:58  [heard] one and two
11:55:01  [heard] that's it
11:55:02  [reply] Blue Mushrooms spawn in Henesys Hunting Ground I and II; check the map names before heading there.
11:55:07  [echo] Mushroom spawn in Hennessy hunting ground  (kept: Mushroom Hennessy)
11:55:07  [heard] Mushroom Hennessy
11:55:14  [heard] well also isn't that like a map that just do mushrooms
11:55:16  [reply] כן—Hen esys Hunting Ground I ו־II הן מפות אימון לפטריות; חפשי במפה את Blue Mushrooms.
11:55:26  [echo] Blue mushrooms
11:55:32  [heard] Where is another place that the sponsor only blew my
11:55:35  [heard] phone searching the Internet please
11:55:37  [reply] Blue Mushroom spawns at Henesys Hunting Ground I and II; this screen shows Henesys, but not the map name.
11:55:40  [reply] At level 16, head to Blue Mushroom Forest 2 in Henesys—Blue Mushrooms are there; Henesys Hunting Ground I or II isn’t the right map.
11:55:44  [heard] Spawn at Hennessey hunting Ground at level 16
11:55:46  [reply] At level 16, go to Blue Mushroom Forest 2 in Henesys; Hunting Ground isn’t the right map.
11:55:49  [heard] Where is the blue mushroom Forest
11:55:51  [reply] Select Henesys on the world map; Blue Mushroom Forest 2 is just outside town.
11:55:57  [echo] On the world map for two  (kept: for two)
11:55:57  [heard] for two
11:55:59  [reply] To reach Blue Mushroom Forest 2, follow the world map’s Henesys route south of town; select the nearby mushroom-forest map.
11:56:10  [echo] Mushroom forest follow the world Genesis route sounds of town there is no self fantasies  (kept: Genesis route sounds of town there is no self fantasies)
11:56:10  [heard] Genesis route sounds of town there is no self fantasies
11:56:11  [reply] צודק—המפה מראה את ויקטוריה איילנד, לא מסלול ברור ליער; אל תנחש לפי המפה הזאת.
11:56:16  [heard] Victoria Island
11:56:18  [reply] אתה בצפון־מזרח ויקטוריה; Blue Mushroom Forest נמצאת מערבית להנסיס—חפש את השם במפת העולם.
11:56:24  [echo] Mushroom forest
11:56:31  [heard] There is no blue mushroom forest
11:56:35  [heard] search for me when can it be
11:56:37  [reply] אתה צודק—אין במפה הזאת אזור בשם Blue Mushroom Forest; חפש Blue Mushroom במפת המפלצות של הנסי.
11:56:53  [heard] How do I open in the map of the monsters
11:56:56  [reply] Press W to open the world map, then use its search field for “Blue Mushroom.”
11:57:04  [echo] Open search for mushroom
11:57:07  [heard] There is no search film here
11:57:09  [reply] נכון, במפה הזאת אין שדה חיפוש; סגור אותה ופתח את Maple Guide עם F1 כדי למצוא אזורי אימון.
11:57:28  [heard] I'm using a classic world so there is no
11:57:30  [heard] medical guide
11:57:32  [reply] Classic World doesn’t have Maple Guide; press W to open the world map, then scroll west of Henesys until you see Blue Mushroom Forest.
11:57:44  [echo] Blue mushroom forest
11:58:53  [heard] Google says to go to Génesis region and travel to the northern auto maps sign in mushroom forest area and look for a hidden portal disguised near environment of assets of specific pile of wooden logs a bush or hollowed tree trunk near the top of the left side of the map and then press up arrow to enter that hidden street
11:58:55  [heard] So guide me there
11:58:57  [reply] אתה צודק—אין מפה כזאת במסך הזה; Google טועה. במפה שמוצגת כאן אתה באזור Henesys, אז תסתכל על השם הכחול למעלה ותעבור מערבה.
11:59:14  [heard] No he says northern
11:59:17  [heard] We
11:59:17  [heard] the West
11:59:18  [reply] המפה שמוצגת היא Henesys; חפש את Blue Mushroom Forest מערבה מהעיר.
11:59:25  [heard] Hennessey
11:59:26  [reply] תשתה שיקוי עכשיו, בדחיפות!
12:00:04  [heard] How do I get
12:00:09  [heard] A slime piece and piece and table
12:00:11  [reply] You’re at Henesys, so head west to the Hunting Grounds; Blue Mushrooms drop Slime Pieces there.
12:00:14  [heard] How do I get a table
12:00:18  [heard] Table OMOK table
12:00:22  [reply] פתח את ה-shop של Henesys General Store וקנה Omok Table.
12:00:31  [heard] Well
12:00:33  [reply] Monster Park is open; press the portal on the left to enter.
12:00:39  [echo] is the portal on the left to enter
12:00:45  [heard] Here
12:00:47  [reply] The portal on the right leads to Mushroom Kingdom; walk there now.
12:00:53  [echo] The portal on the right leads to mushroom kingdom walk there now
12:01:02  [heard] No I want to go to to buy the old table
12:01:05  [heard] at OMOK table
12:01:06  [reply] Go back to Henesys, open the General Store NPC and buy the Omok Table.
12:01:17  [heard] Go to the general store and where is the general store NPC
12:01:18  [reply] General Store NPC is the pink-haired girl at the right-side stall in Henesys; walk to her now.
12:01:23  [heard] Yellowstone in PC
12:01:25  [reply] General Store NPC is the tall raccoon on the right side of the map.
12:01:36  [echo] NPC is the racoon on the right side of the right side of the map OK tell me when to stop  (kept: racoon OK tell me when to stop)
12:01:36  [heard] racoon OK tell me when to stop
12:01:40  [reply] Stop at the blue mushroom house on the right.
12:01:42  [echo] Mushroom
12:01:45  [echo] On the right
12:01:50  [heard] There is no blue mushroom
12:01:57  [heard] Well what do they say that I don't see it
12:02:01  [reply] Blue Mushroom Forest isn’t on this map; the quest marker on the left is the next step.
12:02:16  [heard] No I mean the blue mushroom house you said there is a blue mushroom house that I can buy their stuff at the table
12:02:18  [heard] oh OK table
12:02:19  [reply] The mushroom house on the right is the Henesys General Store; walk inside it.
12:02:26  [echo] The house on the right is the Hennessy general store inside
12:02:28  [heard] Communist
12:02:33  [heard] Correct the town General stopped
12:02:35  [heard] Where is the homework table
12:02:44  [heard] it's just fried chicken and hotdog and stuff and they don't allow me to buy it because this item requires
12:02:48  [heard] a grade of thumb resident of fire
12:02:56  [echo] The NPC
12:02:58  [reply] You need to talk to the NPC at the counter on the right to buy the Omok Table. הקופאית מאחורי הדלפק היא זו שמוכרת את Omok Table; לחץ עליה וקנה.
12:03:11  [heard] Raymond you mean Raymond
12:03:12  [reply] The NPC on the upper-left platform with the yellow name is the General Store clerk; walk up the left stairs to reach her.
12:03:20  [echo] The platform name is the general store
12:03:22  [echo] To reach
12:03:27  [heard] OK but she doesn't have a table here
12:03:37  [reply] Look at the left panel: click the “Crafting” tab at the top, then search “Omok Table” inside that list. No Omok Table here; open the NPC shop again and scroll the list to the bottom. Click the small table item at the top of that list; that’s the Omok Table.
12:03:38  [echo] Search table
12:03:49  [echo] Open the MPC shop again and scroll the list to the bottom click the small table item at the top of that list that's the table
12:03:55  [heard] You are no help
12:03:59  [reply] החנות שמופיעה כאן היא “Lion King’s Castle” ולא Henesys General Store; חזור למפת Henesys וחפש את החנות שם.
12:04:05  [echo] Lion King castle
12:08:33  [warning] about 23 percent MP. Even the mage mules manage better than this.
12:09:23  [warning] Mana check: about 21 percent. Time for a potion.
12:09:57  [heard] How do I make my stuff drop more items when I kill a monsters
12:09:59  [reply] You need an item drop rate buff: equip a drop coupon or join a party with a Kanna or Bishop for Holy Symbol.
12:10:04  [echo] Item
12:10:08  [echo] Join the party with a cannabis  (kept: Join the cannabis)
12:10:08  [heard] Join the cannabis
12:10:13  [heard] Yeah but how can I where is the coupon
12:10:15  [reply] The Cash Shop tab at the top right sells Drop Coupons; open it and search “Drop Coupon”.
12:10:23  [echo] Search drop coupon
12:10:42  [heard] There is no drop coupon over here
12:10:43  [reply] Drop coupons aren’t free here; buy them in the Cash Shop with NX or use the web mall.
12:10:44  [alert] עלית רמה! מזל טוב, וונוואן בוג׳יו!
12:10:53  [echo] Coupons
12:11:23  [heard] I didn't level up I don't know what you mean
12:11:25  [reply] The game shows a level-up effect, but the displayed level is still 16; it was a visual flash only.
12:11:31  [echo] Display 16
12:11:33  [echo] Visual only
12:11:36  [heard] Remember that
12:11:38  [reply] הרמה עדיין 16, התמונה לא מראה עליית רמה חדשה.
12:11:45  [heard] Not a damn shit remember that and then improve yourself
12:11:46  [reply] The screen still shows the “You got your attire” message, not a level-up; the level stays 16.
12:11:52  [echo] The screen still shows that you got your message
12:11:55  [echo] The 16
12:11:57  [heard] what about now
12:11:59  [reply] The level-up effect is still on screen; your level bar shows 16, so nothing changed.
12:12:08  [echo] The level of effect is still on screen your level bar shows 16 so nothing changed no it doesn't  (kept: of no it doesn't)
12:12:08  [heard] of no it doesn't
12:12:09  [reply] The level-up effect is on screen but your character is still level 16; it was just a visual flash.
12:12:19  [echo] The level of effect is on screen but your character is still level 16 it was just a visual flash check again
12:12:28  [heard] Check again
12:12:29  [reply] The level-up effect is still on screen; the character portrait shows level 16, so the level didn’t change.
12:12:38  [echo] The effect on the portrait level 16 didn't change
12:12:40  [alert] 17 already? You're flying.
12:12:46  [heard] Check carefully if the level you're flying that's it now
12:12:55  [heard] please congratulate me with the more enthusiastic response that I graduated to level 17
12:12:56  [reply] מזל טוב! עלית לרמה 17, כל הכבוד, וונוואן בוג׳יו!
12:13:12  [heard] More enthusiastic
12:13:13  [reply] מזל טוב, וונוואן בוג׳יו! עלית לרמה 17, כל הכבוד!
12:13:24  [heard] Enthusiastic get this for one minute
12:13:25  [reply] Still the same.
12:13:29  [echo] Still the same
12:13:36  [heard] Be enthusiastic and congratulate me for 10 sentences straight
12:13:38  [reply] מזל טוב על רמה 17, מיכאל! וואו, איזה יופי! כל הכבוד על ההתקדמות! אתה טס קדימה! רמה 17 נראית עליך מעולה! תמשיך כך, קוסם תותח! איזה הישג כיפי! מגיע לך לחגוג את זה! עוד רמה בדרך! אלוף, תן בראש!
12:14:06  [heard] Mouth mole
12:14:14  [reply] הבנתי, זה ה־Mouth Mole. ביומן שלי הוא נשמר.
12:14:18  [heard] Is auntie is there a mouse
12:14:22  [heard] I want you to congratulate me more
12:14:24  [reply] מזל טוב, מיכאל! איזה כיף, אתה פשוט טס קדימה!
12:14:27  [echo] Don't
12:14:33  [heard] Don't stop don't stop don't stop congratulate me until I say stop
12:14:37  [reply] איזה יופי, תמשיך להפציץ!
12:14:45  [heard] Continue
12:14:46  [reply] מזל טוב, מיכאל! רמה 17, איזה הישג ענק—תמשיך להפציץ!
12:14:49  [heard] Without stopping until
12:14:55  [heard] Continue with without stopping until I say the world stop without stopping
12:14:56  [reply] מזל טוב, מיכאל! רמה 17, איזה הישג אדיר—אתה פשוט תותח!
12:15:11  [heard] At least 100 sentences of congratulations
12:15:14  [reply] מזל טוב, מיכאל! רמה 17, איזה הישג אדיר! רמה 17, איזה הישג אדיר!
12:15:30  [heard] 100 sentences
12:15:31  [reply] איזה הישג אדיר, מיכאל! מזל טוב על רמה 17!
12:15:44  [heard] That's one sentence I want at least 40 sentences
12:15:45  [reply] מזל טוב על רמה 17, מיכאל—איזה הישג אדיר!
12:16:11  [alert] עלית רמה! מזל טוב, וונוואן בוג׳יו!
12:16:17  [heard] A little
12:16:18  [reply] מזל טוב, מיכאל! עוד רמה נהדרת בדרך—אתה אלוף!
12:16:38  [alert] Level up! Congrats.
12:16:47  [heard] At least 1000 walls compressed at least 1000 words a few congratulating me right now
12:16:48  [reply] כל הכבוד—איזה הישג נהדר!
12:16:56  [heard] 1000 words 1000 words
12:16:57  [reply] מזל טוב, מיכאל! איזה הישג אדיר! כל הכבוד, קוסם תותח!
12:17:09  [heard] That's not 1000 words
12:17:13  [heard] from your mistakes
12:17:14  [reply] אתה צודק—לא נתתי אלף מילים; סליחה, מזל טוב ענק על רמה 17!
12:17:32  [alert] Level up! Congrats.
12:17:34  [heard] Give me 1000
12:17:36  [reply] מזל טוב על רמה 17, מיכאל—אתה קוסם תותח, וכל הכבוד על ההתקדמות שלך!
12:20:12  [alert] Level up! Congrats.
12:20:16  [echo] Congrats
12:21:04  [warning] Your MP's down to about 23 percent.
12:21:09  [heard] MP 23%
12:21:12  [reply] Got it—MP’s at 23%.
12:21:16  [warning] Still low on mana, about 10 percent. Blue potion!
12:21:22  [echo] 23% still owe about 10% potion  (kept: 23% still owe)
12:21:22  [heard] 23% still owe
12:21:25  [reply] האזהרה תופעל כשה־MP ירד מתחת ל־23%; כרגע הוא בערך 5%, אז שתה שיקוי MP עכשיו!
12:21:29  [echo] M
12:21:33  [echo] Cuisine
12:21:59  [warning] You're almost out of MP. Pot before the skills stop.
12:22:05  [echo] You're almost out of MP popped before the skill stock  (kept: popped skill stock)
12:22:05  [heard] popped skill stock
12:22:07  [reply] איזה יופי, השתמשת ב־Skill Stock! אלוף, מיכאל!
12:22:11  [heard] Is there a
12:22:13  [reply] ממה אתה שואל—מהיצור שעל המסך, ה־Mouth Mole?
12:22:58  [alert] Level up! Congrats.
12:23:02  [echo] It's level up congrats
12:23:04  [warning] about 16 percent MP. Top it up when you get a second.
12:23:11  [echo] About 16% MP it up when you get a second
12:24:46  [alert] Level up! Congrats.
12:24:50  [warning] Heads up, only about 20 percent MP left.
12:24:55  [echo] Level up congrats heads up only about 20% MP left
12:25:02  [warning] Mana check again: about 22 percent. Top it up.
12:25:08  [echo] My check again about 22% topping up  (kept: My topping up)
12:25:08  [heard] My topping up
12:25:10  [reply] Good, MP topped. Level up! Congrats.
12:25:16  [echo] Top level up congrats
12:25:24  [heard] What's the best skill to use to improve right now
12:25:25  [reply] הכי כדאי להשתמש ב־Magic Claw—מובינג מהיר ויעיל ברמה הזאת.
12:25:37  [heard] What is the best to improve according to what you say on the screen on my skill inventory
12:25:38  [reply] הכי כדאי להמשיך עם Magic Claw—הוא כבר מסומן כמומלץ.
12:25:43  [heard] Magical
12:25:48  [heard] Yeah but how much should I raising to
12:25:49  [reply] Magic Claw is maxed, so put the rest into Magic Guard—keeps you alive.
12:25:52  [heard] Give it
12:25:56  [heard] should I give it a free skill points
12:25:58  [reply] כן, תן את נקודת הכישרון החופשית ל־Magic Claw.
12:26:13  [heard] OK and what about the character stats what to give the five points ability points to
12:26:17  [reply] Give the five points to INT. Put all five points into INT.
12:26:22  [heard] INTINT
12:26:25  [echo] points to INTINTT
12:26:25  [reply] Level up! Congrats. INT's already maxed for your level—save the next points. Save the points—INT's already maxed.
12:26:33  [echo] Congrats is already maxed for your level save the next point eye of the  (kept: Congrats is point eye of the)
12:26:33  [heard] Congrats is point eye of the
12:26:34  [reply] Level up! Congrats. Head left to that quest marker.
12:26:35  [warning] Mana's getting low, about 21 percent. Drink a blue one.
12:26:45  [heard] Level out congrats not leveled up first not darkness about 21% blue one
12:26:46  [reply] נכון, לא עלית רמה—ה־MP בערך 62%, אין צורך בשיקוי כרגע.
12:27:01  [heard] Yeah but what's INT in the character that
12:27:02  [reply] INT היא אינטליגנציה—הנתון הראשי של קוסם, והיא מחזקת את הקסם שלך.
12:27:09  [echo] INT intelligence
12:27:16  [heard] Do you mean I get I get a stronger magic and
12:27:20  [heard] Do more damage that way
12:27:22  [reply] כן—INT מחזקת את הקסם שלך, וככל שהיא גבוהה יותר, בדרך כלל תגרום יותר נזק.
12:27:29  [echo] I took
12:27:32  [warning] MP's at about 18 percent, might want a potion.
12:27:35  [echo] About
12:27:38  [echo] 18%
12:27:46  [heard] I don't have I use lemons
12:27:48  [heard] remember that
12:27:51  [reply] צודק—יש לך לימונים, לא שיקויי MP; השתמש בהם כשצריך.
12:28:01  [heard] What stuff do I have in the mini keyboard layout
12:28:04  [heard] what do you see
12:28:06  [reply] בחפיסה הקטנה נראה שיש שיקויים ומיומנויות, אבל האייקונים זעירים מדי לזיהוי ודאי.
12:28:15  [alert] עלית רמה! מזל טוב, וונוואן בוג׳יו!
12:28:20  [heard] Try to guess I didn't
12:28:24  [heard] stop it I didn't leveled up
12:28:25  [reply] נכון, לא עלית רמה; האפקט הטעה אותי.
12:28:30  [alert] עלית רמה! מזל טוב, וונוואן בוג׳יו!
12:28:45  [heard] Listen don't don't say if I
12:28:54  [heard] don't tell me about my level nothing OK from now on remember to don't do not tell me that
12:28:57  [reply] מובן, לא אזכיר עליית רמה אלא אם תבקש.
12:29:05  [warning] about 20 percent MP. Top it up when you get a second.
12:29:07  [alert] עלית רמה! מזל טוב, וונוואן בוג׳יו!
12:29:12  [echo] About 20% MP when you get a second
12:29:16  [heard] You wouldn't do that
12:29:19  [reply] צודק. לא אזכיר את הרמה או עליית רמה אלא אם תבקש.
12:29:25  [heard] Lose
12:29:26  [reply] הבנתי—לא אדבר על הרמה שלך. מה הפסדת במשחק?
12:30:08  [alert] עלית רמה! מזל טוב, וונוואן בוג׳יו!
12:32:08  [alert] עלית רמה! מזל טוב, וונוואן בוג׳יו!
12:32:12  [heard] A little
12:32:14  [reply] רק רגע—עלו שוב אפקטים, אבל לא אזכיר רמות.
12:34:19  [warning] Heads up, only about 23 percent MP left.
12:34:24  [echo] Heads up only about 23% MP list
12:36:08  [alert] עלית רמה! מזל טוב, וונוואן בוג׳יו!
12:36:17  [warning] Mana check: about 21 percent. Time for a potion.
12:36:23  [echo] About 21% for a potion
12:36:29  [warning] Only about 14 percent MP, still. Drink.
12:36:35  [echo] Only about 14% and still drink
12:39:50  [warning] MP's at about 20 percent, might want a potion.
12:40:17  [warning] Your MP's down to about 15 percent.
12:41:13  [warning] Mana's getting low, about 9 percent. Drink a blue one.
12:42:02  [warning] You're almost out of MP. Pot before the skills stop.
12:43:13  [warning] Your MP's down to about 23 percent.
12:44:08  [alert] עלית רמה! מזל טוב, וונוואן בוג׳יו!
12:44:12  [warning] Mana check: about 20 percent. Time for a potion.
12:45:08  [alert] Level up! Congrats.
12:46:44  [heard] How do I make slime shoes
12:46:46  [reply] To craft Slime Shoes you need 20 Slime Bubbles, 5 Blue Snail Shells, and 3 Leather, then talk to any equipment smith.
12:46:55  [reply] That answer’s wrong: Slime Shoes aren’t crafted in Classic World—Squishy Shoes can drop from King Slime in Kerning PQ.
12:46:59  [echo] Blue snail shells and 311 then talk to any equivalent to that answer is wrong  (kept: 311 equivalent to that answer is wrong)
12:46:59  [heard] 311 equivalent to that answer is wrong
12:47:02  [heard] What's correct
12:47:04  [reply] השם שלך הוא WANWANBUJIO, לא ANWANBUIIO. תיקנתי.
12:47:13  [heard] Yes but how to make slime
12:47:15  [reply] תיקנתי. כדי להכין Slime Shoes אתה צריך 20 Slime Bubbles, 5 Blue Snail Shells ו-3 Leather, ואז לדבר עם נפח ציוד.
12:47:24  [echo] Bubble blue snail shell  (kept: Bubble shell)
12:47:24  [heard] Bubble shell
12:47:28  [reply] אסוף Slime Bubbles מסליימים, לא Blue Snail Shells—הקליפה הכחולה היא חומר נפרד. אסוף 20 Slime Bubbles מסליימים; צריך גם 5 Blue Snail Shells ו-3 Leather.
12:47:31  [echo] Soft slime bubbles
12:47:49  [heard] What model do I need except Islam bubbles
12:47:52  [reply] צריך גם 5 Blue Snail Shells ו־3 Leather, חוץ מ־20 Slime Bubbles. צריך גם 5 Blue Snail Shells ו־3 Leather.
12:48:02  [echo] Snail
12:48:23  [heard] And then what to go enhances to crafty
12:48:23  [heard] is
12:48:29  [reply] אחרי החומרים, גש לנפח ציוד ובחר ביצירה של Slime Shoes.
12:48:35  [heard] Where is the
12:48:37  [reply] ליד שולחן האומוק, פתח את חלון היצירה ולחץ על הכפתור הכחול שבתחתית.
12:53:13  [alert] Level up! Congrats.
12:53:17  [echo] Level up congrats
12:55:44  [alert] Level up! Congrats.
12:55:49  [echo] Congrats
12:56:00  [alert] Level up! Congrats.
12:56:04  [echo] Level up congrats
12:58:36  [alert] עלית רמה! מזל טוב, וונוואן בוג׳יו!
12:58:40  [heard] Little
12:58:42  [reply] הדמות שלך נקראת WANWANBUJIO, לא ANWANBUIIO. תיקנתי.
12:58:52  [alert] עלית רמה! מזל טוב, וונוואן בוג׳יו!
12:58:55  [echo] A little
12:59:22  [alert] עלית רמה! מזל טוב, וונוואן בוג׳יו!
13:00:41  [warning] Heads up, only about 21 percent MP left.
13:00:47  [warning] Careful, your HP's down to about 36 percent. Drink a potion!
13:01:17  [alert] עלית רמה! מזל טוב, וונוואן בוג׳יו!
13:01:18  [warning] about 16 percent MP. Top it up when you get a second.
13:01:56  [warning] HP's at about 42 percent, potion time!
13:02:54  [warning] Hey, HP's getting scary. Potion, please!
13:04:16  [warning] You're almost out of MP. Pot before the skills stop.
13:04:43  [alert] עלית רמה! מזל טוב, וונוואן בוג׳יו!
13:05:00  [warning] Whoa, about 46 percent HP. Drink something!
13:05:14  [warning] about 41 percent left on the bar. Top it up before the next hit.
13:05:17  [warning] Mana's getting low, about 12 percent. Drink a blue one.
13:06:19  [warning] MP's at about 5 percent, might want a potion.
13:08:16  [warning] You're low, 46 percent. Don't push it, drink.
13:08:28  [warning] Calling it again: 46 percent HP. Drink before the next hit.
13:08:58  [warning] Is something stopping you from potting? 46 percent HP.
13:09:58  [warning] Still low, 46 percent. Please drink something.
13:10:04  [alert] עלית רמה! מזל טוב, וונוואן בוג׳יו!
13:11:58  [warning] I said potion! 46 percent HP. Drink.
13:13:58  [warning] Hey, 46 percent HP is still scary. Pot!
13:15:58  [warning] That bar's still red, 46 percent. Drink up.
13:17:58  [warning] Didn't you hear me? 46 percent HP. Potion, now.
13:18:49  [alert] עלית רמה! מזל טוב, וונוואן בוג׳יו!
13:19:58  [warning] That bar's still red, 46 percent. Drink up.
13:21:58  [warning] Calling it again: 46 percent HP. Drink before the next hit.
13:23:58  [warning] Still low, 46 percent. Please drink something.
13:35:58  [warning] Didn't you hear me? about 14 percent HP. Potion, now.
13:37:58  [warning] Hey, about 7 percent HP is still scary. Pot!
13:39:05  [alert] עלית רמה! מזל טוב, וונוואן בוג׳יו!
13:39:29  [alert] עלית רמה! מזל טוב, וונוואן בוג׳יו!
"##;

    /// Its own voice is never the player, replayed over his evening: of the
    /// 85 echoes, 19 had a remainder kept as his words at f9b6047 (his name
    /// learned as "Miha", a mark from "question mark", MP warnings at 23%
    /// from "23% still owe"); now two at most, and only his own words.
    #[test]
    fn its_own_voice_is_never_the_player_through_the_owners_evening() {
        let hms = |time: &str| -> f64 {
            let p: Vec<f64> = time.split(':').map(|x| x.parse().unwrap()).collect();
            p[0] * 3600.0 + p[1] * 60.0 + p[2]
        };
        let mut c = Companion::seeded(Settings::default(), SEED);
        let (mut echoes, mut kept_in_log, mut kept_before) = (0, 0, 0);
        let mut kept_after: Vec<(String, String)> = Vec::new();
        let mut changed: Vec<String> = Vec::new();
        let mut last_kept: Option<String> = None;
        for row in OWNER_EVENING.lines().filter(|r| !r.is_empty()) {
            let (time, rest) = row.split_once("  [").unwrap();
            let (kind, text) = rest.split_once("] ").unwrap();
            let t = hms(time);
            match kind {
                "reply" | "warning" | "alert" => {
                    c.remember_spoken(t, text.trim_start_matches("[ silent ]"));
                    last_kept = None;
                }
                "echo" => {
                    echoes += 1;
                    let (heard, kept) = match text.split_once("  (kept: ") {
                        Some((heard, kept)) => (heard, Some(kept.trim_end_matches(')'))),
                        None => (text, None),
                    };
                    last_kept = kept.map(str::to_string);
                    // (Dropped whole then, dropped now: the rule only takes
                    // away. The main loop's `need` did the rest then.)
                    if kept.is_none() {
                        continue;
                    }
                    kept_in_log += 1;
                    // (The rule before, replayed: strip_echo alone.)
                    if c.strip_echo(t, heard).is_some_and(|rest| rest != heard) {
                        kept_before += 1;
                    }
                    if let Some(rest) = c.own_words(t, heard, 0) {
                        kept_after.push((time.to_string(), rest));
                    }
                }
                "heard" => {
                    // (The part of an echo kept as his: counted above.)
                    if last_kept.take().as_deref() == Some(text) {
                        continue;
                    }
                    if c.strip_echo(t, text).as_deref() == Some(text)
                        && c.own_words(t, text, 0).as_deref() != Some(text)
                    {
                        changed.push(text.to_string());
                    }
                }
                _ => unreachable!("{row}"),
            }
        }
        eprintln!(
            "echoes {echoes}, kept in the log {kept_in_log}, by strip_echo {kept_before}, now {kept_after:?}"
        );
        eprintln!("his sentences now dropped or cut: {changed:?}");
        assert_eq!((echoes, kept_in_log), (85, 19));
        assert!(kept_before >= 15, "{kept_before}");
        // Kept now: only his words — "OK tell me when to stop" (he was
        // walking to the NPC). Not "my name is … Miha", not "question
        // mark", not "23% still owe", not a word with a name in it.
        assert!(kept_after.len() <= 1, "{kept_after:?}");
        for (time, rest) in &kept_after {
            assert!(
                rest.ends_with("OK tell me when to stop"),
                "{time}: {rest:?}"
            );
        }
        // His sentences: its own voice taken for his before — "I'll Miha"
        // (it had said "I'll call you Miha"), "MP 23%" (it had said "MP's
        // down to about 23 percent") — dropped now; at most two of his own
        // lost with them (mostly its words, said right after it).
        for own in ["I'll Miha", "MP 23%"] {
            assert!(changed.iter().any(|c| c == own), "{own}: {changed:?}");
        }
        for lost in &changed {
            assert!(
                [
                    "I'll Miha",
                    "MP 23%",
                    "is the mushroom on what",
                    "Spawn at Hennessey hunting Ground at level 16",
                ]
                .contains(&lost.as_str()),
                "{lost:?}: {changed:?}"
            );
        }
    }
}
