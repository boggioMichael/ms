//! Aggregated world state snapshot and pipeline orchestrator.
//!
//! The [`PerceptionPipeline`] owns all detector instances and produces a
//! single [`WorldState`] struct per frame, collecting the detectors' outputs
//! into a convenient, type-safe bag of information. Downstream AI modules
//! query this struct without needing to know which detector produced each piece
//! of information or what confidence it carries — all of that is encoded in the
//! `Detection<T>` wrapper each value carries.

use image::RgbaImage;

use crate::vision::detectors::{
    combat::CombatIntensityDetector,
    dialog::DialogDetector,
    environment::FootholdDetector,
    hud::HudDetector,
    motion::MotionDetector,
    panels::{ChatLogDetector, IconRowDetector, MinimapDetector},
};
use crate::vision::types::Detection;

/// Complete observed world state from a single frame, carrying confidence
/// and reliability metadata on every component.
#[derive(Debug, Clone)]
pub struct WorldState {
    pub hud: crate::vision::detectors::hud::HudReading,
    pub motion: Detection<Vec<crate::vision::detectors::motion::MovingEntity>>,
    pub dialog: Detection<crate::vision::detectors::dialog::DialogReading>,
    pub minimap: Detection<crate::vision::detectors::panels::MinimapReading>,
    pub chat_log: Detection<crate::vision::detectors::panels::ChatLogReading>,
    pub icon_row: Detection<crate::vision::detectors::panels::IconRowReading>,
    pub footholds: Detection<Vec<crate::vision::detectors::environment::PlatformEdge>>,
    pub combat_intensity: Detection<crate::vision::detectors::combat::CombatReading>,
}

/// Orchestrates all detectors and produces a [`WorldState`] per frame.
///
/// Owns mutable detector state (motion tracker, combat history) so callers
/// don't have to track it themselves. Create once, call `detect()` once per
/// frame with the latest image.
pub struct PerceptionPipeline {
    hud: HudDetector,
    motion: MotionDetector,
    dialog: DialogDetector,
    minimap: MinimapDetector,
    chat_log: ChatLogDetector,
    icon_row: IconRowDetector,
    footholds: FootholdDetector,
    combat: CombatIntensityDetector,
    /// Counts frames when the caller does not supply an id of its own.
    frame_id: u64,
}

impl Default for PerceptionPipeline {
    fn default() -> Self {
        Self::new()
    }
}

impl PerceptionPipeline {
    pub fn new() -> Self {
        Self {
            hud: HudDetector::new(),
            motion: MotionDetector::new(Default::default()),
            dialog: DialogDetector::new(Default::default()),
            minimap: MinimapDetector,
            chat_log: ChatLogDetector,
            icon_row: IconRowDetector::default(),
            footholds: FootholdDetector::new(Default::default()),
            combat: CombatIntensityDetector::default(),
            frame_id: 0,
        }
    }

    /// Run all detectors on the current frame and return an aggregated
    /// [`WorldState`].
    pub fn detect(&mut self, image: &RgbaImage) -> WorldState {
        self.frame_id = self.frame_id.wrapping_add(1);
        self.detect_frame(image, self.frame_id)
    }

    /// Run detection for an explicitly numbered frame, so OCR provenance
    /// records the same frame id the rest of the pipeline reports.
    pub fn detect_frame(&mut self, image: &RgbaImage, frame_id: u64) -> WorldState {
        let hud = self.hud.detect(image, frame_id);
        let motion = self.motion.detect(image);
        let dialog = self.dialog.detect(image);
        let minimap = self.minimap.detect(image);
        let chat_log = self.chat_log.detect(image);
        let icon_row = self.icon_row.detect(image);
        let footholds = self.footholds.detect(image);

        let motion_count = motion.value.as_deref().map(|v| v.len()).unwrap_or(0);
        let diff_magnitude = self.motion.last_diff_magnitude();
        let combat_intensity = self.combat.observe(motion_count, diff_magnitude);

        WorldState {
            hud,
            motion,
            dialog,
            minimap,
            chat_log,
            icon_row,
            footholds,
            combat_intensity,
        }
    }

    pub fn motion_entity_count(&self) -> usize {
        self.motion.tracked_entity_count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    #[test]
    fn pipeline_produces_world_state_on_empty_frame() {
        let mut pipeline = PerceptionPipeline::new();
        let image = RgbaImage::from_pixel(600, 400, Rgba([30, 30, 30, 255]));
        let _state = pipeline.detect(&image);
        // Sanity check: pipeline ran without panicking. Actual detections
        // are tested in their respective detector modules.
    }
}
