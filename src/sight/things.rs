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
//! An object is followed from frame to frame: each one it is seen in, it is
//! looked for around where its track says it will be (a small, cheap
//! search), while a sweep of the whole frame, a stripe a frame, picks up
//! newcomers (`Tracking`). An object also gets better on its own: a near
//! miss that continues a track it was seen on — the same monster in another
//! pose — is taken for a new picture of it; a near miss nobody can account
//! for is kept as a candidate for the vision model to judge.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use image::RgbaImage;
use serde::{Deserialize, Serialize};

use rayon::prelude::*;
use syrup::bars::BarModel;
use syrup::template::{self, Prepared, SetMatch, SetSearch, TemplateSet};
use syrup::threshold::Channel;
use syrup::tracking::ObjectTracker;

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
    /// An object's tracks across frames.
    tracking: Option<Tracking>,
    last_run: Option<Instant>,
    /// Checks in a row the alert's condition held, and whether it may fire.
    streak: u32,
    disarmed: bool,
    /// Since when the condition has been clearly over (the alert arms
    /// again once that has lasted).
    over_since: Option<Instant>,
    /// When the alert's condition was first checked: what holds in the
    /// first moment is how things are, not something happening.
    first_look: Option<Instant>,
    last_fired: Option<Instant>,
    /// How long after firing the alert waits before it may fire again:
    /// [`ALERT_COOLDOWN`] to begin with, doubling each time it fires again
    /// soon after (a level-up effect taught from a few frames of sparkle
    /// "appears" every half minute all night), back to the start once it
    /// has been quiet a long while.
    cooldown: Option<Duration>,
    previous_text: Option<String>,
}

/// A thing's pictures as template sets: scaled to the frame width they
/// were prepared for, mirrored too for an object (a monster faces either
/// way), and rebuilt when a picture is added or the window changes size.
/// `all` holds every picture; `each` one set per picture, for following a
/// track with the picture it was last seen as.
#[derive(Debug, Clone)]
struct Poses {
    frame_width: u32,
    pictures: usize,
    all: TemplateSet,
    each: Vec<TemplateSet>,
}

/// An object followed from frame to frame.
#[derive(Debug, Clone)]
struct Tracking {
    tracker: ObjectTracker,
    /// Per track: which picture it was last seen as, and how well.
    seen: HashMap<u64, LastSeen>,
    /// When a near miss was last taken for a new pose.
    pose_added: Option<Instant>,
}

#[derive(Debug, Clone, Copy)]
struct LastSeen {
    picture: usize,
    score: f32,
    /// Frames in a row it was seen at a confident score.
    confident: u32,
}

impl Tracking {
    /// For an object whose pictures are about `width`×`height`.
    fn new(width: u32, height: u32) -> Tracking {
        Tracking {
            tracker: ObjectTracker::new(
                (width.max(height) as f32 * 0.75 + 8.0).max(12.0),
                TRACK_GRACE,
            ),
            seen: HashMap::new(),
            pose_added: None,
        }
    }

    /// An object known to be at `place` (where it was taught): a track for
    /// it, so the next frame looks there first rather than waiting for the
    /// sweep to come round.
    fn seed(&mut self, place: syrup::Rect) {
        let _ = self.tracker.assign(&[(
            place.x as f32 + place.w as f32 / 2.0,
            place.y as f32 + place.h as f32 / 2.0,
            place.w as f32,
            place.h as f32,
        )]);
    }
}

/// The sweep of the whole frame for newcomers takes this many frames (a
/// little over a second at the companion's frame rate).
const SWEEP_STRIPES: usize = 12;
/// How far, in pixels, a tracked object is looked for around where its
/// track expects it before the window around it is searched.
const TRACK_RADIUS: u32 = 3;
/// A near miss is taken for a new pose when it continues a track seen this
/// confidently for this many frames, and not more often than this.
const POSE_AFTER: u32 = 3;
const POSE_EVERY: Duration = Duration::from_secs(10);
/// A track is kept this many frames without being seen.
const TRACK_GRACE: u32 = 5;

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
    /// How long this alert now waits before firing again, when that has
    /// grown past the usual (it keeps firing): a note for the log.
    pub waits: Option<Duration>,
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
    /// The stripe of the frame the next frame's sweep searches, for every
    /// object alike: one band of the frame prepared once, searched by all.
    sweep: usize,
}

