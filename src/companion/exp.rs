//! EXP per hour, and how long the next level is at that pace.
//!
//! EXP is a percentage of the current level that falls back to near zero at
//! each level-up, so the samples are kept as one rising total: every
//! level-up adds 100 to everything after it. The rate is the Theil–Sen
//! slope of that total over the last stretch of play — the median of the
//! slopes between pairs of samples — which a few bad readings (a dialog
//! over the EXP bar, a misread) cannot drag around the way they would drag
//! a least-squares line.

use std::collections::VecDeque;

/// How far back the rate looks.
const WINDOW_SECS: f64 = 15.0 * 60.0;
/// Too short a stretch says more about the last monster than the pace.
const MIN_SPAN_SECS: f64 = 90.0;
const MIN_SAMPLES: usize = 8;
/// One sample every this many seconds is plenty for a rate per hour.
const SAMPLE_EVERY_SECS: f64 = 2.0;
/// The Theil–Sen estimate is quadratic in the samples it looks at.
const MAX_PAIRS_SAMPLES: usize = 150;

/// Readings a fall must last for before it counts as a level-up when the
/// level itself is not being read (one misread must not add 100%).
const WRAP_CONFIRMATIONS: usize = 3;
/// Recent readings whose median is "where EXP is now".
const RECENT: usize = 5;

/// A level read as risen this soon after a fall in EXP was counted is the
/// same level-up, not another.
const SAME_LEVEL_UP_SECS: f64 = 30.0;

#[derive(Debug, Clone)]
pub struct ExpTracker {
    /// `(seconds, total)` where total = EXP % + 100 × level-ups seen.
    samples: VecDeque<(f64, f64)>,
    levels_gained: u32,
    /// The last few accepted readings.
    recent: VecDeque<f64>,
    /// Readings far below `recent`, waiting to be confirmed as a level-up or
    /// dropped as a glitch.
    pending: Vec<(f64, f64)>,
    /// When the last level-up was counted.
    last_level_up: f64,
}

impl Default for ExpTracker {
    fn default() -> Self {
        Self {
            samples: VecDeque::new(),
            levels_gained: 0,
            recent: VecDeque::new(),
            pending: Vec::new(),
            last_level_up: f64::NEG_INFINITY,
        }
    }
}

