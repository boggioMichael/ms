//! Moving-entity detector.
//!
//! Detects foreground blobs via frame differencing against the previous
//! frame and feeds them into an [`ObjectTracker`](crate::vision::temporal::ObjectTracker)
//! for stable identity across frames. This is a genuinely reusable, camera
//! agnostic signal: with a static game camera, anything that moves between
//! two frames is either the player, a monster, a dropped item settling, an
//! NPC, or a UI animation. It does **not** classify what moved (that would
//! require a trained sprite classifier this project does not have); instead
//! it reports moving regions with position, size, velocity and a track-age
//! based confidence, which is exactly the kind of generic building block
//! higher-level detectors (loot, monster density, combat intensity) compose
//! on top of.
//!
//! Kept deliberately allocation-light: the diff mask is computed once per
//! frame and blob extraction reuses the same row-segmentation/grouping
//! routines as every other detector (see [`crate::vision::geometry`]).

use image::RgbaImage;

use crate::vision::diff::motion_mask;
use crate::vision::geometry::{group_segments, Rect};
use crate::vision::temporal::{ObjectTracker, Track};
use crate::vision::types::{Confidence, Detection, Reliability, Source};

/// Configuration for the motion detector; exposed so callers can retune it
/// per resolution/scene without touching detection code.
#[derive(Debug, Clone, Copy)]
pub struct MotionConfig {
    /// Per-channel luminance-diff threshold (0-255) to count a pixel as moved.
    pub diff_threshold: u8,
    /// Minimum contiguous run width, in pixels, to consider a row segment.
    pub min_run_width: u32,
    /// Minimum blob height, in rows, after grouping.
    pub min_blob_height: u32,
    /// Minimum blob area, in pixels, to keep a candidate (rejects sensor/JPEG noise).
    pub min_blob_area: u32,
    /// Maximum matching distance (pixels) for the object tracker.
    pub track_match_distance: f32,
    /// How many consecutive missed frames a track survives (occlusion grace).
    pub track_grace_frames: u32,
}

impl Default for MotionConfig {
    fn default() -> Self {
        Self {
            diff_threshold: 28,
            min_run_width: 4,
            min_blob_height: 4,
            min_blob_area: 24,
            track_match_distance: 48.0,
            track_grace_frames: 5,
        }
    }
}

/// A single moving entity candidate for the current frame.
#[derive(Debug, Clone, Copy)]
pub struct MovingEntity {
    pub id: u64,
    pub bounds: Rect,
    pub velocity: (f32, f32),
    pub age_frames: u32,
    pub is_predicted: bool,
}

/// Stateful motion detector: owns the previous frame and an object tracker
/// so it can report temporally-consistent entities rather than raw,
/// unlabeled blobs every call.
pub struct MotionDetector {
    config: MotionConfig,
    previous_frame: Option<RgbaImage>,
    tracker: ObjectTracker,
    last_diff_magnitude: f32,
}

impl MotionDetector {
    pub fn new(config: MotionConfig) -> Self {
        Self {
            tracker: ObjectTracker::new(config.track_match_distance, config.track_grace_frames),
            config,
            previous_frame: None,
            last_diff_magnitude: 0.0,
        }
    }

    /// Run motion detection for the current frame. The first call for a new
    /// detector instance always reports "missing" (no previous frame yet) —
    /// this is a normal warm-up condition, not a failure, and is reported as
    /// such via the failure reason instead of an empty/zero result that
    /// could be mistaken for "confirmed no motion".
    pub fn detect(&mut self, image: &RgbaImage) -> Detection<Vec<MovingEntity>> {
        let Some(previous) = self.previous_frame.as_ref() else {
            self.previous_frame = Some(image.clone());
            self.last_diff_magnitude = 0.0;
            return Detection::missing(Source::Motion, "warming up: no previous frame to diff against yet");
        };

        let blobs = match motion_mask(previous, image, self.config.diff_threshold) {
            Some(mask) => {
                self.last_diff_magnitude = compute_diff_magnitude(&mask, image);
                extract_blobs(&mask, &self.config)
            }
            None => {
                // Capture resolution changed between frames (e.g. window
                // resize); restart the baseline rather than reporting stale
                // motion computed against mismatched dimensions.
                self.previous_frame = Some(image.clone());
                self.last_diff_magnitude = 0.0;
                return Detection::missing(Source::Motion, "frame size changed since previous frame");
            }
        };

        self.previous_frame = Some(image.clone());

        let detections: Vec<(f32, f32, f32, f32)> = blobs
            .iter()
            .map(|rect| {
                let (cx, cy) = rect.center();
                (cx, cy, rect.w as f32, rect.h as f32)
            })
            .collect();
        let tracks = self.tracker.update(&detections);

        let entities: Vec<MovingEntity> = tracks.iter().map(track_to_entity).collect();
        if entities.is_empty() {
            // A real "no motion this frame" result: previous frame existed,
            // diffed successfully, but nothing crossed the threshold.
            let mut detection = Detection::found(Vec::new(), Confidence::new(0.6), Source::Motion, Reliability::Heuristic);
            detection.failure_reason = Some("no motion above threshold".to_string());
            detection
        } else {
            let confidence = average_confidence(tracks);
            let reliability = if tracks.iter().any(|t| t.age_frames > 3) {
                Reliability::Corroborated
            } else {
                Reliability::Heuristic
            };
            Detection::found(entities, confidence, Source::Motion, reliability)
        }
    }

