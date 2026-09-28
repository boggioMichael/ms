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

/// Width of a detected span relative to an arbitrary reference width.
///
/// This is **not** a bar fill ratio: it answers "how wide is this region
/// compared to `max_width`", which only coincides with fill percentage if
/// the bar's track happens to be exactly `max_width`. Use
/// [`measure_bar_fill`] to read how full a bar is.
pub fn bar_percent(rect: Option<Rect>, max_width: u32) -> Option<f32> {
    rect.map(|rect| (rect.w as f32 / max_width as f32 * 100.0).min(100.0))
}

/// How full a horizontal bar is, as a percentage of its own track.
///
/// Measuring a bar means comparing the filled span against the track it
/// sits in, and the track's width is not known in advance: it changes with
/// resolution, UI scale, and skin. So rather than dividing by a guessed
/// reference width, this walks outward from the filled span along the bar's
/// own rows and classifies each column as filled (the bar colour) or empty
/// (the groove behind it), stopping at the first column that is neither.
/// The result normalises itself:
///
/// ```text
///   [############--------]   filled=12 empty=8  -> 60%
///    ^ track found by walking out from the filled span
/// ```
///
/// The groove's colour is *learned* from the pixels just past the fill
/// rather than assumed. Assuming is what breaks: MapleStory's EXP track is
/// light grey while other bars sit in a dark groove, so any fixed
/// "empty is dark" rule silently measures one of them as permanently full.
/// Sampling adapts to whatever the skin uses and stops at the bar's border,
/// which is a different colour again.
///
/// A column votes by majority over the rows it spans, so antialiasing and
/// the bar's border do not swing the result. Returns `None` when no track
/// can be established, which is honest about "could not measure" instead of
/// reporting a fabricated 0%.
pub fn measure_bar_fill<F>(image: &RgbaImage, fill: Rect, search: Rect, is_filled: F) -> Option<f32>
where
    F: Fn(&Rgba<u8>) -> bool,
{
    let groove = sample_groove_color(image, fill, search, &is_filled);
    let is_empty = |pixel: &Rgba<u8>| match groove {
        Some(reference) => is_similar_color(pixel, reference, GROOVE_TOLERANCE),
        None => false,
    };
    if fill.w == 0 || fill.h == 0 {
        return None;
    }
    let (y_start, y_end) = bar_core_rows(image, fill)?;

    // Classify one column by majority vote across the bar's rows.
    let classify = |x: u32| -> ColumnKind {
        if x >= image.width() {
            return ColumnKind::Neither;
        }
        let (mut filled, mut empty) = (0u32, 0u32);
        for y in y_start..y_end {
            let pixel = image.get_pixel(x, y);
            if is_filled(pixel) {
                filled += 1;
            } else if is_empty(pixel) {
                empty += 1;
            }
        }
        let rows = y_end - y_start;
        // Require a real majority so a stray matching row cannot claim a
        // column that is mostly background.
        if filled * 2 > rows {
            ColumnKind::Filled
        } else if empty * 2 > rows {
            ColumnKind::Empty
        } else {
            ColumnKind::Neither
        }
    };

    let search_left = search.x;
    let search_right = search.x.saturating_add(search.w).min(image.width());

    let mut filled_columns = 0u32;
    let mut track_columns = 0u32;

    // Walk outward from the filled span in both directions, stopping at the
    // edge of the bar's track.
    //
    // The stop condition tolerates a short run of ambiguous columns. Where
    // the fill meets the groove there is an antialiased boundary that is
    // neither colour, and treating the first such column as the end of the
    // track stops the walk before it ever reaches the groove — which read
    // as "always full" no matter how empty the bar was. Ambiguous columns
    // are held back and only counted once real track is found beyond them.
    let mut walk = |mut step: Box<dyn FnMut() -> Option<u32> + '_>| {
        let mut pending = 0u32;
        while let Some(x) = step() {
            match classify(x) {
                ColumnKind::Filled => {
                    filled_columns += 1 + pending;
                    track_columns += 1 + pending;
                    pending = 0;
                }
                ColumnKind::Empty => {
                    track_columns += 1 + pending;
                    pending = 0;
                }
                ColumnKind::Neither => {
                    pending += 1;
                    if pending > BOUNDARY_TOLERANCE {
                        break;
                    }
                }
            }
        }
    };

    let mut left = Some(fill.x);
    walk(Box::new(move || {
        let current = left?;
        left = if current == search_left {
            None
        } else {
            Some(current - 1)
        };
        Some(current)
    }));

    let mut right = fill.x.saturating_add(1);
    walk(Box::new(move || {
        if right >= search_right {
            return None;
        }
        let current = right;
        right += 1;
        Some(current)
    }));

    if track_columns == 0 {
        return None;
    }
    Some((filled_columns as f32 / track_columns as f32 * 100.0).clamp(0.0, 100.0))
}

/// How many consecutive ambiguous columns may sit inside a bar's track
/// before the walk concludes it has left the bar. Covers the antialiased
/// fill/groove boundary and thin separator pixels.
const BOUNDARY_TOLERANCE: u32 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ColumnKind {
    Filled,
    Empty,
    Neither,
}

