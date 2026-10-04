//! What MapleSyrup learns about the player from playing together, kept in
//! `memory.json` in the settings folder and used in every conversation.
//!
//! Nothing has to be taught. Every few minutes, and when MapleSyrup starts
//! (for the sessions before), the learner reads what was said in the
//! session logs and has the model update its notebook: who the player is,
//! how they like to be talked to, the names they use, what happened last
//! time; the player's corrections become lessons (`knowledge`). It also
//! counts how talking goes and adapts to it: how long the phone waits after
//! the words stop, how eager a live call is to answer, how short to keep it.
//!
//! ```text
//!   session logs ──▶ learner (every few minutes) ──▶ the model: update the notebook
//!                       │                                   │
//!                       ├── counts: cut off? talked over? ──┤
//!                       ▼                                   ▼
//!                  memory.json  ◀──────────────  facts, style, words, last time
//!                       │                        lessons ──▶ knowledge.json
//!                       ▼
//!   every reply and every live call: "what you know about them", lessons
//! ```

use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::knowledge::{self, Entry, Knowledge, Source, id_of};
use super::openai::{Ask, OpenAi};

pub const FILE: &str = "memory.json";
/// At most this many facts, style notes and words are kept.
const MOST_FACTS: usize = 40;
const MOST_STYLE: usize = 8;
const MOST_WORDS: usize = 40;
/// At most this much of the conversation goes to one look back.
const MOST_LINES: usize = 300;

/// The fastest the phone may send a sentence after the words stop, the
/// slowest, and where it starts.
pub const SETTLE_MIN: u32 = 400;
pub const SETTLE_MAX: u32 = 1300;
pub const SETTLE_START: u32 = 550;
/// How eager a live call is to answer, most first.
const EAGERNESS: [&str; 3] = ["high", "medium", "low"];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Fact {
    pub id: String,
    pub text: String,
}

impl Fact {
    fn new(text: &str) -> Fact {
        Fact {
            id: id_of(text),
            text: text.to_string(),
        }
    }
}

/// How far the session logs were read: the last session's folder and how
/// many of its lines.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ReadTo {
    pub session: String,
    pub lines: usize,
}

/// How talking goes, counted from the logs.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Counts {
    /// The player's sentences (the regular mode), and how many of them were
    /// sent too soon (they went on talking).
    pub sentences: u32,
    pub continued: u32,
    /// MapleSyrup's replies (the regular mode), and how many the player
    /// talked over.
    pub replies: u32,
    pub talked_over: u32,
    /// MapleSyrup's turns on live calls, and how many started before the
    /// player had finished.
    pub live_turns: u32,
    pub jumped_in: u32,
}

impl Counts {
    fn add(&mut self, other: &Counts) {
        self.sentences += other.sentences;
        self.continued += other.continued;
        self.replies += other.replies;
        self.talked_over += other.talked_over;
        self.live_turns += other.live_turns;
        self.jumped_in += other.jumped_in;
    }
}

/// What it adapted to (or the player asked for).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Adapt {
    /// How long the phone waits after the words stop before sending them, in
    /// milliseconds.
    pub settle_ms: u32,
    /// How eager a live call is to answer: "high", "medium" or "low".
    pub eagerness: String,
    /// They often talk over long answers: keep them very short.
    pub short: bool,
    /// Warn below these percents (0: never); None: the usual.
    pub hp_low: Option<f32>,
    pub mp_low: Option<f32>,
}

impl Default for Adapt {
    fn default() -> Self {
        Adapt {
            settle_ms: SETTLE_START,
            eagerness: EAGERNESS[0].into(),
            short: false,
            hp_low: None,
            mp_low: None,
        }
    }
}

impl Adapt {
    /// Change with how the latest talk went (`new`: counted since the last
    /// time). Returns what changed, for the log.
    pub fn learn(&mut self, new: &Counts) -> Vec<String> {
        let mut changed = Vec::new();
        if new.sentences >= 8 {
            let rate = new.continued as f32 / new.sentences as f32;
            let before = self.settle_ms;
            if rate > 0.2 {
                self.settle_ms = (self.settle_ms + 150).min(SETTLE_MAX);
            } else if rate < 0.05 {
                self.settle_ms = self.settle_ms.saturating_sub(50).max(SETTLE_MIN);
            }
            if self.settle_ms != before {
                changed.push(format!(
                    "waits {} ms after you stop talking (was {before})",
                    self.settle_ms
                ));
            }
        }
        if new.live_turns >= 8 {
            let rate = new.jumped_in as f32 / new.live_turns as f32;
            let at = EAGERNESS
                .iter()
                .position(|e| *e == self.eagerness)
                .unwrap_or(0);
            let to = if rate > 0.25 {
                (at + 1).min(EAGERNESS.len() - 1)
            } else if rate < 0.05 {
                at.saturating_sub(1)
            } else {
                at
            };
            if to != at {
                self.eagerness = EAGERNESS[to].into();
                changed.push(format!(
                    "answers on live calls with {} eagerness",
                    self.eagerness
                ));
            }
        }
        if new.replies >= 10 {
            let rate = new.talked_over as f32 / new.replies as f32;
            let short = if rate > 0.3 {
                true
            } else if rate < 0.1 {
                false
            } else {
                self.short
            };
            if short != self.short {
                self.short = short;
                changed.push(if short {
                    "keeps answers very short (you often talk over long ones)".into()
                } else {
                    "talks a little more again".into()
                });
            }
        }
        changed
    }
}

