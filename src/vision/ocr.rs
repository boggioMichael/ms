//! OCR helpers for reading HUD text and numeric values.
//!
//! This module uses the free Tesseract OCR engine when available.
//! It is intentionally isolated so detector authors can swap to another OCR
//! backend later without changing the higher-level HUD logic.

use std::env;
use std::fs;
use std::path::PathBuf;
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
        let input_path = write_temp_image(&input_image)?;
        let mut command = Command::new(&binary);
        command.arg(&input_path).arg("stdout").arg("tsv");
        command.args(OcrConfig::default().to_args());

        let output = command.output().ok()?;
        let words = parse_tsv_words(&String::from_utf8_lossy(&output.stdout));
        let _ = fs::remove_file(&input_path);
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

fn preprocess_image(image: &RgbaImage, mode: Preprocess) -> DynamicImage {
    let gray = DynamicImage::ImageRgba8(image.clone())
        .grayscale()
        .to_luma8();
    match mode {
        Preprocess::ContrastSharp => {
            let mut image = DynamicImage::ImageLuma8(gray);
            image = image.adjust_contrast(45.0);
            image.unsharpen(1.0, 1)
        }
    }
}

fn write_temp_image(image: &DynamicImage) -> Option<PathBuf> {
    let temp_dir = env::temp_dir();
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_nanos();
    let path = temp_dir.join(format!("hud-ocr-{timestamp}.png"));
    image.save(&path).ok()?;
    Some(path)
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
