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
//! - they just arrived somewhere new (the scene cut and settled);
//! - they just went up a level (a moment after the companion's cheer);
//! - their EXP has not moved for minutes while the game goes on;
//! - nothing in particular: a look now and then, less often each time the
//!   model has nothing to say.
//!
//! Pacing is deterministic and tested here: a model is consulted at most
//! every few seconds and only while nobody is talking, and nothing is said
//! sooner than a quarter minute after the last unprompted line. The model
//! is never asked what to say about a danger: the companion's own lines
//! for those come at once, with no model in the way.

pub mod scene;

use std::collections::VecDeque;

use crate::companion::Observation;

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
}

/// Why the model is consulted.
#[derive(Debug, Clone, PartialEq)]
pub enum Reason {
    /// They just arrived somewhere new.
    NewScene,
    /// They just reached this level (None when the number is not known).
    LevelUp { level: Option<u32> },
    /// No EXP for this many minutes.
    ExpStalled { minutes: u32 },
    /// Nothing in particular: a look at the game now and then.
    Look,
}

impl Reason {
    /// The more urgent, the lower.
    fn rank(&self) -> u8 {
        match self {
            Reason::NewScene => 0,
            Reason::LevelUp { .. } => 1,
            Reason::ExpStalled { .. } => 2,
            Reason::Look => 3,
        }
    }

    /// What happened, for the model.
    pub fn describe(&self) -> String {
        match self {
            Reason::NewScene => "They just arrived somewhere new (a different map or screen). If you know \
what to do here, or what to watch out for, say it in one line; else [silent]."
                .to_string(),
            Reason::LevelUp { level: Some(level) } => format!(
                "They just reached level {level}. Anything to do now (a new skill to put points in, a better \
map for this level, a quest that just unlocked), in one line; else [silent]."
            ),
            Reason::LevelUp { level: None } => "They just went up a level. Anything to do now (a new skill, a \
better map, a quest that just unlocked), in one line; else [silent]."
                .to_string(),
            Reason::ExpStalled { minutes } => format!(
                "Their EXP hasn't moved for {minutes} minutes: they aren't killing anything. The picture tells \
why: a menu or shop open, standing around, a map with nothing on it, a boss that isn't dead yet, trading. If \
they should be doing something else, tell them what; if what they're doing makes sense, [silent]."
            ),
            Reason::Look => "Nothing in particular happened; you're just watching. A short callout only if \
something on screen deserves one right now (danger coming, a wasted buff, loot on the ground, a better move); \
otherwise [silent]."
                .to_string(),
        }
    }

    /// A few words for the log.
    pub fn label(&self) -> String {
        match self {
            Reason::NewScene => "new scene".into(),
            Reason::LevelUp { level: Some(level) } => format!("level {level}"),
            Reason::LevelUp { level: None } => "level up".into(),
            Reason::ExpStalled { minutes } => format!("no EXP for {minutes} min"),
            Reason::Look => "a look".into(),
        }
    }
}

/// How often the model looks when nothing happens, in seconds, to start
/// with; each time it has nothing to say the wait grows by half, up to
/// [`LOOK_AT_MOST`]; a line said starts it over.
pub const LOOK_EVERY: f64 = 45.0;
pub const LOOK_AT_MOST: f64 = 180.0;
/// Nothing unprompted is said sooner than this after anything was said
/// (by anyone), in seconds: a line a quarter minute is company, two in a
/// row is nagging.
pub const MIN_GAP: f64 = 15.0;
/// Two consults are at least this far apart, in seconds.
pub const CONSULT_GAP: f64 = 8.0;
/// EXP unmoved for this long is a stall, in seconds…
pub const STALL_AFTER: f64 = 180.0;
/// …said again (with the longer figure) after this long.
pub const STALL_AGAIN: f64 = 600.0;
/// The companion cheers a level-up first; the coach's word comes after.
pub const LEVEL_UP_AFTER: f64 = 6.0;
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