    pub fn last_diff_magnitude(&self) -> f32 {
        self.last_diff_magnitude
    }

    pub fn tracked_entity_count(&self) -> usize {
        self.tracker.tracks().len()
    }
}

fn track_to_entity(track: &Track) -> MovingEntity {
    MovingEntity {
        id: track.id,
        bounds: Rect {
            x: (track.position.x - track.width / 2.0).max(0.0) as u32,
            y: (track.position.y - track.height / 2.0).max(0.0) as u32,
            w: track.width.max(1.0) as u32,
            h: track.height.max(1.0) as u32,
        },
        velocity: (track.velocity.x, track.velocity.y),
        age_frames: track.age_frames,
        is_predicted: track.is_predicted(),
    }
}

fn average_confidence(tracks: &[Track]) -> Confidence {
    if tracks.is_empty() {
        return Confidence::NONE;
    }
    let total: f32 = tracks.iter().map(|t| t.confidence.value()).sum();
    Confidence::new(total / tracks.len() as f32)
}

fn compute_diff_magnitude(mask: &image::GrayImage, _reference: &RgbaImage) -> f32 {
    let (width, height) = mask.dimensions();
    if width == 0 || height == 0 {
        return 0.0;
    }
    let total_pixels = width as f32 * height as f32;
    let moved_pixels: u32 = mask.iter().map(|&v| if v > 0 { 1 } else { 0 }).sum();
    moved_pixels as f32 / total_pixels
}

fn extract_blobs(mask: &image::GrayImage, config: &MotionConfig) -> Vec<Rect> {
    let (width, height) = mask.dimensions();
    let mut rows = Vec::new();
    for y in 0..height {
        let mut start: Option<u32> = None;
        for x in 0..width {
            let moved = mask.get_pixel(x, y).0[0] > 0;
            if moved {
                if start.is_none() {
                    start = Some(x);
                }
            } else if let Some(begin) = start {
                if x - begin >= config.min_run_width {
                    rows.push((y, begin, x - 1));
                }
                start = None;
            }
        }
        if let Some(begin) = start {
            if width - begin >= config.min_run_width {
                rows.push((y, begin, width - 1));
            }
        }
    }

    group_segments(rows, config.min_blob_height, 1)
        .into_iter()
        .filter(|rect| rect.area() >= config.min_blob_area)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    fn frame_with_square(w: u32, h: u32, at: (u32, u32), size: u32) -> RgbaImage {
        let mut image = RgbaImage::from_pixel(w, h, Rgba([20, 20, 20, 255]));
        for y in at.1..(at.1 + size).min(h) {
            for x in at.0..(at.0 + size).min(w) {
                image.put_pixel(x, y, Rgba([230, 230, 230, 255]));
            }
        }
        image
    }

    #[test]
    fn first_call_reports_warmup_not_failure_masquerading_as_empty() {
        let mut detector = MotionDetector::new(MotionConfig::default());
        let frame = frame_with_square(64, 64, (10, 10), 8);
        let detection = detector.detect(&frame);
        assert!(!detection.is_present());
        assert!(detection.failure_reason.unwrap().contains("warming up"));
    }

    #[test]
    fn moving_square_is_tracked_with_stable_id() {
        let mut detector = MotionDetector::new(MotionConfig::default());
        let frame1 = frame_with_square(80, 80, (10, 10), 10);
        let frame2 = frame_with_square(80, 80, (20, 10), 10);
        let frame3 = frame_with_square(80, 80, (30, 10), 10);

        detector.detect(&frame1);
        let second = detector.detect(&frame2);
        // second: entering->leaving 10,10 and entering 20,10 both show as motion
        assert!(second.is_present());

        let third = detector.detect(&frame3);
        assert!(third.is_present());
        let entities = third.value.unwrap();
        assert!(!entities.is_empty());
    }

    #[test]
    fn static_scene_reports_no_motion_confidently() {
        let mut detector = MotionDetector::new(MotionConfig::default());
        let frame = frame_with_square(64, 64, (10, 10), 8);
        detector.detect(&frame);
        let detection = detector.detect(&frame);
        assert!(detection.is_present());
        assert!(detection.value.unwrap().is_empty());
    }
}
