//! What MapleSyrup needs from the operating system beyond capture: a
//! console that understands colours and redraws, real pixels on scaled
//! displays, Ctrl+C, and a voice. Windows has all of it; elsewhere these
//! are quiet stand-ins, so the rest of the program runs (and is tested)
//! anywhere.

pub mod loopback;
pub mod overlay;
pub mod sound;
pub mod voice;

#[cfg(windows)]
mod windows_impl;

use std::sync::atomic::{AtomicBool, Ordering};

static STOP: AtomicBool = AtomicBool::new(false);

/// Whether the player asked to stop (Ctrl+C in the console).
pub fn stop_requested() -> bool {
    STOP.load(Ordering::Relaxed)
}

#[cfg_attr(not(windows), allow(dead_code))]
fn request_stop() {
    STOP.store(true, Ordering::Relaxed);
}

/// The console window's size in characters, (columns, rows), when known.
pub fn console_size() -> Option<(usize, usize)> {
    #[cfg(windows)]
    {
        windows_impl::console_size()
    }
    #[cfg(not(windows))]
    {
        let read = |name: &str| std::env::var(name).ok().and_then(|v| v.parse().ok());
        Some((read("COLUMNS")?, read("LINES")?))
    }
}

/// What `init` managed to set up.
#[derive(Debug, Clone, Copy, Default)]
pub struct ConsoleSetup {
    /// The console understands escape codes: colours, and redrawing in place.
    pub ansi: bool,
    /// The process sees real pixels on a display scaled above 100%, so a
    /// capture is the game's own resolution rather than a blurred resize.
    pub dpi_aware: bool,
}

/// Prepare the process and its console: escape codes, DPI awareness, the
/// window title, and Ctrl+C.
pub fn init(title: &str) -> ConsoleSetup {
    #[cfg(windows)]
    {
        windows_impl::init(title)
    }
    #[cfg(not(windows))]
    {
        let _ = title;
        ConsoleSetup {
            ansi: std::env::var_os("TERM").is_some_and(|t| t != "dumb"),
            dpi_aware: true,
        }
    }
}
