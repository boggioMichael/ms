//! Cheap local change detection.
//!
//! Vision calls are the expensive part of the pipeline. Most desktop
//! frames are identical to the one before them, so a sampled pixel
//! comparison decides whether a frame is worth sending at all.

use ms_core::Frame;

/// Decides whether a frame differs enough from the last one to be worth
/// perceiving.
pub struct ChangeDetector {
    last: Option<Vec<u8>>,
    /// Fraction of sampled bytes that must differ, in [0, 1].
    threshold: f32,
    /// Sample every Nth byte. Full comparison of a 4K frame is ~33 MB
    /// of work per frame; sampling keeps this negligible.
    stride: usize,
}

impl ChangeDetector {
    pub fn new(threshold: f32) -> Self {
        Self {
            last: None,
            threshold: threshold.clamp(0.0, 1.0),
            stride: 97,
        }
    }

    /// True when `frame` should be sent to the vision backend. The
    /// first frame, and any frame whose dimensions changed, always
    /// count as changed.
    pub fn is_changed(&mut self, frame: &Frame) -> bool {
        let sampled: Vec<u8> = frame.pixels.iter().step_by(self.stride).copied().collect();

        let changed = match &self.last {
            None => true,
            Some(prev) if prev.len() != sampled.len() => true,
            Some(prev) => {
                let differing = prev.iter().zip(&sampled).filter(|(a, b)| a != b).count();
                let ratio = differing as f32 / sampled.len().max(1) as f32;
                ratio > self.threshold
            }
        };

        self.last = Some(sampled);
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ms_core::{CaptureSource, FrameId, Point};

    fn frame_of(id: u64, fill: u8, w: u32, h: u32) -> Frame {
        Frame::new(
            FrameId(id),
            id,
            w,
            h,
            Point::default(),
            vec![fill; Frame::expected_len(w, h)],
            CaptureSource::Region,
        )
        .unwrap()
    }

    #[test]
    fn first_frame_always_changed() {
        let mut d = ChangeDetector::new(0.01);
        assert!(d.is_changed(&frame_of(0, 0, 64, 64)));
    }

    #[test]
    fn identical_frame_is_unchanged() {
        let mut d = ChangeDetector::new(0.01);
        d.is_changed(&frame_of(0, 7, 64, 64));
        assert!(!d.is_changed(&frame_of(1, 7, 64, 64)));
    }

    #[test]
    fn wholly_different_frame_is_changed() {
        let mut d = ChangeDetector::new(0.01);
        d.is_changed(&frame_of(0, 0, 64, 64));
        assert!(d.is_changed(&frame_of(1, 255, 64, 64)));
    }

    #[test]
    fn resize_counts_as_changed() {
        let mut d = ChangeDetector::new(0.01);
        d.is_changed(&frame_of(0, 5, 64, 64));
        assert!(d.is_changed(&frame_of(1, 5, 128, 128)));
    }
}
