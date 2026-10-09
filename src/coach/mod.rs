//! The coach: MapleSyrup speaking up on its own while the player plays.
//!
//! The companion says the little that is certain from the numbers (HP low,
//! a level-up, a death). The coach is for the rest: it watches the game go
//! by and decides *when* a model should look and *why* — and the model
//! decides whether there is anything worth saying, in one line, or keeps
//! quiet. The player wants to play, not talk: the coach never waits for a
//! question, and never interrupts without a reason.
//!
//! Reasons, in order of urgency:
//! - a close call: HP went under a tenth and came back, with no death
//!   (at most once in five minutes);
//! - a streak: the third death within ten minutes (once per streak);
//! - the picture cut and settled (a portal, a cutscene, a dialog, a
//!   death: the pixels cannot tell which), at most once in a couple of
//!   minutes, and not around a death or a level-up;
//! - they just went up a level (a moment after the companion's cheer);
//! - their EXP has not moved for minutes while the game goes on;
//! - nothing in particular: a look now and then, less often each time the
//!   model has nothing to say, and less often the more the player talks.
//!
//! Each reason tells the model what happened and gives it a few lines a
//! friend would say in that spot, in the player's chosen attitude: the
//! model reacts to the one thing, or keeps quiet ("[silent]"); it never
//! narrates the screen.
//!
//! Pacing is deterministic and tested here: a model is consulted at most
//! every few seconds and only while nobody is talking, and nothing is said
//! sooner than a quarter minute after the last unprompted line — except a
//! reaction (a close call, a streak), which waits only for the talking to
//! stop: it is about the moment, and the moment passes. The model is never
//! asked what to say about a danger: the companion's own lines for those
//! come at once, with no model in the way.

pub mod scene;

use std::collections::VecDeque;

use crate::companion::{Attitude, Observation};

/// What the coach sees of one frame.
pub struct Glance<'a> {
    pub now: f64,
    pub obs: &'a Observation,
    pub scene: Option<&'a scene::Verdict>,
    /// The game is the window in front: what is captured is the game.
    pub in_view: bool,
    /// Someone is talking: the player, or MapleSyrup (a reply being made
    /// or said).
    pub talking: bool,
    pub muted: bool,
    /// The character is dead (HP at zero), until it comes back: the
    /// companion has said so, and the death screen is no new scene.
    pub dead: bool,
}

/// Why the model is consulted.
#[derive(Debug, Clone, PartialEq)]
pub enum Reason {
    /// HP went under a tenth and came back above the mark within seconds,
    /// with no death: a close call, and the lowest it got (percent).
    CloseCall { lowest: u32 },
    /// The third death within ten minutes: `deaths` of them, over
    /// `minutes` (the first to the last).
    Streak { deaths: u32, minutes: u32 },
    /// The picture changed a lot and settled: a portal, a cutscene, a
    /// dialog, a death — the pixels cannot tell which.
    NewScene,
    /// They just reached this level (None when the number is not known).
    LevelUp { level: Option<u32> },
    /// No EXP for this many minutes.
    ExpStalled { minutes: u32 },
    /// Nothing in particular: a look at the game now and then.
    Look,
}

/// Lines a friend would say for each reason, one list per attitude
/// (friendly, blunt, savage), given to the model as the pattern to react
/// in — not a script. `{}` is the number in the reason (the lowest HP, the
/// minutes). Savage stays within the policy in `ai::style`: the play is
/// insulted, never who they are; no questions (a friend who sees
/// something says it).
pub mod examples {
    /// A close call: `{}` the lowest HP got, in percent.
    pub const CLOSE_CALL: [&[&str]; 3] = [
        &[
            "Phew, that was close.",
            "Okay, that one nearly had you. Breathe.",
            "{} percent! My heart. Pot a bit sooner, yeah.",
        ],
        &[
            "That was close. Pot sooner.",
            "You nearly ate it there. {} percent.",
            "Lucky. Next time pot at half, not at {}.",
        ],
        &[
            "That was close, you absolute clown.",
            "{} percent. One more hit and I'd be writing your eulogy.",
            "Lucky, not good. Pot before you're at {} next time.",
        ],
    ];

    /// A streak of deaths: `{}` the minutes they fell in.
    pub const STREAK: [&[&str]; 3] = [
        &[
            "Three deaths in {} minutes. Take five, then try an easier map.",
            "This map's chewing you up. Go somewhere gentler for a bit.",
            "Breather time. The mobs will still be here in ten minutes.",
        ],
        &[
            "Third death in {} minutes. Lower map, or take a break.",
            "You keep dying to the same thing. Stop and work out what.",
            "Three deaths. This map's above you right now. Move.",
        ],
        &[
            "Three deaths in {} minutes. Take a break before the mobs file a complaint.",
            "This map's too hard for you. Say it with me: too hard.",
            "Dying on repeat. Go find a map with training wheels.",
        ],
    ];

    /// A new scene. Only what the reason carries — they went somewhere
    /// new — and nothing a 640×400 picture cannot support: no portal
    /// direction, no "wrong map", no boss at the end, no gear or buffs.
    /// ("That's the long way round. Portal's left." taught the model to
    /// assert a portal it could not see, and the player caught it at
    /// once.)
    pub const NEW_SCENE: [&[&str]; 3] = [
        &[
            "Ooh, new map. Buff up before you jump in.",
            "Somewhere new. Check the map name before you go in.",
            "New map! Take a second and look around first.",
        ],
        &[
            "New map. Buff up.",
            "Rebuff before you go in.",
            "New map. Check the name.",
        ],
        &[
            "New map. Try not to die in the first minute.",
            "Buff first, then walk in.",
            "Somewhere new. Look before you leap, genius.",
        ],
    ];

    /// A level-up: the points, and nothing about quests, maps by name or
    /// job advancements — a level number cannot support them ("Fourth job
    /// quest's up. Go." is false at 150).
    pub const LEVEL_UP: [&[&str]; 3] = [
        &[
            "Put the new points in your main attack.",
            "A level stronger. Push a bit harder now.",
            "Points first, then back to it. Nice one.",
        ],
        &[
            "Points in the main attack. Don't spread them.",
            "Points first, then back to it.",
            "One level stronger. Hit something harder.",
        ],
        &[
            "Put the points somewhere useful for once.",
            "One level up. Still can't dodge, I bet.",
            "Points in the main attack, not wherever you usually dump them.",
        ],
    ];

    /// EXP stalled: `{}` the minutes.
    pub const EXP_STALLED: [&[&str]; 3] = [
        &[
            "Map's empty, hop a channel.",
            "You've been in that menu a while. Come back and grind!",
            "Nothing's spawning here. Try the next map over.",
        ],
        &[
            "Dead map. Change channel.",
            "{} minutes, zero EXP. Move.",
            "Been in the shop a while. Get out and kill something.",
        ],
        &[
            "{} minutes, zero EXP. AFK or just bad, either way: move.",
            "Dead map, dead player. Change channel.",
            "Stop window-shopping and go kill something.",
        ],
    ];

    /// A look at nothing in particular. Only what a small, soft picture
    /// of the screen shows: a crowd of mobs on the character, a boss, a
    /// low bar, a dialog, an empty map, the character standing still.
    /// Never loot, runes or buff icons — the model cannot make those out
    /// at that size, and an invented "loot behind you" is worse than
    /// silence.
    pub const LOOK: [&[&str]; 3] = [
        &[
            "That's a crowd on you. Thin it out before it bites.",
            "Boss on screen. Get ready before you go in.",
            "You've been standing still a while. Go hit something.",
        ],
        &[
            "You're surrounded. Move.",
            "That's a boss. Pot up first.",
            "Standing around again. Play.",
        ],
        &[
            "Half the map's on you and you're just standing there. Move.",
            "A boss. Try not to die in the first ten seconds.",
            "Standing still on an empty map. Change channel, genius.",
        ],
    ];

    /// Every list, by name, for tests.
    pub const ALL: &[(&str, [&[&str]; 3])] = &[
        ("close call", CLOSE_CALL),
        ("streak", STREAK),
        ("new scene", NEW_SCENE),
        ("level up", LEVEL_UP),
        ("EXP stalled", EXP_STALLED),
        ("a look", LOOK),
    ];
}

impl Reason {
    /// A reaction to a moment (a close call, a streak), as against company
    /// (a look, a new scene, a level-up, a stall): said soon or not at all,
    /// and from the facts in the reason alone.
    pub fn is_reaction(&self) -> bool {
        matches!(self, Reason::CloseCall { .. } | Reason::Streak { .. })
    }

    /// Whether the model should be shown the screen for this reason. A
    /// reaction carries its facts (the lowest HP, the deaths); the picture
    /// would cost a second and add nothing — and the moment is the point.
    pub fn wants_picture(&self) -> bool {
        !self.is_reaction()
    }

    /// The more urgent, the lower.
    fn rank(&self) -> u8 {
        match self {
            Reason::CloseCall { .. } => 0,
            Reason::Streak { .. } => 1,
            Reason::NewScene => 2,
            Reason::LevelUp { .. } => 3,
            Reason::ExpStalled { .. } => 4,
            Reason::Look => 5,
        }
    }

