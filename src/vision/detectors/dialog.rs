//! Dialog / popup / notification detector.
//!
//! MapleStory renders modal prompts (death, revive, rune activation, quest
//! dialogs, generic notifications) as a bordered panel over a roughly
//! uniform background color, usually centered or upper-centered on screen.
//! Rather than hardcoding a specific skin's panel color (which breaks the
//! moment the UI skin changes), this detector finds the largest
//! near-uniform-color rectangular region within the likely dialog band and
//! then classifies its OCR'd text against [`crate::knowledge::dialogs`].

use image::RgbaImage;

use crate::knowledge::dialogs::{DialogKind, classify};
use crate::vision::geometry::{Rect, dominant_color_bucket, find_uniform_color_panel};
use crate::vision::ocr;
use crate::vision::types::{Confidence, Detection, Reliability, Source};

/// A detected dialog/popup panel and its classification.
#[derive(Debug, Clone)]
pub struct DialogReading {
    pub bounds: Rect,
    pub kind: DialogKind,
    pub text: Option<String>,
}

#[derive(Debug, Clone, Copy)]
pub struct DialogConfig {
    /// Color-bucket quantization step (larger = more tolerant panel match).
    pub quantization: u8,
    /// Minimum panel area as a fraction of the search band's area.
    pub min_area_fraction: f32,
}

impl Default for DialogConfig {
    fn default() -> Self {
        Self {
            quantization: 12,
            min_area_fraction: 0.06,
        }
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct DialogDetector {
    config: DialogConfig,
}

impl DialogDetector {
    pub fn new(config: DialogConfig) -> Self {
        Self { config }
    }

    pub fn detect(&self, image: &RgbaImage) -> Detection<DialogReading> {
        let width = image.width();
        let height = image.height();
        if width == 0 || height == 0 {
            return Detection::missing(Source::Dialog, "empty frame");
        }

        // Dialogs are rendered above the bottom HUD band and rarely at the
        // very top (reserved for the minimap/quest tracker), so restrict the
        // search to the vertical middle band to avoid false positives.
        let band = Rect {
            x: width / 8,
            y: height / 6,
            w: width * 3 / 4,
            h: height * 2 / 3,
        };

        let Some(dominant) = dominant_color_bucket(image, band, self.config.quantization) else {
            return Detection::missing(Source::Dialog, "no coherent panel color found");
        };

        let Some(panel) = find_uniform_color_panel(image, band, dominant, self.config.quantization)
        else {
            return Detection::missing(Source::Dialog, "no panel-sized uniform region found");
        };

        let min_area = (band.area() as f32 * self.config.min_area_fraction) as u32;
        if panel.area() < min_area {
            return Detection::missing(Source::Dialog, "candidate panel too small to be a dialog");
        }

        let text =
            ocr::ocr_region(image, panel.x, panel.y, panel.w, panel.h).map(|result| result.text);
        let kind = text.as_deref().map(classify).unwrap_or(DialogKind::None);

        let (confidence, reliability) = match (&text, kind) {
            (Some(_), DialogKind::None) => (Confidence::new(0.35), Reliability::Heuristic),
            (Some(_), _) => (Confidence::new(0.75), Reliability::Corroborated),
            (None, _) => (Confidence::new(0.3), Reliability::Heuristic),
        };

        Detection::found(
            DialogReading {
                bounds: panel,
                kind,
                text,
            },
            confidence,
            Source::Dialog,
            reliability,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    fn frame_with_panel(kind_text_marker: bool) -> RgbaImage {
        let mut image = RgbaImage::from_pixel(400, 300, Rgba([15, 18, 22, 255]));
        for y in 100..180 {
            for x in 100..300 {
                image.put_pixel(x, y, Rgba([60, 60, 70, 255]));
            }
        }
        let _ = kind_text_marker;
        image
    }

    #[test]
    fn detects_uniform_panel_region() {
        let image = frame_with_panel(false);
        let detector = DialogDetector::new(DialogConfig::default());
        let detection = detector.detect(&image);
        assert!(detection.is_present());
        let reading = detection.value.unwrap();
        assert!(reading.bounds.w >= 100 && reading.bounds.h >= 40);
    }

    #[test]
    fn empty_frame_is_reported_as_missing_not_panicking() {
        let image = RgbaImage::new(0, 0);
        let detector = DialogDetector::new(DialogConfig::default());
        let detection = detector.detect(&image);
        assert!(!detection.is_present());
    }
}
