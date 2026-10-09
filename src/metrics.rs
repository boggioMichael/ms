//! The session's stats: what each session came to, in numbers, kept on
//! this PC for MapleSyrup's own product metrics — and, only when the player
//! turns it on, the same numbers made ready to share with partners.
//!
//! Two things, kept apart:
//!
//! - **Stats** ([`SessionStats`]): one record per session in
//!   `metrics/sessions.jsonl` under the settings folder
//!   (`%APPDATA%\MapleSyrup`), the newest last, at most [`KEEP_RECORDS`].
//!   The session under way is `metrics/current.json`, rewritten every
//!   [`SNAPSHOT_EVERY`] seconds (a file of a kilobyte, written whole and
//!   then put in place): it joins the others when the session ends — or,
//!   after a crash or a console window closed with the X, when MapleSyrup
//!   next starts — so no session is lost, and at most a minute of one.
//!   Stats never leave the PC.
//! - **Sharing** ([`Consent`], [`Export`]): off unless the player turns it
//!   on (Settings on the phone), apart from the stats. Turned on, it gets a
//!   random install id (a UUID v4 from the operating system's randomness,
//!   through `ring`, already a dependency), and `metrics/share-export.json`
//!   is rebuilt then and at the end of every session from the records
//!   since the day it was turned on, coarsened: the week, not the day;
//!   level bands, not levels; how many maps, not which; latencies rounded;
//!   no commit, no session id. Turning it off, or "Delete it", deletes the
//!   export and the id.
//!
//! **Nothing is sent anywhere.** There is no endpoint: where an export
//! would go, and to whom, is the owner's decision, with a lawyer (see
//! `docs/data-and-metrics.md`).
//! TODO(upload): send `share-export.json` — only while sharing is on, only
//! once there is an endpoint, a privacy policy and terms that say so, and
//! only what [`EXPORT_FIELDS`] allows.
//!
//! Data minimisation by construction: a record holds aggregates and game
//! facts only — minutes, counts, levels, the class, the maps' names (on
//! this PC only), the attitude, the language, the version, the screen's
//! size class — never the character's name, the player's words or voice,
//! a transcript, a picture, the notebook, what was taught, a path, the
//! PC's name, a key, an address or the browser. The hooks the main loop
//! calls take no text of the player's; the words the game shows (the
//! class, a map's name) are kept only when they look like the game's — no
//! slashes, no `@`, not too long — and do not contain the character's
//! name. [`RECORD_FIELDS`] and [`EXPORT_FIELDS`] are the allow-lists the
//! tests hold every record and every export to.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use chrono::{Datelike, NaiveDate};
use serde::{Deserialize, Serialize};

use crate::ai::AiError;
use crate::coach::{CloseCalls, Coach};
use crate::companion::{Companion, Observation, Tally};
use crate::sight::Sight;

/// Records kept in `sessions.jsonl`: a year of a session a day. The
/// oldest go first.
pub const KEEP_RECORDS: usize = 365;
/// How often the session under way is written (`current.json`), in
/// seconds: a closed console window or a crash loses at most this much.
pub const SNAPSHOT_EVERY: f64 = 60.0;
/// The sessions the phone's table shows.
pub const SHOWN_SESSIONS: usize = 10;
/// The sessions a preview of the export shows, while sharing is off.
pub const PREVIEW_SESSIONS: usize = 3;
/// The export's format, for whoever reads it.
pub const EXPORT_FORMAT: u32 = 1;
/// Maps kept per session, at most (a session of portals is still one line).
const MAX_MAPS: usize = 100;
/// A class or a map's name longer than this is not one.
const MAX_GAME_TEXT: usize = 48;
/// How often the sight is looked at for the map and the HUD's style.
const LOOK_EVERY: Duration = Duration::from_secs(2);
/// How often the companion's progress (levels gained, EXP per hour) is
/// read, in seconds of the session.
const PROGRESS_EVERY: f64 = 5.0;
/// Reply latencies kept for the median, at most (the last ones).
const MAX_LATENCIES: usize = 10_000;

/// Every field a session's record may have, nested ones too (`maps` is
/// keyed by the maps' names: data, not fields). A field not here is never
/// written: the tests hold every record to this list.
pub const RECORD_FIELDS: &[&str] = &[
    "session",
    "day",
    "minutes",
    "game_minutes",
    "levels_gained",
    "level_start",
    "level_end",
    "level_start_band",
    "level_end_band",
    "job",
    "hud",
    "deaths",
    "warnings",
    "hp_low",
    "mp_low",
    "beating",
    "taught",
    "close_calls",
    "potions_answered",
    "exp_per_hour",
    "maps",
    "sentences",
    "replies",
    "reply_ms_median",
    "instant_answers",
    "call_minutes",
    "clip_minutes",
    "coach_looks",
    "coach_lines",
    "attitude",
    "language",
    "version",
    "commit",
    "windows",
    "screen",
    "ai_errors",
    "voice_errors",
];

/// Every field the export may have, nested ones too: what could ever be
/// shared. The tests hold every export to this list.
pub const EXPORT_FIELDS: &[&str] = &[
    "format",
    "install_id",
    "app_version",
    "sessions",
    "week",
    "minutes",
    "game_minutes",
    "levels_gained",
    "level_start_band",
    "level_end_band",
    "job",
    "hud",
    "deaths",
    "warnings",
    "hp_low",
    "mp_low",
    "beating",
    "taught",
    "close_calls",
    "potions_answered",
    "exp_per_hour",
    "maps",
    "map_visits",
    "sentences",
    "replies",
    "reply_ms_median",
    "instant_answers",
    "call_minutes",
    "clip_minutes",
    "coach_looks",
    "coach_lines",
    "attitude",
    "language",
    "version",
    "windows",
    "screen",
    "ai_errors",
    "voice_errors",
];

/// The warnings said in a session, by what they were about: HP or MP low,
/// a beating (HP falling fast), a thing the player taught past the mark
/// they set.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Warnings {
    pub hp_low: u32,
    pub mp_low: u32,
    pub beating: u32,
    pub taught: u32,
}

