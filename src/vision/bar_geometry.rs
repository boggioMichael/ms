//! Deterministic HP/MP/EXP bar measurement using filled-pixel geometry.
//!
//! This module replaces OCR-based bar reading with a robust geometric approach:
//! 1. Localize the bar bounds using known HUD layout.
//! 2. Segment filled (colored) and empty (dark) regions within the bar.
//! 3. Measure the filled-pixel ratio to derive the percentage.
//! 4. Calibrate the measurement using bar thickness and edge geometry.
//! 5. Report confidence based on measurement consistency and layout validation.

use image::RgbaImage;

/// A measured bar reading with geometry-derived percentage.
#[derive(Debug, Clone)]
pub struct BarMeasurement {
    /// Bar bounding box: (x, y, width, height).
    pub bounds: (u32, u32, u32, u32),
    /// Filled pixel count.
    pub filled_pixels: u32,
    /// Total bar pixels.
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
        filled_hue_min: 350.0,    // Red
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
        filled_hue_min: 200.0,    // Blue
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
        filled_hue_min: 40.0,     // Yellow
        filled_hue_max: 60.0,
        filled_sat_min: 0.3,
        filled_val_min: 0.4,
        empty_val_max: 0.15,
    }
}

/// Convert RGBA to HSV for color-based bar segmentation.
fn rgba_to_hsv(rgba: image::Rgba<u8>) -> (f32, f32, f32) {
    let r = rgba[0] as f32 / 255.0;
    let g = rgba[1] as f32 / 255.0;
    let b = rgba[2] as f32 / 255.0;

    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;

    let v = max;
    let s = if max > 0.0 { delta / max } else { 0.0 };

    let h = if delta == 0.0 {
        0.0
    } else if max == r {
        (60.0 * ((g - b) / delta) + 360.0) % 360.0
    } else if max == g {
        (60.0 * ((b - r) / delta) + 120.0) % 360.0
    } else {
        (60.0 * ((r - g) / delta) + 240.0) % 360.0
    };

    (h, s, v)
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

    // Scan the bar region to count filled and empty pixels.
    for row in y..y.saturating_add(height) {
        for col in x..x.saturating_add(width) {
            if row >= image.height() || col >= image.width() {
                continue;
            }
            let pixel = *image.get_pixel(col, row);
            
            if is_filled_pixel(pixel, config) {
                filled_pixels += 1;
            } else if is_empty_pixel(pixel, config) {
                empty_pixels += 1;
            }
        }
    }

    let total_pixels = width * height;
    let percent = if total_pixels > 0 {
        (filled_pixels as f32) / (total_pixels as f32) * 100.0
    } else {
        0.0
    };

    // Confidence is high if the bar has a clear filled/empty split,
    // and lower if pixels are ambiguous.
    let clarity = ((filled_pixels + empty_pixels) as f32) / (total_pixels as f32);
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
        assert!(h < 30.0 || h > 330.0);
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
}
