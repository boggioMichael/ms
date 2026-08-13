//! Provenance for every value the engine reads as text.
//!
//! A number on the dashboard is worthless if you cannot ask where it came
//! from. This module carries the whole chain with the value:
//!
//! ```text
//!   frame 778
//!     -> roi (431, 732, 148x38)
//!        -> pixels
//!           -> raw text "1291 / 1351"
//!              -> parser
//!                 -> Amount { current: 1291, max: Some(1351) }
//! ```
//!
//! Two rules are structural rather than conventional, because both were
//! violated by the previous design and the mistakes were invisible:
//!
//! 1. A bar's fill percentage is **not** a reading. It is an estimate from
//!    pixel widths and lives in its own type ([`BarEstimate`]), so it cannot
//!    be assigned where a read value is expected.
//! 2. A value reused from an earlier frame is **not** a reading of this
//!    frame. [`ReadState`] keeps them apart so the display can say which it
//!    is showing.

use crate::vision::geometry::Rect;
use crate::vision::quality::TextQuality;

/// A HUD field the engine tries to read as text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HudField {
    Hp,
    Mp,
    Exp,
    Level,
    PlayerName,
    Job,
    Mesos,
    MapName,
}

impl HudField {
    /// Short label used in the preview and the dashboard.
    pub fn label(self) -> &'static str {
        match self {
            HudField::Hp => "HP",
            HudField::Mp => "MP",
            HudField::Exp => "EXP",
            HudField::Level => "LEVEL",
            HudField::PlayerName => "NAME",
            HudField::Job => "JOB",
            HudField::Mesos => "MESOS",
            HudField::MapName => "MAP",
        }
    }
}

/// What a field's text turned into once parsed.
///
/// [`ParsedValue::Invalid`] is deliberately distinct from
/// [`ParsedValue::NotRead`]: "we saw text and it did not make sense" is a
/// different fact from "there was no text", and hiding the difference is
/// what lets a broken ROI look like an absent HUD.
#[derive(Debug, Clone, PartialEq)]
pub enum ParsedValue {
    /// A `current / max` pair, or a lone current value when the HUD prints
    /// no denominator.
    Amount { current: u64, max: Option<u64> },
    /// A percentage the game itself printed.
    Percent(f32),
    /// Free text, such as a name or job.
    Text(String),
    /// Text was recognised but did not match the field's expected syntax.
    Invalid,
    /// No text was recognised at all.
    NotRead,
}

impl ParsedValue {
    pub fn is_usable(&self) -> bool {
        !matches!(self, ParsedValue::Invalid | ParsedValue::NotRead)
    }

    /// Render for display, never inventing content.
    pub fn display(&self) -> String {
        match self {
            ParsedValue::Amount {
                current,
                max: Some(max),
            } => format!("{current} / {max}"),
            ParsedValue::Amount { current, max: None } => current.to_string(),
            ParsedValue::Percent(percent) => format!("{percent:.3}%"),
            ParsedValue::Text(text) => text.clone(),
            ParsedValue::Invalid => "INVALID".to_string(),
            ParsedValue::NotRead => "unknown".to_string(),
        }
    }
}

/// Whether a value was read from this frame, reused, or is simply unknown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadState {
    /// Recognised from this frame's pixels.
    ReadThisFrame,
    /// Recognised earlier and reused because this frame could not be read.
    /// Carries the frame it came from so the age is visible.
    CarriedForward { from_frame: u64 },
    /// Never successfully read.
    Unknown,
}

impl ReadState {
    /// A single character for compact displays.
    pub fn marker(self) -> char {
        match self {
            ReadState::ReadThisFrame => '*',
            ReadState::CarriedForward { .. } => '~',
            ReadState::Unknown => '?',
        }
    }

    pub fn describe(self) -> String {
        match self {
            ReadState::ReadThisFrame => "read this frame".to_string(),
            ReadState::CarriedForward { from_frame } => format!("carried from frame {from_frame}"),
            ReadState::Unknown => "unknown".to_string(),
        }
    }
}

