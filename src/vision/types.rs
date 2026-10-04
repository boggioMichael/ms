//! The result vocabulary, which is Syrup's: every detector returns a
//! [`Detection`] — a value with its confidence, reliability and failure
//! reason — and names its source with a short label (`"hud"`, `"motion"`,
//! `"dialog"`, `"panel"`, `"environment"`, `"combat"`).

pub use syrup::detection::{Confidence, Detection, Reliability, Timestamp};
