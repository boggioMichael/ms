//! Draws what the vision engine believes it saw onto the captured frame.
//!
//! This is deliberately a pure function of a [`VisionFrameResult`] to an
//! annotated image: no window, no threads, no global state. That keeps the
//! visual layer unit-testable and lets the preview window be a thin shell
//! around it.
//!
//! Visual grammar, so a glance is enough to tell kinds of information apart:
//! - solid boxes  = regions a detector located
//! - crosshair    = an inferred position
//! - filled label = the value extracted from that region
//! - dashed box   = predicted rather than observed this frame

use image::{Rgba, RgbaImage};

use crate::observe::font;
use crate::observe::frame_result::VisionFrameResult;
use crate::vision::geometry::Rect;

const HP_COLOR: Rgba<u8> = Rgba([235, 64, 64, 255]);
const MP_COLOR: Rgba<u8> = Rgba([64, 140, 245, 255]);
const EXP_COLOR: Rgba<u8> = Rgba([240, 200, 60, 255]);
const PLATE_COLOR: Rgba<u8> = Rgba([90, 220, 220, 255]);
const MINIMAP_COLOR: Rgba<u8> = Rgba([220, 90, 220, 255]);
const CHAT_COLOR: Rgba<u8> = Rgba([150, 150, 160, 255]);
const ICON_COLOR: Rgba<u8> = Rgba([245, 150, 50, 255]);
const ENTITY_COLOR: Rgba<u8> = Rgba([80, 230, 110, 255]);
const PRIMARY_COLOR: Rgba<u8> = Rgba([60, 255, 140, 255]);
const FOOTHOLD_COLOR: Rgba<u8> = Rgba([70, 200, 200, 255]);
const DIALOG_COLOR: Rgba<u8> = Rgba([250, 250, 250, 255]);
const TEXT_COLOR: Rgba<u8> = Rgba([255, 255, 255, 255]);

const LABEL_SCALE: u32 = 1;
const LABEL_PAD: u32 = 2;

fn blend_pixel(image: &mut RgbaImage, x: i64, y: i64, color: Rgba<u8>, alpha: f32) {
    if x < 0 || y < 0 || x >= image.width() as i64 || y >= image.height() as i64 {
        return;
    }
    let (x, y) = (x as u32, y as u32);
    if alpha >= 1.0 {
        image.put_pixel(x, y, color);
        return;
    }
    let existing = *image.get_pixel(x, y);
    let mix = |a: u8, b: u8| ((a as f32 * (1.0 - alpha)) + (b as f32 * alpha)).round() as u8;
    image.put_pixel(
        x,
        y,
        Rgba([
            mix(existing[0], color[0]),
            mix(existing[1], color[1]),
            mix(existing[2], color[2]),
            255,
        ]),
    );
}

fn stroke_rect(image: &mut RgbaImage, rect: Rect, color: Rgba<u8>, thickness: u32) {
    let (x0, y0) = (rect.x as i64, rect.y as i64);
    let (x1, y1) = (
        rect.x as i64 + rect.w as i64 - 1,
        rect.y as i64 + rect.h as i64 - 1,
    );
    for t in 0..thickness as i64 {
        for x in x0..=x1 {
            blend_pixel(image, x, y0 + t, color, 1.0);
            blend_pixel(image, x, y1 - t, color, 1.0);
        }
        for y in y0..=y1 {
            blend_pixel(image, x0 + t, y, color, 1.0);
            blend_pixel(image, x1 - t, y, color, 1.0);
        }
    }
}

/// A dashed outline, used for tracks the engine predicted rather than
/// observed this frame so the distinction is visible at a glance.
fn stroke_rect_dashed(image: &mut RgbaImage, rect: Rect, color: Rgba<u8>) {
    let (x0, y0) = (rect.x as i64, rect.y as i64);
    let (x1, y1) = (
        rect.x as i64 + rect.w as i64 - 1,
        rect.y as i64 + rect.h as i64 - 1,
    );
    const DASH: i64 = 4;
    for x in x0..=x1 {
        if (x / DASH) % 2 == 0 {
            blend_pixel(image, x, y0, color, 1.0);
            blend_pixel(image, x, y1, color, 1.0);
        }
    }
    for y in y0..=y1 {
        if (y / DASH) % 2 == 0 {
            blend_pixel(image, x0, y, color, 1.0);
            blend_pixel(image, x1, y, color, 1.0);
        }
    }
}

