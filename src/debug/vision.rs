//! UI vision helpers for MapleStory-style game overlays.
//!
//! Provides generic detection of common status elements such as HP, MP, EXP,
//! character name, class, and level regions.

use std::fs;
use std::path::{Path, PathBuf};

use image::{ImageError, Rgba, RgbaImage};

use crate::debug::crop::draw_rect;
use crate::debug::ocr;
use crate::debug::pixel::hsv_from_rgb;

/// A generic rectangle in image coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

impl Rect {
    /// End X coordinate inclusive.
    pub fn x2(&self) -> u32 {
        self.x.saturating_add(self.w.saturating_sub(1))
    }

    /// End Y coordinate inclusive.
    pub fn y2(&self) -> u32 {
        self.y.saturating_add(self.h.saturating_sub(1))
    }

    /// Rectangle area in pixels.
    pub fn area(&self) -> u32 {
        self.w.saturating_mul(self.h)
    }
}

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

fn hue_in_range(hue: f32, min: f32, max: f32) -> bool {
    if min <= max {
        hue >= min && hue <= max
    } else {
        hue >= min || hue <= max
    }
}

fn is_color_pixel(
    pixel: &Rgba<u8>,
    hue_range: (f32, f32),
    min_saturation: f32,
    min_value: f32,
) -> bool {
    let (h, s, v) = hsv_from_rgb(pixel[0], pixel[1], pixel[2]);
    let alpha = pixel[3] as f32 / 255.0;
    alpha >= 0.5
        && hue_in_range(h, hue_range.0, hue_range.1)
        && s >= min_saturation
        && v >= min_value
}

fn is_text_pixel(pixel: &Rgba<u8>) -> bool {
    let (_, s, v) = hsv_from_rgb(pixel[0], pixel[1], pixel[2]);
    let brightness =
        0.299 * (pixel[0] as f32) + 0.587 * (pixel[1] as f32) + 0.114 * (pixel[2] as f32);
    let is_bright = brightness >= 90.0;
    let is_desaturated = s <= 0.55;
    let is_light = v >= 0.45;
    alpha_is_high(pixel) && is_bright && is_desaturated && is_light
}

fn alpha_is_high(pixel: &Rgba<u8>) -> bool {
    pixel[3] as f32 / 255.0 >= 0.45
}

fn segment_row<F>(
    image: &RgbaImage,
    y: u32,
    x0: u32,
    x1: u32,
    min_width: u32,
    predicate: F,
) -> Vec<(u32, u32)>
where
    F: Fn(&Rgba<u8>) -> bool,
{
    let mut result = Vec::new();
    let mut start = None;
    let x1 = x1.min(image.width().saturating_sub(1));

    for x in x0..=x1 {
        let is_target = predicate(&image.get_pixel(x, y));
        if is_target {
            if start.is_none() {
                start = Some(x);
            }
        } else if let Some(begin) = start {
            let len = x - begin;
            if len >= min_width {
                result.push((begin, x.saturating_sub(1)));
            }
            start = None;
        }
    }

    if let Some(begin) = start {
        let len = x1.saturating_add(1).saturating_sub(begin);
        if len >= min_width {
            result.push((begin, x1));
        }
    }

    result
}

