//! What the screen shows now, and what changed since the last look.
//!
//! The main loop builds a [`Scene`] from everything MapleSyrup reads — the
//! HUD's numbers with their age, the map, the character, what moves on the
//! screen and where, a fight, a dialog and its text, the buff icons, the
//! things the player taught — and hands it to a [`Feed`], which says what
//! is new enough to push to Claude: a death, a level-up, a new map, HP in
//! danger and out of it, MP all but gone, a dialog, the game window going
//! or coming back, and now and then the state of things. Each event goes
//! out with the scene's text, so Claude always has the whole reading.

use serde::Serialize;

/// The game window, as the capture finds it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Window {
    /// Open and the window in front: read every frame.
    InFront,
    /// Open, behind another window: not looked at (another window would be
    /// read instead).
    Behind,
    NotOpen,
    /// There, but it could not be captured (minimised…).
    Unavailable,
}

/// A bar of the HUD.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Bar {
    /// 0 to 100.
    pub percent: f32,
    pub current: Option<u64>,
    pub max: Option<u64>,
    /// From the numbers the game printed (true) or measured off the bar's
    /// fill (false: an estimate).
    pub read: bool,
    /// Seconds since this value was last seen on the screen.
    pub age: f64,
}

/// Something moving on the screen (a monster, the character, an NPC, a
/// drop settling — the motion does not say which), as fractions of the
/// frame from its top left: its centre and its size.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Blob {
    pub id: u64,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// An open dialog or window (an NPC's, a quest's), and its text when read.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Dialog {
    pub kind: String,
    pub text: Option<String>,
    /// Its centre, as fractions of the frame.
    pub x: f32,
    pub y: f32,
}

/// A thing the player taught MapleSyrup to recognise, and what it shows.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Taught {
    pub name: String,
    /// In a few words: "2 on screen (left, middle)", "about 40%", "showing".
    pub now: String,
    /// Where it is, as fractions of the frame (objects).
    pub places: Vec<(f32, f32)>,
}

/// The session so far.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Session {
    pub minutes: f64,
    /// EXP percent gained per hour, lately.
    pub exp_per_hour: Option<f64>,
    pub next_level_minutes: Option<f64>,
    pub deaths: u32,
    pub minutes_since_death: Option<f64>,
    pub level_ups: u32,
    /// The lowest HP read in the last minute, in percent.
    pub lowest_hp_lately: Option<f32>,
}

/// Everything MapleSyrup reads off the screen at one moment.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Scene {
    /// Seconds since MapleSyrup started.
    pub at: f64,
    /// The local time, `HH:MM:SS`.
    pub clock: String,
    pub window: Window,
    /// Why it could not be captured, or the window's title.
    pub window_detail: Option<String>,
    /// MapleStory Classic World (the classic HUD is on the screen).
    pub classic: bool,
    pub name: Option<String>,
    pub job: Option<String>,
    pub level: Option<u32>,
    pub hp: Option<Bar>,
    pub mp: Option<Bar>,
    pub exp: Option<Bar>,
    pub map: Option<String>,
    /// Seconds since the map's name was read.
    pub map_age: Option<f64>,
    pub dead: bool,
    /// What moves on the screen, the largest first (a few of them).
    pub moving: Vec<Blob>,
    /// How many things move in all (None: motion is not being watched).
    pub moving_total: Option<usize>,
    /// How much is going on: "calm", "light", "busy"… (None: not watched).
    pub combat: Option<String>,
    pub dialog: Option<Dialog>,
    /// Buff and cooldown icons in their row (None: not watched).
    pub buffs: Option<usize>,
    pub taught: Vec<Taught>,
    pub session: Session,
}

impl Scene {
    /// A scene with nothing read yet.
    pub fn empty(at: f64, clock: &str, window: Window) -> Scene {
        Scene {
            at,
            clock: clock.to_string(),
            window,
            window_detail: None,
            classic: false,
            name: None,
            job: None,
            level: None,
            hp: None,
            mp: None,
            exp: None,
            map: None,
            map_age: None,
            dead: false,
            moving: Vec::new(),
            moving_total: None,
            combat: None,
            dialog: None,
            buffs: None,
            taught: Vec::new(),
            session: Session::default(),
        }
    }

