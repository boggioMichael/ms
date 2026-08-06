//! Detector trait and detector implementations.
//!
//! Every detector below follows the same contract: given a frame (and, for
//! stateful detectors, their own internal history), produce one or more
//! [`crate::vision::types::Detection`] values. Detector outputs are
//! heterogeneous (a `Rect`, a `String`, a `Vec<MovingEntity>`, ...), so
//! rather than forcing them through a `dyn Any` trait object, the
//! [`Detector`] trait documents the shared shape while [`crate::vision::snapshot::PerceptionPipeline`]
//! calls each concrete detector directly and assembles their output into
//! one [`crate::vision::snapshot::WorldState`].

use image::RgbaImage;

use crate::vision::types::Detection;

pub mod combat;
pub mod dialog;
pub mod environment;
pub mod hud;
pub mod motion;
pub mod panels;

/// Common shape every single-shot (stateless) detector implements.
///
/// Stateful detectors (e.g. [`motion::MotionDetector`], which needs the
/// previous frame) take `&mut self` in their own inherent `detect` method
/// instead of implementing this trait, since the trait is defined over
/// `&self` for the common case of purely-geometric single-frame detectors.
pub trait Detector {
    type Output;

    /// Stable identifier used in logs and the `WorldState` summary.
    fn name(&self) -> &'static str;

    fn detect(&self, image: &RgbaImage) -> Detection<Self::Output>;
}
