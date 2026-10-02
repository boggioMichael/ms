//! Finding a learned picture in a frame: normalised cross-correlation on
//! grey levels, first on a small copy of the frame to find candidates, then
//! at a finer scale around each, then a colour check. A monster faces left
//! or right, so the mirrored picture is tried too.
//!
//! Normalised correlation does not care about brightness or contrast (a
//! monster in a dark map still matches), only about the shape of the light
//! and dark in it.

use image::RgbaImage;

/// A grey picture (0 to 1), with running sums for window statistics.
#[derive(Debug, Clone)]
pub struct Gray {
    pub w: usize,
    pub h: usize,
    pub px: Vec<f32>,
    /// Integral images of the values and their squares, (w+1)×(h+1).
    sum: Vec<f64>,
    sq: Vec<f64>,
}

impl Gray {
    /// The part (`x`, `y`, `w`, `h` in pixels) of `image`, averaged over
    /// `k`×`k` blocks.
    pub fn from_region(image: &RgbaImage, x: u32, y: u32, w: u32, h: u32, k: u32) -> Gray {
        let k = k.max(1);
        let (x1, y1) = ((x + w).min(image.width()), (y + h).min(image.height()));
        let gw = ((x1.saturating_sub(x)) / k) as usize;
        let gh = ((y1.saturating_sub(y)) / k) as usize;
        let mut px = vec![0f32; gw * gh];
        let raw = image.as_raw();
        let stride = image.width() as usize * 4;
        let area = (k * k) as f32 * 255.0;
        for gy in 0..gh {
            for gx in 0..gw {
                let mut acc = 0u32;
                for dy in 0..k as usize {
                    let row = (y as usize + gy * k as usize + dy) * stride;
                    for dx in 0..k as usize {
                        let i = row + (x as usize + gx * k as usize + dx) * 4;
                        // Luma, integer weights (sum 256).
                        acc +=
                            (raw[i] as u32 * 54 + raw[i + 1] as u32 * 183 + raw[i + 2] as u32 * 19)
                                >> 8;
                    }
                }
                px[gy * gw + gx] = acc as f32 / area;
            }
        }
        Gray::from_values(gw, gh, px)
    }

    pub fn from_image(image: &RgbaImage, k: u32) -> Gray {
        Gray::from_region(image, 0, 0, image.width(), image.height(), k)
    }

    fn from_values(w: usize, h: usize, px: Vec<f32>) -> Gray {
        let mut sum = vec![0f64; (w + 1) * (h + 1)];
        let mut sq = vec![0f64; (w + 1) * (h + 1)];
        for y in 0..h {
            let (mut row, mut row2) = (0f64, 0f64);
            for x in 0..w {
                let v = px[y * w + x] as f64;
                row += v;
                row2 += v * v;
                sum[(y + 1) * (w + 1) + x + 1] = sum[y * (w + 1) + x + 1] + row;
                sq[(y + 1) * (w + 1) + x + 1] = sq[y * (w + 1) + x + 1] + row2;
            }
        }
        Gray { w, h, px, sum, sq }
    }

    /// Left to right.
    pub fn mirrored(&self) -> Gray {
        let mut px = vec![0f32; self.px.len()];
        for y in 0..self.h {
            for x in 0..self.w {
                px[y * self.w + x] = self.px[y * self.w + (self.w - 1 - x)];
            }
        }
        Gray::from_values(self.w, self.h, px)
    }

    fn window(&self, x: usize, y: usize, w: usize, h: usize) -> (f64, f64) {
        let s = |t: &Vec<f64>| {
            t[(y + h) * (self.w + 1) + x + w]
                - t[y * (self.w + 1) + x + w]
                - t[(y + h) * (self.w + 1) + x]
                + t[y * (self.w + 1) + x]
        };
        (s(&self.sum), s(&self.sq))
    }
}

/// A picture to look for, ready for correlation: its values less their
/// mean, and their norm.
#[derive(Debug, Clone)]
pub struct Pattern {
    pub w: usize,
    pub h: usize,
    zero_mean: Vec<f32>,
    norm: f64,
}

