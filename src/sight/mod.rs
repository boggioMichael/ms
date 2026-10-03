//! MapleSyrup's learned sight: what it worked out about the player's own
//! screen, kept between runs, and used on every frame.
//!
//! ```text
//!   vision model (the teacher)            the PC, every frame (the student)
//!   ──────────────────────────            ─────────────────────────────────
//!   "the HP bar is here, the level   ──▶  measures the bars where they are,
//!    is there, it says Lv. 61,             keeps the level (and adds one when
//!    HP 4200/5000"                         the EXP bar wraps), runs the things
//!                                          the player taught
//!   checks every few minutes:        ◀──  "HP 4200/5000, the bar agrees"
//!   "HP 4200/5000" (84%) — agrees, or
//!   the bar's end is corrected; looks
//!   again from scratch if it is lost
//! ```
//!
//! The numbers beside the bars are read on every frame in the game's own
//! font ([`numbers`]) once it is learned, and cross-checked against the
//! bars; when the two keep disagreeing, the HUD is read again.
//!
//! The player teaches it too, by talking: corrections ("I'm level 61"),
//! and new things to recognise ([`things`]).

pub mod numbers;
pub mod teacher;
pub mod things;

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use image::RgbaImage;
use serde::{Deserialize, Serialize};

use crate::ai::images::NBox;
use crate::companion::{Gauge, Observation};
use numbers::{Field, Numbers, Value};
use syrup::bars::BarModel;
use teacher::{Calibration, HudValues};
use things::{Fired, Things};

/// Where the HUD is on the player's screen.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Layout {
    /// The frame size it was found on.
    pub frame: (u32, u32),
    pub hp: Option<BarModel>,
    pub mp: Option<BarModel>,
    pub exp: Option<BarModel>,
    pub level: Option<NBox>,
    pub minimap: Option<NBox>,
    /// All of the above together, with a margin: what is read to check.
    pub status: Option<NBox>,
    pub found: String,
}

impl Layout {
    /// Whether a frame of this size is laid out the same (the same shape:
    /// a resized window keeps fractions; a new shape moves the HUD).
    pub fn fits(&self, width: u32, height: u32) -> bool {
        let (w, h) = self.frame;
        if w == 0 || h == 0 || width == 0 || height == 0 {
            return false;
        }
        let a = w as f32 / h as f32;
        let b = width as f32 / height as f32;
        (a - b).abs() / a < 0.02
    }
}

/// What MapleSyrup knows about the character, and where from.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Facts {
    pub level: Option<u32>,
    /// "read" (the vision model), "player" (told), "level-up" (the EXP bar
    /// wrapped; to be read again).
    pub level_from: Option<String>,
    pub name: Option<String>,
    pub job: Option<String>,
    pub map: Option<String>,
    pub hp_max: Option<u64>,
    pub mp_max: Option<u64>,
    /// The EXP percent as last read exactly.
    pub exp_read: Option<f32>,
    /// What the player said is wrong, kept until the next reading agrees.
    #[serde(default)]
    pub corrections: Vec<(String, String)>,
}

/// What the learned sight saw in one frame.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Seen {
    /// Percentages: the number read this frame when there was one, else
    /// the bar's fill.
    pub hp: Option<f32>,
    pub mp: Option<f32>,
    pub exp: Option<f32>,
    /// The numbers read this frame, in the game's own font.
    pub hp_number: Option<(u64, u64)>,
    pub mp_number: Option<(u64, u64)>,
    pub exp_number: Option<Value>,
    pub fired: Vec<Fired>,
    /// The EXP bar just wrapped (a level-up).
    pub leveled: bool,
}

/// Why the teacher should look again soon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Want {
    /// Nothing known yet, or the screen changed shape: find the HUD.
    Calibrate,
    /// Read the HUD again (a level-up, a correction, bars lost).
    Verify,
}

pub struct Sight {
    dir: PathBuf,
    pub layout: Option<Layout>,
    pub facts: Facts,
    pub things: Things,
    /// The HUD's font, and the numbers it reads every frame.
    pub numbers: Numbers,
    want: Option<Want>,
    /// Recent EXP measurements, to see the bar wrap at a level-up.
    exp_trail: VecDeque<(Instant, f32)>,
    /// Since when the bars could not be found though the game is seen.
    lost_since: Option<Instant>,
    /// Readings in a row that disagreed with the bars by a lot.
    disagreements: u32,
    pub last: Seen,
    pub last_look: Option<Instant>,
}

