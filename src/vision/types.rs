//! Shared vocabulary for every detector in the perception pipeline.
//!
//! The vision expansion introduces one consistent contract that every
//! detector (existing or future) must honor: instead of returning a bare
//! `Option<T>`, detectors wrap their output in [`Detection<T>`], which
//! always carries a [`Confidence`] score, a capture [`Timestamp`], the
//! originating [`Source`], a [`Reliability`] estimate for the technique
//! used, and a human readable failure reason when detection did not
//! succeed. This makes "detector honesty" a compile-time-visible property:
//! callers can no longer silently treat "not found" the same as "found but
//! uncertain".

use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

/// A confidence score in the closed range `[0.0, 1.0]`.
///
/// `0.0` means "no evidence", `1.0` means "certain". Detectors should avoid
/// binary (0/1) confidence whenever a graded estimate is available, per the
/// perception architecture requirements.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct Confidence(f32);

impl Confidence {
    pub const NONE: Confidence = Confidence(0.0);
    pub const CERTAIN: Confidence = Confidence(1.0);

    /// Build a confidence value, clamping to `[0.0, 1.0]`.
    pub fn new(value: f32) -> Self {
        Self(value.clamp(0.0, 1.0))
    }

    pub fn value(self) -> f32 {
        self.0
    }

    /// Combine two independent pieces of evidence (probabilistic OR).
    ///
    /// Used when a detector corroborates its geometric estimate with a
    /// second, independent signal (e.g. OCR text confirming a color-based
    /// bar match). This never decreases confidence and saturates at 1.0.
    pub fn combine(self, other: Confidence) -> Confidence {
        Confidence::new(self.0 + other.0 - self.0 * other.0)
    }

    /// Scale confidence down, e.g. to account for a stale/aged observation.
    pub fn decay(self, factor: f32) -> Confidence {
        Confidence::new(self.0 * factor.clamp(0.0, 1.0))
    }

    pub fn is_confident(self, threshold: f32) -> bool {
        self.0 >= threshold
    }
}

impl fmt::Display for Confidence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:.0}%", self.0 * 100.0)
    }
}

/// Wall-clock capture timestamp, stored as milliseconds since the Unix
/// epoch so it is `Copy`, comparable, and cheap to carry on every
/// detection without allocating.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Timestamp(pub u128);

impl Timestamp {
    /// Capture "now" as a timestamp.
    pub fn now() -> Self {
        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        Self(millis)
    }

    pub fn elapsed_ms_since(self, earlier: Timestamp) -> u128 {
        self.0.saturating_sub(earlier.0)
    }
}

/// Which detector/technique produced a piece of information.
///
/// Downstream consumers (AI decision layers) can use this to weigh
/// evidence, and developers can use it to trace unexpected values back to
/// the responsible detector.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Source {
    /// HUD bar/text reading (HP, MP, EXP, name, job, level).
    Hud,
    /// Frame-difference driven moving-entity detection.
    Motion,
    /// Dialog/popup/notification detection.
    Dialog,
    /// Fixed-position UI panel detection (minimap, chat log, icon rows).
    Panel,
    /// Environment/platform layout detection.
    Environment,
    /// Derived/meta detectors that combine other detector outputs.
    Combat,
}

impl fmt::Display for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let label = match self {
            Source::Hud => "hud",
            Source::Motion => "motion",
            Source::Dialog => "dialog",
            Source::Panel => "panel",
            Source::Environment => "environment",
            Source::Combat => "combat",
        };
        write!(f, "{label}")
    }
}

/// A qualitative estimate of how trustworthy a given detection technique is
/// under current conditions (independent of the numeric confidence, which
/// reflects a single sample). For example, color-heuristic bar detection is
/// `Heuristic`, while a value corroborated by OCR text is `Corroborated`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reliability {
    /// Backed by multiple independent signals (e.g. geometry + OCR + temporal
    /// consistency across frames).
    Corroborated,
    /// A single geometric/color heuristic with no independent confirmation.
    Heuristic,
    /// Derived purely from history/prediction with no direct evidence this
    /// frame (e.g. object presumed still present during brief occlusion).
    Predicted,
    /// Detection failed outright.
    Unreliable,
}

/// The result of running a detector once. Always present, even on failure,
/// so failure metadata (reason, confidence trend) is not lost.
#[derive(Debug, Clone)]
pub struct Detection<T> {
    pub value: Option<T>,
    pub confidence: Confidence,
    pub timestamp: Timestamp,
    pub source: Source,
    pub reliability: Reliability,
    pub failure_reason: Option<String>,
}

impl<T> Detection<T> {
    /// Build a successful detection.
    pub fn found(
        value: T,
        confidence: Confidence,
        source: Source,
        reliability: Reliability,
    ) -> Self {
        Self {
            value: Some(value),
            confidence,
            timestamp: Timestamp::now(),
            source,
            reliability,
            failure_reason: None,
        }
    }

    /// Build a failed detection with an explanation, useful for debugging
    /// and for downstream consumers that need to distinguish "absent" from
    /// "not evaluated".
    pub fn missing(source: Source, reason: impl Into<String>) -> Self {
        Self {
            value: None,
            confidence: Confidence::NONE,
            timestamp: Timestamp::now(),
            source,
            reliability: Reliability::Unreliable,
            failure_reason: Some(reason.into()),
        }
    }

    pub fn is_present(&self) -> bool {
        self.value.is_some()
    }

    pub fn as_ref(&self) -> Detection<&T> {
        Detection {
            value: self.value.as_ref(),
            confidence: self.confidence,
            timestamp: self.timestamp,
            source: self.source,
            reliability: self.reliability,
            failure_reason: self.failure_reason.clone(),
        }
    }

    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> Detection<U> {
        Detection {
            value: self.value.map(f),
            confidence: self.confidence,
            timestamp: self.timestamp,
            source: self.source,
            reliability: self.reliability,
            failure_reason: self.failure_reason,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confidence_clamps_and_combines() {
        let a = Confidence::new(1.5);
        assert_eq!(a.value(), 1.0);
        let b = Confidence::new(-0.2);
        assert_eq!(b.value(), 0.0);

        let combined = Confidence::new(0.5).combine(Confidence::new(0.5));
        assert!(combined.value() > 0.5 && combined.value() <= 1.0);
    }

    #[test]
    fn detection_missing_has_no_value_and_a_reason() {
        let detection: Detection<u32> = Detection::missing(Source::Motion, "no frames yet");
        assert!(!detection.is_present());
        assert_eq!(detection.confidence, Confidence::NONE);
        assert_eq!(detection.reliability, Reliability::Unreliable);
        assert!(detection.failure_reason.is_some());
    }

    #[test]
    fn detection_found_round_trips_value() {
        let detection = Detection::found(
            42u32,
            Confidence::new(0.8),
            Source::Hud,
            Reliability::Heuristic,
        );
        assert_eq!(detection.value, Some(42));
        assert!(detection.confidence.is_confident(0.5));
    }
}
