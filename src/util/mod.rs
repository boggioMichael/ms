//! General-purpose utilities shared across the crate.
//!
//! - `timing`: `ScopedTimer`, `FrameTimer`, `FPSCounter`, `MovingAverage`.
//! - `pixel`: RGB/RGBA/HSV/brightness pixel helpers operating on borrowed frames.
//! - `image_ops`: rectangle drawing and crop/annotation saving helpers.

pub mod image_ops;
pub mod pixel;
pub mod timing;

pub use image_ops::{draw_rect, save_annotated, save_crop};
pub use pixel::{Hsv, Rgb, RgbaT, brightness_at, hsv_at, hsv_from_rgb, log_pixel, rgb_at, rgba_at};
pub use timing::{FPSCounter, FrameTimer, MovingAverage, ScopedTimer};

/// Convert u8 alpha-premultiplied RGBA to non-premultiplied RGB bytes in place.
/// This operates on a slice of bytes in RGBA order. Minimal allocations.
pub fn unpremultiply_rgba_inplace(buf: &mut [u8]) {
    for chunk in buf.chunks_exact_mut(4) {
        let a = chunk[3] as f32 / 255.0;
        if a == 0.0 {
            continue;
        }
        for c in 0..3 {
            let v = chunk[c] as f32 / a;
            chunk[c] = v.min(255.0).max(0.0) as u8;
        }
    }
}

