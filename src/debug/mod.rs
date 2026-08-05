//! Debugging subsystem for MapleSyrup
//!
//! Public modules:
//! - frame: Frame inspector utilities
//! - pixel: Pixel inspection helpers
//! - crop: Crop saving and annotation helpers
//! - diff: Frame difference utilities
//! - timing: Timers, FPS counters, moving averages
//! - logging: Tracing-based logging setup
//! - config: Centralized debug configuration
//! - fps: small fps helpers (re-export of timing conveniences)

pub mod capture;
pub mod config;
pub mod crop;
pub mod diff;
pub mod fps;
pub mod frame;
pub mod hp;
pub mod logging;
pub mod ocr;
pub mod pixel;
pub mod timing;
pub mod vision;

pub use capture::{
    capture_game_window_info, capture_window_by_title, capture_window_by_title_info,
};
pub use config::DebugConfig;
pub use frame::Frame;
pub use hp::{HpBar, find_hp_bar};
pub use logging::init_tracing;
pub use timing::{FPSCounter, FrameTimer, MovingAverage, ScopedTimer};
pub use vision::{
    HudMetric, HudSnapshot, Rect, UiMarkers, annotate_ui_markers, detect_hud_snapshot,
    detect_ui_markers, save_ui_debug_overlay,
};

#[doc = "Quick prelude for typical use in detectors"]
pub mod prelude {
    pub use crate::debug::DebugConfig;
    pub use crate::debug::FPSCounter;
    pub use crate::debug::Frame;
    pub use crate::debug::HudSnapshot;
    pub use crate::debug::ScopedTimer;
    pub use crate::debug::UiMarkers;
    pub use crate::debug::detect_hud_snapshot;
    pub use crate::debug::detect_ui_markers;
}
