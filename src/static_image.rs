use crate::errors::CaptureError;
use crate::frame::Frame;
use crate::frame_source::FrameSource;
use image::GenericImageView;
use log::info;
use std::path::Path;

/// StaticImageCapture loads assets/test.png and returns the same frame repeatedly.
pub struct StaticImageCapture {
    frame: Frame,
}

impl StaticImageCapture {
    /// Try to load assets/test.png. If the file does not exist, return Err(CaptureError::NoTestSource).
    pub fn try_load_from_assets() -> Result<Self, CaptureError> {
        let path = Path::new("assets/test.png");
        if !path.exists() {
            return Err(CaptureError::NoTestSource);
        }

        match image::open(path) {
            Ok(img) => {
                let img = img.to_rgba8();
                let (w, h) = img.dimensions();
                let pixels = img.into_raw();
                info!("Loaded assets/test.png {}x{}", w, h);
                Ok(Self { frame: Frame::from_rgba(w, h, pixels) })
            }
            Err(e) => Err(CaptureError::Image(e)),
        }
    }

    #[allow(dead_code)]
    fn generate_placeholder() -> Self {
        // Generate a small in-memory placeholder (not used by default behavior)
        let w = 64u32;
        let h = 64u32;
        let mut pixels = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            for x in 0..w {
                pixels.push(((x * 255) / w) as u8);
                pixels.push(((y * 255) / h) as u8);
                pixels.push(64u8);
                pixels.push(255u8);
            }
        }
        Self { frame: Frame::from_rgba(w, h, pixels) }
    }
}

impl FrameSource for StaticImageCapture {
    fn next_frame(&mut self) -> Result<Frame, CaptureError> {
        Ok(self.frame.clone())
    }

    fn width(&self) -> Option<u32> {
        Some(self.frame.width)
    }

    fn height(&self) -> Option<u32> {
        Some(self.frame.height)
    }
}
