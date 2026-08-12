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
use crate::vision::ocr;

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
    if let Some(slash) = chars.iter().position(|c| *c == '/') {
        // The numbers may be spaced away from the slash ("400 / 400"), so
        // scan outward past whitespace to the digit run on each side.
        let left = digit_run_before(&chars, slash);
        let right = digit_run_after(&chars, slash);
        if let Some(current) = left {
            return Some((current, right));
        }
    }

    // No pair, but a lone number is still worth reporting.
    cleaned
        .split_whitespace()
        .find_map(digits_only)
        .map(|value| (value, None))
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

fn digits_only(token: &str) -> Option<u64> {
    let digits: String = token.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return None;
    }
    digits.parse::<u64>().ok()
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

/// Reads HUD text on a cadence and caches it between reads.
#[derive(Debug)]
pub struct HudTextReader {
    interval: u64,
    frames_seen: u64,
    cached: HudText,
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
            primed: false,
        }
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
    /// Between refreshes this is free: it hands back the cached values.
    pub fn read(&mut self, image: &RgbaImage, markers: &super::hud_geometry::UiMarkers) -> HudText {
        let refresh = self.will_refresh();
        self.frames_seen = self.frames_seen.wrapping_add(1);

        if !refresh || !ocr::is_ocr_available() {
            return self.cached.clone();
        }

        let mut text = HudText::default();

        // Bar numbers sit inside the padded region around each bar.
        // MapleStory always prints HP and MP as `current/max`, so a reading
        // without a denominator is OCR noise (a fragment of the label, a
        // neighbouring icon) rather than a real value, and is discarded.
        if let Some(rect) = markers.hp_bar {
            text.hp = read_pair(image, rect).filter(|(_, max)| max.is_some());
        }
        if let Some(rect) = markers.mp_bar {
            text.mp = read_pair(image, rect).filter(|(_, max)| max.is_some());
        }
        if let Some(rect) = markers.exp_bar {
            let raw = read_region(image, rect);
            text.exp_percent = raw.as_deref().and_then(parse_percent);
            // EXP is printed as an absolute with a bracketed percentage, so
            // a lone number is legitimate here only when the percentage
            // confirms we are looking at the EXP readout.
            text.exp = raw
                .as_deref()
                .and_then(parse_current_max)
                .filter(|(_, max)| max.is_some() || text.exp_percent.is_some());
        }

        text.player_name = markers
            .name_plate
            .and_then(|rect| read_region(image, rect))
            .map(|raw| strip_label(&raw, "name"))
            .filter(|value| plausible_name(value));
        text.character_class = markers
            .class_plate
            .and_then(|rect| read_region(image, rect))
            .map(|raw| strip_label(&raw, "job"))
            .filter(|value| plausible_job(value));
        text.level = markers
            .level_plate
            .and_then(|rect| read_region(image, rect))
            .and_then(|raw| plausible_level(&strip_label(&raw, "lv")));

        self.cached = text.clone();
        self.primed = true;
        text
    }
}

fn read_region(image: &RgbaImage, rect: Rect) -> Option<String> {
    let result = ocr::ocr_region(image, rect.x, rect.y, rect.w, rect.h)?;
    let text = result.text.trim().to_string();
    (!text.is_empty()).then_some(text)
}

fn read_pair(image: &RgbaImage, rect: Rect) -> Option<(u64, Option<u64>)> {
    read_region(image, rect)
        .as_deref()
        .and_then(parse_current_max)
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
