//! Frame inspector utilities
//!
//! Frame provides a lightweight, borrowed view over RGBA image data and
//! exposes metadata commonly needed by detectors: resolution, timestamp,
//! pixel format, frame index and size.

use std::time::SystemTime;

use image::{ImageBuffer, Rgba};

/// A borrowed view of a frame in RGBA8 format.
///
/// Designed to be zero-copy: callers can construct a Frame from a
/// reference to an existing ImageBuffer and call inspection helpers.
#[derive(Debug)]
pub struct Frame<'a> {
    /// Underlying image as RGBA8
    pub image: &'a ImageBuffer<Rgba<u8>, Vec<u8>>,
    /// Capture timestamp
    pub timestamp: SystemTime,
    /// Monotonic frame index (capture-provided)
    pub index: u64,
}

impl<'a> Frame<'a> {
    /// Create a new Frame from an RGBA ImageBuffer reference.
    pub fn new(
        image: &'a ImageBuffer<Rgba<u8>, Vec<u8>>,
        timestamp: SystemTime,
        index: u64,
    ) -> Self {
        Self {
            image,
            timestamp,
            index,
        }
    }

    /// Image resolution as (width, height)
    pub fn resolution(&self) -> (u32, u32) {
        (self.image.width(), self.image.height())
    }

    /// Pixel format name
    pub fn pixel_format(&self) -> &'static str {
        "RGBA8"
    }

    /// Frame index
    pub fn frame_index(&self) -> u64 {
        self.index
    }

    /// Size in bytes of the frame buffer
    pub fn size_bytes(&self) -> usize {
        self.image.as_raw().len()
    }

    /// Height in pixels
    pub fn height(&self) -> u32 {
        self.image.height()
    }

    /// Width in pixels
    pub fn width(&self) -> u32 {
        self.image.width()
    }
}
