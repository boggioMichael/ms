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
    /// "read" (the vision model), "player" (told). (Older files may say
    /// "level-up": the EXP bar wrapped and the level was guessed; a guess
    /// is no longer made.)
    pub level_from: Option<String>,
    pub name: Option<String>,
    pub job: Option<String>,
    /// The map's name as last read (or told), this session only: it is
    /// where they were when it was read, and the player moves on, so it
    /// is never kept between runs and is said with its age
    /// ([`Sight::map_at`]) while it is young enough to be worth saying.
    #[serde(skip)]
    pub map: Option<String>,
    pub hp_max: Option<u64>,
    pub mp_max: Option<u64>,
    /// The EXP percent as last read exactly.
    pub exp_read: Option<f32>,
    /// What the player said is wrong, kept until the next reading agrees.
    #[serde(default)]
    pub corrections: Vec<(String, String)>,
}

/// A field as the teacher read it off the HUD, and when: the time of the
/// frame it read (the ask), not of the answer, which comes seconds later.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Told {
    pub at: Instant,
    /// 0 to 100.
    pub percent: f32,
    /// `current/max`, for HP and MP.
    pub amount: Option<(u64, u64)>,
}

/// What the learned sight saw in one frame.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Seen {
    /// Percentages: the number read this frame when there was one, else
    /// the bar's fill — none when the fill disagrees with the teacher's
    /// last read by more than [`FILL_AGREES`] points (until it reads
    /// again).
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
    /// When the map's name (`facts.map`) was read or told.
    map_at: Option<Instant>,
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
    /// The teacher's last believed reading of each field (HP, MP, EXP): on
    /// a HUD whose font is not learned, the numbers to trust.
    told: [Option<Told>; 3],
    /// Fields whose fill disagreed with their last read by more than
    /// [`FILL_AGREES`] points: no reading until the next read.
    fill_off: [bool; 3],
    /// When the teacher was asked for the answer still to come: the time
    /// of the frame it reads.
    pending_at: Option<Instant>,
    /// When the font last read each field.
    font_read_at: [Option<Instant>; 3],
    /// Since when the font has not read each field, on every frame looked
    /// at since (None while it reads, or before a frame was looked at).
    unread_since: [Option<Instant>; 3],
    /// When a frame was last looked at: the sight looks only while the
    /// game is in front.
    observed_at: Option<Instant>,
    /// A maximum HP and MP read far from the one known, and how many reads
    /// in a row have said it (see [`Sight::believe`]).
    far_max: [Option<(u64, u32)>; 2],
}

/// How far, in points, a bar's fill may be from the teacher's last read
/// before it is no reading until the next read: between two reads the fill
/// says how the number moves, never against what was read.
pub const FILL_AGREES: f32 = 15.0;
/// A line the font read this recently is being read (for the snapshot).
const FONT_FRESH: Duration = Duration::from_secs(5);
/// How long the font must have failed to read the HP or MP line before the
/// teacher's cadence starts: a cursor over it for a moment is not that,
/// and a HUD just found from the pixels gives the OCR engine its turn.
const UNREAD_FOR: Duration = Duration::from_secs(10);
/// The wait between two reads when a fill has disagreed with the last one:
/// the brief's lower end, so a real drop is confirmed (and warned) sooner.
const READ_SOONER: Duration = Duration::from_secs(20);
/// A frame looked at this recently: the game is in front.
const IN_FRONT_FOR: Duration = Duration::from_secs(3);
/// How old a read may be and still judge the fill. Reads come every
/// [`READ_EVERY`] while they are wanted; one this old means the teacher
/// cannot answer, and the fill is taken as before.
const TOLD_JUDGES: Duration = Duration::from_secs(90);
/// How old a read may be and still be said in the snapshot, with its age.
const TOLD_SAID: Duration = Duration::from_secs(600);
/// A maximum HP or MP outside this share of the one known is a misread —
/// the classic HUD's thin slash read as a 7 made `594/671` "3947/6771" —
/// unless the level changed with it, or it holds for [`FAR_MAX_HOLDS`]
/// reads in a row (the same misread came twice in a row: MP "6395").
const MAX_SHARE: (f64, f64) = (0.75, 1.34);
const FAR_MAX_HOLDS: u32 = 3;

/// A bar jumping by this much between two frames is a misread (or a
/// level-up, which the EXP bar does once a session at most): the status
/// strip is saved under `learned/debug/` to be looked at, a few times a
/// session and not more often than once a minute.
const JUMP: f32 = 45.0;
const DEBUG_PICTURES: u32 = 6;
const DEBUG_EVERY: Duration = Duration::from_secs(60);

/// While the font cannot read a line the HUD has, the teacher reads the
/// numbers this often, and never more often, while the game is in front
/// and the HUD in view: its numbers are what the companion trusts, and
/// they are kept fresh (on the owner's Classic HUD the lines were asked
/// about after 120, 240, 480, then every 900 s, and the warnings went on
/// the bars' fill in between). A read is the status strip at "high" detail.
pub const READ_EVERY: Duration = Duration::from_secs(25);

/// How long the bars may be missing before the HUD is looked for again.
const LOST_FOR: Duration = Duration::from_secs(20);
/// A map name read longer ago than this is not said any more: a map is
/// where they were, not where they are, and the regular check reads the
/// status strip alone (the name is on the minimap), so a read seldom
/// refreshes it. Ten minutes is a grinding session on one map; past that
/// the name is more likely wrong than right, and the model would present
/// it as fact.
const MAP_FOR: Duration = Duration::from_secs(600);

fn now_text() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M").to_string()
}

/// Whether `red` and `blue` lie as the HP and MP bars do: the one right
/// over the other with the same left edge (give or take a bar's height),
/// or side by side in one row, HP first, no more than a bar's width apart.
/// Lava, a monster's own health bar, a red border: not beside the MP bar.
fn neighbours(red: &BarModel, blue: &BarModel, fw: u32, fh: u32) -> bool {
    let (rx, ry, rw, rh) = red.band.pixels(fw, fh);
    let (bx, by, bw, bh) = blue.band.pixels(fw, fh);
    let tall = rh.max(bh).max(2);
    let stacked =
        rx.abs_diff(bx) <= tall && ry + rh <= by + tall && by.saturating_sub(ry + rh) <= tall * 3;
    let in_a_row =
        ry.abs_diff(by) <= tall && rx + rw <= bx + tall && bx.saturating_sub(rx + rw) <= rw.max(bw);
    stacked || in_a_row
}