/// The rows to sample a bar along: its central band, not its full height.
///
/// A detected fill rectangle is taller than the bar's coloured core — it
/// picks up the border and a little surrounding UI. Those extra rows are
/// harmless over the fill itself, but past it they outvote the groove and
/// make the very first empty column look like "neither", ending the walk
/// immediately and reporting every bar as full. Sampling the middle half
/// keeps the vote on pixels that actually belong to the bar.
fn bar_core_rows(image: &RgbaImage, fill: Rect) -> Option<(u32, u32)> {
    let height = image.height();
    if fill.y >= height {
        return None;
    }
    let quarter = fill.h / 4;
    let start = fill.y.saturating_add(quarter).min(height.saturating_sub(1));
    let end = fill
        .y
        .saturating_add(fill.h.saturating_sub(quarter))
        .min(height);
    if end <= start {
        // Too thin to trim; use whatever single row is available.
        return Some((start, start.saturating_add(1).min(height)));
    }
    Some((start, end))
}

/// Per-channel tolerance when matching pixels against the sampled groove.
///
/// Wide enough to absorb the track's gradient and JPEG-ish artefacts in a
/// recorded frame, narrow enough that the bar's border and the surrounding
/// UI do not pass as groove.
const GROOVE_TOLERANCE: i32 = 38;

fn is_similar_color(pixel: &Rgba<u8>, reference: [u8; 3], tolerance: i32) -> bool {
    (0..3).all(|channel| (pixel[channel] as i32 - reference[channel] as i32).abs() <= tolerance)
}

/// Learn the groove colour from the columns immediately after the fill.
///
/// Returns `None` for a bar that is completely full, where there is no
/// groove to sample — the walk then measures only filled columns and
/// correctly reports 100%.
fn sample_groove_color<F>(
    image: &RgbaImage,
    fill: Rect,
    search: Rect,
    is_filled: &F,
) -> Option<[u8; 3]>
where
    F: Fn(&Rgba<u8>) -> bool,
{
    let (y_start, y_end) = bar_core_rows(image, fill)?;
    let search_right = search.x.saturating_add(search.w).min(image.width());

    // Step just past the fill, skipping a couple of columns of antialiasing
    // at the fill's leading edge.
    let first = fill.x.saturating_add(fill.w).saturating_add(2);
    let last = first.saturating_add(6).min(search_right);

    let mut samples: Vec<[u8; 3]> = Vec::new();
    for x in first..last {
        for y in y_start..y_end {
            let pixel = image.get_pixel(x, y);
            if pixel[3] < 128 || is_filled(pixel) {
                continue;
            }
            samples.push([pixel[0], pixel[1], pixel[2]]);
        }
    }
    if samples.is_empty() {
        return None;
    }

    // The median per channel resists the border pixels caught at the edges
    // of the sample window.
    let median = |channel: usize| {
        let mut values: Vec<u8> = samples.iter().map(|s| s[channel]).collect();
        values.sort_unstable();
        values[values.len() / 2]
    };
    let candidate = [median(0), median(1), median(2)];

    // Sampling past the fill is ambiguous: for a bar that is completely
    // full, what lies beyond the fill is not groove at all but the world
    // outside the bar. Distinguish them by looking just above the bar,
    // which is definitionally outside it — if the candidate matches that,
    // it is background and there is no groove, so the bar reads as full.
    if let Some(outside) = sample_outside_color(image, fill)
        && is_similar_color(
            &Rgba([candidate[0], candidate[1], candidate[2], 255]),
            outside,
            GROOVE_TOLERANCE,
        )
    {
        return None;
    }

    Some(candidate)
}