/// What one session came to: aggregates and game facts, nothing else (see
/// the module's notes, and [`RECORD_FIELDS`]).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SessionStats {
    /// A random id for the session, so that its snapshot is replaced, not
    /// counted twice (on this PC only: never in the export).
    pub session: String,
    /// The day it started, on this PC's calendar ("2026-10-09"; no time).
    pub day: String,
    /// Minutes MapleSyrup ran, and minutes the game was seen.
    pub minutes: u32,
    pub game_minutes: u32,
    pub levels_gained: u32,
    /// The level at the start and at the end, and their bands
    /// ([`level_band`]).
    pub level_start: Option<u32>,
    pub level_end: Option<u32>,
    pub level_start_band: Option<String>,
    pub level_end_band: Option<String>,
    /// The class ("Night Lord"), as the game shows it.
    pub job: Option<String>,
    /// The HUD the numbers were read on: "modern" or "classic".
    pub hud: Option<String>,
    pub deaths: u32,
    pub warnings: Warnings,
    /// HP under a tenth and back above 40% within 20 s, no death between.
    pub close_calls: u32,
    /// Of the low-HP and low-MP lines, the share a potion answered (0 to
    /// 1; None: there was none).
    pub potions_answered: Option<f32>,
    /// EXP per hour as last measured, in percent of a level.
    pub exp_per_hour: Option<f32>,
    /// The maps seen, by the game's name for them, and how many times each
    /// was come to (on this PC only: the export has counts).
    pub maps: BTreeMap<String, u32>,
    /// Sentences the player said (how many: never what).
    pub sentences: u32,
    /// Replies heard, and the median time to their first words, in
    /// milliseconds (off a call: from the sentence to the reply's first
    /// words; on a call: from the player stopping to its first sound).
    pub replies: u32,
    pub reply_ms_median: Option<u32>,
    /// Answers given at once, without a model (their own numbers, a hello).
    pub instant_answers: u32,
    /// Minutes on a live call, and minutes of the phone connected without
    /// one (its replies as clips).
    pub call_minutes: u32,
    pub clip_minutes: u32,
    /// The coach's looks at the game, and the lines it said.
    pub coach_looks: u32,
    pub coach_lines: u32,
    /// How it talked: "friendly", "blunt" or "savage".
    pub attitude: String,
    /// The phone's language ("he"; no region).
    pub language: Option<String>,
    pub version: String,
    /// The build's commit (on this PC only: never in the export).
    pub commit: String,
    /// "10" or "11".
    pub windows: Option<String>,
    /// The screen's size class ("1080p", "1440p", "4K"…).
    pub screen: Option<String>,
    /// Model errors (a reply or a look that failed), and voice errors (a
    /// line that could not be spoken).
    pub ai_errors: u32,
    pub voice_errors: u32,
}

/// Whether the player shares, since when, and the install id made when
/// they turned it on (`metrics/share.json`). Off unless turned on.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Consent {
    #[serde(default)]
    pub on: bool,
    /// The day it was turned on ("2026-10-09"): only sessions from then on
    /// are in the export.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since: Option<String>,
    /// The random install id (a UUID v4), made when it was turned on and
    /// deleted when it is turned off.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
}

/// What the phone's card shows: whether the player shares (and since
/// when), and what is shared — or, while it is off, a preview of what
/// would be (`preview`), with no id. Serialized as it is, so that the
/// export keeps its fields' order on the phone.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ShareView {
    pub on: bool,
    pub since: Option<String>,
    pub preview: bool,
    pub export: Option<Export>,
}

/// What the phone's Details table shows: the last sessions, the newest
/// first, and whether the player shares (without the id).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StatsView {
    pub sessions: Vec<SessionStats>,
    pub share: Consent,
}

/// What would be shared: `metrics/share-export.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Export {
    pub format: u32,
    /// The install id (None in a preview, while sharing is off).
    pub install_id: Option<String>,
    pub app_version: String,
    pub sessions: Vec<SharedSession>,
}

/// One session as it would be shared: a [`SessionStats`] coarsened (see
/// [`EXPORT_FIELDS`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SharedSession {
    /// The ISO week it started in ("2026-W41").
    pub week: String,
    pub minutes: u32,
    pub game_minutes: u32,
    pub levels_gained: u32,
    pub level_start_band: Option<String>,
    pub level_end_band: Option<String>,
    pub job: Option<String>,
    pub hud: Option<String>,
    pub deaths: u32,
    pub warnings: Warnings,
    pub close_calls: u32,
    /// To a tenth.
    pub potions_answered: Option<f32>,
    /// To a tenth of a percent.
    pub exp_per_hour: Option<f32>,
    /// How many maps, and how many times one was come to: not which.
    pub maps: u32,
    pub map_visits: u32,
    pub sentences: u32,
    pub replies: u32,
    /// To the tenth of a second.
    pub reply_ms_median: Option<u32>,
    pub instant_answers: u32,
    pub call_minutes: u32,
    pub clip_minutes: u32,
    pub coach_looks: u32,
    pub coach_lines: u32,
    pub attitude: String,
    pub language: Option<String>,
    pub version: String,
    pub windows: Option<String>,
    pub screen: Option<String>,
    pub ai_errors: u32,
    pub voice_errors: u32,
}

impl SharedSession {
    /// `record`, coarsened for sharing (None: a record with no day).
    pub fn of(record: &SessionStats) -> Option<SharedSession> {
        let round_to = |value: f32, step: f32| (value / step).round() * step;
        Some(SharedSession {
            week: week_of(&record.day)?,
            minutes: record.minutes,
            game_minutes: record.game_minutes,
            levels_gained: record.levels_gained,
            level_start_band: record.level_start.map(|l| level_band(l).to_string()),
            level_end_band: record.level_end.map(|l| level_band(l).to_string()),
            job: record.job.clone(),
            hud: record.hud.clone(),
            deaths: record.deaths,
            warnings: record.warnings,
            close_calls: record.close_calls,
            potions_answered: record.potions_answered.map(|r| round_to(r, 0.1)),
            exp_per_hour: record.exp_per_hour.map(|r| round_to(r, 0.1)),
            maps: record.maps.len() as u32,
            map_visits: record.maps.values().sum(),
            sentences: record.sentences,
            replies: record.replies,
            reply_ms_median: record
                .reply_ms_median
                .map(|ms| ((ms as f64 / 100.0).round() * 100.0) as u32),
            instant_answers: record.instant_answers,
            call_minutes: record.call_minutes,
            clip_minutes: record.clip_minutes,
            coach_looks: record.coach_looks,
            coach_lines: record.coach_lines,
            attitude: record.attitude.clone(),
            language: record.language.clone(),
            version: record.version.clone(),
            windows: record.windows.clone(),
            screen: record.screen.clone(),
            ai_errors: record.ai_errors,
            voice_errors: record.voice_errors,
        })
    }
}

/// The export of `records` (those since the day sharing was turned on,
/// when it says), under `consent`'s install id.
pub fn build_export(records: &[SessionStats], consent: &Consent) -> Export {
    Export {
        format: EXPORT_FORMAT,
        install_id: consent.id.clone(),
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        sessions: records
            .iter()
            .filter(|r| {
                consent
                    .since
                    .as_deref()
                    .is_none_or(|since| r.day.as_str() >= since)
            })
            .filter_map(SharedSession::of)
            .collect(),
    }
}

/// A level's band: 1-10, 11-30, 31-60, 61-100, 101-140, 141-200, 201+.
pub fn level_band(level: u32) -> &'static str {
    match level {
        0..=10 => "1-10",
        11..=30 => "11-30",
        31..=60 => "31-60",
        61..=100 => "61-100",
        101..=140 => "101-140",
        141..=200 => "141-200",
        _ => "201+",
    }
}

/// A screen's size class, by its short side: 720p, 1080p, 1440p, 4K, 5K+
/// (and "smaller").
pub fn screen_class(width: u32, height: u32) -> &'static str {
    match width.min(height) {
        2880.. => "5K+",
        2160.. => "4K",
        1440.. => "1440p",
        1080.. => "1080p",
        720.. => "720p",
        _ => "smaller",
    }
}

