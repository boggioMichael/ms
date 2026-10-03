//! The dog: the cream chow chow in the syrup captain's hat and cape from the
//! MapleSyrup logo, cut into parts (`assets/companion/dog-parts.png`: the
//! tail, the body, the front paws, the head without its eyes and open mouth,
//! the eyes, the open mouth and the closed smile; made from the logo by
//! `tools/dog_parts.py`) and put together here for the panel over the game.
//! It breathes, blinks, wags its tail, tilts its head while it thinks, and
//! its mouth opens as far as the voice is loud. On the phone it lives in a
//! box of its own (`src/phone/dog.js`, from the same parts).

use std::collections::HashMap;
use std::time::Instant;

use image::{RgbaImage, imageops};
use serde::Deserialize;

pub const PARTS_PNG: &[u8] = include_bytes!("../../assets/companion/dog-parts.png");
pub const PARTS_JSON: &str = include_str!("../../assets/companion/dog-parts.json");

/// Where each part is on the sheet, where it sits on the dog (in the logo's
/// pixels), and where the parts turn.
#[derive(Debug, Deserialize)]
struct Meta {
    size: [f32; 2],
    ground: f32,
    pivots: HashMap<String, [f32; 2]>,
    parts: HashMap<String, Part>,
}

#[derive(Debug, Deserialize, Clone, Copy)]
struct Part {
    /// x, y, width, height on the sheet.
    sheet: [u32; 4],
    /// x, y, width, height on the dog.
    at: [f32; 4],
}

/// How the dog is now, from what MapleSyrup is doing.
#[derive(Debug, Clone, Copy, Default)]
pub struct Mood {
    pub speaking: bool,
    /// How loud the voice is right now (0..1), when it is known.
    pub level: Option<f32>,
    /// Working on an answer: it tilts its head this way and that.
    pub thinking: bool,
}

/// x' = a·x + c·y + e, y' = b·x + d·y + f.
#[derive(Debug, Clone, Copy)]
struct Affine {
    a: f32,
    b: f32,
    c: f32,
    d: f32,
    e: f32,
    f: f32,
}

impl Affine {
    fn scale(sx: f32, sy: f32) -> Affine {
        Affine {
            a: sx,
            b: 0.0,
            c: 0.0,
            d: sy,
            e: 0.0,
            f: 0.0,
        }
    }
    fn translate(x: f32, y: f32) -> Affine {
        Affine {
            a: 1.0,
            b: 0.0,
            c: 0.0,
            d: 1.0,
            e: x,
            f: y,
        }
    }
    fn rotate(r: f32) -> Affine {
        let (s, c) = r.sin_cos();
        Affine {
            a: c,
            b: s,
            c: -s,
            d: c,
            e: 0.0,
            f: 0.0,
        }
    }
    /// This, after `then` (points go through `then` first).
    fn then(self, then: Affine) -> Affine {
        Affine {
            a: self.a * then.a + self.c * then.b,
            b: self.b * then.a + self.d * then.b,
            c: self.a * then.c + self.c * then.d,
            d: self.b * then.c + self.d * then.d,
            e: self.a * then.e + self.c * then.f + self.e,
            f: self.b * then.e + self.d * then.f + self.f,
        }
    }
    /// Turned by `r` round (x, y).
    fn round(self, x: f32, y: f32, r: f32) -> Affine {
        self.then(Affine::translate(x, y))
            .then(Affine::rotate(r))
            .then(Affine::translate(-x, -y))
    }
    fn apply(&self, x: f32, y: f32) -> (f32, f32) {
        (
            self.a * x + self.c * y + self.e,
            self.b * x + self.d * y + self.f,
        )
    }
    fn inverse(&self) -> Option<Affine> {
        let det = self.a * self.d - self.b * self.c;
        if det.abs() < 1e-9 {
            return None;
        }
        let (a, b, c, d) = (self.d / det, -self.b / det, -self.c / det, self.a / det);
        Some(Affine {
            a,
            b,
            c,
            d,
            e: -(a * self.e + c * self.f),
            f: -(b * self.e + d * self.f),
        })
    }
}

