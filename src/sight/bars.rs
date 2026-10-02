//! A bar that fills and empties (HP, MP, EXP, a boss's HP): learned once
//! from roughly where a model said it is, then measured on every frame.
//!
//! Learning finds the bar's colour (the most common saturated hue there,
//! or the one nearest the colour expected) and the rows it fills, which
//! pins the box down to the pixel. How far the fill can go — the track's
//! right end — is first taken from the model's box, then worked out from
//! readings: when the game says HP is 80% and the fill ends at pixel 400,
//! the track is (400 − start) / 0.8 long. Every reading the teacher gives
//! makes it more exact.

use image::RgbaImage;
use serde::{Deserialize, Serialize};

use crate::ai::images::NBox;
use crate::util::pixel::hsv_from_rgb;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BarModel {
    /// The fill's rows (`y0`..`y1`), where the fill starts (`x0`) and where
    /// a full bar would end (`x1`), as fractions of the frame.
    pub band: NBox,
    /// The fill's hue (degrees) and how far from it a pixel may be.
    pub hue: f32,
    pub hue_tolerance: f32,
    /// Readings that fixed where the track ends: (where the fill ended, as
    /// a fraction of the frame width; the percent the game showed).
    #[serde(default)]
    pub readings: Vec<(f32, f32)>,
}

/// Saturation and brightness a fill pixel needs (the dark empty track and
/// white text do not have them).
const MIN_SATURATION: f32 = 0.35;
const MIN_VALUE: f32 = 0.28;

fn hue_distance(a: f32, b: f32) -> f32 {
    let d = (a - b).rem_euclid(360.0);
    d.min(360.0 - d)
}

impl BarModel {
    fn is_fill(&self, p: &image::Rgba<u8>) -> bool {
        let (h, s, v) = hsv_from_rgb(p.0[0], p.0[1], p.0[2]);
        s >= MIN_SATURATION && v >= MIN_VALUE && hue_distance(h, self.hue) <= self.hue_tolerance
    }

