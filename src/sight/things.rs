//! Things the player taught MapleSyrup to recognise, by talking: "this is
//! an Orange Mushroom", "that's the boss's HP, tell me when it's under
//! 20%", "this is what a rune looks like, tell me when one shows up".
//!
//! Each is a small recogniser made from what was on screen when it was
//! taught, kept on disk, and run on every few frames from then on:
//!
//! - object: a picture found anywhere (a monster, an NPC, an item, a
//!   portal), facing either way; how many and where;
//! - indicator: a picture that lives in one place (a buff icon, a warning);
//!   there or not;
//! - gauge: a bar in one place (a boss's HP, a timer); how full;
//! - number / text: something written in one place (mesos, a counter, a
//!   map name); read with OCR.
//!
//! A thing can carry an alert — appears, disappears, below or above a
//! threshold, changes — and the words to say when it fires.
//!
//! An object also gets better on its own: a near miss (a monster in
//! another pose) is kept as a candidate, and when the vision model agrees
//! it is the same thing, its picture is added.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use image::RgbaImage;
use serde::{Deserialize, Serialize};

use syrup::bars::BarModel;
use syrup::template::{self, SetMatch, SetSearch, TemplateSet};
use syrup::threshold::Channel;

use crate::ai::images::{self, NBox};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Object,
    Indicator,
    Gauge,
    Number,
    Text,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Kind::Object => "object",
            Kind::Indicator => "indicator",
            Kind::Gauge => "gauge",
            Kind::Number => "number",
            Kind::Text => "text",
        }
    }

    pub fn parse(text: &str) -> Option<Kind> {
        Some(match text.trim().to_ascii_lowercase().as_str() {
            "object" => Kind::Object,
            "indicator" | "icon" => Kind::Indicator,
            "gauge" | "bar" => Kind::Gauge,
            "number" => Kind::Number,
            "text" => Kind::Text,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum When {
    Appears,
    Disappears,
    Below,
    Above,
    Changes,
}

impl When {
    pub fn parse(text: &str) -> Option<When> {
        Some(match text.trim().to_ascii_lowercase().as_str() {
            "appears" => When::Appears,
            "disappears" => When::Disappears,
            "below" => When::Below,
            "above" => When::Above,
            "changes" => When::Changes,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Alert {
    pub when: When,
    pub threshold: Option<f32>,
    /// What to say when it fires.
    pub say: String,
}

/// What a thing shows now.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Reading {
    /// An object: how many, and where (centres, as fractions of the frame).
    Seen {
        count: usize,
        places: Vec<(f32, f32)>,
    },
    /// An indicator: there or not.
    Present { present: bool },
    /// A gauge: how full, 0 to 100, when it can be seen.
    Percent { percent: Option<f32> },
    /// A number or text, when it could be read.
    Text { text: Option<String> },
}

impl Reading {
    /// In a few words, for the conversation and the phone.
    pub fn describe(&self) -> String {
        match self {
            Reading::Seen { count: 0, .. } => "not on screen".into(),
            Reading::Seen { count, places } => {
                let sides: Vec<&str> = places
                    .iter()
                    .take(4)
                    .map(|(x, _)| {
                        if *x < 0.33 {
                            "left"
                        } else if *x > 0.66 {
                            "right"
                        } else {
                            "middle"
                        }
                    })
                    .collect();
                format!("{count} on screen ({})", sides.join(", "))
            }
            Reading::Present { present: true } => "showing".into(),
            Reading::Present { present: false } => "not showing".into(),
            Reading::Percent { percent: Some(p) } => format!("about {p:.0}%"),
            Reading::Percent { percent: None } => "can't be seen".into(),
            Reading::Text { text: Some(t) } => format!("reads \"{t}\""),
            Reading::Text { text: None } => "can't be read".into(),
        }
    }

    fn number(&self) -> Option<f32> {
        match self {
            Reading::Percent { percent } => *percent,
            Reading::Text { text: Some(t) } => {
                let digits: String = t
                    .chars()
                    .filter(|c| c.is_ascii_digit() || *c == '.')
                    .collect();
                digits.parse().ok()
            }
            Reading::Seen { count, .. } => Some(*count as f32),
            _ => None,
        }
    }

    fn is_there(&self) -> Option<bool> {
        match self {
            Reading::Seen { count, .. } => Some(*count > 0),
            Reading::Present { present } => Some(*present),
            Reading::Percent { percent } => Some(percent.is_some()),
            Reading::Text { text } => Some(text.is_some()),
        }
    }
}

/// A learned thing as kept on disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Thing {
    pub id: String,
    pub name: String,
    pub kind: Kind,
    pub describe: String,
    /// Where it was taught; where it lives, for all but objects.
    pub place: NBox,
    /// Its pictures' file names, in the learned folder (objects, indicators).
    #[serde(default)]
    pub pictures: Vec<String>,
    #[serde(default)]
    pub gauge: Option<BarModel>,
    #[serde(default)]
    pub alert: Option<Alert>,
    /// The frame size it was taught on (its pictures are that size).
    pub frame: (u32, u32),
    pub taught: String,
    #[serde(skip)]
    pub images: Vec<RgbaImage>,
    #[serde(skip)]
    pub live: Live,
}

/// How a thing is doing, while MapleSyrup runs.
#[derive(Debug, Clone, Default)]
pub struct Live {
    pub reading: Option<Reading>,
    /// Its pictures prepared for matching, at the frame size last seen.
    poses: Option<Poses>,
    last_run: Option<Instant>,
    /// Checks in a row the alert's condition held, and whether it may fire.
    streak: u32,
    disarmed: bool,
    last_fired: Option<Instant>,
    previous_text: Option<String>,
}

/// A thing's pictures as a template set: scaled to the frame width they
/// were prepared for, mirrored too for an object (a monster faces either
/// way), and rebuilt when a picture is added or the window changes size.
#[derive(Debug, Clone)]
struct Poses {
    frame_width: u32,
    pictures: usize,
    set: TemplateSet,
}

/// How far the mean colour of a match may be from the picture's, per
/// channel: correlation on grey levels alone would take a monster of
/// another colour for the one taught.
const COLOUR_SHIFT: f32 = 60.0;

/// What the player said to learn.
#[derive(Debug, Clone)]
pub struct Teach {
    pub name: String,
    pub kind: Kind,
    pub place: NBox,
    pub describe: String,
    pub alert: Option<Alert>,
}

/// An alert that fired.
#[derive(Debug, Clone, PartialEq)]
pub struct Fired {
    pub id: String,
    pub name: String,
    pub say: String,
}

/// A near miss to show the vision model: is this the same thing?
#[derive(Debug, Clone)]
pub struct Candidate {
    pub id: String,
    pub picture: RgbaImage,
}

pub struct Things {
    pub list: Vec<Thing>,
    dir: PathBuf,
    /// Near misses waiting to be checked (a few at a time).
    pub candidates: VecDeque<Candidate>,
}

/// How good a match must be, and the band of near misses below it.
const OBJECT_MATCH: f32 = 0.72;
const INDICATOR_MATCH: f32 = 0.78;
const NEAR_MISS: f32 = 0.58;
/// Pictures kept per thing.
const MAX_PICTURES: usize = 8;
/// How often each kind is looked for.
const PICTURE_EVERY: Duration = Duration::from_millis(500);
const GAUGE_EVERY: Duration = Duration::from_millis(250);
const TEXT_EVERY: Duration = Duration::from_secs(3);
/// An alert does not repeat sooner than this.
const ALERT_COOLDOWN: Duration = Duration::from_secs(15);

fn slug(name: &str) -> String {
    let s: String = name
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect();
    let s = s.trim_matches('-').to_string();
    let mut out = String::new();
    for part in s.split('-').filter(|p| !p.is_empty()) {
        if !out.is_empty() {
            out.push('-');
        }
        out.push_str(part);
    }
    if out.is_empty() {
        "thing".into()
    } else {
        out.chars().take(32).collect()
    }
}

/// The part of a crop that stands out from its edges (the monster, not the
/// sky and the grass around it), if that is a fair part of it.
fn foreground(crop: &RgbaImage) -> RgbaImage {
    match template::foreground(crop) {
        Some(r) => image::imageops::crop_imm(crop, r.x, r.y, r.w, r.h).to_image(),
        None => crop.clone(),
    }
}

fn today() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M").to_string()
}

impl Things {
    pub fn load(dir: &Path) -> Things {
        let mut list: Vec<Thing> = std::fs::read_to_string(dir.join("things.json"))
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        for thing in &mut list {
            thing.images = thing
                .pictures
                .iter()
                .filter_map(|p| image::open(dir.join(p)).ok().map(|i| i.to_rgba8()))
                .collect();
        }
        Things {
            list,
            dir: dir.to_path_buf(),
            candidates: VecDeque::new(),
        }
    }

    pub fn save(&self) {
        let _ = std::fs::create_dir_all(&self.dir);
        if let Ok(text) = serde_json::to_string_pretty(&self.list) {
            let _ = std::fs::write(self.dir.join("things.json"), text);
        }
    }

    pub fn find(&self, name_or_id: &str) -> Option<usize> {
        let key = name_or_id.trim().to_lowercase();
        self.list
            .iter()
            .position(|t| t.id == key || t.name.to_lowercase() == key)
            .or_else(|| self.list.iter().position(|t| t.id == slug(&key)))
    }

    /// Learn `teach` from `frame`. Teaching a name again adds to it (a
    /// monster in another pose) or replaces it (a gauge, a text).
    pub fn learn(&mut self, frame: &RgbaImage, teach: Teach) -> Result<String, String> {
        let (fw, fh) = frame.dimensions();
        let (_, _, pw, ph) = teach.place.pixels(fw, fh);
        if pw < 4 || ph < 3 {
            return Err("that box is too small to learn from".into());
        }
        let existing = self.find(&teach.name);
        let mut thing = match existing {
            Some(i) if self.list[i].kind == teach.kind => self.list.remove(i),
            Some(i) => {
                self.forget_at(i);
                self.blank(&teach, frame)
            }
            None => self.blank(&teach, frame),
        };
        thing.describe = teach.describe.clone();
        if teach.alert.is_some() {
            thing.alert = teach.alert.clone();
        }
        thing.place = teach.place;
        let summary = match teach.kind {
            Kind::Object | Kind::Indicator => {
                let crop = images::crop(frame, &teach.place);
                let picture = if teach.kind == Kind::Object {
                    foreground(&crop)
                } else {
                    crop
                };
                if picture.width() < 6 || picture.height() < 6 {
                    return Err("that is too small to recognise".into());
                }
                self.add_picture_to(&mut thing, picture);
                thing.frame = (fw, fh);
                format!("{} picture(s)", thing.images.len())
            }
            Kind::Gauge => {
                let model = BarModel::learn(frame, &teach.place, None)
                    .ok_or("there is no coloured bar in that box")?;
                thing.place = model.band;
                thing.gauge = Some(model);
                thing.frame = (fw, fh);
                "a bar".to_string()
            }
            Kind::Number | Kind::Text => {
                thing.frame = (fw, fh);
                "a place to read".to_string()
            }
        };
        // Look right away, so the answer can say what it sees.
        thing.live = Live::default();
        let reading = Self::read(&mut thing, frame);
        thing.live.reading = Some(reading.clone());
        // "Tell me when it shows up", taught while it is on screen: the
        // next time it shows up, not now.
        if let Some(alert) = &thing.alert {
            let there = reading.is_there();
            thing.live.disarmed = match alert.when {
                When::Appears => there == Some(true),
                When::Disappears => there == Some(false),
                _ => false,
            };
        }
        let line = format!(
            "learned \"{}\" ({}, {summary}); now: {}",
            thing.name,
            thing.kind.label(),
            reading.describe()
        );
        self.list.push(thing);
        self.save();
        Ok(line)
    }

    fn blank(&self, teach: &Teach, frame: &RgbaImage) -> Thing {
        let mut id = slug(&teach.name);
        let base = id.clone();
        let mut n = 2;
        while self.list.iter().any(|t| t.id == id) {
            id = format!("{base}-{n}");
            n += 1;
        }
        Thing {
            id,
            name: teach.name.trim().to_string(),
            kind: teach.kind,
            describe: teach.describe.clone(),
            place: teach.place,
            pictures: Vec::new(),
            gauge: None,
            alert: teach.alert.clone(),
            frame: frame.dimensions(),
            taught: today(),
            images: Vec::new(),
            live: Live::default(),
        }
    }

    fn add_picture_to(&self, thing: &mut Thing, picture: RgbaImage) {
        if thing.images.len() >= MAX_PICTURES {
            thing.images.remove(1.min(thing.images.len() - 1));
            if thing.pictures.len() > 1 {
                let old = thing.pictures.remove(1);
                let _ = std::fs::remove_file(self.dir.join(old));
            }
        }
        let _ = std::fs::create_dir_all(&self.dir);
        let mut n = thing.pictures.len() + 1;
        let mut file = format!("{}-{n}.png", thing.id);
        while thing.pictures.contains(&file) {
            n += 1;
            file = format!("{}-{n}.png", thing.id);
        }
        let _ = picture.save(self.dir.join(&file));
        thing.pictures.push(file);
        thing.images.push(picture);
    }

    /// A near miss the vision model agreed is `id` too.
    pub fn add_picture(&mut self, id: &str, picture: RgbaImage) -> bool {
        let Some(i) = self.find(id) else { return false };
        let mut thing = self.list.remove(i);
        self.add_picture_to(&mut thing, picture);
        self.list.insert(i, thing);
        self.save();
        true
    }

    fn forget_at(&mut self, i: usize) -> Thing {
        let thing = self.list.remove(i);
        for p in &thing.pictures {
            let _ = std::fs::remove_file(self.dir.join(p));
        }
        self.candidates.retain(|c| c.id != thing.id);
        thing
    }

    pub fn forget(&mut self, name_or_id: &str) -> Option<String> {
        let i = self.find(name_or_id)?;
        let thing = self.forget_at(i);
        self.save();
        Some(thing.name)
    }

    /// The thing's pictures ready to match in a frame `width` wide: a thing
    /// taught at another window size has them scaled to this one.
    fn poses(thing: &mut Thing, width: u32) -> Option<&TemplateSet> {
        let fresh = thing
            .live
            .poses
            .as_ref()
            .is_some_and(|p| p.frame_width == width && p.pictures == thing.images.len());
        if !fresh {
            let (tw, _) = thing.frame;
            let scale = if tw == 0 || tw == width {
                1.0
            } else {
                width as f32 / tw as f32
            };
            let mut set = TemplateSet::new(Channel::Luma, thing.kind == Kind::Object);
            for picture in &thing.images {
                if scale == 1.0 {
                    set.add(picture);
                } else {
                    let (w, h) = picture.dimensions();
                    let scaled = image::imageops::resize(
                        picture,
                        ((w as f32 * scale).round() as u32).max(4),
                        ((h as f32 * scale).round() as u32).max(4),
                        image::imageops::FilterType::Triangle,
                    );
                    set.add(&scaled);
                }
            }
            thing.live.poses = Some(Poses {
                frame_width: width,
                pictures: thing.images.len(),
                set,
            });
        }
        thing
            .live
            .poses
            .as_ref()
            .map(|p| &p.set)
            .filter(|s| !s.is_empty())
    }

    /// What `thing` shows in `frame`, finding near misses on the way.
    fn read_with(thing: &mut Thing, frame: &RgbaImage, near: &mut Vec<RgbaImage>) -> Reading {
        let (fw, fh) = frame.dimensions();
        let whole = syrup::Rect {
            x: 0,
            y: 0,
            w: fw,
            h: fh,
        };
        match thing.kind {
            Kind::Object => {
                let kept: Vec<SetMatch> = match Self::poses(thing, fw) {
                    Some(set) => template::find_set(
                        frame,
                        whole,
                        set,
                        SetSearch {
                            min_score: NEAR_MISS,
                            limit: 24,
                            max_colour_shift: Some(COLOUR_SHIFT),
                        },
                    ),
                    None => Vec::new(),
                };
                for f in kept.iter().filter(|f| f.score < OBJECT_MATCH).take(2) {
                    let b = f.bounds;
                    near.push(image::imageops::crop_imm(frame, b.x, b.y, b.w, b.h).to_image());
                }
                let mut places: Vec<(f32, f32)> = kept
                    .iter()
                    .filter(|f| f.score >= OBJECT_MATCH)
                    .map(|f| {
                        (
                            (f.bounds.x + f.bounds.w / 2) as f32 / fw as f32,
                            (f.bounds.y + f.bounds.h / 2) as f32 / fh as f32,
                        )
                    })
                    .collect();
                places.sort_by(|a, b| a.0.total_cmp(&b.0));
                Reading::Seen {
                    count: places.len(),
                    places,
                }
            }
            Kind::Indicator => {
                let region = thing.place.grown(0.5, 0.5).rect(fw, fh);
                let present = Self::poses(thing, fw).is_some_and(|set| {
                    !template::find_set(
                        frame,
                        region,
                        set,
                        SetSearch {
                            min_score: INDICATOR_MATCH,
                            limit: 1,
                            max_colour_shift: Some(COLOUR_SHIFT),
                        },
                    )
                    .is_empty()
                });
                Reading::Present { present }
            }
            Kind::Gauge => Reading::Percent {
                percent: thing.gauge.as_ref().and_then(|g| g.measure(frame)),
            },
            Kind::Number | Kind::Text => {
                let (x, y, w, h) = thing.place.pixels(fw, fh);
                let text = crate::vision::ocr::ocr_region(frame, x, y, w, h)
                    .map(|r| r.text.trim().to_string())
                    .filter(|t| !t.is_empty())
                    .map(|t| {
                        if thing.kind == Kind::Number {
                            t.chars()
                                .filter(|c| {
                                    c.is_ascii_digit() || matches!(c, ',' | '.' | '%' | '/')
                                })
                                .collect()
                        } else {
                            t
                        }
                    })
                    .filter(|t: &String| !t.is_empty());
                Reading::Text { text }
            }
        }
    }

    fn read(thing: &mut Thing, frame: &RgbaImage) -> Reading {
        Self::read_with(thing, frame, &mut Vec::new())
    }

    /// Look at `frame` for everything learned (each at its own pace).
    /// Returns the alerts that fired.
    pub fn run(&mut self, frame: &RgbaImage, now: Instant) -> Vec<Fired> {
        let mut fired = Vec::new();
        for i in 0..self.list.len() {
            let every = match self.list[i].kind {
                Kind::Object | Kind::Indicator => PICTURE_EVERY,
                Kind::Gauge => GAUGE_EVERY,
                Kind::Number | Kind::Text => TEXT_EVERY,
            };
            if self.list[i]
                .live
                .last_run
                .is_some_and(|t| now.duration_since(t) < every)
            {
                continue;
            }
            let mut near = Vec::new();
            let span = match self.list[i].kind {
                Kind::Object => tracing::trace_span!("sight.things.object"),
                Kind::Indicator => tracing::trace_span!("sight.things.indicator"),
                Kind::Gauge => tracing::trace_span!("sight.things.gauge"),
                Kind::Number | Kind::Text => tracing::trace_span!("sight.things.text"),
            };
            let reading = span.in_scope(|| Self::read_with(&mut self.list[i], frame, &mut near));
            let id = self.list[i].id.clone();
            // A few near misses to check, not a flood.
            if self.candidates.len() < 4 && self.list[i].images.len() < MAX_PICTURES {
                for picture in near.into_iter().take(1) {
                    if self.candidates.iter().filter(|c| c.id == id).count() < 2 {
                        self.candidates.push_back(Candidate {
                            id: id.clone(),
                            picture,
                        });
                    }
                }
            }
            let thing = &mut self.list[i];
            thing.live.last_run = Some(now);
            if let Some(say) = Self::check_alert(thing, &reading, now) {
                fired.push(Fired {
                    id: thing.id.clone(),
                    name: thing.name.clone(),
                    say,
                });
            }
            thing.live.reading = Some(reading);
        }
        fired
    }

    /// Whether the thing's alert fires with this reading.
    fn check_alert(thing: &mut Thing, reading: &Reading, now: Instant) -> Option<String> {
        let alert = thing.alert.clone()?;
        let live = &mut thing.live;
        let threshold = alert.threshold.unwrap_or(20.0);
        // Whether the condition holds now, and whether it is clearly over
        // (which arms the alert again).
        let (holds, over) = match alert.when {
            When::Appears => {
                let there = reading.is_there().unwrap_or(false);
                (there, !there)
            }
            When::Disappears => {
                let there = reading.is_there().unwrap_or(true);
                (!there, there)
            }
            When::Below => match reading.number() {
                Some(v) => (v < threshold, v > threshold + 5.0),
                None => (false, false),
            },
            When::Above => match reading.number() {
                Some(v) => (v > threshold, v < threshold - 5.0),
                None => (false, false),
            },
            When::Changes => {
                let text = match reading {
                    Reading::Text { text } => text.clone(),
                    other => Some(other.describe()),
                };
                let changed = match (&live.previous_text, &text) {
                    (Some(a), Some(b)) => a != b,
                    _ => false,
                };
                if text.is_some() && !changed {
                    live.previous_text = text.clone();
                }
                (changed, !changed)
            }
        };
        if over {
            live.streak = 0;
            live.disarmed = false;
            return None;
        }
        if !holds {
            live.streak = 0;
            return None;
        }
        live.streak += 1;
        let cooled = live
            .last_fired
            .is_none_or(|t| now.duration_since(t) >= ALERT_COOLDOWN);
        if live.streak >= 2 && !live.disarmed && cooled {
            live.disarmed = true;
            live.last_fired = Some(now);
            if alert.when == When::Changes
                && let Reading::Text { text } = reading
            {
                live.previous_text = text.clone();
            }
            return Some(alert.say.clone());
        }
        None
    }

    /// Each thing and what it shows, for the conversation.
    pub fn describe(&self) -> Vec<String> {
        self.list
            .iter()
            .map(|t| {
                let now = t
                    .live
                    .reading
                    .as_ref()
                    .map(|r| r.describe())
                    .unwrap_or_else(|| "not looked for yet".into());
                let alert = t
                    .alert
                    .as_ref()
                    .map(|a| match (a.when, a.threshold) {
                        (When::Below | When::Above, Some(v)) => {
                            format!(" (you warn when {:?} {v})", a.when).to_lowercase()
                        }
                        (w, _) => format!(" (you speak up when it {w:?})").to_lowercase(),
                    })
                    .unwrap_or_default();
                format!("{}: {now}{alert}", t.name)
            })
            .collect()
    }

    /// The first picture of each, as PNG, for the phone.
    pub fn thumbnail(&self, id: &str) -> Option<Vec<u8>> {
        let thing = self.list.iter().find(|t| t.id == id)?;
        let picture = thing.images.first()?;
        let picture = images::fit(picture, 96, 96);
        let mut bytes = Vec::new();
        image::DynamicImage::ImageRgba8(picture)
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            )
            .ok()?;
        Some(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ms-things-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    /// A mushroom: an orange cap with white spots on a pale stem.
    fn mushroom() -> RgbaImage {
        RgbaImage::from_fn(36, 34, |x, y| {
            let (fx, fy) = (x as f32 - 18.0, y as f32 - 12.0);
            if fy < 4.0 && fx * fx / 280.0 + fy * fy / 120.0 <= 1.0 {
                if (x / 6 + y / 5) % 3 == 0 {
                    Rgba([250, 245, 235, 255])
                } else {
                    Rgba([240, 120, 30, 255])
                }
            } else if fy >= 4.0 && fx.abs() < 8.0 {
                Rgba([235, 220, 190, 255])
            } else {
                Rgba([70, 140, 210, 255])
            }
        })
    }

    fn field(mushrooms: &[(u32, u32)]) -> RgbaImage {
        let mut f = RgbaImage::from_fn(800, 450, |x, y| {
            let v = ((x / 40 + y / 25) % 2) as u8 * 10;
            Rgba([70 + v, 140 + v, 210, 255])
        });
        for &(x, y) in mushrooms {
            image::imageops::replace(&mut f, &mushroom(), x as i64, y as i64);
        }
        f
    }

    fn teach(name: &str, kind: Kind, place: NBox, alert: Option<Alert>) -> Teach {
        Teach {
            name: name.into(),
            kind,
            place,
            describe: "orange cap with white spots".into(),
            alert,
        }
    }

    #[test]
    fn an_object_is_taught_once_and_found_again_and_alerts() {
        let dir = temp_dir("object");
        let mut things = Things::load(&dir);
        let frame = field(&[(100, 300)]);
        // The model's box: a little loose around it.
        let place = NBox::from_pixels(92, 292, 52, 50, 800, 450);
        let alert = Alert {
            when: When::Appears,
            threshold: None,
            say: "An Orange Mushroom!".into(),
        };
        let line = things
            .learn(
                &frame,
                teach("Orange Mushroom", Kind::Object, place, Some(alert)),
            )
            .unwrap();
        assert!(line.contains("1 on screen"), "{line}");
        // It is kept on disk, picture and all.
        let mut again = Things::load(&dir);
        assert_eq!(again.list.len(), 1);
        assert_eq!(again.list[0].images.len(), 1);
        let t0 = Instant::now();
        // None for a while, then two: the alert fires once, after two looks.
        assert!(again.run(&field(&[]), t0).is_empty());
        assert!(
            again
                .run(
                    &field(&[(150, 100), (600, 320)]),
                    t0 + Duration::from_secs(1)
                )
                .is_empty()
        );
        let fired = again.run(
            &field(&[(150, 100), (600, 320)]),
            t0 + Duration::from_secs(2),
        );
        assert_eq!(fired.len(), 1);
        assert_eq!(fired[0].say, "An Orange Mushroom!");
        assert!(
            again.describe()[0].starts_with("Orange Mushroom: 2 on screen (left, right)"),
            "{:?}",
            again.describe()
        );
        assert!(
            again
                .run(&field(&[(150, 100)]), t0 + Duration::from_secs(3))
                .is_empty()
        );
        assert!(again.thumbnail("orange-mushroom").is_some());
        assert_eq!(
            again.forget("orange mushroom").as_deref(),
            Some("Orange Mushroom")
        );
        assert!(Things::load(&dir).list.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_gauge_warns_below_its_threshold_once() {
        let dir = temp_dir("gauge");
        let mut things = Things::load(&dir);
        let boss = |fill: f32| {
            let mut f = RgbaImage::from_pixel(800, 450, Rgba([20, 20, 30, 255]));
            let end = 200 + (400.0 * fill / 100.0) as u32;
            for y in 30..40 {
                for x in 200..600 {
                    let p = if x < end {
                        Rgba([200, 30, 160, 255])
                    } else {
                        Rgba([50, 50, 50, 255])
                    };
                    f.put_pixel(x, y, p);
                }
            }
            f
        };
        let place = NBox::from_pixels(195, 26, 410, 18, 800, 450);
        let alert = Alert {
            when: When::Below,
            threshold: Some(20.0),
            say: "Boss under 20%!".into(),
        };
        let line = things
            .learn(
                &boss(100.0),
                teach("boss HP", Kind::Gauge, place, Some(alert)),
            )
            .unwrap();
        assert!(line.contains("about 100%"), "{line}");
        let t0 = Instant::now();
        let mut fired = Vec::new();
        for (i, fill) in [60.0, 30.0, 18.0, 15.0, 12.0, 10.0].into_iter().enumerate() {
            fired.extend(things.run(&boss(fill), t0 + Duration::from_secs(i as u64)));
        }
        assert_eq!(
            fired.iter().map(|f| f.say.as_str()).collect::<Vec<_>>(),
            ["Boss under 20%!"]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn foreground_trims_the_background() {
        let mut crop = RgbaImage::from_pixel(60, 60, Rgba([70, 140, 210, 255]));
        image::imageops::replace(&mut crop, &mushroom(), 12, 14);
        let fg = foreground(&crop);
        assert!(
            fg.width() <= 40 && fg.height() <= 38,
            "{:?}",
            fg.dimensions()
        );
        // Sky above, grass below: both are background.
        let mut crop = RgbaImage::from_fn(80, 64, |_, y| {
            if y < 44 {
                Rgba([110, 170, 230, 255])
            } else {
                Rgba([90, 160, 70, 255])
            }
        });
        image::imageops::replace(&mut crop, &mushroom(), 22, 12);
        let fg = foreground(&crop);
        assert!(
            fg.width() <= 40 && fg.height() <= 38,
            "{:?}",
            fg.dimensions()
        );
        assert_eq!(slug("Orange Mushroom!"), "orange-mushroom");
        assert_eq!(slug("רונה"), "רונה");
    }
}
