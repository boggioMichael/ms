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
    hud::{HudDetector, HudReading},
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

/// Which detectors a frame runs through.
///
/// The companion consumes the HUD alone; the rest — motion, dialogs, the
/// panels, the platform edges, the combat gauge — feed the preview window
/// and the tools, and cost a frame's worth of work each. A pipeline runs
/// only what is asked for and reports the rest as not run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Detectors {
    /// The HUD's geometry: the bars and their fills, every frame.
    pub hud: bool,
    /// The HUD's text through OCR, on its cadence: for the preview; the
    /// companion reads the numbers in the game's font instead
    /// (`sight::numbers`).
    pub hud_text: bool,
    pub motion: bool,
    pub dialog: bool,
    pub panels: bool,
    pub footholds: bool,
}

impl Detectors {
    /// Everything, for a view that shows everything.
    pub const ALL: Detectors = Detectors {
        hud: true,
        hud_text: true,
        motion: true,
        dialog: true,
        panels: true,
        footholds: true,
    };

    /// What the companion itself consumes: no OCR on the per-frame path.
    pub const HUD: Detectors = Detectors {
        hud: true,
        hud_text: false,
        motion: false,
        dialog: false,
        panels: false,
        footholds: false,
    };
}

impl Default for Detectors {
    fn default() -> Self {
        Detectors::ALL
    }
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

    /// Run every detector for an explicitly numbered frame, so OCR
    /// provenance records the same frame id the rest of the pipeline
    /// reports.
    pub fn detect_frame(&mut self, image: &RgbaImage, frame_id: u64) -> WorldState {
        self.detect_some(image, frame_id, Detectors::ALL)
    }

    /// Run the detectors in `wanted` for frame `frame_id`; the others
    /// report themselves as not run.
    ///
    /// Each detector runs inside a `TRACE` span named after it
    /// (`vision.motion`, …), so a [`crate::util::stages::StageRecorder`]
    /// can time the stages of a frame one by one.
    pub fn detect_some(
        &mut self,
        image: &RgbaImage,
        frame_id: u64,
        wanted: Detectors,
    ) -> WorldState {
        fn skipped<T>(source: &'static str) -> Detection<T> {
            Detection::missing(source, "not run: nothing is showing it")
        }
        let hud = if wanted.hud {
            tracing::trace_span!("vision.hud")
                .in_scope(|| self.hud.detect_with(image, frame_id, wanted.hud_text))
        } else {
            HudReading::not_run()
        };
        let motion = if wanted.motion {
            tracing::trace_span!("vision.motion").in_scope(|| self.motion.detect(image))
        } else {
            skipped("motion")
        };
        let dialog = if wanted.dialog {
            tracing::trace_span!("vision.dialog").in_scope(|| self.dialog.detect(image, frame_id))
        } else {
            skipped("dialog")
        };
        let (minimap, chat_log, icon_row) = if wanted.panels {
            (
                tracing::trace_span!("vision.minimap").in_scope(|| self.minimap.detect(image)),
                tracing::trace_span!("vision.chat_log").in_scope(|| self.chat_log.detect(image)),
                tracing::trace_span!("vision.icon_row").in_scope(|| self.icon_row.detect(image)),
            )
        } else {
            (skipped("panel"), skipped("panel"), skipped("panel"))
        };
        let footholds = if wanted.footholds {
            tracing::trace_span!("vision.footholds").in_scope(|| self.footholds.detect(image))
        } else {
            skipped("environment")
        };

        let combat_intensity = if wanted.motion {
            let motion_count = motion.value.as_deref().map(|v| v.len()).unwrap_or(0);
            let diff_magnitude = self.motion.last_diff_magnitude();
            tracing::trace_span!("vision.combat")
                .in_scope(|| self.combat.observe(motion_count, diff_magnitude))
        } else {
            skipped("combat")
        };

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
        self.motion.tracked_blob_count()
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

    #[test]
    fn only_the_detectors_asked_for_run() {
        let mut pipeline = PerceptionPipeline::new();
        let image = RgbaImage::from_pixel(600, 400, Rgba([30, 30, 30, 255]));
        let state = pipeline.detect_some(&image, 1, Detectors::HUD);
        for (name, reason) in [
            ("motion", state.motion.failure_reason.as_deref()),
            ("dialog", state.dialog.failure_reason.as_deref()),
            ("minimap", state.minimap.failure_reason.as_deref()),
            ("footholds", state.footholds.failure_reason.as_deref()),
            ("combat", state.combat_intensity.failure_reason.as_deref()),
        ] {
            assert_eq!(reason, Some("not run: nothing is showing it"), "{name}");
        }
        // The HUD ran (and found nothing on a flat frame, for its own reason).
        assert_ne!(
            state.hud.hp.failure_reason.as_deref(),
            Some("not run: nothing is showing it")
        );
        assert_eq!(pipeline.motion_entity_count(), 0);
    }
}
