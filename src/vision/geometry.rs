//! Shared low-level image geometry helpers.
//!
//! These primitives (rectangle segmentation, run grouping, color/text pixel
//! predicates) previously existed in slightly different forms in both
//! `debug::hp` and `debug::vision`. Consolidating them here removes that
//! duplication: every detector that needs "find a run of pixels matching a
//! predicate" or "group rows into rectangles" now shares one implementation
//! instead of each maintaining its own copy with subtly different bugs.
//!
//! All functions operate on borrowed `&RgbaImage` data and avoid copying
//! pixels; the only allocations are the small `Vec<Rect>`/`Vec<(u32,u32)>`
//! result buffers, which is unavoidable since the number of matches is
//! data-dependent.

use image::{Rgba, RgbaImage};

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

    /// Center point in pixel coordinates.
    pub fn center(&self) -> (f32, f32) {
        (
            self.x as f32 + self.w as f32 / 2.0,
            self.y as f32 + self.h as f32 / 2.0,
        )
    }
}

pub fn hue_in_range(hue: f32, min: f32, max: f32) -> bool {
    if min <= max {
        hue >= min && hue <= max
    } else {
        hue >= min || hue <= max
    }
}

pub fn is_color_pixel(
    pixel: &Rgba<u8>,
    hue_range: (f32, f32),
    min_saturation: f32,
    min_value: f32,
) -> bool {
    let (h, s, v) = crate::util::pixel::hsv_from_rgb(pixel[0], pixel[1], pixel[2]);
    let alpha = pixel[3] as f32 / 255.0;
    alpha >= 0.5
        && hue_in_range(h, hue_range.0, hue_range.1)
        && s >= min_saturation
        && v >= min_value
}

pub fn is_text_pixel(pixel: &Rgba<u8>) -> bool {
    let (_, s, v) = crate::util::pixel::hsv_from_rgb(pixel[0], pixel[1], pixel[2]);
    let brightness =
        0.299 * (pixel[0] as f32) + 0.587 * (pixel[1] as f32) + 0.114 * (pixel[2] as f32);
    let is_bright = brightness >= 90.0;
    let is_desaturated = s <= 0.55;
    let is_light = v >= 0.45;
    alpha_is_high(pixel) && is_bright && is_desaturated && is_light
}

pub fn alpha_is_high(pixel: &Rgba<u8>) -> bool {
    pixel[3] as f32 / 255.0 >= 0.45
}