fn fill_rect(image: &mut RgbaImage, rect: Rect, color: Rgba<u8>, alpha: f32) {
    for y in rect.y as i64..(rect.y as i64 + rect.h as i64) {
        for x in rect.x as i64..(rect.x as i64 + rect.w as i64) {
            blend_pixel(image, x, y, color, alpha);
        }
    }
}

fn crosshair(image: &mut RgbaImage, cx: i64, cy: i64, color: Rgba<u8>, arm: i64) {
    for offset in -arm..=arm {
        blend_pixel(image, cx + offset, cy, color, 1.0);
        blend_pixel(image, cx, cy + offset, color, 1.0);
    }
}

/// Draw a label with a translucent backing plate so it stays readable over
/// arbitrary game pixels. The plate is nudged inside the frame when the
/// anchor sits near an edge.
fn draw_label(image: &mut RgbaImage, text: &str, anchor_x: i64, anchor_y: i64, color: Rgba<u8>) {
    if text.is_empty() {
        return;
    }
    let width = font::text_width(text, LABEL_SCALE) + LABEL_PAD * 2;
    let height = font::text_height(LABEL_SCALE) + LABEL_PAD * 2;

    let x = anchor_x.clamp(0, (image.width() as i64 - width as i64).max(0));
    let y = anchor_y.clamp(0, (image.height() as i64 - height as i64).max(0));

    fill_rect(
        image,
        Rect {
            x: x as u32,
            y: y as u32,
            w: width,
            h: height,
        },
        Rgba([0, 0, 0, 255]),
        0.65,
    );
    // A colored spine ties the label back to the box it describes.
    fill_rect(
        image,
        Rect {
            x: x as u32,
            y: y as u32,
            w: 1,
            h: height,
        },
        color,
        1.0,
    );
    font::draw_text(
        text,
        x + LABEL_PAD as i64,
        y + LABEL_PAD as i64,
        LABEL_SCALE,
        |px, py| blend_pixel(image, px, py, TEXT_COLOR, 1.0),
    );
}

/// Place a label above a box, falling back to inside it when there is no
/// room above (detections at the top edge of the frame).
fn label_for_rect(image: &mut RgbaImage, text: &str, rect: Rect, color: Rgba<u8>) {
    let label_height = (font::text_height(LABEL_SCALE) + LABEL_PAD * 2) as i64;
    let above = rect.y as i64 - label_height - 1;
    let anchor_y = if above >= 0 { above } else { rect.y as i64 + 1 };
    draw_label(image, text, rect.x as i64, anchor_y, color);
}

fn format_metric(
    name: &str,
    metric: &crate::vision::types::Detection<crate::vision::detectors::hud::HudMetric>,
) -> String {
    let percent = metric.value.as_ref().and_then(|m| m.percent);
    let absolute = metric.value.as_ref().and_then(|m| m.value);
    match (absolute, percent) {
        (Some(value), Some(pct)) => format!("{name}: {value} ({pct:.1}%)"),
        (Some(value), None) => format!("{name}: {value}"),
        (None, Some(pct)) => format!("{name}: {pct:.1}%"),
        (None, None) => format!("{name}: ?"),
    }
}

