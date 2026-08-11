//! Shared domain types for MapleSyrup.
//!
//! Every stage crate depends on this crate and on nothing else in the
//! workspace, so stages stay swappable and independently runnable.
//! This crate performs no I/O.

use serde::{Deserialize, Serialize};

pub mod error;

pub use error::{Error, Result};

/// A captured screen image plus the metadata needed to interpret and
/// locate it. Pixels are RGBA8, row-major, `width * height * 4` bytes.
#[derive(Clone)]
pub struct Frame {
    pub id: FrameId,
    /// Milliseconds since the pipeline started, not wall-clock: frames
    /// are compared and ordered, never dated.
    pub elapsed_ms: u64,
    pub width: u32,
    pub height: u32,
    /// Screen-space origin of this frame, so annotations computed from
    /// a region can be placed correctly on the full desktop.
    pub origin: Point,
    pub pixels: Vec<u8>,
    pub source: CaptureSource,
}

impl Frame {
    pub fn expected_len(width: u32, height: u32) -> usize {
        width as usize * height as usize * 4
    }

    /// Returns an error rather than panicking on a malformed buffer:
    /// capture backends are external code and can surprise us.
    pub fn new(
        id: FrameId,
        elapsed_ms: u64,
        width: u32,
        height: u32,
        origin: Point,
        pixels: Vec<u8>,
        source: CaptureSource,
    ) -> Result<Self> {
        let expected = Self::expected_len(width, height);
        if pixels.len() != expected {
            return Err(Error::MalformedFrame {
                expected,
                actual: pixels.len(),
            });
        }
        Ok(Self {
            id,
            elapsed_ms,
            width,
            height,
            origin,
            pixels,
            source,
        })
    }
}

impl std::fmt::Debug for Frame {
    /// Never dump pixel data into logs.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Frame")
            .field("id", &self.id)
            .field("elapsed_ms", &self.elapsed_ms)
            .field("size", &format_args!("{}x{}", self.width, self.height))
            .field("origin", &self.origin)
            .field("source", &self.source)
            .field("bytes", &self.pixels.len())
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct FrameId(pub u64);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CaptureSource {
    Monitor { name: String },
    Window { title: String },
    Region,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// What the vision stage understood from one frame.
///
/// Structured, never prose: the agent and overlay consume data. A vision
/// backend that cannot produce this must fail, not return an empty
/// perception that reads as "nothing on screen".
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Perception {
    pub frame_id: FrameId,
    pub elements: Vec<Element>,
    /// One-line description of the screen as a whole, for agent context.
    pub summary: String,
    /// Backend that produced this, e.g. "anthropic:claude-*" or "mock".
    pub backend: String,
}

/// A thing the vision stage located on screen.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Element {
    pub bounds: Rect,
    pub kind: ElementKind,
    pub label: String,
    /// Model-reported confidence in [0, 1]. Deterministic sources use 1.0.
    pub confidence: f32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ElementKind {
    Text,
    Button,
    Input,
    Image,
    Window,
    Other(String),
}

/// Something the agent wants drawn on the overlay.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Annotation {
    pub id: u64,
    pub shape: Shape,
    pub style: Style,
    /// Overlay drops the annotation after this many ms; `None` = until
    /// replaced. Prevents stale guidance from lingering over the desktop.
    pub ttl_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Shape {
    Box { bounds: Rect },
    Label { at: Point, text: String },
    Arrow { from: Point, to: Point },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Style {
    Info,
    Highlight,
    Warning,
}

/// Messages the pipeline sends to the overlay and CLI.
///
/// `StageFailed` exists so a dead stage is visible rather than silently
/// stalling the pipeline (design rule 4).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AgentEvent {
    Annotations(Vec<Annotation>),
    Status(String),
    StageFailed { stage: String, error: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn px(w: u32, h: u32) -> Vec<u8> {
        vec![0; Frame::expected_len(w, h)]
    }

    #[test]
    fn frame_accepts_correctly_sized_buffer() {
        let f = Frame::new(
            FrameId(1),
            0,
            4,
            2,
            Point::default(),
            px(4, 2),
            CaptureSource::Region,
        );
        assert!(f.is_ok());
    }

    #[test]
    fn frame_rejects_malformed_buffer() {
        let err = Frame::new(
            FrameId(1),
            0,
            4,
            2,
            Point::default(),
            vec![0; 3],
            CaptureSource::Region,
        )
        .unwrap_err();
        assert!(matches!(err, Error::MalformedFrame { .. }));
    }

    #[test]
    fn frame_debug_does_not_dump_pixels() {
        let f = Frame::new(
            FrameId(7),
            12,
            2,
            2,
            Point::default(),
            px(2, 2),
            CaptureSource::Region,
        )
        .unwrap();
        let s = format!("{f:?}");
        assert!(s.contains("bytes: 16"));
        assert!(!s.contains("0, 0, 0"));
    }
}