fn group_segments(rows: Vec<(u32, u32, u32)>, min_height: u32, max_gap: u32) -> Vec<Rect> {
    let mut active = Vec::<Rect>::new();
    let mut completed = Vec::<Rect>::new();
    let mut prev_y: Option<u32> = None;

    for (y, x0, x1) in rows {
        if let Some(prev_y) = prev_y {
            if y > prev_y.saturating_add(max_gap).saturating_add(1) {
                completed.extend(active.drain(..).filter(|rect| rect.h >= min_height));
            }
        }
        prev_y = Some(y);

        let mut next_active = Vec::new();
        let mut matched = vec![false; active.len()];
        for (segment_x0, segment_x1) in [(x0, x1)].into_iter() {
            let mut best_match = None;
            let mut best_overlap = 0;
            for (idx, rect) in active.iter().enumerate() {
                let overlap = overlap((rect.x, rect.x2()), (segment_x0, segment_x1));
                if overlap > best_overlap {
                    best_overlap = overlap;
                    best_match = Some(idx);
                }
            }
            if let Some(idx) = best_match {
                if best_overlap * 3 >= (segment_x1 - segment_x0 + 1).max(active[idx].w) {
                    let mut rect = active[idx];
                    rect.x = rect.x.min(segment_x0);
                    rect.w = rect
                        .x2()
                        .max(segment_x1)
                        .saturating_sub(rect.x)
                        .saturating_add(1);
                    rect.h = y.saturating_sub(rect.y).saturating_add(1);
                    matched[idx] = true;
                    next_active.push(rect);
                    continue;
                }
            }
            next_active.push(Rect {
                x: segment_x0,
                y,
                w: segment_x1.saturating_sub(segment_x0).saturating_add(1),
                h: 1,
            });
        }

        for (idx, rect) in active.into_iter().enumerate() {
            if !matched[idx] && rect.h >= min_height {
                completed.push(rect);
            }
        }
        active = next_active;
    }

    completed.extend(active.into_iter().filter(|rect| rect.h >= min_height));
    completed
}

fn overlap(a: (u32, u32), b: (u32, u32)) -> u32 {
    if a.1 < b.0 || b.1 < a.0 {
        return 0;
    }
    let left = a.0.max(b.0);
    let right = a.1.min(b.1);
    right.saturating_sub(left).saturating_add(1)
}

fn find_horizontal_region<F>(
    image: &RgbaImage,
    region: Rect,
    min_width: u32,
    min_height: u32,
    predicate: F,
) -> Option<Rect>
where
    F: Fn(&Rgba<u8>) -> bool,
{
    let mut rows = Vec::new();
    for y in region.y..region.y.saturating_add(region.h).min(image.height()) {
        let segments = segment_row(
            image,
            y,
            region.x,
            region.x.saturating_add(region.w).saturating_sub(1),
            min_width,
            &predicate,
        );
        for (x0, x1) in segments {
            rows.push((y, x0, x1));
        }
    }
    let candidates = group_segments(rows, min_height, 2);
    candidates.into_iter().max_by_key(|rect| rect.area())
}

fn find_text_block(image: &RgbaImage, region: Rect) -> Option<Rect> {
    let mut min_x = None;
    let mut max_x = None;
    let mut min_y = None;
    let mut max_y = None;
    let mut count = 0u32;

    let x_end = region.x.saturating_add(region.w).min(image.width());
    let y_end = region.y.saturating_add(region.h).min(image.height());

    for y in region.y..y_end {
        for x in region.x..x_end {
            if is_text_pixel(&image.get_pixel(x, y)) {
                count += 1;
                min_x = Some(min_x.unwrap_or(x).min(x));
                max_x = Some(max_x.unwrap_or(x).max(x));
                min_y = Some(min_y.unwrap_or(y).min(y));
                max_y = Some(max_y.unwrap_or(y).max(y));
            }
        }
    }

    if count < 6 {
        return None;
    }

    let min_x = min_x.unwrap();
    let max_x = max_x.unwrap();
    let min_y = min_y.unwrap();
    let max_y = max_y.unwrap();
    let padding = 2u32;

    Some(Rect {
        x: min_x.saturating_sub(padding),
        y: min_y.saturating_sub(padding),
        w: max_x
            .saturating_sub(min_x)
            .saturating_add(1)
            .saturating_add(padding.saturating_mul(2)),
        h: max_y
            .saturating_sub(min_y)
            .saturating_add(1)
            .saturating_add(padding.saturating_mul(2)),
    })
}

fn bar_percent(rect: Option<Rect>, max_width: u32) -> Option<f32> {
    rect.map(|rect| (rect.w as f32 / max_width as f32 * 100.0).min(100.0))
}

fn find_text_block_in_regions(image: &RgbaImage, regions: &[Rect]) -> Option<Rect> {
    regions
        .iter()
        .filter_map(|region| find_text_block(image, *region))
        .next()
}