/// Render the frame with every available detection drawn on top.
///
/// Returns a new image; the captured frame is never mutated, since the
/// pipeline may still hold a reference to it.
pub fn render_overlay(result: &VisionFrameResult) -> RgbaImage {
    let mut canvas = (*result.image).clone();
    let world = &result.world;
    let markers = &world.hud.markers;

    // HUD bars: region + the value read from it.
    if let Some(rect) = markers.hp_bar {
        stroke_rect(&mut canvas, rect, HP_COLOR, 2);
        label_for_rect(
            &mut canvas,
            &format_metric("HP", &world.hud.hp),
            rect,
            HP_COLOR,
        );
    }
    if let Some(rect) = markers.mp_bar {
        stroke_rect(&mut canvas, rect, MP_COLOR, 2);
        label_for_rect(
            &mut canvas,
            &format_metric("MP", &world.hud.mp),
            rect,
            MP_COLOR,
        );
    }
    if let Some(rect) = markers.exp_bar {
        stroke_rect(&mut canvas, rect, EXP_COLOR, 2);
        label_for_rect(
            &mut canvas,
            &format_metric("EXP", &world.hud.exp),
            rect,
            EXP_COLOR,
        );
    }

    // Character plates.
    for (rect, value, name) in [
        (
            markers.name_plate,
            world.hud.player_name.value.as_deref(),
            "NAME",
        ),
        (
            markers.class_plate,
            world.hud.character_class.value.as_deref(),
            "JOB",
        ),
        (markers.level_plate, world.hud.level.value.as_deref(), "LV"),
    ] {
        if let Some(rect) = rect {
            stroke_rect(&mut canvas, rect, PLATE_COLOR, 1);
            let text = match value {
                Some(text) if !text.trim().is_empty() => format!("{name}: {text}"),
                // The plate was located but OCR could not read it — say so
                // rather than leaving the box unexplained.
                _ => format!("{name}: ?"),
            };
            label_for_rect(&mut canvas, &text, rect, PLATE_COLOR);
        }
    }

    // Minimap.
    if let Some(minimap) = world.minimap.value.as_ref() {
        stroke_rect(&mut canvas, minimap.bounds, MINIMAP_COLOR, 2);
        label_for_rect(
            &mut canvas,
            &format!(
                "MINIMAP {}x{} conf {:.2}",
                minimap.bounds.w,
                minimap.bounds.h,
                world.minimap.confidence.value()
            ),
            minimap.bounds,
            MINIMAP_COLOR,
        );
    }

    // Chat log.
    if let Some(chat) = world.chat_log.value.as_ref() {
        stroke_rect(&mut canvas, chat.bounds, CHAT_COLOR, 1);
        label_for_rect(&mut canvas, "CHAT LOG", chat.bounds, CHAT_COLOR);
    }

    // Buff/cooldown icons.
    if let Some(icons) = world.icon_row.value.as_ref() {
        for icon in &icons.icons {
            stroke_rect(&mut canvas, icon.bounds, ICON_COLOR, 1);
        }
        if let Some(first) = icons.icons.first() {
            label_for_rect(
                &mut canvas,
                &format!("ICONS x{}", icons.icons.len()),
                first.bounds,
                ICON_COLOR,
            );
        }
    }

    // Platform edges.
    if let Some(edges) = world.footholds.value.as_ref() {
        for edge in edges {
            fill_rect(
                &mut canvas,
                Rect {
                    x: edge.bounds.x,
                    y: edge.bounds.y,
                    w: edge.bounds.w,
                    h: 1,
                },
                FOOTHOLD_COLOR,
                0.85,
            );
        }
    }

    // Dialogs sit above everything else in the game, so draw them last
    // among panels.
    if let Some(dialog) = world.dialog.value.as_ref() {
        stroke_rect(&mut canvas, dialog.bounds, DIALOG_COLOR, 2);
        label_for_rect(
            &mut canvas,
            &format!("DIALOG {:?}", dialog.kind),
            dialog.bounds,
            DIALOG_COLOR,
        );
    }

    // Moving entities. The longest-lived track is highlighted as the
    // primary subject; the engine has no dedicated player classifier, so it
    // is labeled as a track rather than asserted to be the player.
    let entities = world.motion.value.as_deref().unwrap_or(&[]);
    let primary_id = entities.iter().max_by_key(|e| e.age_frames).map(|e| e.id);
    for entity in entities {
        let is_primary = Some(entity.id) == primary_id;
        let color = if is_primary {
            PRIMARY_COLOR
        } else {
            ENTITY_COLOR
        };

        if entity.is_predicted {
            stroke_rect_dashed(&mut canvas, entity.bounds, color);
        } else {
            stroke_rect(
                &mut canvas,
                entity.bounds,
                color,
                if is_primary { 2 } else { 1 },
            );
        }

        let cx = entity.bounds.x as i64 + entity.bounds.w as i64 / 2;
        let cy = entity.bounds.y as i64 + entity.bounds.h as i64 / 2;

        if is_primary {
            crosshair(&mut canvas, cx, cy, color, 7);
            label_for_rect(
                &mut canvas,
                &format!(
                    "TRACK {} X={} Y={} AGE={}{}",
                    entity.id,
                    cx,
                    cy,
                    entity.age_frames,
                    if entity.is_predicted { " PRED" } else { "" }
                ),
                entity.bounds,
                color,
            );
        }
    }

    // Run statistics, anchored bottom-left so they never cover the HUD.
    let stats = format!(
        "FRAME {}  FPS {:.1}  CAP {:.1}MS  VIS {:.1}MS",
        result.frame_id,
        result.fps,
        result.timings.capture.as_secs_f64() * 1000.0,
        result.timings.vision.as_secs_f64() * 1000.0,
    );
    let stats_y =
        canvas.height() as i64 - font::text_height(LABEL_SCALE) as i64 - LABEL_PAD as i64 * 2 - 2;
    draw_label(&mut canvas, &stats, 2, stats_y, PRIMARY_COLOR);

    canvas
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::observe::frame_result::{FrameTimings, VisionFrameResult};
    use crate::vision::snapshot::PerceptionPipeline;
    use std::sync::Arc;

    fn result_for(image: RgbaImage) -> VisionFrameResult {
        let world = PerceptionPipeline::new().detect(&image);
        VisionFrameResult {
            frame_id: 7,
            elapsed_ms: 0,
            source: "test".into(),
            image: Arc::new(image),
            world: Arc::new(world),
            timings: FrameTimings::default(),
            fps: 30.0,
        }
    }

    #[test]
    fn overlay_preserves_frame_dimensions() {
        let image = RgbaImage::from_pixel(200, 150, Rgba([12, 12, 12, 255]));
        let overlay = render_overlay(&result_for(image));
        assert_eq!(overlay.dimensions(), (200, 150));
    }

    #[test]
    fn overlay_does_not_mutate_the_captured_frame() {
        let image = RgbaImage::from_pixel(120, 90, Rgba([12, 12, 12, 255]));
        let result = result_for(image);
        let before = (*result.image).clone();
        let _ = render_overlay(&result);
        assert_eq!(*result.image, before, "capture must stay pristine");
    }

    #[test]
    fn overlay_draws_something_over_a_blank_frame() {
        // Even with no detections, the stats plate is rendered, so the
        // output must differ from the input.
        let image = RgbaImage::from_pixel(200, 150, Rgba([12, 12, 12, 255]));
        let result = result_for(image);
        let overlay = render_overlay(&result);
        assert_ne!(overlay, *result.image);
    }

    #[test]
    fn drawing_clips_at_frame_edges_instead_of_panicking() {
        let mut image = RgbaImage::from_pixel(40, 30, Rgba([0, 0, 0, 255]));
        // Force content that pushes labels against every edge.
        for x in 0..40u32 {
            for y in 0..30u32 {
                if (x + y).is_multiple_of(3) {
                    image.put_pixel(x, y, Rgba([250, 40, 40, 255]));
                }
            }
        }
        let result = result_for(image);
        let overlay = render_overlay(&result);
        assert_eq!(overlay.dimensions(), (40, 30));
    }

    #[test]
    fn labels_are_clamped_into_the_frame() {
        let mut image = RgbaImage::from_pixel(60, 40, Rgba([0, 0, 0, 255]));
        // A label anchored past the right edge must still land in-bounds.
        draw_label(&mut image, "OVERFLOWING TEXT", 1000, 1000, HP_COLOR);
        // Reaching here without a panic is the assertion; verify the plate
        // actually landed somewhere visible.
        assert!(
            image.pixels().any(|p| p[0] > 0 || p[1] > 0 || p[2] > 0),
            "clamped label should have drawn inside the frame"
        );
    }

    #[test]
    fn metric_text_reports_what_was_actually_read() {
        use crate::vision::detectors::hud::HudMetric;
        use crate::vision::types::{Confidence, Detection, Reliability, Source};

        let with_value = Detection::found(
            HudMetric {
                label: "HP".into(),
                percent: Some(75.5),
                value: Some(2341),
                raw_text: None,
            },
            Confidence::new(0.9),
            Source::Hud,
            Reliability::Corroborated,
        );
        assert_eq!(format_metric("HP", &with_value), "HP: 2341 (75.5%)");

        // Geometry-only reads must show the percentage, not invent a value.
        let percent_only = Detection::found(
            HudMetric {
                label: "MP".into(),
                percent: Some(50.0),
                value: None,
                raw_text: None,
            },
            Confidence::new(0.55),
            Source::Hud,
            Reliability::Heuristic,
        );
        assert_eq!(format_metric("MP", &percent_only), "MP: 50.0%");

        let missing: Detection<HudMetric> = Detection::missing(Source::Hud, "none");
        assert_eq!(format_metric("EXP", &missing), "EXP: ?");
    }
}
