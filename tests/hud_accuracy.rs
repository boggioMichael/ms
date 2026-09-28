//! Accuracy regression tests against the committed real capture.
//!
//! `resources/maplestory.png` shows a HUD whose true values are legible in
//! the image itself:
//!
//! ```text
//!   HP  [400/400]        -> 100%
//!   MP  [1291/1351]      -> 95.6%
//!   EXP 35900[37.51%]    -> 37.51%
//! ```
//!
//! These are the numbers the vision engine must agree with. They are
//! asserted here because bar measurement previously divided the bar's width
//! by an arbitrary fraction of the frame, which reported a full HP bar as
//! ~22% and went unnoticed without a test tied to ground truth.
//!
//! Tolerances are a few percent because these assert the *geometric*
//! measurement, which reads a bar by pixels: the track's end is found by
//! colour, so a few columns of border or inter-bar gap land inside it. When
//! OCR can read the printed `current/max`, that exact figure is preferred
//! over this estimate (see `vision::detectors::hud::fuse`). The tolerances
//! are deliberately far tighter than the errors they exist to catch.

use image::RgbaImage;
use ms::vision::hud_geometry::detect_ui_markers;

fn fixture() -> RgbaImage {
    image::io::Reader::open("resources/maplestory.png")
        .expect("fixture image should exist")
        .decode()
        .expect("fixture should decode")
        .to_rgba8()
}

#[test]
fn hp_bar_reads_full_on_a_400_of_400_capture() {
    let markers = detect_ui_markers(&fixture());
    let hp = markers.hp_percent.expect("HP bar should be measurable");
    assert!(
        (hp - 100.0).abs() <= 6.0,
        "HP is 400/400 in the fixture, so it should read ~100%, got {hp:.1}%"
    );
}

#[test]
fn mp_bar_reads_nearly_full_on_a_1291_of_1351_capture() {
    let markers = detect_ui_markers(&fixture());
    let mp = markers.mp_percent.expect("MP bar should be measurable");
    assert!(
        (mp - 95.6).abs() <= 6.0,
        "MP is 1291/1351 in the fixture, so it should read ~95.6%, got {mp:.1}%"
    );
}

#[test]
fn exp_bar_reads_about_a_third_full() {
    let markers = detect_ui_markers(&fixture());
    let exp = markers.exp_percent.expect("EXP bar should be measurable");
    assert!(
        (exp - 37.51).abs() <= 6.0,
        "EXP prints 37.51% in the fixture, so the bar should agree, got {exp:.1}%"
    );
}

#[test]
fn bars_are_not_all_reported_as_identical() {
    // A measurement that collapses every bar to the same number (all 100%,
    // or all "width / half the frame") looks plausible per-bar but cannot
    // be tracking the actual fills.
    let markers = detect_ui_markers(&fixture());
    let hp = markers.hp_percent.unwrap_or(0.0);
    let exp = markers.exp_percent.unwrap_or(0.0);
    assert!(
        (hp - exp).abs() > 20.0,
        "HP is full and EXP is a third full, so they must differ: hp={hp:.1}% exp={exp:.1}%"
    );
}
