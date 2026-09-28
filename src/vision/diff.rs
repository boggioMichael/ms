//! Frame difference utilities
//!
//! Provides pixel-wise absolute difference and a simple motion mask.

use image::{GrayImage, Luma, RgbaImage};

/// Compute absolute per-channel difference between two RGBA images of the same size.
/// Returns a new RGBA image where each channel is abs(a-b).
pub fn abs_diff_rgba(a: &RgbaImage, b: &RgbaImage) -> Option<RgbaImage> {
    if a.dimensions() != b.dimensions() {
        return None;
    }
    let (w, h) = a.dimensions();
    let mut out = RgbaImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let pa = a.get_pixel(x, y);
            let pb = b.get_pixel(x, y);
            out.put_pixel(
                x,
                y,
                image::Rgba([
                    pa[0].abs_diff(pb[0]),
                    pa[1].abs_diff(pb[1]),
                    pa[2].abs_diff(pb[2]),
                    255u8,
                ]),
            );
        }
    }
    Some(out)
}

/// Produce a grayscale motion mask by converting absolute diff to luminance and
/// thresholding. Returns None if sizes mismatch.
pub fn motion_mask(a: &RgbaImage, b: &RgbaImage, threshold: u8) -> Option<GrayImage> {
    abs_diff_rgba(a, b).map(|diff| {
        let (w, h) = diff.dimensions();
        let mut out = GrayImage::new(w, h);
        for y in 0..h {
            for x in 0..w {
                let p = diff.get_pixel(x, y);
                let lum =
                    (0.2126 * p[0] as f32 + 0.7152 * p[1] as f32 + 0.0722 * p[2] as f32) as u8;
                out.put_pixel(x, y, Luma([(if lum >= threshold { 255 } else { 0 }) as u8]));
            }
        }
        out
    })
}
