//! Telling one scene from the next, cheaply: a coarse fingerprint of the
//! frame, and what a run of them says.
//!
//! A fingerprint is the brightness of a 32×18 grid of cells, each the mean
//! of a few samples — the same few thousand pixels whatever the frame's
//! size, so a 4K frame costs no more than a small one, and the whole thing
//! is well under a tenth of a millisecond. Two fingerprints a frame apart
//! differ a little while the camera scrolls, and a lot when the game cuts
//! to another map (a loading screen, then a different place). [`Scenes`]
//! watches a run of them for such cuts, and for how much is going on.
//! A cut that lands back on a picture seen a little while ago (a dialog
//! closing, a death screen giving way to the map) is a return, not a new
//! scene: the pictures the game settled on lately are remembered.

use image::RgbaImage;

/// The grid: 32 across, 18 down (the game's 16:9).
pub const COLUMNS: usize = 32;
pub const ROWS: usize = 18;
/// Samples across and down each cell.
const SAMPLES: u32 = 4;

/// The brightness of each cell, row by row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fingerprint {
    pub cells: Vec<u8>,
}

impl Fingerprint {
    /// Of `frame`: each cell the mean brightness of a 4×4 grid of samples
    /// in it.
    pub fn of(frame: &RgbaImage) -> Fingerprint {
        let (w, h) = (frame.width(), frame.height());
        let mut cells = Vec::with_capacity(COLUMNS * ROWS);
        if w == 0 || h == 0 {
            cells.resize(COLUMNS * ROWS, 0);
            return Fingerprint { cells };
        }
        for row in 0..ROWS as u32 {
            for column in 0..COLUMNS as u32 {
                let mut sum = 0u32;
                for sy in 0..SAMPLES {
                    // The sample sits at the centre of its part of the cell.
                    let y = ((row * SAMPLES + sy) * 2 + 1) * h / (2 * ROWS as u32 * SAMPLES);
                    for sx in 0..SAMPLES {
                        let x =
                            ((column * SAMPLES + sx) * 2 + 1) * w / (2 * COLUMNS as u32 * SAMPLES);
                        let p = frame.get_pixel(x.min(w - 1), y.min(h - 1)).0;
                        sum += luma(p);
                    }
                }
                cells.push((sum / (SAMPLES * SAMPLES)) as u8);
            }
        }
        Fingerprint { cells }
    }

    /// How different two fingerprints are: the mean absolute difference of
    /// their cells, 0 (the same) to 1 (black against white).
    pub fn difference(&self, other: &Fingerprint) -> f32 {
        let n = self.cells.len().min(other.cells.len());
        if n == 0 {
            return 0.0;
        }
        let sum: u32 = self.cells[..n]
            .iter()
            .zip(&other.cells[..n])
            .map(|(a, b)| a.abs_diff(*b) as u32)
            .sum();
        sum as f32 / (n as f32 * 255.0)
    }

    /// The mean brightness, 0 to 255.
    pub fn brightness(&self) -> u8 {
        if self.cells.is_empty() {
            return 0;
        }
        (self.cells.iter().map(|c| *c as u32).sum::<u32>() / self.cells.len() as u32) as u8
    }

    /// One flat colour, near enough: a loading screen, a fade to black.
    /// (A game scene has its HUD, text and sky: never this even.)
    pub fn flat(&self) -> bool {
        let (min, max) = self
            .cells
            .iter()
            .fold((255u8, 0u8), |(lo, hi), c| (lo.min(*c), hi.max(*c)));
        max - min < FLAT_SPREAD
    }
}

/// Cells within this much of each other make a flat picture.
const FLAT_SPREAD: u8 = 24;

/// Rec. 601 brightness of a pixel, 0 to 255.
fn luma(p: [u8; 4]) -> u32 {
    (p[0] as u32 * 77 + p[1] as u32 * 150 + p[2] as u32 * 29) >> 8
}

/// A frame-to-frame difference this big is a cut, not a scroll: the camera
/// following the character changes a tenth of the cells' worth per frame
/// at most; a portal, a loading screen or a different map changes most of
/// the picture at once.
pub const CUT: f32 = 0.22;
/// After a cut, the picture must stay this far from the one before it to be
/// a new scene rather than a flash (a skill effect, a lightning strike).
const STAYS_AWAY: f32 = 0.15;
/// …for this long, in seconds.
const SETTLE_SECS: f64 = 1.5;
/// The pictures the game settled on are remembered this long, in seconds.
/// A cut cannot tell a portal from a dialog box, a death screen or a
/// full-screen effect; but each of those gives the old picture back when
/// it ends, and a picture seen within the last couple of minutes is a
/// return, not a new scene. (Long enough for a dialog read at leisure and
/// a revive; short enough that coming back to a map later still counts.)
pub const SEEN_FOR: f64 = 120.0;
/// Frame-to-frame differences are averaged over this long for `activity`,
/// in seconds.
const ACTIVITY_SECS: f64 = 5.0;

