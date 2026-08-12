//! In-place animated terminal dashboard.
//!
//! Routine per-frame information belongs here, not in the log: the same
//! lines are rewritten every frame so the terminal reads as a live
//! instrument panel instead of scrolling past thousands of entries.
//!
//! Rendering is split in two on purpose. [`Dashboard::render_lines`] is a
//! pure function from a frame result to the exact lines to display, which
//! makes the formatting unit-testable without a terminal; [`Dashboard::draw`]
//! is the only part that touches cursor control. The line count must stay
//! constant across frames or the cursor-up redraw would walk up the screen,
//! so that property is covered by a test.

use std::io::Write;

use crate::observe::frame_result::VisionFrameResult;
use crate::vision::detectors::hud::HudMetric;
use crate::vision::types::Detection;

const DIM: &str = "\u{1b}[2m";
const BOLD: &str = "\u{1b}[1m";
const RED: &str = "\u{1b}[31m";
const GREEN: &str = "\u{1b}[32m";
const BLUE: &str = "\u{1b}[34m";
const YELLOW: &str = "\u{1b}[33m";
const CYAN: &str = "\u{1b}[36m";
const RESET: &str = "\u{1b}[0m";

const HIDE_CURSOR: &str = "\u{1b}[?25l";
const SHOW_CURSOR: &str = "\u{1b}[?25h";
const CLEAR_LINE: &str = "\u{1b}[2K";

const LABEL_WIDTH: usize = 13;
const BAR_WIDTH: usize = 20;

/// Format an integer with thousands separators (18427 -> "18,427").
pub fn format_int(value: u64) -> String {
    let digits = value.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, ch) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

/// Render a proportional bar. `None` renders an empty bar rather than
/// vanishing, so the row keeps its width when a detector drops out.
pub fn render_bar(percent: Option<f32>, color: &str) -> String {
    match percent {
        Some(p) => {
            let clamped = p.clamp(0.0, 100.0);
            let filled = ((clamped / 100.0) * BAR_WIDTH as f32).round() as usize;
            format!(
                "[{color}{}{RESET}{DIM}{}{RESET}]",
                "█".repeat(filled),
                "-".repeat(BAR_WIDTH - filled)
            )
        }
        None => format!("[{DIM}{}{RESET}]", "-".repeat(BAR_WIDTH)),
    }
}

fn label(text: &str) -> String {
    format!("{DIM}{text:<LABEL_WIDTH$}{RESET}")
}

fn format_ms(duration: std::time::Duration) -> String {
    format!("{:.1} ms", duration.as_secs_f64() * 1000.0)
}

/// Format a metric's readout, preferring the printed `current / max` over
/// the percentage: the absolute numbers are what a player acts on, and the
/// percentage is a derived estimate.
pub fn format_amount(current: Option<u64>, max: Option<u64>) -> String {
    match (current, max) {
        (Some(current), Some(max)) => format!("{} / {}", format_int(current), format_int(max)),
        (Some(current), None) => format_int(current),
        // Nothing readable; the bar alongside still carries the percentage.
        _ => "--".to_string(),
    }
}

/// A metric row: the printed amounts when OCR read them, always the bar.
fn metric_row(name: &str, metric: &Detection<HudMetric>, color: &str) -> String {
    let percent = metric.value.as_ref().and_then(|m| m.percent);
    let absolute = metric.value.as_ref().and_then(|m| m.value);
    let maximum = metric.value.as_ref().and_then(|m| m.max);

    let amount = format_amount(absolute, maximum);
    let value_text = if absolute.is_some() {
        format!("{BOLD}{amount:>17}{RESET}")
    } else {
        format!("{DIM}{amount:>17}{RESET}")
    };
    let percent_text = match percent {
        Some(p) => format!("{p:>5.1}%"),
        None => format!("{DIM}{:>6}{RESET}", "--"),
    };

    format!(
        "{}{value_text}   {}  {percent_text}",
        label(name),
        render_bar(percent, color)
    )
}

