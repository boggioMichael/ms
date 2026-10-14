//! The session's stats: what each session came to, in numbers, kept on
//! this PC for MapleSyrup's own product metrics — and, only when the player
//! turns it on, the same numbers made ready to share with partners.
//!
//! Two things, kept apart:
//!
//! - **Stats** ([`SessionStats`]): one record per session in
//!   `metrics/sessions.jsonl` under the settings folder
//!   (`%APPDATA%\MapleSyrup`), the newest last, at most [`KEEP_RECORDS`].
//!   The session under way is `metrics/current-<session>.json` — its own
//!   file, so that two copies of MapleSyrup at once never write over each
//!   other's — rewritten every [`SNAPSHOT_EVERY`] seconds (a file of a
//!   kilobyte, written whole and then put in place): it joins the others
//!   when the session ends — or, after a crash or a console window closed
//!   with the X, when MapleSyrup next starts — and is deleted only once it
//!   has (a write that fails leaves it for the next start), so no session
//!   is lost, and at most a minute of one. Stats never leave the PC.
//! - **Sharing** ([`Consent`], [`Export`]): off unless the player turns it
//!   on (Settings on the phone), apart from the stats. Turned on, it gets a
//!   random install id (a UUID v4 from the operating system's randomness,
//!   through `ring`, already a dependency), and `metrics/share-export.json`
//!   is rebuilt then and at the end of every session from the records
//!   since the day it was turned on, coarsened: the week, not the day;
//!   level bands, not levels; how many maps, not which; latencies rounded;
//!   no commit, no session id. Turning it off, or "Delete it", deletes the
//!   choice (`share.json`: no file is off), the export with the id, and
//!   any half-written file — or says what is still on the PC
//!   ([`ShareError`]): a withdrawal never fails silently.
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
//! calls take no text of the player's. The class is a name from a closed
//! list ([`CLASSES`]) or [`OTHER_CLASS`], never the words read or said; a
//! map's name is kept (on this PC only) when it looks like the game's — no
//! path, no `@`, no link, not too long — and does not contain the
//! character's name. [`RECORD_FIELDS`] and [`EXPORT_FIELDS`] are the
//! allow-lists the tests hold every record and every export to.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
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
/// How often the session under way is written (`current-<session>.json`),
/// in seconds: a closed console window or a crash loses at most this much.
pub const SNAPSHOT_EVERY: f64 = 60.0;
/// The sessions the phone's table shows.
pub const SHOWN_SESSIONS: usize = 10;
/// The sessions a preview of the export shows, while sharing is off.
pub const PREVIEW_SESSIONS: usize = 3;
/// The export's format, for whoever reads it.
pub const EXPORT_FORMAT: u32 = 1;
/// Maps kept per session, at most (a session of portals is still one line).
const MAX_MAPS: usize = 100;
/// A map's name longer than this is not one.
const MAX_GAME_TEXT: usize = 48;
/// The class of a record whose class was read, or said, as words that are
/// no class's name in [`CLASSES`].
pub const OTHER_CLASS: &str = "other";
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
    "characters",
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
    "characters",
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

/// The classes of MapleStory and their advancements, modern and Classic
/// World, by the name the game gives them, each with the other ways it is
/// written or said: an older name, the elements spelled out, the players'
/// short forms, Hebrew. Matched whole ([`class_of`]: case, spaces,
/// hyphens and the kind of apostrophe aside), these names are the only
/// words a record or an export ever holds for the class: anything else is
/// [`OTHER_CLASS`].
pub const CLASSES: &[(&str, &[&str])] = &[
    // Explorers: the beginner, and the warriors.
    ("Beginner", &["בגינר", "ביגינר", "מתחיל"]),
    (
        "Warrior",
        &["Swordman", "Swordsman", "ווריור", "וריור", "לוחם"],
    ),
    ("Fighter", &["פייטר"]),
    ("Crusader", &["קרוסיידר", "קרוסדר"]),
    ("Hero", &["הירו"]),
    ("Page", &["פייג'"]),
    ("White Knight", &["וייט נייט", "ווייט נייט"]),
    ("Paladin", &["Pally", "פלאדין", "פלדין"]),
    ("Spearman", &["ספירמן"]),
    ("Dragon Knight", &["דרגון נייט", "דראגון נייט"]),
    ("Berserker", &["ברזרקר", "ברסרקר"]),
    ("Dark Knight", &["DK", "דארק נייט", "דרק נייט"]),
    // The magicians (F/P: fire and poison; I/L: ice and lightning).
    ("Magician", &["מג'ישן", "קוסם"]),
    ("Wizard", &["ויזארד", "וויזארד"]),
    ("Mage", &["מייג'", "מאג'"]),
    ("Arch Mage", &["ארצ' מייג'", "ארך מייג'"]),
    (
        "Wizard (F/P)",
        &[
            "Wizard (Fire, Poison)",
            "Wizard (Fire/Poison)",
            "F/P Wizard",
            "Fire/Poison Wizard",
        ],
    ),
    (
        "Mage (F/P)",
        &[
            "Mage (Fire, Poison)",
            "Mage (Fire/Poison)",
            "F/P Mage",
            "Fire/Poison Mage",
        ],
    ),
    (
        "Arch Mage (F/P)",
        &[
            "Arch Mage (Fire, Poison)",
            "Arch Mage (Fire/Poison)",
            "F/P Arch Mage",
            "Fire/Poison Arch Mage",
        ],
    ),
    (
        "Wizard (I/L)",
        &[
            "Wizard (Ice, Lightning)",
            "Wizard (Ice/Lightning)",
            "I/L Wizard",
            "Ice/Lightning Wizard",
        ],
    ),
    (
        "Mage (I/L)",
        &[
            "Mage (Ice, Lightning)",
            "Mage (Ice/Lightning)",
            "I/L Mage",
            "Ice/Lightning Mage",
        ],
    ),
    (
        "Arch Mage (I/L)",
        &[
            "Arch Mage (Ice, Lightning)",
            "Arch Mage (Ice/Lightning)",
            "I/L Arch Mage",
            "Ice/Lightning Arch Mage",
        ],
    ),
    ("Cleric", &["קלריק"]),
    ("Priest", &["פריסט"]),
    ("Bishop", &["Bish", "בישופ", "בישוף"]),
    // The bowmen.
    ("Bowman", &["Archer", "באומן", "ארצ'ר", "קשת"]),
    ("Hunter", &["האנטר"]),
    ("Ranger", &["ריינג'ר"]),
    ("Bowmaster", &["BM", "באומאסטר", "באו מאסטר", "בואו מאסטר"]),
    ("Crossbowman", &["קרוסבואומן", "קרוסבומן"]),
    ("Sniper", &["סנייפר"]),
    ("Marksman", &["Crossbow Master", "MM", "מרקסמן"]),
    ("Pathfinder", &["PF", "פאת'פיינדר"]),
    // The thieves.
    ("Thief", &["Rogue", "ת'יף", "גנב"]),
    ("Assassin", &["Sin", "אססין", "אסאסין", "מתנקש"]),
    ("Hermit", &["הרמיט"]),
    ("Night Lord", &["NL", "נייט לורד", "ניט לורד"]),
    ("Bandit", &["בנדיט"]),
    ("Chief Bandit", &["CB", "צ'יף בנדיט"]),
    ("Shadower", &["Shad", "שאדואר", "שדואר"]),
    ("Dual Blade", &["DB", "Dual Blader", "דואל בלייד"]),
    ("Blade Recruit", &[]),
    ("Blade Acolyte", &[]),
    ("Blade Specialist", &[]),
    ("Blade Lord", &[]),
    ("Blade Master", &[]),
    // The pirates.
    ("Pirate", &["פיראט"]),
    ("Brawler", &["Infighter", "בראולר"]),
    ("Marauder", &["מרודר"]),
    ("Buccaneer", &["Bucc", "בוקנייר", "בוקניר"]),
    ("Gunslinger", &["גאנסלינגר"]),
    ("Outlaw", &["אאוטלו"]),
    ("Corsair", &["Sair", "קורסייר", "קורסר"]),
    ("Cannoneer", &["Cannon Shooter", "קנונייר"]),
    ("Cannon Blaster", &[]),
    ("Cannon Trooper", &[]),
    ("Cannon Master", &[]),
    ("Jett", &["ג'ט"]),
    // Cygnus Knights.
    ("Noblesse", &[]),
    ("Dawn Warrior", &["DW"]),
    ("Blaze Wizard", &["BW"]),
    ("Wind Archer", &["WA"]),
    ("Night Walker", &["NW"]),
    ("Thunder Breaker", &["TB"]),
    ("Mihile", &[]),
    // Heroes.
    ("Legend", &[]),
    ("Aran", &["ארן"]),
    ("Evan", &["אוון"]),
    ("Mercedes", &["Merc", "מרצדס", "מרסדס"]),
    ("Phantom", &["פנטום", "פאנטום"]),
    ("Luminous", &["Lumi", "לומינוס"]),
    ("Shade", &["Eunwol", "שייד"]),
    // Resistance.
    ("Citizen", &[]),
    ("Blaster", &["בלאסטר"]),
    ("Battle Mage", &["BaM", "באטל מייג'"]),
    ("Wild Hunter", &["WH", "ווילד האנטר"]),
    ("Mechanic", &["Mech", "מכניק"]),
    ("Xenon", &["זנון"]),
    ("Demon Slayer", &["DS", "דימון סלייר", "דמון סלייר"]),
    ("Demon Avenger", &["DA", "דימון אוונג'ר", "דמון אוונג'ר"]),
    // Nova.
    ("Kaiser", &["קייזר"]),
    ("Angelic Buster", &["AB", "אנג'ליק באסטר"]),
    ("Cadena", &["קדנה", "קאדנה"]),
    ("Kain", &["קיין"]),
    // Flora.
    ("Illium", &["איליום"]),
    ("Ark", &["ארק"]),
    ("Adele", &["אדל"]),
    ("Khali", &["קאלי"]),
    // Anima.
    ("Hoyoung", &["הויונג"]),
    ("Lara", &["לארה"]),
    // And the rest.
    ("Hayato", &["האיאטו"]),
    ("Kanna", &["קאנה"]),
    ("Zero", &["זירו"]),
    ("Kinesis", &["קינסיס"]),
    ("Beast Tamer", &["BT"]),
    ("Lynn", &["לין"]),
    ("Ren", &[]),
    ("Mo Xuan", &[]),
];

