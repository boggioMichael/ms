use std::time::{Duration, Instant};

/// Simple FPS counter with exponential moving average smoothing.
pub struct FpsCounter {
    last: Instant,
    frames: u64,
    ema: f64,
    smoothing: f64,
}

impl FpsCounter {
    pub fn new(target_hz: u32) -> Self {
        Self { last: Instant::now(), frames: 0, ema: 0.0, smoothing: 0.9 }
    }

    /// Call when a frame is produced
    pub fn frame(&mut self) {
        let now = Instant::now();
        let dt = now.duration_since(self.last);
        self.last = now;
        let fps = if dt.as_secs_f64() > 0.0 { 1.0 / dt.as_secs_f64() } else { 0.0 };
        if self.ema == 0.0 {
            self.ema = fps;
        } else {
            self.ema = self.smoothing * self.ema + (1.0 - self.smoothing) * fps;
        }
        self.frames += 1;
    }

    /// Get current smoothed FPS
    pub fn current_fps(&self) -> f64 {
        self.ema
    }
}