fn status_mark(present: bool) -> String {
    if present {
        format!("{GREEN}detected ✓{RESET}")
    } else {
        format!("{DIM}absent   ·{RESET}")
    }
}

/// A detector status row including confidence, so a low-confidence hit is
/// visibly different from a solid one.
fn detector_row(name: &str, present: bool, confidence: f32) -> String {
    let confidence_text = if present {
        format!("{DIM}conf {:.2}{RESET}", confidence)
    } else {
        String::new()
    };
    format!(
        "{}{}  {}",
        label(name),
        status_mark(present),
        confidence_text
    )
}

/// Owns the cursor bookkeeping for redrawing in place.
pub struct Dashboard {
    lines_drawn: usize,
    cursor_hidden: bool,
}

impl Default for Dashboard {
    fn default() -> Self {
        Self::new()
    }
}

impl Dashboard {
    pub fn new() -> Self {
        Self {
            lines_drawn: 0,
            cursor_hidden: false,
        }
    }

    /// Build the full dashboard as display lines.
    ///
    /// Pure: same result in, same lines out. The number of lines must not
    /// depend on the data, only on the layout.
    pub fn render_lines(&self, result: &VisionFrameResult, dropped: u64) -> Vec<String> {
        let world = &result.world;
        let (width, height) = result.frame_size();
        let timings = &result.timings;

        // The engine has no dedicated player detector; the longest-lived
        // motion track is the best available proxy and is labeled as such
        // rather than being presented as a confirmed player detection.
        let entities = world.motion.value.as_deref().unwrap_or(&[]);
        let primary = entities.iter().max_by_key(|entity| entity.age_frames);

        let mut lines = Vec::with_capacity(32);

        lines.push(format!("{BOLD}MapleSyrup Vision Engine{RESET}"));
        lines.push(format!("{DIM}{}{RESET}", "─".repeat(52)));
        lines.push(String::new());

        lines.push(format!(
            "{}{CYAN}{}{RESET} {DIM}{}x{}{RESET}",
            label("Source:"),
            truncate(&result.source, 28),
            width,
            height
        ));
        lines.push(format!(
            "{}{BOLD}{}{RESET}",
            label("Frame:"),
            format_int(result.frame_id)
        ));
        lines.push(format!("{}{:.1}", label("FPS:"), result.fps));
        lines.push(format!(
            "{}{}",
            label("Capture:"),
            format_ms(timings.capture)
        ));
        lines.push(format!("{}{}", label("Vision:"), format_ms(timings.vision)));
        lines.push(format!(
            "{}{BOLD}{}{RESET}",
            label("Total:"),
            format_ms(timings.total())
        ));
        lines.push(String::new());

        lines.push(metric_row("HP:", &world.hud.hp, RED));
        lines.push(metric_row("MP:", &world.hud.mp, BLUE));
        lines.push(metric_row("EXP:", &world.hud.exp, YELLOW));
        lines.push(format!(
            "{}{}",
            label("Level:"),
            optional_text(world.hud.level.value.as_deref())
        ));
        lines.push(format!(
            "{}{}",
            label("Player:"),
            optional_text(world.hud.player_name.value.as_deref())
        ));
        lines.push(format!(
            "{}{}",
            label("Job:"),
            optional_text(world.hud.character_class.value.as_deref())
        ));
        lines.push(String::new());

        lines.push(detector_row(
            "HP bar:",
            world.hud.hp.is_present(),
            world.hud.hp.confidence.value(),
        ));
        lines.push(detector_row(
            "MP bar:",
            world.hud.mp.is_present(),
            world.hud.mp.confidence.value(),
        ));
        lines.push(detector_row(
            "EXP bar:",
            world.hud.exp.is_present(),
            world.hud.exp.confidence.value(),
        ));
        lines.push(detector_row(
            "Minimap:",
            world.minimap.is_present(),
            world.minimap.confidence.value(),
        ));
        lines.push(detector_row(
            "Dialog:",
            world.dialog.is_present(),
            world.dialog.confidence.value(),
        ));
        lines.push(detector_row(
            "Chat log:",
            world.chat_log.is_present(),
            world.chat_log.confidence.value(),
        ));
        lines.push(String::new());

        lines.push(format!(
            "{}{}",
            label("Entities:"),
            format_int(entities.len() as u64)
        ));
        lines.push(format!(
            "{}{}",
            label("Tracked:"),
            match primary {
                Some(entity) => format!(
                    "id {} at ({}, {}) {DIM}age {}f{RESET}",
                    entity.id,
                    entity.bounds.x + entity.bounds.w / 2,
                    entity.bounds.y + entity.bounds.h / 2,
                    entity.age_frames
                ),
                None => format!("{DIM}none{RESET}"),
            }
        ));
        lines.push(format!(
            "{}{}",
            label("Minimap at:"),
            match world.minimap.value.as_ref() {
                Some(minimap) => format!(
                    "({}, {}) {DIM}{}x{}{RESET}",
                    minimap.bounds.x, minimap.bounds.y, minimap.bounds.w, minimap.bounds.h
                ),
                None => format!("{DIM}--{RESET}"),
            }
        ));
        lines.push(format!(
            "{}{}",
            label("Footholds:"),
            format_int(world.footholds.value.as_ref().map(|f| f.len()).unwrap_or(0) as u64)
        ));
        lines.push(format!(
            "{}{}",
            label("Combat:"),
            match world.combat_intensity.value.as_ref() {
                Some(combat) => format!("{:?}", combat.intensity),
                None => format!("{DIM}unknown{RESET}"),
            }
        ));
        lines.push(String::new());

        lines.push(format!(
            "{}{DIM}{} skipped for display{RESET}",
            label("Preview:"),
            format_int(dropped)
        ));
        lines.push(format!(
            "{}{GREEN}RUNNING{RESET} {DIM}· ctrl-c to stop{RESET}",
            label("Status:")
        ));

        lines
    }

