//! HP bar detection utilities
//!
//! Provides heuristic detection of an HP bar in a single frame.
//! This is intended as a developer-facing integration helper when tuning
//! vision detectors on MapleStory-style screenshots.

use image::{Rgba, RgbaImage};

/// A detected HP bar bounding rectangle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HpBar {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

impl HpBar {
    /// Area in pixels.
    pub fn area(&self) -> u32 {
        self.w.saturating_mul(self.h)
    }
}

#[derive(Clone)]
struct ActiveRect {
    x0: u32,
    x1: u32,
    y0: u32,
    y1: u32,
}

fn rgb_to_hsv(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;
    let hue = if delta == 0.0 {
        0.0
    } else if max == r {
        (g - b) / delta
    } else if max == g {
        (b - r) / delta + 2.0
    } else {
        (r - g) / delta + 4.0
    };
    let mut hue = hue * 60.0;
    if hue < 0.0 {
        hue += 360.0;
    }
    let saturation = if max == 0.0 { 0.0 } else { delta / max };
    let value = max;
    (hue, saturation, value)
}

fn is_hp_candidate_pixel(pixel: &Rgba<u8>) -> bool {
    hp_pixel_score(pixel) >= 0.010
}

fn hp_pixel_score(pixel: &Rgba<u8>) -> f32 {
    let alpha = pixel[3] as f32 / 255.0;
    if alpha < 0.25 {
        return 0.0;
    }

    let r = pixel[0] as f32 / 255.0;
    let g = pixel[1] as f32 / 255.0;
    let b = pixel[2] as f32 / 255.0;
    let (hue, saturation, value) = rgb_to_hsv(r, g, b);

    let red_dominance = (r - g * 0.65 - b * 0.65).max(0.0);
    let orange_dominance = (r - g * 0.35 - b * 0.35).max(0.0);
    let redness = 1.0 - ((r - 1.0).powi(2) + g.powi(2) + b.powi(2)).sqrt() / 1.7;
    let redness = redness.max(0.0);

    let hue_score = if hue <= 35.0 || hue >= 330.0 {
        1.0
    } else if hue <= 70.0 {
        0.75
    } else if hue <= 95.0 {
        0.45
    } else {
        0.0
    };

    let saturation_score = (saturation - 0.02).max(0.0);
    let value_score = (value - 0.08).max(0.0);
    let contrast_bonus = if r > 0.22 && (g < 0.70 || b < 0.70) {
        1.1
    } else {
        1.0
    };

    (red_dominance * 0.75 + orange_dominance * 0.45 + redness * 0.50 + hue_score * 0.35)
        * (0.35 + saturation_score * 0.95)
        * (0.35 + value_score * 0.9)
        * contrast_bonus
}