pub struct Dog {
    /// The sheet, premultiplied.
    sheet: RgbaImage,
    meta: Meta,
    /// The sheet scaled down for the last height asked for (and by how much).
    sized: Option<(u32, f32, RgbaImage)>,
    clock: f64,
    last: Option<Instant>,
    blink_in: f64,
    blink_t: Option<f64>,
    mouth: f32,
    flap: (f32, f64),
    tilt: f32,
    seed: u64,
}

impl Dog {
    pub fn load() -> Option<Dog> {
        let mut sheet = image::load_from_memory(PARTS_PNG).ok()?.to_rgba8();
        let meta: Meta = serde_json::from_str(PARTS_JSON).ok()?;
        for p in sheet.pixels_mut() {
            let a = p.0[3] as u32;
            for c in 0..3 {
                p.0[c] = ((p.0[c] as u32 * a + 127) / 255) as u8;
            }
        }
        [
            "tail", "body", "pawL", "pawR", "head", "eyeL", "eyeR", "mouth", "smile",
        ]
        .iter()
        .all(|n| meta.parts.contains_key(*n))
        .then_some(Dog {
            sheet,
            meta,
            sized: None,
            clock: 0.0,
            last: None,
            blink_in: 2.0,
            blink_t: None,
            mouth: 0.0,
            flap: (0.0, 0.0),
            tilt: 0.0,
            seed: 0x9e37_79b9_7f4a_7c15,
        })
    }

