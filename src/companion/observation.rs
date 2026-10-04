//! What the companion needs from one frame, taken out of the vision
//! engine's [`WorldState`].
//!
//! The engine keeps two kinds of numbers apart and so does this: a value the
//! game printed and the engine read as text ([`Gauge::read`] is true), and a
//! bar's fill measured in pixels, which is only an estimate. The companion
//! says "HP 82 percent" for the first and "HP about 82 percent" for the
//! second, and never turns an estimate into a printed number.

use serde::Serialize;

use crate::vision::hud_ocr::{HudField, HudOcrResult, ParsedValue};
use crate::vision::{Reliability, WorldState};

/// One of HP, MP or EXP.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Gauge {
    /// 0 to 100.
    pub percent: f32,
    /// The printed `current`, when it was read and trusted.
    pub current: Option<u64>,
    /// The printed `max`, when it was read and trusted.
    pub max: Option<u64>,
    /// Whether `percent` comes from numbers the game printed (true) or from
    /// the bar's measured fill (false, an estimate).
    pub read: bool,
}

/// Whether the game window could be captured this frame.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "state", content = "detail", rename_all = "snake_case")]
pub enum GameView {
    /// Captured; the window's title.
    Seen(String),
    /// No window that looks like the game.
    NotFound,
    /// The window is there but could not be captured, and why (minimised…).
    Unavailable(String),
}

impl GameView {
    pub fn is_seen(&self) -> bool {
        matches!(self, GameView::Seen(_))
    }
}

/// The companion's view of one frame.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Observation {
    pub game: GameView,
    pub hp: Option<Gauge>,
    pub mp: Option<Gauge>,
    pub exp: Option<Gauge>,
    pub level: Option<u32>,
    pub name: Option<String>,
    pub job: Option<String>,
}

impl Observation {
    /// A frame in which the game was not seen at all.
    pub fn unseen(game: GameView) -> Self {
        Self {
            game,
            hp: None,
            mp: None,
            exp: None,
            level: None,
            name: None,
            job: None,
        }
    }

    /// Everything the engine saw in `world`, from the window called `title`.
    ///
    /// A bar the HUD geometry detector only guessed at — a run of the right
    /// colour somewhere in the status band — is left out, however it is
    /// labelled: only a percent worked out from the numbers read beside it
    /// (`current / max`) is taken. On a screen whose HUD the detector was
    /// not tuned for, a guess can swing from 3% to 46% and back within
    /// seconds, and every swing would be a warning (one night of them ran
    /// to 3,400). What the player's own learned sight measures is put in
    /// afterwards, by [`crate::sight::Sight::apply`].
    pub fn from_world(title: &str, world: &WorldState) -> Self {
        let field = |field: HudField| world.hud.ocr.iter().find(|r| r.field == field);
        let bar = |metric: &crate::vision::Detection<crate::vision::detectors::hud::HudMetric>| {
            metric
                .value
                .as_ref()
                .filter(|_| metric.reliability == Reliability::Corroborated)
                .and_then(|m| match (m.value, m.max) {
                    (Some(current), Some(max)) if max > 0 => {
                        Some((current as f32 / max as f32 * 100.0).clamp(0.0, 100.0))
                    }
                    _ => None,
                })
        };
        Self {
            game: GameView::Seen(title.to_string()),
            hp: gauge(field(HudField::Hp), bar(&world.hud.hp)),
            mp: gauge(field(HudField::Mp), bar(&world.hud.mp)),
            exp: gauge(field(HudField::Exp), bar(&world.hud.exp)),
            level: field(HudField::Level).and_then(level_of),
            name: field(HudField::PlayerName).and_then(text_of),
            job: field(HudField::Job).and_then(text_of),
        }
    }
}

/// The printed value when it was read and is trusted, else the bar's fill.
fn gauge(reading: Option<&HudOcrResult>, bar: Option<f32>) -> Option<Gauge> {
    if let Some(reading) = reading.filter(|r| r.is_trusted())
        && let Some(percent) = reading.percent()
    {
        return Some(Gauge {
            percent: percent.clamp(0.0, 100.0),
            current: reading.current(),
            max: reading.maximum(),
            read: true,
        });
    }
    bar.filter(|p| p.is_finite()).map(|percent| Gauge {
        percent: percent.clamp(0.0, 100.0),
        current: None,
        max: None,
        read: false,
    })
}

