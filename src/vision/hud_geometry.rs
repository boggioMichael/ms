//! Raw HUD geometry + OCR detection (HP/MP/EXP bars, name/job/level plates).
//!
//! This is the concrete, pixel-level implementation used by
//! [`crate::vision::detectors::hud::HudDetector`]. It is kept as its own
//! module (rather than inlined into the detector) because it was tuned
//! iteratively against a real captured frame and is independently unit
//! tested; the detector layer above it is what adds confidence scoring.

use std::fs;
use std::path::{Path, PathBuf};

use image::{ImageError, Rgba, RgbaImage};

use crate::util::draw_rect;
use crate::vision::ocr;
// Geometry primitives (rectangle segmentation/grouping, color & text pixel
// predicates) live in `crate::vision::geometry` so every detector shares one
// implementation instead of duplicating it.
pub use crate::vision::geometry::Rect;
use crate::vision::geometry::{
    bar_percent, find_color_bar, find_color_regions, find_text_block_in_regions,
};

/// A parsed HUD metric from OCR.
#[derive(Debug, Clone)]
pub struct HudMetric {
    pub label: String,
    pub percent: Option<f32>,
    pub value: Option<u64>,
    pub raw_text: Option<String>,
}

/// Full HUD snapshot with markers and OCR-derived values.
#[derive(Debug, Clone)]
pub struct HudSnapshot {
    pub markers: UiMarkers,
    pub hp: Option<HudMetric>,
    pub mp: Option<HudMetric>,
    pub exp: Option<HudMetric>,
    pub player_name: Option<String>,
    pub character_class: Option<String>,
    pub level: Option<String>,
}

/// Detected user interface markers and inferred values.
#[derive(Debug, Clone)]
pub struct UiMarkers {
    pub hp_bar: Option<Rect>,
    pub mp_bar: Option<Rect>,
    pub exp_bar: Option<Rect>,
    pub name_plate: Option<Rect>,
    pub class_plate: Option<Rect>,
    pub level_plate: Option<Rect>,
    pub hp_percent: Option<f32>,
    pub mp_percent: Option<f32>,
    pub exp_percent: Option<f32>,
}

impl Default for UiMarkers {
    fn default() -> Self {
        Self {
            hp_bar: None,
            mp_bar: None,
            exp_bar: None,
            name_plate: None,
            class_plate: None,
            level_plate: None,
            hp_percent: None,
            mp_percent: None,
            exp_percent: None,
        }
    }
}

#[allow(dead_code)]
fn read_metric_from_ocr(image: &RgbaImage, rect: Option<Rect>, label: &str) -> Option<HudMetric> {
    let rect = rect?;
    let text = ocr::ocr_region(image, rect.x, rect.y, rect.w, rect.h)?
        .text
        .trim()
        .to_string();
    if !text
        .to_ascii_lowercase()
        .contains(&label.to_ascii_lowercase())
    {
        return None;
    }
    let percent = parse_percentage_after_label(&text, label);
    let value = parse_value_after_label(&text, label);

    Some(HudMetric {
        label: label.to_string(),
        percent,
        value,
        raw_text: (!text.is_empty()).then_some(text),
    })
}

#[allow(dead_code)]
fn read_metric_with_fallback(image: &RgbaImage, rect: Option<Rect>, label: &str) -> Option<HudMetric> {
    if let Some(metric) = read_metric_from_ocr(image, rect, label) {
        return Some(metric);
    }

    let text = metric_ocr_text(image, rect, label)?;
    let percent = parse_percentage_after_label(&text, label);
    let value = parse_value_after_label(&text, label);

    Some(HudMetric {
        label: label.to_string(),
        percent,
        value,
        raw_text: (!text.is_empty()).then_some(text),
    })
}

