//! What is not vision and not the companion: the per-stage timing of the
//! vision path, read from its tracing spans, and the vision engine's
//! worker threads.
//!
//! The pixel, drawing and timing helpers that used to live here are
//! Syrup's now (`syrup::color`, `syrup::draw`, `syrup::timing`).

pub mod pool;
pub mod stages;
