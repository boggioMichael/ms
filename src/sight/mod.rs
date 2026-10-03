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
//! Deterministic first: the HUD is found from the pixels alone — the red,
//! blue and yellow-green bars in the status band, each learned as a bar
//! model with its track fitted from its own fill — and the font from the
//! OCR engine. The vision model is asked only when that fails: no bars to
//! be found for a while, numbers and bars that keep disagreeing, a font
//! that cannot be labelled because the text is not sharp enough for OCR,
//! or a level-up to confirm; each with a backoff, so a model that cannot
//! answer is not asked again and again.
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
    /// Nothing known yet, or the screen changed shape, and the pixels alone
    /// did not give the HUD: find it.
    Calibrate,
    /// Read the HUD again (a level-up, a correction, bars lost, numbers and
    /// bars disagreeing, a font that cannot be labelled otherwise).
    Verify,
}

/// How long the pixels alone get to find the HUD before the model is asked.
const FIND_FOR: Duration = Duration::from_secs(10);
/// How often the pixels are asked to find the HUD while they cannot.
const FIND_EVERY: Duration = Duration::from_secs(1);
/// How long a field may go without an example the OCR engine could give
/// before the model is asked to spell its line out.
const UNLABELLED_FOR: Duration = Duration::from_secs(15);
/// The first wait between two asks of the model for the same reason; it
/// doubles each time, up to [`ASK_BACKOFF_MAX`].
const ASK_BACKOFF: Duration = Duration::from_secs(60);
const ASK_BACKOFF_MAX: Duration = Duration::from_secs(900);
/// How many readings in a row the EXP bar must have shown nearly full for
/// its emptying to count as a level-up: half a second at the companion's
/// frame rate, which a misreading does not last.
const FULL_FOR_FRAMES: usize = 5;
/// The three bars' colours: a hue range to search (it wraps: 325 to 15 is
/// pink through red), the least saturation and brightness of the fill, and
/// the hue each model is expected to have. Narrow enough to keep lava
/// (20–40°) out of the HP and EXP searches.
const BAR_COLOURS: [((f32, f32), f32, f32, f32); 3] = [
    ((325.0, 15.0), 0.35, 0.30, 350.0),
    ((180.0, 235.0), 0.30, 0.30, 205.0),
    ((48.0, 80.0), 0.25, 0.25, 62.0),
];
/// A learned bar whose hue is farther than this from the colour expected
/// of it was learned from the scenery, and is dropped when the layout is
/// loaded (a layout from before the bars were checked this way).
const BAR_HUE_OFF: f32 = 30.0;

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
    /// The EXP percent as last read from the numbers, and when.
    exp_read_at: Option<(Instant, f32)>,
    /// Since when the bars could not be found though the game is seen.
    lost_since: Option<Instant>,
    /// Readings in a row that disagreed with the bars by a lot.
    disagreements: u32,
    pub last: Seen,
    pub last_look: Option<Instant>,
    /// The pixels alone looking for the HUD: when they last tried, and
    /// since when they have failed.
    find_tried: Option<Instant>,
    find_failing_since: Option<Instant>,
    /// Since when a field has wanted an example the OCR engine did not give.
    unlabelled_since: Option<Instant>,
    /// The model was last asked at, and the wait before asking again.
    asked: Option<Instant>,
    ask_backoff: Duration,
    /// The model was last asked to spell the lines out and could not help,
    /// at; and the wait before asking for that reason again, which doubles
    /// each time. Other reasons to ask are not held back by it.
    labels_asked: Option<Instant>,
    label_backoff: Duration,
    /// When a bar's track was last refitted from the number beside it.
    refit_at: Option<Instant>,
    /// The bars as measured on the last frame, to see one jump.
    last_bars: [Option<f32>; 3],
    /// Pictures of the status strip saved for a look at a misread bar:
    /// how many so far, and when the last was.
    debug_saved: u32,
    debug_at: Option<Instant>,
}