/// The notebook.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Memory {
    /// What it knows about the player.
    pub facts: Vec<Fact>,
    /// How they like it to talk.
    pub style: Vec<String>,
    /// Names and words they use (to hear them right).
    pub words: Vec<String>,
    /// What happened last time (and so far this time).
    pub last_time: String,
    /// Sessions it learned from, and the newest of them (sessions are
    /// folders named by when they started).
    pub sessions: u32,
    pub counted_to: String,
    pub read_to: ReadTo,
    pub counts: Counts,
    pub adapt: Adapt,
    /// What the player told it to forget (a look back that was under way
    /// may hand it back).
    pub forgotten: Vec<String>,
    /// How it talks to the player (picked on the phone).
    pub attitude: super::style::Attitude,
    /// The voice it speaks in (an ElevenLabs voice's id; `None` or
    /// "openai": OpenAI's).
    pub voice: Option<String>,
    /// Whether it speaks up on its own while they play (the coach); None
    /// is the usual, on.
    pub coach: Option<bool>,
    /// Whether it updates itself when a new version is out; None is the
    /// usual, on.
    pub updates: Option<bool>,
    /// Whether the workshop is on (MapleSyrup rewrites itself on this PC
    /// when asked); None is the usual, off.
    pub workshop: Option<bool>,
    /// The coding agent the workshop uses ("claude" or "codex"); None is
    /// the first one found.
    pub workshop_coder: Option<String>,
    #[serde(skip)]
    path: Option<PathBuf>,
}

/// At most this many forgotten things are kept, to keep them forgotten.
const MOST_FORGOTTEN: usize = 100;

