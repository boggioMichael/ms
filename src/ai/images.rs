//! Pictures for the models: frames scaled down, cropped, marked with
//! rulers so a model can say where something is, and encoded as data URLs
//! for the Responses API's `input_image`.

use image::{Rgba, RgbaImage, imageops};

use crate::observe::font;

/// A box in a frame, as fractions of its width and height (0 to 1, from the
/// top left). Models get and give these as 0 to 1000.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct NBox {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
}

impl NBox {
    pub fn new(x0: f32, y0: f32, x1: f32, y1: f32) -> NBox {
        NBox {
            x0: x0.min(x1).clamp(0.0, 1.0),
            y0: y0.min(y1).clamp(0.0, 1.0),
            x1: x0.max(x1).clamp(0.0, 1.0),
            y1: y0.max(y1).clamp(0.0, 1.0),
        }
    }

    /// From a model's `[x0, y0, x1, y1]` in 0 to 1000. `None` for anything
    /// that is not four numbers making a box of some size.
    pub fn from_thousandths(values: &[f64]) -> Option<NBox> {
        if values.len() != 4 || values.iter().any(|v| !v.is_finite()) {
            return None;
        }
        let b = NBox::new(
            values[0] as f32 / 1000.0,
            values[1] as f32 / 1000.0,
            values[2] as f32 / 1000.0,
            values[3] as f32 / 1000.0,
        );
        (b.width() > 0.0005 && b.height() > 0.0005).then_some(b)
    }

    pub fn to_thousandths(&self) -> [u32; 4] {
        [self.x0, self.y0, self.x1, self.y1].map(|v| (v * 1000.0).round() as u32)
    }

    pub fn width(&self) -> f32 {
        self.x1 - self.x0
    }

    pub fn height(&self) -> f32 {
        self.y1 - self.y0
    }

    pub fn center(&self) -> (f32, f32) {
        ((self.x0 + self.x1) / 2.0, (self.y0 + self.y1) / 2.0)
    }

    /// Grown by `fx` of its width and `fy` of its height on each side.
    pub fn grown(&self, fx: f32, fy: f32) -> NBox {
        let (dx, dy) = (self.width() * fx, self.height() * fy);
        NBox::new(self.x0 - dx, self.y0 - dy, self.x1 + dx, self.y1 + dy)
    }

    /// The smallest box holding both.
    pub fn union(&self, other: &NBox) -> NBox {
        NBox::new(
            self.x0.min(other.x0),
            self.y0.min(other.y0),
            self.x1.max(other.x1),
            self.y1.max(other.y1),
        )
    }

    /// In pixels of a `width`×`height` frame: x, y, w, h (at least 1×1).
    pub fn pixels(&self, width: u32, height: u32) -> (u32, u32, u32, u32) {
        // A hair of slack, so a box made from whole pixels comes back exact.
        let lo = |v: f32, n: u32| {
            ((v * n as f32 + 0.01).floor().max(0.0) as u32).min(n.saturating_sub(1))
        };
        let hi = |v: f32, n: u32| (v * n as f32 - 0.01).ceil().max(0.0) as u32;
        let (x0, y0) = (lo(self.x0, width), lo(self.y0, height));
        let x1 = hi(self.x1, width).clamp(x0 + 1, width.max(1));
        let y1 = hi(self.y1, height).clamp(y0 + 1, height.max(1));
        (x0, y0, x1 - x0, y1 - y0)
    }

    /// A box in pixels of a `width`×`height` frame, as fractions.
    pub fn from_pixels(x: u32, y: u32, w: u32, h: u32, width: u32, height: u32) -> NBox {
        let (fw, fh) = (width.max(1) as f32, height.max(1) as f32);
        NBox::new(
            x as f32 / fw,
            y as f32 / fh,
            (x + w) as f32 / fw,
            (y + h) as f32 / fh,
        )
    }

    /// This box (given in fractions of `inner`, which sits at `inner` within
    /// a larger frame) in fractions of the larger frame.
    pub fn within(&self, inner: &NBox) -> NBox {
        NBox::new(
            inner.x0 + self.x0 * inner.width(),
            inner.y0 + self.y0 * inner.height(),
            inner.x0 + self.x1 * inner.width(),
            inner.y0 + self.y1 * inner.height(),
        )
    }
}