    /// Learn a bar from `approx` (roughly where it is). `expected_hue`: the
    /// colour it should be (red HP, blue MP), when known.
    pub fn learn(frame: &RgbaImage, approx: &NBox, expected_hue: Option<f32>) -> Option<BarModel> {
        let (fw, fh) = frame.dimensions();
        let (bx, by, bw, bh) = approx.pixels(fw, fh);
        // The hue: a histogram of the saturated pixels in the box, in 10° bins.
        let mut bins = [0u32; 36];
        for y in by..by + bh {
            for x in bx..bx + bw {
                let p = frame.get_pixel(x, y).0;
                let (h, s, v) = hsv_from_rgb(p[0], p[1], p[2]);
                if s >= MIN_SATURATION && v >= MIN_VALUE {
                    bins[(h / 10.0) as usize % 36] += 1;
                }
            }
        }
        let most = *bins.iter().max()?;
        if most < 12 {
            return None;
        }
        let bin = match expected_hue {
            Some(expected) => (0..36).filter(|&b| bins[b] * 5 >= most).min_by(|&a, &b| {
                hue_distance(a as f32 * 10.0 + 5.0, expected)
                    .total_cmp(&hue_distance(b as f32 * 10.0 + 5.0, expected))
            })?,
            None => (0..36).max_by_key(|&b| bins[b])?,
        };
        let mut model = BarModel {
            band: *approx,
            hue: bin as f32 * 10.0 + 5.0,
            hue_tolerance: 24.0,
            readings: Vec::new(),
        };
        // A finer hue: the circular mean of the pixels near the bin.
        let (mut sx, mut sy) = (0f64, 0f64);
        for y in by..by + bh {
            for x in bx..bx + bw {
                let p = frame.get_pixel(x, y);
                if model.is_fill(p) {
                    let (h, _, _) = hsv_from_rgb(p.0[0], p.0[1], p.0[2]);
                    sx += (h as f64).to_radians().cos();
                    sy += (h as f64).to_radians().sin();
                }
            }
        }
        model.hue = (sy.atan2(sx).to_degrees() as f32).rem_euclid(360.0);
        // Models are rough about a thin bar's height: look well above and
        // below. Sideways, a little past the box's ends.
        let pad_y = bh.max(8);
        let y_from = by.saturating_sub(pad_y);
        let y_to = (by + bh + pad_y).min(fh);
        let x_from = bx.saturating_sub(bw / 12);
        let x_to = (bx + bw + bw / 50 + 2).min(fw);
        let gap = (bw / 8).max(3);
        // Each row's longest run of the colour (gaps for text allowed).
        let runs: Vec<Option<(u32, u32)>> = (y_from..y_to)
            .map(|y| model.longest_run(frame, y, x_from, x_to, gap))
            .collect();
        let min_run = (bw / 60).max(3);
        // Bands: rows in a row with a run, starting at about the same place.
        let mut bands: Vec<(usize, usize)> = Vec::new();
        let mut i = 0;
        while i < runs.len() {
            match runs[i] {
                Some((a, b)) if b - a >= min_run => {
                    let mut j = i + 1;
                    while j < runs.len()
                        && runs[j].is_some_and(|(c, d)| d - c >= min_run && c.abs_diff(a) <= gap)
                    {
                        j += 1;
                    }
                    bands.push((i, j));
                    i = j;
                }
                _ => i += 1,
            }
        }
        // The bar: a band no taller than a bar can be, nearest the box's
        // middle.
        let tallest = (bh * 5 / 2).max(30) as usize;
        let middle = (by + bh / 2) as i64 - y_from as i64;
        let (top, bottom) = bands
            .into_iter()
            .filter(|(a, b)| b - a >= 2 && b - a <= tallest)
            .min_by_key(|(a, b)| ((*a + *b) as i64 / 2 - middle).abs())?;
        let mut starts: Vec<u32> = runs[top..bottom].iter().flatten().map(|r| r.0).collect();
        let mut ends: Vec<u32> = runs[top..bottom].iter().flatten().map(|r| r.1).collect();
        starts.sort_unstable();
        ends.sort_unstable();
        let x0 = starts[starts.len() / 2];
        let x_end = ends[ends.len() / 2];
        let (y0, y1) = (y_from + top as u32, y_from + bottom as u32);
        // Until a reading says otherwise, the track ends where the model's
        // box does (or where the fill does, if it goes further). A fill
        // that nearly reaches the box's end is a full bar: it ends there.
        let box_end = bx + bw;
        let x1 = if box_end.saturating_sub(x_end) * 25 <= box_end.saturating_sub(x0) {
            x_end
        } else {
            box_end.max(x_end)
        }
        .min(fw);
        model.band = NBox::from_pixels(x0, y0, x1.saturating_sub(x0).max(1), y1 - y0, fw, fh);
        Some(model)
    }

    /// The longest run of fill pixels in row `y` between `from` and `to`,
    /// bridging gaps of up to `gap` (text over the bar): start and end
    /// (exclusive).
    fn longest_run(
        &self,
        frame: &RgbaImage,
        y: u32,
        from: u32,
        to: u32,
        gap: u32,
    ) -> Option<(u32, u32)> {
        let mut best: Option<(u32, u32)> = None;
        let mut current: Option<(u32, u32)> = None;
        for x in from..to {
            if self.is_fill(frame.get_pixel(x, y)) {
                current = Some(match current {
                    Some((a, b)) if x - b <= gap => (a, x + 1),
                    _ => (x, x + 1),
                });
                if let Some((a, b)) = current
                    && best.is_none_or(|(c, d)| b - a > d - c)
                {
                    best = Some((a, b));
                }
            }
        }
        best
    }

