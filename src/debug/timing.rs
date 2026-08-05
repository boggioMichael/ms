//! Timing utilities: ScopedTimer, FrameTimer, FPSCounter, MovingAverage
//!
//! Lightweight, allocation-minimizing utilities for measuring durations in
//! detectors and measuring FPS.

use std::time::{Duration, Instant};

/// Scoped timer that logs elapsed time when dropped. Useful for timing a
/// short scope using RAII. The label will be included in the recorded duration.
pub struct ScopedTimer<'a> {
    label: &'a str,
    start: Instant,
}

impl<'a> ScopedTimer<'a> {
    pub fn new(label: &'a str) -> Self {
        Self {
            label,
            start: Instant::now(),
        }
    }

    pub fn elapsed(&self) -> Duration {
        self.start.elapsed()
    }
}

impl<'a> Drop for ScopedTimer<'a> {
    fn drop(&mut self) {
        let ms = self.start.elapsed().as_secs_f64() * 1000.0;
        tracing::debug!(label = self.label, ms, "scoped_timer");
    }
}

/// Simple frame timer for measuring per-frame durations.
pub struct FrameTimer {
    last: Instant,
}

impl FrameTimer {
    pub fn new() -> Self {
        Self {
            last: Instant::now(),
        }
    }
    /// Mark a frame and return duration since last mark
    pub fn mark(&mut self) -> Duration {
        let now = Instant::now();
        let d = now.duration_since(self.last);
        self.last = now;
        d
    }
}

/// Fixed-size moving average. Small, stack-allocated buffer; zero allocs.
pub struct MovingAverage {
    window: Vec<f64>,
    pos: usize,
    sum: f64,
    size: usize,
}

impl MovingAverage {
    pub fn new(size: usize) -> Self {
        let window = vec![0.0; size];
        Self {
            window,
            pos: 0,
            sum: 0.0,
            size,
        }
    }

    pub fn add(&mut self, v: f64) {
        self.sum -= self.window[self.pos];
        self.window[self.pos] = v;
        self.sum += v;
        self.pos = (self.pos + 1) % self.size;
    }

    pub fn average(&self) -> f64 {
        self.sum / (self.size as f64)
    }
}

/// FPS counter using a moving average over recent frame times.
pub struct FPSCounter {
    ma: MovingAverage,
}

impl FPSCounter {
    pub fn new(samples: usize) -> Self {
        Self {
            ma: MovingAverage::new(samples),
        }
    }

    /// Add a frame duration (seconds) and return the smoothed FPS
    pub fn add_frame_seconds(&mut self, secs: f64) -> f64 {
        self.ma.add(secs);
        let avg = self.ma.average();
        if avg <= 0.0 { 0.0 } else { 1.0 / avg }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn moving_avg_works() {
        let mut ma = MovingAverage::new(3);
        ma.add(0.1);
        ma.add(0.1);
        ma.add(0.1);
        assert!((ma.average() - 0.1).abs() < 1e-9);
    }
}