    /// The number the example lines carry, if any.
    fn number(&self) -> Option<u32> {
        match self {
            Reason::CloseCall { lowest } => Some(*lowest),
            Reason::Streak { minutes, .. } | Reason::ExpStalled { minutes } => Some(*minutes),
            _ => None,
        }
    }

    /// Its example lines (one list per attitude).
    fn example_lines(&self) -> [&'static [&'static str]; 3] {
        match self {
            Reason::CloseCall { .. } => examples::CLOSE_CALL,
            Reason::Streak { .. } => examples::STREAK,
            Reason::NewScene => examples::NEW_SCENE,
            Reason::LevelUp { .. } => examples::LEVEL_UP,
            Reason::ExpStalled { .. } => examples::EXP_STALLED,
            Reason::Look => examples::LOOK,
        }
    }

    /// The example lines for this reason in `attitude`'s voice, with the
    /// number filled in, quoted and strung together.
    pub fn examples(&self, attitude: Attitude) -> String {
        let number = self.number().map(|n| n.to_string()).unwrap_or_default();
        attitude
            .lines(self.example_lines())
            .iter()
            .map(|line| format!("\"{}\"", line.replace("{}", &number)))
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// What happened, for the model: why it is asked, what a reaction to
    /// it is (one specific thing, never a run-down of the screen), and a
    /// few lines a friend would say in that spot, in `attitude`'s voice.
    pub fn describe(&self, attitude: Attitude) -> String {
        let what = match self {
            Reason::CloseCall { lowest } => format!(
                "That was close: their HP dropped to {lowest}% a moment ago and they pulled it back. Say it the \
way a friend blurts it out — the scare, the save, pot sooner — in one line; [silent] only if you just said as \
much."
            ),
            Reason::Streak { deaths, minutes } => format!(
                "That's their {} death in {minutes} minutes. Say the thing a friend says now, once, in one \
line: a breather, an easier map, or what keeps killing them; [silent] only if you just said as much.",
                ordinal(*deaths)
            ),
            Reason::NewScene => "The picture just changed and settled: they went somewhere new, or something \
big is on screen (a portal, a cutscene, a dialog, a boss). React, don't narrate: one thing worth saying on \
arriving somewhere new (buff up, check the map name, look around), only if it's worth saying; else [silent]. \
Never say what the screen shows, and never name a portal, a quest, an NPC or a map you can't see in the \
picture."
                .to_string(),
            Reason::LevelUp { level } => {
                let level = level
                    .map(|l| format!("level {l}"))
                    .unwrap_or_else(|| "a new level".to_string());
                format!(
                    "They just hit {level}; the cheer was said already, don't repeat it. One line on the one \
thing that changes now, if anything — a skill to put points in, pushing a bit harder; else [silent]. A level \
number tells you nothing else: never name a portal, a quest, an NPC or a map you can't see in the picture."
                )
            }
            Reason::ExpStalled { minutes } => format!(
                "Their EXP hasn't moved for {minutes} minutes: nothing is dying. Work out why from the picture \
— a shop or menu open, standing around, an empty map, a boss still up, trading, a cutscene. If what they're \
doing makes sense (a menu, a trade, a boss, a chat), [silent]; if not, one line that gets them going again."
            ),
            Reason::Look => "Nothing in particular happened; you're just glancing at their screen. A line only \
if something there deserves one right now — danger building (a crowd of mobs on them, a boss, the HP bar \
low), a dialog they're stuck in, an empty map, them standing still; else [silent], as most glances are. The \
picture is small and soft: you can't make out loot, runes or buff icons in it, so never call those."
                .to_string(),
        };
        format!("{what} Like: {}", self.examples(attitude))
    }

    /// A few words for the log.
    pub fn label(&self) -> String {
        match self {
            Reason::CloseCall { lowest } => format!("close call ({lowest}%)"),
            Reason::Streak { deaths, minutes } => format!("{deaths} deaths in {minutes} min"),
            Reason::NewScene => "new scene".into(),
            Reason::LevelUp { level: Some(level) } => format!("level {level}"),
            Reason::LevelUp { level: None } => "level up".into(),
            Reason::ExpStalled { minutes } => format!("no EXP for {minutes} min"),
            Reason::Look => "a look".into(),
        }
    }
}

/// "3rd", "4th", "21st".
fn ordinal(n: u32) -> String {
    let suffix = match (n % 10, n % 100) {
        (1, 11) | (2, 12) | (3, 13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    };
    format!("{n}{suffix}")
}

/// How often the model looks when nothing happens, in seconds, to start
/// with; each time it has nothing to say the wait grows by half, up to
/// [`LOOK_AT_MOST`]; a line said starts it over.
pub const LOOK_EVERY: f64 = 45.0;
pub const LOOK_AT_MOST: f64 = 180.0;
/// Talk in the last this long puts the looks off, in seconds: each second
/// anyone talked (the player, a reply to them) in the last five minutes
/// puts the next look-at-nothing-in-particular off by a second. The pace
/// above was set for a player who plays and says nothing — a look every
/// three quarters of a minute, growing by half each time there was
/// nothing to say — and is unchanged for them; but in a conversation the
/// same pace put a callout in every gap between the player's sentences,
/// which is a commentator, not company. Now a minute of chat buys a
/// minute of quiet; talk half the time and the looks come every three
/// minutes or so; talk the whole five and the next look is five minutes
/// off. Only the looks: the other reasons (a close call, a streak, a new
/// scene, a level-up, a stall) and the companion's own alerts are not
/// held by it — they wait only for the usual quiet gap.
pub const TALK_WINDOW: f64 = 300.0;
/// A close call: HP under this, in percent…
pub const CLOSE_CALL_UNDER: f32 = 10.0;
/// …and back above this…
pub const CLOSE_CALL_BACK: f32 = 40.0;
/// …within this long of going under, in seconds, with no death between.
pub const CLOSE_CALL_WITHIN: f64 = 20.0;
/// A close call is remarked on at most once in this long, in seconds.
pub const CLOSE_CALL_AGAIN: f64 = 300.0;
/// A reading under the mark, or back above it, must hold this many frames
/// and this long to count: a bar half under a dialog reads as a dip too,
/// and a flicker is not a save.
const HOLD_FRAMES: u32 = 3;
const HOLD_SECS: f64 = 0.6;
/// A streak: this many deaths within this long, in seconds. Remarked on
/// once, when the third lands, and not again until the streak is over —
/// this long without a death.
pub const STREAK_DEATHS: usize = 3;
pub const STREAK_WITHIN: f64 = 600.0;
/// Nothing unprompted is said sooner than this after anything was said
/// (by anyone), in seconds: a line a quarter minute is company, two in a
/// row is nagging.
pub const MIN_GAP: f64 = 15.0;
/// Two consults are at least this far apart, in seconds.
pub const CONSULT_GAP: f64 = 8.0;
/// EXP unmoved for this long is a stall, in seconds of play — time with
/// something going on; a stall is fighting with no EXP, and idle minutes
/// (nobody playing) never count toward one…
pub const STALL_AFTER: f64 = 180.0;
/// …said again (with the longer figure) after this much more play.
pub const STALL_AGAIN: f64 = 600.0;
/// The companion cheers a level-up first; the coach's word comes after.
pub const LEVEL_UP_AFTER: f64 = 6.0;
/// A new scene is looked at no sooner than this after the last one, in
/// seconds: the picture cuts and settles at every dialog box, death
/// screen and full-screen effect, not only at a portal, and a player who
/// stays on one map would otherwise be told about it at every cut.
pub const NEW_SCENE_AGAIN: f64 = 120.0;
/// No new scene while the character is dead, nor within this long of a
/// death or a level-up, in seconds: the death screen, the revive and the
/// level-up's flash each cut and settle, and the companion has already
/// said what there is to say about them.
pub const NEW_SCENE_HUSH: f64 = 10.0;
/// Below this much going on, nobody is playing: no looks (the player is
/// away, or reading).
pub const IDLE_ACTIVITY: f32 = 0.002;
/// The game coming into view gets this long before the first word, in
/// seconds.
pub const SETTLE_IN: f64 = 10.0;
/// A reason not acted on within this long is stale, in seconds.
const STALE_AFTER: f64 = 60.0;
/// A look nothing came back from in this long is given up on, in seconds.
const CONSULT_TIMEOUT: f64 = 45.0;
/// The coach's lines kept for the model (not to repeat itself).
const KEEP_LINES: usize = 6;

/// A scare under way: HP under the mark, and back from it.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Scare {
    /// Nothing going on.
    None,
    /// HP under the mark: since when, for how many frames, and the lowest
    /// it read.
    Under {
        since: f64,
        frames: u32,
        lowest: f32,
    },
    /// Under long enough to be a real dip: since when, the lowest; and HP
    /// back above the mark, since when and for how many frames (None:
    /// not yet).
    Dipped {
        since: f64,
        lowest: f32,
        back: Option<(f64, u32)>,
    },
}