    /// Where row `y`'s fill ends: the run that starts at the track's start
    /// (`x0`, give or take `slack`), bridging gaps up to `gap`.
    fn row_end(
        &self,
        frame: &RgbaImage,
        y: u32,
        x0: u32,
        to: u32,
        slack: u32,
        gap: u32,
    ) -> Option<u32> {
        let mut end: Option<u32> = None;
        for x in x0.saturating_sub(slack)..to {
            if self.is_fill(frame.get_pixel(x, y)) {
                match end {
                    Some(e) if x - e > gap => break,
                    _ => end = Some(x + 1),
                }
            } else if end.is_none() && x > x0 + slack {
                // Nothing at the start: no fill in this row.
                return None;
            } else if let Some(e) = end
                && x - e > gap
            {
                break;
            }
        }
        end
    }

    /// Where the fill ends now, in pixels, if the bar is to be seen: the
    /// track's start and end, and the fill's end.
    fn fill_end(&self, frame: &RgbaImage) -> Option<(u32, u32, u32)> {
        let (fw, fh) = frame.dimensions();
        let (x0, y0, w, h) = self.band.pixels(fw, fh);
        // A little past the track's end, in case it was set short.
        let to = (x0 + w + w / 50 + 2).min(fw);
        let slack = (w / 50).max(3);
        let gap = (w / 8).max(3);
        let mut ends: Vec<u32> = (y0..y0 + h)
            .filter_map(|y| self.row_end(frame, y, x0, to, slack, gap))
            .collect();
        if ends.is_empty() || ends.len() * 10 < h as usize * 4 {
            return None;
        }
        ends.sort_unstable();
        Some((x0, x0 + w, ends[ends.len() / 2]))
    }

    /// How full the bar is, 0 to 100, if it can be seen. An empty bar and
    /// one covered up look the same, so neither gives a number.
    pub fn measure(&self, frame: &RgbaImage) -> Option<f32> {
        let (x0, x1, end) = self.fill_end(frame)?;
        if x1 <= x0 {
            return None;
        }
        Some(((end.saturating_sub(x0)) as f32 / (x1 - x0) as f32 * 100.0).clamp(0.0, 100.0))
    }