/// How long the bars may be missing before the HUD is looked for again.
const LOST_FOR: Duration = Duration::from_secs(20);

fn now_text() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M").to_string()
}

impl Sight {
    /// What was learned before, from `dir` (the settings folder's
    /// `learned`).
    pub fn load(dir: &Path) -> Sight {
        let read = |name: &str| std::fs::read_to_string(dir.join(name)).ok();
        Sight {
            dir: dir.to_path_buf(),
            layout: read("layout.json").and_then(|t| serde_json::from_str(&t).ok()),
            facts: read("facts.json")
                .and_then(|t| serde_json::from_str(&t).ok())
                .unwrap_or_default(),
            things: Things::load(dir),
            numbers: Numbers::load(dir),
            want: None,
            exp_trail: VecDeque::new(),
            lost_since: None,
            disagreements: 0,
            last: Seen::default(),
            last_look: None,
        }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn save(&self) {
        let _ = std::fs::create_dir_all(&self.dir);
        if let Some(layout) = &self.layout
            && let Ok(t) = serde_json::to_string_pretty(layout)
        {
            let _ = std::fs::write(self.dir.join("layout.json"), t);
        }
        if let Ok(t) = serde_json::to_string_pretty(&self.facts) {
            let _ = std::fs::write(self.dir.join("facts.json"), t);
        }
    }

    /// What the teacher should do next for a frame this size, if anything.
    pub fn wants(&self, width: u32, height: u32, every: Duration) -> Option<Want> {
        match &self.layout {
            None => return Some(Want::Calibrate),
            Some(l) if !l.fits(width, height) => return Some(Want::Calibrate),
            _ => {}
        }
        if let Some(want) = self.want {
            return Some(want);
        }
        match self.last_look {
            None => Some(Want::Verify),
            Some(t) if t.elapsed() >= every => Some(Want::Verify),
            _ => None,
        }
    }

    /// One frame: the bars where they were learned, the numbers beside
    /// them, the EXP bar's wrap, and the things the player taught.
    pub fn observe(&mut self, frame: &RgbaImage, now: Instant) -> Seen {
        let mut seen = Seen::default();
        let bars = tracing::trace_span!("sight.bars").entered();
        let mut bands: [Option<NBox>; 3] = [None; 3];
        if let Some(layout) = self
            .layout
            .as_ref()
            .filter(|l| l.fits(frame.width(), frame.height()))
        {
            seen.hp = layout.hp.as_ref().and_then(|b| b.measure(frame));
            seen.mp = layout.mp.as_ref().and_then(|b| b.measure(frame));
            seen.exp = layout.exp.as_ref().and_then(|b| b.measure(frame));
            bands = [
                layout.hp.as_ref().map(|b| b.band),
                layout.mp.as_ref().map(|b| b.band),
                layout.exp.as_ref().map(|b| b.band),
            ];
            let expected = bands.iter().filter(|b| b.is_some()).count();
            let measured = [seen.hp, seen.mp, seen.exp]
                .iter()
                .filter(|v| v.is_some())
                .count();
            if expected > 0 && measured == 0 {
                let since = *self.lost_since.get_or_insert(now);
                if now.duration_since(since) >= LOST_FOR && self.want.is_none() {
                    self.want = Some(Want::Verify);
                }
            } else {
                self.lost_since = None;
            }
        }
        drop(bars);
        // The numbers in the game's own font, where the font is known,
        // checked against the bars; the number wins when it is read.
        let numbers_span = tracing::trace_span!("sight.numbers").entered();
        let mut disagree = 0;
        for (field, band) in Field::ALL.into_iter().zip(bands) {
            let Some(band) = band else { continue };
            let bar = match field {
                Field::Hp => seen.hp,
                Field::Mp => seen.mp,
                Field::Exp => seen.exp,
            };
            let read = self.numbers.read(frame, field, &band);
            let percent = read.as_ref().map(|r| r.value.percent());
            disagree = disagree.max(self.numbers.cross_check(field, percent, bar));
            let Some(read) = read else { continue };
            match (field, &read.value) {
                (Field::Hp, Value::Amount { current, max }) => {
                    seen.hp_number = Some((*current, *max));
                    seen.hp = percent;
                    if self.facts.hp_max != Some(*max) {
                        self.facts.hp_max = Some(*max);
                        self.save();
                    }
                }
                (Field::Mp, Value::Amount { current, max }) => {
                    seen.mp_number = Some((*current, *max));
                    seen.mp = percent;
                    if self.facts.mp_max != Some(*max) {
                        self.facts.mp_max = Some(*max);
                        self.save();
                    }
                }
                (Field::Exp, value) => {
                    seen.exp_number = Some(value.clone());
                    seen.exp = percent;
                }
                _ => {}
            }
        }
        if disagree >= numbers::DISAGREE_FOR && self.want.is_none() {
            self.want = Some(Want::Verify);
        }
        drop(numbers_span);
        // A level-up: the EXP bar goes from nearly full to nearly empty, and
        // stays there.
        if let Some(exp) = seen.exp {
            self.exp_trail.push_back((now, exp));
            while self
                .exp_trail
                .front()
                .is_some_and(|(t, _)| now.duration_since(*t) > Duration::from_secs(20))
            {
                self.exp_trail.pop_front();
            }
            let n = self.exp_trail.len();
            if n >= 3 {
                let recent_low = self.exp_trail.iter().skip(n - 2).all(|(_, e)| *e < 25.0);
                let before_high = self.exp_trail.iter().take(n - 2).any(|(_, e)| *e > 70.0);
                if recent_low && before_high {
                    seen.leveled = true;
                    self.exp_trail.clear();
                    if let Some(level) = self.facts.level {
                        self.facts.level = Some(level + 1);
                        self.facts.level_from = Some("level-up".into());
                        self.save();
                    }
                    self.want = Some(Want::Verify);
                }
            }
        }
        seen.fired = tracing::trace_span!("sight.things").in_scope(|| self.things.run(frame, now));
        self.last = seen.clone();
        seen
    }

    /// Put what the learned sight knows into `obs` (in place of the old
    /// HUD reader's guesses). A gauge is `read` only when its number was
    /// read on this very frame; a bar's fill is an estimate.
    pub fn apply(&self, obs: &mut Observation, seen: &Seen) {
        let gauge = |percent: Option<f32>, number: Option<(u64, u64)>, max: Option<u64>| {
            percent.map(|p| Gauge {
                percent: p,
                current: number.map(|(c, _)| c),
                max: number.map(|(_, m)| m).or(max),
                read: number.is_some(),
            })
        };
        obs.hp = gauge(seen.hp, seen.hp_number, self.facts.hp_max);
        obs.mp = gauge(seen.mp, seen.mp_number, self.facts.mp_max);
        let exp_amount = match &seen.exp_number {
            Some(Value::Amount { current, max }) => Some((*current, *max)),
            _ => None,
        };
        obs.exp = seen.exp.map(|p| Gauge {
            percent: p,
            current: exp_amount.map(|(c, _)| c),
            max: exp_amount.map(|(_, m)| m),
            read: seen.exp_number.is_some(),
        });
        obs.level = self.facts.level;
        obs.name = self.facts.name.clone();
        obs.job = self.facts.job.clone();
    }

    fn take_values(&mut self, v: &HudValues) {
        if let Some(level) = v.level {
            self.facts.level = Some(level);
            self.facts.level_from = Some("read".into());
            self.facts.corrections.retain(|(what, _)| what != "level");
        }
        if v.name.is_some() {
            self.facts.name = v.name.clone();
        }
        if v.job.is_some() {
            self.facts.job = v.job.clone();
        }
        if v.map.is_some() {
            self.facts.map = v.map.clone();
        }
        if let Some((_, max)) = v.hp {
            self.facts.hp_max = Some(max);
        }
        if let Some((_, max)) = v.mp {
            self.facts.mp_max = Some(max);
        }
        if v.exp_percent.is_some() {
            self.facts.exp_read = v.exp_percent;
        }
    }

    /// The teacher found the HUD on `frame`. Returns what was learned, for
    /// the log, or why nothing could be.
    pub fn calibrated(&mut self, frame: &RgbaImage, c: &Calibration) -> Result<String, String> {
        let learn = |b: &Option<NBox>, hue: f32| {
            b.as_ref()
                .and_then(|b| BarModel::learn(frame, b, Some(hue)))
        };
        let mut hp = learn(&c.hp, 0.0);
        let mut mp = learn(&c.mp, 215.0);
        let mut exp = learn(&c.exp, 55.0);
        // The game's numbers fix where each track ends.
        if let (Some(b), Some(p)) = (hp.as_mut(), c.values.hp_percent()) {
            b.reading(frame, p);
        }
        if let (Some(b), Some(p)) = (mp.as_mut(), c.values.mp_percent()) {
            b.reading(frame, p);
        }
        if let (Some(b), Some(p)) = (exp.as_mut(), c.values.exp_percent) {
            b.reading(frame, p);
        }
        let found = [hp.is_some(), mp.is_some(), exp.is_some()]
            .iter()
            .filter(|f| **f)
            .count();
        if found == 0 && c.level.is_none() {
            let why = if c.values.notes.is_empty() {
                "the HUD could not be found".to_string()
            } else {
                format!("the HUD could not be found ({})", c.values.notes)
            };
            return Err(why);
        }
        let mut status: Option<NBox> = None;
        for b in [
            hp.as_ref().map(|b| b.band),
            mp.as_ref().map(|b| b.band),
            exp.as_ref().map(|b| b.band),
            c.level,
        ]
        .into_iter()
        .flatten()
        {
            status = Some(match status {
                Some(s) => s.union(&b),
                None => b,
            });
        }
        let status = status.map(|s| s.grown(0.04, 0.6));
        let mut parts = vec![format!(
            "found {} bar(s){}",
            found,
            if c.level.is_some() {
                " and the level"
            } else {
                ""
            }
        )];
        let values = c.values.summary();
        parts.push(format!("read {values}"));
        self.layout = Some(Layout {
            frame: frame.dimensions(),
            hp,
            mp,
            exp,
            level: c.level,
            minimap: c.minimap,
            status,
            found: now_text(),
        });
        parts.extend(self.learn_font(frame, &c.values));
        self.take_values(&c.values);
        self.want = None;
        self.disagreements = 0;
        self.lost_since = None;
        self.last_look = Some(Instant::now());
        self.save();
        Ok(parts.join("; "))
    }

    /// The HUD's font, from the lines the teacher read character for
    /// character, where the font still has them to learn and the line
    /// agrees with the bar. Returns what was learned, for the log.
    fn learn_font(&mut self, frame: &RgbaImage, v: &HudValues) -> Vec<String> {
        let mut notes = Vec::new();
        let Some(layout) = &self.layout else {
            return notes;
        };
        let now = Instant::now();
        let lines = [
            (Field::Hp, &v.hp_text, layout.hp.as_ref()),
            (Field::Mp, &v.mp_text, layout.mp.as_ref()),
            (Field::Exp, &v.exp_text, layout.exp.as_ref()),
        ];
        let mut todo = Vec::new();
        for (field, text, bar) in lines {
            let (Some(text), Some(bar)) = (text, bar) else {
                continue;
            };
            if !self.numbers.wants_sample(field, now) {
                continue;
            }
            todo.push((field, text.clone(), bar.band, bar.measure(frame)));
        }
        for (field, text, band, measured) in todo {
            let learned = Numbers::believable(field, &text, measured)
                .and_then(|_| self.numbers.learn(frame, field, &band, &text, "model", now));
            match learned {
                Ok(line) => notes.push(line),
                Err(why) => notes.push(format!("{} line not learned: {why}", field.label())),
            }
        }
        notes
    }

    /// The teacher read the HUD again. Corrects the bars' ends and the
    /// facts; asks for a new search when the bars and the numbers keep
    /// disagreeing. Returns what changed, for the log.
    pub fn verified(&mut self, frame: &RgbaImage, v: &HudValues) -> String {
        self.last_look = Some(Instant::now());
        self.want = None;
        let mut notes = Vec::new();
        let mut off = 0;
        if let Some(layout) = self.layout.as_mut() {
            for (name, bar, percent) in [
                ("HP", layout.hp.as_mut(), v.hp_percent()),
                ("MP", layout.mp.as_mut(), v.mp_percent()),
                ("EXP", layout.exp.as_mut(), v.exp_percent),
            ] {
                let (Some(bar), Some(percent)) = (bar, percent) else {
                    continue;
                };
                let measured = bar.measure(frame);
                let agrees = match measured {
                    Some(m) if (m - percent).abs() > 12.0 => {
                        off += 1;
                        notes.push(format!("{name} bar said {m:.0}%, the game {percent:.0}%"));
                        false
                    }
                    None if percent > 10.0 => {
                        off += 1;
                        notes.push(format!("{name} bar not found, the game says {percent:.0}%"));
                        false
                    }
                    _ => true,
                };
                // A reading fixes a new bar's end; a well-learned bar that
                // suddenly disagrees is more likely misread (or moved) than
                // wrong, and is not bent to it.
                if agrees || bar.readings.len() < 3 {
                    bar.reading(frame, percent);
                }
            }
        }
        if let (Some(old), Some(new)) = (self.facts.level, v.level)
            && old != new
        {
            notes.push(format!("level {old} → {new}"));
        }
        notes.extend(self.learn_font(frame, v));
        self.take_values(v);
        if off > 0 {
            self.disagreements += 1;
            if self.disagreements >= 2 {
                self.want = Some(Want::Calibrate);
                notes.push("the HUD will be looked for again".into());
            }
        } else {
            self.disagreements = 0;
        }
        self.save();
        let summary = v.summary();
        if notes.is_empty() {
            format!("read {summary}")
        } else {
            format!("read {summary}; {}", notes.join("; "))
        }
    }

    /// The teacher could not be asked, or did not answer usefully: try
    /// again later rather than at once.
    pub fn looked(&mut self) {
        self.last_look = Some(Instant::now());
    }

    /// The player says `what` is `value`. Returns what was done, for the
    /// model to say.
    pub fn correct(&mut self, what: &str, value: &str) -> Result<String, String> {
        let what = what.trim().to_ascii_lowercase();
        let value = value.trim();
        let digits = || {
            value
                .chars()
                .filter(|c| c.is_ascii_digit() || *c == '.')
                .collect::<String>()
        };
        let reply = match what.as_str() {
            "level" => {
                let level: u32 = digits().parse().map_err(|_| "a level is a number")?;
                if !(1..=300).contains(&level) {
                    return Err("levels go from 1 to 300".into());
                }
                self.facts.level = Some(level);
                self.facts.level_from = Some("player".into());
                format!("level set to {level}")
            }
            "name" => {
                self.facts.name = Some(value.to_string());
                format!("name set to {value}")
            }
            "job" | "class" => {
                self.facts.job = Some(value.to_string());
                format!("job set to {value}")
            }
            "map" => {
                self.facts.map = Some(value.to_string());
                format!("map set to {value}")
            }
            "hp" | "mp" | "exp" => {
                // The bars are measured: a disagreement means looking again.
                self.want = Some(Want::Verify);
                format!("{what} noted; the HUD is being read again to fix the {what} bar")
            }
            other => return Err(format!("{other} can't be corrected")),
        };
        self.facts.corrections.retain(|(w, _)| *w != what);
        self.facts
            .corrections
            .push((what.clone(), value.to_string()));
        if self.facts.corrections.len() > 10 {
            self.facts.corrections.remove(0);
        }
        self.save();
        Ok(reply)
    }

    /// Look again soon (asked by the player or the conversation).
    pub fn look_again(&mut self) {
        if self.want.is_none() {
            self.want = Some(Want::Verify);
        }
    }

    /// Everything the learned sight knows, as lines for the conversation.
    pub fn describe(&self) -> Vec<String> {
        let mut lines = Vec::new();
        let f = &self.facts;
        if let Some(level) = f.level {
            let from = match f.level_from.as_deref() {
                Some("player") => " (the player told you)",
                Some("level-up") => " (you saw the EXP bar wrap; not yet read again)",
                _ => "",
            };
            lines.push(format!("Level {level}{from}."));
        }
        let mut who = Vec::new();
        if let Some(j) = &f.job {
            who.push(format!("job {j}"));
        }
        if let Some(n) = &f.name {
            who.push(format!("named {n}"));
        }
        if let Some(m) = &f.map {
            who.push(format!("last seen on the map {m}"));
        }
        if !who.is_empty() {
            lines.push(format!("Character: {}.", who.join(", ")));
        }
        if let (Some(hp), Some(mp)) = (f.hp_max, f.mp_max) {
            lines.push(format!("Max HP {hp}, max MP {mp} (when last read)."));
        }
        if self.layout.is_none() {
            lines.push(
                "You haven't found the HUD on this screen yet; the bars aren't measured.".into(),
            );
        } else {
            lines.push(format!("{}.", self.numbers.describe()));
        }
        let things = self.things.describe();
        if !things.is_empty() {
            lines.push(format!(
                "Things the player taught you to recognise: {}.",
                things.join("; ")
            ));
        }
        lines
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::companion::GameView;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ms-sight-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    /// A modern status bar: dark background, a red HP bar and a blue MP bar
    /// with white text over them and a gradient on each, and a thin
    /// yellow-green EXP bar along the bottom (here 50% full).
    fn status_bar(hp: f32, mp: f32) -> RgbaImage {
        status_bar_exp(hp, mp, 50.0)
    }

    fn status_bar_exp(hp: f32, mp: f32, exp: f32) -> RgbaImage {
        use image::Rgba;
        let mut f = RgbaImage::from_pixel(1280, 720, Rgba([60, 90, 140, 255]));
        for y in 640..700 {
            for x in 400..880 {
                f.put_pixel(x, y, Rgba([28, 28, 34, 255]));
            }
        }
        let bar = |f: &mut RgbaImage, y0: u32, fill: f32, rgb: [u8; 3]| {
            let (x0, x1) = (520u32, 860u32);
            let end = x0 + ((x1 - x0) as f32 * fill / 100.0) as u32;
            for y in y0..y0 + 10 {
                let shade = 1.0 - (y - y0) as f32 * 0.04;
                for x in x0..x1 {
                    let p = if x < end {
                        Rgba([
                            (rgb[0] as f32 * shade) as u8,
                            (rgb[1] as f32 * shade) as u8,
                            (rgb[2] as f32 * shade) as u8,
                            255,
                        ])
                    } else {
                        Rgba([45, 45, 50, 255])
                    };
                    f.put_pixel(x, y, p);
                }
            }
            for y in y0 + 2..y0 + 8 {
                for x in (660..720).step_by(3) {
                    f.put_pixel(x, y, Rgba([250, 250, 250, 255]));
                }
            }
        };
        bar(&mut f, 652, hp, [230, 40, 50]);
        bar(&mut f, 670, mp, [40, 110, 235]);
        let end = (1280.0 * exp / 100.0) as u32;
        for y in 708..714 {
            for x in 0..1280 {
                let p = if x < end {
                    Rgba([200, 220, 40, 255])
                } else {
                    Rgba([30, 30, 30, 255])
                };
                f.put_pixel(x, y, p);
            }
        }
        f
    }

    fn calibration() -> Calibration {
        // Where the made-up status bar's HP and MP are (`status_bar`).
        Calibration {
            level: Some(NBox::new(0.32, 0.9, 0.38, 0.95)),
            hp: Some(NBox::new(
                512.0 / 1280.0,
                648.0 / 720.0,
                850.0 / 1280.0,
                664.0 / 720.0,
            )),
            mp: Some(NBox::new(
                512.0 / 1280.0,
                667.0 / 720.0,
                870.0 / 1280.0,
                684.0 / 720.0,
            )),
            exp: None,
            minimap: None,
            values: HudValues {
                level: Some(61),
                hp: Some((3000, 5000)),
                mp: Some((2000, 2000)),
                exp_percent: Some(86.25),
                job: Some("Assassin".into()),
                ..Default::default()
            },
        }
    }

    #[test]
    fn the_hud_is_learned_measured_kept_and_checked() {
        let dir = temp_dir("hud");
        let mut sight = Sight::load(&dir);
        assert_eq!(
            sight.wants(1280, 720, Duration::from_secs(120)),
            Some(Want::Calibrate)
        );
        let frame = status_bar(60.0, 100.0);
        let line = sight.calibrated(&frame, &calibration()).unwrap();
        assert!(line.contains("found 2 bar(s) and the level"), "{line}");
        assert_eq!(sight.wants(1280, 720, Duration::from_secs(120)), None);
        // Measured on other frames, and put into the observation.
        let seen = sight.observe(&status_bar(25.0, 50.0), Instant::now());
        assert!((seen.hp.unwrap() - 25.0).abs() < 1.5, "{seen:?}");
        assert!((seen.mp.unwrap() - 50.0).abs() < 1.5, "{seen:?}");
        let mut obs = Observation::unseen(GameView::Seen("MapleStory".into()));
        sight.apply(&mut obs, &seen);
        assert_eq!(obs.level, Some(61));
        assert_eq!(obs.job.as_deref(), Some("Assassin"));
        assert_eq!(obs.hp.unwrap().max, Some(5000));
        // Kept between runs.
        let again = Sight::load(&dir);
        assert_eq!(again.facts.level, Some(61));
        assert!(again.layout.is_some());
        // Another window shape: look for the HUD again.
        assert_eq!(
            again.wants(1920, 800, Duration::from_secs(120)),
            Some(Want::Calibrate)
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_exp_bar_wrapping_is_a_level_up_and_corrections_stick() {
        let dir = temp_dir("level");
        let mut sight = Sight::load(&dir);
        let frame = status_bar_exp(60.0, 100.0, 86.25);
        let mut c = calibration();
        c.exp = Some(NBox::new(0.0, 706.0 / 720.0, 1.0, 716.0 / 720.0));
        sight.calibrated(&frame, &c).unwrap();
        let exp_seen = sight.observe(&frame, Instant::now()).exp.unwrap();
        assert!((exp_seen - 86.25).abs() < 1.0, "{exp_seen}");
        let t0 = Instant::now();
        for (i, exp) in [90.0, 95.0, 98.0, 3.0, 4.0].into_iter().enumerate() {
            let seen = sight.observe(
                &status_bar_exp(60.0, 100.0, exp),
                t0 + Duration::from_secs(i as u64),
            );
            assert_eq!(seen.leveled, i == 4, "{i}: {seen:?}");
        }
        assert_eq!(sight.facts.level, Some(62));
        assert_eq!(
            sight.wants(1280, 720, Duration::from_secs(120)),
            Some(Want::Verify)
        );
        // The player corrects it; a later reading wins again.
        assert_eq!(
            sight.correct("level", "level 61").unwrap(),
            "level set to 61"
        );
        assert!(sight.correct("level", "lots").is_err());
        assert!(sight.describe()[0].contains("the player told you"));
        let line = sight.verified(
            &frame,
            &HudValues {
                level: Some(62),
                ..Default::default()
            },
        );
        assert!(line.contains("level 61 → 62"), "{line}");
        assert_eq!(sight.facts.level_from.as_deref(), Some("read"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_numbers_are_read_every_frame_once_the_teacher_spelled_them_out() {
        let dir = temp_dir("numbers");
        let mut sight = Sight::load(&dir);
        let (frame, bands) = numbers::tests::hud((400, 400), (1291, 1351), 37.51);
        let c = Calibration {
            level: None,
            hp: Some(bands[0].grown(0.05, 0.4)),
            mp: Some(bands[1].grown(0.05, 0.4)),
            exp: Some(bands[2].grown(0.05, 0.4)),
            minimap: None,
            values: HudValues {
                hp: Some((400, 400)),
                mp: Some((1291, 1351)),
                exp_percent: Some(37.51),
                hp_text: Some("HP [400/400]".into()),
                mp_text: Some("MP [1291/1351]".into()),
                exp_text: Some("EXP [37.51%]".into()),
                ..Default::default()
            },
        };
        let line = sight.calibrated(&frame, &c).unwrap();
        assert!(line.contains("found 3 bar(s)"), "{line}");
        assert!(line.contains("learned 11 glyphs"), "{line}");
        assert!(sight.numbers.knows(Field::Exp));
        // Another frame: the numbers come from the font, not the bars.
        let (other, _) = numbers::tests::hud((315, 400), (1000, 1351), 40.01);
        let seen = sight.observe(&other, Instant::now());
        assert_eq!(seen.hp_number, Some((315, 400)), "{seen:?}");
        assert_eq!(seen.mp_number, Some((1000, 1351)));
        assert_eq!(seen.exp_number, Some(Value::Percent(40.01)));
        assert!((seen.hp.unwrap() - 78.75).abs() < 0.01);
        let mut obs = Observation::unseen(GameView::Seen("MapleStory".into()));
        sight.apply(&mut obs, &seen);
        let hp = obs.hp.unwrap();
        assert!(
            hp.read && hp.current == Some(315) && hp.max == Some(400),
            "{hp:?}"
        );
        assert!(obs.exp.unwrap().read);
        assert_eq!(sight.facts.hp_max, Some(400));
        // A digit the font never saw: the bar's estimate, not a reading.
        let (unknown, _) = numbers::tests::hud((88, 400), (1000, 1351), 40.01);
        let seen = sight.observe(&unknown, Instant::now());
        assert_eq!(seen.hp_number, None);
        assert!((seen.hp.unwrap() - 22.0).abs() < 3.0, "{:?}", seen.hp);
        let mut obs = Observation::unseen(GameView::Seen("MapleStory".into()));
        sight.apply(&mut obs, &seen);
        assert!(!obs.hp.unwrap().read);
        assert!(
            sight
                .describe()
                .iter()
                .any(|l| l.contains("the HUD's font"))
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn numbers_and_bars_disagreeing_for_a_while_send_it_looking_again() {
        let dir = temp_dir("disagree-numbers");
        let mut sight = Sight::load(&dir);
        let (frame, bands) = numbers::tests::hud((400, 400), (1291, 1351), 37.51);
        let c = Calibration {
            level: None,
            hp: Some(bands[0].grown(0.05, 0.4)),
            mp: None,
            exp: None,
            minimap: None,
            values: HudValues {
                hp: Some((400, 400)),
                hp_text: Some("HP [400/400]".into()),
                ..Default::default()
            },
        };
        sight.calibrated(&frame, &c).unwrap();
        // The font reads 400/400 while the bar is drawn 10% full: the
        // number wins, and after a while the HUD is read again.
        let (odd, _) = {
            let (mut f, b) = numbers::tests::hud((40, 400), (1291, 1351), 37.51);
            // Paint the HP line as if it said 400/400.
            let (full, _) = numbers::tests::hud((400, 400), (1291, 1351), 37.51);
            let (x, y, w, _) = b[0].pixels(1280, 720);
            for yy in y - 16..y {
                for xx in x..x + w {
                    f.put_pixel(xx, yy, *full.get_pixel(xx, yy));
                }
            }
            (f, b)
        };
        let t0 = Instant::now();
        for i in 0..numbers::DISAGREE_FOR {
            let seen = sight.observe(&odd, t0 + Duration::from_millis(100 * i as u64));
            assert_eq!(seen.hp_number, Some((400, 400)), "{i}: {seen:?}");
            assert_eq!(seen.hp, Some(100.0));
        }
        assert_eq!(
            sight.wants(1280, 720, Duration::from_secs(120)),
            Some(Want::Verify)
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn bars_that_disagree_twice_send_it_looking_again() {
        let dir = temp_dir("disagree");
        let mut sight = Sight::load(&dir);
        let frame = status_bar(60.0, 100.0);
        sight.calibrated(&frame, &calibration()).unwrap();
        let wrong = HudValues {
            hp: Some((1000, 5000)),
            ..Default::default()
        };
        sight.verified(&frame, &wrong);
        assert_eq!(sight.wants(1280, 720, Duration::from_secs(120)), None);
        let line = sight.verified(&frame, &wrong);
        assert!(line.contains("looked for again"), "{line}");
        assert_eq!(
            sight.wants(1280, 720, Duration::from_secs(120)),
            Some(Want::Calibrate)
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