/// The class `text` names — read off the screen, or said by the player —
/// as [`CLASSES`] names it; None when it names none.
pub fn class_of(text: &str) -> Option<&'static str> {
    static NAMES: OnceLock<HashMap<String, &'static str>> = OnceLock::new();
    let names = NAMES.get_or_init(|| {
        CLASSES
            .iter()
            .flat_map(|&(name, also)| {
                std::iter::once(name)
                    .chain(also.iter().copied())
                    .map(move |said| (class_key(said), name))
            })
            .collect()
    });
    names.get(&class_key(text)).copied()
}

/// What a record holds for the class read, or said, as `text`: its name
/// ([`class_of`]), or [`OTHER_CLASS`] — never the words themselves.
pub fn class_or_other(text: &str) -> String {
    class_of(text).unwrap_or(OTHER_CLASS).to_string()
}

/// `text` as it is matched with the classes' names: in lower case, with
/// no spaces, hyphens, underscores, dots or direction marks, and one kind
/// of apostrophe ("Night-Lord", " NIGHT  LORD " and "Night Lord" are one;
/// so are "פייג׳" and "פייג'").
fn class_key(text: &str) -> String {
    text.chars()
        .filter(|&c| {
            !c.is_whitespace()
                && !matches!(c, '-' | '_' | '.' | '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        })
        .map(|c| match c {
            '\u{5f3}' | '\u{2018}' | '\u{2019}' | '`' | '\u{b4}' => '\'',
            c => c,
        })
        .flat_map(char::to_lowercase)
        .collect()
}

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
    /// Levels gained, the level at the start and at the end, and their
    /// bands ([`level_band`]) — of the last character played (see
    /// `characters`).
    pub levels_gained: u32,
    pub level_start: Option<u32>,
    pub level_end: Option<u32>,
    pub level_start_band: Option<String>,
    pub level_end_band: Option<String>,
    /// How many characters the session's levels followed: another
    /// character (a level taken with another name, or a lower level)
    /// starts the levels and the class over, so that they are the last
    /// one's — never "167 → 9" of two (0: no level was read, or a record
    /// kept before there was this count).
    pub characters: u32,
    /// The class: a name from [`CLASSES`] ("Night Lord"), or
    /// [`OTHER_CLASS`] when it was read or said as other words — never the
    /// words themselves.
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

impl SessionStats {
    /// The record as it may be kept: its class a name from [`CLASSES`] or
    /// [`OTHER_CLASS`] — a record kept before there was a list holds the
    /// class as it was read, and is shown, shared and written again only
    /// so.
    fn tidy(mut self) -> SessionStats {
        self.job = self.job.as_deref().map(class_or_other);
        self
    }
}

/// Whether the player shares, since when, and the install id made when
/// they turned it on (`metrics/share.json`, which exists only while it is
/// on: no file is off). Off unless turned on.
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

/// Whether this build offers sharing at all. It does not: sharing play
/// data with anyone needs an approved basis first — a rights review for
/// MapleStory and MapleStory Worlds (Nexon restricts commercial use of
/// gameplay), consent by purpose with a receipt, and proportionate age
/// assurance (the owner's data-program rules, 10 Oct 2026;
/// `docs/data-and-metrics.md`). Until then the stats stay on the PC: the
/// program withdraws any earlier choice at start, and the phone link
/// refuses to turn sharing on ([`crate::phone::Hub::offer_sharing`]). The
/// mechanism below stays, tested, for when a basis is approved.
pub const SHARING_OFFERED: bool = false;

/// What the phone's card shows: whether sharing is offered at all
/// (`available`), whether the player shares (and since when), and what is
/// shared — or, while it is off, a preview of what would be (`preview`),
/// with no id. Serialized as it is, so that the export keeps its fields'
/// order on the phone.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ShareView {
    pub available: bool,
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
    pub characters: u32,
    /// A name from [`CLASSES`], or [`OTHER_CLASS`].
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
            characters: record.characters,
            job: record.job.as_deref().map(class_or_other),
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

/// A map's name as kept: its words, trimmed, and only when they look like
/// the game's — not too long, no path ([`looks_like_a_path`]: the game's
/// own " / ", as in "Victoria Road / Ellinia", is no path), no `@` (an
/// address), no link — and contain none of `names` (the character's
/// names, lowercased).
fn game_text(text: &str, names: &[String]) -> Option<String> {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let length = text.chars().count();
    let lower = text.to_lowercase();
    let looks_like_the_games = (2..=MAX_GAME_TEXT).contains(&length)
        && !looks_like_a_path(&text)
        && !text
            .chars()
            .any(|c| matches!(c, '@' | '=' | '\u{0}'..='\u{1f}'))
        && !lower.contains("http")
        && !lower.contains("www.")
        && !names.iter().any(|name| lower.contains(name.as_str()));
    looks_like_the_games.then_some(text)
}

/// Whether `text` (its spaces single) looks like a file's path: a
/// backslash (`C:\Users\…`, `\\server\…`), a `/` or a `~` first (`/home`,
/// `~/x`), or a `/` with no space on either side (`Users/me`, `C:/x`).
fn looks_like_a_path(text: &str) -> bool {
    let chars: Vec<char> = text.chars().collect();
    let spaced = |at: Option<&char>| at == Some(&' ');
    text.contains('\\')
        || text.starts_with(['/', '~'])
        || chars.iter().enumerate().any(|(i, &c)| {
            c == '/'
                && !spaced(i.checked_sub(1).and_then(|j| chars.get(j)))
                && !spaced(chars.get(i + 1))
        })
}

/// Why sharing could not be changed. The phone is told [`ShareError::code`]
/// and has the words for each, in its own language.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShareError {
    /// The operating system gave no randomness for an install id: sharing
    /// stays off.
    NoId,
    /// The choice could not be saved (`share.json`): sharing stays off.
    NotSaved,
    /// Sharing turned off, but these files (their names, in `metrics/`)
    /// are still on this PC: another program may be holding them.
    NotDeleted(Vec<String>),
}

impl ShareError {
    /// What the phone is told: a code, not words.
    pub fn code(&self) -> &'static str {
        match self {
            ShareError::NoId => "no_id",
            ShareError::NotSaved => "not_saved",
            ShareError::NotDeleted(_) => "not_deleted",
        }
    }
}

/// The file of a session under way: its own, `current-<session>.json`, so
/// that two copies of MapleSyrup at once never write over each other's.
fn snapshot_file(session: &str) -> String {
    let session: String = session
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect();
    format!("current-{session}.json")
}

/// The files under `metrics/` in the settings folder, shared by the main
/// loop (which writes the session's record) and the phone link (which
/// shows the stats and turns sharing on and off). One at a time — and one
/// copy of MapleSyrup at a time (see [`Store::guard`]).
pub struct Store {
    dir: PathBuf,
    lock: Mutex<()>,
}

/// Held while the files are read and written (see [`Store::guard`]); the
/// folder's lock is let go of first (fields drop in order).
struct Held<'a> {
    _folder: Option<std::fs::File>,
    _here: MutexGuard<'a, ()>,
}