    /// Draw the dashboard, overwriting the previous render in place.
    pub fn draw(&mut self, result: &VisionFrameResult, dropped: u64) {
        let lines = self.render_lines(result, dropped);
        let mut out = String::new();

        if !self.cursor_hidden {
            out.push_str(HIDE_CURSOR);
            self.cursor_hidden = true;
        }
        if self.lines_drawn > 0 {
            // Walk back over the previous render rather than clearing the
            // screen, which would flicker.
            out.push_str(&format!("\u{1b}[{}A", self.lines_drawn));
        }
        for line in &lines {
            out.push_str(CLEAR_LINE);
            out.push_str(line);
            out.push('\n');
        }

        let mut stdout = std::io::stdout();
        let _ = stdout.write_all(out.as_bytes());
        let _ = stdout.flush();
        self.lines_drawn = lines.len();
    }

    /// Restore the cursor. Call before exiting or before printing anything
    /// that should scroll normally.
    pub fn finish(&mut self) {
        if self.cursor_hidden {
            let mut stdout = std::io::stdout();
            let _ = stdout.write_all(SHOW_CURSOR.as_bytes());
            let _ = stdout.flush();
            self.cursor_hidden = false;
        }
    }
}

impl Drop for Dashboard {
    fn drop(&mut self) {
        // Leaving a terminal without a cursor would outlive the process.
        self.finish();
    }
}

fn optional_text(value: Option<&str>) -> String {
    match value {
        Some(text) if !text.trim().is_empty() => format!("{BOLD}{text}{RESET}"),
        _ => format!("{DIM}unknown{RESET}"),
    }
}

