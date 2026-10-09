//! The HUD's numbers — HP and MP as `current/max`, the EXP percentage —
//! read on every frame in the game's own font.
//!
//! The game prints its numbers in one fixed pixel font, which Syrup's glyph
//! reader learns from a few labelled examples and then reads in a fraction
//! of a millisecond, refusing rather than guessing when a glyph is not
//! clearly one character. The labels come from whoever can read the line
//! exactly once: the OCR engine, when the text is sharp enough for it, or
//! the vision model. Each example is kept — the crop and what it says, under
//! `learned/font/` — and the font is rebuilt from them at start-up, so the
//! model and the OCR engine are asked less and less as the font fills in.
//!
//! ```text
//!   bar (learned)  ─▶  the text line beside it  ─▶  glyphs  ─▶  "HP[400/400]"
//!                                                         └▶  400 / 400 = 100%  ⇄  the bar's fill
//! ```
//!
//! What was read is cross-checked against the bar's fill every frame; the
//! two disagreeing for a while means the font or the bar is wrong, and the
//! sight asks for a fresh look.
//!
//! The classic HUD (Classic World) prints its numbers above the bars in a
//! thin font of its own, `HP[178/178]`, reaching higher above the bar than
//! a fixed [`Line`] does at 4K: its line is a [`Window`] measured in bar
//! heights, learned into a font of its own (the [`Numbers`] keep both),
//! and a field reads from whichever of its places reads.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use image::RgbaImage;
use serde::{Deserialize, Serialize};
use syrup::geometry::{NormRect, Rect};
use syrup::glyphs::{GlyphOptions, GlyphSet, LearnError};
use syrup::threshold::text_evidence;

use crate::vision::hud_text::{parse_current_max, parse_percent};

/// A value the HUD prints beside a bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Field {
    Hp,
    Mp,
    Exp,
}

impl Field {
    pub const ALL: [Field; 3] = [Field::Hp, Field::Mp, Field::Exp];

    pub fn label(self) -> &'static str {
        match self {
            Field::Hp => "HP",
            Field::Mp => "MP",
            Field::Exp => "EXP",
        }
    }
}

/// Where a field's text line sits relative to its bar: above it (the
/// classic HUD), on it, or somewhere in the band around it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Line {
    Above,
    Over,
    Around,
}

impl Line {
    const ALL: [Line; 3] = [Line::Above, Line::Over, Line::Around];

    /// The text region for a bar whose fill is `band`, in a frame of
    /// `width`×`height`.
    pub fn region(self, band: &NormRect, width: u32, height: u32) -> Rect {
        let (x, y, w, h) = band.pixels(width, height);
        let x0 = x.saturating_sub(6);
        let x1 = (x + w + 6).min(width);
        let (y0, y1) = match self {
            Line::Above => (y.saturating_sub(16), y),
            Line::Over => (y.saturating_sub(2), (y + h + 2).min(height)),
            Line::Around => (y.saturating_sub(16), (y + h + 8).min(height)),
        };
        Rect {
            x: x0,
            y: y0,
            w: x1.saturating_sub(x0),
            h: y1.saturating_sub(y0),
        }
    }
}

/// The classic HUD's text line: above the bar, reaching up `reach` bar
/// heights, and starting `from` across, as a share of the bar's width from
/// its left end (just before it, `-6 px / width`, when nothing in front is
/// left out).
///
/// Classic World prints `HP[178/178]` above its bars in a thin font: at 4K
/// the digits stand 22 rows tall and end 11 rows above the bar, the field's
/// name 30 rows tall beside them, out of reach of [`Line::Above`]'s 16 rows
/// (which hold the digits' feet), while on and around the bar its nine
/// tick marks and two ends split into eleven "glyphs". Measured in bar
/// heights, the line is reached at any window size; and starting past the
/// field's name ("EXP." in front of the number) the line reads as its
/// number alone.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Window {
    pub reach: f32,
    pub from: f32,
}

impl Window {
    /// The text region for a bar whose fill is `band`, in a frame of
    /// `width`×`height`: from `from` across to just past the bar's right
    /// end, and from `reach` bar heights (16 rows at the least) above the
    /// bar down to it.
    pub fn region(&self, band: &NormRect, width: u32, height: u32) -> Rect {
        let (x, y, w, h) = band.pixels(width, height);
        let up = ((h as f32 * self.reach).round() as u32).max(16).min(y);
        let right = (x + w + 6).min(width);
        let left = ((x as f32 + self.from * w as f32).round().max(0.0) as u32).min(right);
        Rect {
            x: left,
            y: y - up,
            w: right - left,
            h: up,
        }
    }

    /// The window over the whole line above `band` (nothing left out).
    fn whole(band: &NormRect, width: u32, height: u32) -> Window {
        let (x, _, _, _) = band.pixels(width, height);
        Window::starting(x.saturating_sub(6), band, width, height)
    }

    /// The window starting at column `left` of the frame.
    fn starting(left: u32, band: &NormRect, width: u32, height: u32) -> Window {
        let (x, _, w, _) = band.pixels(width, height);
        Window {
            reach: CLASSIC_REACH,
            from: (left as f32 - x as f32) / w.max(1) as f32,
        }
    }
}

/// Where a field's line is read: a [`Line`] on or by the bar, in the font,
/// or the classic HUD's [`Window`] above it, in the classic font.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Place {
    Line(Line),
    Window(Window),
}

impl Place {
    fn region(&self, band: &NormRect, width: u32, height: u32) -> Rect {
        match self {
            Place::Line(line) => line.region(band, width, height),
            Place::Window(window) => window.region(band, width, height),
        }
    }

    fn classic(&self) -> bool {
        matches!(self, Place::Window(_))
    }
}

/// A labelled example the font was learned from, as kept on disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sample {
    pub field: Field,
    /// Exactly what the line says, without spaces.
    pub text: String,
    /// The crop's file name, under the learned folder.
    pub picture: String,
    /// For a classic example, [`Line::Above`]: what a MapleSyrup that does
    /// not know windows takes it for.
    pub line: Line,
    /// "ocr" or "model".
    pub from: String,
    pub when: String,
    /// The classic HUD's line it was learned from, when it was: the example
    /// belongs to the classic font.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window: Option<Window>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Saved {
    samples: Vec<Sample>,
    /// The classic HUD's examples, kept apart: a MapleSyrup from before
    /// there was a classic font knows `samples` only, and would learn them
    /// into its one font, where the classic digits (22 rows tall at 4K,
    /// the same size as the modern ones' 18 to the glyph reader) would be
    /// averaged with the modern ones and read as neither.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    classic: Vec<Sample>,
}

/// A number read from the HUD.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Amount { current: u64, max: u64 },
    Percent(f32),
}

impl Value {
    pub fn percent(&self) -> f32 {
        match self {
            Value::Amount { current, max } => {
                (*current as f32 / (*max).max(1) as f32 * 100.0).clamp(0.0, 100.0)
            }
            Value::Percent(p) => *p,
        }
    }
}

/// One field read on one frame.
#[derive(Debug, Clone, PartialEq)]
pub struct Read {
    pub value: Value,
    /// The line as read, for the log.
    pub text: String,
}

/// How a field is doing.
#[derive(Debug, Default)]
struct FieldState {
    /// Frames in a row the glyphs could not read it (while the bar could be
    /// seen, so the line was presumably there).
    unread: u32,
    /// Times the glyphs were asked, for the tests.
    #[cfg(test)]
    tries: u32,
    last_sample: Option<Instant>,
    /// Times a labeller tried since the last example was learned.
    attempts: u32,
    /// Frames in a row the number and the bar disagreed.
    disagreements: u32,
}

