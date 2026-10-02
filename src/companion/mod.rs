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

pub mod chat;
pub mod commands;
pub mod exp;
pub mod observation;

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
    /// Consecutive frames below the HP / MP threshold (one frame can be a
    /// bar half-covered by a dialog).
    hp_low_frames: u32,
    mp_low_frames: u32,
    zero_hp_frames: u32,
    dead: bool,
    listening_until: f64,
    muted: bool,
    exp: ExpTracker,
    last_level: Option<u32>,
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
}

/// How long a new level reading must hold before it is believed, in seconds.
const LEVEL_HOLD_SECS: f64 = 3.0;

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
            zero_hp_frames: 0,
            dead: false,
            listening_until: f64::NEG_INFINITY,
            muted: false,
            exp: ExpTracker::new(),
            last_level: None,
            level_candidate: None,
            announced_level_up: f64::NEG_INFINITY,
            marks: 0,
            now: 0.0,
            spoken: std::collections::VecDeque::new(),
            turn: 0,
        }
    }

    /// Note a line that was spoken aloud (by the PC or the phone).
    pub fn remember_spoken(&mut self, now: f64, text: &str) {
        self.spoken.push_back((now, commands::normalize(text)));
        while self.spoken.front().is_some_and(|(t, _)| now - t > 45.0) {
            self.spoken.pop_front();
        }
    }

    /// Whether `heard` is MapleSyrup's own voice coming back through the
    /// phone's microphone rather than the player.
    pub fn is_echo(&self, now: f64, heard: &str) -> bool {
        let heard = commands::normalize(heard);
        let words: Vec<&str> = heard.split(' ').filter(|w| !w.is_empty()).collect();
        if words.is_empty() {
            return false;
        }
        self.spoken.iter().any(|(t, said)| {
            let age = now - t;
            if !(-1.0..=45.0).contains(&age) {
                return false;
            }
            let said: std::collections::HashSet<&str> = said.split(' ').collect();
            let shared = words.iter().filter(|w| said.contains(*w)).count() as f64;
            let overlap = shared / words.len() as f64;
            (words.len() >= 3 && overlap >= 0.6)
                || (words.len() >= 2 && age < 15.0 && overlap >= 0.8)
        })
    }

    pub fn set_always_listen(&mut self, on: bool) {
        self.settings.always_listen = on;
    }

    pub fn muted(&self) -> bool {
        self.muted
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
        self.last = Some(obs);
        out
    }

    fn track_window(&mut self, now: f64, obs: &Observation, out: &mut Vec<Action>) {
        if obs.game.is_seen() {
            if !self.ever_seen {
                self.ever_seen = true;
                out.push(Action::Say(Say::info("I can see MapleStory.", true)));
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
        // A death: HP at zero for a moment.
        if hp.percent <= 0.5 {
            self.zero_hp_frames += 1;
            if self.zero_hp_frames >= 3 && !self.dead {
                self.dead = true;
                out.push(Action::Say(Say::alert(
                    "Your HP hit zero. Time to revive and head back.",
                )));
            }
            return;
        }
        self.zero_hp_frames = 0;
        if self.dead && hp.percent > 10.0 {
            self.dead = false;
            self.hp_warning = Warning::Armed;
        }
        if hp.percent < self.settings.hp_low {
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
        if self.hp_low_frames >= 2 && due {
            self.hp_warning = Warning::Warned(now);
            out.push(Action::Say(Say::alert(format!(
                "HP low, {}. Drink a potion.",
                percent_words(hp)
            ))));
        }
    }

    fn watch_mp(&mut self, now: f64, obs: &Observation, out: &mut Vec<Action>) {
        let Some(mp) = obs.mp else {
            return;
        };
        if mp.percent < self.settings.mp_low {
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
        if self.mp_low_frames >= 2 && due && !self.dead {
            self.mp_warning = Warning::Warned(now);
            out.push(Action::Say(Say::alert(format!(
                "MP low, {}.",
                percent_words(mp)
            ))));
        }
    }

    fn watch_progress(&mut self, now: f64, obs: &Observation, out: &mut Vec<Action>) {
        // A level reading is taken once it has held for a moment: a misread
        // lasts until the next read of the plate, a real level for good.
        let mut level_rose = false;
        if let Some(level) = obs.level {
            match self.level_candidate {
                Some((candidate, since)) if candidate == level => {
                    if now - since >= LEVEL_HOLD_SECS && self.last_level != Some(level) {
                        level_rose = self.last_level.is_some_and(|before| level == before + 1);
                        self.last_level = Some(level);
                    }
                }
                _ => self.level_candidate = Some((level, now)),
            }
        }
        let mut leveled = level_rose && self.exp.level_rose(now);
        if let Some(exp) = obs.exp {
            leveled |= self.exp.add(now, exp.percent as f64);
        }
        // Both signals can arrive for one level-up, seconds apart.
        let announce = (level_rose || leveled) && now - self.announced_level_up >= 30.0;
        if announce {
            self.announced_level_up = now;
            let text = match self.last_level {
                Some(level) if level_rose => format!("Level up! You're level {level}."),
                _ => "Level up! Nice.".to_string(),
            };
            out.push(Action::Say(Say::alert(text)));
        } else if level_rose && let Some(level) = self.last_level {
            // Already celebrated from the EXP bar; now the number is known.
            out.push(Action::Say(Say::info(format!("Now level {level}."), false)));
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
        if self.is_echo(now, sentence) {
            return out;
        }
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
        c.observe(0.0, frame(90.0, 90.0, 10.0));
        // One low frame is not enough (a dialog over the bar).
        assert!(said(&c.observe(1.0, frame(20.0, 90.0, 10.0))).is_empty());
        assert_eq!(
            said(&c.observe(1.1, frame(20.0, 90.0, 10.0))),
            ["HP low, about 20 percent. Drink a potion."]
        );
        for i in 0..20 {
            assert!(said(&c.observe(1.2 + i as f64, frame(18.0, 90.0, 10.0))).is_empty());
        }
        // Recovered, then low again: warned again.
        c.observe(30.0, frame(80.0, 90.0, 10.0));
        c.observe(31.0, frame(20.0, 90.0, 10.0));
        assert_eq!(said(&c.observe(31.1, frame(20.0, 90.0, 10.0))).len(), 1);
    }

    #[test]
    fn staying_low_is_repeated_only_after_a_long_while() {
        let mut c = Companion::new(Settings::default());
        c.observe(0.0, frame(20.0, 90.0, 10.0));
        assert_eq!(said(&c.observe(0.1, frame(20.0, 90.0, 10.0))).len(), 1);
        assert!(said(&c.observe(30.0, frame(20.0, 90.0, 10.0))).is_empty());
        assert_eq!(said(&c.observe(61.0, frame(20.0, 90.0, 10.0))).len(), 1);
    }

    #[test]
    fn low_mp_is_its_own_warning() {
        let mut c = Companion::new(Settings::default());
        c.observe(0.0, frame(90.0, 10.0, 10.0));
        assert_eq!(
            said(&c.observe(0.1, frame(90.0, 10.0, 10.0))),
            ["MP low, about 10 percent."]
        );
    }

    #[test]
    fn a_death_is_said_once() {
        let mut c = Companion::new(Settings::default());
        c.observe(0.0, frame(50.0, 50.0, 10.0));
        let mut lines = Vec::new();
        for i in 0..10 {
            lines.extend(said(
                &c.observe(1.0 + i as f64 * 0.1, frame(0.0, 50.0, 10.0)),
            ));
        }
        assert_eq!(lines, ["Your HP hit zero. Time to revive and head back."]);
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
        assert_eq!(lines, ["Level up! Nice.", "Now level 58."]);
        // The level itself was taken once it held.
        assert_eq!(said(&c.command(20.0, Command::Level)), ["You're level 58."]);
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