pub struct Coach {
    /// Coaching is on: it may speak up on its own.
    pub on: bool,
    /// When anyone last said anything.
    quiet_since: f64,
    last_consult: f64,
    /// When the model was last consulted about a new scene.
    last_new_scene: f64,
    /// When the character was last seen dead, and when the companion last
    /// cheered a level-up: no new scene around either.
    last_dead: f64,
    last_level_alert: f64,
    look_every: f64,
    silent_in_a_row: u32,
    /// A consult is under way.
    consulting: bool,
    /// The EXP reading as it last changed; how much play there has been
    /// since — seconds with something going on, the game in view: a stall
    /// is fighting with no EXP, and idle time never counts toward it —
    /// and how much there had been when the stall was last said.
    exp_read: Option<f32>,
    stalled_for: f64,
    stall_told: Option<f64>,
    /// A reason waiting for a quiet moment, and since when.
    pending: Option<(Reason, f64)>,
    /// A level-up waiting for its moment.
    level_up: Option<(f64, Option<u32>)>,
    /// Since when the game has been in view.
    seen_since: Option<f64>,
    /// How much was going on lately.
    activity: f32,
    /// What it said on its own lately.
    said: VecDeque<String>,
    /// The scare under way, if any; a close call it ended in, this frame
    /// (the lowest HP got); and when one was last remarked on.
    scare: Scare,
    close_call: Option<u32>,
    last_close_call: f64,
    /// Whether the character was dead last frame (a death is `dead` going
    /// up); when the deaths of the last ten minutes were; whether this
    /// streak was remarked on; and the streak to remark on, this frame
    /// (deaths, minutes).
    was_dead: bool,
    deaths: VecDeque<f64>,
    streak_told: bool,
    streak: Option<(u32, u32)>,
    /// Talk lately: (when, seconds of it), a second or so per entry; and
    /// the last frame's time, to measure it by.
    talk: VecDeque<(f64, f64)>,
    last_frame: Option<f64>,
    /// Consults so far, and lines said: for the log.
    pub consults: u32,
    pub spoken: u32,
}

impl Coach {
    pub fn new(on: bool) -> Self {
        Self {
            on,
            quiet_since: f64::NEG_INFINITY,
            last_consult: f64::NEG_INFINITY,
            last_new_scene: f64::NEG_INFINITY,
            last_dead: f64::NEG_INFINITY,
            last_level_alert: f64::NEG_INFINITY,
            look_every: LOOK_EVERY,
            silent_in_a_row: 0,
            consulting: false,
            exp_read: None,
            stalled_for: 0.0,
            stall_told: None,
            pending: None,
            level_up: None,
            seen_since: None,
            activity: 0.0,
            said: VecDeque::new(),
            scare: Scare::None,
            close_call: None,
            last_close_call: f64::NEG_INFINITY,
            was_dead: false,
            deaths: VecDeque::new(),
            streak_told: false,
            streak: None,
            talk: VecDeque::new(),
            last_frame: None,
            consults: 0,
            spoken: 0,
        }
    }

    /// One frame. Returns a reason to consult the model now, if it is time.
    pub fn observe(&mut self, g: &Glance) -> Option<Reason> {
        let now = g.now;
        let watched = self.on && !g.muted && g.in_view && g.obs.game.is_seen();
        // A stall is fighting with no EXP: the time since it moved counts
        // only while something was going on and the game was watched —
        // the interval this frame ends, a second at most (as `track_talk`
        // has it), by the last frame's picture; `track` starts the clock
        // over at a change. Twenty idle minutes and the player back had
        // "no EXP for 20 min" consulted, with the picture, 18 s after
        // their "I'm back", with EXP already moving.
        if watched
            && self.activity > IDLE_ACTIVITY
            && let Some(last) = self.last_frame
        {
            self.stalled_for += (now - last).clamp(0.0, 1.0);
        }
        self.track(g);
        if !watched {
            self.pending = None;
            self.level_up = None;
            self.close_call = None;
            self.streak = None;
            if !g.in_view || !g.obs.game.is_seen() {
                self.seen_since = None;
            }
            return None;
        }
        let since = *self.seen_since.get_or_insert(now);
        if now - since < SETTLE_IN {
            self.close_call = None;
            self.streak = None;
            return None;
        }
        // What came up this frame. A close call and a streak are moments:
        // said soon or not at all (a reason not acted on goes stale). A
        // close call is remarked on once in a while, a streak once per
        // streak (`track` sees to both).
        if let Some(lowest) = self.close_call.take() {
            self.propose(Reason::CloseCall { lowest }, now);
        }
        if let Some((deaths, minutes)) = self.streak.take() {
            self.propose(Reason::Streak { deaths, minutes }, now);
        }
        // A new scene is not worth a look around a death or a level-up
        // (the companion spoke; the screen cut for that), nor again within
        // a couple of minutes of the last: a cut is as often a dialog box
        // as a portal.
        let hushed = g.dead
            || now - self.last_dead < NEW_SCENE_HUSH
            || now - self.last_level_alert < NEW_SCENE_HUSH;
        if hushed && matches!(self.pending, Some((Reason::NewScene, _))) {
            self.pending = None;
        }
        if g.scene.is_some_and(|s| s.new_scene)
            && !hushed
            && now - self.last_new_scene >= NEW_SCENE_AGAIN
        {
            self.propose(Reason::NewScene, now);
        }
        if let Some((at, level)) = self.level_up
            && now >= at
        {
            self.level_up = None;
            self.propose(Reason::LevelUp { level }, now);
        }
        if self.exp_read.is_some()
            && self.stalled_for >= STALL_AFTER
            && self
                .stall_told
                .is_none_or(|told| self.stalled_for - told >= STALL_AGAIN)
            && self.activity > IDLE_ACTIVITY
        {
            self.stall_told = Some(self.stalled_for);
            let minutes = (self.stalled_for / 60.0).floor() as u32;
            self.propose(Reason::ExpStalled { minutes }, now);
        }
        if self
            .pending
            .as_ref()
            .is_some_and(|(_, since)| now - since > STALE_AFTER)
        {
            self.pending = None;
        }
        if self.consulting && now - self.last_consult >= CONSULT_TIMEOUT {
            // Nothing came back: not held up by it for good.
            self.consulting = false;
        }
        if g.talking || self.consulting {
            return None;
        }
        let quiet = now - self.quiet_since >= MIN_GAP;
        let apart = now - self.last_consult >= CONSULT_GAP;
        // A reaction (a close call, a streak) is about the moment: it waits
        // for nobody's quiet quarter minute, only for the talking to stop
        // and the consults to be apart. "That was close" a quarter minute
        // after the companion's own "pot now" landed when the fight was
        // over. Company (a new scene, a level-up, a stall, a look) keeps
        // the quarter minute.
        let reaction = self
            .pending
            .as_ref()
            .is_some_and(|(reason, _)| reason.is_reaction());
        if (quiet || reaction) && apart && self.pending.is_some() {
            let (reason, _) = self.pending.take()?;
            return Some(self.consult(reason, now));
        }
        // A look at nothing in particular: put off by as long as anyone
        // talked lately (see TALK_WINDOW).
        let wait = self.look_every + self.talked_lately();
        if quiet && now - self.last_consult >= wait && self.activity > IDLE_ACTIVITY {
            return Some(self.consult(Reason::Look, now));
        }
        None
    }

    /// Keep the trails up to date, whatever else is going on.
    fn track(&mut self, g: &Glance) {
        let now = g.now;
        if let Some(scene) = g.scene {
            self.activity = scene.activity;
        }
        if g.dead {
            self.last_dead = now;
        }
        self.track_talk(g);
        self.track_deaths(g);
        self.track_scare(g);
        // (A level-up reaches the coach through `leveled`, from the
        // companion's verified one — not from the level as read each frame:
        // a reading that flips 165, 166, 165, 166 had the coach propose
        // "they just hit 166" every time it came back up.)
        if let Some(exp) = g.obs.exp {
            // A printed number moves in hundredths; a bar's fill flickers.
            let tolerance = if exp.read { 0.005 } else { 0.3 };
            match self.exp_read {
                Some(before) if (exp.percent - before).abs() <= tolerance => {}
                _ => {
                    self.exp_read = Some(exp.percent);
                    self.stalled_for = 0.0;
                    self.stall_told = None;
                }
            }
        }
    }

    /// How much anyone talked in the last [`TALK_WINDOW`], in seconds. A
    /// frame counts for the time since the last (a second at most, so a
    /// stall in the frames is not talk); not while a consult is under way
    /// (the worker is busy with the look itself, which is not talk).
    fn track_talk(&mut self, g: &Glance) {
        let now = g.now;
        if let Some(last) = self.last_frame
            && g.talking
            && !self.consulting
        {
            let dt = (now - last).clamp(0.0, 1.0);
            match self.talk.back_mut() {
                Some((at, secs)) if now - *at < 1.0 => *secs += dt,
                _ => self.talk.push_back((now, dt)),
            }
        }
        self.last_frame = Some(now);
        while self
            .talk
            .front()
            .is_some_and(|(at, _)| now - at > TALK_WINDOW)
        {
            self.talk.pop_front();
        }
    }