/// Examples kept per field; the oldest goes when a new one comes.
const MAX_SAMPLES: usize = 8;
/// After this many frames in a row unread, a line is tried every
/// `UNREAD_EVERY` frames rather than every frame: a line that cannot be
/// read (covered, blurred, not where it was) costs the most to try, as the
/// reader works hardest on ink it cannot make out. The first frame it
/// reads again puts it back on every frame.
const UNREAD_SLOWLY_AFTER: u32 = 10;
const UNREAD_EVERY: u32 = 5;
/// How often a field that cannot be read asks for a new example.
const SAMPLE_EVERY: Duration = Duration::from_secs(3);
/// How far a label's number may be from the bar's fill to be believed.
pub const LABEL_TOLERANCE: f32 = 12.0;
/// Frames of disagreement between the number and the bar before the sight
/// asks for a fresh look.
pub const DISAGREE_FOR: u32 = 30;

/// The HUD's font, learned, and what it reads.
pub struct Numbers {
    dir: PathBuf,
    font: GlyphSet,
    /// The classic HUD's font, apart from the modern one: its thin digits
    /// are as tall as the modern bold ones to the glyph reader, which would
    /// average the two into one template per character.
    classic: GlyphSet,
    /// Both fonts' examples; a classic one has its `window`.
    samples: Vec<Sample>,
    /// The line each field reads from, from the examples that worked.
    lines: HashMap<Field, Line>,
    /// The classic line each field reads from, likewise.
    windows: HashMap<Field, Window>,
    /// Fields last read (or learned) from their classic line, which is
    /// tried first for them: a player who switches between a character on
    /// the classic HUD and one on the modern HUD has each read on the first
    /// frame, and the line not shown is tried only when the one last read
    /// will not read.
    classic_first: HashSet<Field>,
    /// Whether the last number read was on the classic HUD's line (None:
    /// none read yet this run): the HUD's style, for the session's stats.
    read_classic: Option<bool>,
    state: HashMap<Field, FieldState>,
    /// Crops of lines that could not be learned, saved under
    /// `debug/` for a look: how many so far this run.
    failures_kept: u32,
}

/// How many lines that could not be learned are kept as pictures per run.
const FAILURES_KEPT: u32 = 6;

/// How surely a glyph must match a character the font knows. The game
/// draws its HUD font pixel for pixel the same every frame, so a known
/// character scores 0.99 or better; a character the font has not learned
/// yet scores up to 0.90 against the most alike one it has (a 9 read as a
/// 3, on the player's own frames), which the general default of 0.72
/// would have believed. Over the bar a wrong digit is worse than none:
/// none asks for an example and learns the character.
const MIN_SCORE: f32 = 0.95;

fn options() -> GlyphOptions {
    GlyphOptions {
        min_score: MIN_SCORE,
        ..GlyphOptions::default()
    }
}

/// How surely a glyph must match a character of the classic font. The game
/// stretches the classic HUD's pixels by a fraction (2.81 at 4K), and its 6,
/// 8 and 9 differ by one stroke: a digit the font has not learned matches
/// its look-alike at up to 0.954 (an 8 read as a 9, on the player's 4K
/// frame), past the [`MIN_SCORE`] that serves the bold modern font, while
/// every glyph of the font matches its own character at 0.986 or better.
const CLASSIC_MIN_SCORE: f32 = 0.975;
/// How far the best character of the classic font must lead the next. Its
/// 8 leads the 9 by 0.038 to 0.059, its 9 the 8 by 0.042 to 0.057, its 6
/// the 8 by 0.070: at the general 0.06, an 8 is never read once a 9 is
/// known, nor learned beside one (learning reads the example back as
/// surely as reading does). [`CLASSIC_MIN_SCORE`] is what keeps a digit
/// the font does not know from passing for its look-alike.
const CLASSIC_MIN_MARGIN: f32 = 0.02;
/// How far above the bar the classic line is looked for, in bar heights:
/// the field's name, the tallest of it, starts 1.22 bar heights up at 4K.
const CLASSIC_REACH: f32 = 1.5;
/// Runs of ink past the start of a classic window beyond the characters of
/// a spelling, for the window to be tried with it.
const CLASSIC_SPARE_RUNS: usize = 4;
/// Windows and spellings tried at most for one label on the classic line
/// (the player's lines learn within ten; a label that fits none is given up
/// on while the frame loop waits for the sight).
const CLASSIC_TRIES: usize = 32;
/// The classic HUD writes its numbers on the grey panel above a bar, the
/// modern HUD on a bar's fill: the line found above a modern MP bar is the
/// HP bar's own, its numbers on its pink fill, and learned as MP's it would
/// have MP read HP's numbers (a label that swaps the two passes the bar's
/// check when both are full). The median saturation of the text's rows is
/// 0.14 to 0.16 on the classic panel (the player's 4K frame), 0.71 on the
/// modern HP bar, 0.54 in the scenery above it: over this, the line above
/// the bar is not the classic HUD's.
const PANEL_SATURATION: f32 = 0.3;

fn classic_options() -> GlyphOptions {
    GlyphOptions {
        min_score: CLASSIC_MIN_SCORE,
        min_margin: CLASSIC_MIN_MARGIN,
        ..GlyphOptions::default()
    }
}

fn now_text() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M").to_string()
}

/// The label as the glyph reader wants it: the characters drawn, no spaces.
fn normalised(text: &str) -> String {
    text.chars().filter(|c| !c.is_whitespace()).collect()
}

/// The ways `label` (already normalised) might be printed on the line: as
/// given first, then without the field's own name in front, without the
/// brackets, with the thousands grouped by commas and without — every
/// combination, each once. The value in it is never changed.
fn spellings(field: Field, label: &str) -> Vec<String> {
    let prefix = field.label();
    let without_prefix = |s: &str| -> Option<String> {
        let upper = s.to_ascii_uppercase();
        upper
            .strip_prefix(prefix)
            .map(|rest| s[s.len() - rest.len()..].to_string())
    };
    // Brackets go only where dropping them does not run two numbers
    // together: "8,954,288[18.99%]" without its brackets is not a line
    // anyone prints.
    let without_brackets = |s: &str| -> Option<String> {
        let chars: Vec<char> = s.chars().collect();
        let joins = (1..chars.len().saturating_sub(1)).any(|i| {
            matches!(chars[i], '[' | ']' | '(' | ')')
                && chars[i - 1].is_ascii_digit()
                && chars[i + 1].is_ascii_digit()
        });
        (!joins).then(|| {
            chars
                .iter()
                .filter(|c| !matches!(c, '[' | ']' | '(' | ')'))
                .collect()
        })
    };
    let without_commas = |s: &str| -> String { s.chars().filter(|c| *c != ',').collect() };
    let with_commas = |s: &str| -> String {
        // Every run of four or more digits gets its thousands separators;
        // the digits after a decimal point are left alone.
        let mut out = String::new();
        let chars: Vec<char> = s.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            if chars[i].is_ascii_digit() && (i == 0 || chars[i - 1] != '.') {
                let start = i;
                while i < chars.len() && chars[i].is_ascii_digit() {
                    i += 1;
                }
                let run: String = chars[start..i].iter().collect();
                if run.len() >= 4 {
                    for (k, c) in run.chars().enumerate() {
                        if k > 0 && (run.len() - k).is_multiple_of(3) {
                            out.push(',');
                        }
                        out.push(c);
                    }
                } else {
                    out.push_str(&run);
                }
            } else {
                out.push(chars[i]);
                i += 1;
            }
        }
        out
    };
    let mut out: Vec<String> = vec![label.to_string()];
    let mut add = |s: String| {
        if !s.is_empty() && !out.contains(&s) {
            out.push(s);
        }
    };
    let bases: Vec<String> = [Some(label.to_string()), without_prefix(label)]
        .into_iter()
        .flatten()
        .collect();
    for base in &bases {
        for bracketed in [Some(base.clone()), without_brackets(base)]
            .into_iter()
            .flatten()
        {
            add(bracketed.clone());
            add(without_commas(&bracketed));
            add(with_commas(&without_commas(&bracketed)));
        }
    }
    out
}