fn find_color_bar(
    image: &RgbaImage,
    region: Rect,
    hue_range: (f32, f32),
    min_saturation: f32,
    min_value: f32,
) -> Option<Rect> {
    find_horizontal_region(image, region, (region.w / 20).max(8), 2, |pixel| {
        is_color_pixel(pixel, hue_range, min_saturation, min_value)
    })
}

fn find_color_regions(
    image: &RgbaImage,
    region: Rect,
    hue_range: (f32, f32),
    min_saturation: f32,
    min_value: f32,
) -> Vec<Rect> {
    let mut rows = Vec::new();
    for y in region.y..region.y.saturating_add(region.h).min(image.height()) {
        for (x0, x1) in segment_row(
            image,
            y,
            region.x,
            region.x.saturating_add(region.w).saturating_sub(1),
            20,
            |pixel| is_color_pixel(pixel, hue_range, min_saturation, min_value),
        ) {
            rows.push((y, x0, x1));
        }
    }
    group_segments(rows, 4, 2)
        .into_iter()
        .filter(|rect| rect.w >= 20 && rect.h >= 4)
        .collect()
}

fn read_metric_from_ocr(text: Option<&str>, label: &str) -> Option<HudMetric> {
    let text = text?.trim().to_string();
    let percent = parse_percentage_after_label(&text, label);
    let value = parse_value_after_label(&text, label);

    Some(HudMetric {
        label: label.to_string(),
        percent,
        value,
        raw_text: (!text.is_empty()).then_some(text),
    })
}

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

fn ocr_label_rect(words: &[ocr::OcrWord], labels: &[&str]) -> Option<Rect> {
    let label_index = words.iter().position(|word| {
        let normalized = word
            .text
            .trim_matches(|character: char| !character.is_ascii_alphanumeric())
            .to_ascii_lowercase();
        labels.iter().any(|label| normalized == *label)
    })?;
    let label = &words[label_index];
    let value = words
        .iter()
        .skip(label_index + 1)
        .find(|word| {
            let vertically_aligned = word.y.abs_diff(label.y) <= label.h.saturating_mul(2);
            vertically_aligned && word.x >= label.x.saturating_add(label.w)
        })
        .or_else(|| words.get(label_index + 1))?;
    let x = label.x.min(value.x);
    let y = label.y.min(value.y);
    let right = label
        .x
        .saturating_add(label.w)
        .max(value.x.saturating_add(value.w));
    let bottom = label
        .y
        .saturating_add(label.h)
        .max(value.y.saturating_add(value.h));
    Some(Rect {
        x,
        y,
        w: right.saturating_sub(x),
        h: bottom.saturating_sub(y),
    })
}

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
    let label_pos = lowered.find(&label.to_lowercase());
    let mut search = if let Some(pos) = label_pos {
        &text[pos + label.len().min(text.len().saturating_sub(pos))..]
    } else {
        text
    };

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
    let mut search = if let Some(pos) = lowered.find(&label.to_lowercase()) {
        &text[pos + label.len().min(text.len().saturating_sub(pos))..]
    } else {
        text
    };

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
    let ocr_result = ocr::ocr_region(image, 0, 0, image.width(), image.height());
    let ocr_text = ocr_result.map(|result| result.text);

    HudSnapshot {
        markers: markers.clone(),
        hp: read_metric_from_ocr(ocr_text.as_deref(), "HP"),
        mp: read_metric_from_ocr(ocr_text.as_deref(), "MP"),
        exp: read_metric_from_ocr(ocr_text.as_deref(), "EXP"),
        player_name: read_labeled_text(ocr_text.as_deref(), "name"),
        character_class: read_labeled_text(ocr_text.as_deref(), "job"),
        level: read_labeled_text(ocr_text.as_deref(), "level")
            .or_else(|| read_labeled_text(ocr_text.as_deref(), "lv")),
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
    let hp_bar = find_color_bar(image, status_band, (340.0, 30.0), 0.35, 0.30);
    let mp_bar = find_color_bar(image, status_band, (190.0, 250.0), 0.30, 0.30);
    let exp_bar = find_color_bar(image, status_band, (40.0, 80.0), 0.25, 0.25);

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