#[allow(dead_code)]
fn read_labeled_text(text: Option<&str>, label: &str) -> Option<String> {
    let text = text?.trim();
    let lowered = text.to_lowercase();
    let start = lowered.find(&label.to_lowercase())? + label.len();
    let mut end = text.len();
    for next_label in ["name", "job", "class", "level", "lv"] {
        if next_label.eq_ignore_ascii_case(label) {
            continue;
        }
        if let Some(offset) = lowered[start..].find(next_label) {
            end = end.min(start + offset);
        }
    }
    let value = text[start..end]
        .trim_start_matches(|character: char| {
            character.is_whitespace() || matches!(character, ':' | '-' | '=')
        })
        .split_whitespace()
        .take(4)
        .collect::<Vec<_>>()
        .join(" ");
    (!value.is_empty()).then_some(value)
}

#[allow(dead_code)]
fn metric_candidate_regions(
    image: &RgbaImage,
    rect: Option<Rect>,
    label: &str,
) -> Vec<(u32, u32, u32, u32)> {
    let mut regions = Vec::new();
    let width = image.width();
    let height = image.height();

    if let Some(rect) = rect {
        let padding = 40u32;
        regions.push((
            rect.x.saturating_sub(padding),
            rect.y.saturating_sub(padding),
            rect.w.saturating_add(padding.saturating_mul(2)),
            rect.h.saturating_add(padding.saturating_mul(2)),
        ));
    }

    let third_h = height / 3;

    if label.eq_ignore_ascii_case("exp") {
        regions.push((
            width / 5,
            height.saturating_sub(third_h),
            width * 3 / 5,
            third_h,
        ));
        regions.push((
            width / 6,
            height.saturating_sub(third_h),
            width * 2 / 3,
            third_h,
        ));
        regions.push((0, height.saturating_sub(third_h), width, third_h));
    } else {
        regions.push((0, height.saturating_sub(third_h), width / 2, third_h));
        regions.push((0, height.saturating_sub(height / 2), width / 2, height / 2));
        regions.push((0, height / 2, width / 2, height / 2));
    }

    regions.push((0, 0, width, third_h));
    regions.push((0, third_h, width / 2, third_h));

    regions
        .into_iter()
        .map(|(x, y, w, h)| (x, y, w.max(1), h.max(1)))
        .collect()
}

#[allow(dead_code)]
fn score_metric_text(text: &str, label: &str) -> i32 {
    let lowered = text.to_lowercase();
    let label_lower = label.to_lowercase();
    let mut score = 0;

    if lowered.contains(&label_lower) {
        score += 100;
    }
    if label.eq_ignore_ascii_case("exp") && lowered.contains("xp") {
        score += 70;
    }
    if lowered.contains('%') {
        score += 30;
    }
    if lowered.chars().any(|c| c.is_ascii_digit()) {
        score += 20;
    }
    score += lowered.chars().filter(|c| c.is_ascii_alphabetic()).count() as i32;
    score -= lowered.matches("estimating resolution").count() as i32 * 100;
    score
}

#[allow(dead_code)]
fn metric_ocr_text(image: &RgbaImage, rect: Option<Rect>, label: &str) -> Option<String> {
    let mut best_text = None;
    let mut best_score = i32::MIN;

    for (x, y, w, h) in metric_candidate_regions(image, rect, label) {
        if let Some(ocr) = ocr::ocr_region(image, x, y, w, h) {
            let text = ocr.text.trim().to_string();
            let score = score_metric_text(&text, label);
            if score > best_score {
                best_score = score;
                best_text = Some(text);
            }
        }
    }

    best_text.filter(|text| !text.trim().is_empty())
}

#[allow(dead_code)]
fn text_ocr_text(image: &RgbaImage, rect: Option<Rect>) -> Option<String> {
    let mut regions = Vec::new();
    if let Some(rect) = rect {
        let padding = 18u32;
        regions.push((
            rect.x.saturating_sub(padding),
            rect.y.saturating_sub(padding),
            rect.w.saturating_add(padding.saturating_mul(2)),
            rect.h.saturating_add(padding.saturating_mul(2)),
        ));
        regions.push((
            rect.x.saturating_sub(48),
            rect.y.saturating_sub(48),
            rect.w.saturating_add(96),
            rect.h.saturating_add(96),
        ));
    } else {
        let width = image.width();
        let height = image.height();
        regions.push((0, 0, width / 2, height / 3));
        regions.push((0, height / 6, width / 2, height / 3));
        regions.push((0, height / 4, width / 2, height / 3));
        regions.push((width / 2, 0, width / 2, height / 3));
        regions.push((0, height / 2, width / 2, height / 2));
    }

    let mut best_text = None;
    let mut best_score = i32::MIN;

    for (x, y, w, h) in regions {
        if let Some(ocr) = ocr::ocr_region(image, x, y, w.max(1), h.max(1)) {
            let text = ocr.text.trim().to_string();
            let score = score_text_region(&text);
            if score > best_score {
                best_score = score;
                best_text = Some(text);
            }
        }
    }

    best_text.filter(|text| !text.trim().is_empty())
}