    fn random(&mut self) -> f64 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 7;
        self.seed ^= self.seed << 17;
        (self.seed >> 11) as f64 / (1u64 << 53) as f64
    }

    /// The dog now, `height` pixels tall, premultiplied (time goes on with
    /// each call).
    pub fn frame(&mut self, height: u32, mood: Mood) -> RgbaImage {
        let now = Instant::now();
        let dt = self
            .last
            .map(|l| now.duration_since(l).as_secs_f64())
            .unwrap_or(0.0)
            .min(0.5);
        self.last = Some(now);
        self.step(dt, mood);
        self.draw(height.max(8))
    }

    fn step(&mut self, dt: f64, mood: Mood) {
        self.clock += dt;
        // Blinking, every few seconds.
        self.blink_in -= dt;
        if self.blink_in <= 0.0 {
            self.blink_t = Some(0.0);
            self.blink_in = 1.8 + self.random() * 3.7;
        }
        if let Some(t) = self.blink_t.as_mut() {
            *t += dt;
            if *t > 0.16 {
                self.blink_t = None;
            }
        }
        // The mouth: as loud as the voice, or like speech when that isn't known.
        let open = if !mood.speaking {
            0.0
        } else if let Some(level) = mood.level {
            ((level - 0.15) * 1.4).clamp(0.0, 1.05)
        } else {
            self.flap.1 -= dt;
            if self.flap.1 <= 0.0 {
                let r = self.random();
                self.flap = if self.flap.0 > 0.0 && r < 0.3 {
                    (0.0, 0.05 + self.random() * 0.07)
                } else {
                    (
                        0.35 + self.random() as f32 * 0.65,
                        0.09 + self.random() * 0.11,
                    )
                };
            }
            self.flap.0
        };
        let rate = if open > self.mouth { 40.0 } else { 16.0 };
        self.mouth += (open - self.mouth) * (1.0 - (-rate * dt).exp()) as f32;
        // Thinking: the head this way and that.
        let tilt = if mood.thinking {
            if (self.clock * std::f64::consts::TAU / 2.4).sin() > 0.0 {
                0.18
            } else {
                -0.18
            }
        } else if mood.speaking {
            0.04 * (self.clock * 2.1).sin() as f32
        } else {
            0.05 * (self.clock * 0.7).sin() as f32
        };
        self.tilt += (tilt - self.tilt) * (1.0 - (-6.0 * dt).exp()) as f32;
    }

    fn blink(&self) -> f32 {
        match self.blink_t {
            Some(t) if t < 0.07 => (t / 0.07) as f32,
            Some(t) => (1.0 - (t - 0.07) / 0.09).clamp(0.0, 1.0) as f32,
            None => 0.0,
        }
    }

    fn draw(&mut self, height: u32) -> RgbaImage {
        let [w, h] = self.meta.size;
        let k = height as f32 / h;
        // A little room each side for the tail and the hat as they move.
        let margin = 12.0;
        let width = ((w + 2.0 * margin) * k).ceil() as u32;
        let mut out = RgbaImage::new(width, height);
        // The sheet, scaled near the size it's drawn at (sharp, no shimmer).
        let factor = (k / 0.7 * 1.3).min(1.0);
        if self.sized.as_ref().is_none_or(|(hh, _, _)| *hh != height) {
            let (sw, sh) = self.sheet.dimensions();
            let small = imageops::resize(
                &self.sheet,
                ((sw as f32 * factor).round() as u32).max(1),
                ((sh as f32 * factor).round() as u32).max(1),
                imageops::FilterType::Triangle,
            );
            self.sized = Some((height, factor, small));
        }
        let Some((_, factor, sheet)) = self.sized.as_ref() else {
            return out;
        };
        let base = Affine::scale(k, k).then(Affine::translate(margin, 0.0));
        let ground = self.meta.ground;
        let mid = w / 2.0;
        let t = self.clock as f32;
        let tau = std::f32::consts::TAU;
        let pivot = |name: &str| self.meta.pivots.get(name).copied().unwrap_or([mid, ground]);
        // Breathing, from the ground.
        let breath = 1.0 + (t * tau * 0.45).sin() * 0.008;
        let body = base
            .then(Affine::translate(mid, ground))
            .then(Affine::scale(1.0, breath))
            .then(Affine::translate(-mid, -ground));
        let wag = (if self.mouth > 0.05 { 0.22 } else { 0.15 }) * (t * tau * 2.5).sin();
        let [tx, ty] = pivot("tail");
        let draws: Vec<(&str, Affine)> = {
            let head_at = pivot("head");
            let head = base
                .then(Affine::translate(
                    0.0,
                    -(breath - 1.0) * 180.0 - self.mouth * 3.0,
                ))
                .round(head_at[0], head_at[1], self.tilt);
            let mut list = vec![
                ("tail", base.round(tx, ty, wag)),
                ("body", body),
                ("pawL", base),
                ("pawR", base),
                ("head", head),
            ];
            let blink = self.blink();
            for eye in ["eyeL", "eyeR"] {
                if let Some(p) = self.meta.parts.get(eye) {
                    let cy = p.at[1] + p.at[3] / 2.0;
                    let sy = (1.0 - blink * 0.92).max(0.1);
                    list.push((
                        eye,
                        head.then(Affine::translate(0.0, cy))
                            .then(Affine::scale(1.0, sy))
                            .then(Affine::translate(0.0, -cy)),
                    ));
                }
            }
            if self.mouth < 0.07 {
                list.push(("smile", head));
            } else if let Some(p) = self.meta.parts.get("mouth") {
                let top = p.at[1];
                let sy = (0.2 + 0.8 * self.mouth).clamp(0.2, 1.25);
                list.push((
                    "mouth",
                    head.then(Affine::translate(0.0, top))
                        .then(Affine::scale(1.0, sy))
                        .then(Affine::translate(0.0, -top)),
                ));
            }
            list
        };
        for (name, place) in draws {
            if let Some(part) = self.meta.parts.get(name) {
                draw_part(&mut out, sheet, *factor, part, place);
            }
        }
        out
    }
}

