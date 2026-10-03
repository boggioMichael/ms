//! The on-screen panel laid over the game window: HP, MP, EXP, the phone,
//! and the latest line. This module only paints it — an RGBA picture with
//! premultiplied alpha, plus where each piece of text goes — so it can be
//! tested anywhere; the window that shows it is Windows-only
//! (`platform::overlay`).

use image::{Rgba, RgbaImage};

use crate::companion::{Gauge, Observation};

/// A piece of text to write on the panel (by the system's text renderer).
#[derive(Debug, Clone, PartialEq)]
pub struct Text {
    /// `(left, top, right, bottom)` in panel pixels.
    pub rect: (i32, i32, i32, i32),
    pub text: String,
    /// Height of the font in pixels.
    pub size: i32,
    pub bold: bool,
    /// `(r, g, b)`.
    pub color: (u8, u8, u8),
    pub right: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Panel {
    /// Premultiplied RGBA.
    pub image: RgbaImage,
    pub texts: Vec<Text>,
}

/// What the panel shows.
pub struct Content<'a> {
    pub obs: Option<&'a Observation>,
    pub exp_per_hour: Option<f64>,
    pub phone_connected: Option<bool>,
    pub speaking: bool,
    pub muted: bool,
    pub last_line: Option<&'a str>,
    /// The dog's current frame (premultiplied), drawn to the left of the
    /// panel at the panel's height.
    pub dog: Option<&'a RgbaImage>,
}

const BASE_W: f32 = 290.0;
const BASE_H: f32 = 116.0;
const BG: (u8, u8, u8) = (22, 18, 13);
const BORDER: (u8, u8, u8) = (242, 163, 58);
const TRACK: (u8, u8, u8) = (14, 11, 8);
const HP: (u8, u8, u8) = (239, 83, 80);
const MP: (u8, u8, u8) = (66, 165, 245);
const EXP: (u8, u8, u8) = (244, 196, 48);
const WHITE: (u8, u8, u8) = (246, 239, 228);
const DIM: (u8, u8, u8) = (183, 166, 142);
const GREEN: (u8, u8, u8) = (102, 187, 106);

/// The panel's scale for a game drawn `game_height` pixels tall: 1 at 768.
pub fn scale_for(game_height: i32) -> f32 {
    (game_height as f32 / 768.0).clamp(1.0, 2.5)
}

/// Size in pixels at `scale`.
pub fn size(scale: f32) -> (u32, u32) {
    (
        (BASE_W * scale).round() as u32,
        (BASE_H * scale).round() as u32,
    )
}

fn premultiplied(color: (u8, u8, u8), alpha: f32) -> Rgba<u8> {
    let a = alpha.clamp(0.0, 1.0);
    Rgba([
        (color.0 as f32 * a).round() as u8,
        (color.1 as f32 * a).round() as u8,
        (color.2 as f32 * a).round() as u8,
        (255.0 * a).round() as u8,
    ])
}

/// How much of the pixel at `(x, y)` lies inside a rounded rectangle.
fn coverage(x: f32, y: f32, w: f32, h: f32, radius: f32) -> f32 {
    let cx = x.clamp(radius, w - radius);
    let cy = y.clamp(radius, h - radius);
    let d = ((x - cx).powi(2) + (y - cy).powi(2)).sqrt();
    (radius - d + 0.5).clamp(0.0, 1.0)
}

fn fill_rect(img: &mut RgbaImage, x: i32, y: i32, w: i32, h: i32, color: Rgba<u8>) {
    for yy in y.max(0)..(y + h).min(img.height() as i32) {
        for xx in x.max(0)..(x + w).min(img.width() as i32) {
            img.put_pixel(xx as u32, yy as u32, color);
        }
    }
}

fn percent_text(g: Option<Gauge>) -> String {
    match g {
        None => "--".into(),
        Some(g) => {
            let p = if g.percent >= 10.0 {
                format!("{:.0}%", g.percent)
            } else {
                format!("{:.1}%", g.percent)
            };
            if g.read { p } else { format!("~{p}") }
        }
    }
}