    /// The game shows `percent` now: work out from it where the track ends.
    pub fn reading(&mut self, frame: &RgbaImage, percent: f32) {
        let Some((x0, _, end)) = self.fill_end(frame) else {
            return;
        };
        let fw = frame.width() as f32;
        if percent < 8.0 || end <= x0 {
            return;
        }
        self.readings.push((end as f32 / fw, percent.min(100.0)));
        if self.readings.len() > 12 {
            self.readings.remove(0);
        }
        // Least squares through the start: length = Σ(d·p) / Σ(p²).
        let start = self.band.x0;
        let (mut dp, mut pp) = (0f64, 0f64);
        for &(e, p) in &self.readings {
            let (d, p) = ((e - start) as f64, p as f64 / 100.0);
            dp += d * p;
            pp += p * p;
        }
        if pp > 0.0 {
            let length = (dp / pp) as f32;
            if length > 0.0 {
                self.band.x1 = (start + length).min(1.0);
            }
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use image::Rgba;

    /// A modern status bar: dark background, a red HP bar and a blue MP bar
    /// with white text over them and a gradient on each, and a thin
    /// yellow-green EXP bar along the bottom (here 50% full).
    pub(crate) fn status_bar(hp: f32, mp: f32) -> RgbaImage {
        status_bar_exp(hp, mp, 50.0)
    }

    pub(crate) fn status_bar_exp(hp: f32, mp: f32, exp: f32) -> RgbaImage {
        let mut f = RgbaImage::from_pixel(1280, 720, Rgba([60, 90, 140, 255]));
        // The panel.
        for y in 640..700 {
            for x in 400..880 {
                f.put_pixel(x, y, Rgba([28, 28, 34, 255]));
            }
        }
        let bar = |f: &mut RgbaImage, y0: u32, fill: f32, rgb: [u8; 3]| {
            let (x0, x1) = (520u32, 860u32);
            let end = x0 + ((x1 - x0) as f32 * fill / 100.0) as u32;
            for y in y0..y0 + 10 {
                let shade = 1.0 - (y - y0) as f32 * 0.04;
                for x in x0..x1 {
                    let p = if x < end {
                        Rgba([
                            (rgb[0] as f32 * shade) as u8,
                            (rgb[1] as f32 * shade) as u8,
                            (rgb[2] as f32 * shade) as u8,
                            255,
                        ])
                    } else {
                        Rgba([45, 45, 50, 255])
                    };
                    f.put_pixel(x, y, p);
                }
            }
            // "HP 1234 / 2000" in white over the middle.
            for y in y0 + 2..y0 + 8 {
                for x in (660..720).step_by(3) {
                    f.put_pixel(x, y, Rgba([250, 250, 250, 255]));
                }
            }
        };
        bar(&mut f, 652, hp, [230, 40, 50]);
        bar(&mut f, 670, mp, [40, 110, 235]);
        let end = (1280.0 * exp / 100.0) as u32;
        for y in 708..714 {
            for x in 0..1280 {
                let p = if x < end {
                    Rgba([200, 220, 40, 255])
                } else {
                    Rgba([30, 30, 30, 255])
                };
                f.put_pixel(x, y, p);
            }
        }
        f
    }

    #[test]
    fn a_bar_is_learned_from_a_rough_box_and_measured() {
        let frame = status_bar(60.0, 100.0);
        // The model's box: a bit off in every direction.
        let rough = NBox::new(512.0 / 1280.0, 648.0 / 720.0, 850.0 / 1280.0, 664.0 / 720.0);
        let hp = BarModel::learn(&frame, &rough, Some(0.0)).expect("learned");
        let (x, y, _, h) = hp.band.pixels(1280, 720);
        assert_eq!((x, y, h), (520, 652, 10), "{:?}", hp.band);
        assert!(hue_distance(hp.hue, 357.0) < 8.0, "{}", hp.hue);
        // The track's end comes from the rough box until a reading fixes it.
        let first = hp.measure(&frame).unwrap();
        assert!((first - 61.0).abs() < 3.0, "{first}");
        let mut hp = hp;
        hp.reading(&frame, 60.0);
        let (_, _, w, _) = hp.band.pixels(1280, 720);
        assert!(w.abs_diff(340) <= 2, "{w}");
        // Now exact on other frames.
        for fill in [25.0, 80.0, 100.0] {
            let got = hp.measure(&status_bar(fill, 100.0)).unwrap();
            assert!((got - fill).abs() < 1.5, "{fill}: {got}");
        }
        // The blue bar next to it is not mistaken for it.
        let mp = BarModel::learn(&frame, &rough.grown(0.0, 1.5), Some(220.0)).unwrap();
        assert!(hue_distance(mp.hue, 220.0) < 10.0, "{}", mp.hue);
        let (_, my, _, _) = mp.band.pixels(1280, 720);
        assert_eq!(my, 670);
    }

    #[test]
    fn a_covered_or_empty_bar_gives_no_number() {
        let frame = status_bar(60.0, 100.0);
        let rough = NBox::new(520.0 / 1280.0, 652.0 / 720.0, 860.0 / 1280.0, 662.0 / 720.0);
        let hp = BarModel::learn(&frame, &rough, Some(0.0)).unwrap();
        assert_eq!(hp.measure(&status_bar(0.0, 100.0)), None);
        let covered = RgbaImage::from_pixel(1280, 720, Rgba([200, 200, 200, 255]));
        assert_eq!(hp.measure(&covered), None);
        // Nothing to learn where there is no bar.
        assert!(BarModel::learn(&covered, &rough, None).is_none());
    }
}