pub struct Coach {
    /// Coaching is on: it may speak up on its own.
    pub on: bool,
    /// When anyone last said anything.
    quiet_since: f64,
    last_consult: f64,
    look_every: f64,
    silent_in_a_row: u32,
    /// A consult is under way.
    consulting: bool,
    /// When the EXP reading last changed, and what it was.
    exp_changed: Option<(f64, f32)>,
    stall_told: Option<f64>,
    level: Option<u32>,
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
            look_every: LOOK_EVERY,
            silent_in_a_row: 0,
            consulting: false,
            exp_changed: None,
            stall_told: None,
            level: None,
            pending: None,
            level_up: None,
            seen_since: None,
            activity: 0.0,
            said: VecDeque::new(),
            consults: 0,
            spoken: 0,
        }
    }

    /// One frame. Returns a reason to consult the model now, if it is time.
    pub fn observe(&mut self, g: &Glance) -> Option<Reason> {
        let now = g.now;
        self.track(g);
        if !self.on || g.muted || !g.in_view || !g.obs.game.is_seen() {
            self.pending = None;
            self.level_up = None;
            if !g.in_view || !g.obs.game.is_seen() {
                self.seen_since = None;
            }
            return None;
        }
        let since = *self.seen_since.get_or_insert(now);
        if now - since < SETTLE_IN {
            return None;
        }
        // What came up this frame.
        if g.scene.is_some_and(|s| s.new_scene) {
            self.propose(Reason::NewScene, now);
        }
        if let Some((at, level)) = self.level_up
            && now >= at
        {
            self.level_up = None;
            self.propose(Reason::LevelUp { level }, now);
        }
        if let Some((changed, _)) = self.exp_changed
            && now - changed >= STALL_AFTER
            && self.stall_told.is_none_or(|t| now - t >= STALL_AGAIN)
            && self.activity > IDLE_ACTIVITY
        {
            self.stall_told = Some(now);
            let minutes = ((now - changed) / 60.0).floor() as u32;
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
        if quiet && apart && self.pending.is_some() {
            let (reason, _) = self.pending.take()?;
            return Some(self.consult(reason, now));
        }
        if quiet && now - self.last_consult >= self.look_every && self.activity > IDLE_ACTIVITY {
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
        if let Some(level) = g.obs.level {
            if let Some(before) = self.level
                && level == before + 1
            {
                self.leveled(now, Some(level));
            }
            self.level = Some(level);
        }
        if let Some(exp) = g.obs.exp {
            // A printed number moves in hundredths; a bar's fill flickers.
            let tolerance = if exp.read { 0.005 } else { 0.3 };
            match self.exp_changed {
                Some((_, before)) if (exp.percent - before).abs() <= tolerance => {}
                _ => {
                    self.exp_changed = Some((now, exp.percent));
                    self.stall_told = None;
                }
            }
        }
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
        self.consults += 1;
        reason
    }

    /// A level-up (from the companion, which sees it first): the coach's
    /// word comes a moment after the cheer.
    pub fn leveled(&mut self, now: f64, level: Option<u32>) {
        self.level_up = match self.level_up {
            None => Some((now + LEVEL_UP_AFTER, level)),
            // The number came after the cheer.
            Some((at, None)) => Some((at, level)),
            same => same,
        };
        // A level-up moves the EXP bar, whatever the readings say.
        self.exp_changed = Some((now, 0.0));
        self.stall_told = None;
    }

    /// Anyone said anything (the player, a reply, an alert): the coach
    /// keeps quiet for a while.
    pub fn someone_spoke(&mut self, now: f64) {
        self.quiet_since = now;
    }

    /// The consult came back: `line` was said, or nothing was (None).
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
            };
            if let Some(reason) = coach.observe(&g) {
                consults.push((now, reason));
                coach.answered(now, answer);
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
        };
        assert!(coach.consulting());
        assert_eq!(coach.observe(&g), None);
        // A reason that waited too long is dropped.
        coach.answered(now, None);
        let stale = verdict(0.1, true);
        let g = Glance {
            now,
            obs: &o,
            scene: Some(&stale),
            in_view: true,
            talking: true,
            muted: false,
        };
        assert_eq!(coach.observe(&g), None);
        now += STALE_AFTER + 1.0;
        let g = Glance {
            now,
            obs: &o,
            scene: Some(&quiet),
            in_view: true,
            talking: false,
            muted: false,
        };
        assert_ne!(coach.observe(&g), Some(Reason::NewScene));
    }
}
