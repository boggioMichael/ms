//! Reading the HUD's *text* — `current/max` numbers, name, job, level.
//!
//! Bar geometry answers "how full", but only the printed numbers answer
//! "how much", and MapleStory prints them right next to each bar. Those
//! numbers come from OCR, which is a subprocess call costing tens of
//! milliseconds per region — running it on every frame would drop the
//! pipeline from ~40 FPS to a few.
//!
//! So OCR runs on a cadence rather than per frame, and results are cached
//! and re-served in between. That is sound for this data: HP and MP change
//! continuously, but the *maximum*, the character's name, job and level are
//! effectively static, and the current values are still tracked every frame
//! by the bar geometry. In other words the cheap signal stays real-time and
//! the expensive signal refreshes often enough to stay correct.
//!
//! ```text
//!   frame 0   OCR  -> name/job/level/max cached
//!   frame 1..N     -> bars measured per frame, text served from cache
//!   frame N   OCR  -> cache refreshed
//! ```

use image::RgbaImage;

use crate::vision::geometry::Rect;
use crate::vision::hud_ocr::{HudField, HudOcrResult, ParsedValue, ReadState};
use crate::vision::ocr;
use crate::vision::quality::assess_text_quality;

/// Bounds on a region that could plausibly hold one line of HUD text.
///
/// A region far outside these is a layout fault, not a recognition
/// problem: an 8-pixel-tall strip has no room for a glyph, and a
/// 62-pixel-tall one spans several stat rows at once, so whatever OCR
/// returns describes the wrong thing. Recognising them anyway produced
/// exactly that — a job read as "eo Pages os Te".
const MIN_TEXT_ROI_HEIGHT: u32 = 10;
const MAX_TEXT_ROI_HEIGHT: u32 = 48;
const MIN_TEXT_ROI_WIDTH: u32 = 24;

/// Why a region cannot hold a line of text, or `None` when it can.
fn roi_rejection(roi: Rect) -> Option<String> {
    if roi.h < MIN_TEXT_ROI_HEIGHT {
        return Some(format!("region only {}px tall; no room for text", roi.h));
    }
    if roi.h > MAX_TEXT_ROI_HEIGHT {
        return Some(format!("region {}px tall; spans several rows", roi.h));
    }
    if roi.w < MIN_TEXT_ROI_WIDTH {
        return Some(format!("region only {}px wide", roi.w));
    }
    None
}

/// Most whitespace-separated words a real stats row can hold: a label plus
/// a value of at most two words.
const MAX_PLATE_TOKENS: usize = 3;

/// How many frames pass between OCR refreshes by default.
///
/// At ~40 FPS this re-reads roughly every 1.5s, which keeps level-ups and
/// max-value changes current without putting OCR in the per-frame path.
pub const DEFAULT_OCR_INTERVAL: u64 = 60;

/// Text values scraped from the HUD, plus the numbers parsed out of them.
#[derive(Debug, Clone, Default)]
pub struct HudText {
    pub player_name: Option<String>,
    pub character_class: Option<String>,
    pub level: Option<String>,
    /// `(current, max)` per metric, when the HUD printed them.
    pub hp: Option<(u64, Option<u64>)>,
    pub mp: Option<(u64, Option<u64>)>,
    pub exp: Option<(u64, Option<u64>)>,
    /// Exact EXP percentage when the HUD printed one, e.g. `[37.51%]`.
    pub exp_percent: Option<f32>,
}