impl Memory {
    /// What is kept in `settings` (nothing yet: an empty notebook).
    pub fn load(settings: &Path) -> Memory {
        let path = settings.join(FILE);
        let mut memory: Memory = std::fs::read_to_string(&path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        memory.path = Some(path);
        memory
    }

    pub fn save(&self) {
        let Some(path) = &self.path else {
            return;
        };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(text) = serde_json::to_string_pretty(self) {
            // Whole or not at all: written beside it, then put in its place.
            let partial = path.with_extension("json.partial");
            if std::fs::write(&partial, text).is_ok() {
                let _ = std::fs::rename(&partial, path);
            }
        }
    }

    /// What the model is told about the player (empty when nothing is
    /// known yet).
    pub fn prompt(&self) -> String {
        let mut text = String::new();
        if !self.facts.is_empty() {
            text.push_str("What you know about them:\n");
            for fact in &self.facts {
                text.push_str(&format!("- {}\n", fact.text));
            }
        }
        if !self.style.is_empty() || self.adapt.short {
            text.push_str("How they like you to talk:\n");
            for line in &self.style {
                text.push_str(&format!("- {line}\n"));
            }
            if self.adapt.short {
                text.push_str(
                    "- They often talk over long answers: keep it to one short sentence.\n",
                );
            }
        }
        if !self.last_time.trim().is_empty() {
            text.push_str(&format!("Lately: {}\n", self.last_time.trim()));
        }
        text.trim_end().to_string()
    }

    /// The names and words the player uses, for hearing them right.
    pub fn words_hint(&self) -> Option<String> {
        (!self.words.is_empty()).then(|| {
            format!(
                "MapleStory. Names and words the player uses: {}.",
                self.words.join(", ")
            )
        })
    }

    /// Forget a fact by its id. Returns it.
    pub fn forget(&mut self, id: &str) -> Option<String> {
        let at = self.facts.iter().position(|f| f.id == id)?;
        let gone = self.facts.remove(at);
        self.keep_forgotten(&gone.text);
        self.save();
        Some(gone.text)
    }

    /// Forget a note on how they like it to talk, by its id (`id_of` its
    /// text). Returns it.
    pub fn forget_style(&mut self, id: &str) -> Option<String> {
        let at = self.style.iter().position(|s| id_of(s) == id)?;
        let gone = self.style.remove(at);
        self.keep_forgotten(&gone);
        self.save();
        Some(gone)
    }

    fn keep_forgotten(&mut self, text: &str) {
        self.forgotten.push(text.to_string());
        if self.forgotten.len() > MOST_FORGOTTEN {
            self.forgotten
                .drain(..self.forgotten.len() - MOST_FORGOTTEN);
        }
    }

    fn is_forgotten(&self, text: &str) -> bool {
        self.forgotten.iter().any(|f| f.eq_ignore_ascii_case(text))
    }

    /// Take the model's updated notebook. Returns the lessons it found (what
    /// the player corrected), for `knowledge`.
    pub fn take(&mut self, notebook: &Value) -> Result<Vec<(String, String)>, String> {
        let list = |key: &str, most: usize, longest: usize| -> Vec<String> {
            let mut out: Vec<String> = Vec::new();
            for item in notebook[key].as_array().into_iter().flatten() {
                let Some(text) = item.as_str() else { continue };
                let text: String = text.trim().chars().take(longest).collect();
                if !text.is_empty() && !out.iter().any(|o| o.eq_ignore_ascii_case(&text)) {
                    out.push(text);
                }
            }
            out.truncate(most);
            out
        };
        if !notebook["facts"].is_array() || !notebook["last_time"].is_string() {
            return Err("the notebook came back incomplete".into());
        }
        let mut facts = list("facts", MOST_FACTS, 200);
        // Losing everything at once is a mistake, not news.
        if facts.is_empty() && self.facts.len() >= 5 {
            return Err("the notebook came back empty".into());
        }
        facts.retain(|f| !self.is_forgotten(f));
        self.facts = facts.iter().map(|f| Fact::new(f)).collect();
        let mut style = list("style", MOST_STYLE, 200);
        style.retain(|s| !self.is_forgotten(s));
        self.style = style;
        self.words = list("words", MOST_WORDS, 40);
        let last: String = notebook["last_time"]
            .as_str()
            .unwrap_or("")
            .trim()
            .chars()
            .take(600)
            .collect();
        if !last.is_empty() {
            self.last_time = last;
        }
        let lessons = notebook["lessons"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|l| {
                let about = l["about"].as_str()?.trim();
                let right = l["right"].as_str()?.trim();
                (!about.is_empty() && !right.is_empty())
                    .then(|| (about.to_string(), right.to_string()))
            })
            .take(10)
            .collect();
        Ok(lessons)
    }
}

/// The conversation and the counts in the session logs since `from`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Read {
    /// "Player: …" and "MapleSyrup: …", oldest first.
    pub talk: Vec<String>,
    pub counts: Counts,
    /// Where the reading got to.
    pub to: ReadTo,
    /// The sessions the player said something in (in what was read).
    pub talked_in: Vec<String>,
}

/// Read the session logs under `base` from where the last reading stopped.
pub fn read_logs(base: &Path, from: &ReadTo) -> Read {
    let mut read = Read {
        to: from.clone(),
        ..Default::default()
    };
    let mut dirs: Vec<String> = std::fs::read_dir(base)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|name| name.as_str() >= from.session.as_str())
        .collect();
    dirs.sort();
    for dir in dirs {
        let Ok(text) = std::fs::read_to_string(base.join(&dir).join("log.txt")) else {
            continue;
        };
        // A line still being written is read next time.
        let complete = match text.rfind('\n') {
            Some(end) => &text[..=end],
            None => "",
        };
        let lines: Vec<&str> = complete.lines().collect();
        let skip = if dir == from.session { from.lines } else { 0 };
        if skip > lines.len() {
            continue;
        }
        let mut live = false;
        let mut said = false;
        for (i, line) in lines.iter().enumerate() {
            let Some((kind, text)) = parse_line(line) else {
                continue;
            };
            // Whether a live call was on is known from the whole file.
            if kind == "info" {
                if text.starts_with("live call on the phone") {
                    live = true;
                } else if text.starts_with("live call ended") {
                    live = false;
                }
            }
            if i < skip {
                continue;
            }
            match kind {
                "heard" => {
                    said = true;
                    read.talk.push(format!("Player: {text}"));
                    if !live {
                        read.counts.sentences += 1;
                    }
                }
                "reply" => {
                    read.talk.push(format!("MapleSyrup: {text}"));
                    if live {
                        read.counts.live_turns += 1;
                    } else {
                        read.counts.replies += 1;
                    }
                }
                "alert" => read.talk.push(format!("MapleSyrup (game watcher): {text}")),
                "turn" if text.starts_with("talked over") => read.counts.talked_over += 1,
                "turn" if text.starts_with("still talking") => read.counts.continued += 1,
                "turn" if text.starts_with("jumped in") => read.counts.jumped_in += 1,
                _ => {}
            }
        }
        if said {
            read.talked_in.push(dir.clone());
        }
        read.to = ReadTo {
            session: dir,
            lines: lines.len(),
        };
    }
    if read.talk.len() > MOST_LINES {
        read.talk.drain(..read.talk.len() - MOST_LINES);
    }
    read
}

