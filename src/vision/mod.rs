//! Complete computer vision perception pipeline for MapleStory.
//!
//! This module implements a reusable, confidence-aware, temporally-aware
//! vision subsystem that turns raw pixels into actionable game state
//! information. Every detector reports confidence scores, reliability
//! estimates, and failure reasons rather than binary "found/not found"
//! signals, making it possible for downstream AI to distinguish between
//! "very confident the HP is 50%" and "found a red bar, could be HP or
//! something else".

pub mod bar_geometry;
pub mod detectors;
pub mod diff;
pub mod geometry;
pub mod hud_geometry;
pub mod hud_ocr;
pub mod hud_text;
pub mod ocr;
pub mod ocr_windows;
pub mod quality;
pub mod snapshot;
pub mod temporal;
pub mod types;

// Re-export commonly-used types at the vision module level for ergonomic
// downstream imports (e.g. `crate::vision::Detection<T>` rather than
// `crate::vision::types::Detection<T>`).
pub use geometry::Rect;
pub use snapshot::{PerceptionPipeline, WorldState};
pub use temporal::{ObjectTracker, Track};
pub use types::{Confidence, Detection, Reliability, Source, Timestamp};
