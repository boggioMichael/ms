//! Text recognition via the OCR engine built into Windows.
//!
//! Tesseract is trained on document typefaces and fails on game HUD text:
//! on a real MapleStory frame it returned `6201-1` for a plainly visible
//! `1291/1351`, and no combination of page-segmentation mode, whitelist,
//! inversion, binarisation or upscaling moved it. The Windows engine is
//! aimed at screen and UI content instead, ships with the OS, needs no
//! model files or external binary, and reaches us through the `windows`
//! crate this project already depends on — so it costs no new deployment
//! surface.
//!
//! This module is deliberately just "pixels in, text out"; choosing which
//! engine to use and what to do with the text belongs to
//! [`crate::vision::ocr`].

#![cfg(target_os = "windows")]

use image::RgbaImage;
use windows::Graphics::Imaging::{BitmapAlphaMode, BitmapPixelFormat, SoftwareBitmap};
use windows::Media::Ocr::OcrEngine;
use windows::Storage::Streams::{DataWriter, IBuffer};

/// Whether the Windows OCR engine is usable on this machine.
///
/// It depends on an installed language pack, so absence is a normal
/// configuration rather than an error.
pub fn is_available() -> bool {
    engine().is_some()
}

fn engine() -> Option<OcrEngine> {
    OcrEngine::TryCreateFromUserProfileLanguages().ok()
}

/// Recognise the text in `image`, returning it as a single line.
///
/// Returns `None` when the engine is unavailable or recognises nothing, so
/// callers can distinguish "no engine" and "no text" from a bad read.
pub fn recognize(image: &RgbaImage) -> Option<String> {
    let engine = engine()?;
    let bitmap = to_software_bitmap(image)?;
    let result = engine.RecognizeAsync(&bitmap).ok()?.get().ok()?;
    let text = result.Text().ok()?.to_string();
    let trimmed = text.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

/// Wrap RGBA pixels in the BGRA8 `SoftwareBitmap` the engine expects.
fn to_software_bitmap(image: &RgbaImage) -> Option<SoftwareBitmap> {
    let (width, height) = (image.width(), image.height());
    if width == 0 || height == 0 {
        return None;
    }

    // The engine wants BGRA; the crate's images are RGBA.
    let mut bgra = Vec::with_capacity((width as usize) * (height as usize) * 4);
    for pixel in image.pixels() {
        bgra.extend_from_slice(&[pixel[2], pixel[1], pixel[0], 255]);
    }

    let buffer = to_buffer(&bgra)?;
    SoftwareBitmap::CreateCopyFromBuffer(
        &buffer,
        BitmapPixelFormat::Bgra8,
        width as i32,
        height as i32,
    )
    .ok()
    .and_then(|bitmap| {
        // Recognition rejects straight alpha; the pixels are opaque anyway.
        SoftwareBitmap::ConvertWithAlpha(
            &bitmap,
            BitmapPixelFormat::Bgra8,
            BitmapAlphaMode::Premultiplied,
        )
        .ok()
    })
}

fn to_buffer(bytes: &[u8]) -> Option<IBuffer> {
    let writer = DataWriter::new().ok()?;
    writer.WriteBytes(bytes).ok()?;
    writer.DetachBuffer().ok()
}