/// "13:42:10  [reply] Level 57…" as ("reply", "Level 57…").
fn parse_line(line: &str) -> Option<(&str, &str)> {
    let rest = line.get(8..)?.trim_start();
    let rest = rest.strip_prefix('[')?;
    let close = rest.find(']')?;
    Some((&rest[..close], rest[close + 1..].trim()))
}

/// The look back: the notebook as it is, what the player told it to
/// remember, the lessons already kept, and the latest conversation.
pub fn look_back(memory: &Memory, told: &str, lessons: &[Entry], talk: &[String]) -> Ask {
    let notebook = json!({
        "facts": memory.facts.iter().map(|f| f.text.clone()).collect::<Vec<_>>(),
        "style": memory.style,
        "words": memory.words,
        "last_time": memory.last_time,
    });
    let mut text = format!(
        "The notebook now:\n{}\n\n",
        serde_json::to_string_pretty(&notebook).unwrap_or_default()
    );
    if !told.trim().is_empty() {
        text.push_str(&format!(
            "What the player asked to be remembered (kept separately, don't copy it):\n{}\n\n",
            told.trim()
        ));
    }
    if !lessons.is_empty() {
        text.push_str(&format!(
            "Lessons already kept (don't give them again):\n{}\n\n",
            knowledge::as_lines(lessons)
        ));
    }
    text.push_str(
        "The latest conversation (their words were turned into text and may be misheard):\n",
    );
    for line in talk {
        let line: String = line.chars().take(400).collect();
        text.push_str(&line);
        text.push('\n');
    }
    let strings = json!({"type": "array", "items": {"type": "string"}});
    let schema = json!({
        "type": "object",
        "properties": {
            "facts": strings,
            "style": strings,
            "words": strings,
            "last_time": {"type": "string"},
            "lessons": {"type": "array", "items": {
                "type": "object",
                "properties": {"about": {"type": "string"}, "right": {"type": "string"}},
                "required": ["about", "right"],
                "additionalProperties": false,
            }},
        },
        "required": ["facts", "style", "words", "last_time", "lessons"],
        "additionalProperties": false,
    });
    Ask {
        instructions: LOOK_BACK.into(),
        input: vec![json!({"role": "user", "content": text})],
        schema: Some(("notebook".into(), schema)),
        max_output_tokens: 2000,
        timeout: Duration::from_secs(90),
        ..Default::default()
    }
}

