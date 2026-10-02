//! The phone link as a QR code: for the console, drawn with half blocks
//! (two rows of modules per line of text), and as an image.

use image::{Rgba, RgbaImage};
use qrcode::{Color, EcLevel, QrCode};

const QUIET: usize = 2;

fn code(text: &str) -> Option<QrCode> {
    QrCode::with_error_correction_level(text.as_bytes(), EcLevel::L).ok()
}

/// Dark modules as `true`, with a quiet zone, row by row.
fn modules(text: &str) -> Option<Vec<Vec<bool>>> {
    let code = code(text)?;
    let width = code.width();
    let colors = code.to_colors();
    let size = width + 2 * QUIET;
    let mut rows = vec![vec![false; size]; size];
    for y in 0..width {
        for x in 0..width {
            rows[y + QUIET][x + QUIET] = colors[y * width + x] == Color::Dark;
        }
    }
    Some(rows)
}

/// The code as lines of text for a dark console: light blocks on the dark
/// background, which phone cameras read like the printed kind.
pub fn console(text: &str) -> Option<Vec<String>> {
    let rows = modules(text)?;
    let mut lines = Vec::with_capacity(rows.len().div_ceil(2));
    for pair in rows.chunks(2) {
        let top = &pair[0];
        let bottom = pair.get(1);
        let line: String = (0..top.len())
            .map(|x| {
                // A light module is drawn; a dark one is left as background.
                let upper = !top[x];
                let lower = bottom.is_some_and(|b| !b[x]);
                match (upper, lower) {
                    (true, true) => '█',
                    (true, false) => '▀',
                    (false, true) => '▄',
                    (false, false) => ' ',
                }
            })
            .collect();
        lines.push(line);
    }
    Some(lines)
}

/// The code as an image, `scale` pixels per module, dark on white.
pub fn image(text: &str, scale: u32) -> Option<RgbaImage> {
    let rows = modules(text)?;
    let size = rows.len() as u32 * scale;
    let mut img = RgbaImage::from_pixel(size, size, Rgba([255, 255, 255, 255]));
    for (y, row) in rows.iter().enumerate() {
        for (x, &dark) in row.iter().enumerate() {
            if dark {
                for dy in 0..scale {
                    for dx in 0..scale {
                        img.put_pixel(
                            x as u32 * scale + dx,
                            y as u32 * scale + dy,
                            Rgba([0, 0, 0, 255]),
                        );
                    }
                }
            }
        }
    }
    Some(img)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_link_fits_in_a_console() {
        let lines = console("https://192.168.100.200:8443/?k=0123456789abcdef01234567").unwrap();
        // Version 3 or 4 at level L: 29–33 modules plus the quiet zone.
        assert!(lines.len() <= 20, "{}", lines.len());
        assert!(
            lines
                .iter()
                .all(|l| l.chars().count() == lines[0].chars().count())
        );
        // The quiet zone is light: the first line is all upper or full blocks.
        assert!(lines[0].chars().all(|c| c == '█' || c == '▀'));
    }

    #[test]
    fn the_image_has_finder_patterns() {
        let img = image("hello", 4).unwrap();
        // The top-left finder's outer ring starts right after the quiet zone.
        let at = (QUIET as u32) * 4 + 1;
        assert_eq!(img.get_pixel(at, at).0, [0, 0, 0, 255]);
        assert_eq!(img.get_pixel(1, 1).0, [255, 255, 255, 255]);
    }
}