    /// HP as it reads now: a value read in the last few seconds.
    fn hp_now(&self) -> Option<f32> {
        self.hp
            .as_ref()
            .filter(|b| b.age <= FRESH)
            .map(|b| b.percent)
    }

    fn mp_now(&self) -> Option<f32> {
        self.mp
            .as_ref()
            .filter(|b| b.age <= FRESH)
            .map(|b| b.percent)
    }

    /// The scene in plain lines, for Claude: every reading with its age.
    pub fn text(&self) -> String {
        let mut lines = Vec::new();
        let game = if self.classic {
            "MapleStory (Classic World)"
        } else {
            "MapleStory"
        };
        lines.push(match self.window {
            Window::InFront => format!("Now {}: {game} is open, the window in front.", self.clock),
            Window::Behind => format!(
                "Now {}: {game} is open but behind another window, so the screen isn't being read.",
                self.clock
            ),
            Window::NotOpen => format!("Now {}: no MapleStory window is open.", self.clock),
            Window::Unavailable => format!(
                "Now {}: MapleStory can't be captured ({}).",
                self.clock,
                self.window_detail.as_deref().unwrap_or("unknown reason")
            ),
        });
        let mut who = Vec::new();
        if let Some(name) = &self.name {
            who.push(name.clone());
        }
        if let Some(job) = &self.job {
            who.push(job.clone());
        }
        if let Some(level) = self.level {
            who.push(format!("level {level}"));
        }
        if !who.is_empty() {
            lines.push(format!("Character: {}.", who.join(", ")));
        }
        if self.dead {
            lines.push("The character is dead.".to_string());
        }
        let bars: Vec<String> = [("HP", &self.hp), ("MP", &self.mp), ("EXP", &self.exp)]
            .into_iter()
            .filter_map(|(name, bar)| bar.as_ref().map(|b| bar_text(name, b)))
            .collect();
        if bars.is_empty() {
            if self.window == Window::InFront {
                lines.push("HP, MP and EXP can't be read right now.".to_string());
            }
        } else {
            lines.push(format!("{}.", bars.join("; ")));
        }
        if let Some(map) = &self.map {
            lines.push(match self.map_age {
                Some(age) if age >= 60.0 => {
                    format!("Map: {map} (read {} ago; it may have changed).", ago(age))
                }
                Some(age) => format!("Map: {map} (read {} ago).", ago(age)),
                None => format!("Map: {map}."),
            });
        }
        if let Some(total) = self.moving_total {
            let mut line = match total {
                0 => "Nothing is moving on the screen".to_string(),
                1 => "1 thing moving on the screen".to_string(),
                n => format!("{n} things moving on the screen"),
            };
            if !self.moving.is_empty() {
                let places: Vec<String> = self
                    .moving
                    .iter()
                    .map(|b| format!("({:.0}%, {:.0}%)", b.x * 100.0, b.y * 100.0))
                    .collect();
                line.push_str(&format!(
                    ", the biggest at {} (x, y from the top left)",
                    places.join(", ")
                ));
            }
            if let Some(combat) = &self.combat {
                line.push_str(&format!("; action: {combat}"));
            }
            lines.push(format!("{line}."));
        }
        if let Some(dialog) = &self.dialog {
            lines.push(match &dialog.text {
                Some(text) => format!(
                    "A {} is open on the screen, reading: \"{text}\".",
                    dialog.kind
                ),
                None => format!(
                    "A {} is open on the screen (its text isn't read).",
                    dialog.kind
                ),
            });
        }
        if let Some(icons) = self.buffs.filter(|n| *n > 0) {
            lines.push(format!(
                "Small icons in a row at the top right: {icons} (buffs or cooldowns, or other UI — not told apart)."
            ));
        }
        if !self.taught.is_empty() {
            let things: Vec<String> = self
                .taught
                .iter()
                .map(|t| format!("{}: {}", t.name, t.now))
                .collect();
            lines.push(format!(
                "Things he taught MapleSyrup to recognise: {}.",
                things.join("; ")
            ));
        }
        let s = &self.session;
        let mut session = vec![format!("Session: {}", minutes(s.minutes))];
        if let Some(rate) = s.exp_per_hour {
            session.push(format!("EXP {rate:+.1}% per hour"));
        }
        if let Some(next) = s.next_level_minutes {
            session.push(format!("next level in about {}", minutes(next)));
        }
        match (s.deaths, s.minutes_since_death) {
            (0, _) => {}
            (1, Some(m)) => session.push(format!("died once ({} ago)", minutes(m))),
            (n, Some(m)) => session.push(format!("died {n} times (last {} ago)", minutes(m))),
            (n, None) => session.push(format!("died {n} times")),
        }
        if s.level_ups > 0 {
            session.push(format!("{} level-up(s)", s.level_ups));
        }
        if let Some(low) = s.lowest_hp_lately {
            session.push(format!("lowest HP in the last minute {low:.0}%"));
        }
        lines.push(format!("{}.", session.join("; ")));
        lines.join("\n")
    }
}

