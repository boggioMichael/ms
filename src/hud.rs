//! MapleStory HUD detection entrypoints.
//!
//! This module re-exports the HUD detection API from the production vision
//! pipeline, providing a stable interface for applications that want to query
//! HP, MP, EXP, player name, job, and level from a single frame.

// Re-export the low-level geometry types and functions for backward compatibility.
pub use crate::vision::hud_geometry::{
    HudMetric, HudSnapshot, Rect, UiMarkers, annotate_ui_markers, detect_hud_snapshot,
    detect_ui_markers, save_ui_debug_overlay,
};

// Re-export OCR utilities.
pub use crate::vision::ocr::{OcrConfig, OcrResult, is_ocr_available, ocr_region};

// Re-export confidence-aware detector API.
pub use crate::vision::detectors::hud::{HudDetector, HudReading};
pub use crate::vision::{Detection, Confidence};