/// One field's reading, with everything needed to audit it.
#[derive(Debug, Clone)]
pub struct HudOcrResult {
    pub field: HudField,
    /// Exactly the rectangle that was handed to the recogniser.
    pub roi: Rect,
    /// What the recogniser returned, before parsing. Kept even when parsing
    /// fails: seeing `234I / 3I00` is how a misread is recognised as a
    /// misread rather than as an absent HUD.
    pub raw_text: Option<String>,
    pub parsed: ParsedValue,
    /// The frame whose pixels were recognised. For a carried-forward value
    /// this is the frame it was originally read from, not the current one.
    pub frame_id: u64,
    pub state: ReadState,
    /// How legible the ROI was, when measured. A blurred ROI explains a bad
    /// read without having to guess.
    pub quality: Option<TextQuality>,
}

impl HudOcrResult {
    /// A reading that did not happen, for a field whose ROI was located but
    /// whose text could not be recognised.
    pub fn unread(field: HudField, roi: Rect, frame_id: u64, raw_text: Option<String>) -> Self {
        let parsed = if raw_text.is_some() {
            // Text came back but failed validation.
            ParsedValue::Invalid
        } else {
            ParsedValue::NotRead
        };
        Self {
            field,
            roi,
            raw_text,
            parsed,
            frame_id,
            state: ReadState::Unknown,
            quality: None,
        }
    }

    pub fn is_usable(&self) -> bool {
        self.parsed.is_usable()
    }

    /// `current` when this field parsed to an amount.
    pub fn current(&self) -> Option<u64> {
        match self.parsed {
            ParsedValue::Amount { current, .. } => Some(current),
            _ => None,
        }
    }

    /// `max` when this field parsed to an amount that had one.
    pub fn maximum(&self) -> Option<u64> {
        match self.parsed {
            ParsedValue::Amount { max, .. } => max,
            _ => None,
        }
    }

    /// The percentage implied by this reading, when it can be derived from
    /// the numbers the game printed. This is not a bar estimate.
    pub fn percent(&self) -> Option<f32> {
        match self.parsed {
            ParsedValue::Percent(percent) => Some(percent),
            ParsedValue::Amount {
                current,
                max: Some(max),
            } if max > 0 => Some((current as f32 / max as f32 * 100.0).clamp(0.0, 100.0)),
            _ => None,
        }
    }

    /// One line of provenance, for the preview and for `--explain`.
    pub fn provenance(&self) -> String {
        format!(
            "frame {} -> ({}, {}) {}x{} -> {:?} -> {} [{}]",
            self.frame_id,
            self.roi.x,
            self.roi.y,
            self.roi.w,
            self.roi.h,
            self.raw_text.as_deref().unwrap_or(""),
            self.parsed.display(),
            self.state.describe()
        )
    }
}

/// A bar's measured fill, kept deliberately separate from any reading.
///
/// This exists to corroborate a reading or to show that a HUD element is
/// present at all. It is never a substitute for the printed number, so it
/// has no path into [`ParsedValue`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BarEstimate {
    pub percent: f32,
    pub bounds: Rect,
}

/// How a bar estimate compares with what was read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Agreement {
    /// The bar corroborates the reading.
    Consistent,
    /// They disagree by more than measurement error explains, which means
    /// one of them is wrong and the user should be told rather than shown a
    /// confident number.
    Conflicting,
    /// Nothing to compare against.
    Unchecked,
}

/// Largest gap between a printed value and a bar's fill still attributable
/// to measurement error rather than a wrong reading.
///
/// Bar measurement includes a few columns of border and inter-bar gap, worth
/// a few percent on a narrow bar; a genuinely misread number is normally out
/// by far more than this.
pub const AGREEMENT_TOLERANCE: f32 = 8.0;

