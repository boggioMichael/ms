//! Small fps convenience wrappers

pub use crate::debug::timing::FPSCounter;

/// Convenience: compute fps from seconds per frame
pub fn fps_from_seconds(secs: f64) -> f64 {
    if secs <= 0.0 { 0.0 } else { 1.0 / secs }
}