/// How good a match must be, and the band of near misses below it.
const OBJECT_MATCH: f32 = 0.72;
const INDICATOR_MATCH: f32 = 0.78;
const NEAR_MISS: f32 = 0.58;
/// Pictures kept per thing.
const MAX_PICTURES: usize = 8;
/// How often each kind is looked for. Objects are followed on every frame.
const PICTURE_EVERY: Duration = Duration::from_millis(500);
const GAUGE_EVERY: Duration = Duration::from_millis(250);
const TEXT_EVERY: Duration = Duration::from_secs(3);
/// An alert does not repeat sooner than this…
const ALERT_COOLDOWN: Duration = Duration::from_secs(15);
/// …and one that fires again within a few times its wait doubles the wait,
/// up to here; quiet for this long, it starts over.
const ALERT_COOLDOWN_MAX: Duration = Duration::from_secs(600);
/// An alert arms again only once its condition has been over this long:
/// a thing that is always on screen (the character, the minimap) and
/// slips the tracker for a frame or two is not appearing.
const ARM_AFTER: Duration = Duration::from_secs(2);
/// The first moment an alert is checked (MapleSyrup just started, or the
/// thing was just taught) only says how things are: long enough for a
/// sweep of the frame to find what is already there.
const BASELINE: Duration = Duration::from_millis(1500);

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
            sweep: 0,
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
                Self::add_picture_to(&self.dir, &mut thing, picture);
                thing.frame = (fw, fh);
                // It is on screen where the player pointed: a track to
                // start from.
                if teach.kind == Kind::Object {
                    let (x, y, w, h) = teach.place.pixels(fw, fh);
                    thing.live.tracking = Some(Tracking::new(w, h));
                    if let Some(t) = thing.live.tracking.as_mut() {
                        t.seed(syrup::Rect { x, y, w, h });
                    }
                }
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
        // Look right away, so the answer can say what it sees (the track
        // seeded above survives: only the alert's state starts over).
        let tracking = thing.live.tracking.take();
        thing.live = Live::default();
        thing.live.tracking = tracking;
        let reading = Self::read(&mut thing, frame, Instant::now());
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

    fn add_picture_to(dir: &Path, thing: &mut Thing, picture: RgbaImage) {
        if thing.images.len() >= MAX_PICTURES {
            thing.images.remove(1.min(thing.images.len() - 1));
            if thing.pictures.len() > 1 {
                let old = thing.pictures.remove(1);
                let _ = std::fs::remove_file(dir.join(old));
            }
        }
        let _ = std::fs::create_dir_all(dir);
        let mut n = thing.pictures.len() + 1;
        let mut file = format!("{}-{n}.png", thing.id);
        while thing.pictures.contains(&file) {
            n += 1;
            file = format!("{}-{n}.png", thing.id);
        }
        let _ = picture.save(dir.join(&file));
        thing.pictures.push(file);
        thing.images.push(picture);
    }

    /// A near miss the vision model agreed is `id` too.
    pub fn add_picture(&mut self, id: &str, picture: RgbaImage) -> bool {
        let Some(i) = self.find(id) else { return false };
        let mut thing = self.list.remove(i);
        Self::add_picture_to(&self.dir, &mut thing, picture);
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
            let mirrored = thing.kind == Kind::Object;
            let mut all = TemplateSet::new(Channel::Luma, mirrored);
            let mut each = Vec::new();
            for picture in &thing.images {
                let scaled;
                let picture = if scale == 1.0 {
                    picture
                } else {
                    let (w, h) = picture.dimensions();
                    scaled = image::imageops::resize(
                        picture,
                        ((w as f32 * scale).round() as u32).max(4),
                        ((h as f32 * scale).round() as u32).max(4),
                        image::imageops::FilterType::Triangle,
                    );
                    &scaled
                };
                // A flat picture is kept out of both, so the indexes agree.
                if all.add(picture) {
                    let mut one = TemplateSet::new(Channel::Luma, mirrored);
                    one.add(picture);
                    each.push(one);
                }
            }
            thing.live.poses = Some(Poses {
                frame_width: width,
                pictures: thing.images.len(),
                all,
                each,
            });
            // The pictures changed: tracks keep their places, but which
            // picture each was seen as is no longer meaningful.
            if let Some(tracking) = thing.live.tracking.as_mut() {
                tracking.seen.clear();
            }
        }
        thing
            .live
            .poses
            .as_ref()
            .map(|p| &p.all)
            .filter(|s| !s.is_empty())
    }

    /// Follow an object across the frame: each track is looked for around
    /// where it is expected, with the picture it was last seen as (all of
    /// them when that fails), and one stripe of the frame is swept with
    /// the picture it was taught from, for newcomers. Confident matches
    /// feed the tracker; a near miss continuing a confident track becomes
    /// a new picture of the object; other near misses go to `near`.
    fn follow_object(
        thing: &mut Thing,
        prepared: &Prepared<'_>,
        now: Instant,
        stripe: usize,
        near: &mut Vec<RgbaImage>,
        new_pose: &mut Option<RgbaImage>,
    ) -> Reading {
        let frame = prepared.image();
        let (fw, fh) = frame.dimensions();
        let empty = Reading::Seen {
            count: 0,
            places: Vec::new(),
        };
        if Self::poses(thing, fw).is_none() {
            return empty;
        }
        let (tw, th) = thing
            .live
            .poses
            .as_ref()
            .and_then(|p| p.all.templates().next())
            .map(|t| (t.width(), t.height()))
            .unwrap_or((8, 8));
        let tracking = thing
            .live
            .tracking
            .get_or_insert_with(|| Tracking::new(tw, th));
        let poses = thing.live.poses.as_ref().expect("prepared above");
        let near_miss = |limit| SetSearch {
            min_score: NEAR_MISS,
            limit,
            max_colour_shift: Some(COLOUR_SHIFT),
        };
        // Every match this frame: where, how well, as which picture.
        let mut matches: Vec<SetMatch> = Vec::new();
        let add = |m: SetMatch, matches: &mut Vec<SetMatch>| {
            let overlaps = matches.iter().any(|k| {
                2 * k.bounds.x.abs_diff(m.bounds.x) < m.bounds.w.min(k.bounds.w)
                    && 2 * k.bounds.y.abs_diff(m.bounds.y) < m.bounds.h.min(k.bounds.h)
            });
            if !overlaps {
                matches.push(m);
            }
        };

        // 1. Around each track's expected place.
        let tracks_span = tracing::trace_span!("sight.things.object.tracks").entered();
        let predicted: Vec<(u64, f32, f32, f32, f32)> = tracking
            .tracker
            .tracks()
            .iter()
            .map(|t| {
                (
                    t.id,
                    t.position.x + t.velocity.x,
                    t.position.y + t.velocity.y,
                    t.width,
                    t.height,
                )
            })
            .collect();
        for (id, cx, cy, w, h) in predicted {
            let (mx, my) = ((w * 0.35).max(6.0), (h * 0.35).max(6.0));
            let x0 = (cx - w / 2.0 - mx).max(0.0) as u32;
            let y0 = (cy - h / 2.0 - my).max(0.0) as u32;
            let x1 = ((cx + w / 2.0 + mx).max(0.0) as u32).min(fw);
            let y1 = ((cy + h / 2.0 + my).max(0.0) as u32).min(fh);
            if x1 <= x0 || y1 <= y0 {
                continue;
            }
            let window = syrup::Rect {
                x: x0,
                y: y0,
                w: x1 - x0,
                h: y1 - y0,
            };
            let last = tracking.seen.get(&id).copied();
            // Where the track expects it, at full resolution, with the
            // pose it was last seen as and then with every pose: the cheap
            // look that is right most frames. Then the window around it,
            // with every pose. (A set of one picture numbers it 0; it is
            // picture `l.picture`.)
            let own = last.and_then(|l| poses.each.get(l.picture));
            let mut found: Option<(SetMatch, bool)> = None;
            if let Some(one) = own {
                found =
                    template::find_set_near(prepared, one, (cx, cy), TRACK_RADIUS, near_miss(1))
                        .map(|m| (m, true));
            }
            if found.is_none() {
                found = template::find_set_near(
                    prepared,
                    &poses.all,
                    (cx, cy),
                    TRACK_RADIUS,
                    near_miss(1),
                )
                .map(|m| (m, false));
            }
            if found.is_none() {
                found = template::find_set_in(prepared, window, &poses.all, near_miss(1))
                    .into_iter()
                    .next()
                    .map(|m| (m, false));
            }
            if let Some((mut m, from_own)) = found {
                if from_own && let Some(l) = last {
                    m.picture = l.picture;
                }
                add(m, &mut matches);
            }
        }

        drop(tracks_span);

        // 2. One stripe of the frame, with the picture it was taught from.
        //    Each stripe reaches a template's height above its own rows,
        //    so a thing straddling two stripes is whole in the lower one.
        let sweep_span = tracing::trace_span!("sight.things.object.sweep").entered();
        let stripes = SWEEP_STRIPES as u32;
        let stripe = (stripe % SWEEP_STRIPES) as u32;
        let y0 = (fh * stripe / stripes).saturating_sub(th);
        let y1 = (fh * (stripe + 1) / stripes).min(fh);
        if y1 > y0 {
            let band = syrup::Rect {
                x: 0,
                y: y0,
                w: fw,
                h: y1 - y0,
            };
            if let Some(primary) = poses.each.first() {
                for m in template::find_set_in(prepared, band, primary, near_miss(8)) {
                    add(m, &mut matches);
                }
            }
        }
        drop(sweep_span);

        // 3. Confident matches feed the tracker; near misses are judged by
        //    the tracks: continuing a confident one, a new pose; else a
        //    candidate for the model.
        let confident: Vec<&SetMatch> =
            matches.iter().filter(|m| m.score >= OBJECT_MATCH).collect();
        let detections: Vec<(f32, f32, f32, f32)> = confident
            .iter()
            .map(|m| (m.centre.0, m.centre.1, m.bounds.w as f32, m.bounds.h as f32))
            .collect();
        let ids = tracking.tracker.assign(&detections);
        let mut live_ids: Vec<u64> = Vec::new();
        for (m, id) in confident.iter().zip(ids) {
            let Some(id) = id else { continue };
            live_ids.push(id);
            let entry = tracking.seen.entry(id).or_insert(LastSeen {
                picture: m.picture,
                score: m.score,
                confident: 0,
            });
            entry.picture = m.picture;
            entry.score = m.score;
            entry.confident = entry.confident.saturating_add(1);
        }
        let alive: Vec<u64> = tracking.tracker.tracks().iter().map(|t| t.id).collect();
        tracking.seen.retain(|id, _| alive.contains(id));
        for m in matches.iter().filter(|m| m.score < OBJECT_MATCH) {
            let b = m.bounds;
            let crop = || image::imageops::crop_imm(frame, b.x, b.y, b.w, b.h).to_image();
            // The track this near miss sits on, if any: one predicted (not
            // seen this frame) within half a template of it.
            let continues = tracking.tracker.tracks().iter().find(|t| {
                t.is_predicted()
                    && (t.position.x - m.centre.0).abs() < b.w as f32 * 0.5
                    && (t.position.y - m.centre.1).abs() < b.h as f32 * 0.5
            });
            match continues {
                Some(track)
                    if tracking
                        .seen
                        .get(&track.id)
                        .is_some_and(|l| l.confident >= POSE_AFTER)
                        && thing.images.len() < MAX_PICTURES
                        && tracking
                            .pose_added
                            .is_none_or(|t| now.duration_since(t) >= POSE_EVERY) =>
                {
                    tracking.pose_added = Some(now);
                    // Taken for a new pose: the caller adds it, as it owns
                    // the pictures on disk.
                    *new_pose = Some(crop());
                }
                _ => near.push(crop()),
            }
        }
        let mut places: Vec<(f32, f32)> = tracking
            .tracker
            .tracks()
            .iter()
            .filter(|t| !t.is_predicted() && live_ids.contains(&t.id))
            .map(|t| (t.position.x / fw as f32, t.position.y / fh as f32))
            .collect();
        places.sort_by(|a, b| a.0.total_cmp(&b.0));
        Reading::Seen {
            count: places.len(),
            places,
        }
    }

    /// What `thing` shows in `frame`, finding near misses on the way, and
    /// for an object perhaps a new picture of it.
    fn read_with(
        thing: &mut Thing,
        prepared: &Prepared<'_>,
        now: Instant,
        stripe: usize,
        near: &mut Vec<RgbaImage>,
        new_pose: &mut Option<RgbaImage>,
    ) -> Reading {
        let frame = prepared.image();
        let (fw, fh) = frame.dimensions();
        match thing.kind {
            Kind::Object => Self::follow_object(thing, prepared, now, stripe, near, new_pose),
            Kind::Indicator => {
                let region = thing.place.grown(0.5, 0.5).rect(fw, fh);
                let present = Self::poses(thing, fw).is_some_and(|set| {
                    !template::find_set_in(
                        prepared,
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

    fn read(thing: &mut Thing, frame: &RgbaImage, now: Instant) -> Reading {
        let prepared = Prepared::new(frame);
        Self::read_with(thing, &prepared, now, 0, &mut Vec::new(), &mut None)
    }

    /// Look at `frame` for everything learned (each at its own pace).
    /// Returns the alerts that fired.
    pub fn run(&mut self, frame: &RgbaImage, now: Instant) -> Vec<Fired> {
        // The frame prepared once for every search of it this frame, and
        // the one stripe every object sweeps.
        let prepared = Prepared::new(frame);
        let stripe = self.sweep;
        self.sweep = (self.sweep + 1) % SWEEP_STRIPES;
        // Every thing that is due is looked for, each on its own worker
        // thread: the things are independent, and the frame is shared. (On
        // a machine with one worker, right here: a hop to another thread
        // would cost without buying anything.)
        let look = |thing: &mut Thing| -> Option<(Reading, Vec<RgbaImage>, Option<RgbaImage>)> {
            let every = match thing.kind {
                // Followed on every frame, cheaply, around where they are.
                Kind::Object => Duration::ZERO,
                Kind::Indicator => PICTURE_EVERY,
                Kind::Gauge => GAUGE_EVERY,
                Kind::Number | Kind::Text => TEXT_EVERY,
            };
            if !every.is_zero()
                && thing
                    .live
                    .last_run
                    .is_some_and(|t| now.duration_since(t) < every)
            {
                return None;
            }
            let mut near = Vec::new();
            let mut new_pose = None;
            let span = match thing.kind {
                Kind::Object => tracing::trace_span!("sight.things.object"),
                Kind::Indicator => tracing::trace_span!("sight.things.indicator"),
                Kind::Gauge => tracing::trace_span!("sight.things.gauge"),
                Kind::Number | Kind::Text => tracing::trace_span!("sight.things.text"),
            };
            let reading = span.in_scope(|| {
                Self::read_with(thing, &prepared, now, stripe, &mut near, &mut new_pose)
            });
            Some((reading, near, new_pose))
        };
        let pool = crate::util::pool::pool();
        let looked: Vec<(usize, Reading, Vec<RgbaImage>, Option<RgbaImage>)> =
            if pool.current_num_threads() > 1 {
                pool.install(|| {
                    self.list
                        .par_iter_mut()
                        .enumerate()
                        .filter_map(|(i, thing)| look(thing).map(|(r, n, p)| (i, r, n, p)))
                        .collect()
                })
            } else {
                self.list
                    .iter_mut()
                    .enumerate()
                    .filter_map(|(i, thing)| look(thing).map(|(r, n, p)| (i, r, n, p)))
                    .collect()
            };
        let mut fired = Vec::new();
        for (i, reading, near, new_pose) in looked {
            let id = self.list[i].id.clone();
            // A near miss that continued a confident track: another look of
            // the thing, kept without asking.
            if let Some(picture) = new_pose {
                let dir = self.dir.clone();
                Self::add_picture_to(&dir, &mut self.list[i], picture);
                self.save();
            }
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
                    waits: thing.live.cooldown.filter(|c| *c > ALERT_COOLDOWN),
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
        let first = *live.first_look.get_or_insert(now);
        if now.duration_since(first) < BASELINE {
            // The first moment: a condition that already holds is how
            // things are, not something happening.
            if holds {
                live.disarmed = true;
            }
            return None;
        }
        if over {
            live.streak = 0;
            let since = *live.over_since.get_or_insert(now);
            if now.duration_since(since) >= ARM_AFTER {
                live.disarmed = false;
            }
            return None;
        }
        live.over_since = None;
        if !holds {
            live.streak = 0;
            return None;
        }
        live.streak += 1;
        let cooldown = live.cooldown.unwrap_or(ALERT_COOLDOWN);
        let cooled = live
            .last_fired
            .is_none_or(|t| now.duration_since(t) >= cooldown);
        if live.streak >= 2 && !live.disarmed && cooled {
            live.disarmed = true;
            // Firing again soon after the last time: the wait doubles.
            // Quiet for a long while before this: it starts over.
            live.cooldown = Some(match live.last_fired {
                Some(t) if now.duration_since(t) >= ALERT_COOLDOWN_MAX => ALERT_COOLDOWN,
                Some(t) if now.duration_since(t) < cooldown * 4 => {
                    (cooldown * 2).min(ALERT_COOLDOWN_MAX)
                }
                _ => cooldown,
            });
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
        let at = |i: u64| t0 + Duration::from_millis(100 * i);
        // On screen at the first look after starting: how things are, not
        // something happening — no alert.
        let two = field(&[(150, 100), (600, 320)]);
        for i in 0..2 * SWEEP_STRIPES as u64 {
            assert!(again.run(&two, at(i)).is_empty());
        }
        // Gone for a moment (the sweep goes round once, the old tracks
        // die), then back: a thing that slips the tracker for a second is
        // not appearing either.
        for i in 24..24 + SWEEP_STRIPES as u64 {
            assert!(again.run(&field(&[]), at(i)).is_empty());
        }
        for i in 36..36 + 2 * SWEEP_STRIPES as u64 {
            assert!(again.run(&two, at(i)).is_empty());
        }
        // Gone for a while (two seconds), then two appear: within a sweep
        // both are found, and the alert fires once, after two looks.
        for i in 60..85 {
            assert!(again.run(&field(&[]), at(i)).is_empty());
        }
        let mut fired = Vec::new();
        for i in 85..85 + 2 * SWEEP_STRIPES as u64 {
            fired.extend(again.run(&two, at(i)));
        }
        assert_eq!(fired.len(), 1, "{fired:?}");
        assert_eq!(fired[0].say, "An Orange Mushroom!");
        assert!(
            again.describe()[0].starts_with("Orange Mushroom: 2 on screen (left, right)"),
            "{:?}",
            again.describe()
        );
        // One goes: its track is dropped after a few frames; no alert.
        let one = field(&[(150, 100)]);
        for i in 120..120 + 2 * SWEEP_STRIPES as u64 {
            assert!(again.run(&one, at(i)).is_empty());
        }
        assert!(
            again.describe()[0].starts_with("Orange Mushroom: 1 on screen (left)"),
            "{:?}",
            again.describe()
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
    fn a_thing_that_keeps_appearing_is_called_out_less_and_less() {
        let dir = temp_dir("keeps-appearing");
        let mut things = Things::load(&dir);
        let place = NBox::from_pixels(92, 292, 52, 50, 800, 450);
        let alert = Alert {
            when: When::Appears,
            threshold: None,
            say: "Level up!".into(),
        };
        things
            .learn(
                &field(&[(100, 300)]),
                teach("level-up effect", Kind::Object, place, Some(alert)),
            )
            .unwrap();
        // Gone three seconds, there three seconds, for six minutes (a
        // sparkle taught as a level-up), ten frames a second.
        let t0 = Instant::now();
        let (gone, there) = (field(&[]), field(&[(150, 100)]));
        let mut fired_at: Vec<f64> = Vec::new();
        for i in 0..3600u64 {
            let at = t0 + Duration::from_millis(100 * i);
            let frame = if (i / 30) % 2 == 0 { &gone } else { &there };
            for f in things.run(frame, at) {
                fired_at.push(i as f64 * 0.1);
                if let Some(w) = f.waits {
                    assert!(w > ALERT_COOLDOWN && w <= ALERT_COOLDOWN_MAX, "{w:?}");
                }
            }
        }
        // Once every six seconds would be sixty; the wait doubles each
        // time it fires again soon after (up to ten minutes), so the gaps
        // grow: about 12, 30, 60, 120 seconds.
        assert!((4..=8).contains(&fired_at.len()), "{fired_at:?}");
        let gaps: Vec<f64> = fired_at.windows(2).map(|w| w[1] - w[0]).collect();
        assert!(gaps.windows(2).all(|g| g[1] >= g[0]), "{gaps:?}");
        assert!(gaps.last().is_some_and(|g| *g >= 100.0), "{gaps:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The mushroom with its stem bent to one side: the same thing in
    /// another pose, which matches the taught picture only so-so.
    fn bent_mushroom() -> RgbaImage {
        RgbaImage::from_fn(36, 34, |x, y| {
            let (fx, fy) = (x as f32 - 18.0, y as f32 - 12.0);
            if fy < 4.0 && fx * fx / 280.0 + fy * fy / 120.0 <= 1.0 {
                if (x / 6 + y / 5) % 3 == 0 {
                    Rgba([250, 245, 235, 255])
                } else {
                    Rgba([240, 120, 30, 255])
                }
            } else if fy >= 4.0 && (fx + (fy - 4.0) * 0.55).abs() < 6.5 {
                Rgba([235, 220, 190, 255])
            } else {
                Rgba([70, 140, 210, 255])
            }
        })
    }

    #[test]
    fn a_near_miss_on_a_confident_track_becomes_a_new_pose() {
        let dir = temp_dir("pose");
        let mut things = Things::load(&dir);
        let frame = field(&[(300, 200)]);
        let place = NBox::from_pixels(296, 196, 44, 42, 800, 450);
        things
            .learn(&frame, teach("Orange Mushroom", Kind::Object, place, None))
            .unwrap();
        let t0 = Instant::now();
        // Seen confidently for a few frames, standing still.
        for i in 0..4u64 {
            things.run(&frame, t0 + Duration::from_millis(100 * i));
        }
        assert_eq!(things.list[0].images.len(), 1);
        // Then it bends: a near miss where the track expects it.
        let mut bent = field(&[]);
        image::imageops::replace(&mut bent, &bent_mushroom(), 300, 200);
        {
            // Make sure the bent one is a near miss, not a match or a stranger.
            let mut set = TemplateSet::new(Channel::Luma, true);
            set.add(&things.list[0].images[0]);
            let whole = syrup::Rect {
                x: 0,
                y: 0,
                w: 800,
                h: 450,
            };
            let found = template::find_set(
                &bent,
                whole,
                &set,
                SetSearch {
                    min_score: 0.3,
                    limit: 1,
                    max_colour_shift: Some(COLOUR_SHIFT),
                },
            );
            let score = found.first().map(|f| f.score).unwrap_or(0.0);
            assert!(
                (NEAR_MISS..OBJECT_MATCH).contains(&score),
                "the bent mushroom should be a near miss, scored {score}"
            );
        }
        things.run(&bent, t0 + Duration::from_millis(400));
        assert_eq!(things.list[0].images.len(), 2, "the new pose was kept");
        assert!(
            things.candidates.is_empty(),
            "nothing for the model to judge"
        );
        assert_eq!(Things::load(&dir).list[0].images.len(), 2);
        // And from now on the bent one is found outright.
        let mut found = false;
        for i in 5..5 + 2 * SWEEP_STRIPES as u64 {
            things.run(&bent, t0 + Duration::from_millis(100 * i));
            if matches!(
                things.list[0].live.reading,
                Some(Reading::Seen { count: 1, .. })
            ) {
                found = true;
            }
        }
        assert!(found, "{:?}", things.list[0].live.reading);
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
