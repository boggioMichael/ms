//! Graphical preview window for the vision engine.
//!
//! A thin shell around [`crate::observe::overlay`]: it owns an OS window and
//! a scratch buffer, and does nothing else. All drawing decisions live in the
//! overlay module so they stay testable without a display.
//!
//! The window is intentionally a plain framebuffer rather than a UI toolkit.
//! The preview only ever needs "show these pixels", and a full toolkit would
//! add an event loop and a widget tree for no benefit here.

use image::RgbaImage;
use minifb::{Key, Window, WindowOptions};

use crate::observe::frame_result::VisionFrameResult;
use crate::observe::overlay;

/// Largest window edge; larger captures are shown scaled down so a
/// full-resolution game window still fits on screen.
const MAX_WINDOW_EDGE: usize = 1280;

pub struct Preview {
    window: Window,
    /// Reused between frames so steady-state rendering does not allocate.
    buffer: Vec<u32>,
    width: usize,
    height: usize,
}

impl Preview {
    /// Open the preview window sized for `(frame_width, frame_height)`.
    pub fn open(frame_width: u32, frame_height: u32) -> Result<Self, minifb::Error> {
        let (width, height) =
            fit_within(frame_width as usize, frame_height as usize, MAX_WINDOW_EDGE);

        let mut window = Window::new(
            "MapleSyrup — Vision Preview",
            width,
            height,
            WindowOptions {
                resize: true,
                ..WindowOptions::default()
            },
        )?;
        // Cap the window's own refresh so it cannot spin faster than the
        // pipeline produces frames.
        window.set_target_fps(60);

        Ok(Self {
            window,
            buffer: vec![0; width * height],
            width,
            height,
        })
    }

    /// Whether the window is still open and the user has not pressed Escape.
    pub fn is_open(&self) -> bool {
        self.window.is_open() && !self.window.is_key_down(Key::Escape)
    }

    /// Render one result. Cheap when the window was closed.
    pub fn show(&mut self, result: &VisionFrameResult) {
        if !self.is_open() {
            return;
        }
        let annotated = overlay::render_overlay(result);
        self.blit(&annotated);
    }

    /// Keep the window responsive when no new frame has arrived, so it does
    /// not appear hung while the pipeline waits on capture.
    pub fn pump(&mut self) {
        if self.window.is_open() {
            self.window.update();
        }
    }

    fn blit(&mut self, image: &RgbaImage) {
        let (window_width, window_height) = self.window.get_size();
        if window_width != self.width || window_height != self.height {
            self.width = window_width.max(1);
            self.height = window_height.max(1);
            self.buffer.resize(self.width * self.height, 0);
        }

        let (src_width, src_height) = (image.width() as usize, image.height() as usize);
        if src_width == 0 || src_height == 0 {
            return;
        }

        // Nearest-neighbour scale straight into the window buffer. This
        // avoids a second intermediate image on every displayed frame.
        let raw = image.as_raw();
        for y in 0..self.height {
            let src_y = y * src_height / self.height;
            let row_start = src_y * src_width * 4;
            for x in 0..self.width {
                let src_x = x * src_width / self.width;
                let index = row_start + src_x * 4;
                let r = raw[index] as u32;
                let g = raw[index + 1] as u32;
                let b = raw[index + 2] as u32;
                self.buffer[y * self.width + x] = (r << 16) | (g << 8) | b;
            }
        }

        let _ = self
            .window
            .update_with_buffer(&self.buffer, self.width, self.height);
    }
}

/// Scale `(width, height)` down to fit `max_edge`, preserving aspect ratio.
/// Never scales up, so small fixtures are shown at their native size.
pub fn fit_within(width: usize, height: usize, max_edge: usize) -> (usize, usize) {
    if width == 0 || height == 0 {
        return (max_edge.min(640), max_edge.min(480));
    }
    let longest = width.max(height);
    if longest <= max_edge {
        return (width, height);
    }
    let scaled_width = width * max_edge / longest;
    let scaled_height = height * max_edge / longest;
    (scaled_width.max(1), scaled_height.max(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_frames_are_not_scaled_up() {
        assert_eq!(fit_within(640, 480, 1280), (640, 480));
    }

    #[test]
    fn large_frames_scale_down_preserving_aspect_ratio() {
        let (w, h) = fit_within(2560, 1440, 1280);
        assert_eq!(w, 1280);
        // 1440 * 1280 / 2560 = 720
        assert_eq!(h, 720);
    }

    #[test]
    fn portrait_frames_clamp_on_their_longest_edge() {
        let (w, h) = fit_within(1000, 2000, 1000);
        assert_eq!(h, 1000);
        assert_eq!(w, 500);
    }

    #[test]
    fn degenerate_sizes_do_not_divide_by_zero() {
        let (w, h) = fit_within(0, 0, 1280);
        assert!(w > 0 && h > 0);
    }
}
