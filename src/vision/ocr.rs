//! OCR helpers for reading HUD text and numeric values.
//!
//! This module uses the free Tesseract OCR engine when available.
//! It is intentionally isolated so detector authors can swap to another OCR
//! backend later without changing the higher-level HUD logic.

use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use image::{DynamicImage, RgbaImage};

/// OCR configuration for a single crop.
#[derive(Debug, Clone)]
pub struct OcrConfig {
    /// Page segmentation mode passed to Tesseract.
    pub psm: u8,
    /// Optional whitelist of characters to prefer.
    pub whitelist: Option<String>,
}

impl Default for OcrConfig {
    fn default() -> Self {
        Self {
            // PSM 0 only performs orientation detection and never recognizes HUD text.
            // Sparse text handles game HUDs where labels and values are separated.
            psm: 11,
            whitelist: None,
        }
    }
}

impl OcrConfig {
    /// Option flags only, without the leading positional arguments or the
    /// trailing config-file name.
    fn to_args(&self) -> Vec<String> {
        let mut args = vec![
            "--oem".to_string(),
            "3".to_string(),
            "--psm".to_string(),
            self.psm.to_string(),
        ];
        if let Some(whitelist) = &self.whitelist {
            args.push("-c".to_string());
            args.push(format!("tessedit_char_whitelist={whitelist}"));
        }

        args.push("-c".to_string());
        args.push("preserve_interword_spaces=1".to_string());
        args
    }
}

/// Build the full Tesseract command line for one crop.
///
/// Tesseract's grammar is `tesseract IMAGE OUTPUTBASE [options...] [configfile...]`
/// and its parser stops reading options at the first non-flag argument that
/// follows the two positional ones. Putting the `tsv` config file before
/// `--oem`/`--psm`/`-c` makes Tesseract treat every flag as a config file name
/// ("read_params_file: Can't open --oem") and silently fall back to its default
/// page segmentation mode, so `tsv` has to come last.
fn tesseract_args(input: &Path, config: &OcrConfig) -> Vec<OsString> {
    let mut args = vec![input.as_os_str().to_os_string(), OsString::from("stdout")];
    args.extend(config.to_args().into_iter().map(OsString::from));
    args.push(OsString::from("tsv"));
    args
}

/// Owns a temporary OCR input image and deletes it on drop.
///
/// The Tesseract call has several fallible steps after the PNG is written; an
/// early return from any of them used to leak the file into the temp
/// directory. Tying deletion to the value's lifetime covers every exit path.
struct TempImage {
    path: PathBuf,
}