impl Pattern {
    /// `None` for a flat picture (nothing to correlate with).
    pub fn new(gray: &Gray) -> Option<Pattern> {
        let n = gray.px.len();
        if n < 4 {
            return None;
        }
        let mean = gray.px.iter().map(|&v| v as f64).sum::<f64>() / n as f64;
        let zero_mean: Vec<f32> = gray.px.iter().map(|&v| (v as f64 - mean) as f32).collect();
        let norm = zero_mean
            .iter()
            .map(|&v| (v as f64) * (v as f64))
            .sum::<f64>()
            .sqrt();
        (norm > 1e-3 * (n as f64).sqrt()).then_some(Pattern {
            w: gray.w,
            h: gray.h,
            zero_mean,
            norm,
        })
    }
}

/// One place a pattern matched: its top left in the scene, and how well
/// (the correlation, -1 to 1).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hit {
    pub x: usize,
    pub y: usize,
    pub score: f32,
}

/// Every place in `scene` where `pattern` correlates at least `min`: the
/// best of each neighbourhood, best first, at most `limit`.
pub fn correlate(scene: &Gray, pattern: &Pattern, min: f32, limit: usize) -> Vec<Hit> {
    let (pw, ph) = (pattern.w, pattern.h);
    if scene.w < pw || scene.h < ph || limit == 0 {
        return Vec::new();
    }
    let (cols, rows) = (scene.w - pw + 1, scene.h - ph + 1);
    let n = (pw * ph) as f64;
    let mut scores = vec![f32::MIN; cols * rows];
    for y in 0..rows {
        for x in 0..cols {
            let (s, s2) = scene.window(x, y, pw, ph);
            let var = s2 - s * s / n;
            if var < 1e-4 * n {
                continue;
            }
            let mut cross = 0f32;
            for j in 0..ph {
                let row = &scene.px[(y + j) * scene.w + x..(y + j) * scene.w + x + pw];
                let pat = &pattern.zero_mean[j * pw..(j + 1) * pw];
                for (a, b) in row.iter().zip(pat) {
                    cross += a * b;
                }
            }
            scores[y * cols + x] = (cross as f64 / (pattern.norm * var.sqrt())) as f32;
        }
    }
    // Local maxima above `min`, then the best that do not overlap.
    let mut peaks: Vec<Hit> = Vec::new();
    for y in 0..rows {
        for x in 0..cols {
            let v = scores[y * cols + x];
            if v < min {
                continue;
            }
            let mut best = true;
            'n: for dy in -1i64..=1 {
                for dx in -1i64..=1 {
                    let (nx, ny) = (x as i64 + dx, y as i64 + dy);
                    if (dx, dy) == (0, 0)
                        || nx < 0
                        || ny < 0
                        || nx >= cols as i64
                        || ny >= rows as i64
                    {
                        continue;
                    }
                    if scores[ny as usize * cols + nx as usize] > v {
                        best = false;
                        break 'n;
                    }
                }
            }
            if best {
                peaks.push(Hit { x, y, score: v });
            }
        }
    }
    peaks.sort_by(|a, b| b.score.total_cmp(&a.score));
    let mut kept: Vec<Hit> = Vec::new();
    for p in peaks {
        let overlaps = kept.iter().any(|k| {
            (k.x as i64 - p.x as i64).unsigned_abs() < (pw as u64).div_ceil(2)
                && (k.y as i64 - p.y as i64).unsigned_abs() < (ph as u64).div_ceil(2)
        });
        if !overlaps {
            kept.push(p);
            if kept.len() >= limit {
                break;
            }
        }
    }
    kept
}

/// Where a template was found in a frame, in the frame's pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Found {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
    pub score: f32,
    pub mirrored: bool,
}

