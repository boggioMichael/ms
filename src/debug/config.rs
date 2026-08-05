//! Centralized debug configuration
//!
//! Detector authors should read flags from this struct. It is intentionally
//! lightweight and thread-safe. Use `get_global()` to read the shared config
//! and `set_global()` to replace it during startup (e.g., from command-line flags).

use std::sync::{OnceLock, RwLock};

#[derive(Debug, Clone)]
pub struct DebugConfig {
    pub save_frames: bool,
    pub save_crops: bool,
    pub draw_boxes: bool,
    pub show_timestamps: bool,
    pub show_fps: bool,
    pub log_pixel_values: bool,
    pub save_dir: String,
}

impl Default for DebugConfig {
    fn default() -> Self {
        Self {
            save_frames: false,
            save_crops: false,
            draw_boxes: true,
            show_timestamps: false,
            show_fps: true,
            log_pixel_values: false,
            save_dir: String::from("debug_out"),
        }
    }
}

static GLOBAL: OnceLock<RwLock<DebugConfig>> = OnceLock::new();

/// Initialize the global config. Call once during application startup.
pub fn set_global(cfg: DebugConfig) {
    let _ = GLOBAL.set(RwLock::new(cfg));
}

/// Get a clone of the global config. If unset, returns the default.
pub fn get_global() -> DebugConfig {
    GLOBAL
        .get()
        .map(|lock| lock.read().unwrap().clone())
        .unwrap_or_default()
}

/// Convenience to mutate the existing global config
pub fn with_global<F: FnOnce(&mut DebugConfig)>(f: F) {
    if let Some(lock) = GLOBAL.get() {
        let mut cfg = lock.write().unwrap();
        f(&mut cfg);
    }
}