/// Paint the panel at `scale`.
pub fn paint(content: &Content, scale: f32) -> Panel {
    let (w, h) = size(scale);
    let s = |v: f32| (v * scale).round() as i32;
    let mut img = RgbaImage::new(w, h);
    let radius = 10.0 * scale;
    for y in 0..h {
        for x in 0..w {
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            let outer = coverage(fx, fy, w as f32, h as f32, radius);
            if outer <= 0.0 {
                continue;
            }
            let inner = coverage(
                fx - 1.0,
                fy - 1.0,
                w as f32 - 2.0,
                h as f32 - 2.0,
                radius - 1.0,
            );
            // Border where the outer shape is and the inner is not.
            let border = (outer - inner).max(0.0);
            let bg = 0.84 * inner;
            let alpha = bg + 0.9 * border;
            let mix =
                |a: u8, b: u8| ((a as f32 * bg + b as f32 * 0.9 * border) / alpha.max(1e-6)) as u8;
            let color = (
                mix(BG.0, BORDER.0),
                mix(BG.1, BORDER.1),
                mix(BG.2, BORDER.2),
            );
            img.put_pixel(x, y, premultiplied(color, alpha * outer.min(1.0)));
        }
    }

    let mut texts = Vec::new();
    let pad = s(12.0);
    let right = w as i32 - pad;
    texts.push(Text {
        rect: (pad, s(6.0), s(150.0), s(26.0)),
        text: "MapleSyrup".into(),
        size: s(15.0),
        bold: true,
        color: BORDER,
        right: false,
    });
    let (phone_text, phone_color) = match content.phone_connected {
        Some(true) if content.speaking => ("● phone · speaking", GREEN),
        Some(true) => ("● phone", GREEN),
        Some(false) => ("○ phone", DIM),
        None => ("", DIM),
    };
    let header_right = if content.muted {
        format!("muted  {phone_text}")
    } else {
        phone_text.to_string()
    };
    texts.push(Text {
        rect: (s(130.0), s(6.0), right, s(26.0)),
        text: header_right,
        size: s(13.0),
        bold: false,
        color: phone_color,
        right: true,
    });

    let seen = content.obs.filter(|o| o.game.is_seen());
    let rows = [
        ("HP", seen.and_then(|o| o.hp), HP),
        ("MP", seen.and_then(|o| o.mp), MP),
        ("EXP", seen.and_then(|o| o.exp), EXP),
    ];
    for (i, (label, gauge, color)) in rows.into_iter().enumerate() {
        let top = s(30.0 + i as f32 * 20.0);
        let row_h = s(18.0);
        texts.push(Text {
            rect: (pad, top, s(46.0), top + row_h),
            text: label.into(),
            size: s(13.0),
            bold: true,
            color: WHITE,
            right: false,
        });
        let (bx, bw, bh) = (s(48.0), s(150.0), s(9.0));
        let by = top + (row_h - bh) / 2;
        fill_rect(&mut img, bx, by, bw, bh, premultiplied(TRACK, 0.95));
        if let Some(g) = gauge {
            let filled = ((g.percent / 100.0).clamp(0.0, 1.0) * bw as f32).round() as i32;
            fill_rect(&mut img, bx, by, filled, bh, premultiplied(color, 1.0));
        }
        let mut value = percent_text(gauge);
        if label == "EXP"
            && let Some(rate) = content.exp_per_hour
        {
            value = format!("{value} {rate:+.1}/h");
        }
        texts.push(Text {
            rect: (s(202.0), top, right, top + row_h),
            text: value,
            size: s(13.0),
            bold: false,
            color: WHITE,
            right: true,
        });
    }

    let footer = match (seen.is_some(), content.last_line) {
        (_, Some(line)) => line.to_string(),
        (false, None) => "Looking for MapleStory…".into(),
        (true, None) => "Say \"syrup, status\"".into(),
    };
    texts.push(Text {
        rect: (pad, s(92.0), right, s(110.0)),
        text: footer,
        size: s(13.0),
        bold: false,
        color: if content.speaking { WHITE } else { DIM },
        right: false,
    });
    match content.dog {
        Some(dog) => beside(dog, img, texts),
        None => Panel { image: img, texts },
    }
}

