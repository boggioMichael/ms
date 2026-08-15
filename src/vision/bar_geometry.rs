//! Deterministic HP/MP/EXP bar measurement using filled-pixel geometry.
//!
//! This module replaces OCR-based bar reading with a robust geometric approach:
//! 1. Localize the bar bounds using known HUD layout.
//! 2. Segment filled (colored) and empty (dark) regions within the bar.
//! 3. Measure the filled-pixel ratio to derive the percentage.
//! 4. Calibrate the measurement using bar thickness and edge geometry.
//! 5. Report confidence based on measurement consistency and layout validation.

use image::RgbaImage;

use crate::util::pixel::{Hsv, hsv_from_rgb};

/// A measured bar reading with geometry-derived percentage.
#[derive(Debug, Clone)]
pub struct BarMeasurement {
    /// Bar bounding box: (x, y, width, height).
    pub bounds: (u32, u32, u32, u32),
    /// Filled pixel count.
    pub filled_pixels: u32,
    /// Total bar pixels actually sampled, i.e. the requested region clamped
    /// to the image bounds.
    pub total_pixels: u32,
    /// Estimated percentage: filled_pixels / total_pixels.
    pub percent: f32,
    /// Confidence derived from measurement consistency.
    pub confidence: f32,
}

/// Configuration for bar detection and measurement.
#[derive(Debug, Clone)]
pub struct BarConfig {
    /// Expected bar height in pixels.
    pub expected_height: u32,
    /// Hue range for filled bar color (in degrees 0-360).
    pub filled_hue_min: f32,
    pub filled_hue_max: f32,
    /// Saturation threshold for filled color.
    pub filled_sat_min: f32,
    /// Value/brightness threshold for filled color.
    pub filled_val_min: f32,
    /// Dark-background threshold for empty portion.
    pub empty_val_max: f32,
}

/// Default HP bar configuration (red bar).
pub fn hp_config() -> BarConfig {
    BarConfig {
        expected_height: 10,
        filled_hue_min: 350.0, // Red
        filled_hue_max: 10.0,
        filled_sat_min: 0.3,
        filled_val_min: 0.4,
        empty_val_max: 0.15,
    }
}

/// Default MP bar configuration (blue bar).
pub fn mp_config() -> BarConfig {
    BarConfig {
        expected_height: 10,
        filled_hue_min: 200.0, // Blue
        filled_hue_max: 250.0,
        filled_sat_min: 0.3,
        filled_val_min: 0.4,
        empty_val_max: 0.15,
    }
}

/// Default EXP bar configuration (yellow bar).
pub fn exp_config() -> BarConfig {
    BarConfig {
        expected_height: 10,
        filled_hue_min: 40.0, // Yellow
        filled_hue_max: 60.0,
        filled_sat_min: 0.3,
        filled_val_min: 0.4,
        empty_val_max: 0.15,
    }
}

/// Convert RGBA to HSV for color-based bar segmentation.
///
/// Adapter over the crate's single HSV implementation in [`crate::util::pixel`];
/// alpha is ignored. This module used to carry its own copy of the conversion,
/// which meant two implementations that could drift apart independently.
fn rgba_to_hsv(rgba: image::Rgba<u8>) -> Hsv {
    hsv_from_rgb(rgba[0], rgba[1], rgba[2])
}

/// Check if a pixel matches the filled bar color using HSV thresholds.
fn is_filled_pixel(rgba: image::Rgba<u8>, config: &BarConfig) -> bool {
    let (h, s, v) = rgba_to_hsv(rgba);

    // Check if hue is in range (account for wrap-around at 0/360).
    let hue_match = if config.filled_hue_min <= config.filled_hue_max {
        h >= config.filled_hue_min && h <= config.filled_hue_max
    } else {
        h >= config.filled_hue_min || h <= config.filled_hue_max
    };

    hue_match && s >= config.filled_sat_min && v >= config.filled_val_min
}

/// Check if a pixel is an empty (dark) bar background.
fn is_empty_pixel(rgba: image::Rgba<u8>, config: &BarConfig) -> bool {
    let (_, _, v) = rgba_to_hsv(rgba);
    v <= config.empty_val_max
}