#[allow(dead_code)]
fn score_text_region(text: &str) -> i32 {
    let lowered = text.to_lowercase();
    let alpha_count = lowered.chars().filter(|c| c.is_ascii_alphabetic()).count() as i32;
    let digit_count = lowered.chars().filter(|c| c.is_ascii_digit()).count() as i32;
    let mut score = alpha_count * 2 + digit_count;
    if lowered.contains("level") {
        score += 40;
    }
    if lowered.contains("job") {
        score += 30;
    }
    if lowered.contains("name") {
        score += 20;
    }
    if lowered.contains("estimating resolution") {
        score -= 100;
    }
    score
}

fn parse_number_token(token: &str) -> Option<u64> {
    let cleaned = token.replace(',', "").replace('.', "");
    cleaned.parse::<u64>().ok()
}

fn parse_percentage_after_label(text: &str, label: &str) -> Option<f32> {
    let lowered = text.to_lowercase();
    let label_pos = lowered.find(&label.to_lowercase())?;
    let mut search = &text[label_pos + label.len().min(text.len().saturating_sub(label_pos))..];

    while let Some(start) = search.find(|c: char| c.is_ascii_digit()) {
        search = &search[start..];
        let end = search
            .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == ','))
            .unwrap_or(search.len());
        let token = &search[..end];
        let mut tail = &search[end..];
        while let Some(ch) = tail.chars().next() {
            if ch.is_whitespace() {
                tail = &tail[ch.len_utf8()..];
            } else {
                break;
            }
        }
        if tail.starts_with('%') {
            let normalized = token.replace(',', ".");
            if let Ok(value) = normalized.parse::<f32>() {
                return Some(value);
            }
        }
        search = &search[end..];
    }

    None
}

fn parse_value_after_label(text: &str, label: &str) -> Option<u64> {
    let lowered = text.to_lowercase();
    let pos = lowered.find(&label.to_lowercase())?;
    let mut search = &text[pos + label.len().min(text.len().saturating_sub(pos))..];

    while let Some(start) = search.find(|c: char| c.is_ascii_digit()) {
        search = &search[start..];
        let end = search
            .find(|c: char| !(c.is_ascii_digit() || c == ',' || c == '.'))
            .unwrap_or(search.len());
        let token = &search[..end];
        if let Some(value) = parse_number_token(token) {
            return Some(value);
        }
        search = &search[end..];
    }

    None
}

/// Build a full HUD snapshot from a single frame.
pub fn detect_hud_snapshot(image: &RgbaImage) -> HudSnapshot {
    let markers = detect_ui_markers(image);
    let metric_from_percent = |label: &str, percent: Option<f32>| {
        percent.map(|percent| HudMetric {
            label: label.to_string(),
            percent: Some(percent),
            value: None,
            raw_text: None,
        })
    };

    HudSnapshot {
        markers: markers.clone(),
        hp: metric_from_percent("HP", markers.hp_percent),
        mp: metric_from_percent("MP", markers.mp_percent),
        exp: metric_from_percent("EXP", markers.exp_percent),
        player_name: None,
        character_class: None,
        level: None,
    }
}