/// The ways `label` (already normalised) might read on the classic HUD's
/// line: as on any line ([`spellings`]); then without its brackets, even
/// where that runs two numbers together — the classic HUD draws them
/// yellow-green, darker than its panel in the channel the glyph reader
/// measures, so to the reader they are not there, and "619[49.84%]" reads
/// as 619, a gap, 49.84% —; then with the field's name in front, where the
/// line shows it and the labeller left it out. Those without the field's
/// name go first, whatever the labeller said: the number alone is the
/// cheaper line to read (see [`Numbers::learn`]'s classic attempts).
///
/// The name is not only dressing. On the player's 4K frame MP's number
/// alone does not learn: at the digits' own height (22 rows) the glyph
/// reader's cell rounds a 1 drawn 7 pixels wide and one drawn 8 (the game
/// stretches its pixels by a fraction) to 5 columns and 6, and the two
/// match at 0.92, under [`CLASSIC_MIN_SCORE`]; with "MP" in the line, 30
/// rows tall, setting its height, both round to 4 and match at 0.998.
fn classic_spellings(field: Field, label: &str) -> Vec<String> {
    let mut out = spellings(field, label);
    let bare: Vec<String> = out
        .iter()
        .map(|s| {
            s.chars()
                .filter(|c| !matches!(c, '[' | ']' | '(' | ')'))
                .collect()
        })
        .collect();
    for s in bare {
        if !s.is_empty() && !out.contains(&s) {
            out.push(s);
        }
    }
    let named: Vec<String> = out
        .iter()
        .filter(|s| !s.to_ascii_uppercase().starts_with(field.label()))
        .map(|s| format!("{}{s}", field.label()))
        .collect();
    for s in named {
        if !out.contains(&s) {
            out.push(s);
        }
    }
    // Stable: each group keeps its order.
    out.sort_by_key(|s| s.to_ascii_uppercase().starts_with(field.label()));
    out
}

/// Note why `spelling` (of `label`) did not learn at `place`, when it is
/// more telling than what `why` holds: a line that split into as many
/// glyphs as the spelling has characters and still would not learn says
/// more than a count that did not fit.
fn note(why: &mut Option<(u8, String)>, e: &LearnError, spelling: &str, label: &str, place: &str) {
    let rank = match e {
        LearnError::DoesNotReadBack { .. } => 3,
        LearnError::Inconsistent { .. } => 2,
        LearnError::GlyphCountMismatch { .. } => 1,
        LearnError::NoText => 0,
    };
    if why.as_ref().is_none_or(|(r, _)| rank > *r) {
        let said = if spelling == label {
            String::new()
        } else {
            format!(" (as \"{spelling}\")")
        };
        *why = Some((rank, format!("{e}{said}, {place}")));
    }
}

