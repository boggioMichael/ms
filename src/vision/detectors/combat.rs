//! Combat intensity estimator.
//!
//! This is a *meta* detector: it does not look at pixels directly, but
//! combines the outputs of the motion detector (how much is moving) and a
//! short history of frame-difference magnitude to produce a coarse combat
//! intensity signal. This is useful for downstream AI heuristics such as
//! "avoid starting a new action while combat intensity is high" without
//! needing a dedicated skill-animation classifier (which this project does
//! not have).

use crate::util::timing::MovingAverage;
use crate::vision::types::{Confidence, Detection, Reliability, Source};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CombatIntensity {
    Idle,
    Light,
    Moderate,
    Heavy,
}

#[derive(Debug, Clone, Copy)]
pub struct CombatReading {
    pub intensity: CombatIntensity,
    /// Number of independently tracked moving entities this frame.
    pub active_entities: usize,
    /// Smoothed motion-magnitude score driving the intensity classification.
    pub motion_score: f32,
}

#[derive(Debug)]
pub struct CombatIntensityDetector {
    motion_history: MovingAverage,
    window: usize,
    samples_seen: usize,
}

impl Default for CombatIntensityDetector {
    fn default() -> Self {
        Self::new(30)
    }
}

impl CombatIntensityDetector {
    pub fn new(window: usize) -> Self {
        Self {
            motion_history: MovingAverage::new(window.max(1)),
            window: window.max(1),
            samples_seen: 0,
        }
    }

    /// Feed the current frame's moving-entity count and a diff-magnitude
    /// score (e.g. changed-pixel fraction from [`crate::vision::diff`]) and
    /// get back a classified combat intensity.
    pub fn observe(
        &mut self,
        active_entities: usize,
        diff_magnitude: f32,
    ) -> Detection<CombatReading> {
        self.motion_history.add(diff_magnitude as f64);
        self.samples_seen = (self.samples_seen + 1).min(self.window);
        let smoothed = self.motion_history.average() as f32;

        let intensity = match (smoothed, active_entities) {
            (m, _) if m < 0.01 => CombatIntensity::Idle,
            (m, n) if m < 0.05 && n <= 2 => CombatIntensity::Light,
            (m, _) if m < 0.15 => CombatIntensity::Moderate,
            _ => CombatIntensity::Heavy,
        };

        let confidence =
            Confidence::new(0.3 + (self.samples_seen as f32 / self.window as f32) * 0.5);
        Detection::found(
            CombatReading {
                intensity,
                active_entities,
                motion_score: smoothed,
            },
            confidence,
            Source::Combat,
            Reliability::Heuristic,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sustained_high_motion_and_entities_reads_heavy() {
        let mut detector = CombatIntensityDetector::new(5);
        let mut last = detector.observe(4, 0.3);
        for _ in 0..4 {
            last = detector.observe(4, 0.3);
        }
        assert_eq!(last.value.unwrap().intensity, CombatIntensity::Heavy);
    }

    #[test]
    fn no_motion_reads_idle() {
        let mut detector = CombatIntensityDetector::default();
        let reading = detector.observe(0, 0.0);
        assert_eq!(reading.value.unwrap().intensity, CombatIntensity::Idle);
    }
}
