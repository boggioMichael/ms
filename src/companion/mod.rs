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
    /// Warn when MP falls below this percent (0: never).
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
            mp_low: 15.0,
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
    /// How long neither HP nor EXP has moved, with the game in view.
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
/// was at the last line: then once more, at once. Unanswered (the bar stays
/// low), it is said again after [`FALL_COOLDOWNS`], longer each time. A
/// grind of hit, pot, hit, every 8 s, had the line at every hit: 75 "pot
/// now" in ten minutes, in seven wordings — the beating's nag, one rule
/// down.
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
    /// When the bar was last under the mark.
    low_at: f64,
    /// When the line was last said, the reading then, how many lines this
    /// fight, and whether a potion has answered the last — frames in a
    /// row with the bar back up by a potion's worth: one is a misread
    /// (see [`HELD_FRAMES`]).
    told: f64,
    told_at: f32,
    lines: u32,
    answered: bool,
    up_frames: u32,
    /// EXP when the fight's first line was said.
    exp_at_first: Option<f32>,
    /// The bar is not believed, and since when it has read above the mark
    /// (never: minus infinity).
    doubted: bool,
    above_since: f64,
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

impl Low {
    fn new() -> Self {
        Self {
            frames: 0,
            since: 0.0,
            low_at: f64::NEG_INFINITY,
            told: f64::NEG_INFINITY,
            told_at: f32::INFINITY,
            lines: 0,
            answered: false,
            up_frames: 0,
            exp_at_first: None,
            doubted: false,
            above_since: f64::NEG_INFINITY,
        }
    }