/// Sample the colour just above the bar, which is outside its track.
fn sample_outside_color(image: &RgbaImage, fill: Rect) -> Option<[u8; 3]> {
    // Step above the bar far enough to clear its border.
    let y = fill.y.checked_sub(3)?;
    let x_end = fill.x.saturating_add(fill.w).min(image.width());
    if fill.x >= x_end {
        return None;
    }

    let mut reds: Vec<u8> = Vec::new();
    let mut greens: Vec<u8> = Vec::new();
    let mut blues: Vec<u8> = Vec::new();
    for x in fill.x..x_end {
        let pixel = image.get_pixel(x, y);
        reds.push(pixel[0]);
        greens.push(pixel[1]);
        blues.push(pixel[2]);
    }
    if reds.is_empty() {
        return None;
    }
    let median = |mut values: Vec<u8>| {
        values.sort_unstable();
        values[values.len() / 2]
    };
    Some([median(reds), median(greens), median(blues)])
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

    /// Build a frame containing one horizontal bar whose track runs from
    /// `track_x` for `track_w` pixels, filled `fill_ratio` of the way.
    fn bar_frame(track_x: u32, track_w: u32, fill_ratio: f32) -> (RgbaImage, Rect) {
        let mut image = RgbaImage::from_pixel(400, 40, Rgba([120, 180, 90, 255]));
        let filled_w = (track_w as f32 * fill_ratio).round() as u32;
        let (y0, y1) = (14, 22);
        for y in y0..y1 {
            for x in track_x..track_x + track_w {
                let pixel = if x < track_x + filled_w {
                    Rgba([230, 30, 30, 255]) // bar colour
                } else {
                    Rgba([16, 16, 16, 255]) // dark groove
                };
                image.put_pixel(x, y, pixel);
            }
        }
        let fill = Rect {
            x: track_x,
            y: y0,
            w: filled_w.max(1),
            h: y1 - y0,
        };
        (image, fill)
    }

    fn red_filled(pixel: &Rgba<u8>) -> bool {
        is_color_pixel(pixel, (340.0, 30.0), 0.35, 0.30)
    }

    #[test]
    fn measure_bar_fill_reports_true_fill_ratio() {
        let search = Rect {
            x: 0,
            y: 0,
            w: 400,
            h: 40,
        };
        for ratio in [0.25f32, 0.5, 0.75, 1.0] {
            let (image, fill) = bar_frame(100, 200, ratio);
            let percent = measure_bar_fill(&image, fill, search, red_filled)
                .expect("track should be measurable");
            let expected = ratio * 100.0;
            assert!(
                (percent - expected).abs() <= 1.0,
                "fill {ratio} should read ~{expected}%, got {percent}%"
            );
        }
    }

    #[test]
    fn measure_bar_fill_is_independent_of_track_width_and_position() {
        // The old implementation divided by a fixed reference width, so the
        // same 50%-full bar read differently at different sizes.
        let search = Rect {
            x: 0,
            y: 0,
            w: 400,
            h: 40,
        };
        let (narrow_image, narrow_fill) = bar_frame(20, 80, 0.5);
        let (wide_image, wide_fill) = bar_frame(150, 240, 0.5);

        let narrow = measure_bar_fill(&narrow_image, narrow_fill, search, red_filled).unwrap();
        let wide = measure_bar_fill(&wide_image, wide_fill, search, red_filled).unwrap();

        assert!((narrow - 50.0).abs() <= 1.5, "narrow bar read {narrow}%");
        assert!((wide - 50.0).abs() <= 1.5, "wide bar read {wide}%");
    }

    #[test]
    fn measure_bar_fill_handles_a_light_groove_not_only_a_dark_one() {
        // MapleStory's EXP track is light grey while HP/MP sit in dark
        // grooves. A fixed "empty is dark" rule reported the EXP bar as
        // permanently 100% full; the groove colour is sampled instead.
        let mut image = RgbaImage::from_pixel(400, 40, Rgba([25, 25, 28, 255]));
        for y in 14..22 {
            for x in 100..300 {
                let pixel = if x < 100 + 75 {
                    Rgba([225, 225, 40, 255]) // yellow EXP fill
                } else {
                    Rgba([210, 210, 210, 255]) // light grey groove
                };
                image.put_pixel(x, y, pixel);
            }
        }
        let fill = Rect {
            x: 100,
            y: 14,
            w: 75,
            h: 8,
        };
        let search = Rect {
            x: 0,
            y: 0,
            w: 400,
            h: 40,
        };
        let percent = measure_bar_fill(&image, fill, search, |pixel| {
            is_color_pixel(pixel, (40.0, 80.0), 0.25, 0.25)
        })
        .expect("light groove should still yield a track");
        assert!(
            (percent - 37.5).abs() <= 1.5,
            "75/200 of the track is filled, got {percent}%"
        );
    }

    #[test]
    fn a_completely_full_bar_reads_full_not_diluted_by_background() {
        // With no groove to sample, what lies past the fill is the world
        // outside the bar. Counting it as empty track would under-report a
        // full bar.
        let search = Rect {
            x: 0,
            y: 0,
            w: 400,
            h: 40,
        };
        let (image, fill) = bar_frame(100, 200, 1.0);
        let percent = measure_bar_fill(&image, fill, search, red_filled).unwrap();
        assert!((percent - 100.0).abs() <= 1.0, "full bar read {percent}%");
    }

    #[test]
    fn measure_bar_fill_returns_none_without_a_track() {
        // Uniform background: nothing is either filled or empty, so there is
        // no track to measure and the honest answer is "unknown".
        let image = RgbaImage::from_pixel(120, 30, Rgba([120, 180, 90, 255]));
        let fill = Rect {
            x: 40,
            y: 10,
            w: 10,
            h: 6,
        };
        let search = Rect {
            x: 0,
            y: 0,
            w: 120,
            h: 30,
        };
        assert!(measure_bar_fill(&image, fill, search, red_filled).is_none());
    }

    #[test]
    fn measure_bar_fill_handles_a_bar_touching_the_frame_edge() {
        let search = Rect {
            x: 0,
            y: 0,
            w: 400,
            h: 40,
        };
        // Track starts at x=0, so walking left immediately hits the bound.
        let (image, fill) = bar_frame(0, 120, 0.5);
        let percent = measure_bar_fill(&image, fill, search, red_filled).unwrap();
        assert!((percent - 50.0).abs() <= 1.5, "edge bar read {percent}%");
    }

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
