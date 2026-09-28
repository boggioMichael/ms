//! Centralized application configuration.
//!
//! Detector and pipeline authors should read flags from this struct rather
//! than hardcoding behavior. It is intentionally lightweight and
//! thread-safe. Use `get_global()` to read the shared config and
//! `set_global()` to replace it during startup (e.g., from command-line
//! flags or a config file).

use std::sync::{OnceLock, RwLock};

#[derive(Debug, Clone)]
pub struct AppConfig {
    /// Persist every captured frame to `save_dir` (high disk usage; use for
    /// short diagnostic sessions only).
    pub save_frames: bool,
    /// Persist per-detector crops (HP/MP/EXP bars, plates, dialogs) to `save_dir`.
    pub save_crops: bool,
    /// Draw detection rectangles on saved overlay images.
    pub draw_boxes: bool,
    /// Include capture timestamps in log output.
    pub show_timestamps: bool,
    /// Include a live FPS readout in log output.
    pub show_fps: bool,
    /// Log raw pixel values sampled by detectors (very verbose).
    pub log_pixel_values: bool,
    /// Output directory for saved frames/crops/overlays.
    pub save_dir: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            save_frames: false,
            save_crops: false,
            draw_boxes: true,
            show_timestamps: false,
            show_fps: true,
            log_pixel_values: false,
            save_dir: String::from("out"),
        }
    }
}

static GLOBAL: OnceLock<RwLock<AppConfig>> = OnceLock::new();

/// Initialize the global config. Call once during application startup.
pub fn set_global(cfg: AppConfig) {
    let _ = GLOBAL.set(RwLock::new(cfg));
}

/// Get a clone of the global config. If unset, returns the default.
pub fn get_global() -> AppConfig {
    GLOBAL
        .get()
        .map(|lock| lock.read().unwrap().clone())
        .unwrap_or_default()
}

/// Convenience to mutate the existing global config.
pub fn with_global<F: FnOnce(&mut AppConfig)>(f: F) {
    if let Some(lock) = GLOBAL.get() {
        let mut cfg = lock.write().unwrap();
        f(&mut cfg);
    }
}
