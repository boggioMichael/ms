//! The teacher at work, on a thread of its own: it looks at the newest
//! frame when the learned sight wants it to — to find the HUD on a new
//! screen, to read it every few minutes, after a level-up or a correction —
//! and checks the near misses the things the player taught turned up.
//! Before any of that, and with or without a model, it labels the HUD's
//! font for the sight from the OCR engine, when a line is sharp enough for
//! it (`sight::numbers`).
//!
//! Failures (no network, no credit) slow it down rather than stop it.

use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use image::{Rgba, RgbaImage};

use super::openai::OpenAi;
use super::tools::look;
use crate::ai::images::NBox;
use crate::sight::numbers::{Field, Numbers};
use crate::sight::teacher;
use crate::sight::{Layout, Sight, Want};
use crate::vision::ocr;
use crate::vision::quality::assess_text_quality;

/// The newest frame of the game, for whoever needs it.
#[derive(Default)]
pub struct Latest(Mutex<Option<Arc<RgbaImage>>>);

impl Latest {
    pub fn put(&self, frame: Arc<RgbaImage>) {
        *self.0.lock().unwrap_or_else(|e| e.into_inner()) = Some(frame);
    }

    pub fn clear(&self) {
        *self.0.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }

    pub fn get(&self) -> Option<Arc<RgbaImage>> {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

/// What the teacher did, for the log.
pub enum News {
    /// Something to note.
    Line(String),
    /// The HUD was found: what was learned, and the frame with the boxes
    /// drawn on it (kept in the session folder, to check).
    Found { line: String, picture: RgbaImage },
    /// It could not look or make sense of what it saw.
    Trouble(String),
}

/// How often the HUD is read again, to check the bars and the level.
pub const CHECK_EVERY: Duration = Duration::from_secs(120);

fn outline(picture: &mut RgbaImage, b: &NBox, color: [u8; 3]) {
    let (w, h) = picture.dimensions();
    let (x, y, bw, bh) = b.pixels(w, h);
    for t in 0..2u32 {
        for i in x..(x + bw).min(w) {
            for j in [y + t, (y + bh).saturating_sub(1 + t)] {
                if j < h {
                    picture.put_pixel(i, j, Rgba([color[0], color[1], color[2], 255]));
                }
            }
        }
        for j in y..(y + bh).min(h) {
            for i in [x + t, (x + bw).saturating_sub(1 + t)] {
                if i < w {
                    picture.put_pixel(i, j, Rgba([color[0], color[1], color[2], 255]));
                }
            }
        }
    }
}

/// The frame with what was found outlined.
pub fn annotate(frame: &RgbaImage, layout: &Layout) -> RgbaImage {
    let mut picture = frame.clone();
    for (bar, color) in [
        (&layout.hp, [255, 60, 60]),
        (&layout.mp, [60, 140, 255]),
        (&layout.exp, [240, 220, 40]),
    ] {
        if let Some(bar) = bar {
            outline(&mut picture, &bar.band, color);
        }
    }
    if let Some(level) = &layout.level {
        outline(&mut picture, level, [255, 255, 255]);
    }
    if let Some(minimap) = &layout.minimap {
        outline(&mut picture, minimap, [120, 255, 120]);
    }
    if let Some(status) = &layout.status {
        outline(&mut picture, status, [255, 140, 0]);
    }
    picture
}

/// Tell about trouble once, not on every try.
fn report(last: &mut String, message: String, news: &Sender<News>) {
    if *last != message {
        let _ = news.send(News::Trouble(message.clone()));
        *last = message;
    }
}

/// The HUD's font, labelled by the OCR engine: for each field that would
/// do with an example, its line is read where it is sharp enough, and
/// believed when the number agrees with the bar.
fn label_from_ocr(sight: &Mutex<Sight>, frame: &RgbaImage, news: &Sender<News>) {
    let lock = || sight.lock().unwrap_or_else(|e| e.into_inner());
    let now = Instant::now();
    let (fw, fh) = frame.dimensions();
    // What to read, with the sight unlocked while the engine runs.
    let todo: Vec<(Field, NBox, Option<f32>, crate::vision::Rect)> = {
        let sight = lock();
        let Some(layout) = sight.layout.as_ref().filter(|l| l.fits(fw, fh)) else {
            return;
        };
        [
            (Field::Hp, layout.hp.as_ref()),
            (Field::Mp, layout.mp.as_ref()),
            (Field::Exp, layout.exp.as_ref()),
        ]
        .into_iter()
        .filter_map(|(field, bar)| {
            let bar = bar?;
            sight.numbers.wants_sample(field, now).then(|| {
                (
                    field,
                    bar.band,
                    bar.measure(frame),
                    sight.numbers.label_region(field, &bar.band, fw, fh),
                )
            })
        })
        .collect()
    };
    if todo.is_empty() || !ocr::is_ocr_available() {
        return;
    }
    for (field, band, bar, region) in todo {
        lock().numbers.attempted(field, now);
        if !assess_text_quality(frame, region).is_legible() {
            continue;
        }
        let Some(text) = ocr::ocr_region(frame, region.x, region.y, region.w, region.h) else {
            continue;
        };
        if Numbers::believable(field, &text.text, bar).is_err() {
            continue;
        }
        if let Ok(line) = lock()
            .numbers
            .learn(frame, field, &band, &text.text, "ocr", now)
        {
            let _ = news.send(News::Line(line));
        }
    }
}

/// Start the teacher: with a model, the HUD is found and checked and near
/// misses are confirmed; with or without one, the HUD's font is labelled
/// from the OCR engine.
pub fn spawn(
    eyes: Option<Arc<OpenAi>>,
    sight: Arc<Mutex<Sight>>,
    latest: Arc<Latest>,
    news: Sender<News>,
) {
    let _ = std::thread::Builder::new()
        .name("teacher".into())
        .spawn(move || {
            let lock = || sight.lock().unwrap_or_else(|e| e.into_inner());
            // After a failure, wait longer each time (up to five minutes).
            let mut wait_until = Instant::now();
            let mut backoff = Duration::from_secs(15);
            let mut last_trouble = String::new();
            let mut last_candidate = Instant::now();
            loop {
                std::thread::sleep(Duration::from_secs(1));
                let Some(frame) = latest.get() else {
                    continue;
                };
                label_from_ocr(&sight, &frame, &news);
                let Some(eyes) = &eyes else {
                    continue;
                };
                if Instant::now() < wait_until {
                    continue;
                }
                let want = lock().wants(frame.width(), frame.height(), CHECK_EVERY);
                let status = lock().layout.as_ref().and_then(|l| l.status);
                match (want, status) {
                    (Some(Want::Calibrate), _) | (Some(Want::Verify), None) => {
                        let answer = look(eyes, &teacher::calibrate(&frame));
                        let found = answer.and_then(|a| {
                            teacher::parse_calibration(&a).ok_or_else(|| {
                                format!(
                                    "unreadable answer: {}",
                                    a.chars().take(120).collect::<String>()
                                )
                            })
                        });
                        match found.and_then(|c| lock().calibrated(&frame, &c)) {
                            Ok(line) => {
                                let picture = lock()
                                    .layout
                                    .as_ref()
                                    .map(|l| annotate(&frame, l))
                                    .unwrap_or_else(|| frame.as_ref().clone());
                                let _ = news.send(News::Found { line, picture });
                                backoff = Duration::from_secs(15);
                                last_trouble.clear();
                            }
                            Err(why) => {
                                report(
                                    &mut last_trouble,
                                    format!("couldn't find the HUD: {why}"),
                                    &news,
                                );
                                wait_until = Instant::now() + backoff;
                                backoff = (backoff * 2).min(Duration::from_secs(300));
                            }
                        }
                    }
                    (Some(Want::Verify), Some(status)) => {
                        let answer = look(eyes, &teacher::verify(&frame, &status));
                        match answer.map(|a| teacher::parse_values(&a)) {
                            Ok(Some(values)) => {
                                let line = lock().verified(&frame, &values);
                                let _ = news.send(News::Line(line));
                                backoff = Duration::from_secs(15);
                                last_trouble.clear();
                            }
                            Ok(None) => {
                                lock().looked();
                                report(
                                    &mut last_trouble,
                                    "the HUD reading made no sense".into(),
                                    &news,
                                );
                            }
                            Err(why) => {
                                lock().looked();
                                report(
                                    &mut last_trouble,
                                    format!("couldn't read the HUD: {why}"),
                                    &news,
                                );
                                wait_until = Instant::now() + backoff;
                                backoff = (backoff * 2).min(Duration::from_secs(300));
                            }
                        }
                    }
                    (None, _) => {
                        // A near miss of something taught: is it the same thing?
                        if last_candidate.elapsed() < Duration::from_secs(20) {
                            continue;
                        }
                        let candidate = lock().things.candidates.pop_front();
                        let Some(candidate) = candidate else {
                            continue;
                        };
                        last_candidate = Instant::now();
                        let thing = {
                            let sight = lock();
                            sight
                                .things
                                .list
                                .iter()
                                .find(|t| t.id == candidate.id)
                                .and_then(|t| {
                                    Some((
                                        t.name.clone(),
                                        t.describe.clone(),
                                        t.images.first()?.clone(),
                                    ))
                                })
                        };
                        let Some((name, describe, reference)) = thing else {
                            continue;
                        };
                        let question =
                            teacher::same(&candidate.picture, &reference, &name, &describe);
                        if let Ok(answer) = look(eyes, &question)
                            && teacher::parse_same(&answer) == Some(true)
                            && lock().things.add_picture(&candidate.id, candidate.picture)
                        {
                            let _ = news
                                .send(News::Line(format!("learned another look of \"{name}\"")));
                        }
                    }
                }
            }
        });
}