/// A bar jumping by this much between two frames is a misread (or a
/// level-up, which the EXP bar does once a session at most): the status
/// strip is saved under `learned/debug/` to be looked at, a few times a
/// session and not more often than once a minute.
const JUMP: f32 = 45.0;
const DEBUG_PICTURES: u32 = 6;
const DEBUG_EVERY: Duration = Duration::from_secs(60);

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
        let layout: Option<Layout> = read("layout.json")
            .and_then(|t| serde_json::from_str(&t).ok())
            .and_then(|mut layout: Layout| {
                // Bars of the wrong colour were learned from the scenery
                // (lava for HP): dropped, so they are looked for again.
                let off = |bar: &Option<BarModel>, expected: f32| {
                    bar.as_ref()
                        .is_some_and(|b| syrup::bars::hue_distance(b.hue, expected) > BAR_HUE_OFF)
                };
                if off(&layout.hp, BAR_COLOURS[0].3) {
                    layout.hp = None;
                }
                if off(&layout.mp, BAR_COLOURS[1].3) {
                    layout.mp = None;
                }
                if off(&layout.exp, BAR_COLOURS[2].3) {
                    layout.exp = None;
                }
                (layout.hp.is_some() || layout.mp.is_some() || layout.exp.is_some())
                    .then_some(layout)
            });
        Sight {
            dir: dir.to_path_buf(),
            layout,
            facts: read("facts.json")
                .and_then(|t| serde_json::from_str(&t).ok())
                .unwrap_or_default(),
            things: Things::load(dir),
            numbers: Numbers::load(dir),
            want: None,
            exp_trail: VecDeque::new(),
            exp_read_at: None,
            lost_since: None,
            disagreements: 0,
            last: Seen::default(),
            last_look: None,
            find_tried: None,
            find_failing_since: None,
            unlabelled_since: None,
            asked: None,
            labels_asked: None,
            label_backoff: ASK_BACKOFF,
            ask_backoff: ASK_BACKOFF,
            refit_at: None,
            last_bars: [None; 3],
            debug_saved: 0,
            debug_at: None,
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

    /// What the model should be asked for a frame this size, if anything:
    /// to find the HUD when the pixels alone have failed to for a while, or
    /// to read it again for one of the reasons in [`Want`]. Never while a
    /// backoff from the last ask is running.
    pub fn wants(&self, width: u32, height: u32) -> Option<Want> {
        let now = Instant::now();
        if self
            .asked
            .is_some_and(|t| now.duration_since(t) < self.ask_backoff)
        {
            return None;
        }
        let fits = self.layout.as_ref().is_some_and(|l| l.fits(width, height));
        if !fits {
            // The pixels get their turn first.
            return self
                .find_failing_since
                .is_some_and(|t| now.duration_since(t) >= FIND_FOR)
                .then_some(Want::Calibrate);
        }
        if let Some(want) = self.want {
            return Some(want);
        }
        let labels_held_back = self
            .labels_asked
            .is_some_and(|t| now.duration_since(t) < self.label_backoff);
        if !labels_held_back
            && self
                .unlabelled_since
                .is_some_and(|t| now.duration_since(t) >= UNLABELLED_FOR)
        {
            return Some(Want::Verify);
        }
        None
    }

    /// The model is being asked now: the next ask for the same reason
    /// waits longer. An answer that helps ([`Sight::calibrated`],
    /// [`Sight::verified`]) resets the wait.
    pub fn asking(&mut self) {
        self.asked = Some(Instant::now());
    }

    /// The model answered. When its answer `helped` — every line that
    /// wanted an example got one that reads — the next ask waits the usual
    /// time; when it did not (the same label again, learned from nothing),
    /// the next ask waits twice as long as the last, so a line the model
    /// cannot spell out does not cost a call every half minute.
    fn answered(&mut self, helped: bool) {
        self.asked = None;
        self.ask_backoff = ASK_BACKOFF;
        if helped {
            self.unlabelled_since = None;
            self.labels_asked = None;
            self.label_backoff = ASK_BACKOFF;
        } else {
            self.labels_asked = Some(Instant::now());
            self.label_backoff = (self.label_backoff * 2).min(ASK_BACKOFF_MAX);
        }
    }

    /// Does a line the layout has still want the model to spell it out?
    fn still_unlabelled(&self) -> bool {
        let Some(layout) = &self.layout else {
            return false;
        };
        [
            (Field::Hp, layout.hp.is_some()),
            (Field::Mp, layout.mp.is_some()),
            (Field::Exp, layout.exp.is_some()),
        ]
        .into_iter()
        .any(|(field, has_bar)| has_bar && self.numbers.wants_label(field))
    }

    /// The model could not be asked, or did not answer usefully: try
    /// again later rather than at once, and later still each time.
    pub fn looked(&mut self) {
        self.last_look = Some(Instant::now());
        self.asked = Some(Instant::now());
        self.ask_backoff = (self.ask_backoff * 2).min(ASK_BACKOFF_MAX);
    }

    /// Find the HUD from the pixels alone: the red, blue and yellow-green
    /// bars in the status band (the bottom of the screen, left of the
    /// buttons), each learned as a bar model with its track fitted from its
    /// own fill. The level, name and job stay unknown until the model, if
    /// there is one, reads them. Returns what was found, for the log.
    pub fn find_hud(&mut self, frame: &RgbaImage) -> Result<String, String> {
        use syrup::bars::find_bar;
        use syrup::geometry::Rect;
        let (fw, fh) = frame.dimensions();
        if fw < 64 || fh < 64 {
            return Err("the frame is too small to hold a HUD".into());
        }
        let band = Rect {
            x: 0,
            y: fh.saturating_mul(9) / 10,
            w: fw.saturating_mul(3) / 4,
            h: fh - fh.saturating_mul(9) / 10,
        };
        let mut models: [Option<BarModel>; 3] = [None, None, None];
        for (i, (hue, sat, val, expected)) in BAR_COLOURS.into_iter().enumerate() {
            // A band of rows of the colour, not its biggest blob: on a
            // lava map the biggest blob is the lava.
            let Some(fill) = find_bar(frame, band, hue, sat, val) else {
                continue;
            };
            // Where the fill is; where the track ends is not known from
            // the pixels (what lies past the fill may be the scene behind
            // a translucent panel) — the first reading of the number
            // beside it, or the teacher's, says. Until then the bar is
            // not reported (`observe`).
            let approx = NBox::from_pixels(fill.x, fill.y, fill.w, fill.h, fw, fh);
            let Some(model) = BarModel::learn(frame, &approx, Some(expected)) else {
                continue;
            };
            models[i] = Some(model);
        }
        let found = models.iter().filter(|m| m.is_some()).count();
        if found == 0 {
            return Err("no HP, MP or EXP bar in the status band".into());
        }
        let [hp, mp, exp] = models;
        let mut status: Option<NBox> = None;
        for b in [&hp, &mp, &exp].into_iter().flatten() {
            status = Some(match status {
                Some(s) => s.union(&b.band),
                None => b.band,
            });
        }
        let kept_level = self.layout.as_ref().and_then(|l| l.level);
        let kept_minimap = self.layout.as_ref().and_then(|l| l.minimap);
        self.layout = Some(Layout {
            frame: frame.dimensions(),
            hp,
            mp,
            exp,
            level: kept_level,
            minimap: kept_minimap,
            status: status.map(|s| s.grown(0.04, 1.2)),
            found: now_text(),
        });
        self.want = None;
        self.disagreements = 0;
        self.lost_since = None;
        self.find_failing_since = None;
        self.save();
        Ok(format!("found {found} bar(s) from the pixels"))
    }

    /// One frame: the bars where they were learned, the numbers beside
    /// them, the EXP bar's wrap, and the things the player taught.
    /// Does the sight see the HUD on a screen this shape: a layout that
    /// fits it, whose bars it measured the last time it looked? Then its
    /// bars and numbers answer for the HUD and the geometry detector can
    /// rest ([`crate::perceive`]).
    pub fn sees_hud(&self, width: u32, height: u32) -> bool {
        self.lost_since.is_none()
            && self
                .layout
                .as_ref()
                .is_some_and(|l| l.fits(width, height) && (l.hp.is_some() || l.mp.is_some()))
    }

    pub fn observe(&mut self, frame: &RgbaImage, now: Instant) -> Seen {
        let mut seen = Seen::default();
        // No HUD known for a screen this shape: the pixels look for it,
        // once a second, and the clock runs on how long they fail.
        let fits = self
            .layout
            .as_ref()
            .is_some_and(|l| l.fits(frame.width(), frame.height()));
        if !fits
            && self
                .find_tried
                .is_none_or(|t| now.duration_since(t) >= FIND_EVERY)
        {
            self.find_tried = Some(now);
            let _find = tracing::trace_span!("sight.find").entered();
            if self.find_hud(frame).is_err() {
                self.find_failing_since.get_or_insert(now);
            }
        }
        let bars = tracing::trace_span!("sight.bars").entered();
        let mut bands: [Option<NBox>; 3] = [None; 3];
        // A picture to save for a look at a bar that jumped, if one did.
        let mut debug: Option<(Option<NBox>, String)> = None;
        if let Some(layout) = self
            .layout
            .as_ref()
            .filter(|l| l.fits(frame.width(), frame.height()))
        {
            // A bar found from the pixels knows where its fill ends, not
            // where its track does, until a reading says: it measures
            // "full" whatever the fill, and is not reported until then
            // (a 100% that falls to 19% at the first reading would be a
            // level-up to the companion).
            let raw = [&layout.hp, &layout.mp, &layout.exp]
                .map(|b| b.as_ref().and_then(|b| b.measure(frame)));
            let fitted = |b: &Option<BarModel>, m: Option<f32>| {
                m.filter(|_| b.as_ref().is_some_and(|b| !b.readings.is_empty()))
            };
            seen.hp = fitted(&layout.hp, raw[0]);
            seen.mp = fitted(&layout.mp, raw[1]);
            seen.exp = fitted(&layout.exp, raw[2]);
            // A bar that jumped since the last frame: a picture for a look.
            let now_bars = [seen.hp, seen.mp, seen.exp];
            let jumped = now_bars
                .iter()
                .zip(self.last_bars)
                .zip(["hp", "mp", "exp"])
                .find(|((a, b), _)| matches!((a, b), (Some(a), Some(b)) if (a - b).abs() >= JUMP))
                .map(|(_, name)| name);
            self.last_bars = now_bars;
            debug = jumped.map(|name| (layout.status, format!("{name}-jump")));
            bands = [
                layout.hp.as_ref().map(|b| b.band),
                layout.mp.as_ref().map(|b| b.band),
                layout.exp.as_ref().map(|b| b.band),
            ];
            // (A bar seen but not yet fitted counts as seen.)
            let expected = bands.iter().filter(|b| b.is_some()).count();
            let measured = raw.iter().filter(|v| v.is_some()).count();
            if expected > 0 && measured == 0 {
                let since = *self.lost_since.get_or_insert(now);
                if now.duration_since(since) >= LOST_FOR && self.want.is_none() {
                    self.want = Some(Want::Verify);
                }
            } else {
                self.lost_since = None;
            }
        }
        if let Some((status, what)) = debug {
            self.save_debug(frame, status, &what, now);
        }
        drop(bars);
        // The numbers in the game's own font, where the font is known,
        // checked against the bars; the number wins when it is read.
        let numbers_span = tracing::trace_span!("sight.numbers").entered();
        let mut disagree = 0;
        let mut refit: Vec<(Field, f32)> = Vec::new();
        for (field, band) in Field::ALL.into_iter().zip(bands) {
            let Some(band) = band else { continue };
            let bar = match field {
                Field::Hp => seen.hp,
                Field::Mp => seen.mp,
                Field::Exp => seen.exp,
            };
            let read = self.numbers.read(frame, field, &band);
            let percent = read.as_ref().map(|r| r.value.percent());
            let streak = self.numbers.cross_check(field, percent, bar);
            disagree = disagree.max(streak);
            // The game's own number says how far the fill should reach: a
            // bar whose track end was guessed from its fill is refitted to
            // it, the way a reading by the teacher refits it. Only a new
            // bar (under three readings): a well-fitted one that suddenly
            // disagrees is more likely misread than wrong, and is not bent.
            // A bar not fitted at all takes the number at once.
            let unfitted = bar.is_none()
                && self
                    .layout
                    .as_ref()
                    .and_then(|l| match field {
                        Field::Hp => l.hp.as_ref(),
                        Field::Mp => l.mp.as_ref(),
                        Field::Exp => l.exp.as_ref(),
                    })
                    .is_some_and(|b| b.readings.is_empty());
            if (streak > 0 || unfitted)
                && let Some(p) = percent
                && (unfitted
                    || self
                        .refit_at
                        .is_none_or(|t| now.duration_since(t) >= Duration::from_secs(1)))
            {
                refit.push((field, p));
            }
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
        if !refit.is_empty() {
            self.refit_at = Some(now);
            if let Some(layout) = self.layout.as_mut() {
                for (field, percent) in refit {
                    let bar = match field {
                        Field::Hp => layout.hp.as_mut(),
                        Field::Mp => layout.mp.as_mut(),
                        Field::Exp => layout.exp.as_mut(),
                    };
                    if let Some(bar) = bar
                        && bar.readings.len() < 3
                    {
                        bar.reading(frame, percent);
                    }
                }
            }
            self.save();
        }
        if disagree >= numbers::DISAGREE_FOR && self.want.is_none() {
            self.want = Some(Want::Verify);
        }
        // A field that has wanted an example for a while, which the OCR
        // engine did not give (not sharp enough, or no engine): the model
        // will be asked to spell the line out.
        let unlabelled = bands
            .iter()
            .zip(Field::ALL)
            .any(|(band, field)| band.is_some() && self.numbers.wants_label(field));
        if unlabelled {
            self.unlabelled_since.get_or_insert(now);
        } else {
            self.unlabelled_since = None;
        }
        drop(numbers_span);
        // A level-up: the EXP bar goes from nearly full to nearly empty, and
        // stays there. The full must have lasted — a run of readings, not
        // one frame's misreading (something of the bar's colour over it) —
        // and the number, when it was read lately, has the last word.
        if let Some(Value::Percent(p)) = &seen.exp_number {
            self.exp_read_at = Some((now, *p));
        } else if let Some(Value::Amount { current, max }) = &seen.exp_number
            && *max > 0
        {
            self.exp_read_at = Some((now, *current as f32 / *max as f32 * 100.0));
        }
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
                let before_high = {
                    let (mut run, mut longest) = (0usize, 0usize);
                    for (_, e) in self.exp_trail.iter().take(n - 2) {
                        run = if *e > 70.0 { run + 1 } else { 0 };
                        longest = longest.max(run);
                    }
                    longest >= FULL_FOR_FRAMES
                };
                let number_disagrees = self.exp_read_at.is_some_and(|(t, p)| {
                    now.duration_since(t) <= Duration::from_secs(2) && p >= 25.0
                });
                if recent_low && before_high && !number_disagrees {
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

    /// Put what the learned sight knows into `obs`, over what the HUD
    /// geometry guessed — but only what it knows: a gauge the sight could
    /// not see this frame, or a fact it has not learned, leaves the
    /// observation's own value standing rather than blanking it. A gauge
    /// is `read` only when its number was read on this very frame; a bar's
    /// fill is an estimate.
    pub fn apply(&self, obs: &mut Observation, seen: &Seen) {
        let gauge = |percent: Option<f32>, number: Option<(u64, u64)>, max: Option<u64>| {
            percent.map(|p| Gauge {
                percent: p,
                current: number.map(|(c, _)| c),
                max: number.map(|(_, m)| m).or(max),
                read: number.is_some(),
            })
        };
        if let Some(hp) = gauge(seen.hp, seen.hp_number, self.facts.hp_max) {
            obs.hp = Some(hp);
        }
        if let Some(mp) = gauge(seen.mp, seen.mp_number, self.facts.mp_max) {
            obs.mp = Some(mp);
        }
        let exp_amount = match &seen.exp_number {
            Some(Value::Amount { current, max }) => Some((*current, *max)),
            _ => None,
        };
        if let Some(p) = seen.exp {
            obs.exp = Some(Gauge {
                percent: p,
                current: exp_amount.map(|(c, _)| c),
                max: exp_amount.map(|(_, m)| m),
                read: seen.exp_number.is_some(),
            });
        }
        if self.facts.level.is_some() {
            obs.level = self.facts.level;
        }
        if self.facts.name.is_some() {
            obs.name = self.facts.name.clone();
        }
        if self.facts.job.is_some() {
            obs.job = self.facts.job.clone();
        }
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
        self.find_failing_since = None;
        self.last_look = Some(Instant::now());
        // Finding the HUD is help enough; the lines' examples come later.
        self.answered(true);
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
        let wanted_labels = self.still_unlabelled();
        let mut notes = Vec::new();
        let mut off = 0;
        let mut disagreed: Vec<String> = Vec::new();
        if let Some(layout) = self.layout.as_mut() {
            for (name, bar, percent) in [
                ("HP", layout.hp.as_mut(), v.hp_percent()),
                ("MP", layout.mp.as_mut(), v.mp_percent()),
                ("EXP", layout.exp.as_mut(), v.exp_percent),
            ] {
                let (Some(bar), Some(percent)) = (bar, percent) else {
                    continue;
                };
                // A bar found from the pixels, not yet fitted: this reading
                // fits it; there is nothing to disagree with yet.
                if bar.readings.is_empty() {
                    bar.reading(frame, percent);
                    continue;
                }
                let measured = bar.measure(frame);
                let agrees = match measured {
                    Some(m) if (m - percent).abs() > 12.0 => {
                        off += 1;
                        notes.push(format!("{name} bar said {m:.0}%, the game {percent:.0}%"));
                        disagreed.push(name.to_ascii_lowercase());
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
        if let Some(name) = disagreed.first()
            && let Some(status) = self.layout.as_ref().map(|l| l.status)
        {
            self.save_debug(frame, status, &format!("{name}-disagrees"), Instant::now());
        }
        if let (Some(old), Some(new)) = (self.facts.level, v.level)
            && old != new
        {
            notes.push(format!("level {old} → {new}"));
        }
        notes.extend(self.learn_font(frame, v));
        self.take_values(v);
        // The model saw no HUD at all (a cutscene, a dialog over it, the
        // HUD hidden): asking again at once would only ask again at once,
        // every few seconds, for as long as it stays hidden. The next look
        // waits, and longer each time.
        let saw_hud =
            v.level.is_some() || v.hp.is_some() || v.mp.is_some() || v.exp_percent.is_some();
        if !saw_hud {
            self.looked();
            notes.push(format!(
                "no HUD in view; the next look waits {} s",
                self.ask_backoff.as_secs()
            ));
        } else {
            // Asked to spell the lines out and none of them learned: the
            // next ask waits longer, and says so.
            let helped = !wanted_labels || !self.still_unlabelled();
            self.answered(helped);
            if !helped {
                notes.push(format!(
                    "no line learned from this; the lines are next asked about in {} s",
                    self.label_backoff.as_secs()
                ));
            }
        }
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

    /// Save the status strip of `frame` under `learned/debug/` as
    /// `<what>-<n>.png`, for a look at a bar that was misread: a few a
    /// session, not more often than once a minute. (`status`: the strip,
    /// as a fraction of the frame; the whole frame when None.)
    fn save_debug(&mut self, frame: &RgbaImage, status: Option<NBox>, what: &str, now: Instant) {
        if self.debug_saved >= DEBUG_PICTURES
            || self
                .debug_at
                .is_some_and(|t| now.duration_since(t) < DEBUG_EVERY)
        {
            return;
        }
        self.debug_saved += 1;
        self.debug_at = Some(now);
        let (fw, fh) = frame.dimensions();
        let (x, y, w, h) = status.map(|s| s.pixels(fw, fh)).unwrap_or((0, 0, fw, fh));
        let w = w.min(fw.saturating_sub(x)).max(1);
        let h = h.min(fh.saturating_sub(y)).max(1);
        let crop = image::imageops::crop_imm(frame, x, y, w, h).to_image();
        let dir = self.dir.join("debug");
        let _ = std::fs::create_dir_all(&dir);
        let _ = crop.save(dir.join(format!("{what}-{}.png", self.debug_saved)));
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
                "Things the player taught you to recognise, as your vision engine sees them now (for when they \
ask or it matters; not to be read out, and a count of several is often the same thing or a look-alike seen more \
than once): {}.",
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
        // Nothing known: the pixels get their turn before the model is asked.
        assert_eq!(sight.wants(1280, 720), None);
        sight.find_failing_since = Some(Instant::now() - FIND_FOR);
        assert_eq!(sight.wants(1280, 720), Some(Want::Calibrate));
        let frame = status_bar(60.0, 100.0);
        let line = sight.calibrated(&frame, &calibration()).unwrap();
        assert!(line.contains("found 2 bar(s) and the level"), "{line}");
        assert_eq!(sight.wants(1280, 720), None);
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
        // Another window shape: the pixels look for the HUD again, and the
        // model is asked only once they have failed for a while.
        let mut again = again;
        assert_eq!(again.wants(1920, 800), None);
        let blank = RgbaImage::from_pixel(1920, 800, image::Rgba([20, 20, 20, 255]));
        let t0 = Instant::now();
        again.observe(&blank, t0);
        assert!(again.find_failing_since.is_some());
        assert_eq!(again.wants(1920, 800), None);
        again.find_failing_since = Some(t0 - FIND_FOR);
        assert_eq!(again.wants(1920, 800), Some(Want::Calibrate));
        // Asked, and not again until the backoff has run.
        again.asking();
        assert_eq!(again.wants(1920, 800), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_hud_is_found_from_the_pixels_alone() {
        let dir = temp_dir("find");
        let mut sight = Sight::load(&dir);
        let (frame, _) = numbers::tests::hud((240, 400), (1351, 1351), 86.25);
        let seen = sight.observe(&frame, Instant::now());
        let layout = sight.layout.as_ref().expect("the bars were found");
        assert!(layout.hp.is_some() && layout.mp.is_some() && layout.exp.is_some());
        // Where each fill is, is known; where its track ends is not until
        // a reading says, and a bar is not reported before then.
        assert_eq!((seen.hp, seen.mp, seen.exp), (None, None, None), "{seen:?}");
        assert_eq!(
            sight.wants(1280, 720),
            None,
            "nothing for the model to do yet"
        );
        // The teacher reads the numbers: the tracks are fitted to them,
        // and from then on the bars measure right on any frame.
        let values = HudValues {
            hp: Some((240, 400)),
            mp: Some((1351, 1351)),
            exp_percent: Some(86.25),
            ..Default::default()
        };
        sight.verified(&frame, &values);
        let seen = sight.observe(&frame, Instant::now());
        assert!((seen.hp.unwrap() - 60.0).abs() < 3.0, "{seen:?}");
        assert!((seen.mp.unwrap() - 100.0).abs() < 3.0, "{seen:?}");
        assert!((seen.exp.unwrap() - 86.25).abs() < 3.0, "{seen:?}");
        let (other, _) = numbers::tests::hud((100, 400), (675, 1351), 10.0);
        let seen = sight.observe(&other, Instant::now());
        assert!((seen.hp.unwrap() - 25.0).abs() < 3.0, "{seen:?}");
        assert!((seen.mp.unwrap() - 50.0).abs() < 3.0, "{seen:?}");
        assert!((seen.exp.unwrap() - 10.0).abs() < 3.0, "{seen:?}");
        // What the geometry guessed stays when the sight has nothing better.
        let mut obs = Observation::unseen(GameView::Seen("MapleStory".into()));
        obs.level = Some(7);
        sight.apply(&mut obs, &Seen::default());
        assert_eq!(obs.level, Some(7));
        assert!(obs.hp.is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_hud_is_found_on_a_lava_map_and_a_lava_bar_is_dropped_at_load() {
        use image::Rgba;
        let dir = temp_dir("lava");
        let mut sight = Sight::load(&dir);
        // The status strip on a lava map: orange everywhere the bars are
        // not (the HUD is translucent), a red border line under a panel,
        // and the bars themselves as before.
        let (mut frame, bands) = numbers::tests::hud((240, 400), (1351, 1351), 19.0);
        for y in 648..720u32 {
            for x in 0..1280u32 {
                let inside = bands.iter().any(|b| {
                    let (bx, by, bw, bh) = b.pixels(1280, 720);
                    x >= bx && x < bx + bw && y >= by.saturating_sub(12) && y < by + bh
                });
                if !inside {
                    frame.put_pixel(x, y, Rgba([179, 106, 0, 255]));
                }
            }
        }
        for y in [660u32, 661] {
            for x in 100..600 {
                frame.put_pixel(x, y, Rgba([255, 0, 0, 255]));
            }
        }
        sight.observe(&frame, Instant::now());
        let layout = sight.layout.as_ref().expect("the bars were found");
        for (bar, expected) in [
            (&layout.hp, 350.0),
            (&layout.mp, 205.0),
            (&layout.exp, 62.0),
        ] {
            let bar = bar.as_ref().expect("each bar");
            assert!(
                syrup::bars::hue_distance(bar.hue, expected) < 25.0,
                "hue {} for {expected}",
                bar.hue
            );
        }
        // Fitted by the teacher's reading, the bars measure right, the
        // lava showing through the EXP bar's track notwithstanding.
        let values = HudValues {
            hp: Some((240, 400)),
            mp: Some((1351, 1351)),
            exp_percent: Some(19.0),
            ..Default::default()
        };
        sight.verified(&frame, &values);
        let seen = sight.observe(&frame, Instant::now());
        assert!((seen.hp.unwrap() - 60.0).abs() < 3.0, "{seen:?}");
        assert!((seen.mp.unwrap() - 100.0).abs() < 3.0, "{seen:?}");
        assert!((seen.exp.unwrap() - 19.0).abs() < 3.0, "{seen:?}");
        // A layout from before, whose HP bar was learned from the lava:
        // that bar is dropped at load, the others kept.
        let mut lava = sight.layout.clone().unwrap();
        lava.hp.as_mut().unwrap().hue = 22.0;
        std::fs::write(
            dir.join("layout.json"),
            serde_json::to_string(&lava).unwrap(),
        )
        .unwrap();
        let again = Sight::load(&dir);
        let layout = again.layout.as_ref().expect("the other bars stay");
        assert!(layout.hp.is_none() && layout.mp.is_some() && layout.exp.is_some());
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
        // Nearly full for a while (frames a tenth of a second apart), then
        // nearly empty: a level-up on the second empty frame. One frame of
        // "full" would not have been one (something of the bar's colour
        // drawn over it).
        let t0 = Instant::now();
        let steps: Vec<f32> = [90.0, 95.0, 98.0, 98.0, 98.0, 98.0, 98.0, 3.0, 4.0].into();
        let last = steps.len() - 1;
        for (i, exp) in steps.into_iter().enumerate() {
            let seen = sight.observe(
                &status_bar_exp(60.0, 100.0, exp),
                t0 + Duration::from_millis(100 * i as u64),
            );
            assert_eq!(seen.leveled, i == last, "{i}: {seen:?}");
        }
        assert_eq!(sight.facts.level, Some(62));
        // A moment of "full" in a bar that then empties is not one.
        let mut quiet = Sight::load(&temp_dir("level-quiet"));
        quiet.calibrated(&frame, &c).unwrap();
        let t1 = Instant::now();
        for (i, exp) in [20.0, 20.0, 85.0, 20.0, 19.0].into_iter().enumerate() {
            let seen = quiet.observe(
                &status_bar_exp(60.0, 100.0, exp),
                t1 + Duration::from_millis(100 * i as u64),
            );
            assert!(!seen.leveled, "{i}: {seen:?}");
        }
        assert_eq!(quiet.facts.level, Some(61));
        let _ = std::fs::remove_dir_all(temp_dir("level-quiet"));
        assert_eq!(sight.wants(1280, 720), Some(Want::Verify));
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
        assert_eq!(sight.wants(1280, 720), Some(Want::Verify));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_bar_that_jumps_leaves_a_picture_to_look_at_now_and_then() {
        let dir = temp_dir("jump");
        let mut sight = Sight::load(&dir);
        let frame = status_bar_exp(60.0, 100.0, 19.0);
        let mut c = calibration();
        c.exp = Some(NBox::new(0.0, 706.0 / 720.0, 1.0, 716.0 / 720.0));
        c.values.exp_percent = Some(19.0);
        sight.calibrated(&frame, &c).unwrap();
        let t0 = Instant::now();
        let first = sight.observe(&frame, t0);
        assert!((first.exp.unwrap() - 19.0).abs() < 2.0, "{first:?}");
        // Misread as nearly full for a frame: the strip is saved once…
        sight.observe(
            &status_bar_exp(60.0, 100.0, 99.0),
            t0 + Duration::from_millis(100),
        );
        assert!(dir.join("debug").join("exp-jump-1.png").is_file());
        // …and the fall back is not saved again within the minute.
        sight.observe(&frame, t0 + Duration::from_millis(200));
        assert!(!dir.join("debug").join("exp-jump-2.png").exists());
        assert_eq!(sight.debug_saved, 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn labels_that_teach_nothing_are_asked_for_less_and_less_often() {
        let dir = temp_dir("unhelpful");
        let mut sight = Sight::load(&dir);
        let frame = status_bar(60.0, 100.0);
        sight.calibrated(&frame, &calibration()).unwrap();
        // The lines want examples (no OCR engine here), and the model's
        // spelling fits nothing on the made-up bar.
        assert!(sight.still_unlabelled());
        let useless = HudValues {
            hp: Some((3000, 5000)),
            hp_text: Some("HP [3000/5000]".into()),
            ..Default::default()
        };
        let first = sight.verified(&frame, &useless);
        assert!(first.contains("next asked about in 120 s"), "{first}");
        assert_eq!(sight.label_backoff, Duration::from_secs(120));
        // Held back for the lines' sake only; other reasons still ask.
        assert_eq!(sight.wants(1280, 720), None);
        sight.want = Some(Want::Verify);
        assert_eq!(sight.wants(1280, 720), Some(Want::Verify));
        sight.want = None;
        let second = sight.verified(&frame, &useless);
        assert!(second.contains("next asked about in 240 s"), "{second}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_look_that_sees_no_hud_waits_longer_each_time() {
        let dir = temp_dir("hidden");
        let mut sight = Sight::load(&dir);
        let frame = status_bar(60.0, 100.0);
        sight.calibrated(&frame, &calibration()).unwrap();
        // The HUD hidden (a cutscene): the bars are lost, and a look is
        // wanted once they have been for a while.
        let blank = RgbaImage::from_pixel(1280, 720, image::Rgba([20, 20, 20, 255]));
        let t0 = Instant::now();
        sight.observe(&blank, t0);
        sight.observe(&blank, t0 + LOST_FOR);
        assert_eq!(sight.wants(1280, 720), Some(Want::Verify));
        sight.asking();
        // The model saw nothing: the next look is not at once, and each
        // such look waits twice as long.
        let first = sight.verified(&blank, &HudValues::default());
        assert!(
            first.contains("no HUD in view; the next look waits 120 s"),
            "{first}"
        );
        sight.observe(&blank, t0 + LOST_FOR + Duration::from_secs(1));
        assert_eq!(sight.wants(1280, 720), None, "held back");
        sight.asked = Some(Instant::now() - Duration::from_secs(121));
        assert_eq!(sight.wants(1280, 720), Some(Want::Verify));
        let second = sight.verified(&blank, &HudValues::default());
        assert!(second.contains("the next look waits 240 s"), "{second}");
        // The HUD back and read: the usual wait again.
        let values = HudValues {
            hp: Some((3000, 5000)),
            ..Default::default()
        };
        sight.verified(&frame, &values);
        assert_eq!(sight.ask_backoff, ASK_BACKOFF);
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
        assert_eq!(sight.wants(1280, 720), None);
        let line = sight.verified(&frame, &wrong);
        assert!(line.contains("looked for again"), "{line}");
        assert_eq!(sight.wants(1280, 720), Some(Want::Calibrate));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