/// What a run of fingerprints says about the scene.
#[derive(Debug, Clone, PartialEq)]
pub struct Verdict {
    /// How different this frame is from the one before, 0 to 1.
    pub change: f32,
    /// The mean frame-to-frame change over the last few seconds: how much
    /// is going on (0 when nothing moves).
    pub activity: f32,
    /// The scene cut to a new one a moment ago and has settled there: a new
    /// map, or something covering most of the screen — and not a picture
    /// seen lately (a dialog closed, a death screen gave way to the map).
    pub new_scene: bool,
}

/// Watches fingerprints as they come, one per frame.
#[derive(Debug, Default)]
pub struct Scenes {
    last: Option<Fingerprint>,
    /// A cut is being watched: when the last cut was, and the picture
    /// before the first of the run (a loading screen cuts twice: to black,
    /// then to the new map).
    cut: Option<(f64, Fingerprint)>,
    /// (when, change) lately.
    changes: std::collections::VecDeque<(f64, f32)>,
    /// The pictures the game settled on lately (what it cut away from, and
    /// what it settled on), and when each was last seen; oldest first.
    seen: std::collections::VecDeque<(f64, Fingerprint)>,
}

impl Scenes {
    /// Was a picture like this one settled on within [`SEEN_FOR`]?
    fn seen_lately(&self, now: f64, fingerprint: &Fingerprint) -> bool {
        self.seen
            .iter()
            .any(|(at, seen)| now - at <= SEEN_FOR && seen.difference(fingerprint) < STAYS_AWAY)
    }

    /// The game settled on `fingerprint` at `now`: remembered for a while,
    /// standing in for any picture like it seen before (the newest look of
    /// a place is the one to compare with).
    fn remember(&mut self, now: f64, fingerprint: Fingerprint) {
        self.seen.retain(|(at, seen)| {
            now - at <= SEEN_FOR && seen.difference(&fingerprint) >= STAYS_AWAY
        });
        self.seen.push_back((now, fingerprint));
    }