    /// Takes a reading (and EXP as read with it); what it calls for.
    /// `fought_at` is when the fight was last seen going on some other way
    /// (HP falling fast; never, for MP).
    fn due(&mut self, now: f64, percent: f32, mark: f32, fought_at: f64, exp: Option<f32>) -> Due {
        if now - self.low_at.max(fought_at) > FIGHT_OVER_SECS {
            self.lines = 0;
        }
        if percent < mark {
            if self.frames == 0 {
                self.since = now;
            }
            self.frames += 1;
            self.low_at = now;
        } else {
            self.frames = 0;
        }
        if percent >= self.told_at + POTTED {
            self.up_frames += 1;
            if self.up_frames >= HELD_FRAMES {
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
                if now - self.told < wait {
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
        self.told = now;
        self.told_at = percent;
        self.lines += 1;
        self.answered = false;
        self.up_frames = 0;
    }

    /// Whether a line due now would repeat one of this fight's that went
    /// unanswered (the card may say "still"); a first line, or one more
    /// because the bar went lower after a potion, stands on its own.
    fn repeating(&self) -> bool {
        self.lines > 0 && !self.answered
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
    /// with the highest EXP seen since, and whether HP has been back up by
    /// a potion's worth since — frames in a row with it so: one is a
    /// misread (see [`HELD_FRAMES`]).
    unanswered: u32,
    alert_at: f64,
    hp_at_alert: Option<f32>,
    hp_up_frames: u32,
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
    /// When HP or EXP last moved noticeably, and what they were then.
    changed_at: f64,
    hp_at_change: Option<f32>,
    exp_at_change: Option<f32>,
    /// When the first frame came, since when the game has been in view
    /// (the last time it was found, or found again), and whether "still
    /// there?" was asked this session.
    started_at: Option<f64>,
    view_since: f64,
    asked_still_there: bool,
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

/// A death read from the bar's fill (not the printed number) must last
/// this long: a dialog over the bar reads as an empty bar too.
const ZERO_HOLD_SECS: f64 = 2.0;

/// Alerts said with no sign of life from the player — a word, HP going
/// back up (a potion), EXP gained, a level — before the rest are held…
/// (Four: a beating and the low line's first three — at 0, 5, 17 and 47 s
/// — and the hold where the next would come, a minute after; six ran a
/// pet's death to six lines in six minutes to a room that may be empty.
/// The same count as [`DOUBT_AFTER_LINES`], which is about a bar that
/// lies while EXP comes in; this one is about nothing moving at all.)
const UNANSWERED_MAX: u32 = 4;
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

/// How long HP or MP must stay low before it is said, in seconds (and at
/// least three frames): a moment's misread is not worth a warning.
const LOW_HOLD_SECS: f64 = 0.6;

/// HP warnings move sooner after deaths no warning came before, up to here.
/// (A warning came before a death when a low line was said in the fight
/// the death ends — [`Low::warned_this_fight`]: the warning moves sooner
/// only when none did, and the death line may say "told you" only when
/// one did.)
const SOONEST_WARNING: f32 = 50.0;

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
/// A player who handles the beating is trusted with it: after this many
/// fights in a row with a potion within [`TRUST_POT_SECS`] of the line,
/// the next fall is watched instead of shouted, and shouted after all
/// when HP goes under their usual floor (the lowest of the fights they
/// handled) by [`TRUST_MARGIN`] — the depth deserves it; potted in time,
/// it counts like the rest and the floor follows it — or when no potion
/// comes within the same time of the fall. Only a late potion, none, or
/// a death ends the trust. A friend stops saying "back off" the fourth
/// time you handle it (34 beatings in 65 min for a player who potted
/// within 3 s every time; 9 in 10 handled fights when a deeper one
/// started the count over).
const TRUST_FIGHTS: u32 = 3;
const TRUST_POT_SECS: f64 = 3.0;
const TRUST_MARGIN: f32 = 5.0;

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
            "Mana's at {}. Why is it always mana with you?",
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

    /// HP at zero in a fight a low warning was said in: it did say to
    /// pot, and the line may say so (how long ago it said it, it may not:
    /// the warning can be most of a minute old).
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

    /// Alerts held for want of an answer. A card that counts the warnings
    /// counts [`super::UNANSWERED_MAX`] of them.
    pub const HOLD: [&[&str]; 3] = [
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

    /// Every deck, by name, for tests and tools.
    pub const ALL: &[(&str, [&[&str]; 3])] = &[
        ("beating", BEATING),
        ("HP low", HP_LOW),
        ("HP low, again", HP_LOW_AGAIN),
        ("MP low", MP_LOW),
        ("MP low, again", MP_LOW_AGAIN),
        ("death", DEATH),
        ("death, warned", DEATH_WARNED),
        ("sooner", SOONER),
        ("level up", LEVEL_UP),
        ("game seen", SEEN),
        ("game lost", LOST),
        ("game seen again", AGAIN),
        ("muted", MUTED),
        ("unmuted", UNMUTED),
        ("warnings held", HOLD),
        ("still there", STILL_THERE),
        ("misread", MISREAD),
    ];

    /// The decks in [`ALL`] that are dealt a few times a night at most
    /// (the mark moves up four times and no more; a bar is doubted once a
    /// fight, after four lines): three ways to say it is plenty, where the
    /// others have six.
    pub const SHORT: &[&str] = &["sooner", "misread"];
}

/// Something worth speaking up about, seen in a frame; its line is dealt
/// once it is known to be said (`pace` holds alerts nobody answers).
#[derive(Debug, Clone, Copy, PartialEq)]
enum Alert {
    Beating(Gauge),
    /// A low bar, and whether the fight's last line went unanswered (the
    /// card may then say "still").
    HpLow {
        hp: Gauge,
        again: bool,
    },
    MpLow {
        mp: Gauge,
        again: bool,
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

/// A [`Deck`] per situation, so that each is dealt on its own.
#[derive(Debug)]
struct Decks {
    beating: Deck,
    hp_low: Deck,
    hp_low_again: Deck,
    mp_low: Deck,
    mp_low_again: Deck,
    death: Deck,
    death_warned: Deck,
    sooner: Deck,
    level_up: Deck,
    seen: Deck,
    lost: Deck,
    again: Deck,
    muted: Deck,
    unmuted: Deck,
    hold: Deck,
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
            mp_low: Deck::seeded(seed),
            mp_low_again: Deck::seeded(seed),
            death: Deck::seeded(seed),
            death_warned: Deck::seeded(seed),
            sooner: Deck::seeded(seed),
            level_up: Deck::seeded(seed),
            seen: Deck::seeded(seed),
            lost: Deck::seeded(seed),
            again: Deck::seeded(seed),
            muted: Deck::seeded(seed),
            unmuted: Deck::seeded(seed),
            hold: Deck::seeded(seed),
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
            &mut self.mp_low,
            &mut self.mp_low_again,
            &mut self.death,
            &mut self.death_warned,
            &mut self.sooner,
            &mut self.level_up,
            &mut self.seen,
            &mut self.lost,
            &mut self.again,
            &mut self.muted,
            &mut self.unmuted,
            &mut self.hold,
            &mut self.still_there,
            &mut self.misread,
        ] {
            deck.lead_first(lead_first);
        }
    }
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
            hp_up_frames: 0,
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
            started_at: None,
            view_since: f64::NEG_INFINITY,
            asked_still_there: false,
        }
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
            level_ups: self.exp.levels_gained(),
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

    /// When HP or EXP last moved by more than the bar's flicker: the game
    /// is quiet while neither does.
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
        if hp_moved || exp_moved {
            self.changed_at = now;
            self.hp_at_change = hp.or(self.hp_at_change);
            self.exp_at_change = exp.map(|g| g.percent).or(self.exp_at_change);
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
        self.started_at.get_or_insert(now);
        let mut out = Vec::new();
        let mut alerts = Vec::new();
        self.track_window(now, &obs, &mut out);
        if obs.game.is_seen() {
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
            let line = self
                .decks
                .still_there
                .deal(self.settings.attitude, lines::STILL_THERE);
            out.push(Action::Say(Say::alert(line)));
        }
    }

    /// The line for an alert, dealt from its deck.
    fn line(&mut self, alert: Alert) -> String {
        let attitude = self.settings.attitude;
        match alert {
            Alert::Beating(hp) => self
                .decks
                .beating
                .deal(attitude, lines::BEATING)
                .replace("{}", &low_words(hp)),
            // A fight's first line stands alone; one that repeats an
            // unanswered line may say "still".
            Alert::HpLow { hp, again } => if again {
                self.decks.hp_low_again.deal(attitude, lines::HP_LOW_AGAIN)
            } else {
                self.decks.hp_low.deal(attitude, lines::HP_LOW)
            }
            .replace("{}", &low_words(hp)),
            Alert::MpLow { mp, again } => if again {
                self.decks.mp_low_again.deal(attitude, lines::MP_LOW_AGAIN)
            } else {
                self.decks.mp_low.deal(attitude, lines::MP_LOW)
            }
            .replace("{}", &low_words(mp)),
            Alert::Death { warned, sooner } => {
                // "Told you to pot" only when it did: a death with no
                // warning before it is dealt from the deck that makes no
                // such claim (one night a one-shot from 80% got "Dead. Told
                // you to pot. I'll warn you sooner from now on").
                let mut line = if warned {
                    self.decks.death_warned.deal(attitude, lines::DEATH_WARNED)
                } else {
                    self.decks.death.deal(attitude, lines::DEATH)
                }
                .to_string();
                // The mark moving, in the same voice.
                if let Some(sooner) = sooner {
                    let card = self
                        .decks
                        .sooner
                        .deal(attitude, lines::SOONER)
                        .replace("{}", &format!("{sooner:.0}"));
                    line.push(' ');
                    line.push_str(&card);
                }
                line
            }
            Alert::LevelUp(level) => self
                .decks
                .level_up
                .deal(attitude, lines::LEVEL_UP)
                .replace("{}", &level.to_string()),
        }
    }

    /// Warnings that nothing answers are held: after [`UNANSWERED_MAX`] of
    /// them with no sign of life from the player — not a word, no potion
    /// (a reading that held, not a frame), no EXP gained, no level — the
    /// rest wait [`HOLD_SECS`] (and the player is told once why), unless
    /// the player turns up sooner. One night of a misread bar ran to 3,400
    /// warnings said to an empty room.
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
        let exp = obs.exp.map(|g| g.percent);
        let above = |value: Option<f32>, then: Option<f32>, by: f32| matches!((value, then), (Some(value), Some(then)) if value >= then + by);
        if obs.game.is_seen() {
            // (A potion's worth back is a sign of life once it has held,
            // as it answers the warning itself: one frame read high is a
            // misread, and it lifted the hold.)
            if above(hp, self.hp_at_alert, POTTED) {
                self.hp_up_frames += 1;
                if self.hp_up_frames >= HELD_FRAMES {
                    self.potted = true;
                }
            } else {
                self.hp_up_frames = 0;
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
        let leveled = obs.level.is_some() && obs.level != self.level_at_alert;
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
        self.potted = false;
        self.exp_at_alert = exp;
        self.exp_peak = exp;
        self.level_at_alert = obs.level;
        if self.unanswered > UNANSWERED_MAX {
            self.hold_until = now + HOLD_SECS;
            if now - self.hold_told >= HOLD_TOLD_EVERY {
                self.hold_told = now;
                let line = self.decks.hold.deal(self.settings.attitude, lines::HOLD);
                out.push(Action::Say(Say::info(line, true)));
            }
            return news(alerts);
        }
        alerts
    }

    fn track_window(&mut self, now: f64, obs: &Observation, out: &mut Vec<Action>) {
        let attitude = self.settings.attitude;
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
                    self.decks.seen.deal(attitude, lines::SEEN).to_string()
                } else {
                    format!("I can see MapleStory (the window \"{}\").", title.trim())
                };
                out.push(Action::Say(Say::info(line, true)));
            } else if self.announced_lost {
                let line = self.decks.again.deal(attitude, lines::AGAIN);
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
                _ => self.decks.lost.deal(attitude, lines::LOST).to_string(),
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
        if !hp.read && self.hp_steady.unsteady(now, hp.percent) {
            self.falling_frames = 0;
            self.low_hp.frames = 0;
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
                self.deaths += 1;
                self.last_death = now;
                // (A death is the end of trusting them with a beating.)
                self.trust = 0;
                self.trust_floor = f32::INFINITY;
                self.watch = None;
                // Whether it told them to pot in this fight.
                let warned = self.low_hp.warned_this_fight(now, self.fall_at);
                let sooner = self.sooner_warning(now, warned);
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
            // A new life is a new fight.
            self.low_hp = Low::new();
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
            let under_floor = hp.percent < self.trust_floor - TRUST_MARGIN;
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
                let again = self.low_hp.repeating();
                self.low_hp.said(now, hp.percent, exp);
                alerts.push(Alert::HpLow { hp, again });
            }
            Due::Misread => out.push(Action::Say(self.misread("HP"))),
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
    fn misread(&mut self, bar: &str) -> Say {
        let line = self
            .decks
            .misread
            .deal(self.settings.attitude, lines::MISREAD)
            .replace("{}", bar);
        Say::info(line, true)
    }

    /// Died without a warning, though HP went down through where a sooner
    /// one would have come: warn sooner from now on (five points, up to
    /// half the bar). Not when warnings are off, nor after a sudden death
    /// (no warning would have helped).
    fn sooner_warning(&self, now: f64, warned: bool) -> Option<f32> {
        let low = self.settings.hp_low;
        if low <= 0.0 || low >= SOONEST_WARNING {
            return None;
        }
        let lowest = self
            .hp_lately
            .iter()
            .filter(|(t, _)| now - t <= 10.0)
            .map(|(_, p)| *p)
            .fold(f32::INFINITY, f32::min);
        (!warned && lowest < low + 20.0).then(|| (low + 5.0).min(SOONEST_WARNING))
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
                out.push(Action::Say(Say::info(
                    "My MP readings are jumping around, so I'm holding the MP warnings until they settle.",
                    false,
                )));
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
                let again = self.low_mp.repeating();
                self.low_mp.said(now, mp.percent, exp);
                alerts.push(Alert::MpLow { mp, again });
            }
            Due::Misread => out.push(Action::Say(self.misread("MP"))),
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
                                "another character"
                            } else if lower {
                                "a lower level: another character, or misread"
                            } else if top == Some(level) {
                                "back to a level seen before: not celebrated again"
                            } else {
                                "not one level up: not celebrated"
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

    /// An answer it knows without a model, when `sentence` asks for one of
    /// the player's own numbers ([`instant::asks`]): dealt from the
    /// session's decks (one per kind of answer), like its other lines.
    /// `None` when it does not, or the number isn't known right now.
    pub fn instant(&mut self, sentence: &str) -> Option<String> {
        let ask = instant::asks(sentence)?;
        instant::answer(
            ask,
            sentence,
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
                    Action::Say(Say::reply(format!("Marked. That's mark {}.", self.marks))),
                ]
            }
            Command::Mute => {
                self.muted = true;
                let line = self.decks.muted.deal(self.settings.attitude, lines::MUTED);
                vec![Action::Say(Say::info(line, false)), Action::SetMuted(true)]
            }
            Command::Unmute => {
                self.muted = false;
                let line = self
                    .decks
                    .unmuted
                    .deal(self.settings.attitude, lines::UNMUTED);
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
        Attitude::Friendly
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

    /// Whether `line` is one of `deck`'s (the situation, not the words).
    fn from(deck: [&[&str]; 3], line: &str) -> bool {
        variant(deck, line).is_some()
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
        // Still low, nothing done about it: not for a while…
        for i in 0..11 {
            assert!(said(&c.observe(21.8 + i as f64, frame(18.0, 90.0, 10.0))).is_empty());
        }
        // …then said again, put another way — from the deck that may say
        // "still" (and again after longer waits: see the beating's).
        let nagged = said(&c.observe(33.8, frame(18.0, 90.0, 10.0)));
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
        // is no potion either: the unanswered cadence goes on as if it
        // had not been (0.6, 12.6, 42.6, 102.6, and the hold where the
        // fifth line would come, at 222.6 — one frame is no sign of life
        // to the hold any more than it is an answer to the line). One
        // night this was "Back off, you're getting shredded. 20 percent
        // and falling" at 20.2 s, and the next low line 38 s late. The
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
                        Kind::Info if from(lines::HOLD, &say.text) => holds.push((t, say.text)),
                        _ => {}
                    }
                }
            }
            (lines, holds)
        };
        let (steady, held) = cadence(&|_| None, 20.0);
        let expected = [0.6, 12.6, 42.6, 102.6];
        assert_eq!(steady.len(), expected.len(), "{steady:?}");
        assert_eq!(held.len(), 1, "{held:?}");
        assert!((held[0].0 - 222.6).abs() < 0.15, "{held:?}");
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
            assert_eq!(holds.len(), 1, "{shape}: {holds:?}");
            assert!(
                (holds[0].0 - held[0].0).abs() < 0.15,
                "{shape}: the hold at {holds:?}, not {held:?}"
            );
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
                let hp = match i {
                    0..50 => 20.0,
                    50 => 100.0,
                    51..110 if answered => 100.0,
                    110..210 if answered => 100.0 - (i - 110) as f32 * 0.8,
                    _ => 20.0,
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
                assert_eq!(lines.len(), 3, "{lines:?}");
                assert!((lines[1].0 - 12.6).abs() < 0.15, "{lines:?}");
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
        // (Unanswered, the low line follows the beating's cadence.)
        assert!(from(lines::HP_LOW_AGAIN, &lines[1].1), "{lines:?}");
        assert!((lines[1].0 - 15.9).abs() < 0.15, "{lines:?}");
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
        // it down, and the next fight to that depth is watched and silent,
        // as is one within the margin of the new floor. (A deeper handled
        // fall started the streak over: the fight after it was shouted.)
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
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert!(from(lines::BEATING, &lines[0].1), "{lines:?}");
        // (Under 20 at 1.4 s into the fall: 100 to 15 over 1.5 s.)
        assert!((lines[0].0 - 121.5).abs() < 0.15, "{lines:?}");
        // (Said with the reading, when the card carries one: under 20.)
        assert!(
            !lines[0].1.contains("percent")
                || (15..20).any(|n| lines[0].1.contains(&format!("{n} percent"))),
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
    fn staying_low_with_no_potion_is_said_again_after_longer_and_longer_waits() {
        // HP at 20% and nothing done about it: said, then again 12 s on,
        // 30 s after that, then 60, then 120 — the beating's waits — and
        // not for the frames between. (The player grumbles at minutes 2.5
        // and 5 but drinks nothing: a word keeps the lines coming — four
        // with no sign of life at all would have the rest held, see
        // `warnings_nobody_answers_are_held…`.)
        let mut c = Companion::seeded(Settings::default(), SEED);
        let mut told = Vec::new();
        for i in 0..4000 {
            let t = i as f64 * 0.1;
            if i == 1500 || i == 3000 {
                c.player_spoke(t);
            }
            for line in alerts(&c.observe(t, frame(20.0, 90.0, 10.0))) {
                // (The first stands alone; the rest repeat it, and may
                // say so.)
                let deck = if told.is_empty() {
                    lines::HP_LOW
                } else {
                    lines::HP_LOW_AGAIN
                };
                assert!(from(deck, &line), "{line:?}");
                told.push(t);
            }
        }
        let mut expected = vec![0.6];
        for wait in [12.0, 30.0, 60.0, 120.0, 120.0] {
            expected.push(expected.last().unwrap() + wait);
        }
        assert_eq!(told.len(), expected.len(), "{told:?}");
        for (at, want) in told.iter().zip(expected) {
            assert!((at - want).abs() < 0.15, "{told:?}");
        }
    }

    #[test]
    fn low_mp_is_its_own_warning() {
        let mut c = Companion::seeded(Settings::default(), SEED);
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
        let mut c = Companion::seeded(Settings::default(), SEED);
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
        let mut c = Companion::seeded(Settings::default(), SEED);
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
        assert!(dead[0].starts_with("Your HP hit zero."), "{dead:?}");
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
        // HP at 20% for twenty-five minutes and nothing done about it (the
        // player is away, or the bar is misread): the low warning comes
        // four times (after longer and longer waits: 12, 30, 60 s), then,
        // where the fifth would come, one line saying the rest will wait,
        // then nothing for ten minutes; then two more, and quiet again.
        // (Six, and a pet's death was six lines in six minutes to a room
        // that may be empty.)
        let mut c = Companion::seeded(Settings::default(), SEED);
        let mut lines: Vec<(f64, String)> = Vec::new();
        for i in 0..15_000 {
            let t = i as f64 * 0.1;
            for line in said(&c.observe(t, frame(20.0, 90.0, 10.0))) {
                if !from(lines::SEEN, &line) {
                    lines.push((t, line));
                }
            }
        }
        let texts: Vec<&str> = lines.iter().map(|(_, l)| l.as_str()).collect();
        assert_eq!(texts.len(), 7, "{lines:?}");
        assert!(from(lines::HP_LOW, texts[0]), "{texts:?}");
        assert!(
            texts[1..4].iter().all(|l| from(lines::HP_LOW_AGAIN, l)),
            "{texts:?}"
        );
        // The first time, the hold is explained in full.
        assert_eq!(variant(lines::HOLD, texts[4]), Some(0), "{texts:?}");
        // Four warnings over 102 s, the hold where the fifth would come
        // (two minutes on), and it ends ten minutes later.
        assert!((lines[3].0 - lines[0].0 - 102.0).abs() < 0.5, "{lines:?}");
        assert!((lines[4].0 - lines[3].0 - 120.0).abs() < 0.5, "{lines:?}");
        assert!(lines[5].0 - lines[4].0 >= 600.0, "{lines:?}");
        assert!(from(lines::HP_LOW_AGAIN, texts[5]) && from(lines::HP_LOW_AGAIN, texts[6]));
        // (The hold is not announced again so soon.)
        assert!(c.alerts_held(1500.0));
        // The player says something: the warnings come again.
        c.player_spoke(1500.0);
        assert!(!c.alerts_held(1500.0));
        let mut after = Vec::new();
        for i in 0..500 {
            after.extend(said(
                &c.observe(1500.0 + i as f64 * 0.1, frame(20.0, 90.0, 10.0)),
            ));
        }
        assert_eq!(after.len(), 1, "{after:?}");
        // A card that counts the warnings counts four.
        for attitude in Attitude::ALL {
            for card in attitude.lines(lines::HOLD) {
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
        // HP read at 20% for an hour while EXP rises 0.3% a minute: the
        // player is grinding and fine, and the bar is read wrong. Four
        // unanswered lines (0.6, 12.6, 42.6, 102.6 s), then, where the
        // fifth would come, one note in its own voice that it is holding
        // the HP warnings — and nothing more all hour. (The hold for
        // warnings nobody answers never engaged: EXP answered them, every
        // minute. 34 lines in the hour; six hours of it was ~200.)
        type Lines = Vec<(f64, String)>;
        let hour = |hp: f32, exp_per_min: f32| -> (Lines, Lines) {
            let mut c = Companion::seeded(Settings::default(), SEED);
            let mut warnings: Lines = Vec::new();
            let mut notes: Lines = Vec::new();
            for i in 0..36_000 {
                let t = i as f64 * 0.1;
                let mut obs = read(hp);
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
        // a room not answering).
        let (warnings, notes) = hour(20.0, 0.0);
        assert!(warnings.len() >= 6, "{warnings:?}");
        assert!((warnings[3].0 - 102.6).abs() < 0.15, "{warnings:?}");
        assert!(warnings[4].0 - warnings[3].0 >= 600.0, "{warnings:?}");
        assert!((notes[0].0 - 222.6).abs() < 0.15, "{notes:?}");
        assert!(notes.iter().all(|(_, n)| from(lines::HOLD, n)), "{notes:?}");
        // Believed again once it has read above the mark for three
        // seconds (2.9 s is not enough — and the drop back to 20 after
        // those is no beating either: the bar is not believed): worn down
        // slowly to 20 after that, it is a new fight, said at once.
        let mut c = Companion::seeded(Settings::default(), SEED);
        let mut lines: Vec<(f64, String)> = Vec::new();
        for i in 0..3300 {
            let t = i as f64 * 0.1;
            let hp = match i {
                0..3000 => 20.0,
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
        // MP, the same shape.
        let mut c = Companion::seeded(Settings::default(), SEED);
        let mut warnings = Vec::new();
        let mut notes = Vec::new();
        for i in 0..6000 {
            let t = i as f64 * 0.1;
            let mut obs = read(90.0);
            obs.mp = gauge(10.0, true);
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
        // HP read at 20% for eight minutes and nothing done about it: four
        // warnings, then the hold. A death in it is said all the same (the
        // coach counts it either way, and would speak of a third death the
        // player never heard of), and so is a level-up; the warnings stay
        // held.
        let mut c = Companion::seeded(Settings::default(), SEED);
        let mut lines = Vec::new();
        for i in 0..4800 {
            lines.extend(alerts(&c.observe(i as f64 * 0.1, read(20.0))));
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
        // Revived, still at 20%: the warnings are still held…
        let mut after = Vec::new();
        for i in 0..600 {
            after.extend(alerts(&c.observe(505.0 + i as f64 * 0.1, read(20.0))));
        }
        assert!(after.is_empty(), "{after:?}");
        assert!(c.alerts_held(565.0));
        // …and a level-up is said through it.
        let mut up = read(20.0);
        up.level = Some(166);
        let mut cheered = Vec::new();
        for i in 0..40 {
            cheered.extend(alerts(&c.observe(570.0 + i as f64 * 0.1, up.clone())));
        }
        assert_eq!(cheered, ["Level up! You're level 166."]);
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
        // MP has the same shape (a pet pots MP too): the same rule.
        let mut c = Companion::seeded(Settings::default(), SEED);
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
        // MP, the same (a word from the player each fight: a mana potion
        // is no sign of life to the hold, HP and EXP are what it watches).
        let mut c = Companion::seeded(Settings::default(), SEED);
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
            let mut c = Companion::seeded(
                Settings {
                    attitude,
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
            // An unanswered fight: the second line repeats the first, and
            // may say so.
            let mut c = Companion::seeded(
                Settings {
                    attitude,
                    ..Settings::default()
                },
                SEED,
            );
            let mut again = Vec::new();
            for i in 0..=140 {
                again.extend(dealt(&c.observe(i as f64 * 0.1, frame(20.0, 90.0, 10.0))));
            }
            assert_eq!(again.len(), 2, "{}: {again:?}", attitude.word());
            assert!(
                card_of(attitude, lines::HP_LOW, &again[0]),
                "{}: {again:?}",
                attitude.word()
            );
            assert!(
                card_of(attitude, lines::HP_LOW_AGAIN, &again[1]),
                "{}: {again:?}",
                attitude.word()
            );
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
        // Never said in any voice: these are about who they are, not how
        // they play (and the last two are not a joke).
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
        ];
        for (name, deck) in lines::ALL {
            // (A deck dealt a few times a night at most has three.)
            let least = if lines::SHORT.contains(name) { 3 } else { 6 };
            for attitude in Attitude::ALL {
                let list = attitude.lines(*deck);
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
                    assert!(!lower.contains("syrup"), "{name}: {line:?}");
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
            for card in attitude.lines(lines::SOONER) {
                assert!(card.contains("{}"), "{card:?}");
            }
        }
    }

    #[test]
    fn a_death_no_warning_came_before_moves_the_warning_sooner() {
        let mut c = Companion::seeded(Settings::default(), SEED);
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
        let beating = step(&mut c, 40.0, 3);
        assert_eq!(beating.len(), 1, "{beating:?}");
        assert!(from(lines::BEATING, &beating[0]), "{beating:?}");
        assert!(step(&mut c, 32.0, 3).is_empty());
        // (A death read from the bar takes two seconds to believe.) The
        // death line, with the change after it.
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
        step(&mut c, 60.0, 3);
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
        // Down through 40% and dead, with no warning (the mark is 30): the
        // warning moves sooner, and the line says so — and nothing else.
        // One night this was "Dead. Told you to pot. I'll warn you sooner
        // from now on, under 40%."
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
            (6.0, 0.0),
            (6.1, 0.0),
            (6.2, 0.0),
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
        // Four deaths no warning came before, each through where a sooner
        // warning would have come: the mark moves 30, 35, 40, 45, 50, and
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
                    (5.0, 40.0),
                    (5.1, 40.0),
                    (5.2, 40.0),
                    (6.0, 0.0),
                    (6.1, 0.0),
                    (6.2, 0.0),
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
        // it said to pot), the low line 12 s on, and dead at 30 s — 17 s
        // after the last line, with the next not due for 13 s more. It
        // warned, twice, in this fight: the death is a warned one, nothing
        // about warning sooner, and the mark stays. (A window of 15 s had
        // it say "I'll warn you sooner from now on, under 35%" here, and
        // the mark crept up a night at a time for deaths it had called.)
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
        assert_eq!(lines.len(), 3, "{lines:?}");
        assert!(from(lines::BEATING, &lines[0].1), "{lines:?}");
        assert!(from(lines::HP_LOW_AGAIN, &lines[1].1), "{lines:?}");
        assert!((lines[1].0 - 13.2).abs() < 0.15, "{lines:?}");
        let (at, death) = &lines[2];
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
        assert_eq!(lines.len(), 4, "{lines:?}");
        assert!(from(lines::DEATH, &lines[3].1), "{lines:?}");
        assert_eq!(c.settings.hp_low, 30.0);
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
        // 1000 s (MP moving is not the game going on) puts it off to 1300.
        let mut c = Companion::seeded(Settings::default(), SEED);
        let mut lines: Vec<(f64, String)> = Vec::new();
        for i in 0..15_000 {
            let t = i as f64 * 0.1;
            let mut obs = read(90.0);
            obs.mp = gauge(
                if (10_000..10_050).contains(&i) {
                    10.0
                } else {
                    80.0
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
        // Warnings nobody answers (the bar flat under the mark): the hold
        // comes and goes, two lines through between one and the next —
        // held, or within minutes of a line of its own, the whole time:
        // never asked.
        let mut c = Companion::seeded(Settings::default(), SEED);
        let mut lines: Vec<(f64, String)> = Vec::new();
        let mut held_at_twenty_five_minutes = false;
        for i in 0..24_000 {
            let t = i as f64 * 0.1;
            for line in asked(&c.observe(t, read(20.0))) {
                lines.push((t, line));
            }
            if i == 15_000 {
                held_at_twenty_five_minutes = c.alerts_held(t);
            }
        }
        assert!(held_at_twenty_five_minutes);
        assert!(lines.is_empty(), "{lines:?}");
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
}