/// Detect common UI markers in a game overlay frame.
///
/// This returns bounding boxes for HP/MP/EXP bars and text regions for
/// character name, class and level.
pub fn detect_ui_markers(image: &RgbaImage) -> UiMarkers {
    let width = image.width();
    let height = image.height();
    // MapleStory places the HP, MP, and EXP fills together in the lower HUD.
    // Searching the entire frame mistakes timers and action buttons for status bars.
    let status_band = Rect {
        x: 0,
        y: height.saturating_mul(9) / 10,
        w: width.saturating_mul(3) / 4,
        h: height.saturating_sub(height.saturating_mul(9) / 10),
    };
    let metric_region = |fill: Rect, min_width: u32| Rect {
        x: fill.x.saturating_sub(6),
        y: fill.y.saturating_sub(16),
        w: fill.w.saturating_add(12).max(min_width),
        h: fill.h.saturating_add(24),
    };
    let hp_bar = find_color_bar(image, status_band, (340.0, 30.0), 0.35, 0.30)
        .map(|fill| metric_region(fill, 140));
    let mp_bar = find_color_bar(image, status_band, (190.0, 250.0), 0.30, 0.30)
        .map(|fill| metric_region(fill, 140));
    let exp_bar = find_color_bar(image, status_band, (40.0, 80.0), 0.25, 0.25)
        .map(|fill| metric_region(fill, width / 8));

    let _name_plate = find_text_block_in_regions(
        image,
        &[
            Rect {
                x: 0,
                y: 0,
                w: width / 2,
                h: height / 3,
            },
            Rect {
                x: 0,
                y: height / 6,
                w: width / 2,
                h: height / 3,
            },
            Rect {
                x: 0,
                y: height / 4,
                w: width / 2,
                h: height / 3,
            },
        ],
    );

    let _class_plate = find_text_block_in_regions(
        image,
        &[
            Rect {
                x: 0,
                y: 0,
                w: width / 2,
                h: height / 4,
            },
            Rect {
                x: 0,
                y: height / 6,
                w: width / 2,
                h: height / 4,
            },
            Rect {
                x: 0,
                y: height / 3,
                w: width / 2,
                h: height / 4,
            },
        ],
    );

    let _level_plate = find_text_block_in_regions(
        image,
        &[
            Rect {
                x: width / 2,
                y: 0,
                w: width / 2,
                h: height / 4,
            },
            Rect {
                x: width / 2,
                y: height / 10,
                w: width / 2,
                h: height / 3,
            },
            Rect {
                x: width / 2,
                y: 0,
                w: width / 2,
                h: height,
            },
        ],
    );

    // Character-stat panels use a vertical stack of pink field labels. Extending
    // each label across its row boxes the actual name, job, and level values
    // without confusing quest text elsewhere in the frame for player data.
    let mut stat_rows = find_color_regions(
        image,
        Rect {
            x: 0,
            y: height / 8,
            w: width / 2,
            h: height * 2 / 3,
        },
        (320.0, 350.0),
        0.25,
        0.35,
    );
    stat_rows.sort_by_key(|rect| rect.y);
    let stat_value_row = |rect: Rect| Rect {
        x: rect.x,
        y: rect.y,
        w: (width.saturating_mul(3) / 10).min(width.saturating_sub(rect.x)),
        h: rect.h,
    };
    let name_plate = stat_rows.first().copied().map(stat_value_row);
    let class_plate = stat_rows.get(1).copied().map(stat_value_row);
    let level_plate = stat_rows.get(2).copied().map(stat_value_row);

    UiMarkers {
        hp_bar,
        mp_bar,
        exp_bar,
        name_plate,
        class_plate,
        level_plate,
        hp_percent: bar_percent(hp_bar, width / 2),
        mp_percent: bar_percent(mp_bar, width / 2),
        exp_percent: bar_percent(exp_bar, width),
    }
}

