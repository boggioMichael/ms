//! Crop debugging helpers: drawing rectangles, labels and saving crops.
//!
//! Designed so detectors can cheaply save crops or annotated images for offline
//! analysis: debug::crop::save_crop("player_hp", &image, x,y,w,h)

use image::{Rgba, RgbaImage};
use std::fs;
use std::path::Path;

/// Draw a simple rectangle (stroke only) onto a mutable RGBA image.
/// Coordinates and sizes are in pixels.
pub fn draw_rect(
    img: &mut RgbaImage,
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    color: Rgba<u8>,
    thickness: u32,
) {
    let (iw, ih) = (img.width(), img.height());
    // draw horizontal lines
    for t in 0..thickness {
        let yy_top = y.saturating_add(t);
        let yy_bottom = y.saturating_add(h.saturating_sub(1)).saturating_sub(t);
        for px in x..(x + w).min(iw) {
            if yy_top < ih {
                img.put_pixel(px, yy_top, color);
            }
            if yy_bottom < ih {
                img.put_pixel(px, yy_bottom, color);
            }
        }
    }
    // draw vertical lines
    for t in 0..thickness {
        let xx_left = x.saturating_add(t);
        let xx_right = x.saturating_add(w.saturating_sub(1)).saturating_sub(t);
        for py in y..(y + h).min(ih) {
            if xx_left < iw {
                img.put_pixel(xx_left, py, color);
            }
            if xx_right < iw {
                img.put_pixel(xx_right, py, color);
            }
        }
    }
}

/// Save a cropped region to disk. Path is created if necessary.
/// Returns the saved path on success.
pub fn save_crop<P: AsRef<Path>>(
    name: &str,
    img: &RgbaImage,
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    out_dir: P,
) -> Result<std::path::PathBuf, image::ImageError> {
    let out_dir = out_dir.as_ref();
    fs::create_dir_all(out_dir).ok();
    let crop = image::imageops::crop_imm(
        img,
        x,
        y,
        w.min(img.width().saturating_sub(x)),
        h.min(img.height().saturating_sub(y)),
    )
    .to_image();
    let mut path = out_dir.join(format!(
        "{}-{}x{}-{}.png",
        name,
        w,
        h,
        chrono::Utc::now().format("%Y%m%d-%H%M%S")
    ));
    // ensure extension
    if path.extension().is_none() {
        path.set_extension("png");
    }
    crop.save(&path)?;
    Ok(path)
}

/// Save an annotated image (non-destructive copy). Useful for overlays.
pub fn save_annotated<P: AsRef<Path>>(
    name: &str,
    img: &RgbaImage,
    out_dir: P,
) -> Result<std::path::PathBuf, image::ImageError> {
    let out_dir = out_dir.as_ref();
    fs::create_dir_all(out_dir).ok();
    let copy = img.clone();
    let path = out_dir.join(format!(
        "annot-{}-{}.png",
        name,
        chrono::Utc::now().format("%Y%m%d-%H%M%S")
    ));
    copy.save(&path)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn draw_rect_runs() {
        let mut img = RgbaImage::from_pixel(100, 100, Rgba([0, 0, 0, 255]));
        draw_rect(&mut img, 10, 10, 50, 20, Rgba([255, 0, 0, 255]), 2);
    }
}