/// Find runs of `min_width`-or-longer consecutive pixels along row `y` in
/// `[x0, x1]` matching `predicate`. Returns inclusive `(start_x, end_x)` pairs.
pub fn segment_row<F>(
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
        let is_target = predicate(image.get_pixel(x, y));
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

pub fn overlap(a: (u32, u32), b: (u32, u32)) -> u32 {
    if a.1 < b.0 || b.1 < a.0 {
        return 0;
    }
    let left = a.0.max(b.0);
    let right = a.1.min(b.1);
    right.saturating_sub(left).saturating_add(1)
}

/// Group per-row horizontal segments into rectangles by greedily continuing
/// the best-overlapping active rectangle from the previous row, closing out
/// rectangles once a vertical gap larger than `max_gap` rows is seen.
pub fn group_segments(rows: Vec<(u32, u32, u32)>, min_height: u32, max_gap: u32) -> Vec<Rect> {
    let mut active = Vec::<Rect>::new();
    let mut completed = Vec::<Rect>::new();
    let mut prev_y: Option<u32> = None;

    for (y, x0, x1) in rows {
        if let Some(prev_y) = prev_y
            && y > prev_y.saturating_add(max_gap).saturating_add(1)
        {
            completed.extend(active.drain(..).filter(|rect| rect.h >= min_height));
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
            if let Some(idx) = best_match
                && best_overlap * 3 >= (segment_x1 - segment_x0 + 1).max(active[idx].w)
            {
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

pub fn find_horizontal_region<F>(
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

pub fn find_text_block(image: &RgbaImage, region: Rect) -> Option<Rect> {
    let mut min_x = None;
    let mut max_x = None;
    let mut min_y = None;
    let mut max_y = None;
    let mut count = 0u32;

    let x_end = region.x.saturating_add(region.w).min(image.width());
    let y_end = region.y.saturating_add(region.h).min(image.height());

    for y in region.y..y_end {
        for x in region.x..x_end {
            if is_text_pixel(image.get_pixel(x, y)) {
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

pub fn find_text_block_in_regions(image: &RgbaImage, regions: &[Rect]) -> Option<Rect> {
    regions
        .iter()
        .filter_map(|region| find_text_block(image, *region))
        .next()
}

pub fn bar_percent(rect: Option<Rect>, max_width: u32) -> Option<f32> {
    rect.map(|rect| (rect.w as f32 / max_width as f32 * 100.0).min(100.0))
}

pub fn find_color_bar(
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

pub fn find_color_regions(
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

/// Quantize colors in `region` into `(r,g,b)/step` buckets and return the
/// bucket with the most pixels ("mode color"). Ignores near-transparent
/// pixels so alpha-blended overlays do not skew the histogram. Shared by
/// every detector that looks for a solid-color UI panel (dialogs, minimap,
/// icon-row backgrounds) so they don't each reimplement the same histogram.
pub fn dominant_color_bucket(image: &RgbaImage, region: Rect, step: u8) -> Option<(u8, u8, u8)> {
    use std::collections::HashMap;
    let mut counts: HashMap<(u8, u8, u8), u32> = HashMap::new();
    let x_end = region.x.saturating_add(region.w).min(image.width());
    let y_end = region.y.saturating_add(region.h).min(image.height());

    for y in region.y..y_end {
        for x in region.x..x_end {
            let pixel = image.get_pixel(x, y);
            if pixel[3] < 200 {
                continue;
            }
            let bucket = (pixel[0] / step, pixel[1] / step, pixel[2] / step);
            *counts.entry(bucket).or_insert(0) += 1;
        }
    }

    counts
        .into_iter()
        .max_by_key(|(_, count)| *count)
        .map(|(bucket, _)| bucket)
}

/// Find the largest rectangle within `region` whose pixels quantize to
/// `bucket` (see [`dominant_color_bucket`]). Used to locate a solid-color
/// UI panel without hardcoding any specific skin's exact color.
pub fn find_uniform_color_panel(
    image: &RgbaImage,
    region: Rect,
    bucket: (u8, u8, u8),
    step: u8,
) -> Option<Rect> {
    let matches = |pixel: &Rgba<u8>| {
        pixel[3] >= 200
            && pixel[0] / step == bucket.0
            && pixel[1] / step == bucket.1
            && pixel[2] / step == bucket.2
    };

    let mut rows = Vec::new();
    let x_end = region.x.saturating_add(region.w).min(image.width());
    let y_end = region.y.saturating_add(region.h).min(image.height());
    for y in region.y..y_end {
        for (x0, x1) in segment_row(image, y, region.x, x_end.saturating_sub(1), 20, matches) {
            rows.push((y, x0, x1));
        }
    }

    group_segments(rows, 12, 2)
        .into_iter()
        .max_by_key(|rect| rect.area())
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    #[test]
    fn segment_row_finds_runs_above_min_width() {
        let mut image = RgbaImage::from_pixel(40, 4, Rgba([0, 0, 0, 255]));
        for x in 5..20 {
            image.put_pixel(x, 1, Rgba([255, 0, 0, 255]));
        }
        let segments = segment_row(&image, 1, 0, 39, 10, |p| p[0] > 200);
        assert_eq!(segments, vec![(5, 19)]);
    }

    #[test]
    fn find_color_bar_locates_solid_block() {
        let mut image = RgbaImage::from_pixel(200, 40, Rgba([10, 10, 10, 255]));
        for y in 10..20 {
            for x in 20..160 {
                image.put_pixel(x, y, Rgba([220, 20, 20, 255]));
            }
        }
        let region = Rect {
            x: 0,
            y: 0,
            w: 200,
            h: 40,
        };
        let bar = find_color_bar(&image, region, (340.0, 30.0), 0.35, 0.30).expect("bar found");
        assert!(bar.w >= 100);
    }
}
