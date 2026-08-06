//! Environment / platform layout detector.
//!
//! Foothold platforms in MapleStory render as a strong horizontal
//! brightness discontinuity (the ground texture edge) that spans a wide
//! contiguous run of the frame. This detector looks for such edges via a
//! simple vertical luminance gradient, which is resolution- and
//! skin-independent (unlike matching a specific tile texture).
//!
//! This is intentionally a coarse signal: it reports *candidate* horizontal
//! line segments with a confidence based on length and gradient strength,
//! not a verified walkable-foothold graph (which would require matching
//! against the map's actual collision data, unavailable from pixels alone).

use image::RgbaImage;

use crate::vision::geometry::{group_segments, Rect};
use crate::vision::types::{Confidence, Detection, Reliability, Source};

#[derive(Debug, Clone, Copy)]
pub struct EnvironmentConfig {
    /// Minimum luminance delta between adjacent rows to count as an edge pixel.
    pub gradient_threshold: f32,
    /// Minimum contiguous run width, in pixels, for a candidate platform edge.
    pub min_run_width: u32,
}

impl Default for EnvironmentConfig {
    fn default() -> Self {
        Self {
            gradient_threshold: 40.0,
            min_run_width: 60,
        }
    }
}

/// A candidate horizontal platform/foothold edge line.
#[derive(Debug, Clone, Copy)]
pub struct PlatformEdge {
    pub bounds: Rect,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct FootholdDetector {
    config: EnvironmentConfig,
}

impl FootholdDetector {
    pub fn new(config: EnvironmentConfig) -> Self {
        Self { config }
    }

    pub fn detect(&self, image: &RgbaImage) -> Detection<Vec<PlatformEdge>> {
        let width = image.width();
        let height = image.height();
        if width < 8 || height < 8 {
            return Detection::missing(Source::Environment, "frame too small to scan for platform edges");
        }

        let luminance = |x: u32, y: u32| -> f32 {
            let pixel = image.get_pixel(x, y);
            0.2126 * pixel[0] as f32 + 0.7152 * pixel[1] as f32 + 0.0722 * pixel[2] as f32
        };

        let mut rows = Vec::new();
        for y in 0..(height - 1) {
            let mut start: Option<u32> = None;
            for x in 0..width {
                let gradient = (luminance(x, y) - luminance(x, y + 1)).abs();
                if gradient >= self.config.gradient_threshold {
                    if start.is_none() {
                        start = Some(x);
                    }
                } else if let Some(begin) = start {
                    if x - begin >= self.config.min_run_width {
                        rows.push((y, begin, x - 1));
                    }
                    start = None;
                }
            }
            if let Some(begin) = start {
                if width - begin >= self.config.min_run_width {
                    rows.push((y, begin, width - 1));
                }
            }
        }

        let edges: Vec<PlatformEdge> = group_segments(rows, 1, 0)
            .into_iter()
            .map(|bounds| PlatformEdge { bounds })
            .collect();

        if edges.is_empty() {
            return Detection::missing(Source::Environment, "no strong horizontal edges found");
        }

        // Confidence scales with the longest edge's coverage of the frame
        // width: a nearly full-width line is very likely a real foothold.
        let longest = edges.iter().map(|edge| edge.bounds.w).max().unwrap_or(0);
        let confidence = Confidence::new(longest as f32 / width as f32).combine(Confidence::new(0.1));
        Detection::found(edges, confidence, Source::Environment, Reliability::Heuristic)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    #[test]
    fn detects_a_strong_horizontal_edge() {
        let mut image = RgbaImage::from_pixel(200, 100, Rgba([20, 20, 20, 255]));
        for y in 60..100 {
            for x in 0..200 {
                image.put_pixel(x, y, Rgba([200, 200, 200, 255]));
            }
        }
        let detection = FootholdDetector::default().detect(&image);
        assert!(detection.is_present());
        let edges = detection.value.unwrap();
        assert!(edges.iter().any(|edge| edge.bounds.y == 59 || edge.bounds.y == 60));
    }

    #[test]
    fn flat_scene_reports_no_edges() {
        let image = RgbaImage::from_pixel(100, 100, Rgba([50, 50, 50, 255]));
        let detection = FootholdDetector::default().detect(&image);
        assert!(!detection.is_present());
    }
}