impl ExpTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Level-ups seen this session.
    pub fn levels_gained(&self) -> u32 {
        self.levels_gained
    }

    /// The current EXP percent: the median of the last few readings.
    pub fn current(&self) -> Option<f64> {
        median(self.recent.iter().copied())
    }

    /// The level was read as one higher (a level-up the EXP bar may not show
    /// as a fall, e.g. a big quest reward). Returns true when it is counted,
    /// false when a fall in EXP already counted this level-up.
    pub fn level_rose(&mut self, t: f64) -> bool {
        if t - self.last_level_up < SAME_LEVEL_UP_SECS {
            return false;
        }
        let pending = std::mem::take(&mut self.pending);
        self.level_up(t, &pending);
        true
    }

    /// Add a reading of EXP `percent` (0–100) at time `t` seconds. Returns
    /// true when this reading completes a level-up (a fall from high to low
    /// that lasted).
    pub fn add(&mut self, t: f64, percent: f64) -> bool {
        if !percent.is_finite() || !(0.0..=100.0).contains(&percent) {
            return false;
        }
        if let Some(baseline) = self.current()
            && baseline - percent > 50.0
        {
            // A fall from high to low: a level-up if it lasts, else a misread.
            self.pending.push((t, percent));
            if self.pending.len() >= WRAP_CONFIRMATIONS {
                let confirmed = std::mem::take(&mut self.pending);
                self.level_up(t, &confirmed);
                return true;
            }
            return false;
        }
        // Back where it was: whatever fell was a glitch.
        self.pending.clear();
        self.accept(t, percent);
        false
    }

    fn level_up(&mut self, t: f64, readings: &[(f64, f64)]) {
        self.levels_gained += 1;
        self.last_level_up = t;
        self.recent.clear();
        for &(at, percent) in readings {
            self.accept(at, percent);
        }
        self.trim(t);
    }

    fn accept(&mut self, t: f64, percent: f64) {
        self.recent.push_back(percent);
        while self.recent.len() > RECENT {
            self.recent.pop_front();
        }
        let total = percent + 100.0 * self.levels_gained as f64;
        let due = self
            .samples
            .back()
            .is_none_or(|&(last, _)| t - last >= SAMPLE_EVERY_SECS);
        if due {
            self.samples.push_back((t, total));
        }
        self.trim(t);
    }

    fn trim(&mut self, t: f64) {
        while self
            .samples
            .front()
            .is_some_and(|&(first, _)| t - first > WINDOW_SECS)
        {
            self.samples.pop_front();
        }
    }

    /// EXP percent gained per hour, once there is enough play to say.
    pub fn per_hour(&self) -> Option<f64> {
        let (first, last) = (self.samples.front()?, self.samples.back()?);
        if self.samples.len() < MIN_SAMPLES || last.0 - first.0 < MIN_SPAN_SECS {
            return None;
        }
        let step = self.samples.len().div_ceil(MAX_PAIRS_SAMPLES);
        let points: Vec<(f64, f64)> = self.samples.iter().step_by(step).copied().collect();
        let mut slopes = Vec::with_capacity(points.len() * points.len() / 2);
        for (i, a) in points.iter().enumerate() {
            for b in &points[i + 1..] {
                let dt = b.0 - a.0;
                if dt >= 1.0 {
                    slopes.push((b.1 - a.1) / dt);
                }
            }
        }
        if slopes.is_empty() {
            return None;
        }
        slopes.sort_by(f64::total_cmp);
        let median = slopes[slopes.len() / 2];
        Some(median * 3600.0)
    }

    /// Seconds until the next level at the current pace, when EXP is going up.
    pub fn seconds_to_level(&self) -> Option<f64> {
        let rate = self.per_hour()?;
        let current = self.current()?;
        (rate > 0.01).then(|| (100.0 - current).max(0.0) / rate * 3600.0)
    }
}

fn median(values: impl Iterator<Item = f64>) -> Option<f64> {
    let mut values: Vec<f64> = values.collect();
    if values.is_empty() {
        return None;
    }
    values.sort_by(f64::total_cmp);
    Some(values[values.len() / 2])
}

