//! HUD detector: HP/MP/EXP bars, name/job/level plates.
//!
//! This is a thin, confidence-aware wrapper around the existing,
//! battle-tested geometry + OCR pipeline in [`crate::vision::hud_geometry`]. The
//! underlying detection logic (color-bar segmentation, zoned OCR, label
//! anchored parsing) is intentionally left untouched here — it was tuned
//! against a real captured frame across many iterations — but every value
//! is now reported through [`Detection<T>`] so callers can distinguish
//! "bar geometry found, but OCR could not confirm the number" from
//! "OCR corroborated an exact value" instead of getting the same `Some`.

use image::RgbaImage;

use crate::vision::hud_geometry::{
    self, HudMetric as RawHudMetric, HudSnapshot as RawHudSnapshot, UiMarkers,
};
use crate::vision::hud_ocr::HudOcrResult;
use crate::vision::hud_text::HudTextReader;
use crate::vision::types::{Confidence, Detection, Reliability, Source};

/// A HUD metric (HP/MP/EXP) with confidence-scored percent and absolute value.
#[derive(Debug, Clone)]
pub struct HudMetric {
    pub label: String,
    pub percent: Option<f32>,
    pub value: Option<u64>,
    /// Maximum, when the HUD printed `current / max`.
    pub max: Option<u64>,
    pub raw_text: Option<String>,
}

impl From<RawHudMetric> for HudMetric {
    fn from(raw: RawHudMetric) -> Self {
        Self {
            label: raw.label,
            percent: raw.percent,
            value: raw.value,
            max: raw.max,
            raw_text: raw.raw_text,
        }
    }
}

/// Full, confidence-annotated HUD reading for a single frame.
#[derive(Debug, Clone)]
pub struct HudReading {
    pub markers: UiMarkers,
    /// Per-field OCR provenance: ROI, raw text, parsed value, frame and
    /// whether it was read now or carried forward.
    pub ocr: Vec<HudOcrResult>,
    pub hp: Detection<HudMetric>,
    pub mp: Detection<HudMetric>,
    pub exp: Detection<HudMetric>,
    pub player_name: Detection<String>,
    pub character_class: Detection<String>,
    pub level: Detection<String>,
}

/// Score how trustworthy a metric reading is: geometry-only detections are
/// `Heuristic`, and detections where OCR also produced a raw value that
/// contains digits are treated as `Corroborated` (two independent signals
/// agree: the colored bar exists, and the label/number text was read).
fn metric_detection(metric: Option<RawHudMetric>) -> Detection<HudMetric> {
    match metric {
        None => Detection::missing(Source::Hud, "no matching color bar found in the HUD band"),
        Some(raw) => {
            let has_ocr_value = raw.value.is_some()
                || raw
                    .raw_text
                    .as_deref()
                    .is_some_and(|t| t.chars().any(|c| c.is_ascii_digit()));
            let (confidence, reliability) = if has_ocr_value {
                (Confidence::new(0.9), Reliability::Corroborated)
            } else if raw.percent.is_some() {
                (Confidence::new(0.55), Reliability::Heuristic)
            } else {
                (Confidence::new(0.2), Reliability::Heuristic)
            };
            Detection::found(HudMetric::from(raw), confidence, Source::Hud, reliability)
        }
    }
}

fn text_detection(value: Option<String>, plate_found: bool) -> Detection<String> {
    match value {
        Some(text) => Detection::found(
            text,
            Confidence::new(0.8),
            Source::Hud,
            Reliability::Corroborated,
        ),
        None if plate_found => {
            let mut detection =
                Detection::missing(Source::Hud, "plate located but OCR text could not be read");
            detection.reliability = Reliability::Heuristic;
            detection
        }
        None => Detection::missing(Source::Hud, "no stat-row plate detected for this field"),
    }
}

/// HUD detector.
///
/// Stateful only because reading the HUD's *text* costs an OCR subprocess
/// per region. Bar geometry runs every frame; the text is refreshed on a
/// cadence and cached in between (see
/// [`crate::vision::hud_text::HudTextReader`]), which keeps the pipeline
/// real-time while still reporting names, levels and exact `current/max`
/// numbers.
#[derive(Debug, Default)]
pub struct HudDetector {
    text: HudTextReader,
}