fn segment_row_region(
    image: &RgbaImage,
    y: u32,
    x0: u32,
    x1: u32,
    min_width: u32,
) -> Vec<(u32, u32)> {
    let mut result = Vec::new();
    let mut start = None;
    let x1 = x1.min(image.width().saturating_sub(1));
    for x in x0..=x1 {
        let is_hp = is_hp_candidate_pixel(&image.get_pixel(x, y));
        if is_hp {
            if start.is_none() {
                start = Some(x);
            }
        } else if let Some(begin) = start {
            let len = x - begin;
            if len >= min_width {
                result.push((begin, x - 1));
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

fn overlap(a: (u32, u32), b: (u32, u32)) -> u32 {
    if a.1 < b.0 || b.1 < a.0 {
        return 0;
    }
    let left = a.0.max(b.0);
    let right = a.1.min(b.1);
    right - left + 1
}

fn find_hp_bar_by_projection(image: &RgbaImage) -> Option<HpBar> {
    let width = image.width();
    let height = image.height();
    let search_width = width / 2;
    let search_height = height / 2;
    let min_segment_width = width.saturating_div(40).max(10);
    let min_height = 2;
    let mut rows = Vec::new();

    for y in 0..search_height {
        let mut start = None;
        for x in 0..search_width {
            if hp_pixel_score(&image.get_pixel(x, y)) > 0.012 {
                if start.is_none() {
                    start = Some(x);
                }
            } else if let Some(begin) = start {
                let len = x - begin;
                if len >= min_segment_width {
                    rows.push((y, begin, x - 1));
                }
                start = None;
            }
        }
        if let Some(begin) = start {
            let len = search_width - begin;
            if len >= min_segment_width {
                rows.push((y, begin, search_width - 1));
            }
        }
    }

    if rows.is_empty() {
        return None;
    }

    let mut active = Vec::<ActiveRect>::new();
    let mut completed = Vec::<ActiveRect>::new();
    for (y, x0, x1) in rows {
        let mut next_active = Vec::new();
        let mut matched = vec![false; active.len()];
        for &(sx0, sx1) in &[(x0, x1)] {
            let mut best_match = None;
            let mut best_overlap = 0;
            for (idx, rect) in active.iter().enumerate() {
                let overlap_len = overlap((rect.x0, rect.x1), (sx0, sx1));
                if overlap_len > best_overlap {
                    best_overlap = overlap_len;
                    best_match = Some(idx);
                }
            }
            if let Some(idx) = best_match {
                let segment_width = sx1.saturating_sub(sx0).saturating_add(1);
                let rect_width = active[idx]
                    .x1
                    .saturating_sub(active[idx].x0)
                    .saturating_add(1);
                if best_overlap * 3 >= segment_width.max(rect_width) {
                    let mut rect = active[idx].clone();
                    rect.x0 = rect.x0.min(sx0);
                    rect.x1 = rect.x1.max(sx1);
                    rect.y1 = y;
                    matched[idx] = true;
                    next_active.push(rect);
                    continue;
                }
            }
            next_active.push(ActiveRect {
                x0: sx0,
                x1: sx1,
                y0: y,
                y1: y,
            });
        }
        for (idx, rect) in active.into_iter().enumerate() {
            if !matched[idx] {
                completed.push(rect);
            }
        }
        active = next_active;
    }
    completed.extend(active.into_iter());
    best_candidate(&completed, width, height, min_segment_width, min_height)
}

/// Find the most likely HP bar in the supplied RGBA image.
///
/// Returns a rectangle around the detected bar, or None if no candidate is found.
pub fn find_hp_bar(image: &RgbaImage) -> Option<HpBar> {
    let width = image.width();
    let height = image.height();
    if width < 64 || height < 32 {
        return None;
    }

    let min_segment_width = width.saturating_div(40).max(8);
    let min_height = 2;

    let search_regions = [
        (0, 0, width, height / 2),
        (0, 0, width, height / 3),
        (0, 0, width, height),
        (0, height.saturating_sub(height / 3), width, height / 3),
    ];

    for &(x0, y0, w, h) in &search_regions {
        if let Some(best) =
            find_hp_bar_in_region(image, x0, y0, w, h, min_segment_width, min_height)
        {
            tracing::debug!(?best, x0, y0, w, h, "hp bar candidate found in region");
            return Some(best);
        }
    }

    if let Some(best) = find_hp_bar_by_projection(image) {
        tracing::debug!(?best, "hp bar found by projection fallback");
        return Some(best);
    }

    tracing::debug!(width, height, "hp bar not found");
    None
}

fn best_candidate(
    rects: &[ActiveRect],
    width: u32,
    height: u32,
    min_width: u32,
    min_height: u32,
) -> Option<HpBar> {
    rects
        .iter()
        .filter_map(|rect| {
            let h = rect.y1.saturating_sub(rect.y0).saturating_add(1);
            let w = rect.x1.saturating_sub(rect.x0).saturating_add(1);
            if w < min_width || h < min_height || w < h * 3 {
                return None;
            }
            let area = (w as f32) * (h as f32);
            let location_bonus = if rect.x0 < width / 2 { 1.25 } else { 1.0 };
            let top_bonus = if rect.y0 < height / 2 { 1.15 } else { 1.0 };
            let width_bonus = if w >= width / 4 { 1.1 } else { 1.0 };
            let score = area * location_bonus * top_bonus * width_bonus;
            Some((
                score,
                HpBar {
                    x: rect.x0,
                    y: rect.y0,
                    w,
                    h,
                },
            ))
        })
        .max_by(|a, b| a.0.partial_cmp(&b.0).unwrap())
        .map(|(_, bar)| bar)
}

fn find_hp_bar_in_region(
    image: &RgbaImage,
    x0: u32,
    y0: u32,
    w: u32,
    h: u32,
    min_segment_width: u32,
    min_height: u32,
) -> Option<HpBar> {
    let mut active = Vec::<ActiveRect>::new();
    let mut completed = Vec::<ActiveRect>::new();
    let x1 = x0
        .saturating_add(w)
        .saturating_sub(1)
        .min(image.width().saturating_sub(1));
    let y1 = y0
        .saturating_add(h)
        .saturating_sub(1)
        .min(image.height().saturating_sub(1));

    for y in y0..=y1 {
        let segments = segment_row_region(image, y, x0, x1, min_segment_width);
        let mut next_active = Vec::new();
        let mut matched = vec![false; active.len()];

        for segment in segments {
            let mut best_match: Option<usize> = None;
            let mut best_overlap = 0;
            for (idx, rect) in active.iter().enumerate() {
                let overlap_len = overlap((rect.x0, rect.x1), segment);
                if overlap_len > best_overlap {
                    best_overlap = overlap_len;
                    best_match = Some(idx);
                }
            }
            if let Some(idx) = best_match {
                let segment_width = segment.1.saturating_sub(segment.0).saturating_add(1);
                let rect_width = active[idx]
                    .x1
                    .saturating_sub(active[idx].x0)
                    .saturating_add(1);
                if best_overlap * 3 >= segment_width.max(rect_width) {
                    let mut rect = active[idx].clone();
                    rect.x0 = rect.x0.min(segment.0);
                    rect.x1 = rect.x1.max(segment.1);
                    rect.y1 = y;
                    matched[idx] = true;
                    next_active.push(rect);
                    continue;
                }
            }
            next_active.push(ActiveRect {
                x0: segment.0,
                x1: segment.1,
                y0: y,
                y1: y,
            });
        }

        for (idx, rect) in active.into_iter().enumerate() {
            if !matched[idx] {
                completed.push(rect);
            }
        }
        active = next_active;
    }

    completed.extend(active.into_iter());
    best_candidate(
        &completed,
        image.width(),
        image.height(),
        min_segment_width,
        min_height,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};

    fn synthetic_frame() -> RgbaImage {
        let mut image = RgbaImage::from_pixel(320, 180, Rgba([32, 40, 60, 255]));
        for y in 18..32 {
            for x in 16..240 {
                image.put_pixel(x, y, Rgba([210, 40, 40, 255]));
            }
        }
        image
    }

    #[test]
    fn synthetic_hp_bar_is_detected() {
        let image = synthetic_frame();
        let result = find_hp_bar(&image).expect("hp bar should be found");
        assert!(result.w >= 200, "unexpected width {}", result.w);
        assert!(
            (result.y..=result.y + result.h).contains(&18),
            "y range must include bar"
        );
    }

    #[test]
    fn synthetic_dark_red_hp_bar_is_detected() {
        let mut image = RgbaImage::from_pixel(320, 180, Rgba([32, 40, 60, 255]));
        for y in 18..28 {
            for x in 16..186 {
                image.put_pixel(x, y, Rgba([170, 72, 58, 255]));
            }
        }
        let result = find_hp_bar(&image).expect("dark red hp bar should be found");
        assert!(result.w >= 160, "unexpected width {}", result.w);
        assert!(result.h >= 3, "unexpected height {}", result.h);
    }
}
