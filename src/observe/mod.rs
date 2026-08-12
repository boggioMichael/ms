//! Runtime observability for the vision engine.
//!
//! One capture and one perception run produce a single
//! [`frame_result::VisionFrameResult`], which both views render from:
//!
//! ```text
//!            capture -> perception
//!                        |
//!                 VisionFrameResult
//!                        |
//!              +---------+---------+
//!              |                   |
//!        terminal dashboard   vision preview
//! ```
//!
//! The two views can therefore never disagree about a frame. Handoff to the
//! visualization side goes through [`frame_result::LatestFrame`], a
//! single-slot mailbox that drops superseded results rather than queueing
//! them, so a slow display costs visualization frames instead of pipeline
//! throughput.

pub mod dashboard;
pub mod font;
pub mod frame_result;
pub mod overlay;
pub mod preview;