/// The half-written file of `name`: this process's own, so that another
/// copy of MapleSyrup never writes into it, nor puts it in place.
fn partial(name: &str) -> String {
    format!("{name}.{}.partial", std::process::id())
}

impl Store {
    /// The store in the settings folder `settings` (`metrics/` in it).
    pub fn new(settings: &Path) -> Arc<Store> {
        Arc::new(Store {
            dir: settings.join("metrics"),
            lock: Mutex::new(()),
        })
    }

    /// The files to this one alone, until what it returns is dropped: this
    /// process's lock, and the folder's — `metrics/.lock`, locked, which a
    /// second copy of MapleSyrup (another process) waits for, so that two
    /// copies ending at once never read the records both and each write
    /// back its own (w38's p38d: half the sessions lost, and "kept" said
    /// of some). Held for milliseconds; the system lets go of it with the
    /// handle, and when a copy dies. (A folder where no lock file can be
    /// had is one where nothing can be written either: this one's own
    /// lock, then, as before.)
    fn guard(&self) -> Held<'_> {
        let here = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        let folder = std::fs::create_dir_all(&self.dir)
            .and_then(|()| {
                std::fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .create(true)
                    .truncate(false)
                    .open(self.path(".lock"))
            })
            .and_then(|file| file.lock().map(|()| file))
            .ok();
        Held {
            _folder: folder,
            _here: here,
        }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }

    /// Write `text` to `name` whole or not at all: beside it (in this
    /// process's own half-written file), then put in its place (what a
    /// write that failed left half done is deleted). Only under
    /// [`Store::guard`].
    fn write(&self, name: &str, text: &str) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.dir)?;
        let partial = self.path(&partial(name));
        let written = std::fs::write(&partial, text)
            .and_then(|()| std::fs::rename(&partial, self.path(name)));
        if written.is_err() {
            let _ = std::fs::remove_file(&partial);
        }
        written
    }

    /// `record` written to the file `name`; whether it was.
    fn write_record(&self, name: &str, record: &SessionStats) -> bool {
        serde_json::to_string(record).is_ok_and(|text| self.write(name, &text).is_ok())
    }

    /// The records kept, the oldest first — a line that cannot be read is
    /// skipped, and no file yet is none — or the error that kept the file
    /// from being read at all: that is not "none", and the file is not to
    /// be written over.
    fn read_kept(&self) -> std::io::Result<Vec<SessionStats>> {
        let bytes = match std::fs::read(self.path("sessions.jsonl")) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e),
        };
        // (Lossily: a torn line costs that line, not the others.)
        Ok(String::from_utf8_lossy(&bytes)
            .lines()
            .filter_map(|line| serde_json::from_str::<SessionStats>(line).ok())
            .map(SessionStats::tidy)
            .collect())
    }

    /// The records kept, the oldest first (none when they cannot be read).
    fn kept(&self) -> Vec<SessionStats> {
        self.read_kept().unwrap_or_default()
    }

    /// `record` among the records kept, in place of the one of its session
    /// (its snapshot, recovered), the oldest dropped past
    /// [`KEEP_RECORDS`]. Returns whether it landed: when the records kept
    /// cannot be read, nothing is written (over them, with this one), and
    /// a write that failed is no landing.
    fn keep(&self, record: &SessionStats) -> bool {
        let Ok(mut records) = self.read_kept() else {
            return false;
        };
        records.retain(|r| r.session != record.session);
        records.push(record.clone().tidy());
        let oldest = records.len().saturating_sub(KEEP_RECORDS);
        let text: String = records[oldest..]
            .iter()
            .filter_map(|r| serde_json::to_string(r).ok())
            .map(|line| line + "\n")
            .collect();
        self.write("sessions.jsonl", &text).is_ok()
    }

    /// The snapshots of sessions under way, or that never finished: every
    /// `current-<session>.json` — and `current.json`, from before there was
    /// one per session — the last written last.
    fn snapshot_files(&self) -> Vec<String> {
        let mut files: Vec<(Option<std::time::SystemTime>, String)> = std::fs::read_dir(&self.dir)
            .map(|entries| {
                entries
                    .filter_map(|entry| {
                        let entry = entry.ok()?;
                        let name = entry.file_name().into_string().ok()?;
                        let snapshot = name == "current.json"
                            || (name.starts_with("current-") && name.ends_with(".json"));
                        let written = entry.metadata().ok().and_then(|m| m.modified().ok());
                        snapshot.then_some((written, name))
                    })
                    .collect()
            })
            .unwrap_or_default();
        files.sort();
        files.into_iter().map(|(_, name)| name).collect()
    }

    /// The session in the snapshot `name` — None when there is none in it
    /// (and never will be) — or the error that kept it from being read now.
    fn read_snapshot(&self, name: &str) -> std::io::Result<Option<SessionStats>> {
        let bytes = std::fs::read(self.path(name))?;
        Ok(serde_json::from_slice::<SessionStats>(&bytes)
            .ok()
            .map(SessionStats::tidy))
    }

    /// The records kept and the sessions under way (a session's snapshot
    /// in place of its record, when one was kept: the snapshot is the
    /// newer), the newest last.
    fn all(&self) -> Vec<SessionStats> {
        let mut records = self.kept();
        for name in self.snapshot_files() {
            if let Ok(Some(record)) = self.read_snapshot(&name) {
                match records.iter_mut().find(|r| r.session == record.session) {
                    Some(kept) => *kept = record,
                    None => records.push(record),
                }
            }
        }
        records
    }

    /// The session under way, as it stands, in its own file
    /// (`current-<session>.json`). Returns whether it was written.
    pub fn snapshot(&self, record: &SessionStats) -> bool {
        let _guard = self.guard();
        self.write_record(&snapshot_file(&record.session), record)
    }

    /// The session ended: its record joins the others, in place of its
    /// snapshot — which is deleted once the record has landed, and is
    /// otherwise left, as the session ended, for the next start to
    /// recover. The export is rebuilt when sharing is on. Returns whether
    /// the record landed.
    pub fn finish(&self, record: &SessionStats) -> bool {
        let _guard = self.guard();
        let own = snapshot_file(&record.session);
        let kept = self.keep(record);
        let gone = kept && {
            let path = self.path(&own);
            std::fs::remove_file(&path).is_ok() || !path.exists()
        };
        if !gone {
            // (Not kept: the next start keeps it. Kept, but the snapshot
            // stayed: the same record, so that keeping it again changes
            // nothing.)
            self.write_record(&own, record);
        }
        self.rebuild();
        kept
    }

    /// The sessions that never finished — a crash, a console window closed
    /// with the X, a record that could not be kept — join the others: every
    /// snapshot but `own`'s (this session's). Each is deleted once its
    /// record has landed, and left for the next start when it has not (one
    /// with no session in it is deleted). The export is rebuilt when
    /// sharing is on. Returns how many joined.
    pub fn recover(&self, own: &str) -> usize {
        let _guard = self.guard();
        let own = snapshot_file(own);
        let mut joined = 0;
        for name in self.snapshot_files() {
            if name == own {
                continue;
            }
            match self.read_snapshot(&name) {
                Ok(Some(record)) => {
                    if self.keep(&record) {
                        let _ = std::fs::remove_file(self.path(&name));
                        joined += 1;
                    }
                }
                Ok(None) => {
                    let _ = std::fs::remove_file(self.path(&name));
                }
                // (Not readable now — held by another program: next time.)
                Err(_) => {}
            }
        }
        if joined > 0 {
            self.rebuild();
        }
        joined
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
    /// same id and day stand. Off, as "Delete it" ([`Store::delete_shared`]).
    pub fn set_sharing(&self, on: bool, today: NaiveDate) -> Result<Consent, ShareError> {
        let _guard = self.guard();
        if !on {
            return self.turn_off().map(|()| Consent::default());
        }
        let consent = self.read_consent();
        if consent.on {
            return Ok(consent);
        }
        let id = new_install_id().ok_or(ShareError::NoId)?;
        let consent = Consent {
            on: true,
            since: Some(today.format("%Y-%m-%d").to_string()),
            id: Some(id),
        };
        let text = serde_json::to_string_pretty(&consent).map_err(|_| ShareError::NotSaved)?;
        self.write("share.json", &text)
            .map_err(|_| ShareError::NotSaved)?;
        self.rebuild();
        Ok(consent)
    }

    /// "Delete it": sharing off — the choice (`share.json`: no file is
    /// off), the export with its install id, and any half-written file
    /// deleted. An error names the files still on this PC.
    pub fn delete_shared(&self) -> Result<Consent, ShareError> {
        let _guard = self.guard();
        self.turn_off().map(|()| Consent::default())
    }

    /// Sharing off: `share.json`, `share-export.json` and every `*.partial`
    /// (what a write that failed left half done, which may hold the id)
    /// deleted, then looked for: an error names what is still there. No
    /// copy's write is under way meanwhile — every write is made under the
    /// folder's lock, held here — so a `*.partial` found is one a write
    /// left when its copy died (or an older version's `<name>.partial`).
    /// (A copy that had no lock to take: its rename then fails, it says
    /// "not kept", and its snapshot is kept at the next start.)
    fn turn_off(&self) -> Result<(), ShareError> {
        let mut files = vec!["share.json".to_string(), "share-export.json".to_string()];
        files.extend(
            std::fs::read_dir(&self.dir)
                .map(|entries| {
                    entries
                        .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
                        .filter(|name| name.ends_with(".partial"))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default(),
        );
        let mut left = Vec::new();
        for name in files {
            let path = self.path(&name);
            if std::fs::remove_file(&path).is_err() {
                // (A folder by that name goes too, when it is empty.)
                let _ = std::fs::remove_dir(&path);
            }
            if std::fs::symlink_metadata(&path).is_ok() {
                left.push(name);
            }
        }
        if left.is_empty() {
            Ok(())
        } else {
            Err(ShareError::NotDeleted(left))
        }
    }

    /// The export rebuilt from the records since sharing was turned on —
    /// only while it is on. While it is off there is none: one left behind
    /// by a deletion that failed goes now.
    fn rebuild(&self) {
        let consent = self.read_consent();
        if !consent.on {
            let _ = std::fs::remove_file(self.path("share-export.json"));
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
                available: SHARING_OFFERED,
                on: true,
                since: consent.since,
                preview: false,
                export,
            };
        }
        let records = self.all();
        let last = &records[records.len().saturating_sub(PREVIEW_SESSIONS)..];
        ShareView {
            available: SHARING_OFFERED,
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
    /// The class as last read (held here, never written: what is kept is
    /// a name from [`CLASSES`], or [`OTHER_CLASS`]).
    job_read: Option<String>,
    /// The level the companion last took, the character's name then
    /// (lowercased; held here, never written), and the level-ups the
    /// companion had counted when the last character's began.
    level_taken: Option<u32>,
    character: Option<String>,
    levels_before: u32,
    /// Time to the first words of each reply, in milliseconds.
    latencies: Vec<u32>,
}

impl Stats {
    /// The session's stats, starting now, kept in the settings folder
    /// `settings`. The sessions that never finished (a crash, a closed
    /// window, a record that could not be kept) join the others first.
    pub fn start(settings: &Path) -> Stats {
        let store = Store::new(settings);
        let session = crate::phone::tls::random_hex(8);
        store.recover(&session);
        let sharing = store.consent().on;
        let commit: String = env!("MS_COMMIT")
            .chars()
            .take_while(|c| c.is_ascii_hexdigit())
            .take(12)
            .collect();
        Stats {
            record: SessionStats {
                session,
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
            job_read: None,
            level_taken: None,
            character: None,
            levels_before: 0,
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
        self.follow_character(obs, companion);
        if let Some(read) = obs.job.as_deref().map(str::trim).filter(|j| !j.is_empty())
            && self.job_read.as_deref() != Some(read)
        {
            self.job_read = Some(read.to_string());
            // A class's name; else "other" — which never takes the place of
            // a class read before (a misread since, the player's words).
            match class_of(read) {
                Some(class) => self.record.job = Some(class.to_string()),
                None => {
                    self.record
                        .job
                        .get_or_insert_with(|| OTHER_CLASS.to_string());
                }
            }
        }
        let hp = obs.hp;
        if self.close_calls.track(at, hp, companion.dead()).is_some() {
            self.record.close_calls += 1;
        }
        self.tally = companion.tally();
        // (EXP per hour is worked out from a quarter hour of samples, a
        // thousand pairs of them: read every few seconds, not every frame.)
        if at - self.progress_at >= PROGRESS_EVERY {
            self.progress_at = at;
            let progress = companion.progress();
            self.record.levels_gained = progress.levels_gained.saturating_sub(self.levels_before);
            if let Some(rate) = progress.exp_per_hour.filter(|r| r.is_finite()) {
                self.record.exp_per_hour = Some(((rate * 100.0).round() / 100.0) as f32);
            }
            self.record.attitude = companion.settings.attitude.word().to_string();
        }
    }

    /// The level, as the companion takes it, and whose it is. Another
    /// character — a level taken with another name than the last one's,
    /// or a lower level: the companion's own "another character" — starts
    /// the levels and the class over, and is counted (`characters`): a
    /// session is one run of MapleSyrup, its minutes, words and errors
    /// the run's, but its levels and class are a character's, the last
    /// one played.
    fn follow_character(&mut self, obs: &Observation, companion: &Companion) {
        // (Only when it is needed: a level taken, or no name yet.)
        let name = || {
            obs.name
                .as_deref()
                .map(|n| n.trim().to_lowercase())
                .filter(|n| !n.is_empty())
        };
        let level = companion.level();
        if level != self.level_taken {
            let name = name();
            if let (Some(before), Some(now)) = (self.level_taken, level) {
                let renamed =
                    matches!((&self.character, &name), (Some(then), Some(seen)) if then != seen);
                if renamed || now < before {
                    self.record.characters += 1;
                    self.record.level_start = None;
                    self.record.levels_gained = 0;
                    self.levels_before = companion.so_far().level_ups;
                    self.record.job = None;
                    self.job_read = None;
                }
            }
            self.level_taken = level;
            if name.is_some() {
                self.character = name;
            }
        } else if self.character.is_none() {
            self.character = name();
        }
        if let Some(level) = level {
            self.record.characters = self.record.characters.max(1);
            self.record.level_start.get_or_insert(level);
            self.record.level_end = Some(level);
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
    /// the phone's locale). The maps' names are checked once more against
    /// every name known by now, and the class against the list.
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
        record.job = record.job.as_deref().map(class_or_other);
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

    /// The session ended: its record kept — or, when it cannot be, left
    /// for the next start — and the export rebuilt when sharing is on.
    /// Returns whether it was kept.
    pub fn finish(&mut self, coach: &Coach, language: Option<&str>) -> bool {
        self.store.finish(&self.record(coach, language))
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
            characters: 1,
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
        // Off: the choice, the export and the id are deleted (no
        // `share.json` is off).
        assert_eq!(
            store.set_sharing(false, day("2026-10-21")),
            Ok(Consent::default())
        );
        assert!(!export.exists());
        assert!(store.export().is_none());
        assert!(!settings.join("metrics").join("share.json").exists());
        // On again: a new id. "Delete it": off, and the export and the id gone.
        let again = store.set_sharing(true, day("2026-10-22")).unwrap();
        assert_ne!(again.id, on.id);
        assert!(export.exists());
        assert_eq!(store.delete_shared(), Ok(Consent::default()));
        assert!(!export.exists());
        assert_eq!(store.consent(), Consent::default());
        // The stats themselves stay (they never leave the PC).
        assert_eq!(store.all().len(), 2);
        let _ = std::fs::remove_dir_all(&settings);
    }

    #[test]
    fn a_session_is_written_as_it_goes_and_kept_after_a_crash_once() {
        let settings = temp_dir("crash");
        // A session under way writes its snapshot every minute, to its own
        // file.
        let mut stats = Stats::start(&settings);
        let coach = Coach::new(true);
        let current = settings
            .join("metrics")
            .join(format!("current-{}.json", stats.record.session));
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
        let current = settings
            .join("metrics")
            .join(format!("current-{}.json", stats.record.session));
        stats.phone(61.0, false, false);
        stats.save_every(&coach, None);
        assert!(current.exists());
        stats.said();
        assert!(stats.finish(&coach, None));
        let kept = Store::new(&settings).kept();
        assert_eq!(kept.len(), 2);
        assert_eq!(kept[1].sentences, 1);
        assert!(!current.exists());
        // A snapshot from before there was one per session (`current.json`)
        // is kept at the next start too.
        let older = full_record("s-older", "2026-10-08");
        std::fs::write(
            settings.join("metrics").join("current.json"),
            serde_json::to_string(&older).unwrap(),
        )
        .unwrap();
        let _later = Stats::start(&settings);
        assert!(!settings.join("metrics").join("current.json").exists());
        assert_eq!(Store::new(&settings).kept().len(), 3);
        let _ = std::fs::remove_dir_all(&settings);
    }

    #[test]
    fn a_session_whose_record_cannot_be_kept_is_kept_at_the_next_start() {
        // `sessions.jsonl` cannot be written (on Windows: another program
        // holds it; here a folder where its half-written file goes — w32's
        // p21 C1): the session's end leaves its snapshot, as the session
        // ended, and the phone still has it; the next start that can keep
        // it does, once.
        let settings = temp_dir("unkept");
        let metrics = settings.join("metrics");
        let coach = Coach::new(true);
        let mut stats = Stats::start(&settings);
        let own = metrics.join(format!("current-{}.json", stats.record.session));
        stats.phone(90.0, true, false);
        stats.said();
        stats.save_every(&coach, None);
        stats.said();
        std::fs::create_dir_all(metrics.join(partial("sessions.jsonl"))).unwrap();
        assert!(!stats.finish(&coach, None));
        let left: SessionStats = serde_json::from_slice(&std::fs::read(&own).unwrap()).unwrap();
        assert_eq!(left.sentences, 2, "the session as it ended");
        let shown = Store::new(&settings).stats_view(SHOWN_SESSIONS).sessions;
        assert_eq!(shown.len(), 1);
        assert_eq!(shown[0].sentences, 2);
        // The next start cannot keep it either (p21 C1b): still left.
        drop(Stats::start(&settings));
        assert!(own.exists());
        assert_eq!(Store::new(&settings).kept().len(), 0);
        // Once it can be written, the next start keeps it, once.
        std::fs::remove_dir(metrics.join(partial("sessions.jsonl"))).unwrap();
        let third = Stats::start(&settings);
        assert!(!own.exists());
        let kept = Store::new(&settings).kept();
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].sentences, 2);
        assert_eq!(third.store().stats_view(SHOWN_SESSIONS).sessions.len(), 1);
        let _ = std::fs::remove_dir_all(&settings);
    }

    #[test]
    fn a_torn_line_in_the_kept_records_costs_that_line_not_the_others() {
        // A record cut off half way (a power cut mid-write on an older
        // copy; a byte that is not text): the others stay when the next
        // session joins them — never all of them written over with it.
        let settings = temp_dir("torn");
        let metrics = settings.join("metrics");
        std::fs::create_dir_all(&metrics).unwrap();
        let mut bytes = Vec::new();
        for session in ["s0", "s1"] {
            bytes.extend(serde_json::to_vec(&full_record(session, "2026-10-08")).unwrap());
            bytes.push(b'\n');
        }
        bytes.extend(b"{\"session\":\"s2\",\"day\":\"2026-10-0\xff\xfe");
        bytes.push(b'\n');
        std::fs::write(metrics.join("sessions.jsonl"), bytes).unwrap();
        let store = Store::new(&settings);
        assert!(store.finish(&full_record("s3", "2026-10-09")));
        let sessions: Vec<String> = store.kept().into_iter().map(|r| r.session).collect();
        assert_eq!(sessions, ["s0", "s1", "s3"]);
        let _ = std::fs::remove_dir_all(&settings);
    }

    #[test]
    fn two_copies_at_once_keep_both_sessions_and_neither_twice() {
        // Two copies of MapleSyrup on one settings folder (w32's p21 C2):
        // each writes its own snapshot; the second's start keeps the
        // first's (running, so its end replaces it); the first's end
        // deletes only its own; the second, closed with the X, is kept at
        // the next start.
        let settings = temp_dir("two");
        let metrics = settings.join("metrics");
        let coach = Coach::new(true);
        let mut a = Stats::start(&settings);
        a.phone(61.0, true, false);
        a.said();
        a.save_every(&coach, None);
        let mut b = Stats::start(&settings);
        b.phone(61.0, true, false);
        for _ in 0..5 {
            b.said();
        }
        b.save_every(&coach, None);
        a.phone(130.0, true, false);
        assert!(a.finish(&coach, None));
        let snapshots = || {
            std::fs::read_dir(&metrics)
                .unwrap()
                .filter(|e| {
                    let name = e.as_ref().unwrap().file_name();
                    name.to_string_lossy().starts_with("current")
                })
                .count()
        };
        assert_eq!(
            snapshots(),
            1,
            "the first copy's end took the second's snapshot"
        );
        drop(b);
        let _c = Stats::start(&settings);
        assert_eq!(snapshots(), 0);
        let mut sentences: Vec<u32> = Store::new(&settings)
            .kept()
            .iter()
            .map(|r| r.sentences)
            .collect();
        sentences.sort_unstable();
        assert_eq!(sentences, [1, 5]);
        let _ = std::fs::remove_dir_all(&settings);
    }

    #[test]
    fn two_copies_ending_at_the_same_moment_lose_no_session_and_kept_means_kept() {
        // w38's p38d: two copies of MapleSyrup are two processes, and two
        // `Store`s on one folder are as two (each its own in-process lock
        // and its own handle on `metrics/.lock`). A hundred pairs of
        // sessions ending at the same instant (a Windows shutdown closing
        // both): at 03303a9 101–102 of the 200 were kept, and `finish()`
        // said "kept" of some that were not (A's rename moved B's file,
        // written over A's at the one shared `sessions.jsonl.partial`).
        let settings = temp_dir("race");
        let barrier = Arc::new(std::sync::Barrier::new(2));
        let copies: Vec<_> = ["A", "B"]
            .into_iter()
            .map(|who| {
                let (store, barrier) = (Store::new(&settings), barrier.clone());
                std::thread::spawn(move || {
                    (0..100)
                        .filter(|i| {
                            let record = SessionStats {
                                session: format!("{who}{i:03}"),
                                day: "2026-10-10".into(),
                                ..Default::default()
                            };
                            store.snapshot(&record);
                            barrier.wait();
                            store.finish(&record)
                        })
                        .count()
                })
            })
            .collect();
        let said_kept: usize = copies.into_iter().map(|c| c.join().unwrap()).sum();
        let store = Store::new(&settings);
        assert_eq!(store.kept().len(), 200);
        assert_eq!(said_kept, 200);
        assert_eq!(store.snapshot_files(), Vec::<String>::new());
        let _ = std::fs::remove_dir_all(&settings);
    }

    #[test]
    fn a_copy_waits_for_the_other_and_deletes_only_what_no_write_is_making() {
        // While one copy reads or writes the files, the other waits (the
        // lock is the folder's, not the process's): its "Delete it" comes
        // after the write in flight, never in the middle of it.
        let settings = temp_dir("wait");
        let metrics = settings.join("metrics");
        let (a, b) = (Store::new(&settings), Store::new(&settings));
        a.finish(&full_record("s0", "2026-10-09"));
        let held = a.guard();
        let other = std::thread::spawn(move || b.delete_shared());
        std::thread::sleep(Duration::from_millis(300));
        assert!(!other.is_finished(), "the other copy did not wait");
        drop(held);
        assert_eq!(other.join().unwrap(), Ok(Consent::default()));
        // A write's half-written file is its own process's: another copy's
        // (or an older version's) left by a write that died half way is
        // deleted with sharing; this one's never collides with it.
        let theirs = metrics.join("share-export.json.4242.partial");
        std::fs::write(&theirs, "half").unwrap();
        std::fs::create_dir_all(metrics.join("sessions.jsonl.4242.partial")).unwrap();
        assert!(a.finish(&full_record("s1", "2026-10-10")));
        assert_eq!(a.kept().len(), 2);
        assert_eq!(a.delete_shared(), Ok(Consent::default()));
        assert!(!theirs.exists());
        assert!(!metrics.join("sessions.jsonl.4242.partial").exists());
        let _ = std::fs::remove_dir_all(&settings);
    }

    #[test]
    fn turning_sharing_off_deletes_every_file_of_it_and_never_fails_silently() {
        let settings = temp_dir("withdraw");
        let metrics = settings.join("metrics");
        let store = Store::new(&settings);
        store.finish(&full_record("s0", "2026-10-09"));
        let id = store
            .set_sharing(true, day("2026-10-01"))
            .unwrap()
            .id
            .unwrap();
        // What writes that failed half way left behind, with the id in it;
        // and `share.json` that cannot be written (a folder where its
        // half-written file goes: w32's p21 C3, where "Delete it" said
        // "Deleted" and the next session rebuilt the export under the id).
        std::fs::write(metrics.join("share-export.json.partial"), &id).unwrap();
        std::fs::create_dir_all(metrics.join("share.json.partial")).unwrap();
        assert_eq!(store.delete_shared(), Ok(Consent::default()));
        assert_eq!(store.consent(), Consent::default());
        for name in [
            "share.json",
            "share-export.json",
            "share-export.json.partial",
            "share.json.partial",
        ] {
            assert!(!metrics.join(name).exists(), "{name} is still there");
        }
        // The next session's end rebuilds nothing, and no file holds the id.
        store.finish(&full_record("s1", "2026-10-10"));
        assert!(!metrics.join("share-export.json").exists());
        assert!(store.export().is_none());
        for entry in std::fs::read_dir(&metrics).unwrap() {
            let path = entry.unwrap().path();
            let text = std::fs::read_to_string(&path).unwrap_or_default();
            assert!(!text.contains(&id), "{} holds the id", path.display());
        }
        // A file that cannot be deleted (held by another program; here a
        // folder that is not empty in the export's place): turning sharing
        // off says so, naming it — the choice itself is gone, so sharing is
        // off — and "Delete it" says so again until it can go.
        store.set_sharing(true, day("2026-10-11")).unwrap();
        let export = metrics.join("share-export.json");
        std::fs::remove_file(&export).unwrap();
        std::fs::create_dir_all(export.join("held")).unwrap();
        let left = Err(ShareError::NotDeleted(vec!["share-export.json".into()]));
        assert_eq!(store.set_sharing(false, day("2026-10-12")), left);
        assert_eq!(store.consent(), Consent::default());
        assert_eq!(store.delete_shared(), left);
        std::fs::remove_dir_all(&export).unwrap();
        assert_eq!(store.delete_shared(), Ok(Consent::default()));
        // (And `.lock`, the copies' lock: empty, never written.)
        let mut left: Vec<String> = std::fs::read_dir(&metrics)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        left.sort();
        assert_eq!(left, [".lock", "sessions.jsonl"]);
        assert_eq!(std::fs::metadata(metrics.join(".lock")).unwrap().len(), 0);
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
        // A path is no map; the game's own " / " is (w32's S5: his Classic
        // maps were never counted).
        assert_eq!(
            game_text("Victoria Road  /  Ellinia", &names).as_deref(),
            Some("Victoria Road / Ellinia")
        );
        for path in [
            r"\\pc\share",
            "/home/me",
            "~/maps",
            "Users/me",
            "C:/Users",
            "Documents/",
            "Road /x/y",
        ] {
            assert_eq!(game_text(path, &names), None, "{path}");
        }
        let a = new_install_id().unwrap();
        let b = new_install_id().unwrap();
        assert_ne!(a, b);
        assert!(
            a.chars().all(|c| c.is_ascii_hexdigit() || c == '-') && a.matches('-').count() == 4,
            "{a}"
        );
    }

    #[test]
    fn the_class_is_a_name_from_the_list_or_other_never_the_words() {
        // w32's p21 B: the class as read off the screen (the name beside it,
        // misread, a guild tag, a chat line) or as the player said it, his
        // name known as WanWanBoggio. A record, its file and the export hold
        // a class's name or "other" — never the words.
        let coach = Coach::new(true);
        for (read, kept) in [
            ("Night Lord", "Night Lord"),
            ("  night   LORD ", "Night Lord"),
            ("נייט לורד", "Night Lord"),
            ("Night Lord WanWan", OTHER_CLASS),
            ("WanWanBoggi Night Lord", OTHER_CLASS),
            ("WANWANBUJIO Beginner", OTHER_CLASS),
            ("Bishop, my girlfriend's account", OTHER_CLASS),
            ("Hero from Moshav Livnim", OTHER_CLASS),
            ("Night Lord [Guild: Livnim]", OTHER_CLASS),
            ("Michael's thief", OTHER_CLASS),
            ("WanWan: selling pots 5m DM me", OTHER_CLASS),
        ] {
            let settings = temp_dir("class");
            let mut stats = Stats::start(&settings);
            let mut companion = Companion::seeded(Settings::default(), 7);
            for i in 0..20 {
                let t = i as f64 * 0.1;
                let obs = seen(90.0, 167, "WanWanBoggio", read, "MapleStory");
                companion.observe(t, obs.clone());
                stats.frame(t, &obs, &companion);
            }
            assert_eq!(
                stats.record(&coach, None).job.as_deref(),
                Some(kept),
                "{read}"
            );
            stats.finish(&coach, Some("he-IL"));
            let store = Store::new(&settings);
            store.set_sharing(true, day("2000-01-01")).unwrap();
            let export = store.export().unwrap();
            assert_eq!(export.sessions[0].job.as_deref(), Some(kept), "{read}");
            let files: String = ["sessions.jsonl", "share-export.json"]
                .iter()
                .map(|f| std::fs::read_to_string(settings.join("metrics").join(f)).unwrap())
                .collect();
            if read.trim() != kept {
                assert!(!files.contains(read.trim()), "{read:?} in {files}");
            }
            for word in ["WanWan", "Livnim", "Michael", "girlfriend", "DM me", "נייט"] {
                assert!(!files.contains(word), "{word:?} in {files}");
            }
            let _ = std::fs::remove_dir_all(&settings);
        }
        // A class read before stays when other words are read after it (a
        // misread, the player's words); "other" only when no class was.
        let settings = temp_dir("class-kept");
        let mut stats = Stats::start(&settings);
        let companion = Companion::seeded(Settings::default(), 7);
        for (i, read) in [
            "Hermit",
            "Night Lord [Guild: Livnim]",
            "Night Lord",
            "Michael's thief",
        ]
        .iter()
        .enumerate()
        {
            let obs = seen(90.0, 120, "WanWanBoggio", read, "MapleStory");
            stats.frame(i as f64, &obs, &companion);
        }
        assert_eq!(
            stats.record(&coach, None).job.as_deref(),
            Some("Night Lord")
        );
        let _ = std::fs::remove_dir_all(&settings);
    }

    #[test]
    fn the_classes_are_matched_whole_in_any_case_spacing_and_in_hebrew() {
        for (name, also) in CLASSES {
            assert_eq!(class_of(name), Some(*name));
            assert_eq!(class_of(&name.to_uppercase()), Some(*name));
            for said in *also {
                assert_eq!(class_of(said), Some(*name), "{said}");
            }
        }
        // No two classes are written alike.
        let mut written = HashMap::new();
        for (name, also) in CLASSES {
            for said in std::iter::once(name).chain(also.iter()) {
                if let Some(other) = written.insert(class_key(said), *name) {
                    assert_eq!(other, *name, "{said:?} is two classes'");
                }
            }
        }
        for (said, class) in [
            ("night-lord", Some("Night Lord")),
            ("NightLord", Some("Night Lord")),
            ("\u{200f}נייט לורד\u{200f}", Some("Night Lord")),
            ("פייג׳", Some("Page")),
            ("Arch Mage (Fire, Poison)", Some("Arch Mage (F/P)")),
            ("arch mage(i/l)", Some("Arch Mage (I/L)")),
            ("Crossbow Master", Some("Marksman")),
            ("Rogue", Some("Thief")),
            ("Dragon Knight", Some("Dragon Knight")),
            ("Night Lords", None),
            ("Lord", None),
            ("", None),
            ("   ", None),
        ] {
            assert_eq!(class_of(said), class, "{said:?}");
        }
        assert_eq!(class_or_other("Michael's thief"), OTHER_CLASS);
        assert_eq!(class_or_other(OTHER_CLASS), OTHER_CLASS);
    }

    #[test]
    fn a_class_kept_before_the_list_is_shown_and_shared_as_a_name_or_other() {
        // A record kept by a copy from before the list holds the class as it
        // was read: the phone, the export and the file written next hold
        // its name, or "other".
        let settings = temp_dir("class-older");
        let metrics = settings.join("metrics");
        std::fs::create_dir_all(&metrics).unwrap();
        let mut older = full_record("s0", "2026-10-09");
        older.job = Some("Michael's thief".into());
        let mut named = full_record("s1", "2026-10-09");
        named.job = Some("night lord".into());
        let lines: String = [older, named]
            .iter()
            .map(|r| serde_json::to_string(r).unwrap() + "\n")
            .collect();
        std::fs::write(metrics.join("sessions.jsonl"), lines).unwrap();
        let store = Store::new(&settings);
        let shown: Vec<Option<String>> = store
            .stats_view(SHOWN_SESSIONS)
            .sessions
            .into_iter()
            .map(|s| s.job)
            .collect();
        assert_eq!(shown, [Some("Night Lord".into()), Some(OTHER_CLASS.into())]);
        let preview = store.share_view().export.unwrap();
        let jobs: Vec<Option<&str>> = preview.sessions.iter().map(|s| s.job.as_deref()).collect();
        assert_eq!(jobs, [Some(OTHER_CLASS), Some("Night Lord")]);
        assert!(store.finish(&full_record("s2", "2026-10-10")));
        let file = std::fs::read_to_string(metrics.join("sessions.jsonl")).unwrap();
        assert!(
            !file.contains("Michael") && !file.contains("night lord"),
            "{file}"
        );
        let _ = std::fs::remove_dir_all(&settings);
    }

    #[test]
    fn a_map_written_with_a_slash_between_spaces_is_counted() {
        // How the sight reads his Classic maps: counted, by name on this PC
        // and as a count in the export.
        let settings = temp_dir("slash");
        let mut stats = Stats::start(&settings);
        let coach = Coach::new(true);
        let sight = Arc::new(Mutex::new(Sight::load(&settings.join("learned"))));
        sight.lock().unwrap().facts.map = Some("Victoria Road / Ellinia".into());
        stats.look(Some(&sight), None);
        let record = stats.record(&coach, None);
        assert_eq!(
            record.maps,
            BTreeMap::from([("Victoria Road / Ellinia".to_string(), 1)])
        );
        assert_eq!(SharedSession::of(&record).unwrap().maps, 1);
        let _ = std::fs::remove_dir_all(&settings);
    }

    #[test]
    fn another_character_starts_the_levels_and_the_class_over() {
        // His log, 11:19:03: "Level 9 now, from 167 (another character)".
        // The record is the last character's levels and class — never
        // "167 → 9" — and counts the characters.
        let settings = temp_dir("characters");
        let mut stats = Stats::start(&settings);
        let mut companion = Companion::seeded(Settings::default(), 7);
        let coach = Coach::new(true);
        let mut t = 0.0;
        let mut play = |secs: f64, level: u32, name: Option<&str>, job: &str| {
            let mut obs = seen(90.0, level, name.unwrap_or(""), job, "MapleStory");
            obs.name = name.map(String::from);
            for _ in 0..(secs * 10.0) as usize {
                companion.observe(t, obs.clone());
                stats.frame(t, &obs, &companion);
                t += 0.1;
            }
            let record = stats.record(&coach, None);
            (
                record.level_start,
                record.level_end,
                record.characters,
                record.job,
                record.levels_gained,
            )
        };
        let class = |name: &str| Some(name.to_string());
        // His main, and a level-up of it: one character.
        assert_eq!(
            play(10.0, 167, Some("WanWanBoggio"), "Night Lord"),
            (Some(167), Some(167), 1, class("Night Lord"), 0)
        );
        assert_eq!(
            play(35.0, 168, Some("WanWanBoggio"), "Night Lord"),
            (Some(167), Some(168), 1, class("Night Lord"), 1)
        );
        // Another character (another name): its levels and its class.
        assert_eq!(
            play(35.0, 9, Some("WanLittle"), "Beginner"),
            (Some(9), Some(9), 2, class("Beginner"), 0)
        );
        assert_eq!(
            play(35.0, 10, Some("WanLittle"), "Beginner"),
            (Some(9), Some(10), 2, class("Beginner"), 1)
        );
        // Back to the first: the third character played.
        assert_eq!(
            play(10.0, 168, Some("WanWanBoggio"), "Night Lord"),
            (Some(168), Some(168), 3, class("Night Lord"), 0)
        );
        // A lower level with no name read: another character too.
        assert_eq!(
            play(40.0, 30, None, "Hermit"),
            (Some(30), Some(30), 4, class("Hermit"), 0)
        );
        let record = stats.record(&coach, None);
        assert_eq!(
            (
                record.level_start_band.as_deref(),
                record.level_end_band.as_deref()
            ),
            (Some("11-30"), Some("11-30"))
        );
        assert_eq!(SharedSession::of(&record).unwrap().characters, 4);
        let _ = std::fs::remove_dir_all(&settings);
    }
}