fn level_of(reading: &HudOcrResult) -> Option<u32> {
    if !reading.is_trusted() {
        return None;
    }
    let text = match &reading.parsed {
        ParsedValue::Text(text) => text.clone(),
        ParsedValue::Amount { current, .. } => current.to_string(),
        _ => return None,
    };
    let level: u32 = text.trim().parse().ok()?;
    (1..=300).contains(&level).then_some(level)
}

fn text_of(reading: &HudOcrResult) -> Option<String> {
    match &reading.parsed {
        ParsedValue::Text(text) if reading.is_trusted() && !text.trim().is_empty() => {
            Some(text.trim().to_string())
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vision::Rect;
    use crate::vision::hud_ocr::ReadState;

    fn reading(field: HudField, parsed: ParsedValue, confidence: f32) -> HudOcrResult {
        HudOcrResult {
            field,
            roi: Rect {
                x: 0,
                y: 0,
                w: 10,
                h: 10,
            },
            raw_text: Some("x".into()),
            parsed,
            frame_id: 1,
            state: ReadState::ReadThisFrame,
            quality: None,
            note: None,
            confidence,
        }
    }

    #[test]
    fn a_trusted_reading_wins_over_the_bar() {
        let r = reading(
            HudField::Hp,
            ParsedValue::Amount {
                current: 500,
                max: Some(1000),
            },
            0.9,
        );
        let g = gauge(Some(&r), Some(80.0)).unwrap();
        assert!(g.read);
        assert_eq!(g.percent, 50.0);
        assert_eq!((g.current, g.max), (Some(500), Some(1000)));
    }

    #[test]
    fn an_untrusted_reading_falls_back_to_the_bar_estimate() {
        let r = reading(
            HudField::Hp,
            ParsedValue::Amount {
                current: 500,
                max: Some(1000),
            },
            0.2,
        );
        let g = gauge(Some(&r), Some(80.0)).unwrap();
        assert!(!g.read);
        assert_eq!(g.percent, 80.0);
        assert_eq!(g.current, None);
    }

    #[test]
    fn a_bar_the_detector_only_guessed_at_is_not_an_observation() {
        use crate::vision::detectors::hud::HudMetric;
        use crate::vision::{Confidence, Detection, Detectors, PerceptionPipeline};
        let blank = image::RgbaImage::new(64, 64);
        let mut world = PerceptionPipeline::new().detect_some(&blank, 1, Detectors::NONE);
        let metric = |percent: f32, value: Option<u64>| HudMetric {
            label: "x".into(),
            percent: Some(percent),
            value,
            max: None,
            raw_text: None,
        };
        // A run of red somewhere in the band, nothing read beside it.
        world.hud.hp = Detection::found(
            metric(9.0, None),
            Confidence::new(0.55),
            "hud",
            Reliability::Heuristic,
        );
        // A blue bar with "3574 / 3574" read over it.
        world.hud.mp = Detection::found(
            HudMetric {
                max: Some(3574),
                ..metric(100.0, Some(3574))
            },
            Confidence::new(0.9),
            "hud",
            Reliability::Corroborated,
        );
        // A yellow bar "corroborated" by a stray digit in the text beside
        // it, its percent still the fill's guess: not taken either.
        world.hud.exp = Detection::found(
            metric(40.0, Some(8)),
            Confidence::new(0.9),
            "hud",
            Reliability::Corroborated,
        );
        let obs = Observation::from_world("MapleStory", &world);
        assert_eq!(obs.hp, None, "{:?}", obs.hp);
        assert_eq!(obs.mp.map(|g| g.percent), Some(100.0));
        assert_eq!(obs.exp, None, "{:?}", obs.exp);
    }

    #[test]
    fn nothing_seen_gives_no_gauge() {
        assert_eq!(gauge(None, None), None);
        assert_eq!(gauge(None, Some(f32::NAN)), None);
    }

    #[test]
    fn levels_come_only_from_trusted_plausible_reads() {
        assert_eq!(
            level_of(&reading(
                HudField::Level,
                ParsedValue::Text("57".into()),
                0.8
            )),
            Some(57)
        );
        assert_eq!(
            level_of(&reading(
                HudField::Level,
                ParsedValue::Text("57".into()),
                0.1
            )),
            None
        );
        assert_eq!(
            level_of(&reading(
                HudField::Level,
                ParsedValue::Text("999".into()),
                0.8
            )),
            None
        );
    }
}