    /// The next frame's fingerprint, at `now` seconds.
    pub fn observe(&mut self, now: f64, fingerprint: Fingerprint) -> Verdict {
        let change = self
            .last
            .as_ref()
            .map(|last| last.difference(&fingerprint))
            .unwrap_or(0.0);
        self.changes.push_back((now, change));
        while self
            .changes
            .front()
            .is_some_and(|(t, _)| now - t > ACTIVITY_SECS)
        {
            self.changes.pop_front();
        }
        let activity = if self.changes.is_empty() {
            0.0
        } else {
            self.changes.iter().map(|(_, c)| c).sum::<f32>() / self.changes.len() as f32
        };
        let mut new_scene = false;
        let mut settled = false;
        if change >= CUT {
            // A cut: the settling starts over; what it cut away from is
            // kept from the first cut of the run (and remembered: the
            // game was settled on it).
            match (self.cut.take(), self.last.take()) {
                (Some((_, before)), _) => self.cut = Some((now, before)),
                (None, Some(before)) => {
                    self.remember(now, before.clone());
                    self.cut = Some((now, before));
                }
                (None, None) => {}
            }
        } else if let Some((at, before)) = &mut self.cut {
            if fingerprint.flat() {
                // A loading screen: the settling starts when it ends (the
                // map may fade in rather than cut in).
                *at = now;
            } else if now - *at >= SETTLE_SECS {
                // Settled on a picture: a new scene if it stayed away from
                // the old one.
                new_scene = before.difference(&fingerprint) >= STAYS_AWAY;
                settled = true;
            }
        }
        if settled {
            // …and is not one seen lately: a dialog closing, a death screen
            // giving way to the map, a trip through a portal and back land
            // on a known picture.
            self.cut = None;
            if new_scene && self.seen_lately(now, &fingerprint) {
                new_scene = false;
            }
            self.remember(now, fingerprint.clone());
        }
        self.last = Some(fingerprint);
        Verdict {
            change,
            activity,
            new_scene,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    fn flat(w: u32, h: u32, v: u8) -> RgbaImage {
        RgbaImage::from_pixel(w, h, Rgba([v, v, v, 255]))
    }

    /// Dark on the left, bright on the right, split at `split` of the width.
    fn split(w: u32, h: u32, split: f32) -> RgbaImage {
        let mut image = flat(w, h, 20);
        let edge = (w as f32 * split) as u32;
        for y in 0..h {
            for x in edge..w {
                image.put_pixel(x, y, Rgba([220, 220, 220, 255]));
            }
        }
        image
    }

    #[test]
    fn a_fingerprint_is_the_same_whatever_the_frames_size() {
        let small = Fingerprint::of(&split(320, 180, 0.5));
        let big = Fingerprint::of(&split(3840, 2160, 0.5));
        assert_eq!(small.cells.len(), COLUMNS * ROWS);
        assert!(small.difference(&big) < 0.01, "{}", small.difference(&big));
        assert_eq!(small.cells[0], 20);
        assert_eq!(small.cells[COLUMNS - 1], 220);
        assert_eq!(
            Fingerprint::of(&flat(64, 36, 0)).difference(&Fingerprint::of(&flat(64, 36, 255))),
            1.0
        );
        assert_eq!(Fingerprint::of(&flat(1, 1, 7)).cells.len(), COLUMNS * ROWS);
    }

    #[test]
    fn a_scroll_is_a_small_change_and_a_cut_a_new_scene() {
        let mut scenes = Scenes::default();
        let mut t = 0.0;
        // The camera scrolls: the split moves a little every frame.
        let mut last = Verdict {
            change: 0.0,
            activity: 0.0,
            new_scene: false,
        };
        for i in 0..20 {
            let at = 0.3 + i as f32 * 0.01;
            last = scenes.observe(t, Fingerprint::of(&split(320, 180, at)));
            assert!(!last.new_scene, "{i}: {last:?}");
            t += 0.1;
        }
        assert!(last.change > 0.0 && last.change < CUT, "{last:?}");
        assert!(last.activity > 0.0 && last.activity < 0.05, "{last:?}");
        // A loading screen (black), then a different map (bright on the
        // left): a cut, settled after a moment.
        let black = Fingerprint::of(&flat(320, 180, 0));
        let v = scenes.observe(t, black.clone());
        assert!(v.change >= CUT && !v.new_scene, "{v:?}");
        t += 0.1;
        let other = Fingerprint::of(&split(320, 180, 0.9));
        let mut told = Vec::new();
        for _ in 0..25 {
            let v = scenes.observe(t, other.clone());
            if v.new_scene {
                told.push(t);
            }
            t += 0.1;
        }
        assert_eq!(told.len(), 1, "{told:?}");
        assert!(
            told[0] - 2.0 >= SETTLE_SECS - 0.01 && told[0] - 2.0 < SETTLE_SECS + 0.3,
            "{told:?}"
        );
        // Standing still: no activity.
        for _ in 0..60 {
            last = scenes.observe(t, other.clone());
            t += 0.1;
        }
        assert_eq!(last.activity, 0.0);
    }

    #[test]
    fn a_loading_screen_keeps_the_cut_open_until_the_map_comes_up() {
        let mut scenes = Scenes::default();
        let scene = Fingerprint::of(&split(320, 180, 0.5));
        let black = Fingerprint::of(&flat(320, 180, 0));
        let other = Fingerprint::of(&split(320, 180, 0.9));
        assert!(black.flat() && !scene.flat() && !other.flat());
        let mut t = 0.0;
        for _ in 0..10 {
            scenes.observe(t, scene.clone());
            t += 0.1;
        }
        // Four seconds of loading screen: nothing yet.
        for _ in 0..40 {
            let v = scenes.observe(t, black.clone());
            assert!(!v.new_scene, "{v:?}");
            t += 0.1;
        }
        // The map comes up: one new scene, once it has settled.
        let mut told = 0;
        let up = t;
        for _ in 0..30 {
            let v = scenes.observe(t, other.clone());
            if v.new_scene {
                told += 1;
                assert!(t - up >= SETTLE_SECS - 0.01, "{}", t - up);
            }
            t += 0.1;
        }
        assert_eq!(told, 1);
    }

    #[test]
    fn a_cut_back_to_a_picture_seen_lately_is_a_return_not_a_new_scene() {
        let mut scenes = Scenes::default();
        let map = Fingerprint::of(&split(320, 180, 0.5));
        let dialog = Fingerprint::of(&split(320, 180, 0.9));
        let mut t = 0.0;
        let mut play = |scenes: &mut Scenes, picture: &Fingerprint, seconds: f64| -> Vec<f64> {
            let mut told = Vec::new();
            for _ in 0..(seconds * 10.0) as usize {
                if scenes.observe(t, picture.clone()).new_scene {
                    told.push(t);
                }
                t += 0.1;
            }
            told
        };
        // On a map for a while; then a dialog opens over it and stays: a
        // cut that settles, which a fingerprint cannot tell from a portal.
        assert!(play(&mut scenes, &map, 3.0).is_empty());
        assert_eq!(play(&mut scenes, &dialog, 3.0).len(), 1);
        // Read for half a minute, then closed: the map again — a picture
        // seen lately, not a new scene.
        play(&mut scenes, &dialog, 30.0);
        assert_eq!(play(&mut scenes, &map, 3.0), Vec::<f64>::new());
        assert!(scenes.seen.len() <= 2, "{}", scenes.seen.len());
        // Two and a half minutes on the map: the dialog is forgotten, and
        // the same cut is a new scene again.
        play(&mut scenes, &map, 150.0);
        assert_eq!(play(&mut scenes, &dialog, 3.0).len(), 1);
    }

    #[test]
    fn a_flash_that_goes_away_is_not_a_new_scene() {
        let mut scenes = Scenes::default();
        let scene = Fingerprint::of(&split(320, 180, 0.5));
        let flash = Fingerprint::of(&flat(320, 180, 255));
        let mut t = 0.0;
        for _ in 0..10 {
            scenes.observe(t, scene.clone());
            t += 0.1;
        }
        let v = scenes.observe(t, flash);
        assert!(v.change >= CUT);
        t += 0.1;
        for _ in 0..30 {
            let v = scenes.observe(t, scene.clone());
            assert!(!v.new_scene, "{v:?}");
            t += 0.1;
        }
    }
}