/// The part of `image` in `b`.
pub fn crop(image: &RgbaImage, b: &NBox) -> RgbaImage {
    let (x, y, w, h) = b.pixels(image.width(), image.height());
    imageops::crop_imm(image, x, y, w, h).to_image()
}

/// `image` no wider than `width` (and no taller than `height`), keeping
/// its shape.
pub fn fit(image: &RgbaImage, width: u32, height: u32) -> RgbaImage {
    let (w, h) = image.dimensions();
    let scale = (width as f32 / w.max(1) as f32)
        .min(height as f32 / h.max(1) as f32)
        .min(1.0);
    if scale >= 0.999 {
        return image.clone();
    }
    let (nw, nh) = (
        ((w as f32 * scale).round() as u32).max(1),
        ((h as f32 * scale).round() as u32).max(1),
    );
    imageops::resize(image, nw, nh, imageops::FilterType::Triangle)
}

/// `image` blown up `times` over (pixels kept sharp), for small text.
pub fn enlarged(image: &RgbaImage, times: u32) -> RgbaImage {
    let (w, h) = image.dimensions();
    imageops::resize(
        image,
        w * times.max(1),
        h * times.max(1),
        imageops::FilterType::CatmullRom,
    )
}

/// How wide the ruler margin is around a marked picture.
const MARGIN: u32 = 22;

/// `image` with rulers along its edges and a faint grid, numbered 0 to 1000
/// across the picture (the margin is outside it), so a model can say where
/// things are.
pub fn with_rulers(image: &RgbaImage) -> RgbaImage {
    let (w, h) = image.dimensions();
    let mut out = RgbaImage::from_pixel(w + 2 * MARGIN, h + 2 * MARGIN, Rgba([255, 255, 255, 255]));
    imageops::replace(&mut out, image, MARGIN as i64, MARGIN as i64);
    let ink = Rgba([20, 20, 20, 255]);
    let plot = |out: &mut RgbaImage, x: i64, y: i64, color: Rgba<u8>| {
        if x >= 0 && y >= 0 && (x as u32) < out.width() && (y as u32) < out.height() {
            out.put_pixel(x as u32, y as u32, color);
        }
    };
    // A faint grid every 100, so the middle of the picture has marks too.
    for step in 1..10 {
        let gx = MARGIN + (w * step) / 10;
        let gy = MARGIN + (h * step) / 10;
        for y in MARGIN..MARGIN + h {
            let p = out.get_pixel(gx, y).0;
            let faint = Rgba([
                ((p[0] as u16 * 2 + 255) / 3) as u8,
                ((p[1] as u16 * 2 + 255) / 3) as u8,
                ((p[2] as u16 * 2 + 255) / 3) as u8,
                255,
            ]);
            if y % 6 < 3 {
                out.put_pixel(gx, y, faint);
            }
        }
        for x in MARGIN..MARGIN + w {
            let p = out.get_pixel(x, gy).0;
            let faint = Rgba([
                ((p[0] as u16 * 2 + 255) / 3) as u8,
                ((p[1] as u16 * 2 + 255) / 3) as u8,
                ((p[2] as u16 * 2 + 255) / 3) as u8,
                255,
            ]);
            if x % 6 < 3 {
                out.put_pixel(x, gy, faint);
            }
        }
    }
    // Ticks every 50, numbers every 100, on all four sides.
    for step in 0..=20u32 {
        let fx = MARGIN as i64 + ((w.saturating_sub(1)) * step / 20) as i64;
        let fy = MARGIN as i64 + ((h.saturating_sub(1)) * step / 20) as i64;
        let long = step % 2 == 0;
        let len = if long { 7 } else { 4 };
        for d in 0..len {
            plot(&mut out, fx, MARGIN as i64 - 1 - d, ink);
            plot(&mut out, fx, (MARGIN + h) as i64 + d, ink);
            plot(&mut out, MARGIN as i64 - 1 - d, fy, ink);
            plot(&mut out, (MARGIN + w) as i64 + d, fy, ink);
        }
        if long {
            let label = (step * 50).to_string();
            let tw = font::text_width(&label, 1) as i64;
            // Top and bottom.
            font::draw_text(&label, fx - tw / 2, 2, 1, |x, y| plot(&mut out, x, y, ink));
            font::draw_text(&label, fx - tw / 2, (MARGIN + h) as i64 + 10, 1, |x, y| {
                plot(&mut out, x, y, ink)
            });
            // Left and right (beside the tick).
            font::draw_text(&label, 0, fy - 10, 1, |x, y| plot(&mut out, x, y, ink));
            font::draw_text(&label, (MARGIN + w) as i64 + 1, fy - 10, 1, |x, y| {
                plot(&mut out, x, y, ink)
            });
        }
    }
    out
}

