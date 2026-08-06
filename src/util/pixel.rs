//! Pixel inspection helpers
//!
//! Functions operate on a borrowed Frame (RGBA8) and return common
//! color space values. Implemented with minimal allocations so detectors
//! can call them in hot paths.

use crate::frame::Frame;

/// RGB tuple
pub type Rgb = (u8, u8, u8);
/// RGBA tuple
pub type RgbaT = (u8, u8, u8, u8);
/// HSV tuple with H in [0,360), S and V in [0,1]
pub type Hsv = (f32, f32, f32);

fn clamp_f32(v: f32, lo: f32, hi: f32) -> f32 {
    if v < lo {
        lo
    } else if v > hi {
        hi
    } else {
        v
    }
}

/// Read raw RGBA at integer coordinates. Returns None if out of bounds.
pub fn rgba_at(frame: &Frame<'_>, x: u32, y: u32) -> Option<RgbaT> {
    if x >= frame.width() || y >= frame.height() {
        return None;
    }
    let px = frame.image.get_pixel(x, y);
    Some((px[0], px[1], px[2], px[3]))
}

/// Read RGB at integer coordinates (drops alpha)
pub fn rgb_at(frame: &Frame<'_>, x: u32, y: u32) -> Option<Rgb> {
    rgba_at(frame, x, y).map(|(r, g, b, _)| (r, g, b))
}

/// Brightness computed as luminance using Rec. 709 coefficients in [0,1]
pub fn brightness_at(frame: &Frame<'_>, x: u32, y: u32) -> Option<f32> {
    rgb_at(frame, x, y).map(|(r, g, b)| {
        let r = r as f32 / 255.0;
        let g = g as f32 / 255.0;
        let b = b as f32 / 255.0;
        0.2126 * r + 0.7152 * g + 0.0722 * b
    })
}

/// Convert RGB to HSV. H in degrees [0,360), S and V in [0,1]
pub fn hsv_from_rgb(r: u8, g: u8, b: u8) -> Hsv {
    let rf = r as f32 / 255.0;
    let gf = g as f32 / 255.0;
    let bf = b as f32 / 255.0;
    let max = rf.max(gf).max(bf);
    let min = rf.min(gf).min(bf);
    let delta = max - min;
    let h = if delta == 0.0 {
        0.0
    } else if max == rf {
        60.0 * ((gf - bf) / delta % 6.0)
    } else if max == gf {
        60.0 * ((bf - rf) / delta + 2.0)
    } else {
        60.0 * ((rf - gf) / delta + 4.0)
    };
    let h = if h < 0.0 { h + 360.0 } else { h };
    let s = if max == 0.0 { 0.0 } else { delta / max };
    (h, clamp_f32(s, 0.0, 1.0), clamp_f32(max, 0.0, 1.0))
}

/// HSV at pixel coordinates
pub fn hsv_at(frame: &Frame<'_>, x: u32, y: u32) -> Option<Hsv> {
    rgb_at(frame, x, y).map(|(r, g, b)| hsv_from_rgb(r, g, b))
}

/// Utility: log pixel values when enabled by the debug config
pub fn log_pixel(frame: &Frame<'_>, x: u32, y: u32) {
    if crate::config::get_global().log_pixel_values {
        if let Some((r, g, b, a)) = rgba_at(frame, x, y) {
            tracing::debug!(x, y, r, g, b, a, "pixel");
        }
    }
}