/// Parse `1,234 / 5,678` or `[1234/5678]` into its two numbers.
///
/// MapleStory groups digits with commas and wraps the pair in brackets, and
/// OCR frequently reads the slash as something else, so the parse is
/// deliberately forgiving about separators while staying strict about
/// digits.
pub fn parse_current_max(text: &str) -> Option<(u64, Option<u64>)> {
    // Digit grouping is dropped outright rather than turned into a space,
    // so "65,122" stays one number instead of becoming two.
    let cleaned: String = text
        .chars()
        .filter(|c| *c != ',')
        .map(|c| match c {
            '[' | ']' | '(' | ')' => ' ',
            // OCR commonly substitutes these for the separating slash.
            '\\' | '|' | 'l' | 'I' => '/',
            other => other,
        })
        .collect();

    let chars: Vec<char> = cleaned.chars().collect();

    // Every slash is tried, not just the first. OCR turns the label's own
    // characters into slashes too ("HP [400/400]" reads as "HIP | 400/408)",
    // whose first slash is the mangled label), so stopping at the first one
    // finds no digits and the pair is lost.
    for (index, character) in chars.iter().enumerate() {
        if *character != '/' {
            continue;
        }
        // The numbers may be spaced away from the slash ("400 / 400"), so
        // scan outward past whitespace to the digit run on each side.
        if let Some(current) = digit_run_before(&chars, index) {
            return Some((current, digit_run_after(&chars, index)));
        }
    }

    // No pair, but a lone number is still worth reporting. Take the first
    // maximal digit run rather than every digit in the token, so a
    // still-unsplit "400/408" cannot fuse into 400408.
    first_digit_run(&cleaned).map(|value| (value, None))
}

/// First maximal run of digits in `text`.
fn first_digit_run(text: &str) -> Option<u64> {
    let mut digits = String::new();
    for character in text.chars() {
        if character.is_ascii_digit() {
            digits.push(character);
        } else if !digits.is_empty() {
            break;
        }
    }
    digits.parse().ok()
}

/// Collect the digit run ending before `index`, skipping intervening spaces.
fn digit_run_before(chars: &[char], index: usize) -> Option<u64> {
    let mut end = index;
    while end > 0 && chars[end - 1].is_whitespace() {
        end -= 1;
    }
    let mut start = end;
    while start > 0 && chars[start - 1].is_ascii_digit() {
        start -= 1;
    }
    (start < end)
        .then(|| chars[start..end].iter().collect::<String>())?
        .parse()
        .ok()
}

/// Collect the digit run starting after `index`, skipping spaces.
fn digit_run_after(chars: &[char], index: usize) -> Option<u64> {
    let mut start = index + 1;
    while start < chars.len() && chars[start].is_whitespace() {
        start += 1;
    }
    let mut end = start;
    while end < chars.len() && chars[end].is_ascii_digit() {
        end += 1;
    }
    (start < end)
        .then(|| chars[start..end].iter().collect::<String>())?
        .parse()
        .ok()
}

/// Parse a trailing percentage such as `37.51%`.
pub fn parse_percent(text: &str) -> Option<f32> {
    let bytes: Vec<char> = text.chars().collect();
    let percent_at = bytes.iter().position(|&c| c == '%')?;
    let mut start = percent_at;
    while start > 0 {
        let candidate = bytes[start - 1];
        if candidate.is_ascii_digit() || candidate == '.' {
            start -= 1;
        } else {
            break;
        }
    }
    if start == percent_at {
        return None;
    }
    bytes[start..percent_at]
        .iter()
        .collect::<String>()
        .parse::<f32>()
        .ok()
        .filter(|value| (0.0..=100.0).contains(value))
}

/// Does this look like a character name rather than OCR noise?
///
/// OCR aimed at a mis-located plate happily returns a sentence from
/// whatever dialog was underneath. Reporting that as the player's name is
/// worse than reporting nothing, so values that cannot be a MapleStory name
/// are rejected: in-game names are 4-12 characters, alphanumeric, no spaces.
pub fn plausible_name(value: &str) -> bool {
    let trimmed = value.trim();
    let length = trimmed.chars().count();
    (4..=12).contains(&length)
        && trimmed.chars().all(|c| c.is_ascii_alphanumeric())
        && trimmed.chars().any(|c| c.is_ascii_alphabetic())
}

/// Does this look like a job/class name?
///
/// Job names are short alphabetic words, optionally two of them
/// ("Dawn Warrior"). A long clause is OCR spill from a nearby dialog.
pub fn plausible_job(value: &str) -> bool {
    let trimmed = value.trim();
    let length = trimmed.chars().count();
    (3..=20).contains(&length)
        && trimmed.split_whitespace().count() <= 2
        && trimmed
            .chars()
            .all(|c| c.is_ascii_alphabetic() || c.is_whitespace())
}

