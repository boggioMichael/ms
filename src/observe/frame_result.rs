//! One coherent result per processed frame.
//!
//! Both the terminal dashboard and the graphical preview render from this
//! single struct, so the two views can never disagree about what the engine
//! saw: there is exactly one capture, one perception run, and one result.
//!
//! The captured image travels inside the result because the preview needs
//! the actual pixels to draw on. It is an `Arc` so publishing a result to
//! the visualization layer costs a refcount bump rather than a full frame
//! copy.

use std::sync::Arc;
use std::time::Duration;

use image::RgbaImage;

use crate::vision::snapshot::WorldState;

/// Per-frame timing, measured around the real work rather than estimated.
#[derive(Debug, Clone, Copy, Default)]
pub struct FrameTimings {
    /// Time spent acquiring the frame from the OS.
    pub capture: Duration,
    /// Time spent running the perception pipeline over that frame.
    pub vision: Duration,
    /// Wall-clock time between this frame and the previous one, which is
    /// what actually determines throughput.
    pub frame_interval: Duration,
}

impl FrameTimings {
    /// Capture + vision. This is pipeline cost, deliberately excluding the
    /// idle gap between frames.
    pub fn total(&self) -> Duration {
        self.capture.saturating_add(self.vision)
    }

    /// Instantaneous rate implied by the gap to the previous frame.
    pub fn instantaneous_fps(&self) -> f64 {
        let secs = self.frame_interval.as_secs_f64();
        if secs <= 0.0 { 0.0 } else { 1.0 / secs }
    }
}

/// Everything known about a single processed frame.
#[derive(Clone)]
pub struct VisionFrameResult {
    /// Monotonically increasing frame counter for this run.
    pub frame_id: u64,
    /// Milliseconds since the run started.
    pub elapsed_ms: u64,
    /// Title of the window the frame came from, or the fixture path.
    pub source: String,
    /// The captured pixels. Shared, not copied, when handed to the preview.
    pub image: Arc<RgbaImage>,
    /// Aggregated detector output for this frame.
    pub world: Arc<WorldState>,
    pub timings: FrameTimings,
    /// Smoothed frames per second across a recent window.
    pub fps: f64,
}

impl VisionFrameResult {
    pub fn frame_size(&self) -> (u32, u32) {
        (self.image.width(), self.image.height())
    }
}

/// A single-slot mailbox holding only the newest result.
///
/// Visualization is a debugging layer and must never become backpressure on
/// the vision loop, so publishing overwrites whatever the consumer has not
/// picked up yet. A queue would let stale frames pile up and make the
/// preview lag behind reality; overwriting means the viewer always sees the
/// freshest frame and slow rendering costs dropped visualization frames
/// instead of pipeline throughput.
#[derive(Default)]
pub struct LatestFrame {
    slot: std::sync::Mutex<Option<VisionFrameResult>>,
    /// Counts results that were overwritten before anyone rendered them.
    dropped: std::sync::atomic::AtomicU64,
}

impl LatestFrame {
    pub fn new() -> Self {
        Self::default()
    }

    /// Publish a result, discarding any unread previous one.
    pub fn publish(&self, result: VisionFrameResult) {
        let mut slot = self.slot.lock().unwrap_or_else(|e| e.into_inner());
        if slot.is_some() {
            self.dropped
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        *slot = Some(result);
    }

    /// Take the newest result, if one arrived since the last call.
    pub fn take(&self) -> Option<VisionFrameResult> {
        self.slot.lock().unwrap_or_else(|e| e.into_inner()).take()
    }

    /// How many results were superseded before being rendered.
    pub fn dropped_count(&self) -> u64 {
        self.dropped.load(std::sync::atomic::Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vision::snapshot::PerceptionPipeline;
    use image::Rgba;

    fn sample_result(frame_id: u64) -> VisionFrameResult {
        let image = RgbaImage::from_pixel(64, 48, Rgba([20, 20, 20, 255]));
        let world = PerceptionPipeline::new().detect(&image);
        VisionFrameResult {
            frame_id,
            elapsed_ms: 0,
            source: "test".into(),
            image: Arc::new(image),
            world: Arc::new(world),
            timings: FrameTimings::default(),
            fps: 0.0,
        }
    }

    #[test]
    fn total_latency_is_capture_plus_vision() {
        let timings = FrameTimings {
            capture: Duration::from_millis(3),
            vision: Duration::from_millis(5),
            frame_interval: Duration::from_millis(20),
        };
        assert_eq!(timings.total(), Duration::from_millis(8));
    }

    #[test]
    fn fps_derives_from_frame_interval() {
        let timings = FrameTimings {
            frame_interval: Duration::from_millis(20),
            ..Default::default()
        };
        assert!((timings.instantaneous_fps() - 50.0).abs() < 0.01);
    }

    #[test]
    fn zero_interval_reports_zero_fps_rather_than_infinity() {
        let timings = FrameTimings::default();
        assert_eq!(timings.instantaneous_fps(), 0.0);
    }

    #[test]
    fn mailbox_returns_newest_and_counts_dropped() {
        let mailbox = LatestFrame::new();
        mailbox.publish(sample_result(1));
        mailbox.publish(sample_result(2));
        mailbox.publish(sample_result(3));

        // Only the freshest survives; the two it replaced are counted.
        let taken = mailbox.take().expect("a result should be available");
        assert_eq!(taken.frame_id, 3);
        assert_eq!(mailbox.dropped_count(), 2);
    }

    #[test]
    fn mailbox_is_empty_after_taking() {
        let mailbox = LatestFrame::new();
        mailbox.publish(sample_result(1));
        assert!(mailbox.take().is_some());
        assert!(mailbox.take().is_none());
        // Nothing was overwritten, so nothing counts as dropped.
        assert_eq!(mailbox.dropped_count(), 0);
    }
}
