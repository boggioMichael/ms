//! Judging whether a captured region is sharp enough for text recognition.
//!
//! A blurred capture and a crisp one look alike to the rest of the pipeline:
//! OCR simply returns wrong text, and every downstream layer faithfully
//! reports the wrong answer. That failure is expensive to diagnose by hand —
//! it was reached here only after testing two OCR engines across a dozen
//! preprocessing variants before the input itself turned out to be at fault.
//!
//! So the engine measures its own input. Game HUD text is a pixel font:
//! rendered natively, a glyph edge is a single-pixel jump from background to
//! full brightness. Rescaling or video compression averages those jumps into
//! ramps, and once the ramp is wider than the stroke the glyph's identity is
//! gone — no recogniser can recover it.
//!
//! ```text
//!   native      blurred
//!   ▁▁███▁▁     ▁▂▅██▅▂▁
//!   hard edge   ramp: information lost
//! ```
//!
//! The metric is the share of strong luminance steps that are *abrupt*.

use image::RgbaImage;

use crate::vision::geometry::Rect;

/// Luminance step small enough to be noise or a gradient background rather
/// than a glyph edge; steps below this are ignored entirely.
const NOISE_STEP: i32 = 18;

/// Luminance step large enough to be a real pixel-font glyph edge.
const HARD_STEP: i32 = 70;

/// Below this share of hard edges, text is too smeared to recognise. Chosen
/// from measurements on real captures: a native window capture of terminal
/// text scores far above it, while the rescaled fixture scores far below.
pub const LEGIBLE_SHARPNESS: f32 = 0.35;

/// How readable a region's text is likely to be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Legibility {
    /// Edges are crisp; OCR has a fair chance.
    Sharp,
    /// Edges are ramps; OCR will return plausible-looking wrong text.
    Blurred,
    /// Nothing with enough contrast to judge.
    NoText,
}

/// The result of assessing a region.
#[derive(Debug, Clone, Copy)]
pub struct TextQuality {
    /// Share of strong luminance steps that are abrupt, in `[0, 1]`.
    pub sharpness: f32,
    /// How many strong steps were found; a handful means low confidence.
    pub edge_samples: u32,
    pub legibility: Legibility,
}

impl TextQuality {
    /// Whether OCR output from this region should be trusted at all.
    pub fn is_legible(&self) -> bool {
        matches!(self.legibility, Legibility::Sharp)
    }
}

fn luminance(pixel: &image::Rgba<u8>) -> i32 {
    // Integer approximation of Rec. 601 luma; exactness is irrelevant here
    // because only the size of differences matters.
    (pixel[0] as i32 * 299 + pixel[1] as i32 * 587 + pixel[2] as i32 * 114) / 1000
}

/// Measure how sharply text edges are rendered inside `region`.
///
/// Scans horizontally, because glyph stems produce vertical edges and those
/// are what a horizontal scan crosses.
pub fn assess_text_quality(image: &RgbaImage, region: Rect) -> TextQuality {
    let x_end = region.x.saturating_add(region.w).min(image.width());
    let y_end = region.y.saturating_add(region.h).min(image.height());

    let mut strong = 0u32;
    let mut hard = 0u32;

    for y in region.y..y_end {
        for x in region.x..x_end.saturating_sub(1) {
            let step =
                (luminance(image.get_pixel(x + 1, y)) - luminance(image.get_pixel(x, y))).abs();
            if step < NOISE_STEP {
                continue;
            }
            strong += 1;
            if step >= HARD_STEP {
                hard += 1;
            }
        }
    }

    // Too little contrast anywhere to call it text.
    if strong < 24 {
        return TextQuality {
            sharpness: 0.0,
            edge_samples: strong,
            legibility: Legibility::NoText,
        };
    }

    let sharpness = hard as f32 / strong as f32;
    let legibility = if sharpness >= LEGIBLE_SHARPNESS {
        Legibility::Sharp
    } else {
        Legibility::Blurred
    };

    TextQuality {
        sharpness,
        edge_samples: strong,
        legibility,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    fn region(width: u32, height: u32) -> Rect {
        Rect {
            x: 0,
            y: 0,
            w: width,
            h: height,
        }
    }

    /// Hard-edged stripes: a pixel font rendered natively.
    fn crisp_text() -> RgbaImage {
        let mut image = RgbaImage::from_pixel(80, 20, Rgba([10, 10, 10, 255]));
        for x in 0..80u32 {
            if (x / 3).is_multiple_of(2) {
                for y in 4..16 {
                    image.put_pixel(x, y, Rgba([240, 240, 240, 255]));
                }
            }
        }
        image
    }

    /// The same stripes with every edge spread into a gradual ramp, which is
    /// what rescaling a screenshot does. Step sizes here match what the
    /// rescaled fixture actually exhibits: strong enough to register as
    /// edges, too gradual to be glyph boundaries.
    fn blurred_text() -> RgbaImage {
        let mut image = RgbaImage::from_pixel(80, 20, Rgba([10, 10, 10, 255]));
        for x in 0..80u32 {
            let level = match x % 8 {
                0 => 40u8,
                1 => 85,
                2 => 130,
                3 => 175,
                4 => 220,
                5 => 175,
                6 => 130,
                _ => 85,
            };
            for y in 4..16 {
                image.put_pixel(x, y, Rgba([level, level, level, 255]));
            }
        }
        image
    }

    #[test]
    fn native_pixel_text_reads_as_sharp() {
        let quality = assess_text_quality(&crisp_text(), region(80, 20));
        assert_eq!(quality.legibility, Legibility::Sharp);
        assert!(quality.is_legible());
        assert!(quality.sharpness > 0.9, "got {}", quality.sharpness);
    }

    #[test]
    fn rescaled_text_reads_as_blurred() {
        let quality = assess_text_quality(&blurred_text(), region(80, 20));
        assert_eq!(quality.legibility, Legibility::Blurred);
        assert!(!quality.is_legible());
        assert!(
            quality.sharpness < LEGIBLE_SHARPNESS,
            "got {}",
            quality.sharpness
        );
    }

    #[test]
    fn a_flat_region_is_reported_as_having_no_text() {
        let flat = RgbaImage::from_pixel(60, 20, Rgba([30, 30, 30, 255]));
        let quality = assess_text_quality(&flat, region(60, 20));
        assert_eq!(quality.legibility, Legibility::NoText);
        // "No text" must not masquerade as good input.
        assert!(!quality.is_legible());
    }

    #[test]
    fn assessment_is_clipped_to_the_image() {
        // A region running past the edge must not panic.
        let image = crisp_text();
        let quality = assess_text_quality(&image, region(500, 500));
        assert!(quality.edge_samples > 0);
    }
}