impl HudDetector {
    pub fn new() -> Self {
        Self::default()
    }

    /// Build a detector with an explicit OCR cadence, in frames.
    pub fn with_ocr_interval(interval: u64) -> Self {
        Self {
            text: HudTextReader::new(interval),
        }
    }

    pub fn detect(&mut self, image: &RgbaImage, frame_id: u64) -> HudReading {
        let RawHudSnapshot {
            markers,
            hp,
            mp,
            exp,
            player_name,
            character_class,
            level,
        } = hud_geometry::detect_hud_snapshot(image);

        // Geometry gives the fill ratio every frame; OCR contributes the
        // printed numbers and plate text when its cadence comes round.
        let reading = self.text.read(image, &markers, frame_id);
        let text = reading.text;

        let hp = fuse(hp, "HP", text.hp, None);
        let mp = fuse(mp, "MP", text.mp, None);
        let exp = fuse(exp, "EXP", text.exp, text.exp_percent);

        HudReading {
            hp: metric_detection(hp),
            mp: metric_detection(mp),
            exp: metric_detection(exp),
            player_name: text_detection(
                player_name.or(text.player_name),
                markers.name_plate.is_some(),
            ),
            character_class: text_detection(
                character_class.or(text.character_class),
                markers.class_plate.is_some(),
            ),
            level: text_detection(level.or(text.level), markers.level_plate.is_some()),
            ocr: reading.ocr,
            markers,
        }
    }
}

/// Combine the geometric bar reading with whatever OCR could read.
///
/// When the HUD printed `current / max`, that pair is authoritative: it is
/// an exact figure, whereas the bar fill is a pixel estimate that skins,
/// borders and antialiasing all nudge. The measured fill is kept as the
/// fallback so a failed OCR read degrades to an approximate percentage
/// rather than to nothing.
fn fuse(
    geometric: Option<RawHudMetric>,
    label: &str,
    ocr_pair: Option<(u64, Option<u64>)>,
    ocr_percent: Option<f32>,
) -> Option<RawHudMetric> {
    let measured_percent = geometric.as_ref().and_then(|metric| metric.percent);
    let (value, max) = match ocr_pair {
        Some((current, max)) => (Some(current), max),
        None => (None, None),
    };

    // An exact percentage is preferred, then current/max, then the bar.
    let percent = ocr_percent
        .or_else(|| match (value, max) {
            (Some(current), Some(max)) if max > 0 => {
                Some((current as f32 / max as f32 * 100.0).clamp(0.0, 100.0))
            }
            _ => None,
        })
        .or(measured_percent);

    if percent.is_none() && value.is_none() {
        return None;
    }

    Some(RawHudMetric {
        label: label.to_string(),
        percent,
        value,
        max,
        raw_text: geometric.and_then(|metric| metric.raw_text),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_metric_reports_failure_reason() {
        let detection = metric_detection(None);
        assert!(!detection.is_present());
        assert!(detection.failure_reason.is_some());
    }

    #[test]
    fn metric_with_ocr_value_is_corroborated() {
        let raw = RawHudMetric {
            label: "HP".into(),
            percent: Some(100.0),
            value: Some(400),
            max: None,
            raw_text: Some("HP 400/400".into()),
        };
        let detection = metric_detection(Some(raw));
        assert_eq!(detection.reliability, Reliability::Corroborated);
        assert!(detection.confidence.is_confident(0.8));
    }

    #[test]
    fn metric_with_geometry_only_is_heuristic() {
        let raw = RawHudMetric {
            label: "MP".into(),
            percent: Some(50.0),
            value: None,
            max: None,
            raw_text: None,
        };
        let detection = metric_detection(Some(raw));
        assert_eq!(detection.reliability, Reliability::Heuristic);
        assert!(!detection.confidence.is_confident(0.8));
    }
}