fn truncate(text: &str, max: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= max {
        return text.to_string();
    }
    format!(
        "{}…",
        chars[..max.saturating_sub(1)].iter().collect::<String>()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::observe::frame_result::{FrameTimings, VisionFrameResult};
    use crate::vision::snapshot::PerceptionPipeline;
    use image::{Rgba, RgbaImage};
    use std::sync::Arc;
    use std::time::Duration;

    fn result_from(image: RgbaImage, frame_id: u64) -> VisionFrameResult {
        let world = PerceptionPipeline::new().detect(&image);
        VisionFrameResult {
            frame_id,
            elapsed_ms: 1000,
            source: "fixture".into(),
            image: Arc::new(image),
            world: Arc::new(world),
            timings: FrameTimings {
                capture: Duration::from_millis(3),
                vision: Duration::from_millis(5),
                frame_interval: Duration::from_millis(16),
            },
            fps: 60.0,
        }
    }

    #[test]
    fn amounts_prefer_current_and_max_over_a_bare_number() {
        assert_eq!(format_amount(Some(2341), Some(3100)), "2,341 / 3,100");
        assert_eq!(format_amount(Some(400), Some(400)), "400 / 400");
        // A maximum the HUD did not print must not be invented.
        assert_eq!(format_amount(Some(35900), None), "35,900");
        assert_eq!(format_amount(None, None), "--");
        assert_eq!(format_amount(None, Some(400)), "--");
    }

    #[test]
    fn format_int_inserts_thousands_separators() {
        assert_eq!(format_int(0), "0");
        assert_eq!(format_int(999), "999");
        assert_eq!(format_int(1000), "1,000");
        assert_eq!(format_int(18427), "18,427");
        assert_eq!(format_int(1234567), "1,234,567");
    }

    #[test]
    fn bar_fills_proportionally_and_clamps() {
        let full = render_bar(Some(100.0), "");
        assert_eq!(full.matches('█').count(), BAR_WIDTH);

        let empty = render_bar(Some(0.0), "");
        assert_eq!(empty.matches('█').count(), 0);

        let half = render_bar(Some(50.0), "");
        assert_eq!(half.matches('█').count(), BAR_WIDTH / 2);

        // Out-of-range input must not produce a wider bar than the layout.
        let over = render_bar(Some(150.0), "");
        assert_eq!(over.matches('█').count(), BAR_WIDTH);
    }

    #[test]
    fn missing_percent_still_renders_a_full_width_bar() {
        let absent = render_bar(None, "");
        assert_eq!(absent.matches('-').count(), BAR_WIDTH);
    }

    #[test]
    fn line_count_is_stable_across_different_frames() {
        // The in-place redraw walks the cursor up by the previous line
        // count, so a layout whose height depends on the data would
        // progressively corrupt the display.
        let dashboard = Dashboard::new();

        let blank = RgbaImage::from_pixel(320, 240, Rgba([0, 0, 0, 255]));
        let noisy = {
            let mut image = RgbaImage::from_pixel(320, 240, Rgba([10, 10, 10, 255]));
            for x in 20..80 {
                for y in 20..60 {
                    image.put_pixel(x, y, Rgba([220, 40, 40, 255]));
                }
            }
            image
        };

        let a = dashboard.render_lines(&result_from(blank, 1), 0);
        let b = dashboard.render_lines(&result_from(noisy, 999_999), 4242);
        assert_eq!(a.len(), b.len());
    }

    #[test]
    fn dashboard_reports_frame_and_timing_values() {
        let dashboard = Dashboard::new();
        let image = RgbaImage::from_pixel(320, 240, Rgba([0, 0, 0, 255]));
        let lines = dashboard.render_lines(&result_from(image, 18427), 0);
        let text = lines.join("\n");

        assert!(text.contains("18,427"), "frame number should be formatted");
        assert!(text.contains("3.0 ms"), "capture latency should appear");
        assert!(text.contains("5.0 ms"), "vision latency should appear");
        assert!(text.contains("8.0 ms"), "total should be capture + vision");
        assert!(text.contains("60.0"), "fps should appear");
    }

    #[test]
    fn absent_detectors_render_as_absent_not_as_values() {
        let dashboard = Dashboard::new();
        // A blank frame gives the detectors nothing to find.
        let image = RgbaImage::from_pixel(320, 240, Rgba([0, 0, 0, 255]));
        let lines = dashboard.render_lines(&result_from(image, 1), 0);
        let text = lines.join("\n");
        assert!(
            text.contains("absent"),
            "missing detections must be shown as absent"
        );
    }
}