/// Extract a level, rejecting anything outside the game's real range.
///
/// Returns the level as a string so callers keep the existing text-shaped
/// field, but only when it parses to a number a character can actually be.
pub fn plausible_level(value: &str) -> Option<String> {
    let digits: String = value
        .trim()
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    let digits = if digits.is_empty() {
        value
            .trim()
            .chars()
            .filter(|c| c.is_ascii_digit())
            .collect()
    } else {
        digits
    };
    let level: u32 = digits.parse().ok()?;
    // MapleStory caps at 300; 0 is not a level.
    (1..=300).contains(&level).then(|| level.to_string())
}

/// Choose a field's value out of an OCR'd plate row.
///
/// The row holds the field's label followed by its value, so the obvious
/// approach is to strip the label — but the label is small stylised text and
/// OCR mangles it constantly ("NAME" comes back as "HATE"), leaving the
/// label glued to the value and failing validation. Rather than trusting the
/// label to be readable, every candidate is tested and the first plausible
/// one wins: the stripped row, then individual tokens from the right, since
/// the value follows the label.
pub fn pick_value(raw: &str, label: &str, accept: impl Fn(&str) -> bool) -> Option<String> {
    let stripped = strip_label(raw, label);
    if accept(&stripped) {
        return Some(stripped);
    }

    let tokens: Vec<&str> = stripped.split_whitespace().collect();

    // A stats row is a label plus a short value. A row of many words is
    // text from something overlapping the panel, and picking the most
    // name-shaped word out of a sentence would confidently report
    // "advancement" as the player's name.
    if tokens.len() > MAX_PLATE_TOKENS {
        return None;
    }

    // Two-word values (job names like "Dawn Warrior") before single tokens.
    for window in tokens.windows(2).rev() {
        let joined = window.join(" ");
        if accept(&joined) {
            return Some(joined);
        }
    }
    for token in tokens.iter().rev() {
        if accept(token) {
            return Some((*token).to_string());
        }
    }
    None
}

/// Strip a leading `NAME:` style label from an OCR'd plate value.
pub fn strip_label(text: &str, label: &str) -> String {
    let trimmed = text.trim();
    let lowered = trimmed.to_ascii_lowercase();
    let label_lower = label.to_ascii_lowercase();
    let body = match lowered.find(&label_lower) {
        Some(index) => &trimmed[index + label.len()..],
        None => trimmed,
    };
    body.trim_start_matches(|c: char| c.is_whitespace() || matches!(c, ':' | '-' | '='))
        .trim()
        .to_string()
}

/// Everything one OCR pass produced: the parsed values the pipeline
/// consumes, and the per-field provenance the debugger displays.
#[derive(Debug, Clone, Default)]
pub struct HudTextReading {
    pub text: HudText,
    /// One entry per field whose ROI was located, whether or not it read.
    pub ocr: Vec<HudOcrResult>,
}

/// Reads HUD text on a cadence and caches it between reads.
#[derive(Debug)]
pub struct HudTextReader {
    interval: u64,
    frames_seen: u64,
    cached: HudText,
    /// Last usable reading per field, so a value can be carried across a
    /// frame that failed to read — labelled as carried, never as fresh.
    remembered: Vec<HudOcrResult>,
    /// Every field from the most recent OCR pass, including the ones that
    /// failed. Kept so a field that cannot be read still appears in the
    /// debugger between passes: a ROI that exists and fails must look
    /// different from a ROI that was never located at all.
    last_pass: Vec<HudOcrResult>,
    /// True once at least one OCR pass has completed, so callers can tell
    /// "not read yet" from "read and genuinely empty".
    primed: bool,
}

impl Default for HudTextReader {
    fn default() -> Self {
        Self::new(DEFAULT_OCR_INTERVAL)
    }
}

impl HudTextReader {
    pub fn new(interval: u64) -> Self {
        Self {
            interval: interval.max(1),
            frames_seen: 0,
            cached: HudText::default(),
            remembered: Vec::new(),
            last_pass: Vec::new(),
            primed: false,
        }
    }

