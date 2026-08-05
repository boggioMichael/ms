use crate::frame::Frame;

/// Estimate HP percentage from a captured frame.
///
/// This is a lightweight, best-effort heuristic intended as a placeholder
/// "vision" implementation. It does not perform advanced image processing
/// and is intentionally simple and fast. It searches for a horizontal red
/// bar in the lower-left area of the frame and returns a percentage 0.0-100.0
/// when detected. If no bar-like structure is found, returns None.
///
/// The algorithm:
/// - Define a candidate bar rectangle relative to the image size (5% from left,
///   vertical center near 90% of image height, width 40% of image width).
/// - For each column in that rectangle, compute the average red channel across
///   the bar height.
/// - Treat a column as "filled" when red is dominant (red > green+20 and
///   red > blue+20) and red intensity above a small floor.
/// - Consider contiguous filled columns from the left; when a run of
///   consecutive empty columns exceeds a small tolerance, stop.
/// - HP percentage = contiguous_filled_columns / bar_width.
pub fn estimate_hp_percentage(frame: &Frame) -> Option<f32> {
    let w = frame.width as usize;
    let h = frame.height as usize;
    if w == 0 || h == 0 || frame.pixels.len() < 4 {
        return None;
    }

    // Bar geometry (relative)
    let start_x = (w as f32 * 0.05).max(0.0) as usize;
    let bar_width = (w as f32 * 0.40).max(1.0) as usize;
    let bar_height = (h as f32 * 0.05).max(3.0) as usize; // at least a few rows
    let center_y = (h as f32 * 0.9) as isize; // near bottom
    // top-left y coordinate of bar
    let start_y = (center_y - (bar_height as isize / 2)).max(0) as usize;
    if start_x + bar_width > w || start_y + bar_height > h {
        return None;
    }

    // Helper to get RGBA channels
    let stride = w * 4;
    let mut column_scores: Vec<f32> = Vec::with_capacity(bar_width);
    for cx in 0..bar_width {
        let x = start_x + cx;
        let mut red_sum: u32 = 0;
        let mut green_sum: u32 = 0;
        let mut blue_sum: u32 = 0;
        for ry in 0..bar_height {
            let y = start_y + ry;
            let idx = y * stride + x * 4;
            if idx + 3 >= frame.pixels.len() {
                continue;
            }
            let r = frame.pixels[idx];
            let g = frame.pixels[idx + 1];
            let b = frame.pixels[idx + 2];
            // Note: Frame stores RGBA in this codebase
            red_sum += r as u32;
            green_sum += g as u32;
            blue_sum += b as u32;
        }
        let denom = (bar_height * 255) as f32;
        if denom == 0.0 {
            column_scores.push(0.0);
            continue;
        }
        let red_avg = red_sum as f32 / denom; // 0.0 - 1.0
        let green_avg = green_sum as f32 / denom;
        let blue_avg = blue_sum as f32 / denom;
        // Score is red dominance and intensity
        let score = (red_avg - (green_avg + blue_avg) * 0.5).max(0.0) * red_avg;
        column_scores.push(score);
    }

    // Determine contiguous filled columns from left
    let intensity_threshold = 0.08_f32; // empirical floor
    let mut filled_columns = 0usize;
    let mut empty_run = 0usize;
    let max_empty_tolerance = 3usize; // allow small gaps
    for (i, &s) in column_scores.iter().enumerate() {
        if s >= intensity_threshold {
            filled_columns += 1 + empty_run; // count previous tolerated empties as filled
            empty_run = 0;
        } else {
            empty_run += 1;
            if empty_run > max_empty_tolerance {
                break; // stop at a sufficiently long empty run
            }
        }
    }

    if filled_columns == 0 {
        return None;
    }
    let pct = (filled_columns as f32 / bar_width as f32) * 100.0;
    Some(pct.min(100.0))
}