/// A value read this recently is the value now.
const FRESH: f64 = 5.0;

fn bar_text(name: &str, bar: &Bar) -> String {
    let pct = if name == "EXP" {
        format!("{:.2}%", bar.percent)
    } else if bar.percent >= 10.0 {
        format!("{:.0}%", bar.percent)
    } else {
        format!("{:.1}%", bar.percent)
    };
    let value = match (bar.current, bar.max) {
        (Some(c), Some(m)) if name != "EXP" => format!("{name} {c}/{m} ({pct})"),
        (Some(c), _) if name == "EXP" => format!("{name} {c} ({pct})"),
        _ if bar.read => format!("{name} {pct}"),
        _ => format!("{name} about {pct} (from the bar)"),
    };
    if bar.age < 1.5 {
        format!("{value}, read just now")
    } else if bar.age <= FRESH {
        format!("{value}, read {} ago", ago(bar.age))
    } else {
        format!("{value}, last read {} ago (may have changed)", ago(bar.age))
    }
}

fn ago(seconds: f64) -> String {
    let s = seconds.max(0.0).round() as u64;
    match s {
        0..=89 => format!("{s} s"),
        90..=5399 => format!("{} min", (s + 30) / 60),
        _ => format!("{:.1} h", s as f64 / 3600.0),
    }
}

fn minutes(m: f64) -> String {
    let m = m.max(0.0);
    if m < 1.0 {
        "under a minute".to_string()
    } else if m < 90.0 {
        format!("{:.0} min", m)
    } else {
        let h = (m / 60.0).floor();
        format!("{h:.0} h {:.0} min", m - h * 60.0)
    }
}

/// Something new to tell Claude.
#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    /// What kind: `death`, `level_up`, `map`, `hp`, `mp`, `dialog`,
    /// `window`, `state`, `thing`.
    pub kind: &'static str,
    /// What happened, in a line.
    pub headline: String,
}

impl Event {
    fn new(kind: &'static str, headline: String) -> Event {
        Event { kind, headline }
    }
}

/// HP's danger, with room between the marks so a bar that hovers at one
/// is not news every second.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Band {
    Fine,
    Low,
    Critical,
}

/// HP at or under this is low; this far back up, fine again.
const HP_LOW: f32 = 30.0;
const HP_CRITICAL: f32 = 15.0;
const HP_OUT_OF_CRITICAL: f32 = 22.0;
const HP_FINE_AGAIN: f32 = 45.0;
/// MP all but gone (a mage's MP goes up and down all the time: only the
/// bottom of it is news), and back.
const MP_GONE: f32 = 10.0;
const MP_BACK: f32 = 25.0;
/// A window that changed stays changed this long before it is news (a
/// glance at another window and back is not).
const WINDOW_HOLDS: f64 = 4.0;
/// A new level must read the same this long (a misread digit is not a
/// level-up).
const LEVEL_HOLDS: f64 = 2.0;
/// How often the state of things goes out while the game is played, when
/// something moved since the last time.
const STATE_EVERY: f64 = 120.0;

/// What changed, from one scene to the next.
#[derive(Debug, Default)]
pub struct Feed {
    started: bool,
    window: Option<Window>,
    window_candidate: Option<(Window, f64)>,
    level: Option<u32>,
    level_candidate: Option<(u32, f64)>,
    map: Option<String>,
    band: Option<Band>,
    mp_gone: bool,
    dead: bool,
    dialog: Option<(String, Option<String>)>,
    last_state: f64,
    state_exp: Option<f32>,
    state_map: Option<String>,
}

impl Feed {
    pub fn new() -> Feed {
        Feed::default()
    }