/// Where the classic line's region may start, left to right, each with how
/// many runs of ink lie past it: its own left end, then the middle of every
/// gap between two runs of ink along the text's rows (as the glyph reader
/// takes them: the tallest run of rows holding ink), so whatever stands in
/// front of the number can be left out — the field's name, or the number
/// in front of a percent a labeller gave alone. `None` when the region
/// holds no text, or text written on something coloured rather than on
/// the panel (see [`PANEL_SATURATION`]).
fn cuts(frame: &RgbaImage, region: Rect) -> Option<Vec<(u32, usize)>> {
    let o = classic_options();
    let evidence = text_evidence(frame, region, o.channel, o.polarity, o.evidence_span);
    let (w, h) = evidence.dimensions();
    let ink = |x: u32, y: u32| evidence.get_pixel(x, y).0[0] >= o.ink_threshold;
    let rows: Vec<bool> = (0..h).map(|y| (0..w).any(|x| ink(x, y))).collect();
    let (top, bottom) = tallest_run(&rows)?;
    // The median saturation of the text's rows, in 0..=255.
    let mut saturation: Vec<u8> = (region.y + top..region.y + bottom)
        .flat_map(|y| (region.x..region.x + w).map(move |x| (x, y)))
        .map(|(x, y)| {
            let [r, g, b, _] = frame.get_pixel(x, y).0;
            let (max, min) = (r.max(g).max(b), r.min(g).min(b));
            if max == 0 {
                0
            } else {
                (u32::from(max - min) * 255 / u32::from(max)) as u8
            }
        })
        .collect();
    let middle = saturation.len() / 2;
    let median = *saturation.select_nth_unstable(middle).1;
    if f32::from(median) / 255.0 > PANEL_SATURATION {
        return None;
    }
    let mut runs: Vec<(u32, u32)> = Vec::new();
    let mut start = None;
    for x in 0..w {
        match ((top..bottom).any(|y| ink(x, y)), start) {
            (true, None) => start = Some(x),
            (false, Some(s)) => {
                runs.push((s, x));
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        runs.push((s, w));
    }
    let mut cuts = vec![(region.x, runs.len())];
    cuts.extend(
        runs.windows(2)
            .enumerate()
            .map(|(i, pair)| (region.x + (pair[0].1 + pair[1].0) / 2, runs.len() - i - 1)),
    );
    Some(cuts)
}

/// The tallest run of `true` in `rows`, a one-row gap allowed (the dot of
/// an i, the gap in a colon), as half-open `(start, end)`.
fn tallest_run(rows: &[bool]) -> Option<(u32, u32)> {
    let mut best: Option<(usize, usize)> = None;
    let mut y = 0;
    while y < rows.len() {
        if !rows[y] {
            y += 1;
            continue;
        }
        let start = y;
        let mut end = y + 1;
        while end < rows.len() && (rows[end] || (end + 1 < rows.len() && rows[end + 1])) {
            end += 1;
        }
        if best.is_none_or(|(s, e)| end - start > e - s) {
            best = Some((start, end));
        }
        y = end;
    }
    best.map(|(s, e)| (s as u32, e as u32))
}

/// The number a line says, if it says one that makes sense for `field`.
pub fn parse(field: Field, text: &str) -> Option<Value> {
    match field {
        Field::Hp | Field::Mp => match parse_current_max(text) {
            Some((current, Some(max))) if max > 0 && current <= max && max < 1_000_000_000 => {
                Some(Value::Amount { current, max })
            }
            _ => None,
        },
        Field::Exp => {
            if let Some(p) = parse_percent(text).filter(|p| (0.0..=100.0).contains(p)) {
                return Some(Value::Percent(p));
            }
            match parse_current_max(text) {
                Some((current, Some(max))) if max > 0 && current <= max => {
                    Some(Value::Amount { current, max })
                }
                _ => None,
            }
        }
    }
}

impl Numbers {
    /// The font learned before, from `dir` (the settings folder's
    /// `learned`): every kept example is learned again.
    pub fn load(dir: &Path) -> Numbers {
        let saved: Saved = std::fs::read_to_string(dir.join("font.json"))
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        let mut numbers = Numbers {
            dir: dir.to_path_buf(),
            font: GlyphSet::new(options()),
            classic: GlyphSet::new(classic_options()),
            samples: Vec::new(),
            lines: HashMap::new(),
            windows: HashMap::new(),
            classic_first: HashSet::new(),
            read_classic: None,
            state: HashMap::new(),
            failures_kept: 0,
        };
        for sample in saved.samples.into_iter().chain(saved.classic) {
            numbers.relearn(sample);
        }
        numbers
    }

    /// Learn a kept example again, from its picture, into its own font.
    fn relearn(&mut self, sample: Sample) {
        let Ok(picture) = image::open(self.dir.join(&sample.picture)) else {
            return;
        };
        let picture = picture.to_rgba8();
        let whole = Rect {
            x: 0,
            y: 0,
            w: picture.width(),
            h: picture.height(),
        };
        let font = match sample.window {
            Some(_) => &mut self.classic,
            None => &mut self.font,
        };
        if font.learn(&picture, whole, &sample.text).is_ok() {
            match sample.window {
                Some(window) => {
                    self.windows.entry(sample.field).or_insert(window);
                }
                None => {
                    self.lines.entry(sample.field).or_insert(sample.line);
                }
            }
            self.samples.push(sample);
        }
    }

    fn save(&self) {
        let _ = std::fs::create_dir_all(&self.dir);
        let (classic, samples) = self
            .samples
            .iter()
            .cloned()
            .partition(|s| s.window.is_some());
        let saved = Saved { samples, classic };
        if let Ok(t) = serde_json::to_string_pretty(&saved) {
            let _ = std::fs::write(self.dir.join("font.json"), t);
        }
    }

    /// How many characters the fonts know (each once, in either font).
    pub fn glyphs(&self) -> usize {
        let mut chars: Vec<char> = self.font.chars().chain(self.classic.chars()).collect();
        chars.sort_unstable();
        chars.dedup();
        chars.len()
    }

    pub fn samples(&self) -> &[Sample] {
        &self.samples
    }

    /// The HUD the numbers were last read on: the classic one (Classic
    /// World's, the numbers above the bars) or the modern one; None until
    /// a number has been read this run.
    pub fn classic(&self) -> Option<bool> {
        self.read_classic
    }

    /// Whether anything has been learned for `field`'s line to be read from.
    pub fn knows(&self, field: Field) -> bool {
        self.glyphs() > 0 && (self.lines.contains_key(&field) || self.windows.contains_key(&field))
    }

    /// The line `field` is read from: the one its examples came from, else
    /// the line the other fields use, else above the bar.
    fn line_for(&self, field: Field) -> Line {
        self.lines
            .get(&field)
            .or_else(|| self.lines.values().next())
            .copied()
            .unwrap_or(Line::Above)
    }

    /// Where `field` is read, in the order tried: its line (or the one the
    /// other fields use), and its classic line, if it has one (or the first
    /// field's that has one, if it has no line of its own); the classic
    /// line first when it was the last to read.
    fn places(&self, field: Field) -> Vec<Place> {
        let mut places = vec![Place::Line(self.line_for(field))];
        let window = self.windows.get(&field).copied().or_else(|| {
            (!self.lines.contains_key(&field))
                .then(|| Field::ALL.iter().find_map(|f| self.windows.get(f).copied()))
                .flatten()
        });
        if let Some(window) = window {
            places.push(Place::Window(window));
            if self.classic_first.contains(&field) {
                places.reverse();
            }
        }
        places
    }

    /// Read `field` beside the bar at `band`, if the glyphs are sure of
    /// every character and the line says a number that makes sense.
    pub fn read(&mut self, frame: &RgbaImage, field: Field, band: &NormRect) -> Option<Read> {
        if self.font.chars().next().is_none() && self.classic.chars().next().is_none() {
            return None;
        }
        let (fw, fh) = frame.dimensions();
        let places: Vec<(Place, Rect)> = self
            .places(field)
            .into_iter()
            .map(|place| (place, place.region(band, fw, fh)))
            .filter(|(_, region)| region.w >= 8 && region.h >= 6)
            .collect();
        if places.is_empty() {
            return None;
        }
        let state = self.state.entry(field).or_default();
        if state.unread >= UNREAD_SLOWLY_AFTER && !state.unread.is_multiple_of(UNREAD_EVERY) {
            state.unread = state.unread.saturating_add(1);
            return None;
        }
        #[cfg(test)]
        {
            state.tries += 1;
        }
        let mut read = None;
        for (place, region) in places {
            let font = if place.classic() {
                &self.classic
            } else {
                &self.font
            };
            if font.chars().next().is_none() {
                continue;
            }
            let reading = font.read(frame, region);
            read = reading.value.and_then(|r| {
                parse(field, &r.text).map(|value| Read {
                    value,
                    text: r.text,
                })
            });
            if read.is_some() {
                if place.classic() {
                    self.classic_first.insert(field);
                } else {
                    self.classic_first.remove(&field);
                }
                self.read_classic = Some(place.classic());
                break;
            }
        }
        let state = self.state.entry(field).or_default();
        match read {
            Some(_) => state.unread = 0,
            None => state.unread = state.unread.saturating_add(1),
        }
        read
    }

    /// Whether `field` would do with a new labelled example now: nothing
    /// learned for it yet, or the glyphs have not been able to read it, and
    /// not asked too recently.
    pub fn wants_sample(&self, field: Field, now: Instant) -> bool {
        if self.full(field) {
            return false;
        }
        let state = self.state.get(&field);
        let unread = state.is_none_or(|s| s.unread > 0);
        let recent = state
            .and_then(|s| s.last_sample)
            .is_some_and(|t| now.duration_since(t) < SAMPLE_EVERY);
        (!self.knows(field) || unread) && !recent
    }

    /// A labeller tried to read `field`'s line just now (whatever came of
    /// it): not again for a while.
    pub fn attempted(&mut self, field: Field, now: Instant) {
        let state = self.state.entry(field).or_default();
        state.last_sample = Some(now);
        state.attempts = state.attempts.saturating_add(1);
    }

    /// Whether `field` still wants an example after the OCR engine has had
    /// its tries: nothing learned for it, or the glyphs cannot read it, and
    /// the engine was asked at least twice without an example coming of it
    /// (or was never there to ask). The sight asks the model then.
    pub fn wants_label(&self, field: Field) -> bool {
        if self.full(field) {
            return false;
        }
        let state = self.state.get(&field);
        let unread = state.is_none_or(|s| s.unread > 0);
        let tried =
            state.is_none_or(|s| s.attempts >= 2 || !crate::vision::ocr::is_ocr_available());
        (!self.knows(field) || unread) && tried
    }

    /// Whether `field` keeps all the examples it may, in both fonts: one
    /// full of examples from the modern HUD still learns the classic one's
    /// line (a player with a character on each), and the other way round.
    fn full(&self, field: Field) -> bool {
        let kept = |classic: bool| {
            self.samples
                .iter()
                .filter(|s| s.field == field && s.window.is_some() == classic)
                .count()
        };
        kept(false) >= MAX_SAMPLES && kept(true) >= MAX_SAMPLES
    }

    /// The region a labeller should read `field`'s text from: the classic
    /// line when it is where the field was last read (or the only place it
    /// was learned), else the line known for it, or the band around the
    /// bar, which holds the line wherever it is.
    pub fn label_region(&self, field: Field, band: &NormRect, width: u32, height: u32) -> Rect {
        if let Some(window) = self
            .windows
            .get(&field)
            .filter(|_| self.classic_first.contains(&field) || !self.lines.contains_key(&field))
        {
            return window.region(band, width, height);
        }
        self.lines
            .get(&field)
            .copied()
            .unwrap_or(Line::Around)
            .region(band, width, height)
    }

    /// Whether `text`, as a label for `field`, says a number that agrees
    /// with the bar's fill (when the bar could be measured).
    pub fn believable(field: Field, text: &str, bar_percent: Option<f32>) -> Result<Value, String> {
        let value = parse(field, text)
            .ok_or_else(|| format!("{text:?} is not a {} value", field.label()))?;
        if let Some(bar) = bar_percent {
            let p = value.percent();
            if (p - bar).abs() > LABEL_TOLERANCE {
                return Err(format!("{text:?} says {p:.0}% but the bar shows {bar:.0}%"));
            }
        }
        Ok(value)
    }

    /// Learn `field`'s line from `frame`, where it reads exactly `text`
    /// (as the labeller saw it; spaces do not count). The line is looked
    /// for above the bar, on it and around it, and the first that splits
    /// into as many glyphs as the label has characters is learned from and
    /// remembered for `field`; failing those, the classic HUD's line above
    /// the bar (a [`Window`]), into the classic font — first, for a field
    /// last read there. Returns what was learned, for the log.
    pub fn learn(
        &mut self,
        frame: &RgbaImage,
        field: Field,
        band: &NormRect,
        text: &str,
        from: &str,
        now: Instant,
    ) -> Result<String, String> {
        let label = normalised(text);
        if label.is_empty() {
            return Err("an empty label".into());
        }
        self.state.entry(field).or_default().last_sample = Some(now);
        // Of all the ways tried (three lines, each spelling), the failure
        // reported is the most telling one: a line that split into as many
        // glyphs as a spelling has characters and still would not learn
        // says more than a spelling whose count did not fit — the last
        // tried used to be reported, and read "the label has 16 characters
        // but the region splits into 18" for a line the first spelling had
        // matched glyph for glyph. The places tried second (the classic
        // line, for most) add theirs when they got as far.
        let classic_first = self.classic_first.contains(&field);
        let mut whys: [Option<(u8, String)>; 2] = [None, None];
        for (classic, why) in [classic_first, !classic_first].into_iter().zip(&mut whys) {
            let learned = if classic {
                self.learn_classic(frame, field, band, &label, from, why)
            } else {
                self.learn_lines(frame, field, band, &label, from, why)
            };
            if let Some(line) = learned {
                return Ok(line);
            }
        }
        let [first, second] = whys;
        let why = match (first, second) {
            (Some((r1, w1)), Some((r2, w2))) if r2 >= r1 => format!("{w1}; {w2}"),
            (Some((_, w)), _) | (None, Some((_, w))) => w,
            (None, None) => "no line to learn from".into(),
        };
        let kept = self.keep_failure(frame, field, band, &label);
        Err(format!("\"{label}\" could not be learned: {why}{kept}"))
    }

    /// Learn `field`'s line on, by or around the bar, into the font: what
    /// was learned, or `None` with the most telling failure in `why`.
    fn learn_lines(
        &mut self,
        frame: &RgbaImage,
        field: Field,
        band: &NormRect,
        label: &str,
        from: &str,
        why: &mut Option<(u8, String)>,
    ) -> Option<String> {
        let (fw, fh) = frame.dimensions();
        let first = self.line_for(field);
        let mut lines = vec![first];
        lines.extend(Line::ALL.iter().copied().filter(|l| *l != first));
        // A labeller reads the value right and the dressing wrong: "HP
        // [6370/6370]" for a line that shows 6370 / 6370, or 8954288 for
        // 8,954,288. The line decides, among the ways the value could be
        // printed; the glyph reader keeps only what reads back.
        let labels = spellings(field, label);
        for line in lines {
            let region = line.region(band, fw, fh);
            if region.w < 8 || region.h < 6 {
                continue;
            }
            for spelling in &labels {
                match self.font.learn(frame, region, spelling) {
                    Ok(count) => {
                        let crop = image::imageops::crop_imm(
                            frame, region.x, region.y, region.w, region.h,
                        )
                        .to_image();
                        self.keep(field, spelling, crop, line, None, from);
                        self.lines.insert(field, line);
                        self.classic_first.remove(&field);
                        self.state.entry(field).or_default().attempts = 0;
                        let as_said = if spelling == label {
                            String::new()
                        } else {
                            format!(" (the {from} said \"{label}\")")
                        };
                        return Some(format!(
                            "learned {count} glyphs of \"{spelling}\"{as_said} ({:?} the {} bar, from the {from}); the font knows {} characters",
                            line,
                            field.label(),
                            self.glyphs()
                        ));
                    }
                    Err(e) => note(why, &e, spelling, label, &format!("{line:?} the bar")),
                }
            }
        }
        None
    }

    /// Learn `field`'s line as the classic HUD prints it, above the bar on
    /// the panel, into the classic font: each spelling in turn, in the
    /// window it was last learned from, then over the whole line above the
    /// bar, then with whatever stands in front of each gap in it left out —
    /// the first that learns (reading back as surely as the classic font
    /// reads) is kept. The spellings without the field's name go before
    /// those with it, in every window: the number alone, where it learns,
    /// is read in a fraction of a millisecond, while a name drawn in a
    /// bolder hand, its letters touching, is cut apart glyph by glyph on
    /// every frame (EXP's: 4 ms). What was learned, or `None` with the most
    /// telling failure in `why`.
    fn learn_classic(
        &mut self,
        frame: &RgbaImage,
        field: Field,
        band: &NormRect,
        label: &str,
        from: &str,
        why: &mut Option<(u8, String)>,
    ) -> Option<String> {
        let (fw, fh) = frame.dimensions();
        let whole = Window::whole(band, fw, fh).region(band, fw, fh);
        if whole.w < 8 || whole.h < 6 {
            return None;
        }
        let Some(cuts) = cuts(frame, whole) else {
            why.get_or_insert((
                0,
                "no text on the panel above the bar, where the classic HUD writes it".into(),
            ));
            return None;
        };
        // The window known for the field, then each cut, with the runs of
        // ink past it.
        let mut windows: Vec<(Window, Option<usize>)> = self
            .windows
            .get(&field)
            .map(|w| (*w, None))
            .into_iter()
            .collect();
        for (left, runs) in cuts {
            let window = Window::starting(left, band, fw, fh);
            if !windows.iter().any(|(w, _)| *w == window) {
                windows.push((window, Some(runs)));
            }
        }
        let labels = classic_spellings(field, label);
        let mut tries = 0;
        for spelling in &labels {
            let chars = spelling.chars().count();
            for &(window, runs) in &windows {
                // Far more runs of ink than the spelling has characters is
                // not its line (the scenery above a modern bar, or the
                // screen's whole width above the modern EXP bar): passed
                // over without the glyph reader, which would only cut it
                // up for nothing. A few more are allowed: a name drawn bold
                // is dropped as a block, a bar's corner is debris.
                if runs.is_some_and(|r| r > chars + CLASSIC_SPARE_RUNS) {
                    continue;
                }
                let region = window.region(band, fw, fh);
                if region.w < 8 || region.h < 6 {
                    continue;
                }
                if tries == CLASSIC_TRIES {
                    return None;
                }
                tries += 1;
                match self.classic.learn(frame, region, spelling) {
                    Ok(count) => {
                        let crop = image::imageops::crop_imm(
                            frame, region.x, region.y, region.w, region.h,
                        )
                        .to_image();
                        self.keep(field, spelling, crop, Line::Above, Some(window), from);
                        self.windows.insert(field, window);
                        self.classic_first.insert(field);
                        self.state.entry(field).or_default().attempts = 0;
                        let as_said = if spelling == label {
                            String::new()
                        } else {
                            format!(" (the {from} said \"{label}\")")
                        };
                        return Some(format!(
                            "learned {count} glyphs of \"{spelling}\"{as_said} (the line above the {} bar, in the classic HUD's font, from the {from}); the font knows {} characters",
                            field.label(),
                            self.glyphs()
                        ));
                    }
                    Err(e) => note(why, &e, spelling, label, "the line above the bar"),
                }
            }
        }
        None
    }

    /// A line that could not be learned, as a picture under `debug/` with
    /// the label in its name, a few per run: what the labeller said and
    /// what the pixels showed can then be compared.
    fn keep_failure(
        &mut self,
        frame: &RgbaImage,
        field: Field,
        band: &NormRect,
        label: &str,
    ) -> String {
        if self.failures_kept >= FAILURES_KEPT {
            return String::new();
        }
        let (fw, fh) = frame.dimensions();
        // Around the bar, and up to the classic line above it.
        let around = Line::Around.region(band, fw, fh);
        let top = Window::whole(band, fw, fh)
            .region(band, fw, fh)
            .y
            .min(around.y);
        let region = Rect {
            x: around.x,
            y: top,
            w: around.w,
            h: around.y + around.h - top,
        };
        if region.w < 8 || region.h < 6 {
            return String::new();
        }
        self.failures_kept += 1;
        let safe: String = label
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
            .collect();
        let name = format!(
            "debug/unlearned-{}-{}-{safe}.png",
            field.label().to_lowercase(),
            self.failures_kept
        );
        let crop =
            image::imageops::crop_imm(frame, region.x, region.y, region.w, region.h).to_image();
        let _ = std::fs::create_dir_all(self.dir.join("debug"));
        match crop.save(self.dir.join(&name)) {
            Ok(()) => format!("; the line is kept as learned/{name}"),
            Err(_) => String::new(),
        }
    }

    /// Keep an example on disk; with too many for a field in its font, the
    /// oldest of them goes and the fonts are rebuilt from the rest. A
    /// classic example's picture has a name of its own (`hp-classic-1.png`),
    /// which a MapleSyrup that does not know classic examples never gives
    /// one of its own.
    fn keep(
        &mut self,
        field: Field,
        label: &str,
        crop: RgbaImage,
        line: Line,
        window: Option<Window>,
        from: &str,
    ) {
        let folder = self.dir.join("font");
        let _ = std::fs::create_dir_all(&folder);
        let classic = window.is_some();
        let same = |s: &Sample| s.field == field && s.window.is_some() == classic;
        let mut n = self.samples.iter().filter(|s| same(s)).count() + 1;
        let name = |n: usize| {
            let field = field.label().to_lowercase();
            if classic {
                format!("font/{field}-classic-{n}.png")
            } else {
                format!("font/{field}-{n}.png")
            }
        };
        while self.samples.iter().any(|s| s.picture == name(n)) {
            n += 1;
        }
        let picture = name(n);
        let _ = crop.save(self.dir.join(&picture));
        self.samples.push(Sample {
            field,
            text: label.to_string(),
            picture,
            line,
            from: from.to_string(),
            when: now_text(),
            window,
        });
        let kept = self.samples.iter().filter(|s| same(s)).count();
        if kept > MAX_SAMPLES
            && let Some(i) = self.samples.iter().position(same)
        {
            let old = self.samples.remove(i);
            let _ = std::fs::remove_file(self.dir.join(&old.picture));
            self.rebuild();
        }
        self.save();
    }

    /// The fonts from the kept examples alone.
    fn rebuild(&mut self) {
        let samples = std::mem::take(&mut self.samples);
        self.font = GlyphSet::new(options());
        self.classic = GlyphSet::new(classic_options());
        self.lines.clear();
        self.windows.clear();
        for sample in samples {
            self.relearn(sample);
        }
    }

    /// The number read for `field` against the bar's fill: how many frames
    /// in a row they have disagreed (0 when they agree or either is
    /// missing).
    pub fn cross_check(&mut self, field: Field, read: Option<f32>, bar: Option<f32>) -> u32 {
        let state = self.state.entry(field).or_default();
        match crate::vision::hud_ocr::agreement(read, bar) {
            crate::vision::hud_ocr::Agreement::Conflicting => {
                state.disagreements = state.disagreements.saturating_add(1);
            }
            _ => state.disagreements = 0,
        }
        state.disagreements
    }

    /// Forget everything: the examples, their pictures, the font.
    pub fn forget(&mut self) {
        for sample in &self.samples {
            let _ = std::fs::remove_file(self.dir.join(&sample.picture));
        }
        self.samples.clear();
        self.lines.clear();
        self.windows.clear();
        self.classic_first.clear();
        self.state.clear();
        self.font = GlyphSet::new(options());
        self.classic = GlyphSet::new(classic_options());
        self.save();
    }

    /// In a few words, for the conversation and the phone.
    pub fn describe(&self) -> String {
        if self.samples.is_empty() {
            return "the HUD's font is not learned yet".into();
        }
        format!(
            "the HUD's font: {} characters from {} example(s)",
            self.glyphs(),
            self.samples.len()
        )
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use image::Rgba;
    use syrup::draw;

    /// A status panel drawn the way the game draws its own: a dark panel,
    /// each bar with its line of text above it in a pixel font.
    pub(crate) fn hud(hp: (u64, u64), mp: (u64, u64), exp: f32) -> (RgbaImage, [NormRect; 3]) {
        let (fw, fh) = (1280u32, 720u32);
        let mut f = RgbaImage::from_pixel(fw, fh, Rgba([70, 100, 150, 255]));
        for y in 640..720 {
            for x in 0..fw {
                f.put_pixel(x, y, Rgba([44, 46, 52, 255]));
            }
        }
        let mut bands = Vec::new();
        let specs = [
            (
                100u32,
                format!("HP[{}/{}]", hp.0, hp.1),
                hp.0 as f32 / hp.1 as f32,
                [230u8, 40, 50],
            ),
            (
                420,
                format!("MP[{}/{}]", mp.0, mp.1),
                mp.0 as f32 / mp.1 as f32,
                [40, 110, 235],
            ),
            (700, format!("EXP[{exp:.2}%]"), exp / 100.0, [200, 220, 40]),
        ];
        for (x0, text, fill, rgb) in specs {
            let (y0, w, h) = (690u32, 260u32, 10u32);
            let end = x0 + (w as f32 * fill) as u32;
            for y in y0..y0 + h {
                for x in x0..x0 + w {
                    let p = if x < end {
                        Rgba([rgb[0], rgb[1], rgb[2], 255])
                    } else {
                        Rgba([90, 90, 96, 255])
                    };
                    f.put_pixel(x, y, p);
                }
            }
            // The line above the bar, in the 5x7 font at scale 1 (7 px tall).
            draw::draw_text(&text, x0 as i64 + 2, y0 as i64 - 11, 1, |x, y| {
                if x >= 0 && y >= 0 {
                    f.put_pixel(x as u32, y as u32, Rgba([250, 250, 250, 255]));
                }
            });
            bands.push(NormRect::from_pixels(x0, y0, w, h, fw, fh));
        }
        (f, [bands[0], bands[1], bands[2]])
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ms-numbers-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    /// The classic HUD of a player's 4K screen (see `tests/classic_hud.rs`)
    /// and its HP, MP and EXP bars.
    fn classic() -> (RgbaImage, [NormRect; 3]) {
        let strip = image::open(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/resources/hud-classic-4k-strip.png"
        ))
        .expect("the classic 4K strip fixture")
        .to_rgba8();
        let mut frame = RgbaImage::from_pixel(3840, 2160, Rgba([20, 20, 30, 255]));
        image::imageops::replace(&mut frame, &strip, 0, (2160 - strip.height()) as i64);
        let bands = [
            NormRect::new(0.36822918, 0.9777778, 0.4450521, 0.9925926),
            NormRect::new(0.44739583, 0.9777778, 0.5239583, 0.9930556),
            NormRect::new(0.53020835, 0.9777778, 0.6132866, 0.9930556),
        ];
        (frame, bands)
    }

    #[test]
    fn the_classic_line_is_reached_at_any_size_of_the_screen() {
        let (_, bands) = classic();
        let hp = bands[0];
        // At 4K the HP bar is 295 × 32 at (1414, 2112), and its line stands
        // 39 to 9 rows above it: the window reaches 48 up, from just before
        // the bar to just past it.
        let whole = Window::whole(&hp, 3840, 2160);
        assert_eq!(
            whole.region(&hp, 3840, 2160),
            Rect {
                x: 1408,
                y: 2064,
                w: 307,
                h: 48
            }
        );
        // Line::Above holds 16 rows of it: the digits' feet.
        assert_eq!(Line::Above.region(&hp, 3840, 2160).y, 2096);
        // On a screen half the size, half of everything (give or take a
        // pixel), and a window that leaves the field's name out keeps its
        // place on the line.
        let half = whole.region(&hp, 1920, 1080);
        assert!(
            half.x.abs_diff(704) <= 1 && half.y.abs_diff(1032) <= 1,
            "{half:?}"
        );
        assert!(
            half.h.abs_diff(24) <= 1 && half.w.abs_diff(154) <= 3,
            "{half:?}"
        );
        let past = Window::starting(1460, &hp, 3840, 2160);
        assert_eq!(past.region(&hp, 3840, 2160).x, 1460);
        assert!(past.region(&hp, 1920, 1080).x.abs_diff(730) <= 1);
    }

    #[test]
    fn font_json_with_classic_examples_is_read_by_an_older_maplesyrup_and_reads_its_files() {
        // A MapleSyrup from before the classic font read font.json as this
        // (its own types, as they were): it must still read the new file.
        #[derive(Deserialize)]
        #[allow(dead_code)]
        struct OldSample {
            field: Field,
            text: String,
            picture: String,
            line: Line,
            from: String,
            when: String,
        }
        #[derive(Deserialize)]
        struct OldSaved {
            samples: Vec<OldSample>,
        }
        let dir = temp_dir("compat");
        let mut numbers = Numbers::load(&dir);
        let now = Instant::now();
        let (frame, bands) = hud((400, 400), (1291, 1351), 37.51);
        numbers
            .learn(&frame, Field::Hp, &bands[0], "HP [400/400]", "ocr", now)
            .unwrap();
        let (classic, classic_bands) = classic();
        let learned = numbers
            .learn(
                &classic,
                Field::Mp,
                &classic_bands[1],
                "MP[101/101]",
                "model",
                now,
            )
            .unwrap();
        assert!(learned.contains("the line above the MP bar"), "{learned}");
        let text = std::fs::read_to_string(dir.join("font.json")).unwrap();
        let old: OldSaved = serde_json::from_str(&text).expect("an older MapleSyrup reads it");
        // It sees the modern example only: the classic one, learned into
        // its one font, would be averaged with the modern digits.
        assert_eq!(old.samples.len(), 1, "{text}");
        assert_eq!(old.samples[0].text, "HP[400/400]");
        assert!(
            text.contains("\"classic\"") && text.contains("mp-classic-1.png"),
            "{text}"
        );
        assert!(dir.join("font/mp-classic-1.png").exists());
        // Both kept between runs, each in its font.
        let mut again = Numbers::load(&dir);
        assert_eq!(again.samples().len(), 2);
        assert!(again.read(&frame, Field::Hp, &bands[0]).is_some());
        let mp = again.read(&classic, Field::Mp, &classic_bands[1]);
        assert_eq!(
            mp.map(|r| r.value),
            Some(Value::Amount {
                current: 101,
                max: 101
            })
        );
        // And a font.json an older MapleSyrup wrote (no classic examples,
        // no windows) is read as it always was.
        let older = r#"{"samples":[{"field":"hp","text":"HP[400/400]","picture":"font/hp-1.png","line":"above","from":"ocr","when":"2026-10-03 21:00"}]}"#;
        std::fs::write(dir.join("font.json"), older).unwrap();
        let mut older = Numbers::load(&dir);
        assert_eq!(older.samples().len(), 1);
        assert!(older.samples()[0].window.is_none());
        assert_eq!(older.glyphs(), 7, "H, P, the brackets, the slash, 4 and 0");
        assert!(older.read(&frame, Field::Hp, &bands[0]).is_some());
        assert!(older.read(&classic, Field::Mp, &classic_bands[1]).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_line_that_will_not_learn_says_the_most_telling_reason_and_keeps_its_picture() {
        let dir = temp_dir("unlearned");
        let mut numbers = Numbers::load(&dir);
        let now = Instant::now();
        let (frame, bands) = hud((240, 400), (1291, 1351), 37.51);
        // The line shows HP[240/400]; the labeller says HP[240/440]. As
        // given, the glyph count fits and the two 4s do not look alike;
        // the other spellings do not even fit the count. The reason
        // reported is the first, not the last tried.
        let why = numbers
            .learn(&frame, Field::Hp, &bands[0], "HP [240/440]", "model", now)
            .unwrap_err();
        assert!(why.contains("look nothing alike"), "{why}");
        assert!(!why.contains("splits into"), "{why}");
        // The line is kept as a picture, named after the label.
        let kept = dir.join("debug/unlearned-hp-1-HP_240_440_.png");
        assert!(why.contains("unlearned-hp-1-HP_240_440_.png"), "{why}");
        assert!(kept.exists());
        // A label no spelling of which fits: the count mismatch it is.
        let why = numbers
            .learn(&frame, Field::Hp, &bands[0], "2400/400", "model", now)
            .unwrap_err();
        assert!(why.contains("splits into"), "{why}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_font_is_learned_from_labels_read_every_frame_and_kept() {
        let dir = temp_dir("learn");
        let mut numbers = Numbers::load(&dir);
        let now = Instant::now();
        let (frame, bands) = hud((400, 400), (1291, 1351), 37.51);
        assert!(numbers.read(&frame, Field::Hp, &bands[0]).is_none());
        assert!(numbers.wants_sample(Field::Hp, now));
        // A label that disagrees with the bar is not believed.
        assert!(Numbers::believable(Field::Hp, "HP [100/400]", Some(100.0)).is_err());
        assert!(Numbers::believable(Field::Hp, "HP [400/400]", Some(100.0)).is_ok());
        let line = numbers
            .learn(&frame, Field::Hp, &bands[0], "HP [400/400]", "ocr", now)
            .unwrap();
        assert!(line.contains("learned 11 glyphs"), "{line}");
        assert!(!numbers.wants_sample(Field::Hp, now), "just asked");
        // Only 0 and 4 are known: the MP line has other digits.
        assert!(numbers.read(&frame, Field::Mp, &bands[1]).is_none());
        assert!(numbers.wants_sample(Field::Mp, now + SAMPLE_EVERY));
        numbers
            .learn(&frame, Field::Mp, &bands[1], "MP[1291/1351]", "model", now)
            .unwrap();
        numbers
            .learn(&frame, Field::Exp, &bands[2], "EXP[37.51%]", "model", now)
            .unwrap();
        // Now every line reads, on another frame with other numbers.
        let (other, bands) = hud((315, 400), (1000, 1351), 40.01);
        let hp = numbers.read(&other, Field::Hp, &bands[0]).expect("HP read");
        assert_eq!(
            hp.value,
            Value::Amount {
                current: 315,
                max: 400
            }
        );
        assert_eq!(hp.text, "HP[315/400]");
        let mp = numbers.read(&other, Field::Mp, &bands[1]).expect("MP read");
        assert_eq!(
            mp.value,
            Value::Amount {
                current: 1000,
                max: 1351
            }
        );
        let exp = numbers
            .read(&other, Field::Exp, &bands[2])
            .expect("EXP read");
        assert_eq!(exp.value, Value::Percent(40.01));
        assert!(!numbers.wants_sample(Field::Hp, now + SAMPLE_EVERY));
        // A digit never seen (6, 7, 8, 9 are known; 2 is) — 8 is not.
        let (unknown, bands) = hud((88, 400), (1000, 1351), 40.01);
        assert!(numbers.read(&unknown, Field::Hp, &bands[0]).is_none());
        // Kept between runs, pictures and all.
        let again = Numbers::load(&dir);
        assert_eq!(again.samples().len(), 3);
        assert_eq!(again.glyphs(), numbers.glyphs());
        let mut again = again;
        let hp = again
            .read(&other, Field::Hp, &bands[0])
            .expect("HP read after reload");
        assert_eq!(
            hp.value,
            Value::Amount {
                current: 315,
                max: 400
            }
        );
        assert!(again.describe().contains("3 example(s)"));
        again.forget();
        assert_eq!(again.glyphs(), 0);
        assert!(!dir.join("font/hp-1.png").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_character_the_font_has_not_learned_is_not_read_as_the_most_alike_one() {
        let dir = temp_dir("alike");
        let mut numbers = Numbers::load(&dir);
        let now = Instant::now();
        // Every digit but 0 is learned.
        let (frame, bands) = hud((1234, 5678), (9, 9), 37.51);
        numbers
            .learn(&frame, Field::Hp, &bands[0], "HP[1234/5678]", "ocr", now)
            .unwrap();
        numbers
            .learn(&frame, Field::Mp, &bands[1], "MP[9/9]", "ocr", now)
            .unwrap();
        assert_eq!(
            numbers.glyphs(),
            15,
            "H, P, M, the brackets, the slash, nine digits"
        );
        // In this font a 0 is most like an 8, alike enough for a general
        // reader to believe; over the HUD it must stay unread instead, so an
        // example gets asked for and the 0 learned.
        let (frame, bands) = hud((1000, 5678), (9, 9), 37.51);
        assert!(numbers.read(&frame, Field::Hp, &bands[0]).is_none());
        assert!(numbers.wants_sample(Field::Hp, now + SAMPLE_EVERY));
        numbers
            .learn(&frame, Field::Hp, &bands[0], "HP[1000/5678]", "ocr", now)
            .unwrap();
        let read = numbers
            .read(&frame, Field::Hp, &bands[0])
            .expect("read once learned");
        assert_eq!(read.text, "HP[1000/5678]");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_line_that_cannot_be_read_is_tried_less_often_until_it_reads() {
        let dir = temp_dir("slowly");
        let mut numbers = Numbers::load(&dir);
        let now = Instant::now();
        let (frame, bands) = hud((400, 400), (1291, 1351), 37.51);
        numbers
            .learn(&frame, Field::Hp, &bands[0], "HP [400/400]", "ocr", now)
            .unwrap();
        // Digits the font does not know: unreadable, frame after frame.
        let (other, bands) = hud((315, 400), (1291, 1351), 37.51);
        let tries = |n: &Numbers| n.state.get(&Field::Hp).map_or(0, |s| s.tries);
        let before = tries(&numbers);
        for _ in 0..UNREAD_SLOWLY_AFTER {
            assert!(numbers.read(&other, Field::Hp, &bands[0]).is_none());
        }
        assert_eq!(
            tries(&numbers) - before,
            UNREAD_SLOWLY_AFTER,
            "every frame at first"
        );
        let before = tries(&numbers);
        for _ in 0..UNREAD_EVERY * 4 {
            assert!(numbers.read(&other, Field::Hp, &bands[0]).is_none());
        }
        assert_eq!(
            tries(&numbers) - before,
            4,
            "then one frame in {UNREAD_EVERY}"
        );
        // The line reads again: back to every frame from the next one.
        let (readable, bands) = hud((400, 400), (1291, 1351), 37.51);
        let mut read = None;
        for _ in 0..UNREAD_EVERY {
            read = numbers.read(&readable, Field::Hp, &bands[0]);
            if read.is_some() {
                break;
            }
        }
        assert!(read.is_some(), "read within {UNREAD_EVERY} frames");
        let before = tries(&numbers);
        for _ in 0..3 {
            assert!(numbers.read(&readable, Field::Hp, &bands[0]).is_some());
        }
        assert_eq!(tries(&numbers) - before, 3);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_wrong_label_is_refused_and_disagreement_is_counted() {
        let dir = temp_dir("refuse");
        let mut numbers = Numbers::load(&dir);
        let now = Instant::now();
        let (frame, bands) = hud((400, 400), (1291, 1351), 37.51);
        // Eleven glyphs are drawn; a label of another length cannot be them.
        let err = numbers
            .learn(&frame, Field::Hp, &bands[0], "HP [40/400]", "ocr", now)
            .unwrap_err();
        assert!(err.contains("could not be learned"), "{err}");
        assert_eq!(numbers.glyphs(), 0);
        // Numbers against the bar: a streak of conflict, reset by agreement.
        for _ in 0..3 {
            numbers.cross_check(Field::Hp, Some(100.0), Some(60.0));
        }
        assert_eq!(numbers.cross_check(Field::Hp, Some(100.0), Some(60.0)), 4);
        assert_eq!(numbers.cross_check(Field::Hp, Some(100.0), Some(97.0)), 0);
        assert_eq!(numbers.cross_check(Field::Hp, None, Some(97.0)), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_label_is_tried_in_every_spelling_the_line_might_use() {
        let hp = spellings(Field::Hp, "HP[6370/6370]");
        assert_eq!(hp[0], "HP[6370/6370]", "as said, first");
        for want in ["[6370/6370]", "6370/6370", "6,370/6,370", "HP6,370/6,370"] {
            assert!(hp.iter().any(|s| s == want), "{want} in {hp:?}");
        }
        let exp = spellings(Field::Exp, "EXP8954288[18.99%]");
        for want in [
            "8,954,288[18.99%]",
            "8954288[18.99%]",
            "EXP8,954,288[18.99%]",
        ] {
            assert!(exp.iter().any(|s| s == want), "{want} in {exp:?}");
        }
        // Not without its brackets: that would run the two numbers together.
        assert!(exp.iter().all(|s| s.contains('[')), "{exp:?}");
        assert!(
            exp.iter()
                .all(|s| !s.contains("18,.99") && !s.contains("18.,99")),
            "{exp:?}"
        );
        let mut sorted = hp.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), hp.len(), "each once");
    }

    #[test]
    fn values_parse_the_lines_the_game_prints() {
        assert_eq!(
            parse(Field::Hp, "HP[400/400]"),
            Some(Value::Amount {
                current: 400,
                max: 400
            })
        );
        assert_eq!(
            parse(Field::Mp, "MP [1,291 / 1,351]"),
            Some(Value::Amount {
                current: 1291,
                max: 1351
            })
        );
        assert_eq!(
            parse(Field::Exp, "EXP35900[37.51%]"),
            Some(Value::Percent(37.51))
        );
        assert_eq!(
            parse(Field::Hp, "HP[500/400]"),
            None,
            "more than the maximum"
        );
        assert_eq!(parse(Field::Hp, "HP 400"), None, "no maximum");
        assert!((Value::Amount { current: 1, max: 3 }.percent() - 33.33).abs() < 0.01);
        assert_eq!(normalised(" HP [400 / 400] "), "HP[400/400]");
    }
}