/// Measure a single horizontal bar within the given bounds.
///
/// This function:
/// 1. Scans pixels horizontally across the bar region.
/// 2. Counts filled and empty pixels.
/// 3. Calculates the filled-pixel ratio.
/// 4. Derives confidence from measurement consistency.
pub fn measure_bar(
    image: &RgbaImage,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    config: &BarConfig,
) -> BarMeasurement {
    let mut filled_pixels = 0u32;
    let mut empty_pixels = 0u32;

    // Clamp the region to the image. Pixels outside it cannot be sampled, so
    // counting them in the denominator would bias `percent` low for any bar
    // that runs past an image edge.
    let x_end = x.saturating_add(width).min(image.width());
    let y_end = y.saturating_add(height).min(image.height());

    // Scan the bar region to count filled and empty pixels.
    for row in y..y_end {
        for col in x..x_end {
            let pixel = *image.get_pixel(col, row);

            if is_filled_pixel(pixel, config) {
                filled_pixels += 1;
            } else if is_empty_pixel(pixel, config) {
                empty_pixels += 1;
            }
        }
    }

    let total_pixels = x_end
        .saturating_sub(x)
        .saturating_mul(y_end.saturating_sub(y));
    let percent = if total_pixels > 0 {
        (filled_pixels as f32) / (total_pixels as f32) * 100.0
    } else {
        0.0
    };

    // Confidence is high if the bar has a clear filled/empty split,
    // and lower if pixels are ambiguous. An empty region has no evidence
    // either way, so it scores zero rather than NaN from a 0/0 divide.
    let clarity = if total_pixels > 0 {
        (filled_pixels.saturating_add(empty_pixels) as f32) / (total_pixels as f32)
    } else {
        0.0
    };
    let confidence = clarity * (1.0 - (percent.abs() - 50.0).abs() / 100.0).max(0.1);

    BarMeasurement {
        bounds: (x, y, width, height),
        filled_pixels,
        total_pixels,
        percent,
        confidence,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hsv_conversion_white() {
        let (_, s, v) = rgba_to_hsv(image::Rgba([255, 255, 255, 255]));
        assert_eq!(s, 0.0);
        assert_eq!(v, 1.0);
    }

    #[test]
    fn hsv_conversion_red() {
        let (h, s, v) = rgba_to_hsv(image::Rgba([255, 0, 0, 255]));
        assert!(!(30.0..=330.0).contains(&h));
        assert!(s > 0.9);
        assert!(v > 0.9);
    }

    #[test]
    fn filled_pixel_detection() {
        let config = hp_config();
        let red = image::Rgba([255, 0, 0, 255]);
        assert!(is_filled_pixel(red, &config));
    }

    #[test]
    fn empty_pixel_detection() {
        let config = hp_config();
        let dark = image::Rgba([20, 20, 20, 255]);
        assert!(is_empty_pixel(dark, &config));
    }

    /// Pins the values the module-local HSV conversion produced before it was
    /// folded into `util::pixel`, so the shared implementation cannot silently
    /// change the bar segmentation thresholds.
    #[test]
    fn hsv_matches_shared_implementation() {
        let cases = [
            ([255u8, 0, 0], (0.0f32, 1.0f32, 1.0f32)),
            ([0, 255, 0], (120.0, 1.0, 1.0)),
            ([0, 0, 255], (240.0, 1.0, 1.0)),
            ([255, 255, 0], (60.0, 1.0, 1.0)),
            ([0, 255, 255], (180.0, 1.0, 1.0)),
            ([255, 0, 255], (300.0, 1.0, 1.0)),
            ([10, 20, 30], (210.0, 2.0 / 3.0, 30.0 / 255.0)),
            ([0, 0, 0], (0.0, 0.0, 0.0)),
            ([255, 255, 255], (0.0, 0.0, 1.0)),
        ];

        for ([r, g, b], expected) in cases {
            let actual = rgba_to_hsv(image::Rgba([r, g, b, 255]));
            assert_eq!(actual, hsv_from_rgb(r, g, b));
            assert!(
                (actual.0 - expected.0).abs() < 1e-3
                    && (actual.1 - expected.1).abs() < 1e-3
                    && (actual.2 - expected.2).abs() < 1e-3,
                "rgb {r},{g},{b} -> {actual:?}, expected {expected:?}"
            );
        }
    }

    #[test]
    fn measure_bar_clamps_region_to_image() {
        let mut image = RgbaImage::new(8, 4);
        for pixel in image.pixels_mut() {
            *pixel = image::Rgba([255, 0, 0, 255]);
        }

        // Ask for a region twice the size of the image in both axes: only the
        // 8x4 pixels that exist may count toward the denominator.
        let measurement = measure_bar(&image, 0, 0, 16, 8, &hp_config());
        assert_eq!(measurement.total_pixels, 32);
        assert_eq!(measurement.filled_pixels, 32);
        assert!((measurement.percent - 100.0).abs() < 1e-3);
    }

    #[test]
    fn measure_bar_outside_image_is_not_nan() {
        let image = RgbaImage::new(4, 4);
        let measurement = measure_bar(&image, 10, 10, 4, 4, &hp_config());
        assert_eq!(measurement.total_pixels, 0);
        assert_eq!(measurement.percent, 0.0);
        assert!(measurement.confidence.is_finite());
    }
}