    /// What is new in `scene`, in the order it happened.
    pub fn update(&mut self, scene: &Scene) -> Vec<Event> {
        let mut events = Vec::new();
        if !self.started {
            // The first look: what is there is the state, not news.
            self.started = true;
            self.window = Some(scene.window);
            self.level = scene.level;
            self.map = scene.map.clone();
            self.dead = scene.dead;
            self.band = scene.hp_now().map(band_of);
            self.mp_gone = scene.mp_now().is_some_and(|m| m <= MP_GONE);
            self.dialog = scene
                .dialog
                .as_ref()
                .map(|d| (d.kind.clone(), d.text.clone()));
            self.last_state = scene.at;
            self.state_exp = scene.exp.as_ref().map(|b| b.percent);
            self.state_map = scene.map.clone();
            return events;
        }
        self.window_news(scene, &mut events);
        if scene.window == Window::InFront {
            if scene.dead && !self.dead {
                events.push(Event::new("death", "The character died.".into()));
            }
            self.level_news(scene, &mut events);
            if let Some(map) = scene.map.as_ref().filter(|m| !m.trim().is_empty())
                && self.map.as_ref() != Some(map)
            {
                // A map first read is the state; a different one is news.
                if let Some(was) = &self.map {
                    events.push(Event::new("map", format!("New map: {map} (was {was}).")));
                }
                self.map = Some(map.clone());
            }
            if !scene.dead {
                self.hp_news(scene, &mut events);
                self.mp_news(scene, &mut events);
            }
            self.dialog_news(scene, &mut events);
            self.dead = scene.dead;
            if events.is_empty() && scene.at - self.last_state >= STATE_EVERY {
                let exp = scene.exp.as_ref().map(|b| b.percent);
                let moved = exp != self.state_exp || scene.map != self.state_map;
                if moved {
                    events.push(Event::new("state", "The state of things.".into()));
                }
            }
        }
        if !events.is_empty() {
            self.last_state = scene.at;
            self.state_exp = scene.exp.as_ref().map(|b| b.percent);
            self.state_map = scene.map.clone();
        }
        events
    }

    /// A taught thing spoke up ("the boss under 20%", "a portal showed").
    pub fn thing(&mut self, name: &str, says: &str) -> Event {
        Event::new("thing", format!("{name}: {says}"))
    }

    fn window_news(&mut self, scene: &Scene, events: &mut Vec<Event>) {
        if self.window == Some(scene.window) {
            self.window_candidate = None;
            return;
        }
        match self.window_candidate {
            Some((w, since)) if w == scene.window => {
                if scene.at - since >= WINDOW_HOLDS {
                    let was_open =
                        matches!(self.window, Some(Window::Behind | Window::Unavailable));
                    let headline = match scene.window {
                        Window::InFront if was_open => {
                            "MapleStory is back in front: reading the screen again."
                        }
                        Window::InFront => {
                            "MapleStory is open, the window in front: reading the screen."
                        }
                        Window::Behind => {
                            "MapleStory went behind another window: the screen isn't read until it's back in front."
                        }
                        Window::NotOpen => "MapleStory was closed.",
                        Window::Unavailable => "MapleStory can't be captured right now.",
                    };
                    events.push(Event::new("window", headline.to_string()));
                    self.window = Some(scene.window);
                    self.window_candidate = None;
                }
            }
            _ => self.window_candidate = Some((scene.window, scene.at)),
        }
    }

    fn level_news(&mut self, scene: &Scene, events: &mut Vec<Event>) {
        let Some(level) = scene.level else {
            self.level_candidate = None;
            return;
        };
        match self.level {
            None => self.level = Some(level),
            Some(known) if level > known => match self.level_candidate {
                Some((l, since)) if l == level => {
                    if scene.at - since >= LEVEL_HOLDS {
                        events.push(Event::new(
                            "level_up",
                            format!("Level up: {known} → {level}."),
                        ));
                        self.level = Some(level);
                        self.level_candidate = None;
                    }
                }
                _ => self.level_candidate = Some((level, scene.at)),
            },
            // The same, or lower (a misread): nothing.
            Some(_) => self.level_candidate = None,
        }
    }