/// Annotate an image with detected UI rectangles.
pub fn annotate_ui_markers(image: &RgbaImage, markers: &UiMarkers) -> RgbaImage {
    let mut output = image.clone();
    if let Some(rect) = markers.hp_bar {
        draw_rect(
            &mut output,
            rect.x,
            rect.y,
            rect.w,
            rect.h,
            Rgba([255, 0, 0, 255]),
            2,
        );
    }
    if let Some(rect) = markers.mp_bar {
        draw_rect(
            &mut output,
            rect.x,
            rect.y,
            rect.w,
            rect.h,
            Rgba([0, 128, 255, 255]),
            2,
        );
    }
    if let Some(rect) = markers.exp_bar {
        draw_rect(
            &mut output,
            rect.x,
            rect.y,
            rect.w,
            rect.h,
            Rgba([255, 192, 0, 255]),
            2,
        );
    }
    if let Some(rect) = markers.name_plate {
        draw_rect(
            &mut output,
            rect.x,
            rect.y,
            rect.w,
            rect.h,
            Rgba([0, 255, 0, 255]),
            2,
        );
    }
    if let Some(rect) = markers.class_plate {
        draw_rect(
            &mut output,
            rect.x,
            rect.y,
            rect.w,
            rect.h,
            Rgba([255, 0, 255, 255]),
            2,
        );
    }
    if let Some(rect) = markers.level_plate {
        draw_rect(
            &mut output,
            rect.x,
            rect.y,
            rect.w,
            rect.h,
            Rgba([0, 255, 255, 255]),
            2,
        );
    }
    output
}

/// Save an annotated debug image showing detected UI markers.
pub fn save_ui_debug_overlay<P: AsRef<Path>>(
    name: &str,
    image: &RgbaImage,
    markers: &UiMarkers,
    out_dir: P,
) -> Result<PathBuf, ImageError> {
    let out_dir = out_dir.as_ref();
    fs::create_dir_all(out_dir).ok();
    let annotated = annotate_ui_markers(image, markers);
    let mut path = out_dir.join(format!(
        "debug-{}-{}.png",
        name,
        chrono::Utc::now().format("%Y%m%d-%H%M%S")
    ));
    if path.extension().is_none() {
        path.set_extension("png");
    }
    annotated.save(&path)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::RgbaImage;

    fn synthetic_ui_frame() -> RgbaImage {
        let mut image = RgbaImage::from_pixel(640, 360, Rgba([16, 20, 24, 255]));
        for y in 326..340 {
            for x in 24..224 {
                image.put_pixel(x, y, Rgba([216, 40, 40, 255]));
            }
        }
        for y in 326..340 {
            for x in 240..420 {
                image.put_pixel(x, y, Rgba([32, 120, 220, 255]));
            }
        }
        for y in 326..340 {
            for x in 436..600 {
                image.put_pixel(x, y, Rgba([220, 210, 60, 255]));
            }
        }
        image
    }

    #[test]
    fn detect_synthetic_ui_markers() {
        let image = synthetic_ui_frame();
        let markers = detect_ui_markers(&image);
        assert!(markers.hp_bar.is_some());
        assert!(markers.mp_bar.is_some());
        assert!(markers.exp_bar.is_some());
    }

    #[test]
    fn detect_synthetic_name_job_level_blocks() {
        let mut image = synthetic_ui_frame();
        for y in [60, 82, 104] {
            for yy in y..y + 12 {
                for x in 24..104 {
                    image.put_pixel(x, yy, Rgba([220, 70, 130, 255]));
                }
            }
        }

        let markers = detect_ui_markers(&image);
        assert!(markers.name_plate.is_some(), "expected name block");
        assert!(markers.class_plate.is_some(), "expected job block");
        assert!(markers.level_plate.is_some(), "expected level block");
    }

    #[test]
    fn parses_percentages_and_exp_values() {
        assert_eq!(parse_percentage_after_label("HP 37.51%", "HP"), Some(37.51));
        assert_eq!(parse_percentage_after_label("MP 100 %", "MP"), Some(100.0));
        assert_eq!(
            parse_value_after_label("EXP 35900 37.51%", "EXP"),
            Some(35900)
        );
        assert_eq!(parse_value_after_label("HP 1500 / 2000", "HP"), Some(1500));
    }

    #[test]
    fn reads_labeled_player_text_without_consuming_the_next_label() {
        let text = "Name: MapleHero Job: Arch Mage Level: 275";
        assert_eq!(
            read_labeled_text(Some(text), "name"),
            Some("MapleHero".to_string())
        );
        assert_eq!(
            read_labeled_text(Some(text), "job"),
            Some("Arch Mage".to_string())
        );
        assert_eq!(
            read_labeled_text(Some(text), "level"),
            Some("275".to_string())
        );
    }
}