    /// The last usable reading of `field`, if there is one.
    fn recall(&self, field: HudField) -> Option<&HudOcrResult> {
        self.remembered.iter().find(|entry| entry.field == field)
    }

    /// Record a usable reading so later frames can carry it forward.
    fn remember(&mut self, result: &HudOcrResult) {
        if !result.is_usable() {
            return;
        }
        self.remembered.retain(|entry| entry.field != result.field);
        self.remembered.push(result.clone());
    }

    /// Re-serve a remembered reading, relabelled so it cannot be mistaken
    /// for a fresh one.
    fn carry_forward(&self, field: HudField) -> Option<HudOcrResult> {
        let remembered = self.recall(field)?;
        let mut carried = remembered.clone();
        carried.state = ReadState::CarriedForward {
            from_frame: remembered.frame_id,
        };
        Some(carried)
    }

    /// Read one field: recognise, parse, validate, and fall back to a
    /// carried value only when this frame genuinely could not be read.
    fn read_field(
        &self,
        image: &RgbaImage,
        field: HudField,
        roi: Rect,
        frame_id: u64,
        parse: impl Fn(&str) -> ParsedValue,
    ) -> HudOcrResult {
        // Reject a malformed region before spending an OCR call on it.
        if let Some(reason) = roi_rejection(roi) {
            return HudOcrResult::rejected_roi(field, roi, frame_id, &reason);
        }
        let raw = read_region(image, roi);
        let parsed = raw.as_deref().map(&parse).unwrap_or(ParsedValue::NotRead);

        if parsed.is_usable() {
            let quality = Some(assess_text_quality(image, roi));
            return HudOcrResult {
                confidence: HudOcrResult::score(&parsed, quality),
                field,
                roi,
                raw_text: raw,
                parsed,
                frame_id,
                state: ReadState::ReadThisFrame,
                quality,
                note: None,
            };
        }

        // This frame did not read. A remembered value may still be shown,
        // but only labelled as carried forward.
        if let Some(carried) = self.carry_forward(field) {
            return carried;
        }

        let mut unread = HudOcrResult::unread(field, roi, frame_id, raw);
        unread.quality = Some(assess_text_quality(image, roi));
        unread
    }

    /// Whether the next [`Self::read`] will actually invoke OCR.
    pub fn will_refresh(&self) -> bool {
        !self.primed || self.frames_seen.is_multiple_of(self.interval)
    }

    pub fn is_primed(&self) -> bool {
        self.primed
    }

