//! The dog: Yohai Simhony's animated MapleSyrup mascot (the idle loop from
//! his voice-companion PR, `ui/animations/idle/idle_01`), packed into one
//! sprite sheet that ships inside the program. The panel over the game and
//! the phone page both show it; it moves faster while MapleSyrup talks.

use std::collections::HashMap;

use image::{RgbaImage, imageops};

/// The sheet: 120 frames of 128×160, twelve to a row.
pub const SHEET_PNG: &[u8] = include_bytes!("../../assets/companion/dog-idle.png");
pub const SHEET_JSON: &str = include_str!("../../assets/companion/dog-idle.json");
pub const FRAME_W: u32 = 128;
pub const FRAME_H: u32 = 160;
pub const COLUMNS: u32 = 12;
pub const FRAMES: u32 = 120;
pub const FPS: f64 = 15.0;
/// How much faster it moves while talking.
pub const TALKING_SPEED: f64 = 1.7;

pub struct Dog {
    sheet: RgbaImage,
    /// Frames already scaled (and premultiplied), by height and index.
    cache: HashMap<(u32, u32), RgbaImage>,
    phase: f64,
    last: Option<std::time::Instant>,
}

impl Dog {
    pub fn load() -> Option<Dog> {
        let sheet = image::load_from_memory(SHEET_PNG).ok()?.to_rgba8();
        (sheet.width() >= FRAME_W * COLUMNS).then_some(Dog {
            sheet,
            cache: HashMap::new(),
            phase: 0.0,
            last: None,
        })
    }

    /// The frame to show now: the loop advances with real time, faster while
    /// `talking`.
    pub fn advance(&mut self, talking: bool) -> u32 {
        let now = std::time::Instant::now();
        let dt = self
            .last
            .map(|l| now.duration_since(l).as_secs_f64())
            .unwrap_or(0.0);
        self.last = Some(now);
        let speed = if talking { TALKING_SPEED } else { 1.0 };
        self.phase = (self.phase + dt.min(0.5) * FPS * speed) % FRAMES as f64;
        self.phase as u32
    }

    /// Frame `index` scaled to `height` pixels, with premultiplied alpha.
    pub fn frame(&mut self, index: u32, height: u32) -> &RgbaImage {
        let index = index % FRAMES;
        let sheet = &self.sheet;
        self.cache.entry((height, index)).or_insert_with(|| {
            let (x, y) = ((index % COLUMNS) * FRAME_W, (index / COLUMNS) * FRAME_H);
            let tile = imageops::crop_imm(sheet, x, y, FRAME_W, FRAME_H).to_image();
            let width = (FRAME_W as f32 * height as f32 / FRAME_H as f32)
                .round()
                .max(1.0) as u32;
            let mut scaled =
                imageops::resize(&tile, width, height.max(1), imageops::FilterType::Triangle);
            for p in scaled.pixels_mut() {
                let a = p.0[3] as u32;
                for c in 0..3 {
                    p.0[c] = ((p.0[c] as u32 * a + 127) / 255) as u8;
                }
            }
            scaled
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sheet_is_inside_and_its_frames_are_whole() {
        let mut dog = Dog::load().expect("the sheet decodes");
        let meta: serde_json::Value = serde_json::from_str(SHEET_JSON).unwrap();
        assert_eq!(meta["frames"], FRAMES);
        assert_eq!(meta["frame_width"], FRAME_W);
        let frame = dog.frame(0, 100).clone();
        assert_eq!(frame.dimensions(), (80, 100));
        // Transparent corners, an opaque middle, premultiplied throughout.
        assert_eq!(frame.get_pixel(0, 0).0[3], 0);
        assert!(frame.get_pixel(40, 60).0[3] > 200);
        assert!(
            frame
                .pixels()
                .all(|p| p.0[0] <= p.0[3] && p.0[1] <= p.0[3] && p.0[2] <= p.0[3])
        );
        // The last frame is there too.
        assert!(dog.frame(FRAMES - 1, 100).get_pixel(40, 60).0[3] > 200);
    }

    #[test]
    fn talking_moves_it_faster() {
        let mut quiet = Dog::load().unwrap();
        let mut talking = Dog::load().unwrap();
        quiet.advance(false);
        talking.advance(true);
        std::thread::sleep(std::time::Duration::from_millis(200));
        let (a, b) = (quiet.advance(false), talking.advance(true));
        assert!(b > a, "{a} {b}");
    }
}
