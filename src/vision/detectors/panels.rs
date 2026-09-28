//! Fixed-position UI panel detectors: minimap, chat log, and buff/cooldown
//! icon rows.
//!
//! These panels are conventionally anchored to a screen corner regardless
//! of resolution, so each detector searches a proportionally-sized region
//! of the frame rather than a fixed pixel offset (which would break the
//! moment the game window is resized).

use image::RgbaImage;

use crate::vision::geometry::{
    Rect, dominant_color_bucket, find_uniform_color_panel, group_segments, segment_row,
};
use crate::vision::types::{Confidence, Detection, Reliability, Source};

#[derive(Debug, Clone)]
pub struct MinimapReading {
    pub bounds: Rect,
}

/// Detects the minimap as the largest uniform-color panel in the top-left
/// quadrant of the frame (MapleStory always anchors the minimap there).
#[derive(Debug, Default, Clone, Copy)]
pub struct MinimapDetector;

impl MinimapDetector {
    pub fn detect(&self, image: &RgbaImage) -> Detection<MinimapReading> {
        let width = image.width();
        let height = image.height();
        let region = Rect {
            x: 0,
            y: 0,
            w: width / 3,
            h: height / 3,
        };
        let Some(bucket) = dominant_color_bucket(image, region, 10) else {
            return Detection::missing(Source::Panel, "no coherent color found in minimap region");
        };
        let Some(panel) = find_uniform_color_panel(image, region, bucket, 10) else {
            return Detection::missing(
                Source::Panel,
                "no panel-shaped region found in minimap corner",
            );
        };
        // The minimap frame is a compact, roughly-square-to-wide panel; reject
        // tiny slivers (a stray solid-colored icon) and full-width bands
        // (the top HUD bar itself, which is not the minimap).
        if panel.area() < 900 || panel.w > region.w.saturating_mul(9) / 10 {
            return Detection::missing(
                Source::Panel,
                "candidate region does not match minimap proportions",
            );
        }
        Detection::found(
            MinimapReading { bounds: panel },
            Confidence::new(0.55),
            Source::Panel,
            Reliability::Heuristic,
        )
    }
}

#[derive(Debug, Clone)]
pub struct ChatLogReading {
    pub bounds: Rect,
}

/// Detects the chat log as a dense block of text pixels in the lower-left
/// quadrant of the frame.
#[derive(Debug, Default, Clone, Copy)]
pub struct ChatLogDetector;

impl ChatLogDetector {
    pub fn detect(&self, image: &RgbaImage) -> Detection<ChatLogReading> {
        let width = image.width();
        let height = image.height();
        let region = Rect {
            x: 0,
            y: height * 3 / 5,
            w: width * 2 / 5,
            h: height * 3 / 10,
        };
        match crate::vision::geometry::find_text_block(image, region) {
            Some(bounds) if bounds.area() >= 400 => Detection::found(
                ChatLogReading { bounds },
                Confidence::new(0.5),
                Source::Panel,
                Reliability::Heuristic,
            ),
            _ => Detection::missing(
                Source::Panel,
                "no dense text block found in chat log region",
            ),
        }
    }
}

/// A single small icon detected in a buff/cooldown row.
#[derive(Debug, Clone, Copy)]
pub struct IconSlot {
    pub bounds: Rect,
}

#[derive(Debug, Clone)]
pub struct IconRowReading {
    pub icons: Vec<IconSlot>,
}

/// Detects small, roughly-square, saturated icon glyphs arranged in a row
/// near the top-right of the frame (buff/cooldown icon rows in MapleStory).
///
/// Limitation: this identifies *that* icons are present and how many, using
/// generic "small saturated blob" heuristics; it cannot identify *which*
/// buff/skill each icon represents without a trained icon classifier, which
/// this project does not have. See `docs/vision-architecture.md` for the
/// documented extension point.
#[derive(Debug, Clone, Copy)]
pub struct IconRowDetector {
    pub min_saturation: f32,
    pub min_value: f32,
}

impl Default for IconRowDetector {
    fn default() -> Self {
        Self {
            min_saturation: 0.35,
            min_value: 0.45,
        }
    }
}

impl IconRowDetector {
    pub fn detect(&self, image: &RgbaImage) -> Detection<IconRowReading> {
        let width = image.width();
        let height = image.height();
        if width < 32 || height < 32 {
            return Detection::missing(Source::Panel, "frame too small for icon-row search");
        }
        let region = Rect {
            x: width * 3 / 5,
            y: 0,
            w: width * 2 / 5,
            h: height / 6,
        };

        let is_icon_pixel = |pixel: &image::Rgba<u8>| {
            let (_, s, v) = crate::util::pixel::hsv_from_rgb(pixel[0], pixel[1], pixel[2]);
            pixel[3] > 200 && s >= self.min_saturation && v >= self.min_value
        };

        let mut rows = Vec::new();
        for y in region.y..region.y.saturating_add(region.h).min(height) {
            for (x0, x1) in segment_row(
                image,
                y,
                region.x,
                region.x.saturating_add(region.w).saturating_sub(1),
                8,
                is_icon_pixel,
            ) {
                rows.push((y, x0, x1));
            }
        }
        let blobs: Vec<Rect> = group_segments(rows, 8, 1)
            .into_iter()
            .filter(|rect| rect.w <= 64 && rect.h <= 64 && rect.w >= 8 && rect.h >= 8)
            .collect();

        if blobs.is_empty() {
            return Detection::missing(Source::Panel, "no icon-sized saturated blobs found");
        }

        let icons: Vec<IconSlot> = blobs
            .into_iter()
            .map(|bounds| IconSlot { bounds })
            .collect();
        let confidence = Confidence::new((0.35 + 0.08 * icons.len() as f32).min(0.8));
        Detection::found(
            IconRowReading { icons },
            confidence,
            Source::Panel,
            Reliability::Heuristic,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    #[test]
    fn minimap_detector_rejects_empty_region() {
        let image = RgbaImage::from_pixel(600, 400, Rgba([5, 5, 5, 255]));
        let detection = MinimapDetector.detect(&image);
        // Uniform background with no panel should not be detected
        assert!(!detection.is_present());
    }

    #[test]
    fn icon_row_detector_rejects_empty_row() {
        let image = RgbaImage::from_pixel(600, 400, Rgba([10, 10, 10, 255]));
        let detection = IconRowDetector::default().detect(&image);
        assert!(!detection.is_present());
    }
}