/// The ISO week of a day ("2026-10-09" → "2026-W41").
pub fn week_of(day: &str) -> Option<String> {
    let date = NaiveDate::parse_from_str(day, "%Y-%m-%d").ok()?;
    let week = date.iso_week();
    Some(format!("{}-W{:02}", week.year(), week.week()))
}

/// A locale's language, without the region ("he-IL" → "he"; the old "iw"
/// is "he").
pub fn language_of(locale: &str) -> Option<String> {
    let primary = locale.trim().split(['-', '_']).next()?.to_ascii_lowercase();
    if !(2..=3).contains(&primary.len()) || !primary.chars().all(|c| c.is_ascii_lowercase()) {
        return None;
    }
    Some(if primary == "iw" {
        "he".into()
    } else {
        primary
    })
}

/// The Windows major version in what `ver` prints ("Microsoft Windows
/// [Version 10.0.22631.4317]", the word in the system's language): "11"
/// for a build of 22000 or later, else the major number.
pub fn windows_from_ver(text: &str) -> Option<String> {
    text.split(|c: char| !(c.is_ascii_digit() || c == '.'))
        .find_map(|run| {
            let parts: Vec<u32> = run
                .split('.')
                .map(|p| p.parse().ok())
                .collect::<Option<_>>()?;
            if parts.len() < 3 {
                return None;
            }
            Some(match (parts[0], parts[2]) {
                (10, build) if build >= 22_000 => "11".to_string(),
                (major, _) => major.to_string(),
            })
        })
}

/// The Windows major version of this PC (None elsewhere, or when `ver`
/// says nothing it can read).
fn windows_major() -> Option<String> {
    if !cfg!(windows) {
        return None;
    }
    let mut command = std::process::Command::new("cmd");
    command
        .args(["/C", "ver"])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // No console window of its own.
        command.creation_flags(0x0800_0000);
    }
    let output = command.output().ok()?;
    windows_from_ver(&String::from_utf8_lossy(&output.stdout))
}

/// The call's timing line for its first sound — the phone's own words,
/// "live: first sound 1.2 s after the player stopped" — in milliseconds.
pub fn call_latency(line: &str) -> Option<u32> {
    let rest = line.strip_prefix("live: first sound ")?;
    let (secs, _) = rest.split_once(" s after")?;
    let secs: f64 = secs.trim().parse().ok()?;
    (secs.is_finite() && (0.0..600.0).contains(&secs)).then(|| (secs * 1000.0).round() as u32)
}

/// A random install id: a UUID v4 from the operating system's randomness
/// (None if it fails: then no id is made, and sharing is not turned on).
pub fn new_install_id() -> Option<String> {
    use ring::rand::{SecureRandom, SystemRandom};
    let mut bytes = [0u8; 16];
    SystemRandom::new().fill(&mut bytes).ok()?;
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    Some(format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    ))
}

/// The median of `values`.
fn median(values: &[u32]) -> Option<u32> {
    if values.is_empty() {
        return None;
    }
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    let mid = sorted.len() / 2;
    Some(if sorted.len().is_multiple_of(2) {
        ((sorted[mid - 1] as u64 + sorted[mid] as u64) / 2) as u32
    } else {
        sorted[mid]
    })
}

/// Whole minutes in `seconds`, rounded.
fn minutes(seconds: f64) -> u32 {
    (seconds.max(0.0) / 60.0).round() as u32
}

/// The words the game shows (a class, a map's name) as kept: trimmed, and
/// only when they look like the game's — not too long, no slash (a path),
/// no `@` (an address), no link — and contain none of `names` (the
/// character's names, lowercased).
fn game_text(text: &str, names: &[String]) -> Option<String> {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let length = text.chars().count();
    let lower = text.to_lowercase();
    let looks_like_the_games = (2..=MAX_GAME_TEXT).contains(&length)
        && !text
            .chars()
            .any(|c| matches!(c, '\\' | '/' | '@' | '=' | '\u{0}'..='\u{1f}'))
        && !lower.contains("http")
        && !lower.contains("www.")
        && !names.iter().any(|name| lower.contains(name.as_str()));
    looks_like_the_games.then_some(text)
}

/// The files under `metrics/` in the settings folder, shared by the main
/// loop (which writes the session's record) and the phone link (which
/// shows the stats and turns sharing on and off). One at a time.
pub struct Store {
    dir: PathBuf,
    lock: Mutex<()>,
}

impl Store {
    /// The store in the settings folder `settings` (`metrics/` in it).
    pub fn new(settings: &Path) -> Arc<Store> {
        Arc::new(Store {
            dir: settings.join("metrics"),
            lock: Mutex::new(()),
        })
    }