    /// Return HUD text, refreshing from OCR when the cadence calls for it.
    ///
    /// Between refreshes this is free: it hands back the cached values,
    /// every one of them labelled as carried forward.
    pub fn read(
        &mut self,
        image: &RgbaImage,
        markers: &super::hud_geometry::UiMarkers,
        frame_id: u64,
    ) -> HudTextReading {
        let refresh = self.will_refresh();
        self.frames_seen = self.frames_seen.wrapping_add(1);

        if !refresh || !ocr::is_ocr_available() {
            // Re-serve the whole previous pass so every located ROI stays
            // visible, with usable values relabelled as carried forward and
            // failures still shown as failures.
            let ocr = self
                .last_pass
                .iter()
                .map(|entry| {
                    self.carry_forward(entry.field)
                        .unwrap_or_else(|| entry.clone())
                })
                .collect();
            return HudTextReading {
                text: self.cached.clone(),
                ocr,
            };
        }

        let mut results = Vec::new();

        // MapleStory always prints HP and MP as `current/max`, so a reading
        // without a denominator is OCR noise (a fragment of the label, a
        // neighbouring icon) rather than a real value, and is rejected.
        for (field, roi) in [
            (HudField::Hp, markers.hp_bar),
            (HudField::Mp, markers.mp_bar),
        ] {
            let Some(roi) = roi else { continue };
            results.push(self.read_field(
                image,
                field,
                roi,
                frame_id,
                |raw| match parse_current_max(raw) {
                    Some((current, Some(max))) => ParsedValue::Amount {
                        current,
                        max: Some(max),
                    },
                    _ => ParsedValue::Invalid,
                },
            ));
        }

        // EXP prints an absolute with a bracketed percentage. The exact
        // printed percentage is preferred, since that is the game's own
        // number rather than anything derived.
        if let Some(roi) = markers.exp_bar {
            results.push(self.read_field(image, HudField::Exp, roi, frame_id, |raw| {
                if let Some(percent) = parse_percent(raw) {
                    return ParsedValue::Percent(percent);
                }
                match parse_current_max(raw) {
                    Some((current, max @ Some(_))) => ParsedValue::Amount { current, max },
                    _ => ParsedValue::Invalid,
                }
            }));
        }

        for (field, roi, label) in [
            (HudField::PlayerName, markers.name_plate, "name"),
            (HudField::Job, markers.class_plate, "job"),
            (HudField::Level, markers.level_plate, "lv"),
        ] {
            let Some(roi) = roi else { continue };
            results.push(self.read_field(image, field, roi, frame_id, |raw| {
                let picked = match field {
                    HudField::PlayerName => pick_value(raw, label, plausible_name),
                    HudField::Job => pick_value(raw, label, plausible_job),
                    _ => pick_value(raw, label, |candidate| plausible_level(candidate).is_some())
                        .and_then(|value| plausible_level(&value)),
                };
                match picked {
                    Some(value) => ParsedValue::Text(value),
                    None => ParsedValue::Invalid,
                }
            }));
        }

        for result in &results {
            self.remember(result);
        }
        self.last_pass = results.clone();

        let text = HudText::from_results(&results);
        self.cached = text.clone();
        self.primed = true;
        HudTextReading { text, ocr: results }
    }
}

impl HudText {
    /// Project provenance records onto the flat shape the fusion step and
    /// the serialised game state already consume.
    fn from_results(results: &[HudOcrResult]) -> Self {
        let mut text = HudText::default();
        for result in results {
            match result.field {
                HudField::Hp => text.hp = amount_of(result),
                HudField::Mp => text.mp = amount_of(result),
                HudField::Exp => {
                    text.exp = amount_of(result);
                    if let ParsedValue::Percent(percent) = result.parsed {
                        text.exp_percent = Some(percent);
                    }
                }
                HudField::PlayerName => text.player_name = text_of(result),
                HudField::Job => text.character_class = text_of(result),
                HudField::Level => text.level = text_of(result),
                HudField::Mesos | HudField::MapName => {}
            }
        }
        text
    }
}

fn amount_of(result: &HudOcrResult) -> Option<(u64, Option<u64>)> {
    match result.parsed {
        ParsedValue::Amount { current, max } => Some((current, max)),
        _ => None,
    }
}

fn text_of(result: &HudOcrResult) -> Option<String> {
    match &result.parsed {
        ParsedValue::Text(value) => Some(value.clone()),
        _ => None,
    }
}