    /// Seconds of talk in the last [`TALK_WINDOW`].
    fn talked_lately(&self) -> f64 {
        self.talk.iter().map(|(_, secs)| secs).sum()
    }

    /// Deaths: `dead` going up is one, however long it stays. The third
    /// within [`STREAK_WITHIN`] is a streak, remarked on once; the streak
    /// is over after that long without a death.
    fn track_deaths(&mut self, g: &Glance) {
        let now = g.now;
        let died = g.dead && !self.was_dead;
        self.was_dead = g.dead;
        while self
            .deaths
            .front()
            .is_some_and(|at| now - at > STREAK_WITHIN)
        {
            self.deaths.pop_front();
        }
        if self.deaths.is_empty() {
            self.streak_told = false;
        }
        if !died {
            return;
        }
        self.deaths.push_back(now);
        if self.deaths.len() >= STREAK_DEATHS && !self.streak_told {
            self.streak_told = true;
            let first = self.deaths.front().copied().unwrap_or(now);
            let minutes = ((now - first) / 60.0).ceil().max(1.0) as u32;
            self.streak = Some((self.deaths.len() as u32, minutes));
        }
    }

    /// A close call: HP under [`CLOSE_CALL_UNDER`] (held a moment), then
    /// back above [`CLOSE_CALL_BACK`] (held a moment) within
    /// [`CLOSE_CALL_WITHIN`] of going under, with no death between. A
    /// death ends the scare; so does a dip that drags on (they sat at low
    /// HP: the companion's warnings are for that). Remarked on at most
    /// once in [`CLOSE_CALL_AGAIN`].
    fn track_scare(&mut self, g: &Glance) {
        let now = g.now;
        if g.dead {
            self.scare = Scare::None;
            return;
        }
        // No reading, or a zero (a dialog over the bar, or a death on its
        // way: `dead` says which): the scare stands as it is.
        let Some(hp) = g.obs.hp.map(|h| h.percent).filter(|p| *p > 0.5) else {
            return;
        };
        let under = hp < CLOSE_CALL_UNDER;
        let back = hp >= CLOSE_CALL_BACK;
        let held = |since: f64, frames: u32| frames >= HOLD_FRAMES && now - since >= HOLD_SECS;
        self.scare = match self.scare {
            Scare::None if under => Scare::Under {
                since: now,
                frames: 1,
                lowest: hp,
            },
            Scare::None => Scare::None,
            Scare::Under {
                since,
                frames,
                lowest,
            } if under => {
                let (frames, lowest) = (frames + 1, lowest.min(hp));
                if held(since, frames) {
                    Scare::Dipped {
                        since,
                        lowest,
                        back: None,
                    }
                } else {
                    Scare::Under {
                        since,
                        frames,
                        lowest,
                    }
                }
            }
            // A flicker, not a dip.
            Scare::Under { .. } => Scare::None,
            Scare::Dipped { since, lowest, .. } if under => Scare::Dipped {
                since,
                lowest: lowest.min(hp),
                back: None,
            },
            Scare::Dipped {
                since,
                lowest,
                back: Some((at, frames)),
            } if back => {
                let frames = frames + 1;
                if !held(at, frames) {
                    Scare::Dipped {
                        since,
                        lowest,
                        back: Some((at, frames)),
                    }
                } else {
                    if at - since <= CLOSE_CALL_WITHIN
                        && now - self.last_close_call >= CLOSE_CALL_AGAIN
                    {
                        self.last_close_call = now;
                        self.close_call = Some(lowest.round().max(1.0) as u32);
                    }
                    Scare::None
                }
            }
            Scare::Dipped { since, lowest, .. } if back => {
                if now - since > CLOSE_CALL_WITHIN {
                    // Too slow a save to be a close call.
                    Scare::None
                } else {
                    Scare::Dipped {
                        since,
                        lowest,
                        back: Some((now, 1)),
                    }
                }
            }
            // Between the marks: not back yet; and a dip that drags on is
            // not a close call.
            Scare::Dipped { since, lowest, .. } => {
                if now - since > CLOSE_CALL_WITHIN {
                    Scare::None
                } else {
                    Scare::Dipped {
                        since,
                        lowest,
                        back: None,
                    }
                }
            }
        };
    }

    fn propose(&mut self, reason: Reason, now: f64) {
        match &self.pending {
            Some((waiting, _)) if waiting.rank() <= reason.rank() && *waiting != reason => {}
            _ => self.pending = Some((reason, now)),
        }
    }

    fn consult(&mut self, reason: Reason, now: f64) -> Reason {
        self.consulting = true;
        self.last_consult = now;
        if reason == Reason::NewScene {
            self.last_new_scene = now;
        }
        self.consults += 1;
        reason
    }

    /// A level-up (from the companion, which sees it first): the coach's
    /// word comes a moment after the cheer.
    pub fn leveled(&mut self, now: f64, level: Option<u32>) {
        self.last_level_alert = now;
        self.level_up = match self.level_up {
            None => Some((now + LEVEL_UP_AFTER, level)),
            // The number came after the cheer.
            Some((at, None)) => Some((at, level)),
            same => same,
        };
        // A level-up moves the EXP bar, whatever the readings say.
        self.exp_read = Some(0.0);
        self.stalled_for = 0.0;
        self.stall_told = None;
    }

    /// Anyone said anything (the player, a reply, an alert): the coach
    /// keeps quiet for a while.
    pub fn someone_spoke(&mut self, now: f64) {
        self.quiet_since = now;
    }

    /// The consult came back: `line` was said, or nothing was (None): the
    /// looks come less often each time there was nothing to say.
    pub fn answered(&mut self, now: f64, line: Option<&str>) {
        self.consulting = false;
        match line {
            Some(line) => {
                self.said.push_back(line.to_string());
                while self.said.len() > KEEP_LINES {
                    self.said.pop_front();
                }
                self.quiet_since = now;
                self.silent_in_a_row = 0;
                self.look_every = LOOK_EVERY;
                self.spoken += 1;
            }
            None => {
                self.silent_in_a_row += 1;
                self.look_every = (self.look_every * 1.5).min(LOOK_AT_MOST);
            }
        }
    }

    /// The consult was called off before it answered (the player spoke:
    /// every word of theirs calls off the look in flight). Not "nothing to
    /// say": the pace of the looks is left as it was, and the game is
    /// looked at again at the next gap. Counted as silent, a chatty hour
    /// had slowed the looks to their fewest.
    pub fn called_off(&mut self) {
        self.consulting = false;
    }

    /// What it said on its own lately, oldest first.
    pub fn lines(&self) -> Vec<String> {
        self.said.iter().cloned().collect()
    }

    /// Whether a consult is under way.
    pub fn consulting(&self) -> bool {
        self.consulting
    }

