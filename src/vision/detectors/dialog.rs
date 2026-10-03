//! Dialog / popup / notification detector.
//!
//! MapleStory renders modal prompts (death, revive, rune activation, quest
//! dialogs, generic notifications) as a bordered panel over a roughly
//! uniform background color, usually centered or upper-centered on screen.
//! Rather than hardcoding a specific skin's panel color (which breaks the
//! moment the UI skin changes), this detector finds the largest
//! near-uniform-color rectangular region within the likely dialog band and
//! then classifies its OCR'd text against [`crate::knowledge::dialogs`].
//!
//! OCR costs a process (or the OS engine) per call, so a panel's text is
//! read when the panel appears or moves and then every
//! [`DialogConfig::ocr_every`] frames; in between, the last text read is
//! carried with the panel.

use image::RgbaImage;

use crate::knowledge::dialogs::{DialogKind, classify};
use crate::vision::geometry::{Rect, dominant_color_bucket, find_uniform_color_panel};
use crate::vision::ocr;
use crate::vision::types::{Confidence, Detection, Reliability};

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
    /// Frames between OCR passes over a panel that stays where it is.
    pub ocr_every: u64,
}

impl Default for DialogConfig {
    fn default() -> Self {
        Self {
            quantization: 12,
            min_area_fraction: 0.06,
            ocr_every: 30,
        }
    }
}

/// The last text read from a panel, and where and when.
#[derive(Debug, Clone)]
struct LastRead {
    bounds: Rect,
    frame_id: u64,
    text: Option<String>,
}

#[derive(Debug, Default, Clone)]
pub struct DialogDetector {
    config: DialogConfig,
    last: Option<LastRead>,
}

impl DialogDetector {
    pub fn new(config: DialogConfig) -> Self {
        Self { config, last: None }
    }

    /// The panel's text: read now when the panel is new, has moved, or the
    /// cadence has come round; otherwise what was read last.
    fn text_of(&mut self, image: &RgbaImage, panel: Rect, frame_id: u64) -> Option<String> {
        let moved = |a: Rect, b: Rect| {
            a.x.abs_diff(b.x) > 4
                || a.y.abs_diff(b.y) > 4
                || a.w.abs_diff(b.w) > 8
                || a.h.abs_diff(b.h) > 8
        };
        if let Some(last) = &self.last
            && !moved(last.bounds, panel)
            && frame_id.wrapping_sub(last.frame_id) < self.config.ocr_every
        {
            return last.text.clone();
        }
        let text =
            ocr::ocr_region(image, panel.x, panel.y, panel.w, panel.h).map(|result| result.text);
        self.last = Some(LastRead {
            bounds: panel,
            frame_id,
            text: text.clone(),
        });
        text
    }

    pub fn detect(&mut self, image: &RgbaImage, frame_id: u64) -> Detection<DialogReading> {
        let width = image.width();
        let height = image.height();
        if width == 0 || height == 0 {
            return Detection::missing("dialog", "empty frame");
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
            return Detection::missing("dialog", "no coherent panel color found");
        };

        let Some(panel) = find_uniform_color_panel(image, band, dominant, self.config.quantization)
        else {
            return Detection::missing("dialog", "no panel-sized uniform region found");
        };

        let min_area = (band.area() as f32 * self.config.min_area_fraction) as u32;
        if panel.area() < min_area {
            return Detection::missing("dialog", "candidate panel too small to be a dialog");
        }

        let text = self.text_of(image, panel, frame_id);
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
            "dialog",
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
        let mut detector = DialogDetector::new(DialogConfig::default());
        let detection = detector.detect(&image, 1);
        assert!(detection.is_present());
        let reading = detection.value.unwrap();
        assert!(reading.bounds.w >= 100 && reading.bounds.h >= 40);
    }

    #[test]
    fn empty_frame_is_reported_as_missing_not_panicking() {
        let image = RgbaImage::new(0, 0);
        let mut detector = DialogDetector::new(DialogConfig::default());
        let detection = detector.detect(&image, 1);
        assert!(!detection.is_present());
    }

    #[test]
    fn a_panel_that_stays_put_is_read_on_a_cadence() {
        let image = frame_with_panel(false);
        let mut detector = DialogDetector::new(DialogConfig {
            ocr_every: 10,
            ..Default::default()
        });
        let panel = detector.detect(&image, 1).value.unwrap().bounds;
        // Pretend the first read, on frame 1, gave some text.
        detector.last = Some(LastRead {
            bounds: panel,
            frame_id: 1,
            text: Some("You have died".into()),
        });
        let carried = detector.detect(&image, 5).value.unwrap();
        assert_eq!(carried.text.as_deref(), Some("You have died"));
        assert_eq!(detector.last.as_ref().unwrap().frame_id, 1);
        // The cadence comes round: read again, whatever the engine says.
        let _ = detector.detect(&image, 11);
        assert_eq!(detector.last.as_ref().unwrap().frame_id, 11);
        // The panel moves: read at once.
        let moved = Rect {
            x: panel.x + 60,
            ..panel
        };
        detector.last = Some(LastRead {
            bounds: moved,
            frame_id: 11,
            text: Some("old".into()),
        });
        let _ = detector.detect(&image, 12);
        assert_eq!(detector.last.as_ref().unwrap().frame_id, 12);
    }
}