/// Bytes as base64 (standard alphabet, padded).
pub fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = (chunk[0] as u32) << 16
            | (*chunk.get(1).unwrap_or(&0) as u32) << 8
            | *chunk.get(2).unwrap_or(&0) as u32;
        out.push(ALPHABET[(n >> 18) as usize & 63] as char);
        out.push(ALPHABET[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

/// A PNG data URL: sharp, for text and small crops.
pub fn png_url(image: &RgbaImage) -> String {
    let mut bytes = Vec::new();
    let encoder = image::codecs::png::PngEncoder::new(&mut bytes);
    let _ = image::ImageEncoder::write_image(
        encoder,
        image.as_raw(),
        image.width(),
        image.height(),
        image::ColorType::Rgba8,
    );
    format!("data:image/png;base64,{}", base64(&bytes))
}

/// A JPEG data URL: small, for whole frames.
pub fn jpeg_url(image: &RgbaImage, quality: u8) -> String {
    let rgb = image::DynamicImage::ImageRgba8(image.clone()).to_rgb8();
    let mut bytes = Vec::new();
    let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, quality);
    let _ = encoder.encode(
        rgb.as_raw(),
        rgb.width(),
        rgb.height(),
        image::ColorType::Rgb8,
    );
    format!("data:image/jpeg;base64,{}", base64(&bytes))
}

/// An `input_image` part for the Responses API.
pub fn input_image(url: String, detail: &str) -> serde_json::Value {
    serde_json::json!({"type": "input_image", "image_url": url, "detail": detail})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_the_standard() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
        assert_eq!(base64(&[0xff, 0xfe, 0x00]), "//4A");
    }

    #[test]
    fn boxes_go_between_fractions_pixels_and_thousandths() {
        let b = NBox::from_thousandths(&[100.0, 900.0, 300.0, 950.0]).unwrap();
        assert_eq!(b.pixels(1000, 500), (100, 450, 200, 25));
        assert_eq!(b.to_thousandths(), [100, 900, 300, 950]);
        // Corners in any order; out of range is clamped; empty is refused.
        let flipped = NBox::from_thousandths(&[300.0, 950.0, 100.0, 1200.0]).unwrap();
        assert_eq!(flipped.to_thousandths(), [100, 950, 300, 1000]);
        assert!(NBox::from_thousandths(&[5.0, 5.0, 5.0, 9.0]).is_none());
        assert!(NBox::from_thousandths(&[1.0, 2.0, 3.0]).is_none());
        // A box found in a crop, placed back in the frame.
        let crop = NBox::new(0.5, 0.5, 1.0, 1.0);
        let inside = NBox::new(0.0, 0.5, 0.5, 1.0).within(&crop);
        assert_eq!(inside, NBox::new(0.5, 0.75, 0.75, 1.0));
    }

    #[test]
    fn pictures_encode_and_rulers_frame_them() {
        let image = RgbaImage::from_pixel(200, 100, Rgba([10, 120, 200, 255]));
        let marked = with_rulers(&image);
        assert_eq!(marked.dimensions(), (200 + 2 * MARGIN, 100 + 2 * MARGIN));
        // The picture itself is where it was, apart from the faint grid.
        assert_eq!(
            marked.get_pixel(MARGIN + 3, MARGIN + 3).0,
            [10, 120, 200, 255]
        );
        assert!(png_url(&image).starts_with("data:image/png;base64,iVBOR"));
        assert!(jpeg_url(&image, 80).starts_with("data:image/jpeg;base64,/9j/"));
        assert_eq!(fit(&image, 100, 100).dimensions(), (100, 50));
        assert_eq!(fit(&image, 400, 400).dimensions(), (200, 100));
        assert_eq!(
            crop(&image, &NBox::new(0.5, 0.0, 1.0, 0.5)).dimensions(),
            (100, 50)
        );
    }
}