/// Part `part` of the (premultiplied) sheet, scaled by `factor`, drawn over
/// `out` where `place` puts it.
fn draw_part(out: &mut RgbaImage, sheet: &RgbaImage, factor: f32, part: &Part, place: Affine) {
    let Some(back) = place.inverse() else {
        return;
    };
    let [x, y, w, h] = part.at;
    // Where it lands.
    let corners =
        [(x, y), (x + w, y), (x, y + h), (x + w, y + h)].map(|(cx, cy)| place.apply(cx, cy));
    let (ow, oh) = (out.width() as i32, out.height() as i32);
    let x0 = corners
        .iter()
        .map(|c| c.0)
        .fold(f32::MAX, f32::min)
        .floor()
        .max(0.0) as i32;
    let x1 = corners
        .iter()
        .map(|c| c.0)
        .fold(f32::MIN, f32::max)
        .ceil()
        .min(ow as f32) as i32;
    let y0 = corners
        .iter()
        .map(|c| c.1)
        .fold(f32::MAX, f32::min)
        .floor()
        .max(0.0) as i32;
    let y1 = corners
        .iter()
        .map(|c| c.1)
        .fold(f32::MIN, f32::max)
        .ceil()
        .min(oh as f32) as i32;
    // The part on the scaled sheet.
    let [sx, sy, sw, sh] = part.sheet;
    let (sx, sy) = (sx as f32 * factor, sy as f32 * factor);
    let (sw, sh) = (sw as f32 * factor, sh as f32 * factor);
    let (kx, ky) = (sw / w, sh / h);
    let (lw, lh) = (sheet.width() as i32, sheet.height() as i32);
    for oy in y0..y1 {
        for ox in x0..x1 {
            let (ax, ay) = back.apply(ox as f32 + 0.5, oy as f32 + 0.5);
            let (u, v) = ((ax - x) * kx, (ay - y) * ky);
            if u < -0.5 || v < -0.5 || u > sw + 0.5 || v > sh + 0.5 {
                continue;
            }
            // Bilinear, inside the part only.
            let (fu, fv) = (sx + u - 0.5, sy + v - 0.5);
            let (iu, iv) = (fu.floor(), fv.floor());
            let (du, dv) = (fu - iu, fv - iv);
            let mut acc = [0f32; 4];
            for (dx, dy, wgt) in [
                (0, 0, (1.0 - du) * (1.0 - dv)),
                (1, 0, du * (1.0 - dv)),
                (0, 1, (1.0 - du) * dv),
                (1, 1, du * dv),
            ] {
                let (px, py) = (iu as i32 + dx, iv as i32 + dy);
                if px < sx.floor() as i32
                    || py < sy.floor() as i32
                    || px >= (sx + sw).ceil() as i32
                    || py >= (sy + sh).ceil() as i32
                    || px >= lw
                    || py >= lh
                    || px < 0
                    || py < 0
                {
                    continue;
                }
                let p = sheet.get_pixel(px as u32, py as u32).0;
                for (sum, value) in acc.iter_mut().zip(p) {
                    *sum += value as f32 * wgt;
                }
            }
            let alpha = acc[3] / 255.0;
            if alpha <= 0.0 {
                continue;
            }
            let dst = out.get_pixel_mut(ox as u32, oy as u32);
            for (value, sum) in dst.0.iter_mut().zip(acc) {
                *value = (sum + *value as f32 * (1.0 - alpha))
                    .round()
                    .clamp(0.0, 255.0) as u8;
            }
        }
    }
}

/// How far the dog's mouth opens with a voice: how loud it is every
/// `MOUTH_STEP_MS`, from 0 (quiet) to 255 (loud), on a scale that follows the
/// ear (-50 dB to -10 dB of full scale).
pub const MOUTH_STEP_MS: u32 = 40;

pub fn mouth_levels(samples: &[i16], rate: u32) -> Vec<u8> {
    let step = (rate as usize * MOUTH_STEP_MS as usize / 1000).max(1);
    samples
        .chunks(step)
        .map(|chunk| {
            let power = chunk.iter().map(|&s| (s as f64).powi(2)).sum::<f64>() / chunk.len() as f64;
            let db = 20.0 * (power.sqrt() / 32768.0 + 1e-9).log10();
            (((db + 50.0) / 40.0).clamp(0.0, 1.0) * 255.0).round() as u8
        })
        .collect()
}