    /// Turn coaching on or off (off drops what was waiting).
    pub fn set_on(&mut self, on: bool) {
        self.on = on;
        if !on {
            self.pending = None;
            self.level_up = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::companion::{GameView, Gauge};

    fn obs(exp: f32, level: u32) -> Observation {
        Observation {
            game: GameView::Seen("MapleStory".into()),
            hp: Some(Gauge {
                percent: 100.0,
                current: None,
                max: None,
                read: false,
            }),
            mp: None,
            exp: Some(Gauge {
                percent: exp,
                current: None,
                max: None,
                read: true,
            }),
            level: Some(level),
            name: None,
            job: None,
        }
    }

    fn verdict(activity: f32, new_scene: bool) -> scene::Verdict {
        scene::Verdict {
            change: activity,
            activity,
            new_scene,
        }
    }

    /// Play `seconds` of frames (ten a second) through the coach, with EXP
    /// `exp` (creeping up a hundredth a second when `gaining`) and
    /// `activity`; every consult is answered at once with `answer`.
    /// Returns (when, reason) of each consult.
    fn play(
        coach: &mut Coach,
        from: f64,
        seconds: f64,
        exp: f32,
        gaining: bool,
        activity: f32,
        answer: Option<&str>,
    ) -> Vec<(f64, Reason)> {
        play_with(
            coach,
            from,
            seconds,
            exp,
            gaining,
            activity,
            |coach, now| coach.answered(now, answer),
        )
    }

    /// [`play`], every consult met with `came_back` (what the worker came
    /// back with) at once.
    fn play_with(
        coach: &mut Coach,
        from: f64,
        seconds: f64,
        exp: f32,
        gaining: bool,
        activity: f32,
        mut came_back: impl FnMut(&mut Coach, f64),
    ) -> Vec<(f64, Reason)> {
        let mut consults = Vec::new();
        let frames = (seconds * 10.0) as usize;
        for i in 0..frames {
            let now = from + i as f64 * 0.1;
            let gained = if gaining { (i / 10) as f32 * 0.01 } else { 0.0 };
            let o = obs(exp + gained, 150);
            let v = verdict(activity, false);
            let g = Glance {
                now,
                obs: &o,
                scene: Some(&v),
                in_view: true,
                talking: false,
                muted: false,
                dead: false,
            };
            if let Some(reason) = coach.observe(&g) {
                consults.push((now, reason));
                came_back(coach, now);
            }
        }
        consults
    }

    #[test]
    fn it_looks_now_and_then_and_less_often_when_it_has_nothing_to_say() {
        let mut coach = Coach::new(true);
        let consults = play(&mut coach, 0.0, 600.0, 18.99, true, 0.02, None);
        let times: Vec<f64> = consults
            .iter()
            .map(|(t, _)| (t * 10.0).round() / 10.0)
            .collect();
        assert!(
            consults.iter().all(|(_, r)| *r == Reason::Look),
            "{consults:?}"
        );
        // The first look once the game has settled in; then every 45 s,
        // growing by half each time it had nothing to say, up to three
        // minutes.
        assert_eq!(times[0], SETTLE_IN);
        let gaps: Vec<f64> = times
            .windows(2)
            .map(|w| ((w[1] - w[0]) * 10.0).round() / 10.0)
            .collect();
        assert_eq!(gaps, vec![67.5, 101.3, 151.9, 180.0], "{times:?}");
        // A line said starts the pace over.
        let more = play(&mut coach, 600.0, 100.0, 25.0, true, 0.02, Some("Buff up."));
        assert_eq!(more.len(), 1, "{more:?}");
        assert_eq!(coach.lines(), vec!["Buff up.".to_string()]);
        let after = play(&mut coach, 700.0, 100.0, 26.0, true, 0.02, None);
        assert_eq!(after.len(), 1, "{after:?}");
        assert!(
            (after[0].0 - more[0].0 - LOOK_EVERY).abs() < 0.11,
            "{after:?} {more:?}"
        );
    }

    #[test]
    fn a_look_called_off_is_not_nothing_to_say_and_the_looks_come_no_less_often() {
        // Every word of the player's calls off the look in flight. Counted
        // as "nothing to say", each one grew the wait by half: a chatty
        // hour had the looks three minutes apart, and then none. Called
        // off, the look is simply had again at the next gap.
        let mut coach = Coach::new(true);
        let consults = play_with(&mut coach, 0.0, 600.0, 18.99, true, 0.02, |coach, _| {
            coach.called_off()
        });
        let times: Vec<f64> = consults
            .iter()
            .map(|(t, _)| (t * 10.0).round() / 10.0)
            .collect();
        assert_eq!(times[0], SETTLE_IN);
        let gaps: Vec<f64> = times
            .windows(2)
            .map(|w| ((w[1] - w[0]) * 10.0).round() / 10.0)
            .collect();
        assert!(gaps.len() >= 10, "{gaps:?}");
        assert!(
            gaps.iter().all(|gap| *gap == LOOK_EVERY),
            "every 45 s, not growing: {gaps:?}"
        );
        assert!(!coach.consulting());
        // A look that did come back with nothing still slows them.
        let silent = play(&mut coach, 600.0, 300.0, 25.0, true, 0.02, None);
        let gaps: Vec<f64> = silent
            .windows(2)
            .map(|w| ((w[1].0 - w[0].0) * 10.0).round() / 10.0)
            .collect();
        assert_eq!(gaps, vec![67.5, 101.3], "{silent:?}");
    }

    #[test]
    fn nothing_going_on_means_nobody_is_playing() {
        let mut coach = Coach::new(true);
        // Away from the keyboard: not a word in ten minutes.
        assert!(play(&mut coach, 0.0, 600.0, 18.99, false, 0.0, None).is_empty());
        // Off, or muted: the same.
        let mut off = Coach::new(false);
        assert!(play(&mut off, 0.0, 120.0, 18.99, true, 0.05, None).is_empty());
        let mut muted = Coach::new(true);
        let o = obs(10.0, 150);
        let v = verdict(0.05, false);
        for i in 0..1200 {
            let g = Glance {
                now: i as f64 * 0.1,
                obs: &o,
                scene: Some(&v),
                in_view: true,
                talking: false,
                muted: true,
                dead: false,
            };
            assert_eq!(muted.observe(&g), None);
        }
    }

    #[test]
    fn a_new_scene_a_level_up_and_a_stall_are_each_looked_at_in_turn() {
        let mut coach = Coach::new(true);
        // Settled in and quiet for a while (one look, silent).
        let first = play(&mut coach, 0.0, 20.0, 18.99, true, 0.02, None);
        assert_eq!(first.len(), 1);
        let mut now = 20.0;
        // A new scene: consulted at once (the consult gap is long past).
        let o = obs(18.99, 150);
        let v = verdict(0.1, true);
        let g = Glance {
            now,
            obs: &o,
            scene: Some(&v),
            in_view: true,
            talking: false,
            muted: false,
            dead: false,
        };
        assert_eq!(coach.observe(&g), Some(Reason::NewScene));
        let said_at = now;
        coach.answered(now, Some("Grab the rune on the left."));
        // A level-up: the companion's cheer first, the coach's word six
        // seconds later — and not before the quiet gap after the last line
        // is over.
        now += 1.0;
        coach.leveled(now, Some(151));
        let consults = play(
            &mut coach,
            now,
            30.0,
            0.5,
            false,
            0.02,
            Some("Put the point in Hurricane."),
        );
        assert_eq!(consults.len(), 1, "{consults:?}");
        assert_eq!(consults[0].1, Reason::LevelUp { level: Some(151) });
        assert!(
            (consults[0].0 - said_at - MIN_GAP).abs() < 0.11,
            "{}",
            consults[0].0 - said_at
        );
        // Then nothing for three minutes while the game goes on: a stall,
        // said once, and again ten minutes on with the longer figure.
        let stalled_since = now;
        now += 30.0;
        let consults = play(
            &mut coach,
            now,
            800.0,
            0.5,
            false,
            0.02,
            Some("Move to a fuller map."),
        );
        let stalls: Vec<&(f64, Reason)> = consults
            .iter()
            .filter(|(_, r)| matches!(r, Reason::ExpStalled { .. }))
            .collect();
        assert_eq!(stalls.len(), 2, "{consults:?}");
        assert_eq!(stalls[0].1, Reason::ExpStalled { minutes: 3 });
        assert!(
            (stalls[0].0 - stalled_since - STALL_AFTER).abs() < 0.5,
            "{}",
            stalls[0].0
        );
        assert_eq!(stalls[1].1, Reason::ExpStalled { minutes: 13 });
        assert!((stalls[1].0 - stalls[0].0 - STALL_AGAIN).abs() < 0.5);
        // EXP moving again clears the stall.
        let moving = play(&mut coach, now + 800.0, 150.0, 0.51, true, 0.02, None);
        assert!(moving.iter().all(|(_, r)| *r == Reason::Look), "{moving:?}");
    }

    #[test]
    fn a_stall_is_fighting_with_no_exp_and_idle_time_never_counts() {
        // A minute's grind (EXP creeping), then twenty idle minutes —
        // nothing moves, EXP flat — then fighting again with nothing
        // dying: no stall in the first three minutes of fighting (one
        // evening had "no EXP for 20 min" consulted, with the picture,
        // 18 s after the player came back from making tea, with EXP
        // already moving), and the stall three minutes into it, counting
        // the fighting only.
        let stalls = |consults: &[(f64, Reason)]| -> Vec<(f64, Reason)> {
            consults
                .iter()
                .filter(|(_, r)| matches!(r, Reason::ExpStalled { .. }))
                .cloned()
                .collect()
        };
        let mut coach = Coach::new(true);
        let grinding = play(&mut coach, 0.0, 60.0, 18.99, true, 0.02, None);
        assert!(
            grinding.iter().all(|(_, r)| *r == Reason::Look),
            "{grinding:?}"
        );
        // (EXP where the grind left it: 18.99 and 59 hundredths.)
        let flat = 19.58;
        let idle = play(&mut coach, 60.0, 1200.0, flat, false, 0.0, None);
        assert!(idle.is_empty(), "{idle:?}");
        let back = play(&mut coach, 1260.0, 179.0, flat, false, 0.02, None);
        assert!(back.iter().all(|(_, r)| *r == Reason::Look), "{back:?}");
        let fighting = play(&mut coach, 1439.0, 61.0, flat, false, 0.02, None);
        let first = stalls(&fighting);
        assert_eq!(first.len(), 1, "{fighting:?}");
        assert_eq!(first[0].1, Reason::ExpStalled { minutes: 3 });
        // Three minutes of play with that reading: the grind's last second
        // (EXP moves once a second there) and 179 s after the tea — at
        // 1439; a look just before it may hold it the consults' gap.
        let after = first[0].0 - 1439.0;
        assert!((-0.5..=CONSULT_GAP + 0.5).contains(&after), "{first:?}");
        // Twenty idle minutes more, then fighting on: said again after ten
        // minutes more of fighting, not at the moment they come back, and
        // the figure counts the fighting only (13 minutes, not 43).
        let idle = play(&mut coach, 1500.0, 1200.0, flat, false, 0.0, None);
        assert!(idle.is_empty(), "{idle:?}");
        let fighting = play(&mut coach, 2700.0, 700.0, flat, false, 0.02, None);
        let again = stalls(&fighting);
        assert_eq!(again.len(), 1, "{fighting:?}");
        assert_eq!(again[0].1, Reason::ExpStalled { minutes: 13 });
        // (A minute of fighting before the tea, nine after: at 3239.)
        let after = again[0].0 - 3239.0;
        assert!((-0.5..=CONSULT_GAP + 0.5).contains(&after), "{again:?}");
    }

    #[test]
    fn a_level_reading_that_flips_is_not_the_coachs_to_celebrate() {
        // The sight's reader gives 165, then 166, then 165 again, every
        // 40 s (a misread it holds for a while): the coach took each 166
        // for a level-up and had the model told "they just hit level 166"
        // five times in 400 s. A level-up reaches the coach only through
        // `leveled`, from the companion's verified one — once.
        fn flipping(coach: &mut Coach, from: f64, seconds: f64) -> Vec<(f64, Reason)> {
            let mut consults = Vec::new();
            for i in 0..(seconds * 10.0) as usize {
                let now = from + i as f64 * 0.1;
                let level = if (i / 400) % 2 == 0 { 165 } else { 166 };
                // (EXP creeping: no stall.)
                let o = obs(10.0 + i as f32 * 0.001, level);
                let v = verdict(0.02, false);
                let g = Glance {
                    now,
                    obs: &o,
                    scene: Some(&v),
                    in_view: true,
                    talking: false,
                    muted: false,
                    dead: false,
                };
                if let Some(reason) = coach.observe(&g) {
                    coach.answered(now, None);
                    consults.push((now, reason));
                }
            }
            consults
        }
        let level_ups = |consults: &[(f64, Reason)]| {
            consults
                .iter()
                .filter(|(_, r)| matches!(r, Reason::LevelUp { .. }))
                .count()
        };
        let mut coach = Coach::new(true);
        let consults = flipping(&mut coach, 0.0, 400.0);
        assert_eq!(level_ups(&consults), 0, "{consults:?}");
        // The companion's verified level-up: one word on it, and the
        // flipping reading after it adds nothing.
        coach.leveled(400.0, Some(166));
        let after = flipping(&mut coach, 400.0, 400.0);
        assert_eq!(level_ups(&after), 1, "{after:?}");
        let up = after
            .iter()
            .find(|(_, r)| matches!(r, Reason::LevelUp { .. }))
            .unwrap();
        assert_eq!(up.1, Reason::LevelUp { level: Some(166) });
        assert!((up.0 - 400.0 - LEVEL_UP_AFTER).abs() < 0.11, "{after:?}");
    }

    #[test]
    fn it_waits_while_someone_talks_and_after_anything_was_said() {
        let mut coach = Coach::new(true);
        play(&mut coach, 0.0, 12.0, 18.99, true, 0.02, None);
        let o = obs(18.99, 150);
        let v = verdict(0.1, true);
        let mut now = 12.0;
        // A new scene while the player is talking: it waits.
        for _ in 0..50 {
            let g = Glance {
                now,
                obs: &o,
                scene: Some(&v),
                in_view: true,
                talking: true,
                muted: false,
                dead: false,
            };
            assert_eq!(coach.observe(&g), None);
            now += 0.1;
        }
        // They stop; a reply was just said: the quiet gap, then the look.
        coach.someone_spoke(now);
        let quiet = verdict(0.02, false);
        let mut asked = None;
        let from = now;
        while asked.is_none() && now - from < 30.0 {
            let g = Glance {
                now,
                obs: &o,
                scene: Some(&quiet),
                in_view: true,
                talking: false,
                muted: false,
                dead: false,
            };
            asked = coach.observe(&g).map(|r| (now, r));
            now += 0.1;
        }
        let (at, reason) = asked.expect("looked at the new scene");
        assert_eq!(reason, Reason::NewScene);
        assert!((at - from - MIN_GAP).abs() < 0.11, "{}", at - from);
        // Nothing more while the consult is under way.
        let g = Glance {
            now,
            obs: &o,
            scene: Some(&quiet),
            in_view: true,
            talking: false,
            muted: false,
            dead: false,
        };
        assert!(coach.consulting());
        assert_eq!(coach.observe(&g), None);
        // A reason that waited too long is dropped (a couple of minutes
        // on, so that a new scene is looked at again at all).
        coach.answered(now, None);
        now += NEW_SCENE_AGAIN;
        let stale = verdict(0.1, true);
        let g = Glance {
            now,
            obs: &o,
            scene: Some(&stale),
            in_view: true,
            talking: true,
            muted: false,
            dead: false,
        };
        assert_eq!(coach.observe(&g), None);
        assert!(matches!(coach.pending, Some((Reason::NewScene, _))));
        now += STALE_AFTER + 1.0;
        let g = Glance {
            now,
            obs: &o,
            scene: Some(&quiet),
            in_view: true,
            talking: false,
            muted: false,
            dead: false,
        };
        assert_ne!(coach.observe(&g), Some(Reason::NewScene));
    }

    /// Play `seconds` of frames (ten a second), the scene cut and settled
    /// on the first when `cut`, the character `dead` throughout; every
    /// consult is answered at once with nothing. Returns (when, reason)
    /// of each consult.
    fn frames(
        coach: &mut Coach,
        from: f64,
        seconds: f64,
        cut: bool,
        dead: bool,
    ) -> Vec<(f64, Reason)> {
        let mut consults = Vec::new();
        let o = obs(18.99, 150);
        for i in 0..(seconds * 10.0) as usize {
            let now = from + i as f64 * 0.1;
            let v = verdict(0.02, cut && i == 0);
            let g = Glance {
                now,
                obs: &o,
                scene: Some(&v),
                in_view: true,
                talking: false,
                muted: false,
                dead,
            };
            if let Some(reason) = coach.observe(&g) {
                consults.push((now, reason));
                coach.answered(now, None);
            }
        }
        consults
    }

    fn new_scenes(consults: &[(f64, Reason)]) -> Vec<f64> {
        consults
            .iter()
            .filter(|(_, r)| *r == Reason::NewScene)
            .map(|(t, _)| *t)
            .collect()
    }

    #[test]
    fn a_new_scene_is_looked_at_once_in_a_while_and_not_around_a_death_or_a_level_up() {
        let mut coach = Coach::new(true);
        play(&mut coach, 0.0, 20.0, 18.99, true, 0.02, None);
        // A cut: looked at. Another forty seconds on (a dialog closing, a
        // full-screen effect) is not: one look in a couple of minutes,
        // whatever cuts. Two minutes after the look: again.
        let first = frames(&mut coach, 20.0, 40.0, true, false);
        assert_eq!(new_scenes(&first), vec![20.0], "{first:?}");
        let second = frames(&mut coach, 60.0, 80.0, true, false);
        assert!(new_scenes(&second).is_empty(), "{second:?}");
        let third = frames(&mut coach, 140.0, 160.0, true, false);
        assert_eq!(new_scenes(&third), vec![140.0], "{third:?}");
        // They die: the death screen cuts and settles, and so does the
        // revive. The companion said what there was to say: no look at
        // either, nor for ten seconds after; then as usual.
        let dead = frames(&mut coach, 300.0, 5.0, true, true);
        assert!(new_scenes(&dead).is_empty(), "{dead:?}");
        let revived = frames(&mut coach, 305.0, 9.0, true, false);
        assert!(new_scenes(&revived).is_empty(), "{revived:?}");
        let later = frames(&mut coach, 315.0, 20.0, true, false);
        assert_eq!(new_scenes(&later), vec![315.0], "{later:?}");
        // A level-up: the companion cheers, and its flash cuts the
        // picture. The coach's word is about the level, not the scene.
        let cheered = 500.0;
        coach.someone_spoke(cheered);
        coach.leveled(cheered, Some(151));
        let mut around = frames(&mut coach, cheered, 3.0, false, false);
        around.extend(frames(&mut coach, cheered + 3.0, 27.0, true, false));
        assert!(new_scenes(&around).is_empty(), "{around:?}");
        let level_ups: Vec<&(f64, Reason)> = around
            .iter()
            .filter(|(_, r)| matches!(r, Reason::LevelUp { .. }))
            .collect();
        assert_eq!(level_ups.len(), 1, "{around:?}");
        assert!(
            (level_ups[0].0 - cheered - MIN_GAP).abs() < 0.11,
            "{around:?}"
        );
        let after = frames(&mut coach, cheered + 30.0, 10.0, true, false);
        assert_eq!(new_scenes(&after), vec![cheered + 30.0], "{after:?}");
        // A cut while they are talking waits for a quiet moment; they die
        // before it comes: it is dropped, not looked at from the grave.
        let o = obs(18.99, 150);
        let v = verdict(0.1, true);
        let g = Glance {
            now: 700.0,
            obs: &o,
            scene: Some(&v),
            in_view: true,
            talking: true,
            muted: false,
            dead: false,
        };
        assert_eq!(coach.observe(&g), None);
        assert!(matches!(coach.pending, Some((Reason::NewScene, _))));
        let died = frames(&mut coach, 701.0, 3.0, false, true);
        let back = frames(&mut coach, 704.0, 30.0, false, false);
        assert!(new_scenes(&died).is_empty() && new_scenes(&back).is_empty());
        assert_eq!(coach.pending, None);
    }

    /// Play `seconds` of frames (ten a second) with HP at `hp` percent
    /// (read from the number), the character `dead` throughout, someone
    /// `talking` throughout, and EXP creeping up (no stall); every consult
    /// is answered at once with nothing. Returns (when, reason) of each
    /// consult.
    fn hp_frames(
        coach: &mut Coach,
        from: f64,
        seconds: f64,
        hp: f32,
        dead: bool,
        talking: bool,
    ) -> Vec<(f64, Reason)> {
        let mut consults = Vec::new();
        let v = verdict(0.02, false);
        for i in 0..(seconds * 10.0) as usize {
            let now = from + i as f64 * 0.1;
            let mut o = obs(18.99 + (i / 10) as f32 * 0.01, 150);
            o.hp = Some(Gauge {
                percent: hp,
                current: Some((hp * 100.0) as u64),
                max: Some(10_000),
                read: true,
            });
            let g = Glance {
                now,
                obs: &o,
                scene: Some(&v),
                in_view: true,
                talking,
                muted: false,
                dead,
            };
            if let Some(reason) = coach.observe(&g) {
                consults.push((now, reason));
                coach.answered(now, None);
            }
        }
        consults
    }

    fn close_calls(consults: &[(f64, Reason)]) -> Vec<(f64, u32)> {
        consults
            .iter()
            .filter_map(|(t, r)| match r {
                Reason::CloseCall { lowest } => Some(((t * 10.0).round() / 10.0, *lowest)),
                _ => None,
            })
            .collect()
    }

    fn streaks(consults: &[(f64, Reason)]) -> Vec<(f64, u32, u32)> {
        consults
            .iter()
            .filter_map(|(t, r)| match r {
                Reason::Streak { deaths, minutes } => {
                    Some(((t * 10.0).round() / 10.0, *deaths, *minutes))
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_close_call_is_remarked_on_once_in_a_while_and_never_from_the_grave() {
        let mut coach = Coach::new(true);
        play(&mut coach, 0.0, 20.0, 18.99, true, 0.02, None);
        // A hit to 8%, a potion a second later: a close call, looked at as
        // soon as the save has held a moment (nothing was said lately).
        let mut all = hp_frames(&mut coach, 20.0, 1.0, 8.0, false, false);
        all.extend(hp_frames(&mut coach, 21.0, 30.0, 60.0, false, false));
        assert_eq!(close_calls(&all), vec![(21.6, 8)], "{all:?}");
        // The same a minute on: not again within five minutes.
        let mut again = hp_frames(&mut coach, 80.0, 1.0, 7.0, false, false);
        again.extend(hp_frames(&mut coach, 81.0, 30.0, 60.0, false, false));
        assert!(close_calls(&again).is_empty(), "{again:?}");
        // Five minutes on: a dip that ends in a death is no close call,
        // and neither is the revive after it (HP from nothing to full).
        let mut died = hp_frames(&mut coach, 330.0, 1.0, 8.0, false, false);
        died.extend(hp_frames(&mut coach, 331.0, 3.0, 0.0, true, false));
        died.extend(hp_frames(&mut coach, 334.0, 30.0, 100.0, false, false));
        assert!(close_calls(&died).is_empty(), "{died:?}");
        // One frame at 8% is a misread, not a dip.
        let mut flicker = hp_frames(&mut coach, 400.0, 0.1, 8.0, false, false);
        flicker.extend(hp_frames(&mut coach, 400.1, 10.0, 60.0, false, false));
        assert!(close_calls(&flicker).is_empty(), "{flicker:?}");
        // Sitting at 8% for half a minute before the potion is not a close
        // call (the companion's warnings are for that)…
        let mut slow = hp_frames(&mut coach, 420.0, 25.0, 8.0, false, false);
        slow.extend(hp_frames(&mut coach, 445.0, 10.0, 60.0, false, false));
        assert!(close_calls(&slow).is_empty(), "{slow:?}");
        // …nor is a slow climb back through the thirties.
        let mut climb = hp_frames(&mut coach, 460.0, 1.0, 8.0, false, false);
        climb.extend(hp_frames(&mut coach, 461.0, 25.0, 30.0, false, false));
        climb.extend(hp_frames(&mut coach, 486.0, 10.0, 60.0, false, false));
        assert!(close_calls(&climb).is_empty(), "{climb:?}");
        // A dip to 5 that comes back through 30 to 60 within the time: a
        // close call, with the lowest it got.
        let mut saved = hp_frames(&mut coach, 600.0, 1.0, 5.4, false, false);
        saved.extend(hp_frames(&mut coach, 601.0, 5.0, 30.0, false, false));
        saved.extend(hp_frames(&mut coach, 606.0, 30.0, 60.0, false, false));
        assert_eq!(close_calls(&saved), vec![(606.6, 5)], "{saved:?}");
        // One while they talk waits for a quiet moment, and is dropped
        // when none comes within the minute: "that was close" has a shelf
        // life.
        let mut talked = hp_frames(&mut coach, 1000.0, 1.0, 8.0, false, true);
        talked.extend(hp_frames(&mut coach, 1001.0, 30.0, 60.0, false, true));
        assert!(talked.is_empty());
        assert!(matches!(
            coach.pending,
            Some((Reason::CloseCall { lowest: 8 }, _))
        ));
        talked.extend(hp_frames(&mut coach, 1031.0, 40.0, 60.0, false, true));
        coach.someone_spoke(1071.0);
        talked.extend(hp_frames(&mut coach, 1071.0, 60.0, 60.0, false, false));
        assert!(close_calls(&talked).is_empty(), "{talked:?}");
        assert_eq!(coach.pending, None);
    }

    #[test]
    fn a_close_call_is_reacted_to_at_once_not_a_quarter_minute_after_the_warning() {
        // The companion's own "HP 8 percent. Pot now!" at 20 s, the potion
        // a second later: "that was close" waited the quarter minute's
        // quiet after the warning and was consulted at 35 s, with the
        // fight long over. A reaction waits for nothing but the talking
        // to stop: consulted as soon as the save has held.
        let mut coach = Coach::new(true);
        play(&mut coach, 0.0, 20.0, 18.99, true, 0.02, None);
        coach.someone_spoke(20.0);
        let mut all = hp_frames(&mut coach, 20.0, 1.0, 8.0, false, false);
        all.extend(hp_frames(&mut coach, 21.0, 30.0, 60.0, false, false));
        assert_eq!(close_calls(&all), vec![(21.6, 8)], "{all:?}");
        // With the warning's clip still playing for three seconds after
        // the save: as soon as it is done.
        hp_frames(&mut coach, 51.0, 349.0, 60.0, false, false);
        coach.someone_spoke(400.0);
        let mut all = hp_frames(&mut coach, 400.0, 1.0, 8.0, false, true);
        all.extend(hp_frames(&mut coach, 401.0, 3.0, 60.0, false, true));
        all.extend(hp_frames(&mut coach, 404.0, 30.0, 60.0, false, false));
        assert_eq!(close_calls(&all), vec![(404.0, 8)], "{all:?}");
        // A reaction carries its facts: no picture for it; company looks.
        assert!(!Reason::CloseCall { lowest: 8 }.wants_picture());
        assert!(
            !Reason::Streak {
                deaths: 3,
                minutes: 7
            }
            .wants_picture()
        );
        for company in [
            Reason::Look,
            Reason::NewScene,
            Reason::LevelUp { level: None },
            Reason::ExpStalled { minutes: 3 },
        ] {
            assert!(company.wants_picture(), "{company:?}");
            assert!(!company.is_reaction(), "{company:?}");
        }
    }

    #[test]
    fn the_third_death_in_ten_minutes_is_a_streak_said_once_per_streak() {
        let mut coach = Coach::new(true);
        play(&mut coach, 0.0, 20.0, 18.99, true, 0.02, None);
        // A death: the companion says so at once; dead three seconds, then
        // back at full. The coach's word comes after the quiet gap.
        let die = |coach: &mut Coach, at: f64| -> Vec<(f64, Reason)> {
            coach.someone_spoke(at);
            let mut consults = hp_frames(coach, at, 3.0, 0.0, true, false);
            consults.extend(hp_frames(coach, at + 3.0, 40.0, 100.0, false, false));
            consults
        };
        let first = die(&mut coach, 100.0);
        // (Thirty frames dead are one death.)
        assert_eq!(coach.deaths.len(), 1);
        let second = die(&mut coach, 300.0);
        assert!(streaks(&first).is_empty() && streaks(&second).is_empty());
        // The third, 6 min 40 s after the first: a streak, looked at as
        // soon as the companion's line is out of the way (a reaction, not
        // company: no quarter minute's wait).
        let third = die(&mut coach, 500.0);
        assert_eq!(streaks(&third), vec![(500.0, 3, 7)], "{third:?}");
        // A fourth and a fifth, each within ten minutes of the last: the
        // same streak, said already.
        let fourth = die(&mut coach, 700.0);
        let fifth = die(&mut coach, 1000.0);
        assert!(streaks(&fourth).is_empty() && streaks(&fifth).is_empty());
        // Ten minutes without a death: the streak is over. Three more
        // within four minutes: a new one.
        hp_frames(&mut coach, 1043.0, 600.0, 100.0, false, false);
        let mut next = die(&mut coach, 1700.0);
        next.extend(die(&mut coach, 1800.0));
        next.extend(die(&mut coach, 1900.0));
        assert_eq!(streaks(&next), vec![(1900.0, 3, 4)], "{next:?}");
        // Two deaths far apart are no streak at all.
        let mut sparse = Coach::new(true);
        play(&mut sparse, 0.0, 20.0, 18.99, true, 0.02, None);
        let mut lone = die(&mut sparse, 100.0);
        lone.extend(die(&mut sparse, 800.0));
        lone.extend(die(&mut sparse, 1500.0));
        assert!(streaks(&lone).is_empty(), "{lone:?}");
    }

    #[test]
    fn looks_come_less_often_while_the_player_talks_a_lot() {
        // A player who plays and says nothing: the usual pace, five looks
        // in ten minutes (see above).
        let mut quiet = Coach::new(true);
        let silent = play(&mut quiet, 0.0, 600.0, 18.99, true, 0.02, None);
        assert_eq!(silent.len(), 5);
        // One who talks for two minutes after the first look (a reply said
        // at the end of it): the next look is put off by those two minutes
        // — 67.5 s after the last look as usual, plus 120 s of talk.
        let mut chatty = Coach::new(true);
        let first = play(&mut chatty, 0.0, 20.0, 18.99, true, 0.02, None);
        assert_eq!(first.len(), 1);
        assert!(hp_frames(&mut chatty, 20.0, 120.0, 90.0, false, true).is_empty());
        chatty.someone_spoke(140.0);
        let after = hp_frames(&mut chatty, 140.0, 460.0, 90.0, false, false);
        assert!(after.iter().all(|(_, r)| *r == Reason::Look), "{after:?}");
        let times: Vec<f64> = after.iter().map(|(t, _)| *t).collect();
        assert!((times[0] - 197.5).abs() < 0.15, "{times:?}");
        // The next: 101.25 s on as usual, plus whatever of the talk is
        // still within the last five minutes (it slides out of the window
        // a second at a time from 320 s on): at 369 s, 70 s of it.
        assert!((times[1] - 369.1).abs() < 0.15, "{times:?}");
        // Two minutes of talk cost a look in the ten minutes; the looks
        // never stop for good.
        assert_eq!(after.len() + first.len(), 4, "{times:?}");
        // The pace constants themselves are as they were.
        assert_eq!((LOOK_EVERY, LOOK_AT_MOST, MIN_GAP), (45.0, 180.0, 15.0));
    }

    #[test]
    fn every_reason_reacts_in_the_attitudes_voice_and_never_narrates() {
        let reasons = [
            Reason::CloseCall { lowest: 8 },
            Reason::Streak {
                deaths: 3,
                minutes: 7,
            },
            Reason::NewScene,
            Reason::LevelUp { level: Some(151) },
            Reason::LevelUp { level: None },
            Reason::ExpStalled { minutes: 4 },
            Reason::Look,
        ];
        for reason in &reasons {
            for attitude in Attitude::ALL {
                let text = reason.describe(attitude);
                // Quiet is always an answer.
                assert!(text.contains("[silent]"), "{text}");
                // Its own attitude's lines, the number filled in, and no
                // other attitude's.
                let number = reason.number().map(|n| n.to_string()).unwrap_or_default();
                for line in attitude.lines(reason.example_lines()) {
                    let filled = format!("\"{}\"", line.replace("{}", &number));
                    assert!(text.contains(&filled), "{text}\nmissing {filled}");
                }
                for other in Attitude::ALL.into_iter().filter(|a| *a != attitude) {
                    for line in other.lines(reason.example_lines()) {
                        let filled = format!("\"{}\"", line.replace("{}", &number));
                        assert!(!text.contains(&filled), "{text}\nhas {filled}");
                    }
                }
                assert!(!text.contains("{}"), "{text}");
            }
        }
        // What each says happened, and what a reaction to it is.
        let blunt = |r: &Reason| r.describe(Attitude::Blunt);
        assert!(blunt(&reasons[0]).starts_with(
            "That was close: their HP dropped to 8% a moment ago and they pulled it back. Say it \
the way a friend blurts it out"
        ));
        assert!(blunt(&reasons[1]).starts_with(
            "That's their 3rd death in 7 minutes. Say the thing a friend says now, once, in one \
line: a breather, an easier map, or what keeps killing them"
        ));
        let scene = blunt(&reasons[2]);
        assert!(scene.contains("React, don't narrate"), "{scene}");
        assert!(
            scene.contains("Never say what the screen shows, and never name"),
            "{scene}"
        );
        assert!(
            blunt(&reasons[3]).starts_with(
                "They just hit level 151; the cheer was said already, don't repeat it."
            )
        );
        assert!(blunt(&reasons[4]).starts_with("They just hit a new level;"));
        assert!(
            blunt(&reasons[5])
                .starts_with("Their EXP hasn't moved for 4 minutes: nothing is dying.")
        );
        let look = blunt(&reasons[6]);
        assert!(
            look.contains("you're just glancing at their screen"),
            "{look}"
        );
        assert!(
            look.contains("else [silent], as most glances are"),
            "{look}"
        );
        // The look's examples are of what a small, soft picture shows (a
        // crowd, a boss, standing still), never of what it cannot (loot,
        // runes, buffs): "Loot. Behind you." was an invitation to invent
        // one, and a wrong callout is worse than silence.
        assert!(
            look.contains(
                "you can't make out loot, runes or buff icons in it, so never call those"
            ),
            "{look}"
        );
        for attitude in Attitude::ALL {
            for line in attitude.lines(examples::LOOK) {
                let lower = line.to_lowercase();
                for unseen in ["loot", "rune", "buff"] {
                    assert!(!lower.contains(unseen), "{line:?}");
                }
            }
        }
        // A new scene and a level-up: the examples use only what the
        // reason carries — somewhere new, a level gained — and never a
        // portal, a quest, an NPC, a named map or the player's gear, which
        // a 640×400 picture and a level number cannot support ("That's
        // the long way round. Portal's left.", "Fourth job quest's up.
        // Go." — false at 150); and the model is told so in words.
        for reason in [&reasons[2], &reasons[3]] {
            for attitude in Attitude::ALL {
                assert!(
                    reason.describe(attitude).contains(
                        "never name a portal, a quest, an NPC or a map you can't see in the picture"
                    ),
                    "{}",
                    reason.describe(attitude)
                );
            }
        }
        for lists in [examples::NEW_SCENE, examples::LEVEL_UP] {
            for attitude in Attitude::ALL {
                for line in attitude.lines(lists) {
                    let lower = line.to_lowercase();
                    for unseen in [
                        "portal",
                        "quest",
                        "instructor",
                        "job",
                        "monster park",
                        "wrong map",
                        "boss at the end",
                        "gear",
                        "naked",
                        "under you",
                        "beneath you",
                        "left",
                        "right",
                    ] {
                        assert!(!lower.contains(unseen), "{line:?} names {unseen:?}");
                    }
                }
            }
        }
        // The savage close call, as the model gets it.
        assert!(reasons[0].describe(Attitude::Savage).ends_with(
            "Like: \"That was close, you absolute clown.\" \"8 percent. One more hit and I'd be \
writing your eulogy.\" \"Lucky, not good. Pot before you're at 8 next time.\""
        ));
        // The examples: three per attitude and reason, none twice, each a
        // short line a friend says (no questions: a friend who sees
        // something says it), and savage insults the play, not the person.
        let mut all_lines = Vec::new();
        for (name, lists) in examples::ALL {
            for attitude in Attitude::ALL {
                let lines = attitude.lines(*lists);
                assert_eq!(lines.len(), 3, "{name} {}", attitude.word());
                for line in lines {
                    assert!(!line.contains('?'), "{name}: {line:?}");
                    assert!(line.split_whitespace().count() <= 16, "{name}: {line:?}");
                    all_lines.push(*line);
                }
            }
        }
        let distinct: std::collections::HashSet<&str> = all_lines.iter().copied().collect();
        assert_eq!(distinct.len(), all_lines.len());
        // Labels for the log.
        assert_eq!(reasons[0].label(), "close call (8%)");
        assert_eq!(reasons[1].label(), "3 deaths in 7 min");
        assert_eq!(ordinal(3), "3rd");
        assert_eq!(ordinal(11), "11th");
        assert_eq!(ordinal(22), "22nd");
    }
}
