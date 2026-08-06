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

use crate::vision::hud_geometry::{self, HudMetric as RawHudMetric, HudSnapshot as RawHudSnapshot, UiMarkers};
use crate::vision::types::{Confidence, Detection, Reliability, Source};

/// A HUD metric (HP/MP/EXP) with confidence-scored percent and absolute value.
#[derive(Debug, Clone)]
pub struct HudMetric {
    pub label: String,
    pub percent: Option<f32>,
    pub value: Option<u64>,
    pub raw_text: Option<String>,
}

impl From<RawHudMetric> for HudMetric {
    fn from(raw: RawHudMetric) -> Self {
        Self {
            label: raw.label,
            percent: raw.percent,
            value: raw.value,
            raw_text: raw.raw_text,
        }
    }
}

/// Full, confidence-annotated HUD reading for a single frame.
#[derive(Debug, Clone)]
pub struct HudReading {
    pub markers: UiMarkers,
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
            let has_ocr_value = raw.value.is_some() || raw.raw_text.as_deref().is_some_and(|t| t.chars().any(|c| c.is_ascii_digit()));
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
        Some(text) => Detection::found(text, Confidence::new(0.8), Source::Hud, Reliability::Corroborated),
        None if plate_found => {
            let mut detection = Detection::missing(Source::Hud, "plate located but OCR text could not be read");
            detection.reliability = Reliability::Heuristic;
            detection
        }
        None => Detection::missing(Source::Hud, "no stat-row plate detected for this field"),
    }
}

/// Stateless HUD detector. Cheap to construct; holds no per-frame state
/// (temporal smoothing of HUD values is handled by the pipeline via
/// [`crate::vision::temporal::ConfidenceAccumulator`] so a single dropped
/// OCR read does not make a value flicker to "unknown").
#[derive(Debug, Default, Clone, Copy)]
pub struct HudDetector;

impl HudDetector {
    pub fn new() -> Self {
        Self
    }

    pub fn detect(&self, image: &RgbaImage) -> HudReading {
        let RawHudSnapshot {
            markers,
            hp,
            mp,
            exp,
            player_name,
            character_class,
            level,
        } = hud_geometry::detect_hud_snapshot(image);

        HudReading {
            hp: metric_detection(hp),
            mp: metric_detection(mp),
            exp: metric_detection(exp),
            player_name: text_detection(player_name, markers.name_plate.is_some()),
            character_class: text_detection(character_class, markers.class_plate.is_some()),
            level: text_detection(level, markers.level_plate.is_some()),
            markers,
        }
    }
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
            raw_text: None,
        };
        let detection = metric_detection(Some(raw));
        assert_eq!(detection.reliability, Reliability::Heuristic);
        assert!(!detection.confidence.is_confident(0.8));
    }
}