const LOOK_BACK: &str = "You keep the notebook of MapleSyrup, a buddy who watches the player's MapleStory game \
and talks with them while they play. Read the latest conversation and give back the whole notebook, updated:
- facts: what is worth knowing about the player for next time: their characters (names, classes, levels), what \
they're working towards, where they train and what they're doing in the game, what they enjoy or find annoying, \
their name if they said it, who they play with. Keep what is still true, update what changed (a new level), drop \
what is no longer true or no longer matters, merge repeats; most important first, at most 40, each one short \
sentence in the third person. Never passwords, payment details, addresses or private things about other people.
- style: how they like MapleSyrup to talk to them, only from what they said or showed (shorter, more jokes, the \
language they speak, less warnings...); at most 8.
- words: names and words they say that speech-to-text could get wrong (their character's name, maps, bosses, \
items, slang), spelled right; at most 40.
- last_time: two or three sentences on what they did and talked about lately, so MapleSyrup can pick up from \
there.
- lessons: only what the player corrected MapleSyrup on in this conversation (a game fact, a name, how to do \
something, or how MapleSyrup behaves) and isn't kept already: what it is about in a few words, and the right \
version in one sentence.
Write in the language the player speaks with MapleSyrup most (English when unclear); keep names as they are.";

/// What MapleSyrup learned, shared by the conversation, live calls, the
/// learner and the phone: the notebook, what it looked up and was
/// corrected on, and what the player asked it to remember
/// (`about-me.txt`).
#[derive(Clone)]
pub struct Learning {
    pub memory: Arc<Mutex<Memory>>,
    pub knowledge: Arc<Mutex<Knowledge>>,
    /// The settings folder.
    pub settings: PathBuf,
}

/// The file with what the player asked it to remember, one thing a line.
pub const TOLD: &str = "about-me.txt";

/// The lines of `about-me.txt`, without their dashes.
fn told_lines(text: &str) -> impl Iterator<Item = &str> {
    text.lines()
        .map(|l| l.trim().trim_start_matches("- ").trim())
        .filter(|l| !l.is_empty())
}

impl Learning {
    /// What is kept in `settings`.
    pub fn load(settings: &Path) -> Learning {
        Learning {
            memory: Arc::new(Mutex::new(Memory::load(settings))),
            knowledge: Arc::new(Mutex::new(Knowledge::load(settings))),
            settings: settings.to_path_buf(),
        }
    }

    pub fn memory(&self) -> MutexGuard<'_, Memory> {
        self.memory.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn knowledge(&self) -> MutexGuard<'_, Knowledge> {
        self.knowledge.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// What the player asked it to remember.
    pub fn told(&self) -> String {
        std::fs::read_to_string(self.settings.join(TOLD)).unwrap_or_default()
    }

    /// For the model: what it knows about the player, how they like it to
    /// talk, what happened lately, and what they corrected it on (empty
    /// when nothing is known yet).
    pub fn prompt(&self) -> String {
        let mut parts = Vec::new();
        let told: Vec<String> = told_lines(&self.told()).map(|l| format!("- {l}")).collect();
        if !told.is_empty() {
            parts.push(format!(
                "About the player (they told you this):\n{}",
                told.join("\n")
            ));
        }
        let memory = self.memory().prompt();
        if !memory.is_empty() {
            parts.push(memory);
        }
        let lessons = lessons_prompt(&self.knowledge(), 12);
        if !lessons.is_empty() {
            parts.push(lessons);
        }
        parts.join("\n\n")
    }

    /// What it learned before that may help with `text`, as lines (empty
    /// when nothing does).
    pub fn helps(&self, text: &str) -> String {
        let found = self.knowledge().relevant(text, 3);
        knowledge::as_lines(&found)
    }

    /// Whether it knows anything about the player yet.
    pub fn knows_player(&self) -> bool {
        let known = {
            let memory = self.memory();
            !memory.facts.is_empty() || !memory.last_time.trim().is_empty()
        };
        known || told_lines(&self.told()).next().is_some()
    }

    /// Forget one thing the phone showed, by its id: `m:` a fact, `s:` a
    /// note on how they like it to talk, `a:` something they asked it to
    /// remember, `k:` a lesson or something looked up. Returns what it was.
    pub fn forget(&self, id: &str) -> Option<String> {
        let (kind, id) = id.split_once(':')?;
        match kind {
            "m" => self.memory().forget(id),
            "s" => self.memory().forget_style(id),
            "k" => self.knowledge().forget(id),
            "a" => {
                let file = self.settings.join(TOLD);
                let text = std::fs::read_to_string(&file).ok()?;
                let gone = told_lines(&text).find(|l| id_of(l) == id)?.to_string();
                let kept: String = told_lines(&text)
                    .filter(|l| *l != gone)
                    .map(|l| format!("- {l}\n"))
                    .collect();
                std::fs::write(&file, kept).ok()?;
                // (A look back under way may have read it: kept forgotten.)
                let mut memory = self.memory();
                memory.keep_forgotten(&gone);
                memory.save();
                Some(gone)
            }
            _ => None,
        }
    }

    /// What the phone shows of what it learned.
    pub fn status(&self) -> Value {
        let told: Vec<Value> = told_lines(&self.told())
            .map(|t| json!({"id": format!("a:{}", id_of(t)), "text": t}))
            .collect();
        let (facts, style, lately, sessions, adapt) = {
            let memory = self.memory();
            (
                memory
                    .facts
                    .iter()
                    .map(|f| json!({"id": format!("m:{}", f.id), "text": f.text}))
                    .collect::<Vec<_>>(),
                memory
                    .style
                    .iter()
                    .map(|s| json!({"id": format!("s:{}", id_of(s)), "text": s}))
                    .collect::<Vec<_>>(),
                memory.last_time.clone(),
                memory.sessions,
                memory.adapt.clone(),
            )
        };
        let knowledge = self.knowledge();
        let lessons: Vec<Value> = knowledge
            .lessons(30)
            .iter()
            .map(|e| json!({"id": format!("k:{}", e.id), "about": e.about, "answer": e.answer}))
            .collect();
        json!({
            "told": told,
            "facts": facts,
            "style": style,
            "lately": lately,
            "sessions": sessions,
            "lessons": lessons,
            "looked_up": knowledge.looked_up(),
            "settle_ms": adapt.settle_ms,
            "eagerness": adapt.eagerness,
            "short": adapt.short,
        })
    }
}

/// When the conversation is worth a look back: plenty said, or some said a
/// while after the last one (`since`), so the notebook keeps up without a
/// request every minute.
fn due(talked: usize, since: Duration) -> bool {
    talked >= 30
        || (talked >= 6 && since >= Duration::from_secs(4 * 60))
        || (talked >= 2 && since >= Duration::from_secs(8 * 60))
}

/// Learning, on a thread of its own: reads the session logs under
/// `sessions` every little while and looks back when there is enough new
/// talk. `news` gets a line when it learned or adapted something.
pub fn spawn(openai: Arc<OpenAi>, learning: Learning, sessions: PathBuf, news: Sender<String>) {
    let _ = std::thread::Builder::new()
        .name("learner".into())
        .spawn(move || {
            let mut looked_back: Option<Instant> = None;
            let mut wait_until = Instant::now() + Duration::from_secs(3);
            loop {
                std::thread::sleep(Duration::from_secs(5));
                if Instant::now() < wait_until {
                    continue;
                }
                wait_until = Instant::now() + Duration::from_secs(20);
                let from = learning.memory().read_to.clone();
                let read = read_logs(&sessions, &from);
                if read.to == from {
                    continue;
                }
                let talked = read.talk.iter().filter(|l| l.starts_with("Player:")).count();
                let since = looked_back.map(|t| t.elapsed()).unwrap_or(Duration::MAX);
                if talked > 0 && !due(talked, since) {
                    // Read again with more, later.
                    continue;
                }
                if talked > 0 {
                    let lessons = learning.knowledge().lessons(40);
                    let ask = look_back(&learning.memory(), &learning.told(), &lessons, &read.talk);
                    let notebook = openai
                        .ask(&ask, None)
                        .map_err(|e| e.to_string())
                        .and_then(|a| serde_json::from_str::<Value>(&a.text).map_err(|e| e.to_string()));
                    let taken = {
                        let mut memory = learning.memory();
                        notebook
                            .and_then(|n| memory.take(&n))
                            .map(|lessons| (lessons, memory.facts.len(), memory.style.len()))
                    };
                    match taken {
                        Ok((lessons, facts, style)) => {
                            looked_back = Some(Instant::now());
                            // (What it noted while talking is kept already.)
                            let lessons: Vec<(String, String)> = {
                                let knowledge = learning.knowledge();
                                lessons
                                    .into_iter()
                                    .filter(|(_, right)| !knowledge.has_lesson(right))
                                    .collect()
                            };
                            if !lessons.is_empty() {
                                let mut knowledge = learning.knowledge();
                                for (about, right) in &lessons {
                                    knowledge.add(about, right, Source::Player);
                                }
                                let _ = news.send(format!(
                                    "learned from your corrections: {}",
                                    lessons
                                        .iter()
                                        .map(|(a, r)| format!("{a}: {r}"))
                                        .collect::<Vec<_>>()
                                        .join("; ")
                                ));
                            }
                            let _ = news.send(format!(
                                "notebook updated: {facts} thing{} about you, {style} on how you like to talk",
                                if facts == 1 { "" } else { "s" }
                            ));
                        }
                        Err(why) => {
                            // Not now (no network, no credit): later, from the same place.
                            let _ = news.send(format!("couldn't look back on the conversation: {why}"));
                            wait_until = Instant::now() + Duration::from_secs(5 * 60);
                            continue;
                        }
                    }
                }
                finish(&mut learning.memory(), &read, &news);
            }
        });
}

/// The counts, what they change, and where the reading got to.
fn finish(memory: &mut Memory, read: &Read, news: &Sender<String>) {
    memory.counts.add(&read.counts);
    for session in &read.talked_in {
        if *session > memory.counted_to {
            memory.sessions += 1;
            memory.counted_to = session.clone();
        }
    }
    for change in memory.adapt.learn(&read.counts) {
        let _ = news.send(format!("adapted: {change}"));
    }
    memory.read_to = read.to.clone();
    memory.save();
}

/// The lessons as lines for a model, newest first (empty when there are
/// none).
pub fn lessons_prompt(knowledge: &Knowledge, n: usize) -> String {
    let lessons = knowledge.lessons(n);
    if lessons.is_empty() {
        return String::new();
    }
    format!(
        "What the player corrected you on before (trust these over what you think you know):\n{}",
        knowledge::as_lines(&lessons)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(base: &Path, name: &str, lines: &[&str]) {
        let dir = base.join(name);
        std::fs::create_dir_all(&dir).unwrap();
        let mut text = lines.join("\n");
        text.push('\n');
        std::fs::write(dir.join("log.txt"), text).unwrap();
    }

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ms-memory-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn the_logs_are_read_from_where_the_last_reading_stopped() {
        let base = temp("logs");
        session(
            &base,
            "2026-10-01 20-00-00",
            &[
                "20:00:01  [info] Maple companion is on.",
                "20:00:05  [heard] what's my level",
                "20:00:06  [reply] You're level 61!",
                "20:00:09  [turn] still talking: what's my level and",
                "20:00:10  [turn] talked over: wait",
                "20:00:12  [alert] Careful, your HP's down to 25%.",
            ],
        );
        session(
            &base,
            "2026-10-02 21-00-00",
            &[
                "21:00:01  [info] live call on the phone: talking in real time",
                "21:00:03  [heard] מה הרמה שלי",
                "21:00:04  [reply] רמה 62!",
                "21:00:05  [turn] jumped in",
            ],
        );
        let read = read_logs(&base, &ReadTo::default());
        assert_eq!(
            read.talk,
            [
                "Player: what's my level",
                "MapleSyrup: You're level 61!",
                "MapleSyrup (game watcher): Careful, your HP's down to 25%.",
                "Player: מה הרמה שלי",
                "MapleSyrup: רמה 62!",
            ]
        );
        assert_eq!(
            read.counts,
            Counts {
                sentences: 1,
                continued: 1,
                replies: 1,
                talked_over: 1,
                live_turns: 1,
                jumped_in: 1,
            }
        );
        assert_eq!(
            read.talked_in,
            ["2026-10-01 20-00-00", "2026-10-02 21-00-00"]
        );
        assert_eq!(read.to.session, "2026-10-02 21-00-00");
        assert_eq!(read.to.lines, 4);
        // More of the same session later: only the new lines.
        session(
            &base,
            "2026-10-02 21-00-00",
            &[
                "21:00:01  [info] live call on the phone: talking in real time",
                "21:00:03  [heard] מה הרמה שלי",
                "21:00:04  [reply] רמה 62!",
                "21:00:05  [turn] jumped in",
                "21:00:09  [heard] תודה",
            ],
        );
        let again = read_logs(&base, &read.to);
        assert_eq!(again.talk, ["Player: תודה"]);
        assert_eq!(again.talked_in, ["2026-10-02 21-00-00"]);
        // A session is counted once, however many times it is read.
        let mut memory = Memory::default();
        let (tx, _rx) = std::sync::mpsc::channel();
        finish(&mut memory, &read, &tx);
        finish(&mut memory, &again, &tx);
        assert_eq!(memory.sessions, 2);
        assert_eq!(memory.read_to, again.to);
        // Nothing new: nowhere to go.
        assert_eq!(read_logs(&base, &again.to).to, again.to);
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn the_notebook_comes_back_whole_and_its_lessons_go_to_knowledge() {
        let mut memory = Memory::default();
        let lessons = memory
            .take(&json!({
                "facts": ["Their main is a Night Lord, level 62.", "They want to beat Zakum.", "Their main is a Night Lord, level 62."],
                "style": ["Short answers.", "Mixes Hebrew and English."],
                "words": ["Zakum", "Ellinia", "MoonWalker77"],
                "last_time": "They trained at Ellinia and reached 62.",
                "lessons": [{"about": "Easy Zakum level", "right": "Easy Zakum needs level 50."}],
            }))
            .unwrap();
        assert_eq!(memory.facts.len(), 2);
        assert_eq!(
            memory.facts[0].id,
            id_of("Their main is a Night Lord, level 62.")
        );
        assert_eq!(
            lessons,
            [(
                "Easy Zakum level".into(),
                "Easy Zakum needs level 50.".into()
            )]
        );
        let prompt = memory.prompt();
        assert!(prompt.contains("- They want to beat Zakum."));
        assert!(prompt.contains("How they like you to talk:\n- Short answers."));
        assert!(prompt.contains("Lately: They trained at Ellinia"));
        assert!(memory.words_hint().unwrap().contains("MoonWalker77"));
        // A broken or emptied notebook doesn't wipe what is known.
        for _ in 0..5 {
            memory.facts.push(Fact::new("x"));
        }
        assert!(
            memory
                .take(
                    &json!({"facts": [], "style": [], "words": [], "last_time": "", "lessons": []})
                )
                .is_err()
        );
        assert!(memory.take(&json!({"oops": 1})).is_err());
        assert_eq!(memory.facts.len(), 7);
        // Forgotten by its id.
        let id = memory.facts[1].id.clone();
        assert_eq!(
            memory.forget(&id).as_deref(),
            Some("They want to beat Zakum.")
        );
    }

    #[test]
    fn it_adapts_to_how_the_player_talks() {
        let mut adapt = Adapt::default();
        assert_eq!(adapt.settle_ms, SETTLE_START);
        // They often go on after a pause: it waits longer.
        let changed = adapt.learn(&Counts {
            sentences: 10,
            continued: 4,
            ..Default::default()
        });
        assert_eq!(adapt.settle_ms, SETTLE_START + 150);
        assert_eq!(changed.len(), 1);
        // They never do: quicker, but never below the floor.
        for _ in 0..20 {
            adapt.learn(&Counts {
                sentences: 10,
                ..Default::default()
            });
        }
        assert_eq!(adapt.settle_ms, SETTLE_MIN);
        // A live call that keeps jumping in calms down, then speeds up again.
        adapt.learn(&Counts {
            live_turns: 10,
            jumped_in: 4,
            ..Default::default()
        });
        assert_eq!(adapt.eagerness, "medium");
        adapt.learn(&Counts {
            live_turns: 10,
            jumped_in: 0,
            ..Default::default()
        });
        assert_eq!(adapt.eagerness, "high");
        // Talked over a lot: shorter.
        adapt.learn(&Counts {
            replies: 10,
            talked_over: 5,
            ..Default::default()
        });
        assert!(adapt.short);
        // Too little to go by: nothing changes.
        assert!(
            adapt
                .learn(&Counts {
                    sentences: 3,
                    continued: 3,
                    ..Default::default()
                })
                .is_empty()
        );
    }

    #[test]
    fn it_is_kept_and_read_back() {
        let dir = temp("keep");
        let mut memory = Memory::load(&dir);
        memory.facts.push(Fact::new("Their main is a Bishop."));
        memory.adapt.settle_ms = 700;
        memory.save();
        let again = Memory::load(&dir);
        assert_eq!(again.facts, memory.facts);
        assert_eq!(again.adapt.settle_ms, 700);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn what_it_learned_is_told_shown_and_forgotten_on_the_phone() {
        let dir = temp("learning");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join(TOLD),
            "- Their boss key is F10\n- They stream on Fridays\n",
        )
        .unwrap();
        let learning = Learning::load(&dir);
        assert!(learning.knows_player());
        learning
            .memory()
            .take(&json!({
                "facts": ["Their main is a Bishop.", "They want to beat Zakum."],
                "style": ["Short answers."],
                "words": [],
                "last_time": "They trained at Ellinia.",
                "lessons": [],
            }))
            .unwrap();
        learning.knowledge().add(
            "Easy Zakum level",
            "Easy Zakum needs level 50.",
            Source::Player,
        );
        // What the model is told: what they said, the notebook, the lessons.
        let prompt = learning.prompt();
        for part in [
            "About the player (they told you this):\n- Their boss key is F10",
            "- They want to beat Zakum.",
            "How they like you to talk:\n- Short answers.",
            "Lately: They trained at Ellinia.",
            "Easy Zakum needs level 50. (the player corrected you; trust this)",
        ] {
            assert!(prompt.contains(part), "{part}\n{prompt}");
        }
        assert!(
            learning
                .helps("what level for easy zakum")
                .contains("level 50")
        );
        assert!(learning.helps("hello there").is_empty());
        // What the phone shows, every piece with an id to forget it by.
        let status = learning.status();
        assert_eq!(status["told"][1]["text"], "They stream on Fridays");
        assert_eq!(status["settle_ms"], SETTLE_START);
        for (list, which) in [("told", 0), ("facts", 1), ("style", 0), ("lessons", 0)] {
            let id = status[list][which]["id"].as_str().unwrap().to_string();
            assert!(learning.forget(&id).is_some(), "{list}");
        }
        let status = learning.status();
        assert_eq!(status["told"].as_array().unwrap().len(), 1);
        assert_eq!(status["facts"][0]["text"], "Their main is a Bishop.");
        assert!(status["style"].as_array().unwrap().is_empty());
        assert!(status["lessons"].as_array().unwrap().is_empty());
        assert_eq!(
            std::fs::read_to_string(dir.join(TOLD)).unwrap(),
            "- They stream on Fridays\n"
        );
        assert!(learning.forget("m:nothing").is_none());
        assert!(learning.forget("no-colon").is_none());
        // A look back that read the notebook before it was forgotten doesn't
        // bring it back.
        learning
            .memory()
            .take(&json!({
                "facts": ["Their main is a Bishop.", "They want to beat Zakum."],
                "style": ["Short answers."],
                "words": [],
                "last_time": "",
                "lessons": [],
            }))
            .unwrap();
        assert_eq!(learning.memory().facts.len(), 1);
        assert!(learning.memory().style.is_empty());
        // Kept between sessions.
        assert_eq!(Learning::load(&dir).memory().facts.len(), 1);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn it_looks_back_when_there_is_enough_new_talk_without_asking_every_minute() {
        let minutes = |m: u64| Duration::from_secs(m * 60);
        assert!(due(2, Duration::MAX));
        assert!(!due(1, Duration::MAX));
        assert!(!due(6, minutes(1)));
        assert!(due(6, minutes(5)));
        assert!(!due(3, minutes(5)));
        assert!(due(3, minutes(9)));
        assert!(due(30, Duration::ZERO));
    }

    #[test]
    fn the_look_back_asks_for_the_whole_notebook() {
        let mut memory = Memory::default();
        memory.facts.push(Fact::new("Their main is a Bishop."));
        let ask = look_back(
            &memory,
            "- Their boss key is F10",
            &[],
            &["Player: hi".into()],
        );
        let text = ask.input[0]["content"].as_str().unwrap();
        assert!(text.contains("Their main is a Bishop."));
        assert!(text.contains("boss key is F10"));
        assert!(text.contains("Player: hi"));
        assert_eq!(ask.schema.as_ref().unwrap().0, "notebook");
    }
}
