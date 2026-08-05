use image::{ImageBuffer, Rgba};
use std::path::Path;

/// Frame represents a single RGBA8 image captured from a source.
#[derive(Clone, Debug)]
pub struct Frame {
    /// width in pixels
    pub width: u32,
    /// height in pixels
    pub height: u32,
    /// rgba pixels, row-major order
    pub pixels: Vec<u8>,
}

impl Frame {
    /// Construct a Frame from raw RGBA bytes
    pub fn from_rgba(width: u32, height: u32, pixels: Vec<u8>) -> Self {
        Self { width, height, pixels }
    }

    /// Save frame as a PNG to path. Returns an io::Error on failure.
    pub fn save_png<P: AsRef<Path>>(&self, path: P) -> Result<(), image::ImageError> {
        let buf: ImageBuffer<Rgba<u8>, _> = ImageBuffer::from_raw(self.width, self.height, self.pixels.clone())
            .expect("slice has correct length");
        buf.save(path)
    }
}
