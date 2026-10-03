//! Golden parity: what the vision read on the fixtures before Syrup took
//! over every primitive, and what it must still read after.
//!
//! `tests/golden/maplestory.json` was written by the code as it was before
//! the migration (run with `MS_GOLDEN_WRITE=1` to rewrite it, which is
//! only right when a change of behaviour is intended and reviewed). The
//! test compares the same measurements now: HUD bar geometry and fills,
//! the fixed-position panels, the platform edges, the text sharpness of
//! the HUD regions, the moving blobs between the frame and a shifted copy,
//! the bar models learned from the HUD and measured on a dimmer copy, and
//! where three crops of the frame are found again, facing either way.
//!
//! Rectangles must agree within a pixel or two, percentages within a
//! point, counts exactly. OCR is left out: it depends on the engine
//! installed (Tesseract here, the OS's on Windows), not on this code.

use std::path::Path;

use image::{Rgba, RgbaImage};
use serde::{Deserialize, Serialize};

use ms::ai::images::NBox;
use ms::sight::bars::BarModel;
use ms::vision::PerceptionPipeline;
use ms::vision::hud_geometry::detect_ui_markers;
use ms::vision::quality::assess_text_quality;

const GOLDEN: &str = "tests/golden/maplestory.json";

fn fixture() -> RgbaImage {
    image::open("resources/maplestory.png")
        .expect("resources/maplestory.png")
        .to_rgba8()
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
struct R {
    x: u32,
    y: u32,
    w: u32,
    h: u32,
}

impl From<ms::vision::Rect> for R {
    fn from(r: ms::vision::Rect) -> Self {
        R {
            x: r.x,
            y: r.y,
            w: r.w,
            h: r.h,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Hit {
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    score: f32,
    mirrored: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Golden {
    hp_bar: Option<R>,
    mp_bar: Option<R>,
    exp_bar: Option<R>,
    name_plate: Option<R>,
    class_plate: Option<R>,
    level_plate: Option<R>,
    hp_percent: Option<f32>,
    mp_percent: Option<f32>,
    exp_percent: Option<f32>,
    minimap: Option<R>,
    chat_log: Option<R>,
    icons: Vec<R>,
    footholds: Vec<R>,
    dialog: Option<R>,
    /// Sharpness of the three HUD plates, where found.
    sharpness: Vec<f32>,
    /// Moving blobs between the frame and a copy with a patch moved.
    motion: Vec<R>,
    /// Learned from the HUD bars: fill on the frame and on a dimmer copy.
    bars: Vec<(f32, Option<f32>, Option<f32>)>,
    /// Three crops of the frame found in it and in its mirror image.
    hits: Vec<Vec<Hit>>,
}

/// The crops used as taught things: where the game draws something.
fn crops(frame: &RgbaImage) -> Vec<(RgbaImage, NBox)> {
    let (fw, fh) = frame.dimensions();
    let side = 56.0 / fw as f32;
    let tall = side * fw as f32 / fh as f32;
    [(0.5, 0.45), (0.22, 0.5), (0.74, 0.5)]
        .into_iter()
        .map(|(cx, cy): (f32, f32)| {
            let place = NBox::new(
                cx - side / 2.0,
                cy - tall / 2.0,
                cx + side / 2.0,
                cy + tall / 2.0,
            );
            (ms::ai::images::crop(frame, &place), place)
        })
        .collect()
}

fn dimmer(frame: &RgbaImage) -> RgbaImage {
    RgbaImage::from_fn(frame.width(), frame.height(), |x, y| {
        let p = frame.get_pixel(x, y).0;
        Rgba([p[0] * 3 / 4, p[1] * 3 / 4, p[2] * 3 / 4, 255])
    })
}

/// The frame with a creature-sized patch moved: a bright block where there
/// was none, and the frame's own pixels shifted right by `dx` in a band.
fn moved(frame: &RgbaImage, dx: u32) -> RgbaImage {
    let mut out = frame.clone();
    for y in 300..330 {
        for x in 300..340 {
            out.put_pixel(x, y, Rgba([250, 250, 250, 255]));
        }
    }
    let band = image::imageops::crop_imm(frame, 600, 380, 200, 60).to_image();
    image::imageops::replace(&mut out, &band, 600 + i64::from(dx), 380);
    out
}

fn measure(frame: &RgbaImage) -> Golden {
    let markers = detect_ui_markers(frame);
    let mut pipeline = PerceptionPipeline::new();
    let world = pipeline.detect_frame(frame, 1);
    let motion_world = pipeline.detect_frame(&moved(frame, 6), 2);
    let plates = [markers.name_plate, markers.class_plate, markers.level_plate];
    let sharpness = plates
        .iter()
        .flatten()
        .map(|r| assess_text_quality(frame, *r).sharpness)
        .collect();
    let dim = dimmer(frame);
    let (fw, fh) = frame.dimensions();
    let bars = [
        (markers.hp_bar, 0.0),
        (markers.mp_bar, 215.0),
        (markers.exp_bar, 55.0),
    ]
    .into_iter()
    .filter_map(|(rect, hue)| {
        let r = rect?;
        let model = BarModel::learn(
            frame,
            &NBox::from_pixels(r.x, r.y, r.w, r.h, fw, fh),
            Some(hue),
        )?;
        Some((model.hue, model.measure(frame), model.measure(&dim)))
    })
    .collect();
    let mirror = image::imageops::flip_horizontal(frame);
    let hits = crops(frame)
        .iter()
        .map(|(picture, _)| {
            let mut all: Vec<Hit> = Vec::new();
            for scene in [frame, &mirror] {
                for f in ms::sight::matcher::locate(scene, picture, None, 0.72, 12, true) {
                    all.push(Hit {
                        x: f.x,
                        y: f.y,
                        w: f.w,
                        h: f.h,
                        score: f.score,
                        mirrored: f.mirrored,
                    });
                }
            }
            all
        })
        .collect();
    Golden {
        hp_bar: markers.hp_bar.map(R::from),
        mp_bar: markers.mp_bar.map(R::from),
        exp_bar: markers.exp_bar.map(R::from),
        name_plate: markers.name_plate.map(R::from),
        class_plate: markers.class_plate.map(R::from),
        level_plate: markers.level_plate.map(R::from),
        hp_percent: markers.hp_percent,
        mp_percent: markers.mp_percent,
        exp_percent: markers.exp_percent,
        minimap: world.minimap.value.map(|m| R::from(m.bounds)),
        chat_log: world.chat_log.value.map(|c| R::from(c.bounds)),
        icons: world
            .icon_row
            .value
            .map(|r| r.icons.iter().map(|i| R::from(i.bounds)).collect())
            .unwrap_or_default(),
        footholds: world
            .footholds
            .value
            .map(|f| f.iter().map(|e| R::from(e.bounds)).collect())
            .unwrap_or_default(),
        dialog: world.dialog.value.map(|d| R::from(d.bounds)),
        sharpness,
        motion: motion_world
            .motion
            .value
            .map(|m| m.iter().map(|e| R::from(e.bounds)).collect())
            .unwrap_or_default(),
        bars,
        hits,
    }
}

fn close(a: Option<R>, b: Option<R>, tolerance: u32, what: &str) {
    match (a, b) {
        (None, None) => {}
        (Some(a), Some(b)) => {
            assert!(
                a.x.abs_diff(b.x) <= tolerance
                    && a.y.abs_diff(b.y) <= tolerance
                    && a.w.abs_diff(b.w) <= tolerance
                    && a.h.abs_diff(b.h) <= tolerance,
                "{what}: {a:?} vs golden {b:?}"
            );
        }
        _ => panic!("{what}: {a:?} vs golden {b:?}"),
    }
}

fn close_all(a: &[R], b: &[R], tolerance: u32, what: &str) {
    assert_eq!(a.len(), b.len(), "{what}: {a:?} vs golden {b:?}");
    for (a, b) in a.iter().zip(b) {
        close(Some(*a), Some(*b), tolerance, what);
    }
}

fn near(a: Option<f32>, b: Option<f32>, tolerance: f32, what: &str) {
    match (a, b) {
        (None, None) => {}
        (Some(a), Some(b)) => assert!((a - b).abs() <= tolerance, "{what}: {a} vs golden {b}"),
        _ => panic!("{what}: {a:?} vs golden {b:?}"),
    }
}

#[test]
fn the_fixture_reads_as_it_did_before_the_migration() {
    let frame = fixture();
    let now = measure(&frame);
    if std::env::var_os("MS_GOLDEN_WRITE").is_some() {
        let _ = std::fs::create_dir_all(Path::new(GOLDEN).parent().unwrap());
        std::fs::write(GOLDEN, serde_json::to_string_pretty(&now).unwrap()).unwrap();
        eprintln!("written {GOLDEN}");
    }
    let golden: Golden =
        serde_json::from_str(&std::fs::read_to_string(GOLDEN).expect(GOLDEN)).expect("golden json");

    close(now.hp_bar, golden.hp_bar, 2, "hp bar");
    close(now.mp_bar, golden.mp_bar, 2, "mp bar");
    close(now.exp_bar, golden.exp_bar, 2, "exp bar");
    close(now.name_plate, golden.name_plate, 2, "name plate");
    close(now.class_plate, golden.class_plate, 2, "class plate");
    close(now.level_plate, golden.level_plate, 2, "level plate");
    near(now.hp_percent, golden.hp_percent, 1.0, "hp %");
    near(now.mp_percent, golden.mp_percent, 1.0, "mp %");
    near(now.exp_percent, golden.exp_percent, 1.0, "exp %");
    close(now.minimap, golden.minimap, 2, "minimap");
    close(now.chat_log, golden.chat_log, 2, "chat log");
    close_all(&now.icons, &golden.icons, 2, "icons");
    close_all(&now.footholds, &golden.footholds, 2, "footholds");
    close(now.dialog, golden.dialog, 2, "dialog");
    assert_eq!(now.sharpness.len(), golden.sharpness.len(), "sharpness");
    for (a, b) in now.sharpness.iter().zip(&golden.sharpness) {
        near(Some(*a), Some(*b), 0.02, "sharpness");
    }
    close_all(&now.motion, &golden.motion, 2, "motion blobs");
    assert_eq!(now.bars.len(), golden.bars.len(), "bars learned");
    for ((hue, fill, dim), (ghue, gfill, gdim)) in now.bars.iter().zip(&golden.bars) {
        assert!((hue - ghue).abs() <= 3.0, "bar hue {hue} vs golden {ghue}");
        near(*fill, *gfill, 1.0, "bar fill");
        near(*dim, *gdim, 1.0, "bar fill, dimmer");
    }
    assert_eq!(now.hits.len(), golden.hits.len());
    for (i, (a, b)) in now.hits.iter().zip(&golden.hits).enumerate() {
        assert_eq!(a.len(), b.len(), "crop {i} hits: {a:?} vs golden {b:?}");
        for (a, b) in a.iter().zip(b) {
            assert!(
                a.x.abs_diff(b.x) <= 2
                    && a.y.abs_diff(b.y) <= 2
                    && a.mirrored == b.mirrored
                    && (a.score - b.score).abs() <= 0.05,
                "crop {i}: {a:?} vs golden {b:?}"
            );
        }
    }
}