    fn guard(&self) -> MutexGuard<'_, ()> {
        self.lock.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }

    /// Write `text` to `name` whole or not at all: beside it, then put in
    /// its place.
    fn write(&self, name: &str, text: &str) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.dir)?;
        let partial = self.path(&format!("{name}.partial"));
        std::fs::write(&partial, text)?;
        std::fs::rename(&partial, self.path(name))
    }

    /// The records kept, the oldest first (a line that cannot be read is
    /// skipped).
    fn kept(&self) -> Vec<SessionStats> {
        std::fs::read_to_string(self.path("sessions.jsonl"))
            .map(|text| {
                text.lines()
                    .filter_map(|line| serde_json::from_str(line).ok())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// `record` among the records kept, in place of the one of its session
    /// (its snapshot, recovered after a crash), the oldest dropped past
    /// [`KEEP_RECORDS`].
    fn keep(&self, record: &SessionStats) {
        let mut records = self.kept();
        records.retain(|r| r.session != record.session);
        records.push(record.clone());
        let oldest = records.len().saturating_sub(KEEP_RECORDS);
        let text: String = records[oldest..]
            .iter()
            .filter_map(|r| serde_json::to_string(r).ok())
            .map(|line| line + "\n")
            .collect();
        let _ = self.write("sessions.jsonl", &text);
    }

    /// The session under way, as last written.
    fn current(&self) -> Option<SessionStats> {
        std::fs::read_to_string(self.path("current.json"))
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
    }

    /// The records kept and the session under way, the newest last.
    fn all(&self) -> Vec<SessionStats> {
        let mut records = self.kept();
        if let Some(current) = self.current()
            && !records.iter().any(|r| r.session == current.session)
        {
            records.push(current);
        }
        records
    }

    /// The session under way, as it stands (`current.json`).
    pub fn snapshot(&self, record: &SessionStats) {
        let _guard = self.guard();
        if let Ok(text) = serde_json::to_string(record) {
            let _ = self.write("current.json", &text);
        }
    }

    /// The session ended: its record joins the others (in place of its
    /// snapshot), and the export is rebuilt when sharing is on.
    pub fn finish(&self, record: &SessionStats) {
        let _guard = self.guard();
        self.keep(record);
        let _ = std::fs::remove_file(self.path("current.json"));
        self.rebuild();
    }

    /// A session that never finished (a crash, a console window closed
    /// with the X): its last snapshot joins the others, and the export is
    /// rebuilt when sharing is on. Returns whether there was one.
    pub fn recover(&self) -> bool {
        let _guard = self.guard();
        let path = self.path("current.json");
        if !path.exists() {
            return false;
        }
        let found = self.current();
        if let Some(record) = &found {
            self.keep(record);
        }
        let _ = std::fs::remove_file(path);
        if found.is_some() {
            self.rebuild();
        }
        found.is_some()
    }

    /// Whether the player shares, as they last said (off unless they
    /// turned it on).
    pub fn consent(&self) -> Consent {
        let _guard = self.guard();
        self.read_consent()
    }

    fn read_consent(&self) -> Consent {
        std::fs::read_to_string(self.path("share.json"))
            .ok()
            .and_then(|text| serde_json::from_str::<Consent>(&text).ok())
            .filter(|c| c.on && c.id.is_some())
            .unwrap_or_default()
    }

    /// Sharing on (`today`: the day it starts from) or off. On, it gets an
    /// install id and the export is built at once; turned on again, the
    /// same id and day stand. Off, the export and the id are deleted.
    pub fn set_sharing(&self, on: bool, today: NaiveDate) -> Result<Consent, String> {
        let _guard = self.guard();
        if !on {
            return Ok(self.turn_off());
        }
        let consent = self.read_consent();
        if consent.on {
            return Ok(consent);
        }
        let id = new_install_id().ok_or("this PC gave no randomness for an install id")?;
        let consent = Consent {
            on: true,
            since: Some(today.format("%Y-%m-%d").to_string()),
            id: Some(id),
        };
        let text = serde_json::to_string_pretty(&consent).map_err(|e| e.to_string())?;
        self.write("share.json", &text)
            .map_err(|e| format!("couldn't save the choice: {e}"))?;
        self.rebuild();
        Ok(consent)
    }

    /// "Delete it": sharing off, the export and the install id deleted.
    pub fn delete_shared(&self) -> Consent {
        let _guard = self.guard();
        self.turn_off()
    }

    fn turn_off(&self) -> Consent {
        let consent = Consent::default();
        if let Ok(text) = serde_json::to_string_pretty(&consent) {
            let _ = self.write("share.json", &text);
        }
        let _ = std::fs::remove_file(self.path("share-export.json"));
        consent
    }

    /// The export rebuilt from the records since sharing was turned on —
    /// only while it is on.
    fn rebuild(&self) {
        let consent = self.read_consent();
        if !consent.on {
            return;
        }
        let export = build_export(&self.all(), &consent);
        if let Ok(text) = serde_json::to_string_pretty(&export) {
            let _ = self.write("share-export.json", &text);
        }
    }

    /// What is ready to share (`share-export.json`), while sharing is on.
    pub fn export(&self) -> Option<Export> {
        let _guard = self.guard();
        if !self.read_consent().on {
            return None;
        }
        std::fs::read_to_string(self.path("share-export.json"))
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
    }

    /// For the phone: whether it shares, and what is shared — the export
    /// while sharing is on, else a preview of what it would look like,
    /// from the last sessions, with no id (made only when turned on, and
    /// written nowhere).
    pub fn share_view(&self) -> ShareView {
        let _guard = self.guard();
        let consent = self.read_consent();
        if consent.on {
            let read = || {
                std::fs::read_to_string(self.path("share-export.json"))
                    .ok()
                    .and_then(|text| serde_json::from_str::<Export>(&text).ok())
            };
            let export = read().or_else(|| {
                self.rebuild();
                read()
            });
            return ShareView {
                on: true,
                since: consent.since,
                preview: false,
                export,
            };
        }
        let records = self.all();
        let last = &records[records.len().saturating_sub(PREVIEW_SESSIONS)..];
        ShareView {
            on: false,
            since: None,
            preview: true,
            export: Some(build_export(last, &Consent::default())),
        }
    }

    /// For the phone: the last `n` sessions (the newest first; the one
    /// under way among them), and whether it shares.
    pub fn stats_view(&self, n: usize) -> StatsView {
        let _guard = self.guard();
        let mut sessions = self.all();
        sessions.reverse();
        sessions.truncate(n);
        StatsView {
            sessions,
            share: Consent {
                id: None,
                ..self.read_consent()
            },
        }
    }
}

/// A session's stats as it goes: the main loop tells it what happens, in
/// one line where it happens; it keeps counts and game facts only, and
/// writes the record every [`SNAPSHOT_EVERY`] seconds and at the end.
pub struct Stats {
    store: Arc<Store>,
    /// The record as it stands: counts kept as they come, the rest filled
    /// in by `record`.
    record: SessionStats,
    sharing: bool,
    /// The latest moment the main loop spoke of (seconds since the start),
    /// and when the record was last written.
    now: f64,
    saved_at: f64,
    /// When the last frame came, and the seconds the game was seen.
    last_frame: Option<f64>,
    game_secs: f64,
    /// When the phone was last looked at, and the seconds on a call and
    /// connected without one.
    last_phone: Option<f64>,
    call_secs: f64,
    clip_secs: f64,
    /// What the companion's warnings came to, as of the last frame, and
    /// when its progress was last read.
    tally: Tally,
    progress_at: f64,
    close_calls: CloseCalls,
    /// When the sight was last looked at, and the map then.
    looked: Option<Instant>,
    map: Option<String>,
    /// The character's names (lowercased), to keep out of the words kept:
    /// held here, never written.
    names: Vec<String>,
    /// Time to the first words of each reply, in milliseconds.
    latencies: Vec<u32>,
}

impl Stats {
    /// The session's stats, starting now, kept in the settings folder
    /// `settings`. A session that never finished (a crash, a closed
    /// window) joins the others first.
    pub fn start(settings: &Path) -> Stats {
        let store = Store::new(settings);
        store.recover();
        let sharing = store.consent().on;
        let commit: String = env!("MS_COMMIT")
            .chars()
            .take_while(|c| c.is_ascii_hexdigit())
            .take(12)
            .collect();
        Stats {
            record: SessionStats {
                session: crate::phone::tls::random_hex(8),
                day: chrono::Local::now().format("%Y-%m-%d").to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                commit: if commit.len() >= 7 {
                    commit
                } else {
                    "unknown".into()
                },
                windows: windows_major(),
                ..SessionStats::default()
            },
            store,
            sharing,
            now: 0.0,
            saved_at: 0.0,
            last_frame: None,
            game_secs: 0.0,
            last_phone: None,
            call_secs: 0.0,
            clip_secs: 0.0,
            tally: Tally::default(),
            progress_at: f64::NEG_INFINITY,
            close_calls: CloseCalls::default(),
            looked: None,
            map: None,
            names: Vec::new(),
            latencies: Vec::new(),
        }
    }

    /// The files, for the phone link.
    pub fn store(&self) -> Arc<Store> {
        Arc::clone(&self.store)
    }

    /// Whether sharing was on when the session started.
    pub fn sharing(&self) -> bool {
        self.sharing
    }

    /// A frame, as the companion took it (`at`: seconds since the start).
    pub fn frame(&mut self, at: f64, obs: &Observation, companion: &Companion) {
        let dt = self.last_frame.map_or(0.0, |t| (at - t).clamp(0.0, 1.0));
        self.last_frame = Some(at);
        self.now = self.now.max(at);
        if obs.game.is_seen() {
            self.game_secs += dt;
        }
        if let Some(name) = &obs.name {
            self.know_name(name);
        }
        if let Some(job) = &obs.job
            && self.record.job.as_deref() != Some(job.trim())
            && let Some(job) = game_text(job, &self.names)
        {
            self.record.job = Some(job);
        }
        if let Some(level) = companion.level() {
            self.record.level_start.get_or_insert(level);
            self.record.level_end = Some(level);
        }
        let hp = obs.hp.map(|g| g.percent);
        if self.close_calls.track(at, hp, companion.dead()).is_some() {
            self.record.close_calls += 1;
        }
        self.tally = companion.tally();
        // (EXP per hour is worked out from a quarter hour of samples, a
        // thousand pairs of them: read every few seconds, not every frame.)
        if at - self.progress_at >= PROGRESS_EVERY {
            self.progress_at = at;
            let progress = companion.progress();
            self.record.levels_gained = progress.levels_gained;
            if let Some(rate) = progress.exp_per_hour.filter(|r| r.is_finite()) {
                self.record.exp_per_hour = Some(((rate * 100.0).round() / 100.0) as f32);
            }
            self.record.attitude = companion.settings.attitude.word().to_string();
        }
    }

    /// A thing the player taught fired: a warning (past their mark) is
    /// counted; news (it showed up) is not a warning.
    pub fn fired(&mut self, warning: bool) {
        if warning {
            self.record.warnings.taught += 1;
        }
    }

    /// The player said a sentence (how many, never what).
    pub fn said(&mut self) {
        self.record.sentences += 1;
    }

    /// An answer given at once, without a model.
    pub fn instant(&mut self) {
        self.record.instant_answers += 1;
    }

    /// A line's first words were heard, `after` its job was handed over:
    /// counted when it is the reply the player is waiting for (`reply`).
    pub fn first_words(&mut self, reply: bool, after: Duration) {
        if reply {
            self.replied(after.as_millis().min(u32::MAX as u128) as u32);
        }
    }

    /// A timing line from the phone's call: its first sound after the
    /// player stopped is a reply, and how long it took (any other line is
    /// not).
    pub fn call_timing(&mut self, line: &str) {
        if let Some(ms) = call_latency(line) {
            self.replied(ms);
        }
    }

    fn replied(&mut self, ms: u32) {
        self.record.replies += 1;
        if self.latencies.len() >= MAX_LATENCIES {
            self.latencies.remove(0);
        }
        self.latencies.push(ms);
    }

    /// A job failed: a reply's (`reply`: the model), or a line's voice. A
    /// job called off is no failure.
    pub fn failed(&mut self, reply: bool, error: &AiError) {
        match (error, reply) {
            (AiError::Cancelled, _) => {}
            (_, true) => self.record.ai_errors += 1,
            (_, false) => self.record.voice_errors += 1,
        }
    }

    /// The coach's look failed.
    pub fn coach_failed(&mut self) {
        self.record.ai_errors += 1;
    }

    /// The phone, as of `at`: connected, and on a live call.
    pub fn phone(&mut self, at: f64, connected: bool, call: bool) {
        let dt = self.last_phone.map_or(0.0, |t| (at - t).clamp(0.0, 1.0));
        self.last_phone = Some(at);
        self.now = self.now.max(at);
        if call {
            self.call_secs += dt;
        } else if connected {
            self.clip_secs += dt;
        }
    }

    /// The screen's size (`frame`: the game's frames), and, every couple
    /// of seconds, what the sight knows: the map, the HUD's style (and the
    /// character's name, to keep it out).
    pub fn look(&mut self, sight: Option<&Arc<Mutex<Sight>>>, frame: Option<(u32, u32)>) {
        if let Some((width, height)) = frame {
            let class = screen_class(width, height);
            if self.record.screen.as_deref() != Some(class) {
                self.record.screen = Some(class.to_string());
            }
        }
        if self.looked.is_some_and(|at| at.elapsed() < LOOK_EVERY) {
            return;
        }
        self.looked = Some(Instant::now());
        let Some(sight) = sight else {
            return;
        };
        let (map, name, classic) = {
            let sight = sight.lock().unwrap_or_else(|e| e.into_inner());
            (
                sight.facts.map.clone(),
                sight.facts.name.clone(),
                sight.numbers.classic(),
            )
        };
        if let Some(name) = name {
            self.know_name(&name);
        }
        if let Some(classic) = classic {
            self.record.hud = Some(if classic { "classic" } else { "modern" }.into());
        }
        if let Some(map) = map
            && self.map.as_ref() != Some(&map)
        {
            if let Some(kept) = game_text(&map, &self.names)
                && (self.record.maps.len() < MAX_MAPS || self.record.maps.contains_key(&kept))
            {
                *self.record.maps.entry(kept).or_insert(0) += 1;
            }
            self.map = Some(map);
        }
    }

    fn know_name(&mut self, name: &str) {
        let name = name.trim().to_lowercase();
        if name.chars().count() >= 3 && !self.names.contains(&name) && self.names.len() < 8 {
            self.names.push(name);
        }
    }

    /// The record as of now (`coach`: its looks and lines; `language`:
    /// the phone's locale). The words kept are checked once more against
    /// every name known by now.
    pub fn record(&self, coach: &Coach, language: Option<&str>) -> SessionStats {
        let mut record = self.record.clone();
        record.minutes = minutes(self.now);
        record.game_minutes = minutes(self.game_secs);
        record.call_minutes = minutes(self.call_secs);
        record.clip_minutes = minutes(self.clip_secs);
        record.level_start_band = record.level_start.map(|l| level_band(l).to_string());
        record.level_end_band = record.level_end.map(|l| level_band(l).to_string());
        record.deaths = self.tally.deaths;
        record.warnings.hp_low = self.tally.hp_low;
        record.warnings.mp_low = self.tally.mp_low;
        record.warnings.beating = self.tally.beating;
        record.potions_answered = (self.tally.low_lines > 0).then(|| {
            let ratio = (self.tally.potted as f32 / self.tally.low_lines as f32).min(1.0);
            (ratio * 100.0).round() / 100.0
        });
        record.reply_ms_median = median(&self.latencies);
        record.coach_looks = coach.consults;
        record.coach_lines = coach.spoken;
        record.language = language.and_then(language_of);
        if record.attitude.is_empty() {
            record.attitude = crate::companion::Attitude::default().word().to_string();
        }
        record.job = record.job.and_then(|job| game_text(&job, &self.names));
        record.maps = std::mem::take(&mut record.maps)
            .into_iter()
            .filter_map(|(map, n)| game_text(&map, &self.names).map(|map| (map, n)))
            .collect();
        record
    }

    /// Every [`SNAPSHOT_EVERY`] seconds: the session so far, written.
    pub fn save_every(&mut self, coach: &Coach, language: Option<&str>) {
        if self.now - self.saved_at >= SNAPSHOT_EVERY {
            self.saved_at = self.now;
            self.store.snapshot(&self.record(coach, language));
        }
    }

    /// The session ended: its record kept, and the export rebuilt when
    /// sharing is on.
    pub fn finish(&mut self, coach: &Coach, language: Option<&str>) {
        self.store.finish(&self.record(coach, language));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::companion::{GameView, Gauge, Settings};
    use serde_json::Value;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ms-metrics-{name}-{}",
            crate::phone::tls::random_hex(4)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn day(text: &str) -> NaiveDate {
        NaiveDate::parse_from_str(text, "%Y-%m-%d").unwrap()
    }

    /// Every key in `value`, nested ones too — but not the keys of `maps`
    /// (the maps' names: data, not fields).
    fn keys(value: &Value, out: &mut Vec<String>) {
        match value {
            Value::Object(map) => {
                for (key, value) in map {
                    out.push(key.clone());
                    if key != "maps" {
                        keys(value, out);
                    }
                }
            }
            Value::Array(items) => items.iter().for_each(|v| keys(v, out)),
            _ => {}
        }
    }

    fn seen(hp: f32, level: u32, name: &str, job: &str, title: &str) -> Observation {
        Observation {
            game: GameView::Seen(title.into()),
            hp: Some(Gauge {
                percent: hp,
                current: None,
                max: None,
                read: true,
            }),
            mp: Some(Gauge {
                percent: 80.0,
                current: None,
                max: None,
                read: true,
            }),
            exp: Some(Gauge {
                percent: 10.0,
                current: None,
                max: None,
                read: true,
            }),
            level: Some(level),
            name: Some(name.into()),
            job: Some(job.into()),
        }
    }

    /// A full record: every field there is set.
    fn full_record(session: &str, day: &str) -> SessionStats {
        SessionStats {
            session: session.into(),
            day: day.into(),
            minutes: 95,
            game_minutes: 90,
            levels_gained: 2,
            level_start: Some(152),
            level_end: Some(154),
            level_start_band: Some("141-200".into()),
            level_end_band: Some("141-200".into()),
            job: Some("Night Lord".into()),
            hud: Some("modern".into()),
            deaths: 1,
            warnings: Warnings {
                hp_low: 3,
                mp_low: 1,
                beating: 2,
                taught: 1,
            },
            close_calls: 1,
            potions_answered: Some(0.67),
            exp_per_hour: Some(1.23),
            maps: BTreeMap::from([("Henesys".to_string(), 2), ("Ellinia".to_string(), 1)]),
            sentences: 40,
            replies: 38,
            reply_ms_median: Some(1234),
            instant_answers: 5,
            call_minutes: 60,
            clip_minutes: 20,
            coach_looks: 30,
            coach_lines: 9,
            attitude: "savage".into(),
            language: Some("he".into()),
            version: "0.9.0".into(),
            commit: "30004c5abcde".into(),
            windows: Some("11".into()),
            screen: Some("4K".into()),
            ai_errors: 1,
            voice_errors: 2,
        }
    }

    #[test]
    fn a_record_and_its_export_never_hold_the_name_the_words_or_a_path() {
        // A session with everything private in what goes in: the
        // character's name in every frame and in the sight's facts, the
        // player's words (to the companion, which the stats read from), a
        // window title with a path in it, the settings folder under a
        // Windows user's name, a timing line from the phone with words in
        // it. Out come counts, the class and the map — and none of those.
        let base = temp_dir("privacy");
        let settings = base
            .join("C")
            .join("Users")
            .join("מיכאל")
            .join("AppData")
            .join("Roaming")
            .join("MapleSyrup");
        let mut stats = Stats::start(&settings);
        let mut companion = Companion::seeded(Settings::default(), 7);
        let coach = Coach::new(true);
        let sight = Arc::new(Mutex::new(Sight::load(&settings.join("learned"))));
        {
            let mut sight = sight.lock().unwrap();
            sight.facts.name = Some("WanWanBoggio".into());
            sight.facts.map = Some("Henesys".into());
        }
        let title = r"MapleStory - C:\Users\מיכאל\Nexon\MapleStory.exe";
        let transcript = "my name is Michael and my password is hunter2, I live in Moshav Livnim";
        for i in 0..1200 {
            let t = i as f64 * 0.1;
            let obs = seen(90.0, 152, "WanWanBoggio", "Night Lord", title);
            companion.observe(t, obs.clone());
            stats.frame(t, &obs, &companion);
            stats.phone(t, true, false);
            stats.look(Some(&sight), Some((3840, 2160)));
            if i == 600 {
                companion.player_spoke(t);
                companion.heard(t, transcript);
                stats.said();
                stats.call_timing(&format!("live: the player said {transcript}"));
                stats.call_timing("live: first sound 1.4 s after the player stopped");
            }
        }
        // A class misread as the name, and a map with a path in it: not kept.
        let odd = seen(
            90.0,
            152,
            "WanWanBoggio",
            "WanWanBoggio the Night Lord",
            title,
        );
        stats.frame(121.0, &odd, &companion);
        stats.looked = None;
        sight.lock().unwrap().facts.map = Some(r"C:\Users\מיכאל\Documents".into());
        stats.look(Some(&sight), Some((3840, 2160)));
        stats.finish(&coach, Some("he-IL"));
        assert!(
            Store::new(&settings)
                .set_sharing(true, day("2000-01-01"))
                .is_ok()
        );

        let record = Store::new(&settings).all().pop().unwrap();
        let record_text = serde_json::to_string(&record).unwrap();
        let export = Store::new(&settings).export().unwrap();
        let export_text = serde_json::to_string(&export).unwrap();
        let files: String = ["sessions.jsonl", "share-export.json", "share.json"]
            .iter()
            .map(|f| std::fs::read_to_string(settings.join("metrics").join(f)).unwrap())
            .collect();
        for text in [&record_text, &export_text, &files] {
            for private in [
                "WanWanBoggio",
                "wanwanboggio",
                "Michael",
                "hunter2",
                "Moshav",
                "מיכאל",
                "Users",
                "Nexon",
                "Documents",
                &base.display().to_string(),
            ] {
                assert!(!text.contains(private), "{private:?} in {text}");
            }
        }
        // What is kept: the class and the map (by name on this PC, as a
        // count in the export), the language without its region.
        assert_eq!(record.job.as_deref(), Some("Night Lord"));
        assert_eq!(record.maps, BTreeMap::from([("Henesys".to_string(), 1)]));
        assert_eq!(record.language.as_deref(), Some("he"));
        assert_eq!(record.sentences, 1);
        assert_eq!(record.replies, 1);
        assert!(export_text.contains("Night Lord"));
        assert!(!export_text.contains("Henesys"), "{export_text}");
        assert_eq!(export.sessions[0].maps, 1);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn every_record_and_every_export_keeps_to_its_allow_list() {
        let record = full_record("s1", "2026-10-09");
        let mut found = Vec::new();
        keys(&serde_json::to_value(&record).unwrap(), &mut found);
        for key in &found {
            assert!(
                RECORD_FIELDS.contains(&key.as_str()),
                "{key} is not allowed in a record"
            );
        }
        // (Every field of the record is set here: the list is what a record has.)
        for field in RECORD_FIELDS {
            assert!(found.iter().any(|k| k == field), "{field} is in no record");
        }
        let consent = Consent {
            on: true,
            since: Some("2026-10-01".into()),
            id: new_install_id(),
        };
        let export = build_export(&[record.clone(), full_record("s2", "2026-10-10")], &consent);
        assert_eq!(export.sessions.len(), 2);
        let mut found = Vec::new();
        keys(&serde_json::to_value(&export).unwrap(), &mut found);
        for key in &found {
            assert!(
                EXPORT_FIELDS.contains(&key.as_str()),
                "{key} is not allowed in an export"
            );
        }
        // Never shared, whatever the record has: the day, the exact levels,
        // the maps' names, the session's id, the commit — and no name of
        // any kind.
        for kept_home in [
            "day",
            "level_start",
            "level_end",
            "session",
            "commit",
            "name",
            "title",
            "path",
            "text",
        ] {
            assert!(!EXPORT_FIELDS.contains(&kept_home), "{kept_home}");
            assert!(!found.iter().any(|k| k == kept_home), "{kept_home}");
        }
        let shared = serde_json::to_value(&export).unwrap();
        assert!(shared["sessions"][0]["maps"].is_u64(), "{shared}");
    }

    #[test]
    fn the_export_is_coarsened_and_only_from_the_day_sharing_began() {
        let mut early = full_record("s0", "2026-09-30");
        early.job = Some("Hero".into());
        let records = [early, full_record("s1", "2026-10-09")];
        let consent = Consent {
            on: true,
            since: Some("2026-10-01".into()),
            id: Some("id".into()),
        };
        let export = build_export(&records, &consent);
        assert_eq!(export.format, EXPORT_FORMAT);
        assert_eq!(export.install_id.as_deref(), Some("id"));
        assert_eq!(export.app_version, env!("CARGO_PKG_VERSION"));
        assert_eq!(export.sessions.len(), 1, "a session before the consent");
        let s = &export.sessions[0];
        assert_eq!(s.week, "2026-W41");
        assert_eq!(s.level_start_band.as_deref(), Some("141-200"));
        assert_eq!((s.maps, s.map_visits), (2, 3));
        assert_eq!(s.reply_ms_median, Some(1200));
        assert_eq!(s.potions_answered, Some(0.7));
        assert_eq!(s.exp_per_hour, Some(1.2));
        assert_eq!(s.job.as_deref(), Some("Night Lord"));
    }

    #[test]
    fn sharing_is_off_until_turned_on_and_off_deletes_the_export_and_the_id() {
        let settings = temp_dir("consent");
        let store = Store::new(&settings);
        let export = settings.join("metrics").join("share-export.json");
        // Off unless turned on: no id, no export; the phone sees a preview
        // of the last sessions, with no id, written nowhere.
        assert_eq!(store.consent(), Consent::default());
        store.finish(&full_record("s0", "2026-10-08"));
        let view = store.share_view();
        assert!(!view.on && view.preview, "{view:?}");
        let preview = view.export.unwrap();
        assert_eq!(preview.install_id, None);
        assert_eq!(preview.sessions.len(), 1);
        assert!(!export.exists());
        // On: an id (a UUID v4), the day, the export built at once — from
        // that day on.
        let on = store.set_sharing(true, day("2026-10-09")).unwrap();
        let id = on.id.clone().unwrap();
        assert_eq!(on.since.as_deref(), Some("2026-10-09"));
        assert_eq!(id.len(), 36);
        assert_eq!(&id[14..15], "4", "{id}");
        assert!("89ab".contains(&id[19..20]), "{id}");
        assert!(export.exists());
        assert_eq!(store.export().unwrap().install_id.as_deref(), Some(&*id));
        assert!(store.export().unwrap().sessions.is_empty());
        // Rebuilt at a session's end.
        store.finish(&full_record("s1", "2026-10-09"));
        assert_eq!(store.export().unwrap().sessions.len(), 1);
        let view = store.share_view();
        assert!(view.on && !view.preview, "{view:?}");
        assert_eq!(view.export.unwrap().install_id.as_deref(), Some(&*id));
        // Turned on again: the same id and day.
        assert_eq!(store.set_sharing(true, day("2026-10-20")).unwrap(), on);
        // Off: the export and the id are deleted.
        assert_eq!(
            store.set_sharing(false, day("2026-10-21")).unwrap(),
            Consent::default()
        );
        assert!(!export.exists());
        assert!(store.export().is_none());
        let saved = std::fs::read_to_string(settings.join("metrics").join("share.json")).unwrap();
        assert!(!saved.contains(&id), "{saved}");
        // On again: a new id. "Delete it": off, and the export and the id gone.
        let again = store.set_sharing(true, day("2026-10-22")).unwrap();
        assert_ne!(again.id, on.id);
        assert!(export.exists());
        assert_eq!(store.delete_shared(), Consent::default());
        assert!(!export.exists());
        assert_eq!(store.consent(), Consent::default());
        // The stats themselves stay (they never leave the PC).
        assert_eq!(store.all().len(), 2);
        let _ = std::fs::remove_dir_all(&settings);
    }

    #[test]
    fn a_session_is_written_as_it_goes_and_kept_after_a_crash_once() {
        let settings = temp_dir("crash");
        // A session under way writes its snapshot every minute.
        let mut stats = Stats::start(&settings);
        let coach = Coach::new(true);
        let current = settings.join("metrics").join("current.json");
        stats.phone(30.0, true, false);
        stats.save_every(&coach, None);
        assert!(!current.exists(), "not before the first minute");
        stats.said();
        stats.phone(90.0, true, false);
        stats.save_every(&coach, None);
        let snapshot: SessionStats =
            serde_json::from_str(&std::fs::read_to_string(&current).unwrap()).unwrap();
        assert_eq!(snapshot.sentences, 1);
        // The phone's table has it already.
        let view = stats.store().stats_view(SHOWN_SESSIONS);
        assert_eq!(view.sessions.len(), 1);
        assert_eq!(view.sessions[0].sentences, 1);
        // The window closed with the X: the next start keeps it, once.
        drop(stats);
        let next = Stats::start(&settings);
        assert!(!current.exists());
        let kept = Store::new(&settings).kept();
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].sentences, 1);
        assert_ne!(kept[0].session, next.record.session);
        // A session's end replaces its snapshot, never doubles it.
        let mut stats = next;
        stats.phone(61.0, false, false);
        stats.save_every(&coach, None);
        stats.said();
        stats.finish(&coach, None);
        let kept = Store::new(&settings).kept();
        assert_eq!(kept.len(), 2);
        assert_eq!(kept[1].sentences, 1);
        assert!(!current.exists());
        let _ = std::fs::remove_dir_all(&settings);
    }

    #[test]
    fn at_most_a_year_of_sessions_is_kept_the_oldest_dropped() {
        let settings = temp_dir("keep");
        let store = Store::new(&settings);
        for i in 0..(KEEP_RECORDS + 5) {
            store.finish(&full_record(&format!("s{i}"), "2026-10-09"));
        }
        let kept = store.kept();
        assert_eq!(kept.len(), KEEP_RECORDS);
        assert_eq!(kept[0].session, "s5");
        assert_eq!(
            kept.last().unwrap().session,
            format!("s{}", KEEP_RECORDS + 4)
        );
        // The phone sees the last ten, the newest first.
        let view = store.stats_view(SHOWN_SESSIONS);
        assert_eq!(view.sessions.len(), SHOWN_SESSIONS);
        assert_eq!(view.sessions[0].session, format!("s{}", KEEP_RECORDS + 4));
        assert_eq!(view.share, Consent::default());
        let _ = std::fs::remove_dir_all(&settings);
    }

    #[test]
    fn the_stats_count_what_the_main_loop_tells_them() {
        let settings = temp_dir("count");
        let mut stats = Stats::start(&settings);
        let mut companion = Companion::seeded(Settings::default(), 7);
        let mut coach = Coach::new(true);
        coach.consults = 4;
        coach.spoken = 2;
        // Two minutes of the game at level 61, a close call at the first
        // (a hit to 8%, potted a second later), a level-up read at the end.
        for i in 0..1200 {
            let t = i as f64 * 0.1;
            let hp = if (20.0..21.0).contains(&t) { 8.0 } else { 90.0 };
            let level = if t < 110.0 { 61 } else { 62 };
            let obs = seen(hp, level, "Someone", "Bishop", "MapleStory");
            companion.observe(t, obs.clone());
            stats.frame(t, &obs, &companion);
            // On a call the first minute, connected without one the next.
            stats.phone(t, true, t < 60.0);
        }
        // Then three minutes with the game gone.
        for i in 1200..3000 {
            let t = i as f64 * 0.1;
            let obs = Observation::unseen(GameView::NotFound);
            companion.observe(t, obs.clone());
            stats.frame(t, &obs, &companion);
        }
        for _ in 0..3 {
            stats.said();
        }
        stats.instant();
        stats.first_words(true, Duration::from_millis(1250));
        stats.first_words(false, Duration::from_millis(9000));
        stats.call_timing("live: first sound 0.8 s after the player stopped");
        stats
            .call_timing("live: look_it_up ran in 2.0 s; the answer after them is timed from here");
        stats.failed(true, &AiError::Http(500, "oops".into()));
        stats.failed(false, &AiError::Network("down".into()));
        stats.failed(true, &AiError::Cancelled);
        stats.coach_failed();
        stats.fired(true);
        stats.fired(false);
        stats.look(None, Some((2560, 1440)));
        let record = stats.record(&coach, Some("en-US"));
        assert_eq!(record.minutes, 5);
        assert_eq!(record.game_minutes, 2);
        assert_eq!((record.call_minutes, record.clip_minutes), (1, 1));
        assert_eq!((record.level_start, record.level_end), (Some(61), Some(62)));
        assert_eq!(
            (
                record.level_start_band.as_deref(),
                record.level_end_band.as_deref()
            ),
            (Some("61-100"), Some("61-100"))
        );
        assert_eq!(record.levels_gained, companion.progress().levels_gained);
        assert_eq!(record.job.as_deref(), Some("Bishop"));
        assert_eq!(record.close_calls, 1);
        assert_eq!(record.sentences, 3);
        assert_eq!(record.instant_answers, 1);
        assert_eq!(record.replies, 2);
        assert_eq!(record.reply_ms_median, Some(1025));
        assert_eq!((record.ai_errors, record.voice_errors), (2, 1));
        assert_eq!(record.warnings.taught, 1);
        assert_eq!((record.coach_looks, record.coach_lines), (4, 2));
        assert_eq!(record.screen.as_deref(), Some("1440p"));
        assert_eq!(record.language.as_deref(), Some("en"));
        assert_eq!(record.attitude, "friendly");
        assert_eq!(record.deaths, companion.tally().deaths);
        assert_eq!(record.version, env!("CARGO_PKG_VERSION"));
        assert!(record.commit == "unknown" || record.commit.len() >= 7);
        let _ = std::fs::remove_dir_all(&settings);
    }

    #[test]
    fn the_small_things_are_read_as_they_should_be() {
        assert_eq!(
            windows_from_ver("\r\nMicrosoft Windows [Version 10.0.22631.4317]\r\n").as_deref(),
            Some("11")
        );
        assert_eq!(
            windows_from_ver("Microsoft Windows [Version 10.0.19045.5011]").as_deref(),
            Some("10")
        );
        // (The word is in the system's language.)
        assert_eq!(
            windows_from_ver("Microsoft Windows [גרסה 10.0.26100.2033]").as_deref(),
            Some("11")
        );
        assert_eq!(windows_from_ver("no version here 1.2"), None);
        assert_eq!(screen_class(3840, 2160), "4K");
        assert_eq!(screen_class(2560, 1440), "1440p");
        assert_eq!(screen_class(3440, 1440), "1440p");
        assert_eq!(screen_class(1920, 1080), "1080p");
        assert_eq!(screen_class(1366, 768), "720p");
        assert_eq!(screen_class(1024, 600), "smaller");
        assert_eq!(screen_class(5120, 2880), "5K+");
        for (level, band) in [
            (1, "1-10"),
            (10, "1-10"),
            (11, "11-30"),
            (60, "31-60"),
            (61, "61-100"),
            (140, "101-140"),
            (200, "141-200"),
            (201, "201+"),
            (300, "201+"),
        ] {
            assert_eq!(level_band(level), band, "{level}");
        }
        assert_eq!(week_of("2026-10-09").as_deref(), Some("2026-W41"));
        assert_eq!(week_of("2027-01-01").as_deref(), Some("2026-W53"));
        assert_eq!(week_of("nope"), None);
        assert_eq!(language_of("he-IL").as_deref(), Some("he"));
        assert_eq!(language_of("iw").as_deref(), Some("he"));
        assert_eq!(language_of("zh_Hant").as_deref(), Some("zh"));
        assert_eq!(language_of("<script>"), None);
        assert_eq!(
            call_latency("live: first sound 1.2 s after the player stopped"),
            Some(1200)
        );
        assert_eq!(call_latency("live: first sound soon s after"), None);
        assert_eq!(median(&[]), None);
        assert_eq!(median(&[3, 1, 2]), Some(2));
        assert_eq!(median(&[4, 1, 3, 2]), Some(2));
        let names = vec!["wanwan".to_string()];
        assert_eq!(
            game_text("  Night   Lord ", &names).as_deref(),
            Some("Night Lord")
        );
        assert_eq!(
            game_text("Kerning City Subway: Line 1 <Area 1>", &names).as_deref(),
            Some("Kerning City Subway: Line 1 <Area 1>")
        );
        assert_eq!(game_text(r"C:\Users\x\Desktop", &names), None);
        assert_eq!(game_text("me@example.com", &names), None);
        assert_eq!(game_text("WanWan's map", &names), None);
        assert_eq!(game_text(&"x".repeat(60), &names), None);
        let a = new_install_id().unwrap();
        let b = new_install_id().unwrap();
        assert_ne!(a, b);
        assert!(
            a.chars().all(|c| c.is_ascii_hexdigit() || c == '-') && a.matches('-').count() == 4,
            "{a}"
        );
    }
}