fn read_region(image: &RgbaImage, rect: Rect) -> Option<String> {
    let result = ocr::ocr_region(image, rect.x, rect.y, rect.w, rect.h)?;
    let text = result.text.trim().to_string();
    (!text.is_empty()).then_some(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_current_and_max_with_separators() {
        assert_eq!(parse_current_max("400/400"), Some((400, Some(400))));
        assert_eq!(parse_current_max("[1291/1351]"), Some((1291, Some(1351))));
        assert_eq!(
            parse_current_max("65,122 / 71,867"),
            Some((65122, Some(71867)))
        );
    }

    #[test]
    fn tolerates_ocr_confusing_the_slash() {
        // Tesseract regularly reads '/' as a pipe or letter l.
        assert_eq!(parse_current_max("400|400"), Some((400, Some(400))));
        assert_eq!(parse_current_max("400l400"), Some((400, Some(400))));
    }

    #[test]
    fn a_lone_number_still_reports_a_current_value() {
        assert_eq!(parse_current_max("35900"), Some((35900, None)));
    }

    #[test]
    fn returns_none_when_there_are_no_digits() {
        assert_eq!(parse_current_max("HP"), None);
        assert_eq!(parse_current_max(""), None);
    }

    #[test]
    fn parses_a_trailing_percentage() {
        assert_eq!(parse_percent("35900[37.51%]"), Some(37.51));
        assert_eq!(parse_percent("EXP 26.888%"), Some(26.888));
        assert_eq!(parse_percent("no percentage here"), None);
    }

    #[test]
    fn rejects_out_of_range_percentages() {
        // A misread like "1937%" is not a percentage worth trusting.
        assert_eq!(parse_percent("1937%"), None);
    }

    #[test]
    fn strips_plate_labels() {
        assert_eq!(strip_label("NAME: SnareDrumGuy", "name"), "SnareDrumGuy");
        assert_eq!(strip_label("JOB - MAGICIAN", "job"), "MAGICIAN");
        assert_eq!(strip_label("LV 30", "lv"), "30");
        // Without the label the value passes through untouched.
        assert_eq!(strip_label("Cleric", "job"), "Cleric");
    }

    #[test]
    fn picks_the_value_even_when_ocr_mangles_the_label() {
        // The stylised "NAME" label routinely comes back as "HATE", so the
        // value cannot be found by stripping a label that is not there.
        assert_eq!(
            pick_value("HATE SnareDremGuy", "name", plausible_name),
            Some("SnareDremGuy".into())
        );
        assert_eq!(
            pick_value("NAME: SnareDremGuy", "name", plausible_name),
            Some("SnareDremGuy".into())
        );
        assert_eq!(
            pick_value("J0B MAGICIAN", "job", plausible_job),
            Some("MAGICIAN".into())
        );
    }

    #[test]
    fn picks_two_word_values_before_single_tokens() {
        assert_eq!(
            pick_value("JOB Dawn Warrior", "job", plausible_job),
            Some("Dawn Warrior".into())
        );
    }

    #[test]
    fn picks_nothing_when_no_candidate_is_plausible() {
        assert_eq!(
            pick_value("Can make the job advancement to", "name", plausible_name),
            None
        );
    }

    #[test]
    fn rejects_ocr_spill_as_a_character_name() {
        assert!(plausible_name("SnareDrumGuy"));
        assert!(plausible_name("Cloto"));
        // A sentence scraped from a dialog under a mis-located plate.
        assert!(!plausible_name(
            "CEN MAKE THE JOB ADVANCEMENT TO WWARRIOR BO WMAN"
        ));
        assert!(!plausible_name("A"));
        assert!(!plausible_name(""));
        // Names have no spaces or punctuation.
        assert!(!plausible_name("Snare Drum"));
        assert!(!plausible_name("A... . . AM . F"));
    }

    #[test]
    fn rejects_ocr_spill_as_a_job_name() {
        assert!(plausible_job("MAGICIAN"));
        assert!(plausible_job("Dawn Warrior"));
        assert!(!plausible_job(
            "Can make the job advancement to Warrior Bowman Thief"
        ));
        assert!(!plausible_job("A... . . AM . F"));
    }

    #[test]
    fn level_must_be_a_number_in_range() {
        assert_eq!(plausible_level("30"), Some("30".into()));
        assert_eq!(plausible_level("230"), Some("230".into()));
        // Out of range, or not a level at all.
        assert_eq!(plausible_level("0"), None);
        assert_eq!(plausible_level("9999"), None);
        assert_eq!(plausible_level("CEN MAKE THE JOB"), None);
        assert_eq!(plausible_level(""), None);
    }

    /// A reader primed with one usable and one failed field, without
    /// invoking OCR.
    fn primed_reader() -> HudTextReader {
        let roi = Rect {
            x: 10,
            y: 20,
            w: 30,
            h: 12,
        };
        let mut reader = HudTextReader::new(60);
        let good = HudOcrResult {
            field: HudField::Hp,
            roi,
            raw_text: Some("400 / 400".into()),
            parsed: ParsedValue::Amount {
                current: 400,
                max: Some(400),
            },
            frame_id: 7,
            state: ReadState::ReadThisFrame,
            quality: None,
            note: None,
            confidence: 0.9,
        };
        let bad = HudOcrResult::unread(HudField::Mp, roi, 7, Some("OAP 6201".into()));
        reader.remember(&good);
        reader.remember(&bad);
        reader.last_pass = vec![good, bad];
        reader
    }

    #[test]
    fn regions_too_small_or_too_tall_for_text_are_rejected() {
        // An 8px strip cannot hold a glyph; recognising it produced the
        // job "eo Pages os Te" from the real fixture.
        assert!(
            roi_rejection(Rect {
                x: 0,
                y: 0,
                w: 210,
                h: 8
            })
            .is_some()
        );
        // 62px spans several stat rows, so whatever is read describes the
        // wrong thing.
        assert!(
            roi_rejection(Rect {
                x: 0,
                y: 0,
                w: 210,
                h: 62
            })
            .is_some()
        );
        assert!(
            roi_rejection(Rect {
                x: 0,
                y: 0,
                w: 10,
                h: 20
            })
            .is_some()
        );
        // A normal HUD text row passes.
        assert!(
            roi_rejection(Rect {
                x: 0,
                y: 0,
                w: 152,
                h: 38
            })
            .is_none()
        );
    }

    #[test]
    fn a_rejected_region_reports_the_layout_fault_not_a_read_failure() {
        let roi = Rect {
            x: 75,
            y: 270,
            w: 210,
            h: 8,
        };
        let result = HudOcrResult::rejected_roi(HudField::Job, roi, 12, "region only 8px tall");
        assert!(!result.is_usable());
        assert_eq!(result.raw_text, None, "nothing was recognised");
        assert!(
            result.note.as_deref().is_some_and(|n| n.contains("8px")),
            "the reason must be reported"
        );
    }

    #[test]
    fn a_carried_value_keeps_its_original_frame_and_is_relabelled() {
        let reader = primed_reader();
        let carried = reader
            .carry_forward(HudField::Hp)
            .expect("a usable reading should be carried");
        // The frame id must stay the frame it was read from, so provenance
        // never claims the current frame produced it.
        assert_eq!(carried.frame_id, 7);
        assert_eq!(carried.state, ReadState::CarriedForward { from_frame: 7 });
    }

    #[test]
    fn a_failed_field_is_never_carried_forward() {
        let reader = primed_reader();
        // Nothing usable was ever read for MP, so there is nothing to reuse.
        assert!(reader.carry_forward(HudField::Mp).is_none());
    }

    #[test]
    fn results_are_projected_onto_the_flat_text_shape() {
        let roi = Rect {
            x: 0,
            y: 0,
            w: 10,
            h: 10,
        };
        let results = vec![
            HudOcrResult {
                field: HudField::Hp,
                roi,
                raw_text: None,
                parsed: ParsedValue::Amount {
                    current: 2341,
                    max: Some(3100),
                },
                frame_id: 1,
                state: ReadState::ReadThisFrame,
                quality: None,
                note: None,
                confidence: 0.9,
            },
            HudOcrResult {
                field: HudField::Exp,
                roi,
                raw_text: None,
                parsed: ParsedValue::Percent(59.823),
                frame_id: 1,
                state: ReadState::ReadThisFrame,
                quality: None,
                note: None,
                confidence: 0.9,
            },
            // A failed field must contribute nothing at all.
            HudOcrResult::unread(HudField::Mp, roi, 1, Some("junk".into())),
        ];
        let text = HudText::from_results(&results);
        assert_eq!(text.hp, Some((2341, Some(3100))));
        assert_eq!(text.exp_percent, Some(59.823));
        assert_eq!(text.mp, None, "an invalid read must not produce a value");
    }

    #[test]
    fn cadence_refreshes_on_the_first_frame_then_on_interval() {
        let reader = HudTextReader::new(10);
        // Nothing read yet, so the first call must do real work.
        assert!(reader.will_refresh());
        assert!(!reader.is_primed());
    }

    #[test]
    fn cadence_treats_zero_interval_as_every_frame() {
        // A zero interval would divide by zero in the multiple check.
        let reader = HudTextReader::new(0);
        assert!(reader.will_refresh());
    }
}