/// The dog to the left of the panel, both in one picture.
fn beside(dog: &RgbaImage, panel: RgbaImage, mut texts: Vec<Text>) -> Panel {
    let gap = (dog.width() / 12).max(2);
    let left = dog.width() + gap;
    let height = panel.height().max(dog.height());
    let mut canvas = RgbaImage::new(left + panel.width(), height);
    let dog_top = (height - dog.height()) / 2;
    image::imageops::replace(&mut canvas, dog, 0, dog_top as i64);
    let panel_top = (height - panel.height()) / 2;
    image::imageops::replace(&mut canvas, &panel, left as i64, panel_top as i64);
    for text in &mut texts {
        text.rect.0 += left as i32;
        text.rect.2 += left as i32;
        text.rect.1 += panel_top as i32;
        text.rect.3 += panel_top as i32;
    }
    Panel {
        image: canvas,
        texts,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::companion::GameView;

    fn obs() -> Observation {
        Observation {
            game: GameView::Seen("MapleStory".into()),
            hp: Some(Gauge {
                percent: 50.0,
                current: None,
                max: None,
                read: true,
            }),
            mp: Some(Gauge {
                percent: 25.0,
                current: None,
                max: None,
                read: false,
            }),
            exp: None,
            level: Some(57),
            name: None,
            job: None,
        }
    }

    #[test]
    fn the_panel_is_premultiplied_with_see_through_corners() {
        let o = obs();
        let panel = paint(
            &Content {
                obs: Some(&o),
                exp_per_hour: Some(9.1),
                phone_connected: Some(true),
                speaking: false,
                muted: false,
                last_line: Some("HP 50 percent."),
                dog: None,
            },
            1.0,
        );
        assert_eq!(panel.image.dimensions(), size(1.0));
        // The corner is outside the rounded shape: fully transparent.
        assert_eq!(panel.image.get_pixel(0, 0).0, [0, 0, 0, 0]);
        // Every pixel is premultiplied: no channel above alpha.
        assert!(
            panel
                .image
                .pixels()
                .all(|p| p.0[0] <= p.0[3] && p.0[1] <= p.0[3] && p.0[2] <= p.0[3])
        );
        // The HP bar is half filled: red at a quarter of the bar, track at three quarters.
        let y = (30.0 + 9.0) as u32;
        let red = panel.image.get_pixel(48 + 37, y).0;
        let track = panel.image.get_pixel(48 + 112, y).0;
        assert!(red[0] > 200 && red[3] == 255, "{red:?}");
        assert!(track[0] < 30, "{track:?}");
        let texts: Vec<&str> = panel.texts.iter().map(|t| t.text.as_str()).collect();
        assert!(texts.contains(&"50%") && texts.contains(&"~25%") && texts.contains(&"-- +9.1/h"));
        assert!(texts.contains(&"HP 50 percent."));
    }

    #[test]
    fn the_dog_sits_to_the_left() {
        let o = obs();
        let mut dog = crate::app::dog::Dog::load().unwrap();
        let (_, h) = size(1.0);
        let frame = dog.frame(h, crate::app::dog::Mood::default());
        let panel = paint(
            &Content {
                obs: Some(&o),
                exp_per_hour: None,
                phone_connected: Some(true),
                speaking: true,
                muted: false,
                last_line: Some("Hey!"),
                dog: Some(&frame),
            },
            1.0,
        );
        let (pw, ph) = size(1.0);
        assert_eq!(panel.image.height(), ph);
        assert!(panel.image.width() > pw + frame.width());
        // Every text moved right of the dog.
        assert!(panel.texts.iter().all(|t| t.rect.0 >= frame.width() as i32));
        // The dog is there, opaque, left of the panel.
        assert!(panel.image.get_pixel(frame.width() / 2, ph * 4 / 10).0[3] > 200);
    }

    #[test]
    fn the_panel_grows_with_the_game() {
        assert_eq!(scale_for(768), 1.0);
        assert_eq!(scale_for(1440), 1440.0 / 768.0);
        assert_eq!(scale_for(4000), 2.5);
        let o = obs();
        let content = Content {
            obs: Some(&o),
            exp_per_hour: None,
            phone_connected: None,
            speaking: false,
            muted: true,
            last_line: None,
            dog: None,
        };
        let big = paint(&content, 2.0);
        assert_eq!(big.image.dimensions(), (580, 232));
        assert!(big.texts.iter().any(|t| t.text.starts_with("muted")));
    }
}
