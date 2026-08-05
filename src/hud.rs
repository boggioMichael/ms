//! MapleStory HUD detection entrypoints for the first playable prototype.
//!
//! This module exposes the HUD parsing API from the crate root so the project
//! can evolve as a focused detector prototype rather than a generic debug toolkit.

pub use crate::debug::ocr::{OcrConfig, OcrResult, is_ocr_available, ocr_region};
pub use crate::debug::vision::{
    HudMetric, HudSnapshot, Rect, UiMarkers, annotate_ui_markers, detect_hud_snapshot,
    detect_ui_markers, save_ui_debug_overlay,
};