    fn hp_news(&mut self, scene: &Scene, events: &mut Vec<Event>) {
        let Some(hp) = scene.hp_now() else {
            return;
        };
        let was = self.band.unwrap_or(Band::Fine);
        let now = match was {
            Band::Fine if hp <= HP_CRITICAL => Band::Critical,
            Band::Fine if hp <= HP_LOW => Band::Low,
            Band::Low if hp <= HP_CRITICAL => Band::Critical,
            Band::Low if hp >= HP_FINE_AGAIN => Band::Fine,
            Band::Critical if hp >= HP_FINE_AGAIN => Band::Fine,
            Band::Critical if hp >= HP_OUT_OF_CRITICAL => Band::Low,
            same => same,
        };
        self.band = Some(now);
        let headline = match (was, now) {
            (Band::Fine, Band::Low) => format!("HP low: {hp:.0}%."),
            (_, Band::Critical) if was != Band::Critical => {
                format!("HP critical: {hp:.0}%!")
            }
            (Band::Low | Band::Critical, Band::Fine) => format!("HP back up: {hp:.0}%."),
            _ => return,
        };
        events.push(Event::new("hp", headline));
    }

    fn mp_news(&mut self, scene: &Scene, events: &mut Vec<Event>) {
        let Some(mp) = scene.mp_now() else {
            return;
        };
        if !self.mp_gone && mp <= MP_GONE {
            self.mp_gone = true;
            events.push(Event::new("mp", format!("MP almost gone: {mp:.0}%.")));
        } else if self.mp_gone && mp >= MP_BACK {
            self.mp_gone = false;
        }
    }

    fn dialog_news(&mut self, scene: &Scene, events: &mut Vec<Event>) {
        let now = scene
            .dialog
            .as_ref()
            .map(|d| (d.kind.clone(), d.text.clone()));
        match (&self.dialog, &now) {
            (None, Some((kind, text))) => events.push(Event::new(
                "dialog",
                match text {
                    Some(text) => format!("A {kind} opened: \"{text}\"."),
                    None => format!("A {kind} opened."),
                },
            )),
            // Its text read after it opened: that is the news.
            (Some((_, None)), Some((kind, Some(text)))) => events.push(Event::new(
                "dialog",
                format!("The {kind} reads: \"{text}\"."),
            )),
            _ => {}
        }
        self.dialog = now;
    }
}