impl TempImage {
    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempImage {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

#[derive(Clone, Copy)]
enum Preprocess {
    ContrastSharp,
}

/// OCR result for a single crop.
#[derive(Debug, Clone, Default)]
pub struct OcrResult {
    pub text: String,
    pub available: bool,
    pub words: Vec<OcrWord>,
}

/// A word recognized by OCR and its bounding rectangle in crop coordinates.
#[derive(Debug, Clone)]
pub struct OcrWord {
    pub text: String,
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

/// Run OCR over an image crop and return the recognized text.
pub fn ocr_region(image: &RgbaImage, x: u32, y: u32, w: u32, h: u32) -> Option<OcrResult> {
    let binary = find_tesseract_binary()?;
    let crop = crop_region(image, x, y, w, h)?;

    // OCR starts an external process, so process one contrast-enhanced frame
    // per stream tick rather than repeatedly scanning overlapping crops.
    {
        let preprocess = Preprocess::ContrastSharp;
        let input_image = preprocess_image(&crop, preprocess);
        // `input` deletes the PNG when it drops, including on the `?` below.
        let input = write_temp_image(&input_image)?;
        let mut command = Command::new(&binary);
        command.args(tesseract_args(input.path(), &OcrConfig::default()));

        let output = command.output().ok()?;
        let words = parse_tsv_words(&String::from_utf8_lossy(&output.stdout));
        let text = normalize_text(
            &words
                .iter()
                .map(|word| word.text.as_str())
                .collect::<Vec<_>>()
                .join(" "),
        );
        if text.trim().is_empty() {
            return None;
        }

        return Some(OcrResult {
            text,
            available: true,
            words,
        });
    }
    fn parse_tsv_words(tsv: &str) -> Vec<OcrWord> {
        tsv.lines()
            .skip(1)
            .filter_map(|line| {
                let fields = line.split('\t').collect::<Vec<_>>();
                if fields.len() < 12 || fields[11].trim().is_empty() {
                    return None;
                }
                Some(OcrWord {
                    text: fields[11].trim().to_string(),
                    x: fields[6].parse().ok()?,
                    y: fields[7].parse().ok()?,
                    w: fields[8].parse().ok()?,
                    h: fields[9].parse().ok()?,
                })
            })
            .collect()
    }
}

/// Check whether an OCR backend is available on the current machine.
pub fn is_ocr_available() -> bool {
    find_tesseract_binary().is_some()
}

fn crop_region(image: &RgbaImage, x: u32, y: u32, w: u32, h: u32) -> Option<RgbaImage> {
    if w == 0 || h == 0 {
        return None;
    }
    let x_end = (x + w).min(image.width());
    let y_end = (y + h).min(image.height());
    if x_end <= x || y_end <= y {
        return None;
    }

    let mut crop = RgbaImage::new(x_end - x, y_end - y);
    for yy in 0..(y_end - y) {
        for xx in 0..(x_end - x) {
            let src_x = x + xx;
            let src_y = y + yy;
            crop.put_pixel(xx, yy, *image.get_pixel(src_x, src_y));
        }
    }
    Some(crop)
}

/// Crop height, in pixels, that OCR is given to work with.
///
/// Game HUD text is drawn around 8-10 pixels tall, far below what Tesseract
/// is trained for, and at that size it returns near-noise: `HP [400/400]`
/// came back as `ace".`. Enlarging the crop first is what makes small UI
/// text legible to it, so crops are scaled up to roughly this height.
const MIN_OCR_TEXT_HEIGHT: u32 = 48;

fn preprocess_image(image: &RgbaImage, mode: Preprocess) -> DynamicImage {
    let gray = DynamicImage::ImageRgba8(image.clone())
        .grayscale()
        .to_luma8();
    match mode {
        Preprocess::ContrastSharp => {
            let mut image = DynamicImage::ImageLuma8(gray);
            image = upscale_for_ocr(image);
            image = image.adjust_contrast(45.0);
            image.unsharpen(1.0, 1)
        }
    }
}

/// Enlarge a crop so its text is tall enough for OCR, preserving aspect
/// ratio. Uses Lanczos3, which keeps thin glyph strokes intact where a
/// nearest-neighbour blow-up would leave them jagged and unreadable.
fn upscale_for_ocr(image: DynamicImage) -> DynamicImage {
    let height = image.height();
    if height == 0 || height >= MIN_OCR_TEXT_HEIGHT {
        return image;
    }
    // Integer factors avoid resampling artefacts on pixel-art UI text.
    let factor = MIN_OCR_TEXT_HEIGHT.div_ceil(height).clamp(2, 8);
    let (width, height) = (
        image.width().saturating_mul(factor),
        height.saturating_mul(factor),
    );
    image.resize_exact(width, height, image::imageops::FilterType::Lanczos3)
}

fn write_temp_image(image: &DynamicImage) -> Option<TempImage> {
    let temp_dir = env::temp_dir();
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_nanos();
    let path = temp_dir.join(format!("hud-ocr-{timestamp}.png"));
    // Take ownership before writing: a failed save can still leave a partial
    // file on disk, and the guard cleans that up when this returns None.
    let image_file = TempImage { path };
    image.save(image_file.path()).ok()?;
    Some(image_file)
}

fn find_tesseract_binary() -> Option<PathBuf> {
    if let Ok(path) = env::var("TESSERACT_BIN") {
        let candidate = PathBuf::from(path);
        if candidate.exists() {
            return Some(candidate);
        }
    }

    if let Ok(path) = env::var("PATH") {
        for entry in env::split_paths(&path) {
            let candidate = entry.join("tesseract.exe");
            if candidate.exists() {
                return Some(candidate);
            }
            let candidate = entry.join("tesseract");
            if candidate.exists() {
                return Some(candidate);
            }
        }
    }

    let candidates = [
        PathBuf::from(r"C:\Program Files\Tesseract-OCR\tesseract.exe"),
        PathBuf::from(r"C:\Program Files (x86)\Tesseract-OCR\tesseract.exe"),
        PathBuf::from(r"C:\Users\magshimim\AppData\Local\Programs\Tesseract-OCR\tesseract.exe"),
    ];

    candidates.into_iter().find(|path| path.exists())
}

fn normalize_text(text: &str) -> String {
    let mut normalized = text
        .replace('\r', "")
        .lines()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    normalized = normalized
        .chars()
        .map(|ch| match ch {
            '\u{2019}' | '\u{2018}' => '\'',
            '\u{2013}' | '\u{2014}' => '-',
            '\u{00A0}' => ' ',
            _ => ch,
        })
        .collect();
    normalized.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn as_strings(args: &[OsString]) -> Vec<String> {
        args.iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn tsv_config_comes_after_option_flags() {
        let args = as_strings(&tesseract_args(
            Path::new("crop.png"),
            &OcrConfig {
                psm: 11,
                whitelist: Some("0123456789".to_string()),
            },
        ));

        assert_eq!(args[0], "crop.png");
        assert_eq!(args[1], "stdout");
        // Tesseract stops parsing options at the first config-file argument,
        // so every flag must precede `tsv` or it is read as a config name.
        assert_eq!(args.last().map(String::as_str), Some("tsv"));
        let tsv = args.iter().position(|arg| arg == "tsv").unwrap();
        for flag in ["--oem", "--psm", "-c"] {
            let at = args.iter().position(|arg| arg == flag).unwrap();
            assert!(at < tsv, "{flag} must come before the tsv config file");
        }
        assert!(
            args.iter()
                .any(|arg| arg == "tessedit_char_whitelist=0123456789")
        );
    }

    #[test]
    fn temp_image_is_removed_on_drop() {
        let image = DynamicImage::ImageRgba8(RgbaImage::new(4, 4));
        let path = {
            let temp = write_temp_image(&image).expect("temp image written");
            let path = temp.path().to_path_buf();
            assert!(path.exists());
            path
        };
        assert!(!path.exists(), "temp OCR image outlived its guard");
    }
}
