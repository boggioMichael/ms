//! Tracing-based logging helpers

use tracing_subscriber::{EnvFilter, fmt, prelude::*};

/// Initialize tracing subscriber with a default environment filter or provided level.
/// Safe to call multiple times; subsequent calls are no-op if the global subscriber
/// has already been set.
pub fn init_tracing(default_level: &str) {
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_level));
    let fmt_layer = fmt::layer().with_target(false);
    let _ = tracing_subscriber::registry()
        .with(filter)
        .with(fmt_layer)
        .try_init();
}