/// The same from a WAV made by `ai::wav_bytes` (16-bit mono); nothing from
/// anything else.
pub fn mouth_of_wav(wav: &[u8]) -> Vec<u8> {
    if wav.len() < 44 || &wav[0..4] != b"RIFF" || &wav[8..12] != b"WAVE" || &wav[36..40] != b"data"
    {
        return Vec::new();
    }
    let rate = u32::from_le_bytes([wav[24], wav[25], wav[26], wav[27]]);
    let samples: Vec<i16> = wav[44..]
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| i16::from_le_bytes(*b))
        .collect();
    mouth_levels(&samples, rate)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_parts_are_inside_and_put_together_make_the_dog() {
        let mut dog = Dog::load().expect("the parts decode");
        let frame = dog.frame(200, Mood::default());
        assert_eq!(frame.height(), 200);
        assert!(
            frame.width() > 120 && frame.width() < 150,
            "{}",
            frame.width()
        );
        // Transparent corners, fur in the middle of the face, the cape lower
        // down; premultiplied throughout.
        assert_eq!(frame.get_pixel(0, 0).0[3], 0);
        let x = frame.width() / 2;
        assert!(frame.get_pixel(x, 90).0[3] > 240);
        assert!(frame.get_pixel(x, 170).0[3] > 240);
        assert!(
            frame
                .pixels()
                .all(|p| p.0[0] <= p.0[3] && p.0[1] <= p.0[3] && p.0[2] <= p.0[3])
        );
    }

    #[test]
    fn its_mouth_opens_with_the_voice() {
        let mut dog = Dog::load().unwrap();
        // How dark the mouth is (the inside of an open mouth is dark).
        let dark = |f: &RgbaImage| {
            let (w, h) = f.dimensions();
            let mut n = 0;
            for y in h * 52 / 100..h * 64 / 100 {
                for x in w * 35 / 100..w * 65 / 100 {
                    let p = f.get_pixel(x, y).0;
                    if p[3] > 200 && (p[0] as u32 + p[1] as u32 + p[2] as u32) < 300 {
                        n += 1;
                    }
                }
            }
            n
        };
        let shut = dark(&dog.frame(300, Mood::default()));
        let loud = Mood {
            speaking: true,
            level: Some(0.9),
            thinking: false,
        };
        let mut open = dog.frame(300, loud);
        for _ in 0..10 {
            std::thread::sleep(std::time::Duration::from_millis(15));
            open = dog.frame(300, loud);
        }
        assert!(dark(&open) > shut + 50, "{} {}", dark(&open), shut);
        // Quiet again: it closes.
        let mut closed = open.clone();
        for _ in 0..20 {
            std::thread::sleep(std::time::Duration::from_millis(15));
            closed = dog.frame(300, Mood::default());
        }
        assert!(
            dark(&closed) < dark(&open) / 2,
            "{} {}",
            dark(&closed),
            dark(&open)
        );
    }

    #[test]
    fn the_mouth_follows_how_loud_the_voice_is() {
        // A tenth of a second of silence, then loud, then soft, at 24 kHz.
        let mut samples = vec![0i16; 2_400];
        samples.extend((0..2_400).map(|i| if i % 2 == 0 { 8_000 } else { -8_000 }));
        samples.extend((0..2_400).map(|i| if i % 2 == 0 { 300 } else { -300 }));
        let levels = mouth_levels(&samples, 24_000);
        // 40 ms steps: 2.5 of each part.
        assert_eq!(levels.len(), 8);
        assert_eq!(levels[0], 0);
        assert!(levels[3] > 200, "{levels:?}");
        assert!(levels[6] > 20 && levels[6] < 120, "{levels:?}");
        // From a WAV, the same; from anything else, nothing.
        assert_eq!(
            mouth_of_wav(&crate::ai::wav_bytes(&samples, 24_000)),
            levels
        );
        assert!(mouth_of_wav(b"RIFF....WAVE").is_empty());
    }

    #[test]
    fn turning_round_a_point_keeps_the_point() {
        let t = Affine::scale(2.0, 2.0).round(10.0, 20.0, 0.7);
        let (x, y) = t.apply(10.0, 20.0);
        assert!((x - 20.0).abs() < 1e-4 && (y - 40.0).abs() < 1e-4);
        let back = t.inverse().unwrap();
        let (x, y) = back.apply(t.apply(3.0, 4.0).0, t.apply(3.0, 4.0).1);
        assert!((x - 3.0).abs() < 1e-3 && (y - 4.0).abs() < 1e-3);
    }
}