/// "3 hours 10 minutes", "45 minutes", "under a minute".
pub fn spoken_duration(seconds: f64) -> String {
    let minutes = (seconds / 60.0).round() as u64;
    let (hours, minutes) = (minutes / 60, minutes % 60);
    let unit = |n: u64, one: &str, many: &str| format!("{n} {}", if n == 1 { one } else { many });
    match (hours, minutes) {
        (0, 0) => "under a minute".to_string(),
        (0, m) => unit(m, "minute", "minutes"),
        (h, 0) => unit(h, "hour", "hours"),
        (h, m) => format!(
            "{} {}",
            unit(h, "hour", "hours"),
            unit(m, "minute", "minutes")
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_steady_pace_is_measured() {
        let mut tracker = ExpTracker::new();
        // 0.5% a minute = 30% an hour, sampled every 5 s for 10 minutes.
        for i in 0..=120 {
            let t = i as f64 * 5.0;
            tracker.add(t, 10.0 + t / 60.0 * 0.5);
        }
        let rate = tracker.per_hour().unwrap();
        assert!((rate - 30.0).abs() < 0.01, "{rate}");
        let eta = tracker.seconds_to_level().unwrap();
        // 15% gained in 10 minutes, 85% left: 2 h 50 m (give or take the
        // few seconds the median of the latest readings lags behind).
        assert!((eta - 85.0 / 30.0 * 3600.0).abs() < 30.0, "{eta}");
    }

    #[test]
    fn not_enough_play_gives_no_rate() {
        let mut tracker = ExpTracker::new();
        for i in 0..10 {
            tracker.add(i as f64 * 5.0, 10.0 + i as f64 * 0.1);
        }
        assert_eq!(tracker.per_hour(), None);
    }

    #[test]
    fn a_level_up_keeps_the_total_rising() {
        let mut tracker = ExpTracker::new();
        let mut leveled = 0;
        for i in 0..=240 {
            let t = i as f64 * 5.0; // 20 minutes
            let total = 90.0 + t / 60.0; // 1% a minute, crosses 100 at minute 10
            let percent = if total >= 100.0 { total - 100.0 } else { total };
            if tracker.add(t, percent) {
                leveled += 1;
            }
        }
        assert_eq!(leveled, 1);
        assert_eq!(tracker.levels_gained(), 1);
        let rate = tracker.per_hour().unwrap();
        assert!((rate - 60.0).abs() < 0.01, "{rate}");
    }

    #[test]
    fn a_level_up_is_seen_without_a_level_reading() {
        let mut tracker = ExpTracker::new();
        tracker.add(0.0, 99.0);
        assert!(!tracker.add(2.0, 1.0));
        assert!(!tracker.add(4.0, 1.1));
        assert!(tracker.add(6.0, 1.2), "a fall that lasts is a level");
        assert_eq!(tracker.levels_gained(), 1);
        // A small fall is lost EXP or a misread, not a level.
        assert!(!tracker.add(8.0, 0.5));
        assert_eq!(tracker.levels_gained(), 1);
    }

    #[test]
    fn a_level_read_as_risen_counts_once() {
        let mut tracker = ExpTracker::new();
        tracker.add(0.0, 99.0);
        tracker.add(2.0, 0.5);
        tracker.add(4.0, 0.6);
        assert!(tracker.add(6.0, 0.7));
        // The level reading catches up a few seconds later: the same level-up.
        assert!(!tracker.level_rose(10.0));
        assert_eq!(tracker.levels_gained(), 1);
        // A level-up with no fall (a big reward from 30% to 40% of the next).
        tracker.add(100.0, 30.0);
        assert!(tracker.level_rose(101.0));
        tracker.add(102.0, 40.0);
        assert_eq!(tracker.levels_gained(), 2);
    }

    #[test]
    fn a_misread_fall_is_not_a_level_up() {
        let mut tracker = ExpTracker::new();
        for i in 0..5 {
            tracker.add(i as f64 * 2.0, 70.0);
        }
        assert!(!tracker.add(10.0, 3.0));
        assert!(!tracker.add(12.0, 70.1));
        assert!(!tracker.add(14.0, 2.0));
        assert!(!tracker.add(16.0, 70.2));
        assert_eq!(tracker.levels_gained(), 0);
        // And one high misread does not make the next normal reading a fall.
        assert!(!tracker.add(18.0, 99.0));
        assert!(!tracker.add(20.0, 70.3));
        assert_eq!(tracker.levels_gained(), 0);
    }

    #[test]
    fn a_few_misreads_do_not_move_the_rate() {
        let mut tracker = ExpTracker::new();
        for i in 0..=120 {
            let t = i as f64 * 5.0;
            let mut percent = 20.0 + t / 60.0 * 0.5;
            if i % 17 == 0 {
                percent = 3.0; // a dialog over the bar
            }
            tracker.add(t, percent);
        }
        let rate = tracker.per_hour().unwrap();
        assert!((rate - 30.0).abs() < 1.0, "{rate}");
    }

    #[test]
    fn durations_are_spoken_naturally() {
        assert_eq!(spoken_duration(20.0), "under a minute");
        assert_eq!(spoken_duration(60.0), "1 minute");
        assert_eq!(spoken_duration(45.0 * 60.0), "45 minutes");
        assert_eq!(spoken_duration(3600.0), "1 hour");
        assert_eq!(spoken_duration(3.0 * 3600.0 + 600.0), "3 hours 10 minutes");
    }
}