fn band_of(hp: f32) -> Band {
    if hp <= HP_CRITICAL {
        Band::Critical
    } else if hp <= HP_LOW {
        Band::Low
    } else {
        Band::Fine
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bar(percent: f32, age: f64) -> Bar {
        Bar {
            percent,
            current: None,
            max: None,
            read: true,
            age,
        }
    }

    fn scene(at: f64) -> Scene {
        let mut s = Scene::empty(at, "12:00:00", Window::InFront);
        s.level = Some(16);
        s.hp = Some(bar(90.0, 0.0));
        s.mp = Some(bar(80.0, 0.0));
        s.exp = Some(bar(50.0, 0.0));
        s.map = Some("Henesys".into());
        s
    }

    fn kinds(events: &[Event]) -> Vec<&'static str> {
        events.iter().map(|e| e.kind).collect()
    }

    #[test]
    fn the_first_look_is_the_state_not_news() {
        let mut feed = Feed::new();
        let mut first = scene(0.0);
        first.hp = Some(bar(10.0, 0.0));
        first.dead = true;
        assert!(feed.update(&first).is_empty());
    }

    #[test]
    fn a_death_a_level_up_and_a_new_map_are_news_once() {
        let mut feed = Feed::new();
        assert!(feed.update(&scene(0.0)).is_empty());
        let mut dead = scene(1.0);
        dead.dead = true;
        assert_eq!(kinds(&feed.update(&dead)), ["death"]);
        assert!(feed.update(&dead).is_empty());
        // A level must hold before it is news.
        let mut up = scene(2.0);
        up.level = Some(17);
        assert!(feed.update(&up).is_empty());
        up.at = 4.5;
        let events = feed.update(&up);
        assert_eq!(kinds(&events), ["level_up"]);
        assert_eq!(events[0].headline, "Level up: 16 → 17.");
        up.at = 6.0;
        assert!(feed.update(&up).is_empty());
        // A misread lower level is nothing.
        let mut misread = scene(7.0);
        misread.level = Some(11);
        assert!(feed.update(&misread).is_empty());
        let mut moved = scene(8.0);
        moved.level = Some(17);
        moved.map = Some("Ellinia".into());
        let events = feed.update(&moved);
        assert_eq!(kinds(&events), ["map"]);
        assert_eq!(events[0].headline, "New map: Ellinia (was Henesys).");
        assert!(feed.update(&moved).is_empty());
    }

    #[test]
    fn a_misread_level_for_a_moment_is_no_level_up() {
        let mut feed = Feed::new();
        feed.update(&scene(0.0));
        let mut blip = scene(1.0);
        blip.level = Some(61);
        assert!(feed.update(&blip).is_empty());
        // Back to 16 before it held.
        assert!(feed.update(&scene(1.5)).is_empty());
        blip.at = 5.0;
        assert!(feed.update(&blip).is_empty(), "a new candidate starts over");
    }

    #[test]
    fn hp_danger_is_news_going_in_and_coming_out_not_while_it_hovers() {
        let mut feed = Feed::new();
        feed.update(&scene(0.0));
        let at = |t: f64, hp: f32| {
            let mut s = scene(t);
            s.hp = Some(bar(hp, 0.0));
            s
        };
        assert_eq!(kinds(&feed.update(&at(1.0, 28.0))), ["hp"]);
        // Hovering around the mark: nothing.
        for (i, hp) in [31.0, 29.0, 33.0, 27.0, 40.0].into_iter().enumerate() {
            assert!(feed.update(&at(2.0 + i as f64, hp)).is_empty(), "{hp}");
        }
        let critical = feed.update(&at(8.0, 12.0));
        assert_eq!(critical[0].headline, "HP critical: 12%!");
        assert!(feed.update(&at(9.0, 18.0)).is_empty());
        assert!(feed.update(&at(10.0, 14.0)).is_empty());
        let back = feed.update(&at(11.0, 60.0));
        assert_eq!(back[0].headline, "HP back up: 60%.");
        // A reading too old to be the HP now is no news.
        let mut stale = scene(12.0);
        stale.hp = Some(bar(5.0, 30.0));
        assert!(feed.update(&stale).is_empty());
    }

    #[test]
    fn mp_is_news_only_when_all_but_gone() {
        let mut feed = Feed::new();
        feed.update(&scene(0.0));
        let at = |t: f64, mp: f32| {
            let mut s = scene(t);
            s.mp = Some(bar(mp, 0.0));
            s
        };
        assert!(feed.update(&at(1.0, 20.0)).is_empty());
        assert_eq!(kinds(&feed.update(&at(2.0, 8.0))), ["mp"]);
        assert!(feed.update(&at(3.0, 5.0)).is_empty());
        assert!(feed.update(&at(4.0, 30.0)).is_empty());
        assert_eq!(kinds(&feed.update(&at(5.0, 9.0))), ["mp"]);
    }

    #[test]
    fn a_window_change_that_holds_is_news_a_glance_is_not() {
        let mut feed = Feed::new();
        feed.update(&scene(0.0));
        let behind = |t: f64| {
            let mut s = scene(t);
            s.window = Window::Behind;
            s
        };
        assert!(feed.update(&behind(1.0)).is_empty());
        assert!(feed.update(&scene(2.0)).is_empty(), "back before it held");
        assert!(feed.update(&behind(3.0)).is_empty());
        let events = feed.update(&behind(7.5));
        assert_eq!(kinds(&events), ["window"]);
        // While behind, the bars are not news.
        let mut low = behind(8.0);
        low.hp = Some(bar(5.0, 0.0));
        assert!(feed.update(&low).is_empty());
        assert!(feed.update(&scene(9.0)).is_empty());
        assert_eq!(
            feed.update(&scene(13.5))[0].headline,
            "MapleStory is back in front: reading the screen again."
        );
        // A game opened after MapleSyrup started was never "back".
        let mut feed = Feed::new();
        feed.update(&Scene::empty(0.0, "12:00:00", Window::NotOpen));
        assert!(feed.update(&scene(1.0)).is_empty());
        assert_eq!(
            feed.update(&scene(5.5))[0].headline,
            "MapleStory is open, the window in front: reading the screen."
        );
    }

    #[test]
    fn a_dialog_opening_and_its_text_are_news() {
        let mut feed = Feed::new();
        feed.update(&scene(0.0));
        let mut open = scene(1.0);
        open.dialog = Some(Dialog {
            kind: "dialog".into(),
            text: None,
            x: 0.5,
            y: 0.5,
        });
        assert_eq!(feed.update(&open)[0].headline, "A dialog opened.");
        assert!(feed.update(&open).is_empty());
        open.dialog.as_mut().unwrap().text = Some("Bring me 10 snail shells.".into());
        assert_eq!(
            feed.update(&open)[0].headline,
            "The dialog reads: \"Bring me 10 snail shells.\"."
        );
        assert!(feed.update(&scene(3.0)).is_empty(), "closing is not news");
    }

    #[test]
    fn the_state_goes_out_now_and_then_when_something_moved() {
        let mut feed = Feed::new();
        feed.update(&scene(0.0));
        assert!(feed.update(&scene(60.0)).is_empty());
        // Two minutes on, nothing moved: still nothing.
        assert!(feed.update(&scene(130.0)).is_empty());
        let mut gained = scene(131.0);
        gained.exp = Some(bar(51.5, 0.0));
        assert_eq!(kinds(&feed.update(&gained)), ["state"]);
        gained.at = 140.0;
        assert!(feed.update(&gained).is_empty());
    }

    #[test]
    fn the_text_says_every_reading_with_its_age() {
        let mut s = Scene::empty(100.0, "17:42:05", Window::InFront);
        s.classic = true;
        s.name = Some("WANWANBUJIO".into());
        s.job = Some("Magician".into());
        s.level = Some(17);
        s.hp = Some(Bar {
            percent: 62.0,
            current: Some(416),
            max: Some(671),
            read: true,
            age: 0.4,
        });
        s.mp = Some(Bar {
            percent: 27.2,
            current: Some(189),
            max: Some(695),
            read: true,
            age: 40.0,
        });
        s.exp = Some(bar(19.32, 0.0));
        s.map = Some("Henesys Market".into());
        s.map_age = Some(40.0);
        s.moving_total = Some(5);
        s.moving = vec![Blob {
            id: 3,
            x: 0.31,
            y: 0.62,
            w: 0.05,
            h: 0.08,
        }];
        s.combat = Some("light".into());
        s.buffs = Some(3);
        s.taught = vec![Taught {
            name: "Slime".into(),
            now: "2 on screen (left, middle)".into(),
            places: vec![(0.2, 0.6), (0.5, 0.6)],
        }];
        s.session = Session {
            minutes: 47.0,
            exp_per_hour: Some(12.5),
            next_level_minutes: Some(380.0),
            deaths: 1,
            minutes_since_death: Some(12.0),
            level_ups: 0,
            lowest_hp_lately: None,
        };
        let text = s.text();
        assert!(
            text.contains("Now 17:42:05: MapleStory (Classic World) is open"),
            "{text}"
        );
        assert!(
            text.contains("Character: WANWANBUJIO, Magician, level 17."),
            "{text}"
        );
        assert!(text.contains("HP 416/671 (62%), read just now"), "{text}");
        assert!(
            text.contains("MP 189/695 (27%), last read 40 s ago (may have changed)"),
            "{text}"
        );
        assert!(text.contains("EXP 19.32%"), "{text}");
        assert!(
            text.contains("Map: Henesys Market (read 40 s ago)."),
            "{text}"
        );
        assert!(
            text.contains("5 things moving on the screen, the biggest at (31%, 62%)"),
            "{text}"
        );
        assert!(text.contains("action: light"), "{text}");
        assert!(
            text.contains("Small icons in a row at the top right: 3"),
            "{text}"
        );
        assert!(text.contains("Slime: 2 on screen (left, middle)"), "{text}");
        assert!(text.contains("Session: 47 min; EXP +12.5% per hour; next level in about 6 h 20 min; died once (12 min ago)"), "{text}");
        // Nothing read at all, in front: it says so.
        let blank = Scene::empty(1.0, "12:00:00", Window::InFront).text();
        assert!(
            blank.contains("HP, MP and EXP can't be read right now."),
            "{blank}"
        );
        let closed = Scene::empty(1.0, "12:00:00", Window::NotOpen).text();
        assert!(closed.contains("no MapleStory window is open"), "{closed}");
        assert!(!closed.contains("can't be read"), "{closed}");
    }
}