/// The bar of `colour` beside `sibling`, where the HP and MP bars lie
/// together: right above it (or below, `first` false), with the same left
/// edge; or, failing that, before it in the same row (or after). Lined up
/// give or take a bar's height, and about as tall (heights are rough: text
/// over a bar leaves a band of its lower rows, a shadow under one adds
/// rows). A bar found over or under its sibling takes the sibling's track,
/// where that is known; the numbers fix it from there.
fn bar_beside(
    frame: &RgbaImage,
    sibling: &BarModel,
    first: bool,
    colour: ((f32, f32), f32, f32, f32),
) -> Option<BarModel> {
    use syrup::bars::find_bar;
    use syrup::geometry::Rect;
    let (fw, fh) = frame.dimensions();
    let (hue, sat, val, expected) = colour;
    let (bx, by, bw, bh) = sibling.band.pixels(fw, fh);
    let tall = bh.max(2);
    let margin = (bw / 8).max(4);
    let reach = bh.saturating_mul(3).max(6);
    // A region clipped to the frame.
    let within = |x: u32, y: u32, w: u32, h: u32| Rect {
        x: x.min(fw),
        y: y.min(fh),
        w: w.min(fw.saturating_sub(x)),
        h: h.min(fh.saturating_sub(y)),
    };
    // Over or under the sibling, as wide as it with a little to spare.
    let stacked_y = if first {
        by.saturating_sub(reach)
    } else {
        by + bh
    };
    let stacked = within(bx.saturating_sub(margin), stacked_y, bw + 2 * margin, reach);
    // Before or after it in the same row: up to a bar and a half away.
    let span = bw + bw / 2 + margin;
    let row_x = if first {
        bx.saturating_sub(span)
    } else {
        bx + bw
    };
    let row = within(row_x, by.saturating_sub(tall), span, bh + 2 * tall);
    for (region, over_under) in [(stacked, true), (row, false)] {
        if region.w == 0 || region.h == 0 {
            continue;
        }
        let Some(fill) = find_bar(frame, region, hue, sat, val) else {
            continue;
        };
        let approx = NBox::from_pixels(fill.x, fill.y, fill.w, fill.h, fw, fh);
        let Some(mut model) = BarModel::learn(frame, &approx, Some(expected)) else {
            continue;
        };
        let (mx, my, mw, mh) = model.band.pixels(fw, fh);
        let as_tall = mh * 3 >= bh && mh <= bh * 3;
        let lined_up = if over_under {
            mx.abs_diff(bx) <= tall
        } else {
            // In the row, and clear of the sibling.
            my.abs_diff(by) <= tall && (mx + mw <= bx + tall || mx >= bx + bw)
        };
        if !as_tall || !lined_up {
            continue;
        }
        if over_under {
            model.band.x0 = sibling.band.x0;
            model.band.x1 = sibling.band.x1.max(model.band.x1);
            // As tall as its sibling, from the bottom up: the text over a
            // bar leaves only its lower rows as a run of its colour, and
            // the numbers are read from the band's rows — a band of the
            // lower rows alone would cut the digits in half.
            if mh < bh {
                let bottom = my + mh;
                let top = bottom.saturating_sub(bh);
                model.band.y0 = top as f32 / fh as f32;
                model.band.y1 = bottom as f32 / fh as f32;
            }
        }
        return Some(model);
    }
    None
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
            map_at: None,
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
            told: [None; 3],
            fill_off: [false; 3],
            pending_at: None,
            font_read_at: [None; 3],
            unread_since: [None; 3],
            observed_at: None,
            far_max: [None; 2],
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
        self.wants_at(width, height, Instant::now())
    }

    /// [`Sight::wants`] at `now`.
    fn wants_at(&self, width: u32, height: u32, now: Instant) -> Option<Want> {
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
        // A HUD known but for the HP or MP bar, which the pixels have not
        // found beside the other for a while: the model is asked to box
        // it (with the usual backoff, should it fail too).
        if self.lacks_a_bar()
            && self
                .find_failing_since
                .is_some_and(|t| now.duration_since(t) >= FIND_FOR)
        {
            return Some(Want::Calibrate);
        }
        if self.numbers_due(now) {
            return Some(Want::Verify);
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
        let now = Instant::now();
        self.asked = Some(now);
        self.pending_at = Some(now);
    }

    /// Whether the numbers are due to be read by the teacher: the HP or MP
    /// line, whose bar the HUD has, unread by the font for [`UNREAD_FOR`];
    /// the game in front and the HUD in view; and the last look
    /// [`READ_EVERY`] ago or more ([`READ_SOONER`] when a fill disagreed
    /// with the last read). Not held back by how little the reads teach
    /// the font: while it cannot read the line, the teacher's numbers are
    /// the ones to trust. (EXP alone, which no warning rests on, is asked
    /// about for its line's sake only.)
    fn numbers_due(&self, now: Instant) -> bool {
        let Some(layout) = &self.layout else {
            return false;
        };
        let in_front = self
            .observed_at
            .is_some_and(|t| now.saturating_duration_since(t) <= IN_FRONT_FOR);
        if !in_front || self.lost_since.is_some() {
            return false;
        }
        let unread = [&layout.hp, &layout.mp]
            .into_iter()
            .zip(self.unread_since)
            .any(|(bar, since)| {
                bar.is_some()
                    && since.is_some_and(|t| now.saturating_duration_since(t) >= UNREAD_FOR)
            });
        let every = if self.fill_off[..2].contains(&true) {
            READ_SOONER
        } else {
            READ_EVERY
        };
        unread
            && self
                .last_look
                .is_none_or(|t| now.saturating_duration_since(t) >= every)
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
        self.pending_at = None;
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
        let [mut hp, mp, exp] = models;
        // The HP and MP bars lie together: MP right under HP with the
        // same left edge, or the two side by side. Two "bars" that do not
        // — the lava, a monster's own health bar — are not both bars: the
        // blue one is trusted (less of the scenery is blue) and the red
        // one looked for right above it.
        if let Some(blue) = &mp
            && !hp.as_ref().is_some_and(|red| neighbours(red, blue, fw, fh))
        {
            hp = bar_beside(frame, blue, true, BAR_COLOURS[0]);
        }
        let found = [&hp, &mp, &exp].iter().filter(|m| m.is_some()).count();
        if found == 0 {
            return Err("no HP, MP or EXP bar in the status band".into());
        }
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

    /// Whether the layout has no HP or no MP bar (the pixels took the
    /// lava for one, or found neither; the model boxed the wrong place).
    pub fn lacks_a_bar(&self) -> bool {
        self.layout
            .as_ref()
            .is_some_and(|l| l.hp.is_none() || l.mp.is_none())
    }

    /// Find the HP or MP bar a layout lacks, beside the one it has: the
    /// two are stacked, MP right under HP, with the same left edge and
    /// the same track — where the scenery seldom passes for a bar, as it
    /// does in the whole status band (on a lava map, the lava is the
    /// biggest red thing there). With both missing, the blue bar is
    /// looked for first (less of the scenery is blue) and the red one
    /// above it. Returns what was found, for the log.
    pub fn find_missing_bars(&mut self, frame: &RgbaImage) -> Result<String, String> {
        use syrup::bars::find_bar;
        use syrup::geometry::Rect;
        let (fw, fh) = frame.dimensions();
        let Some(layout) = self.layout.as_ref() else {
            return Err("no layout".into());
        };
        if !layout.fits(fw, fh) {
            return Err("the layout is for another shape of screen".into());
        }
        let (mut hp, mut mp) = (layout.hp.clone(), layout.mp.clone());
        if hp.is_none() && mp.is_none() {
            // Neither: the blue bar in the status band, then the red one
            // right above it.
            let band = Rect {
                x: 0,
                y: fh.saturating_mul(9) / 10,
                w: fw.saturating_mul(3) / 4,
                h: fh - fh.saturating_mul(9) / 10,
            };
            let (hue, sat, val, expected) = BAR_COLOURS[1];
            if let Some(fill) = find_bar(frame, band, hue, sat, val) {
                let approx = NBox::from_pixels(fill.x, fill.y, fill.w, fill.h, fw, fh);
                mp = BarModel::learn(frame, &approx, Some(expected));
            }
            if let Some(blue) = &mp {
                hp = bar_beside(frame, blue, true, BAR_COLOURS[0]);
                // The blue bar alone could be the scenery; with the red one
                // lined up above it, it is the MP bar.
                if hp.is_none() {
                    mp = None;
                }
            }
        } else if hp.is_none() {
            hp = layout
                .mp
                .as_ref()
                .and_then(|blue| bar_beside(frame, blue, true, BAR_COLOURS[0]));
        } else if mp.is_none() {
            mp = layout
                .hp
                .as_ref()
                .and_then(|red| bar_beside(frame, red, false, BAR_COLOURS[1]));
        }
        let found_hp = hp.is_some() && layout.hp.is_none();
        let found_mp = mp.is_some() && layout.mp.is_none();
        if !found_hp && !found_mp {
            return Err("the missing bar is not beside the other".into());
        }
        let mut notes = Vec::new();
        if found_hp {
            notes.push("HP bar");
        }
        if found_mp {
            notes.push("MP bar");
        }
        let layout = self.layout.as_mut().expect("checked above");
        layout.hp = hp;
        layout.mp = mp;
        // The status strip takes the new bar in.
        let mut status = layout.status;
        for b in [&layout.hp, &layout.mp].into_iter().flatten() {
            let grown = b.band.grown(0.04, 1.2);
            status = Some(match status {
                Some(s) => s.union(&grown),
                None => grown,
            });
        }
        layout.status = status;
        layout.found = now_text();
        self.lost_since = None;
        self.find_failing_since = None;
        self.save();
        Ok(format!(
            "found the {} beside the other from the pixels",
            notes.join(" and ")
        ))
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
        self.observed_at = Some(now);
        // No HUD known for a screen this shape: the pixels look for it,
        // once a second, and the clock runs on how long they fail. A HUD
        // known but for the HP or MP bar: the missing bar is looked for
        // beside the other, once a second, the same way.
        let fits = self
            .layout
            .as_ref()
            .is_some_and(|l| l.fits(frame.width(), frame.height()));
        let due = self
            .find_tried
            .is_none_or(|t| now.duration_since(t) >= FIND_EVERY);
        if !fits && due {
            self.find_tried = Some(now);
            let _find = tracing::trace_span!("sight.find").entered();
            if self.find_hud(frame).is_err() {
                self.find_failing_since.get_or_insert(now);
            }
        } else if fits && due && self.lacks_a_bar() {
            self.find_tried = Some(now);
            let _find = tracing::trace_span!("sight.find").entered();
            if self.find_missing_bars(frame).is_err() {
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
            let i = field as usize;
            let Some(read) = read else {
                self.unread_since[i].get_or_insert(now);
                continue;
            };
            // The game's own number, read: the last read, as good as the
            // teacher's (the fill is judged against it until the next).
            self.unread_since[i] = None;
            self.font_read_at[i] = Some(now);
            self.told[i] = Some(Told {
                at: now,
                percent: read.value.percent(),
                amount: match (field, &read.value) {
                    (Field::Hp | Field::Mp, Value::Amount { current, max }) => {
                        Some((*current, *max))
                    }
                    _ => None,
                },
            });
            self.fill_off[i] = false;
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
        // No bar, no line to read.
        for (i, band) in bands.iter().enumerate() {
            if band.is_none() {
                self.unread_since[i] = None;
            }
        }
        // The HP or MP line the font did not read this frame: its fill only
        // while it agrees with the last read. (EXP's fill is left as it is:
        // no warning rests on it, and its wrap at a level-up is a fill far
        // from the last read by nature.)
        if seen.hp_number.is_none() {
            seen.hp = self.judge_fill(0, seen.hp, now);
        }
        if seen.mp_number.is_none() {
            seen.mp = self.judge_fill(1, seen.mp, now);
        }
        // The EXP bar wrapping — from nearly full to nearly empty, and
        // staying there — is most likely a level-up: the level is to be
        // read again (`Want::Verify`), and the number at the bottom left,
        // read, is what says the level went up; the bar alone says
        // nothing, and the level fact is not touched on its account. The
        // full must have lasted — a run of readings, not one frame's
        // misreading (something of the bar's colour over it) — and the
        // number, when it was read lately, has the last word.
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

    /// The fill of field `i` (0 HP, 1 MP, 2 EXP), on a frame the font did
    /// not read it, as a reading: none once it has disagreed with the
    /// teacher's last read by more than [`FILL_AGREES`] points, until the
    /// next read — so a wrong fill never drives a warning by itself, and
    /// does not drop in and out either. With no read lately (no teacher,
    /// or it cannot answer), the fill as it is.
    fn judge_fill(&mut self, i: usize, fill: Option<f32>, now: Instant) -> Option<f32> {
        let fill = fill?;
        let Some(told) =
            self.told[i].filter(|t| now.saturating_duration_since(t.at) <= TOLD_JUDGES)
        else {
            return Some(fill);
        };
        if self.fill_off[i] || (fill - told.percent).abs() > FILL_AGREES {
            self.fill_off[i] = true;
            return None;
        }
        Some(fill)
    }

    /// `v` as believed: an HP or MP whose maximum is far from the one known
    /// (outside [`MAX_SHARE`] of it) is left out, with its text, unless the
    /// level read with it changed or it has held for [`FAR_MAX_HOLDS`]
    /// reads in a row. A maximum moves at a level-up, a little; the
    /// classic HUD's thin slash read as a 7 moves it tenfold ("3947/6771"
    /// for 594/671), and the same misread can come twice in a row. Returns
    /// the values and a note for each left out.
    fn believe(&mut self, v: &HudValues) -> (HudValues, Vec<String>) {
        let mut v = v.clone();
        let mut notes = Vec::new();
        let level_changed = matches!((self.facts.level, v.level), (Some(a), Some(b)) if a != b);
        for i in 0..2 {
            let (amount, known, name) = if i == 0 {
                (v.hp, self.facts.hp_max, "HP")
            } else {
                (v.mp, self.facts.mp_max, "MP")
            };
            let Some((current, max)) = amount else {
                continue;
            };
            let near = known.is_none_or(|k| {
                let share = max as f64 / k.max(1) as f64;
                (MAX_SHARE.0..=MAX_SHARE.1).contains(&share)
            });
            let held = match self.far_max[i] {
                Some((m, n)) if m == max => n + 1,
                _ => 1,
            };
            if near || level_changed || held >= FAR_MAX_HOLDS {
                self.far_max[i] = None;
                continue;
            }
            self.far_max[i] = Some((max, held));
            notes.push(format!(
                "{name} {current}/{max} not believed: the maximum known is {} (a misread, unless it holds)",
                known.unwrap_or_default()
            ));
            if i == 0 {
                v.hp = None;
                v.hp_text = None;
            } else {
                v.mp = None;
                v.mp_text = None;
            }
        }
        (v, notes)
    }

    /// The teacher's numbers in `v`, read off the frame taken at `at`: the
    /// ones to trust while the font cannot read the lines.
    fn tell(&mut self, v: &HudValues, at: Instant) {
        let told = [
            v.hp.map(|(c, m)| (c as f32 / m.max(1) as f32 * 100.0, Some((c, m)))),
            v.mp.map(|(c, m)| (c as f32 / m.max(1) as f32 * 100.0, Some((c, m)))),
            v.exp_percent.map(|p| (p, None)),
        ];
        for (i, told) in told.into_iter().enumerate() {
            if let Some((percent, amount)) = told {
                self.told[i] = Some(Told {
                    at,
                    percent: percent.clamp(0.0, 100.0),
                    amount,
                });
                self.fill_off[i] = false;
            }
        }
    }

    /// The numbers as the teacher last read them, with their age, for the
    /// fields the font is not reading: on a HUD whose font is not learned,
    /// what the conversation answers from.
    fn told_line(&self, now: Instant) -> Option<String> {
        let mut parts = Vec::new();
        for (i, name) in ["HP", "MP", "EXP"].into_iter().enumerate() {
            let Some(t) = self.told[i] else { continue };
            let age = now.saturating_duration_since(t.at);
            let font_reads = self.font_read_at[i]
                .is_some_and(|f| now.saturating_duration_since(f) <= FONT_FRESH);
            if age > TOLD_SAID || font_reads {
                continue;
            }
            let ago = match age.as_secs() {
                s if s < 120 => format!("{s} s ago"),
                s => format!("{} min ago", s / 60),
            };
            parts.push(match t.amount {
                Some((c, m)) => format!("{name} {c}/{m} ({:.0}%, {ago})", t.percent),
                None => format!("{name} {:.2}% ({ago})", t.percent),
            });
        }
        (!parts.is_empty()).then(|| {
            format!(
                "{}, as read from the HUD (the numbers then; the bars are measured in between, and a bar that \
disagrees with them is not believed).",
                parts.join(", ")
            )
        })
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
            self.map_at = Some(Instant::now());
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
        let at = self.pending_at.take().unwrap_or_else(Instant::now);
        let (values, disbelieved) = self.believe(&c.values);
        let learn = |b: &Option<NBox>, hue: f32| {
            b.as_ref()
                .and_then(|b| BarModel::learn(frame, b, Some(hue)))
        };
        // A bar the model's box did not give (boxed over the scenery, or
        // not boxed) keeps the one already learned for this shape of
        // screen, if there is one: a bad box must not lose a good bar.
        let (fw, fh) = frame.dimensions();
        let kept = self
            .layout
            .as_ref()
            .filter(|l| l.fits(fw, fh))
            .map(|l| (l.hp.clone(), l.mp.clone(), l.exp.clone()))
            .unwrap_or_default();
        let missing_before = [&kept.0, &kept.1].iter().filter(|b| b.is_none()).count();
        let mut hp = learn(&c.hp, 0.0).or(kept.0);
        let mut mp = learn(&c.mp, 215.0).or(kept.1);
        let mut exp = learn(&c.exp, 55.0).or(kept.2);
        // The game's numbers fix where each track ends (those believed).
        if let (Some(b), Some(p)) = (hp.as_mut(), values.hp_percent()) {
            b.reading(frame, p);
        }
        if let (Some(b), Some(p)) = (mp.as_mut(), values.mp_percent()) {
            b.reading(frame, p);
        }
        if let (Some(b), Some(p)) = (exp.as_mut(), values.exp_percent) {
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
        parts.push(format!("read {}", c.values.summary()));
        parts.extend(disbelieved);
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
        parts.extend(self.learn_font(frame, &values));
        self.take_values(&values);
        self.tell(&values, at);
        self.want = None;
        self.disagreements = 0;
        self.lost_since = None;
        self.find_failing_since = None;
        self.last_look = Some(Instant::now());
        // Finding the HUD is help enough; the lines' examples come later.
        // Asked for a bar the layout lacked and given none, the next ask
        // waits longer each time: a model that boxes the lava for the HP
        // bar every time must not cost a call every ten seconds.
        let missing_after = self
            .layout
            .as_ref()
            .map(|l| [&l.hp, &l.mp].iter().filter(|b| b.is_none()).count())
            .unwrap_or(0);
        if missing_before > 0 && missing_after >= missing_before {
            self.looked();
            parts.push(format!(
                "the {} still not found; the next look waits {} s",
                if missing_after == 2 {
                    "HP and MP bars"
                } else if self.layout.as_ref().is_some_and(|l| l.hp.is_none()) {
                    "HP bar"
                } else {
                    "MP bar"
                },
                self.ask_backoff.as_secs()
            ));
        } else {
            self.answered(true);
        }
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
        let at = self.pending_at.take().unwrap_or_else(Instant::now);
        let read = v;
        let (believed, disbelieved) = self.believe(v);
        let v = &believed;
        let wanted_labels = self.still_unlabelled();
        let mut notes = disbelieved;
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
        self.tell(v, at);
        // The model saw no HUD at all (a cutscene, a dialog over it, the
        // HUD hidden): asking again at once would only ask again at once,
        // every few seconds, for as long as it stays hidden. The next look
        // waits, and longer each time. (A read not believed was a HUD seen.)
        let saw_hud = read.level.is_some()
            || read.hp.is_some()
            || read.mp.is_some()
            || read.exp_percent.is_some();
        if !saw_hud {
            self.looked();
            notes.push(format!(
                "no HUD in view; the next look waits {} s",
                self.ask_backoff.as_secs()
            ));
        } else {
            // Asked to spell the lines out and none of them learned: the
            // next ask for the lines' sake waits longer; the numbers are
            // read again on the cadence all the same (`numbers_due`).
            let helped = !wanted_labels || !self.still_unlabelled();
            self.answered(helped);
            if !helped {
                notes.push(format!(
                    "no line learned from this; the numbers are read again every {} s while the font \
cannot read them",
                    READ_EVERY.as_secs()
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
                self.map_at = Some(Instant::now());
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

    /// How long ago the map's name ([`Facts::map`]) was read.
    pub fn map_age(&self) -> Option<Duration> {
        self.map_at.map(|at| at.elapsed())
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
        // The map with its age, while it is young enough to be worth
        // saying: where they were, not where they are.
        if let (Some(m), Some(at)) = (&f.map, self.map_at) {
            let age = at.elapsed();
            if age <= MAP_FOR {
                let ago = match age.as_secs() / 60 {
                    0 => "less than a minute ago".to_string(),
                    1 => "a minute ago".to_string(),
                    minutes => format!("{minutes} min ago"),
                };
                who.push(format!("map {m}, as read {ago} (it may have changed)"));
            }
        }
        if !who.is_empty() {
            lines.push(format!("Character: {}.", who.join(", ")));
        }
        if let (Some(hp), Some(mp)) = (f.hp_max, f.mp_max) {
            lines.push(format!("Max HP {hp}, max MP {mp} (when last read)."));
        }
        if let Some(line) = self.told_line(Instant::now()) {
            lines.push(line);
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

    /// `secs` ago — or `None` when the monotonic clock is younger than that
    /// (a CI runner that booted a minute before the tests), where
    /// `Instant - Duration` would panic.
    fn earlier(secs: u64) -> Option<Instant> {
        Instant::now().checked_sub(Duration::from_secs(secs))
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
        // Measured on other frames, and put into the observation (within
        // FILL_AGREES of the read, 60% and 100%: a fill farther from it is
        // no reading until the next read).
        let seen = sight.observe(&status_bar(50.0, 90.0), Instant::now());
        assert!((seen.hp.unwrap() - 50.0).abs() < 1.5, "{seen:?}");
        assert!((seen.mp.unwrap() - 90.0).abs() < 1.5, "{seen:?}");
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
    fn a_map_name_is_said_with_its_age_and_not_after_ten_minutes_nor_next_run() {
        let dir = temp_dir("map");
        let mut sight = Sight::load(&dir);
        let frame = status_bar(60.0, 100.0);
        sight.calibrated(&frame, &calibration()).unwrap();
        // A read with the map: said with its age, and that it may have
        // changed (a map is where they were, not where they are).
        let with_map = HudValues {
            hp: Some((3000, 5000)),
            map: Some("Gate of the Future".into()),
            ..Default::default()
        };
        sight.verified(&frame, &with_map);
        let text = sight.describe().join("\n");
        assert!(
            text.contains(
                "map Gate of the Future, as read less than a minute ago (it may have changed)"
            ),
            "{text}"
        );
        let (Some(five_min_ago), Some(quarter_hour_ago)) = (earlier(5 * 60), earlier(15 * 60))
        else {
            eprintln!("the clock is too young for this test's aged map: skipping the rest");
            let _ = std::fs::remove_dir_all(&dir);
            return;
        };
        sight.map_at = Some(five_min_ago);
        let text = sight.describe().join("\n");
        assert!(
            text.contains("map Gate of the Future, as read 5 min ago (it may have changed)"),
            "{text}"
        );
        // A quarter of an hour on, a read without a map (the regular check
        // reads the status strip, where the name is not): the name is not
        // said any more.
        sight.map_at = Some(quarter_hour_ago);
        let without_map = HudValues {
            hp: Some((3000, 5000)),
            ..Default::default()
        };
        sight.verified(&frame, &without_map);
        let text = sight.describe().join("\n");
        assert!(!text.contains("Gate of the Future"), "{text}");
        // The player says where they are: as good as a read, and as fresh.
        sight.correct("map", "Henesys").unwrap();
        let text = sight.describe().join("\n");
        assert!(
            text.contains("map Henesys, as read less than a minute ago"),
            "{text}"
        );
        // Never kept for the next run.
        let again = Sight::load(&dir);
        assert_eq!(again.facts.map, None);
        assert!(!again.describe().join("\n").contains("Henesys"));
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
        // (HP and MP within FILL_AGREES of the read: a fill farther from it
        // is no reading until the next read.)
        let (other, _) = numbers::tests::hud((200, 400), (1216, 1351), 10.0);
        let seen = sight.observe(&other, Instant::now());
        assert!((seen.hp.unwrap() - 50.0).abs() < 3.0, "{seen:?}");
        assert!((seen.mp.unwrap() - 90.0).abs() < 3.0, "{seen:?}");
        assert!((seen.exp.unwrap() - 10.0).abs() < 3.0, "{seen:?}");
        let (far, _) = numbers::tests::hud((100, 400), (1216, 1351), 10.0);
        assert_eq!(sight.observe(&far, Instant::now()).hp, None);
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
        // The bar it lacks is found beside the other on the next frame
        // (the layout fits this screen, so the pixels would otherwise
        // never have looked again), lined up with it; and is reported
        // once the number beside it fits its track.
        let mut again = again;
        let t0 = Instant::now();
        again.observe(&frame, t0);
        let layout = again.layout.as_ref().unwrap();
        let (hp, mp) = (
            layout
                .hp
                .as_ref()
                .expect("the HP bar, found beside the MP bar"),
            layout.mp.as_ref().unwrap(),
        );
        assert!(
            syrup::bars::hue_distance(hp.hue, 350.0) < 25.0,
            "{}",
            hp.hue
        );
        // (This HUD lays the bars in a row: HP before MP.)
        assert!(neighbours(hp, mp, 1280, 720), "{hp:?} beside {mp:?}");
        assert!((hp.band.x0 - 100.0 / 1280.0).abs() < 0.004, "{hp:?}");
        assert!(!again.lacks_a_bar());
        again.verified(&frame, &values);
        let seen = again.observe(&frame, t0 + Duration::from_secs(2));
        assert!((seen.hp.unwrap() - 60.0).abs() < 3.0, "{seen:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_layout_lacking_both_bars_gets_them_as_a_pair_and_asks_the_model_only_in_time() {
        let dir = temp_dir("pair");
        let mut sight = Sight::load(&dir);
        let (frame, _) = numbers::tests::hud((240, 400), (1351, 1351), 50.0);
        let t0 = Instant::now();
        sight.observe(&frame, t0);
        // A layout saved with the EXP bar alone (as a player's was, after
        // the model boxed the lava for the HP bar and the box was dropped).
        {
            let layout = sight.layout.as_mut().unwrap();
            layout.hp = None;
            layout.mp = None;
        }
        assert!(sight.lacks_a_bar());
        // A blank strip (a cutscene): nothing to find yet, and the model is
        // not asked before the pixels have had their time.
        let blank = RgbaImage::from_pixel(1280, 720, image::Rgba([20, 20, 30, 255]));
        sight.observe(&blank, t0 + Duration::from_secs(1));
        assert!(sight.lacks_a_bar());
        assert_eq!(sight.wants(1280, 720), None);
        // Still nothing after a while (as if the pixels had been failing
        // for twelve seconds; `wants` reads the clock): the model is asked
        // to box them…
        for i in 2..14 {
            sight.observe(&blank, t0 + Duration::from_secs(i));
        }
        assert!(sight.find_failing_since.is_some());
        sight.find_failing_since = Some(Instant::now() - Duration::from_secs(12));
        assert_eq!(sight.wants(1280, 720), Some(Want::Calibrate));
        // …and its boxes over the scenery do not lose the EXP bar, nor
        // bring the missing ones; the next ask waits (and says so).
        sight.asking();
        let bad = Calibration {
            hp: Some(NBox::new(0.1, 0.1, 0.3, 0.12)),
            mp: Some(NBox::new(0.1, 0.2, 0.3, 0.22)),
            exp: None,
            level: None,
            minimap: None,
            values: HudValues::default(),
        };
        let note = sight.calibrated(&frame, &bad).unwrap();
        assert!(
            note.contains("still not found; the next look waits"),
            "{note}"
        );
        let layout = sight.layout.as_ref().unwrap();
        assert!(layout.exp.is_some() && layout.hp.is_none() && layout.mp.is_none());
        assert_eq!(sight.wants(1280, 720), None, "held back by the backoff");
        // The HUD back in view: both bars, as a pair, from the pixels.
        sight.observe(&frame, t0 + Duration::from_secs(20));
        let layout = sight.layout.as_ref().unwrap();
        let (hp, mp) = (layout.hp.as_ref().unwrap(), layout.mp.as_ref().unwrap());
        assert!(neighbours(hp, mp, 1280, 720), "{hp:?} beside {mp:?}");
        assert!((hp.band.x0 - 100.0 / 1280.0).abs() < 0.004, "{hp:?}");
        assert!((mp.band.x0 - 420.0 / 1280.0).abs() < 0.004, "{mp:?}");
        assert!(!sight.lacks_a_bar());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The bottom of a player's 4K screen (3,840 × 216 of 3,840 × 2,160),
    /// as captured: the HP and MP bars in the middle under the text, the
    /// EXP bar along the bottom — and monsters in the status band, whose
    /// red passed for the HP bar when each colour was looked for alone.
    fn real_4k_frame() -> RgbaImage {
        let strip = image::open(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/resources/hud-4k-strip.png"
        ))
        .expect("the 4K strip fixture")
        .to_rgba8();
        let mut frame = RgbaImage::from_pixel(3840, 2160, image::Rgba([20, 20, 30, 255]));
        image::imageops::replace(&mut frame, &strip, 0, (2160 - strip.height()) as i64);
        frame
    }

    #[test]
    fn on_a_real_4k_screen_the_hp_and_mp_bars_are_found_as_a_pair() {
        let dir = temp_dir("4k");
        let mut sight = Sight::load(&dir);
        let frame = real_4k_frame();
        let note = sight.find_hud(&frame).unwrap();
        assert!(note.starts_with("found 3 bar(s)"), "{note}");
        // The player's bars: 1,732 px in, HP over MP, the EXP bar below.
        let layout = sight.layout.clone().unwrap();
        let (hp, mp, exp) = (layout.hp.unwrap(), layout.mp.unwrap(), layout.exp.unwrap());
        let (hx, hy, _, hh) = hp.band.pixels(3840, 2160);
        let (mx, my, _, _) = mp.band.pixels(3840, 2160);
        assert!(
            hx.abs_diff(1732) <= 4 && mx.abs_diff(1732) <= 4,
            "{hp:?} {mp:?}"
        );
        assert!((2050..=2072).contains(&hy) && hy + hh <= 2092, "{hp:?}");
        assert!(my.abs_diff(2092) <= 4, "{mp:?}");
        assert!(
            syrup::bars::hue_distance(hp.hue, 340.0) < 10.0,
            "{}",
            hp.hue
        );
        assert!(exp.band.y0 > 0.98, "{exp:?}");
        // Lacking both (as the player's layout did), they are found again
        // as a pair, in the same place.
        {
            let layout = sight.layout.as_mut().unwrap();
            layout.hp = None;
            layout.mp = None;
        }
        let note = sight.find_missing_bars(&frame).unwrap();
        assert!(note.contains("HP bar and MP bar"), "{note}");
        let layout = sight.layout.as_ref().unwrap();
        let (hx2, hy2, _, _) = layout.hp.as_ref().unwrap().band.pixels(3840, 2160);
        assert!(hx2.abs_diff(1732) <= 4 && hy2.abs_diff(hy) <= 4);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The bottom of a player's 4K screen on Classic World, whose old
    /// status bar prints `HP[178/178]`, `MP[101/101]` and `EXP. 619[49.84%]`
    /// above the bars, small and thin (see `tests/classic_hud.rs`).
    fn classic_4k_frame() -> RgbaImage {
        let strip = image::open(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/resources/hud-classic-4k-strip.png"
        ))
        .expect("the classic 4K strip fixture")
        .to_rgba8();
        let mut frame = RgbaImage::from_pixel(3840, 2160, image::Rgba([20, 20, 30, 255]));
        image::imageops::replace(&mut frame, &strip, 0, (2160 - strip.height()) as i64);
        frame
    }

    #[test]
    fn on_the_classic_hud_the_numbers_are_read_every_frame_once_the_teacher_spelled_them_out() {
        let dir = temp_dir("classic");
        let mut sight = Sight::load(&dir);
        let frame = classic_4k_frame();
        // The bars where the player's sight boxed them, and the lines as the
        // teacher spelled them on its first look (the session of 9 October,
        // when every one of them "split into 11 glyphs": the bar's ticks).
        let c = Calibration {
            level: None,
            hp: Some(NBox::new(0.36822918, 0.9777778, 0.4450521, 0.9925926)),
            mp: Some(NBox::new(0.44739583, 0.9777778, 0.5239583, 0.9930556)),
            exp: Some(NBox::new(0.53020835, 0.9777778, 0.6132866, 0.9930556)),
            minimap: None,
            values: HudValues {
                level: Some(9),
                hp: Some((178, 178)),
                mp: Some((101, 101)),
                exp_percent: Some(49.84),
                hp_text: Some("178/178".into()),
                mp_text: Some("101/101".into()),
                exp_text: Some("619[49.84%]".into()),
                ..Default::default()
            },
        };
        let line = sight.calibrated(&frame, &c).unwrap();
        assert!(line.contains("found 3 bar(s)"), "{line}");
        assert!(!line.contains("not learned"), "{line}");
        // Every frame, the numbers in the game's font, the same each time
        // (on the classic HUD: its style, for the session's stats).
        let t0 = Instant::now();
        let mut seen = Seen::default();
        for i in 0..5 {
            seen = sight.observe(&frame, t0 + Duration::from_millis(100 * i));
            assert_eq!(sight.numbers.classic(), Some(true));
            assert_eq!(seen.hp_number, Some((178, 178)), "{i}: {seen:?}");
            assert_eq!(seen.mp_number, Some((101, 101)), "{i}: {seen:?}");
            assert_eq!(
                seen.exp_number,
                Some(Value::Percent(49.84)),
                "{i}: {seen:?}"
            );
        }
        // Read, so the companion trusts them over the bars' fill (which a
        // cursor over a bar throws off).
        let mut obs = Observation::unseen(GameView::Seen("MapleStory".into()));
        sight.apply(&mut obs, &seen);
        let (hp, mp, exp) = (obs.hp.unwrap(), obs.mp.unwrap(), obs.exp.unwrap());
        assert!(
            hp.read && hp.current == Some(178) && hp.max == Some(178),
            "{hp:?}"
        );
        assert!(mp.read && mp.current == Some(101), "{mp:?}");
        assert!(exp.read && (exp.percent - 49.84).abs() < 0.001, "{exp:?}");
        // And after a restart, from what was kept.
        let mut again = Sight::load(&dir);
        let seen = again.observe(&frame, t0 + Duration::from_secs(1));
        assert_eq!(seen.hp_number, Some((178, 178)), "{seen:?}");
        assert_eq!(seen.mp_number, Some((101, 101)), "{seen:?}");
        assert_eq!(seen.exp_number, Some(Value::Percent(49.84)), "{seen:?}");
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
        // (The level fact waits for the number to be read again.)
        assert_eq!(sight.facts.level, Some(61));
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
        // (Read on the modern HUD: its style, for the session's stats.)
        assert_eq!(sight.numbers.classic(), Some(false));
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
        // A digit the font never saw (6): the bar's estimate, not a reading
        // (and believed, being near the number last read, 315/400).
        let (unknown, _) = numbers::tests::hud((260, 400), (1000, 1351), 40.01);
        let seen = sight.observe(&unknown, Instant::now());
        assert_eq!(seen.hp_number, None);
        assert!((seen.hp.unwrap() - 65.0).abs() < 3.0, "{:?}", seen.hp);
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
        assert!(
            first.contains("no line learned from this; the numbers are read again every 25 s"),
            "{first}"
        );
        assert_eq!(sight.label_backoff, Duration::from_secs(120));
        // Held back for the lines' sake only; other reasons still ask (and
        // the numbers' own cadence: see the test after this one).
        assert_eq!(sight.wants(1280, 720), None);
        sight.want = Some(Want::Verify);
        assert_eq!(sight.wants(1280, 720), Some(Want::Verify));
        sight.want = None;
        sight.verified(&frame, &useless);
        assert_eq!(sight.label_backoff, Duration::from_secs(240));
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
        let Some(two_minutes_ago) = earlier(121) else {
            eprintln!("the clock is younger than two minutes: skipping the rest");
            let _ = std::fs::remove_dir_all(&dir);
            return;
        };
        sight.asked = Some(two_minutes_ago);
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

    /// The teacher's reads in the owner's Classic World session of 10
    /// October, as his log has them (`[sight] read …`; Israel time), names
    /// left out: time, level, HP, MP, EXP %. Checked against the frames he
    /// sent: 11:48 "659/659" and 11:52 "653/653" were 655/655 on screen;
    /// 12:21 and 12:36:15 read the classic HUD's thin slash as a 7 —
    /// `HP[594/671]` (the bar 88% full) came back as 3947/6771 (58%).
    type SessionRead = (
        &'static str,
        Option<u32>,
        Option<(u64, u64)>,
        Option<(u64, u64)>,
        Option<f32>,
    );
    const SESSION_READS: &[SessionRead] = &[
        (
            "11:46:07",
            Some(16),
            Some((655, 655)),
            Some((671, 671)),
            Some(80.42),
        ),
        (
            "11:48:10",
            Some(16),
            Some((659, 659)),
            Some((671, 671)),
            Some(80.42),
        ),
        (
            "11:52:11",
            Some(16),
            Some((653, 653)),
            Some((671, 671)),
            Some(80.42),
        ),
        (
            "12:00:14",
            Some(16),
            Some((659, 659)),
            Some((649, 671)),
            Some(84.75),
        ),
        (
            "12:12:36",
            Some(17),
            Some((671, 671)),
            Some((695, 695)),
            Some(2.16),
        ),
        (
            "12:21:12",
            Some(17),
            Some((5837, 7671)),
            Some((1027, 6395)),
            Some(7.32),
        ),
        (
            "12:36:15",
            Some(17),
            Some((3947, 6771)),
            Some((1347, 6395)),
            Some(16.13),
        ),
        (
            "12:36:20",
            Some(17),
            Some((576, 671)),
            Some((139, 695)),
            Some(16.22),
        ),
        (
            "12:36:37",
            None,
            Some((557, 671)),
            Some((57, 695)),
            Some(16.6),
        ),
        (
            "12:38:39",
            None,
            Some((416, 671)),
            Some((180, 695)),
            Some(19.32),
        ),
        (
            "12:38:44",
            Some(17),
            Some((416, 671)),
            Some((189, 695)),
            Some(19.32),
        ),
        (
            "12:39:02",
            None,
            Some((416, 671)),
            Some((207, 695)),
            Some(19.32),
        ),
        (
            "12:41:05",
            None,
            Some((567, 671)),
            Some((227, 695)),
            Some(22.45),
        ),
        (
            "12:45:08",
            None,
            Some((565, 671)),
            Some((695, 695)),
            Some(28.48),
        ),
        (
            "12:53:10",
            None,
            Some((671, 671)),
            Some((648, 695)),
            Some(30.82),
        ),
        (
            "13:08:14",
            None,
            Some((372, 671)),
            Some((227, 695)),
            Some(44.63),
        ),
        (
            "13:23:17",
            None,
            Some((306, 671)),
            Some((695, 695)),
            Some(44.67),
        ),
        (
            "13:38:21",
            None,
            Some((44, 671)),
            Some((695, 695)),
            Some(44.67),
        ),
        (
            "13:53:23",
            None,
            Some((0, 671)),
            Some((695, 695)),
            Some(44.67),
        ),
    ];

    fn session_read(row: &SessionRead) -> HudValues {
        let (_, level, hp, mp, exp) = *row;
        HudValues {
            level,
            hp,
            mp,
            exp_percent: exp,
            hp_text: hp.map(|(c, m)| format!("{c}/{m}")),
            mp_text: mp.map(|(c, m)| format!("{c}/{m}")),
            ..Default::default()
        }
    }

    #[test]
    fn while_the_font_cannot_read_a_line_the_teacher_reads_the_numbers_every_25_s() {
        let dir = temp_dir("cadence");
        let mut sight = Sight::load(&dir);
        let frame = status_bar(60.0, 100.0);
        sight.calibrated(&frame, &calibration()).unwrap();
        // The lines want examples the font cannot learn from the made-up
        // bar: as on the owner's Classic HUD, where every read but four
        // taught nothing and the lines were next asked about in 120, 240,
        // 480, then 900 s — the warnings went on the bars' fill meanwhile.
        let read = HudValues {
            hp: Some((3000, 5000)),
            mp: Some((2000, 2000)),
            hp_text: Some("HP [3000/5000]".into()),
            ..Default::default()
        };
        let t0 = Instant::now();
        sight.observe(&frame, t0);
        for i in 0..6u32 {
            let at = t0 + READ_EVERY * i;
            let line = sight.verified(&frame, &read);
            sight.last_look = Some(at);
            assert!(!line.contains("next asked about in"), "{i}: {line}");
            // Not again within 25 s, never more often…
            let soon = at + READ_EVERY - Duration::from_secs(1);
            sight.observe(&frame, soon);
            assert_eq!(sight.wants_at(1280, 720, soon), None, "{i}: {line}");
            // …and at 25 s, while the game is in front and the HUD in view.
            let due = at + READ_EVERY;
            sight.observe(&frame, due);
            assert_eq!(
                sight.wants_at(1280, 720, due),
                Some(Want::Verify),
                "read {i}: {line}"
            );
        }
        // The game not in front (no frame looked at lately): no read.
        let away = t0 + READ_EVERY * 6 + Duration::from_secs(30);
        assert_eq!(sight.wants_at(1280, 720, away), None);
        // The HUD not in view (a cutscene): no read for the numbers.
        let blank = RgbaImage::from_pixel(1280, 720, image::Rgba([20, 20, 20, 255]));
        sight.observe(&blank, away);
        assert_eq!(sight.wants_at(1280, 720, away), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_line_the_font_reads_is_not_asked_about_on_the_cadence() {
        let dir = temp_dir("cadence-read");
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
        sight.calibrated(&frame, &c).unwrap();
        let t0 = Instant::now();
        for s in 0..40u64 {
            let seen = sight.observe(&frame, t0 + Duration::from_secs(s));
            assert!(seen.hp_number.is_some() && seen.mp_number.is_some());
        }
        assert_eq!(
            sight.wants_at(1280, 720, t0 + Duration::from_secs(40)),
            None,
            "the font reads every line: nothing to ask"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_fill_that_disagrees_with_the_last_read_is_no_reading_until_the_next_read() {
        let dir = temp_dir("gated");
        let mut sight = Sight::load(&dir);
        let frame = status_bar(60.0, 100.0);
        sight.calibrated(&frame, &calibration()).unwrap();
        let t0 = Instant::now();
        // Read: HP 3000/5000 (60%). The fill a little lower: believed.
        let seen = sight.observe(&status_bar(52.0, 100.0), t0);
        assert!((seen.hp.unwrap() - 52.0).abs() < 2.0, "{seen:?}");
        let mut obs = Observation::unseen(GameView::Seen("MapleStory".into()));
        sight.apply(&mut obs, &seen);
        assert!(!obs.hp.unwrap().read);
        // 30% against the read's 60%: no reading, and none on the way back
        // (it does not drop in and out) until the teacher reads again.
        for (i, fill) in [30.0, 31.0, 58.0, 60.0].into_iter().enumerate() {
            let at = t0 + Duration::from_secs(1 + i as u64);
            let seen = sight.observe(&status_bar(fill, 100.0), at);
            assert_eq!(seen.hp, None, "{fill}: {seen:?}");
            let mut obs = Observation::unseen(GameView::Seen("MapleStory".into()));
            sight.apply(&mut obs, &seen);
            assert_eq!(obs.hp, None, "{fill}: never drives a warning by itself");
            // MP, which agrees with its read, goes on.
            assert!(obs.mp.is_some(), "{fill}");
        }
        // The next read says 30%: the fill is believed again.
        sight.verified(
            &status_bar(30.0, 100.0),
            &HudValues {
                hp: Some((1500, 5000)),
                mp: Some((2000, 2000)),
                ..Default::default()
            },
        );
        let seen = sight.observe(&status_bar(29.0, 100.0), t0 + Duration::from_secs(6));
        assert!((seen.hp.unwrap() - 29.0).abs() < 2.0, "{seen:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_read_whose_maximum_is_far_from_the_one_known_is_not_believed_unless_it_holds() {
        let dir = temp_dir("max");
        let mut sight = Sight::load(&dir);
        // HP 594/671 (88%) and MP 194/695 (28%), as on the owner's screen at
        // 12:36:15 (`hp-disagrees-6.png`), the bars fitted by a good read.
        let frame = status_bar(88.5, 27.9);
        let mut c = calibration();
        c.values.level = Some(17);
        c.values.hp = Some((594, 671));
        c.values.mp = Some((194, 695));
        sight.calibrated(&frame, &c).unwrap();
        // The slash read as a 7, as at 12:21:12 and then 12:36:15 (MP 6395
        // both times): not believed, not taken for the facts, no
        // disagreement with the bars (which were right) and so no new
        // search for the HUD — at 12:36:20 that search lost the HP bar.
        for row in &SESSION_READS[5..7] {
            let line = sight.verified(&frame, &session_read(row));
            assert!(line.contains("not believed"), "{}: {line}", row.0);
            assert!(!line.contains("bar said"), "{}: {line}", row.0);
            assert!(!line.contains("looked for again"), "{}: {line}", row.0);
            assert_eq!(
                (sight.facts.hp_max, sight.facts.mp_max),
                (Some(671), Some(695)),
                "{}",
                row.0
            );
        }
        // Every other read of the session is believed — a level-up's new
        // maximum (655 → 671) at once — and the snapshot says the last,
        // with its age.
        let dir2 = temp_dir("max-session");
        let mut whole = Sight::load(&dir2);
        whole.calibrated(&frame, &c).unwrap();
        for row in SESSION_READS {
            let line = whole.verified(&frame, &session_read(row));
            let misread = matches!(row.0, "12:21:12" | "12:36:15");
            assert_eq!(line.contains("not believed"), misread, "{}: {line}", row.0);
        }
        assert_eq!(
            (whole.facts.hp_max, whole.facts.mp_max),
            (Some(671), Some(695))
        );
        let text = whole.describe().join("\n");
        assert!(text.contains("HP 0/671"), "{text}");
        assert!(text.contains("as read from the HUD"), "{text}");
        // Another character (another level): believed at once.
        let other = HudValues {
            level: Some(200),
            hp: Some((4785, 4785)),
            ..Default::default()
        };
        assert!(!whole.verified(&frame, &other).contains("not believed"));
        assert_eq!(whole.facts.hp_max, Some(4785));
        // The same level, a far maximum: believed once it has held for
        // three reads in a row.
        let back = HudValues {
            level: Some(200),
            hp: Some((671, 671)),
            ..Default::default()
        };
        assert!(whole.verified(&frame, &back).contains("not believed"));
        assert!(whole.verified(&frame, &back).contains("not believed"));
        assert!(!whole.verified(&frame, &back).contains("not believed"));
        assert_eq!(whole.facts.hp_max, Some(671));
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&dir2);
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
