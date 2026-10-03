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

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use image::RgbaImage;
use serde::{Deserialize, Serialize};
use syrup::geometry::{NormRect, Rect};
use syrup::glyphs::{GlyphOptions, GlyphSet};

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

/// A labelled example the font was learned from, as kept on disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sample {
    pub field: Field,
    /// Exactly what the line says, without spaces.
    pub text: String,
    /// The crop's file name, under the learned folder.
    pub picture: String,
    pub line: Line,
    /// "ocr" or "model".
    pub from: String,
    pub when: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Saved {
    samples: Vec<Sample>,
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
    last_sample: Option<Instant>,
    /// Times a labeller tried since the last example was learned.
    attempts: u32,
    /// Frames in a row the number and the bar disagreed.
    disagreements: u32,
}

/// Examples kept per field; the oldest goes when a new one comes.
const MAX_SAMPLES: usize = 8;
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
    samples: Vec<Sample>,
    /// The line each field reads from, from the examples that worked.
    lines: HashMap<Field, Line>,
    state: HashMap<Field, FieldState>,
}

fn options() -> GlyphOptions {
    GlyphOptions::default()
}

fn now_text() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M").to_string()
}

/// The label as the glyph reader wants it: the characters drawn, no spaces.
fn normalised(text: &str) -> String {
    text.chars().filter(|c| !c.is_whitespace()).collect()
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
            samples: Vec::new(),
            lines: HashMap::new(),
            state: HashMap::new(),
        };
        for sample in saved.samples {
            numbers.relearn(sample);
        }
        numbers
    }

    /// Learn a kept example again, from its picture.
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
        if self.font.learn(&picture, whole, &sample.text).is_ok() {
            self.lines.entry(sample.field).or_insert(sample.line);
            self.samples.push(sample);
        }
    }

    fn save(&self) {
        let _ = std::fs::create_dir_all(&self.dir);
        let saved = Saved {
            samples: self.samples.clone(),
        };
        if let Ok(t) = serde_json::to_string_pretty(&saved) {
            let _ = std::fs::write(self.dir.join("font.json"), t);
        }
    }

    /// How many characters the font knows.
    pub fn glyphs(&self) -> usize {
        self.font.chars().count()
    }

    pub fn samples(&self) -> &[Sample] {
        &self.samples
    }

    /// Whether anything has been learned for `field`'s line to be read from.
    pub fn knows(&self, field: Field) -> bool {
        self.glyphs() > 0 && self.lines.contains_key(&field)
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

    /// Read `field` beside the bar at `band`, if the glyphs are sure of
    /// every character and the line says a number that makes sense.
    pub fn read(&mut self, frame: &RgbaImage, field: Field, band: &NormRect) -> Option<Read> {
        if self.glyphs() == 0 {
            return None;
        }
        let (fw, fh) = frame.dimensions();
        let region = self.line_for(field).region(band, fw, fh);
        if region.w < 8 || region.h < 6 {
            return None;
        }
        let reading = self.font.read(frame, region);
        let state = self.state.entry(field).or_default();
        let read = reading.value.and_then(|r| {
            parse(field, &r.text).map(|value| Read {
                value,
                text: r.text,
            })
        });
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
        let kept = self.samples.iter().filter(|s| s.field == field).count();
        if kept >= MAX_SAMPLES {
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
        let kept = self.samples.iter().filter(|s| s.field == field).count();
        if kept >= MAX_SAMPLES {
            return false;
        }
        let state = self.state.get(&field);
        let unread = state.is_none_or(|s| s.unread > 0);
        let tried =
            state.is_none_or(|s| s.attempts >= 2 || !crate::vision::ocr::is_ocr_available());
        (!self.knows(field) || unread) && tried
    }

    /// The region a labeller should read `field`'s text from: the line
    /// known for it, or the band around the bar, which holds the line
    /// wherever it is.
    pub fn label_region(&self, field: Field, band: &NormRect, width: u32, height: u32) -> Rect {
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
    /// remembered for `field`. Returns what was learned, for the log.
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
        let (fw, fh) = frame.dimensions();
        let first = self.line_for(field);
        let mut lines = vec![first];
        lines.extend(Line::ALL.iter().copied().filter(|l| *l != first));
        let mut why = String::new();
        for line in lines {
            let region = line.region(band, fw, fh);
            if region.w < 8 || region.h < 6 {
                continue;
            }
            match self.font.learn(frame, region, &label) {
                Ok(count) => {
                    let crop =
                        image::imageops::crop_imm(frame, region.x, region.y, region.w, region.h)
                            .to_image();
                    self.keep(field, &label, crop, line, from);
                    self.lines.insert(field, line);
                    self.state.entry(field).or_default().attempts = 0;
                    return Ok(format!(
                        "learned {count} glyphs of \"{label}\" ({:?} the {} bar, from the {from}); the font knows {} characters",
                        line,
                        field.label(),
                        self.glyphs()
                    ));
                }
                Err(e) => why = e.to_string(),
            }
        }
        Err(format!("\"{label}\" could not be learned: {why}"))
    }

    /// Keep an example on disk; with too many for a field, the oldest goes
    /// and the font is rebuilt from the rest.
    fn keep(&mut self, field: Field, label: &str, crop: RgbaImage, line: Line, from: &str) {
        let folder = self.dir.join("font");
        let _ = std::fs::create_dir_all(&folder);
        let mut n = self.samples.iter().filter(|s| s.field == field).count() + 1;
        let name = |n: usize| format!("font/{}-{n}.png", field.label().to_lowercase());
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
        });
        let kept = self.samples.iter().filter(|s| s.field == field).count();
        if kept > MAX_SAMPLES
            && let Some(i) = self.samples.iter().position(|s| s.field == field)
        {
            let old = self.samples.remove(i);
            let _ = std::fs::remove_file(self.dir.join(&old.picture));
            self.rebuild();
        }
        self.save();
    }

    /// The font from the kept examples alone.
    fn rebuild(&mut self) {
        let samples = std::mem::take(&mut self.samples);
        self.font = GlyphSet::new(options());
        self.lines.clear();
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
        self.state.clear();
        self.font = GlyphSet::new(options());
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