/// Compare a reading against the bar beside it.
pub fn agreement(reading: Option<f32>, bar: Option<f32>) -> Agreement {
    match (reading, bar) {
        (Some(read), Some(bar)) => {
            if (read - bar).abs() <= AGREEMENT_TOLERANCE {
                Agreement::Consistent
            } else {
                Agreement::Conflicting
            }
        }
        _ => Agreement::Unchecked,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roi() -> Rect {
        Rect {
            x: 431,
            y: 732,
            w: 148,
            h: 38,
        }
    }

    fn reading(parsed: ParsedValue, state: ReadState) -> HudOcrResult {
        HudOcrResult {
            field: HudField::Mp,
            roi: roi(),
            raw_text: Some("1291 / 1351".into()),
            parsed,
            frame_id: 778,
            state,
            quality: None,
        }
    }

    #[test]
    fn an_amount_exposes_current_max_and_a_derived_percent() {
        let result = reading(
            ParsedValue::Amount {
                current: 1291,
                max: Some(1351),
            },
            ReadState::ReadThisFrame,
        );
        assert_eq!(result.current(), Some(1291));
        assert_eq!(result.maximum(), Some(1351));
        assert!((result.percent().unwrap() - 95.6).abs() < 0.1);
        assert_eq!(result.parsed.display(), "1291 / 1351");
    }

    #[test]
    fn a_lone_number_has_no_maximum_and_no_percent() {
        // Nothing may invent a denominator the game did not print.
        let result = reading(
            ParsedValue::Amount {
                current: 35900,
                max: None,
            },
            ReadState::ReadThisFrame,
        );
        assert_eq!(result.maximum(), None);
        assert_eq!(result.percent(), None);
        assert_eq!(result.parsed.display(), "35900");
    }

    #[test]
    fn a_printed_percentage_is_reported_exactly() {
        let result = reading(ParsedValue::Percent(59.823), ReadState::ReadThisFrame);
        assert_eq!(result.parsed.display(), "59.823%");
        assert_eq!(result.percent(), Some(59.823));
    }

    #[test]
    fn unreadable_text_is_invalid_and_unusable() {
        let result = HudOcrResult::unread(HudField::Hp, roi(), 778, Some("234I / 3I00".into()));
        assert_eq!(result.parsed, ParsedValue::Invalid);
        assert!(!result.is_usable());
        // The raw text survives so the misread can be seen.
        assert_eq!(result.raw_text.as_deref(), Some("234I / 3I00"));
        assert_eq!(result.parsed.display(), "INVALID");
    }

    #[test]
    fn absent_text_is_distinguished_from_invalid_text() {
        let result = HudOcrResult::unread(HudField::Hp, roi(), 778, None);
        assert_eq!(result.parsed, ParsedValue::NotRead);
        assert_eq!(result.parsed.display(), "unknown");
    }

    #[test]
    fn carried_forward_values_report_their_origin_frame() {
        let result = reading(
            ParsedValue::Amount {
                current: 1291,
                max: Some(1351),
            },
            ReadState::CarriedForward { from_frame: 776 },
        );
        assert_eq!(result.state.marker(), '~');
        assert!(result.state.describe().contains("776"));
        // A reused value must never claim to be from this frame.
        assert_ne!(result.state, ReadState::ReadThisFrame);
    }

    #[test]
    fn provenance_traces_a_value_back_to_pixels() {
        let result = reading(
            ParsedValue::Amount {
                current: 1291,
                max: Some(1351),
            },
            ReadState::ReadThisFrame,
        );
        let trace = result.provenance();
        assert!(trace.contains("frame 778"));
        assert!(trace.contains("431"), "roi origin missing: {trace}");
        assert!(trace.contains("1291 / 1351"));
    }

    #[test]
    fn agreement_flags_a_bar_that_contradicts_the_reading() {
        // 95.6% read, bar measured 93.8%: within measurement error.
        assert_eq!(agreement(Some(95.6), Some(93.8)), Agreement::Consistent);
        // 95.6% read, bar measured 21.7%: something is wrong.
        assert_eq!(agreement(Some(95.6), Some(21.7)), Agreement::Conflicting);
        assert_eq!(agreement(None, Some(93.8)), Agreement::Unchecked);
        assert_eq!(agreement(Some(95.6), None), Agreement::Unchecked);
    }
}
