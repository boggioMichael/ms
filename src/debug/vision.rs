//! UI vision helpers for MapleStory-style game overlays.
//!
//! Provides generic detection of common status elements such as HP, MP, EXP,
//! character name, class, and level regions.

use std::fs;
use std::path::{Path, PathBuf};

use image::{ImageError, Rgba, RgbaImage};

use crate::debug::crop::draw_rect;
use crate::debug::hp::{HpBar, find_hp_bar};
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
        w: max_x.saturating_sub(min_x).saturating_add(1).saturating_add(padding.saturating_mul(2)),
        h: max_y.saturating_sub(min_y).saturating_add(1).saturating_add(padding.saturating_mul(2)),
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

fn hp_rect_from_bar(bar: HpBar) -> Rect {
    Rect {
        x: bar.x,
        y: bar.y,
        w: bar.w,
        h: bar.h,
    }
}

/// Detect common UI markers in a game overlay frame.
///
/// This returns bounding boxes for HP/MP/EXP bars and text regions for
/// character name, class and level.
pub fn detect_ui_markers(image: &RgbaImage) -> UiMarkers {
    let width = image.width();
    let height = image.height();

    let hp_bar = find_hp_bar(image)
        .map(hp_rect_from_bar)
        .or_else(|| {
            find_color_bar(
                image,
                Rect {
                    x: 0,
                    y: 0,
                    w: width,
                    h: height / 3,
                },
                (300.0, 90.0),
                0.05,
                0.05,
            )
        })
        .or_else(|| {
            find_color_bar(
                image,
                Rect {
                    x: 0,
                    y: 0,
                    w: width / 2,
                    h: height / 2,
                },
                (300.0, 90.0),
                0.04,
                0.04,
            )
        })
        .or_else(|| {
            find_color_bar(
                image,
                Rect {
                    x: 0,
                    y: 0,
                    w: width,
                    h: height,
                },
                (300.0, 90.0),
                0.04,
                0.04,
            )
        });

    let mp_bar = find_color_bar(
        image,
        Rect {
            x: 0,
            y: 0,
            w: width,
            h: height / 3,
        },
        (170.0, 280.0),
        0.16,
        0.12,
    )
    .or_else(|| {
        find_color_bar(
            image,
            Rect {
                x: 0,
                y: 0,
                w: width,
                h: height / 2,
            },
            (100.0, 170.0),
            0.12,
            0.10,
        )
    });

    let exp_bar = find_color_bar(
        image,
        Rect {
            x: 0,
            y: height.saturating_sub(height / 3),
            w: width,
            h: height / 3,
        },
        (220.0, 360.0),
        0.12,
        0.08,
    )
    .or_else(|| {
        find_color_bar(
            image,
            Rect {
                x: 0,
                y: height.saturating_sub(height / 2),
                w: width,
                h: height / 2,
            },
            (220.0, 360.0),
            0.10,
            0.07,
        )
    })
    .or_else(|| {
        find_color_bar(
            image,
            Rect {
                x: 0,
                y: 0,
                w: width,
                h: height,
            },
            (220.0, 360.0),
            0.08,
            0.06,
        )
    });

    let name_plate = find_text_block_in_regions(
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

    let class_plate = find_text_block_in_regions(
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

    let level_plate = find_text_block_in_regions(
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
        for y in 24..38 {
            for x in 24..364 {
                image.put_pixel(x, y, Rgba([216, 40, 40, 255]));
            }
        }
        for y in 54..68 {
            for x in 24..204 {
                image.put_pixel(x, y, Rgba([32, 120, 220, 255]));
            }
        }
        for y in 312..330 {
            for x in 32..616 {
                image.put_pixel(x, y, Rgba([144, 48, 200, 255]));
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
        for y in 8..18 {
            for x in 28..116 {
                image.put_pixel(x, y, Rgba([240, 240, 240, 255]));
            }
        }
        for y in 20..30 {
            for x in 28..108 {
                image.put_pixel(x, y, Rgba([232, 232, 232, 255]));
            }
        }
        for y in 12..20 {
            for x in 520..560 {
                image.put_pixel(x, y, Rgba([248, 248, 248, 255]));
            }
        }

        let markers = detect_ui_markers(&image);
        assert!(markers.name_plate.is_some(), "expected name block");
        assert!(markers.class_plate.is_some(), "expected job block");
        assert!(markers.level_plate.is_some(), "expected level block");
    }
}
