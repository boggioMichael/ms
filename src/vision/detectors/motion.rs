//! Moving-entity detection is Syrup's: frame differencing against the
//! previous frame, blobs with stable identities from its tracker. With a
//! static game camera, anything that moves between two frames is the
//! player, a monster, a dropped item settling, an NPC or a UI animation; it
//! does not say which.

pub use syrup::motion::{MotionConfig, MotionDetector, MovingBlob};

/// A moving region with a stable identity, as MapleSyrup has always called it.
pub type MovingEntity = MovingBlob;