/// The average colour of a picture's opaque pixels.
pub fn mean_rgb(image: &RgbaImage) -> [f32; 3] {
    let mut acc = [0f64; 3];
    let mut n = 0f64;
    for p in image.pixels() {
        if p.0[3] < 128 {
            continue;
        }
        for (sum, &v) in acc.iter_mut().zip(&p.0[..3]) {
            *sum += v as f64;
        }
        n += 1.0;
    }
    if n == 0.0 {
        return [0.0; 3];
    }
    acc.map(|v| (v / n) as f32)
}

/// Look for `template` in `frame` (within `region`, x/y/w/h in pixels, if
/// given): coarse candidates, refined at a finer scale, checked for colour.
/// `min` is the correlation needed (0.7 is a good default).
pub fn locate(
    frame: &RgbaImage,
    template: &RgbaImage,
    region: Option<(u32, u32, u32, u32)>,
    min: f32,
    limit: usize,
    mirror: bool,
) -> Vec<Found> {
    let (tw, th) = template.dimensions();
    let (rx, ry, rw, rh) = region.unwrap_or((0, 0, frame.width(), frame.height()));
    let (rw, rh) = (
        rw.min(frame.width().saturating_sub(rx)),
        rh.min(frame.height().saturating_sub(ry)),
    );
    if tw < 4 || th < 4 || rw < tw || rh < th {
        return Vec::new();
    }
    // Coarse: the template about 10 pixels on its short side.
    let coarse = (tw.min(th) / 10).clamp(1, 8);
    let fine = (coarse / 3).max(1);
    let scene = Gray::from_region(frame, rx, ry, rw, rh, coarse);
    let target_rgb = mean_rgb(template);
    let tpl = Gray::from_image(template, coarse);
    let mut variants = vec![(Pattern::new(&tpl), false)];
    if mirror {
        variants.push((Pattern::new(&tpl.mirrored()), true));
    }
    let fine_tpl = Gray::from_image(template, fine);
    let mut found: Vec<Found> = Vec::new();
    for (pattern, mirrored) in variants {
        let Some(pattern) = pattern else { continue };
        let fine_pattern = if mirrored {
            Pattern::new(&fine_tpl.mirrored())
        } else {
            Pattern::new(&fine_tpl)
        };
        let Some(fine_pattern) = fine_pattern else {
            continue;
        };
        let loose = (min - 0.2).max(0.3);
        for hit in correlate(&scene, &pattern, loose, limit * 4 + 4) {
            // Around the coarse hit, at the fine scale.
            let cx = rx + hit.x as u32 * coarse;
            let cy = ry + hit.y as u32 * coarse;
            let pad = coarse * 2;
            let (sx, sy) = (
                cx.saturating_sub(pad).max(rx),
                cy.saturating_sub(pad).max(ry),
            );
            let sw = (tw + 2 * pad).min(rx + rw - sx);
            let sh = (th + 2 * pad).min(ry + rh - sy);
            let local = Gray::from_region(frame, sx, sy, sw, sh, fine);
            let Some(best) = correlate(&local, &fine_pattern, min, 1).into_iter().next() else {
                continue;
            };
            let (x, y) = (sx + best.x as u32 * fine, sy + best.y as u32 * fine);
            let window = image::imageops::crop_imm(
                frame,
                x,
                y,
                tw.min(frame.width() - x),
                th.min(frame.height() - y),
            )
            .to_image();
            let rgb = mean_rgb(&window);
            let off = (0..3)
                .map(|c| (rgb[c] - target_rgb[c]).abs())
                .fold(0f32, f32::max);
            if off > 60.0 {
                continue;
            }
            found.push(Found {
                x,
                y,
                w: tw,
                h: th,
                score: best.score,
                mirrored,
            });
        }
    }
    found.sort_by(|a, b| b.score.total_cmp(&a.score));
    let mut kept: Vec<Found> = Vec::new();
    for f in found {
        let overlaps = kept.iter().any(|k| {
            (k.x as i64 - f.x as i64).unsigned_abs() < (tw as u64).div_ceil(2)
                && (k.y as i64 - f.y as i64).unsigned_abs() < (th as u64).div_ceil(2)
        });
        if !overlaps {
            kept.push(f);
            if kept.len() >= limit {
                break;
            }
        }
    }
    kept
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    /// A made-up "monster": a face with eyes, in its own colours.
    fn monster(flip: bool) -> RgbaImage {
        let mut m = RgbaImage::from_pixel(40, 36, Rgba([240, 140, 40, 255]));
        for y in 0..36 {
            for x in 0..40 {
                let (fx, fy) = (x as f32 - 20.0, y as f32 - 18.0);
                if fx * fx / 300.0 + fy * fy / 250.0 > 1.0 {
                    m.put_pixel(x, y, Rgba([90, 160, 90, 255]));
                }
            }
        }
        let eye = if flip { [26, 30] } else { [10, 14] };
        for y in 10..16 {
            for x in eye[0]..eye[1] {
                m.put_pixel(x, y, Rgba([20, 20, 20, 255]));
            }
        }
        for y in 24..27 {
            for x in 8..32 {
                m.put_pixel(x, y, Rgba([120, 40, 20, 255]));
            }
        }
        m
    }

    fn scene() -> RgbaImage {
        // A green field with a few stripes, and the monster twice: as
        // taught, and facing the other way.
        let mut s = RgbaImage::from_fn(640, 360, |x, y| {
            let v = ((x / 23 + y / 31) % 3) as u8 * 12;
            Rgba([80 + v, 150 + v, 80 + v, 255])
        });
        image::imageops::replace(&mut s, &monster(false), 100, 200);
        image::imageops::replace(
            &mut s,
            &image::imageops::flip_horizontal(&monster(false)),
            420,
            90,
        );
        s
    }

    #[test]
    fn a_taught_picture_is_found_both_ways_round() {
        let found = locate(&scene(), &monster(false), None, 0.75, 5, true);
        assert_eq!(found.len(), 2, "{found:?}");
        let mut places: Vec<(u32, u32, bool)> =
            found.iter().map(|f| (f.x, f.y, f.mirrored)).collect();
        places.sort();
        assert!(places[0].0.abs_diff(100) <= 2 && places[0].1.abs_diff(200) <= 2 && !places[0].2);
        assert!(places[1].0.abs_diff(420) <= 2 && places[1].1.abs_diff(90) <= 2 && places[1].2);
        assert!(found.iter().all(|f| f.score > 0.9));
        // Not mirrored: only the one as taught.
        assert_eq!(
            locate(&scene(), &monster(false), None, 0.75, 5, false).len(),
            1
        );
        // Within a region that holds neither.
        assert!(
            locate(
                &scene(),
                &monster(false),
                Some((200, 0, 200, 360)),
                0.75,
                5,
                true
            )
            .is_empty()
        );
    }

    #[test]
    fn something_else_is_not_found() {
        // A blue square with a hole is not the monster.
        let mut other = RgbaImage::from_pixel(40, 36, Rgba([40, 60, 220, 255]));
        for y in 12..24 {
            for x in 14..26 {
                other.put_pixel(x, y, Rgba([250, 250, 250, 255]));
            }
        }
        assert!(locate(&scene(), &other, None, 0.75, 5, true).is_empty());
    }

    #[test]
    fn correlation_is_one_for_the_same_picture_and_ignores_brightness() {
        let m = monster(false);
        let darker = RgbaImage::from_fn(40, 36, |x, y| {
            let p = m.get_pixel(x, y).0;
            Rgba([p[0] / 2, p[1] / 2, p[2] / 2, 255])
        });
        let pattern = Pattern::new(&Gray::from_image(&m, 1)).unwrap();
        let hit = correlate(&Gray::from_image(&darker, 1), &pattern, 0.5, 1)[0];
        assert_eq!((hit.x, hit.y), (0, 0));
        assert!(hit.score > 0.99, "{}", hit.score);
        // A flat picture has nothing to look for.
        assert!(Pattern::new(&Gray::from_image(&RgbaImage::new(8, 8), 1)).is_none());
    }
}
