//! The running companion's side of the channel: what it posts for Claude
//! (events, the reading now, the screen) and what Claude asks to have said,
//! served under `/local/` on this PC only.
//!
//! - `GET /local/events?since=N&wait=MS` — the events after `N`, waiting up
//!   to `wait` milliseconds for one; `?fresh=1` instead starts afresh,
//!   with nothing from before the bridge came (only the number to go on
//!   from). Each answer carries the board's `boot`, which a companion
//!   started again changes. Each poll says a bridge is there
//!   ([`Board::bridge_seen_ago`]); one with `listen=1` says Claude takes the
//!   events, and MapleSyrup leaves the talking to it ([`Board::connected`]).
//! - `POST /local/quit` — the bridge that started the companion is done
//!   with it ([`Board::quit_requested`]).
//! - `GET /local/status` — the reading now, as text and as fields, with the
//!   last few events.
//! - `POST /local/say` `{"text": …, "answers": N}` — a line for MapleSyrup's
//!   voice; `answers`, the number of the player's words it answers (their
//!   `heard` event's). He comes first: a line for words he has followed
//!   with newer ones, or written before Claude could read his newest, or
//!   coming while he talked over Claude's voice, is not said (409, and
//!   why: Claude answers the newest instead).
//! - `GET /local/look?region=…` — the screen (or part of it) as a JPEG,
//!   while the game is in view.
//! - `GET /local/wiki?q=…` / `POST /local/wiki` `{"about", "answer",
//!   "source"}` — the local MapleStory wiki: what MapleSyrup knows about the
//!   game (looked up, or the player's corrections; `knowledge.json`), and a
//!   fact added to it.
//! - `POST /local/tone` `{"tone"}` — the bar of how it talks to him moved
//!   to a level (a level's name in `spectrum.json`), as on his phone: he
//!   asked Claude to talk another way ([`Board::take_tone`]).
//! - `GET /local/player` / `POST /local/player` `{"fact"}` — what MapleSyrup
//!   knows about the player and his game (his own file, `about-me.txt`, and
//!   its notebook), and a fact added to his file.
//!
//! Every request carries the board's key (`k=` or `X-MapleSyrup-Key`), and
//! one that came through a tunnel or a proxy (it carries their headers) is
//! not answered: this is for the bridge on this PC.

use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use image::RgbaImage;
use serde::Serialize;
use serde_json::{Value, json};

use crate::phone::http::{Request, Response};
use crate::util::latency::{self, Key, Stage};

/// Events kept for a bridge that polls late.
const KEEP: usize = 200;
/// The most events one poll hands over.
const PER_POLL: usize = 50;
/// The longest a poll waits.
const MAX_WAIT: Duration = Duration::from_secs(25);
/// The bridge counts as there while it polled this recently (a poll waits
/// up to 25 s, and comes again at once).
const CONNECTED_FOR: Duration = Duration::from_secs(45);
/// The longest line Claude can have said at once.
const MAX_SAY: usize = 800;
/// A line that names no words of his it answers, coming this soon after
/// his newest words were handed to Claude (or posted, when no bridge took
/// them), was written before Claude could have read them: Claude takes
/// longer than this to read a message and answer it.
const SAY_GRACE: Duration = Duration::from_millis(1000);
/// How long after he talked over Claude's voice Claude's lines wait for
/// what he says (it comes as `heard`), when nothing comes.
const TALKED_OVER_WAIT: Duration = Duration::from_secs(6);
/// A picture's longest side by default: what Claude takes in whole.
const LOOK_SIZE: u32 = 1568;
/// How long his words wait for Claude's answer before MapleSyrup's own
/// brain answers them (the safety net; the brief tells Claude so). On 25.10
/// Claude answered in 2 s at the median and within 6 s nine times in ten,
/// but a turn of its own can wait much longer — behind a game event, a
/// permission prompt in its window, the conversation being compacted — and
/// the player only hears silence.
pub const ANSWER_WAIT: Duration = Duration::from_secs(6);
/// The requests that are Claude's tools (the bridge makes one for each),
/// by method and path, with the tool's name: timed for the words of his
/// Claude is answering ([`latency`]).
const TOOLS: &[(&str, &str, &str)] = &[
    ("GET", "/local/status", "game_status"),
    ("GET", "/local/look", "look_at_screen"),
    ("GET", "/local/picture", "save_picture"),
    ("GET", "/local/wiki", "maple_wiki_search"),
    ("POST", "/local/wiki", "maple_wiki_save"),
    ("GET", "/local/player", "player_profile"),
    ("POST", "/local/player", "player_remember"),
    ("POST", "/local/tone", "set_tone"),
];

/// One event as posted.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Posted {
    pub seq: u64,
    /// Seconds since the board was made.
    pub t: f64,
    pub kind: String,
    /// What happened, in a line.
    pub headline: String,
    /// The whole event: the headline and the reading at the time.
    pub text: String,
}

struct Inner {
    events: VecDeque<Posted>,
    next: u64,
    status: Value,
    text: String,
    bridge_seen: Option<Instant>,
    /// A bridge whose Claude takes the events (Claude Code) polled then.
    listener_seen: Option<Instant>,
    quit: bool,
    says: Vec<String>,
    frame: Option<Arc<RgbaImage>>,
    /// His newest words (`heard`): their event, and when the bridge took
    /// them (None: not yet).
    newest: Option<Newest>,
    /// When he last talked over Claude's voice and stopped it, until his
    /// words come.
    talked_over: Option<Instant>,
    /// Lines of Claude's that were not said (he had moved on), for the
    /// log.
    refused: Vec<String>,
    /// Where Claude moved the bar of how it talks to him, for the main
    /// loop to take.
    tone: Option<String>,
    /// The game as it was when the last event went to Claude: a line of
    /// its own written about it is not said once that has changed
    /// ([`Basis`]).
    last_event: Option<Basis>,
}

/// What a line about a game event takes as still so when it is said: the
/// character alive or dead, and the map. (On 25.10 "you died, click Revive"
/// came after the tester was up and playing again.)
#[derive(Debug, Clone, PartialEq)]
struct Basis {
    dead: Option<bool>,
    map: Option<String>,
}

impl Basis {
    /// The game as the reading `status` (the scene) has it.
    fn of(status: &Value) -> Basis {
        Basis {
            dead: status["dead"].as_bool(),
            map: status["map"].as_str().map(str::to_string),
        }
    }

    /// What changed from `self` to `now`, in words for Claude (None:
    /// nothing a line could have been about). An unknown side is no
    /// change.
    fn changed(&self, now: &Basis) -> Option<String> {
        if let (Some(then), Some(dead)) = (self.dead, now.dead)
            && then != dead
        {
            return Some(if dead {
                "he died since".into()
            } else {
                "he is alive again since".into()
            });
        }
        if let (Some(then), Some(map)) = (&self.map, &now.map)
            && then != map
        {
            return Some(format!("he is on another map since ({map})"));
        }
        None
    }
}

/// The player's newest words posted for Claude.
struct Newest {
    seq: u64,
    headline: String,
    posted: Instant,
    taken: Option<Instant>,
    /// A line of Claude's was said after them: he has been answered.
    answered: bool,
    /// Claude did not answer in time and MapleSyrup's own brain answered
    /// them ([`Board::answered_here`]): a line of Claude's for them is not
    /// said any more.
    ours: bool,
}

pub struct Board {
    key: String,
    /// What MapleSyrup knows about the game and the player (None: nothing
    /// kept, as in the tests).
    learning: Mutex<Option<crate::ai::Learning>>,
    /// Where pictures of the game are saved for Claude (None: nowhere).
    pictures: Mutex<Option<std::path::PathBuf>>,
    /// This run's: a bridge that sees it change knows the companion
    /// started again (and numbers its events from one).
    boot: String,
    started: Instant,
    inner: Mutex<Inner>,
    changed: Condvar,
}

impl Board {
    pub fn new(key: String) -> Arc<Board> {
        Arc::new(Board {
            key,
            learning: Mutex::new(None),
            pictures: Mutex::new(None),
            boot: crate::phone::tls::random_hex(4),
            started: Instant::now(),
            inner: Mutex::new(Inner {
                events: VecDeque::new(),
                next: 1,
                status: json!({}),
                text: String::new(),
                bridge_seen: None,
                listener_seen: None,
                quit: false,
                says: Vec::new(),
                frame: None,
                last_event: None,
                newest: None,
                talked_over: None,
                refused: Vec::new(),
                tone: None,
            }),
            changed: Condvar::new(),
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn key(&self) -> &str {
        &self.key
    }

    /// Post an event: `headline` is what happened, `text` the whole of it
    /// (the headline and the reading). Returns its number.
    pub fn post(&self, kind: &str, headline: &str, text: String) -> u64 {
        let t = self.started.elapsed().as_secs_f64();
        let mut inner = self.lock();
        let seq = inner.next;
        inner.next += 1;
        // His words: the newest Claude is to answer, and what he says after
        // talking over Claude's voice has come.
        if kind != "heard" {
            inner.last_event = Some(Basis::of(&inner.status));
        }
        if kind == "heard" {
            inner.newest = Some(Newest {
                seq,
                headline: headline.to_string(),
                posted: Instant::now(),
                taken: None,
                answered: false,
                ours: false,
            });
            inner.talked_over = None;
        }
        inner.events.push_back(Posted {
            seq,
            t,
            kind: kind.to_string(),
            headline: headline.to_string(),
            text,
        });
        while inner.events.len() > KEEP {
            inner.events.pop_front();
        }
        drop(inner);
        self.changed.notify_all();
        seq
    }

    /// The reading now: its fields, and the same in plain lines.
    pub fn set_status(&self, status: Value, text: String) {
        let mut inner = self.lock();
        inner.status = status;
        inner.text = text;
    }

    /// The screen now (None while the game is not in view).
    pub fn set_frame(&self, frame: Option<Arc<RgbaImage>>) {
        self.lock().frame = frame;
    }

    /// What MapleSyrup knows about the game and the player: the wiki and
    /// the player's file are served from it.
    pub fn set_learning(&self, learning: crate::ai::Learning) {
        if let Ok(mut slot) = self.learning.lock() {
            *slot = Some(learning);
        }
    }

    /// Where `/local/picture` saves the pictures it takes (made when
    /// needed).
    pub fn set_pictures_dir(&self, dir: std::path::PathBuf) {
        if let Ok(mut slot) = self.pictures.lock() {
            *slot = Some(dir);
        }
    }

    fn learning(&self) -> Option<crate::ai::Learning> {
        self.learning.lock().ok().and_then(|l| l.clone())
    }

    /// How long ago a bridge was last heard from (None: never).
    pub fn bridge_seen_ago(&self) -> Option<Duration> {
        self.lock().bridge_seen.map(|at| at.elapsed())
    }

    /// Whether Claude takes the events (a Claude Code session is polling):
    /// MapleSyrup leaves the talking to it.
    pub fn connected(&self) -> bool {
        self.lock()
            .listener_seen
            .is_some_and(|at| at.elapsed() < CONNECTED_FOR)
    }

    /// Whether the bridge that started this companion is done with it.
    pub fn quit_requested(&self) -> bool {
        self.lock().quit
    }

    /// Where Claude moved the bar of how it talks to him, once (the main
    /// loop sets it as the phone's bar would).
    pub fn take_tone(&self) -> Option<String> {
        self.lock().tone.take()
    }

    /// What Claude asked to have said since the last call.
    pub fn take_says(&self) -> Vec<String> {
        std::mem::take(&mut self.lock().says)
    }

    /// Since when his words number `seq` have waited for Claude's answer:
    /// from when the bridge handed them to Claude (or were posted, when no
    /// bridge took them). None once anything of Claude's was said after
    /// them, once MapleSyrup answered them itself, or once newer words of
    /// his came (those wait instead).
    pub fn waiting_since(&self, seq: u64) -> Option<Instant> {
        let inner = self.lock();
        let newest = inner.newest.as_ref().filter(|n| n.seq == seq)?;
        (!newest.answered && !newest.ours).then(|| newest.taken.unwrap_or(newest.posted))
    }

    /// Claude did not answer his words number `seq` in time, and
    /// MapleSyrup's own brain answers them: a line of Claude's for them
    /// is refused from now on, with why ([`Board::refusal`]).
    pub fn answered_here(&self, seq: u64) {
        if let Some(newest) = self.lock().newest.as_mut().filter(|n| n.seq == seq) {
            newest.ours = true;
        }
    }

    /// Lines Claude asked to have said that were not, since the last call:
    /// he had moved on ([`Board::refusal`]).
    pub fn take_refused(&self) -> Vec<String> {
        std::mem::take(&mut self.lock().refused)
    }

    /// He talked over Claude's voice and stopped it: Claude's lines wait
    /// for what he says ([`TALKED_OVER_WAIT`] at most), and he is answered.
    pub fn talked_over(&self) {
        self.lock().talked_over = Some(Instant::now());
    }

    /// Why a line of Claude's that answers his words number `answers` (or
    /// names none) is not to be said now: he talked over Claude's voice
    /// and has not said what he wants yet; or he has said something newer
    /// than what it answers; or it names nothing and came too soon after
    /// his newest words to be about them ([`SAY_GRACE`]). None: it is said.
    fn refusal(inner: &Inner, answers: Option<u64>, now: Instant) -> Option<String> {
        if inner
            .talked_over
            .is_some_and(|at| now.saturating_duration_since(at) < TALKED_OVER_WAIT)
        {
            return Some(
                "he talked over you just now and MapleSyrup stopped your voice: wait for what he says \
(it comes as heard) and answer that"
                    .into(),
            );
        }
        // A line of its own (about the last event) written before the game
        // changed: what it says is no longer so.
        if answers.is_none()
            && let Some(changed) = inner
                .last_event
                .as_ref()
                .and_then(|then| then.changed(&Basis::of(&inner.status)))
        {
            return Some(format!(
                "the game changed after the event you're speaking of — {changed}: what you wrote is no longer so; \
say nothing about it"
            ));
        }
        let newest = inner.newest.as_ref()?;
        if newest.ours && answers == Some(newest.seq) {
            return Some(format!(
                "you took too long, so MapleSyrup answered him itself: {} (heard_id {}) is answered — \
don't answer it again; answer what he says next",
                newest.headline, newest.seq
            ));
        }
        let older = match answers {
            Some(answers) => answers < newest.seq,
            None => {
                now.saturating_duration_since(newest.taken.unwrap_or(newest.posted)) < SAY_GRACE
            }
        };
        older.then(|| {
            format!(
                "he said something newer: {} (heard_id {}). Answer that instead — with answers={} — \
or say nothing",
                newest.headline, newest.seq, newest.seq
            )
        })
    }

    /// Answer a request under `/local/`.
    pub fn handle(&self, request: &Request) -> Response {
        // Not through a tunnel or a proxy: their requests carry these.
        let forwarded = ["cf-ray", "cf-connecting-ip", "x-forwarded-for", "forwarded"]
            .iter()
            .any(|h| request.header(h).is_some());
        if forwarded {
            return Response::text(404, "not found");
        }
        let key = request
            .param("k")
            .or_else(|| request.header("x-maplesyrup-key"))
            .unwrap_or("");
        if !same(key.as_bytes(), self.key.as_bytes()) {
            return Response::json(403, &json!({"error": "wrong key"}));
        }
        // Any use of it (not a look whether it is up, nor a word to stop)
        // says a Claude is there.
        if !matches!(request.path.as_str(), "/local/hello" | "/local/quit") {
            self.lock().bridge_seen = Some(Instant::now());
        }
        let began = Instant::now();
        let response = match (request.method.as_str(), request.path.as_str()) {
            ("GET", "/local/hello") => {
                Response::json(200, &json!({"ok": true, "pid": std::process::id()}))
            }
            ("GET", "/local/events") => self.events(request),
            ("POST", "/local/quit") => {
                self.lock().quit = true;
                Response::json(200, &json!({"ok": true}))
            }
            ("GET", "/local/status") => self.status(),
            ("POST", "/local/say") => {
                let body = serde_json::from_slice::<Value>(&request.body).unwrap_or(Value::Null);
                let text = body["text"].as_str().map(str::trim).unwrap_or_default();
                // (A number, or a number in a string: as Claude wrote it.)
                let answers = body["answers"]
                    .as_u64()
                    .or_else(|| body["answers"].as_str().and_then(|s| s.trim().parse().ok()));
                if text.is_empty() {
                    // Quiet on purpose about his words (he asked for quiet,
                    // or he talked to someone else): they are answered, and
                    // MapleSyrup does not answer them either.
                    let mut inner = self.lock();
                    inner.bridge_seen = Some(Instant::now());
                    return match (answers, inner.newest.as_mut()) {
                        (Some(answers), Some(newest)) if answers == newest.seq => {
                            newest.answered = true;
                            latency::TRACER.ended(Key::Heard(answers), latency::End::Silent);
                            Response::json(200, &json!({"ok": true, "quiet": true}))
                        }
                        (Some(_), _) => Response::json(200, &json!({"ok": true, "quiet": true})),
                        (None, _) => Response::json(400, &json!({"error": "nothing to say"})),
                    };
                }
                let text: String = text.chars().take(MAX_SAY).collect();
                let mut inner = self.lock();
                inner.bridge_seen = Some(Instant::now());
                // (Timed for the words it answers: those it names, else his
                // newest.)
                let answering = answers.or(inner.newest.as_ref().map(|n| n.seq));
                if let Some(why) = Self::refusal(&inner, answers, Instant::now()) {
                    inner.refused.push(text);
                    if let Some(heard) = answering {
                        latency::TRACER.refused(heard);
                    }
                    return Response::json(409, &json!({"error": why}));
                }
                // (Said after his newest words reached Claude: he has his
                // answer, whatever it names.)
                if let Some(newest) = inner.newest.as_mut()
                    && answers.is_none_or(|answers| answers == newest.seq)
                {
                    newest.answered = true;
                }
                if let Some(heard) = answering {
                    latency::TRACER.say(heard, &text);
                }
                inner.says.push(text);
                Response::json(200, &json!({"ok": true}))
            }
            ("GET", "/local/look") => self.look(request),
            ("GET", "/local/picture") => self.picture(request),
            ("GET", "/local/wiki") => self.wiki(request.param("q").unwrap_or_default()),
            ("POST", "/local/wiki") => {
                let body = serde_json::from_slice::<Value>(&request.body).unwrap_or(Value::Null);
                self.wiki_save(
                    body["about"].as_str().unwrap_or_default(),
                    body["answer"].as_str().unwrap_or_default(),
                    body["source"].as_str().unwrap_or_default(),
                )
            }
            ("GET", "/local/player") => self.player(),
            ("POST", "/local/player") => {
                let body = serde_json::from_slice::<Value>(&request.body).unwrap_or(Value::Null);
                self.remember(body["fact"].as_str().unwrap_or_default())
            }
            ("POST", "/local/tone") => {
                let body = serde_json::from_slice::<Value>(&request.body).unwrap_or(Value::Null);
                let spectrum = crate::companion::spectrum::Spectrum::get();
                match body["tone"]
                    .as_str()
                    .and_then(|id| spectrum.index(id))
                    .map(|at| spectrum.level(at).id.clone())
                {
                    Some(id) => {
                        self.lock().tone = Some(id.clone());
                        Response::json(200, &json!({"ok": true, "tone": id}))
                    }
                    None => Response::json(
                        400,
                        &json!({"error": format!("the levels are {}", spectrum.ids().join(", "))}),
                    ),
                }
            }
            _ => Response::json(404, &json!({"error": "not found"})),
        };
        if latency::TRACER.enabled() {
            self.timed(request, began);
        }
        response
    }

    /// A tool of Claude's ([`TOOLS`]) ran from `began` to now: a stretch of
    /// the turn of the words of his the bridge last took for Claude.
    fn timed(&self, request: &Request, began: Instant) {
        let Some((_, _, tool)) = TOOLS
            .iter()
            .find(|(method, path, _)| *method == request.method && *path == request.path)
        else {
            return;
        };
        let heard = self
            .lock()
            .newest
            .as_ref()
            .filter(|n| n.taken.is_some())
            .map(|n| n.seq);
        if let Some(heard) = heard {
            latency::TRACER.span(Key::Heard(heard), tool, began, Instant::now());
        }
    }

    fn events(&self, request: &Request) -> Response {
        let number = |name: &str| -> u64 {
            request
                .param(name)
                .and_then(|s| s.parse().ok())
                .unwrap_or(0)
        };
        let since = number("since");
        let wait = Duration::from_millis(number("wait")).min(MAX_WAIT);
        let listening = request.param("listen") == Some("1");
        let mut inner = self.lock();
        inner.bridge_seen = Some(Instant::now());
        if listening {
            inner.listener_seen = Some(Instant::now());
        }
        if request.param("fresh").is_some() {
            // A bridge that just came: from here on. (His words from before
            // it are in the reading it starts with.)
            if let Some(newest) = inner.newest.as_mut() {
                newest.taken.get_or_insert_with(Instant::now);
            }
            let last = inner.next - 1;
            return Response::json(200, &json!({"events": [], "last": last, "boot": self.boot}));
        }
        let deadline = Instant::now() + wait;
        while inner.next - 1 <= since {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break;
            }
            inner = match self.changed.wait_timeout(inner, left) {
                Ok((guard, _)) => guard,
                Err(poisoned) => poisoned.into_inner().0,
            };
        }
        // (Still there: a poll that waited long is still the bridge's.)
        inner.bridge_seen = Some(Instant::now());
        if listening {
            inner.listener_seen = Some(Instant::now());
        }
        let waiting = inner
            .newest
            .as_ref()
            .filter(|n| n.taken.is_none())
            .map(|n| n.seq);
        let events: Vec<&Posted> = inner
            .events
            .iter()
            .filter(|e| e.seq > since)
            .take(PER_POLL)
            .collect();
        let last = events.last().map(|e| e.seq).unwrap_or(inner.next - 1);
        let handed = waiting.is_some_and(|seq| events.iter().any(|e| e.seq == seq));
        let answer = json!({"events": events, "last": last, "boot": self.boot});
        // His newest words are on their way to Claude now.
        if handed && let Some(newest) = inner.newest.as_mut() {
            newest.taken = Some(Instant::now());
            latency::TRACER.stamp(Key::Heard(newest.seq), Stage::Bridge);
        }
        Response::json(200, &answer)
    }

    /// What the local wiki has on `query`: the best few entries, the
    /// player's corrections first among equals.
    fn wiki(&self, query: &str) -> Response {
        let Some(learning) = self.learning() else {
            return Response::json(503, &json!({"error": "MapleSyrup keeps no wiki here"}));
        };
        let query = query.trim();
        if query.is_empty() {
            return Response::json(400, &json!({"error": "nothing to look for"}));
        }
        let found = learning.knowledge().relevant(query, 8);
        let entries: Vec<Value> = found
            .iter()
            .map(|e| {
                json!({
                    "about": e.about,
                    "answer": e.answer,
                    "from": match e.from {
                        crate::ai::knowledge::Source::Player => "the player (a correction: trust it)",
                        crate::ai::knowledge::Source::Web => "looked up",
                    },
                    "when": e.when,
                })
            })
            .collect();
        let size = learning.knowledge().entries.len();
        Response::json(200, &json!({"found": entries, "size": size}))
    }

    /// A fact for the local wiki (it takes the place of one about the same
    /// thing, unless that one is the player's correction).
    fn wiki_save(&self, about: &str, answer: &str, source: &str) -> Response {
        let Some(learning) = self.learning() else {
            return Response::json(503, &json!({"error": "MapleSyrup keeps no wiki here"}));
        };
        let (about, answer, source) = (about.trim(), answer.trim(), source.trim());
        if about.is_empty() || answer.is_empty() {
            return Response::json(400, &json!({"error": "say what it is about and the fact"}));
        }
        let answer = if source.is_empty() {
            answer.to_string()
        } else {
            format!("{answer} (source: {source})")
        };
        let id = learning
            .knowledge()
            .add(about, &answer, crate::ai::knowledge::Source::Web);
        Response::json(200, &json!({"ok": true, "id": id}))
    }

    /// What MapleSyrup knows about the player and his game: his own file,
    /// its notebook, his character as read now, and how he set Claude to
    /// talk to him (the bar on his phone).
    fn player(&self) -> Response {
        let character = {
            let inner = self.lock();
            let s = &inner.status;
            let mut who = Vec::new();
            for field in ["name", "job"] {
                if let Some(v) = s[field].as_str() {
                    who.push(v.to_string());
                }
            }
            if let Some(level) = s["level"].as_u64() {
                who.push(format!("level {level}"));
            }
            if s["classic"].as_bool() == Some(true) {
                who.push("MapleStory Classic World".to_string());
            }
            who.join(", ")
        };
        let known = self.learning().map(|l| l.prompt()).unwrap_or_default();
        let mut text = String::new();
        if !character.is_empty() {
            text.push_str(&format!(
                "His character as MapleSyrup reads it now (the name can be misread): {character}.\n\n"
            ));
        }
        if known.trim().is_empty() {
            text.push_str("MapleSyrup knows nothing else about him yet.");
        } else {
            text.push_str(&known);
        }
        if let Some(learning) = self.learning() {
            let tone = super::tone(learning.memory().level());
            text.push_str("\n\n");
            text.push_str(&tone);
        }
        Response::json(200, &json!({"text": text}))
    }

    /// A fact about the player for good, in his own file (one a line; the
    /// same line is not written twice).
    fn remember(&self, fact: &str) -> Response {
        let Some(learning) = self.learning() else {
            return Response::json(503, &json!({"error": "MapleSyrup keeps nothing here"}));
        };
        let fact: String = fact
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .chars()
            .take(400)
            .collect();
        if fact.is_empty() {
            return Response::json(400, &json!({"error": "nothing to remember"}));
        }
        let file = learning.settings.join(crate::ai::memory::TOLD);
        let mut about = std::fs::read_to_string(&file).unwrap_or_default();
        let known = about
            .lines()
            .any(|l| l.trim().trim_start_matches("- ").trim() == fact);
        if !known {
            if !about.is_empty() && !about.ends_with('\n') {
                about.push('\n');
            }
            about.push_str(&format!("- {fact}\n"));
            let _ = std::fs::create_dir_all(&learning.settings);
            if let Err(e) = std::fs::write(&file, about) {
                return Response::json(500, &json!({"error": format!("couldn't keep it: {e}")}));
            }
        }
        Response::json(200, &json!({"ok": true, "new": !known}))
    }

    fn status(&self) -> Response {
        let now = self.started.elapsed().as_secs_f64();
        let inner = self.lock();
        let recent: Vec<Value> = inner
            .events
            .iter()
            .rev()
            .filter(|e| e.kind != "heard")
            .take(10)
            .map(|e| {
                json!({
                    "kind": e.kind,
                    "what": e.headline,
                    "seconds_ago": (now - e.t).max(0.0).round(),
                })
            })
            .collect();
        Response::json(
            200,
            &json!({"status": inner.status, "text": inner.text, "recent": recent}),
        )
    }

    fn look(&self, request: &Request) -> Response {
        use crate::ai::images;
        let Some(frame) = self.lock().frame.clone() else {
            return Response::json(
                404,
                &json!({"error": "MapleStory isn't in view (closed, minimised, or behind a window where it can't be captured), so there is no picture of it"}),
            );
        };
        let name = request.param("region").unwrap_or("full");
        let Some(region) = region(name) else {
            return Response::json(400, &json!({"error": format!("no such region: {name}")}));
        };
        let size = request
            .param("max")
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(LOOK_SIZE)
            .clamp(256, 2048);
        let part = images::crop(&frame, &region);
        let picture = images::fit(&part, size, size);
        let url = images::jpeg_url(&picture, 82);
        let data = url
            .split_once(',')
            .map(|(_, data)| data.to_string())
            .unwrap_or_default();
        Response::json(
            200,
            &json!({
                "mime": "image/jpeg",
                "data": data,
                "width": picture.width(),
                "height": picture.height(),
                "region": name,
                "frame": [frame.width(), frame.height()],
            }),
        )
    }
}

impl Board {
    /// The game's window (or a part of it) saved as a PNG file at full
    /// size, for Claude to edit or keep: its path.
    fn picture(&self, request: &Request) -> Response {
        use crate::ai::images;
        let Some(dir) = self.pictures.lock().ok().and_then(|d| d.clone()) else {
            return Response::json(503, &json!({"error": "MapleSyrup keeps no pictures here"}));
        };
        let Some(frame) = self.lock().frame.clone() else {
            return Response::json(
                404,
                &json!({"error": "MapleStory isn't in view (closed, minimised, or behind a window where it can't be captured), so there is no picture of it"}),
            );
        };
        let name = request.param("region").unwrap_or("full");
        let Some(region) = region(name) else {
            return Response::json(400, &json!({"error": format!("no such region: {name}")}));
        };
        let part = images::crop(&frame, &region);
        let stamp = chrono::Local::now().format("%Y-%m-%d %H-%M-%S");
        let label: String = name
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        let path = dir.join(format!("MapleStory {stamp} {label}.png"));
        let saved = std::fs::create_dir_all(&dir)
            .map_err(|e| e.to_string())
            .and_then(|_| part.save(&path).map_err(|e| e.to_string()));
        match saved {
            Ok(()) => Response::json(
                200,
                &json!({
                    "path": path.to_string_lossy(),
                    "width": part.width(),
                    "height": part.height(),
                    "region": name,
                }),
            ),
            Err(e) => Response::json(500, &json!({"error": format!("couldn't save it: {e}")})),
        }
    }
}

/// The part of the screen a region's name means, as fractions of it:
/// `full`, `center` (around the character: the camera follows him),
/// `top`, `bottom`, `left`, `right`, the four corners (`top-left`: the
/// classic minimap and map name), or `x0,y0,x1,y1` in fractions.
pub fn region(name: &str) -> Option<crate::ai::images::NBox> {
    use crate::ai::images::NBox;
    let b = |x0, y0, x1, y1| Some(NBox::new(x0, y0, x1, y1));
    match name.trim().to_ascii_lowercase().as_str() {
        "" | "full" | "all" | "screen" => b(0.0, 0.0, 1.0, 1.0),
        "center" | "centre" | "character" | "middle" => b(0.25, 0.2, 0.75, 0.85),
        "top" => b(0.0, 0.0, 1.0, 0.4),
        "bottom" => b(0.0, 0.6, 1.0, 1.0),
        "left" => b(0.0, 0.0, 0.5, 1.0),
        "right" => b(0.5, 0.0, 1.0, 1.0),
        "top-left" | "minimap" => b(0.0, 0.0, 0.4, 0.4),
        "top-right" => b(0.6, 0.0, 1.0, 0.4),
        "bottom-left" => b(0.0, 0.6, 0.5, 1.0),
        "bottom-right" => b(0.5, 0.6, 1.0, 1.0),
        custom => {
            let values: Vec<f32> = custom
                .split(',')
                .map(|v| v.trim().parse::<f32>())
                .collect::<Result<_, _>>()
                .ok()?;
            let [x0, y0, x1, y1] = values[..] else {
                return None;
            };
            let ok = |v: f32| (0.0..=1.0).contains(&v);
            (ok(x0) && ok(y0) && ok(x1) && ok(y1) && x1 - x0 >= 0.02 && y1 - y0 >= 0.02)
                .then(|| NBox::new(x0, y0, x1, y1))
        }
    }
}

/// Byte strings compared in a time that does not depend on where they
/// differ.
fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |d, (x, y)| d | (x ^ y)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &str = "0123456789abcdef0123456789abcdef";

    fn get(target: &str) -> Request {
        request("GET", target, &[], &[])
    }

    fn request(method: &str, target: &str, body: &[u8], headers: &[(&str, &str)]) -> Request {
        let (path, query) = match target.split_once('?') {
            Some((p, q)) => (p, crate::phone::http::parse_query(q)),
            None => (target, Vec::new()),
        };
        Request {
            method: method.into(),
            path: path.into(),
            query,
            headers: headers
                .iter()
                .map(|(n, v)| (n.to_string(), v.to_string()))
                .collect(),
            body: body.to_vec(),
        }
    }

    fn json_of(response: &Response) -> Value {
        serde_json::from_slice(&response.body).unwrap()
    }

    #[test]
    fn only_the_key_and_never_through_a_tunnel() {
        let board = Board::new(KEY.into());
        assert_eq!(board.handle(&get("/local/hello")).status, 403);
        assert_eq!(board.handle(&get("/local/hello?k=nope")).status, 403);
        assert_eq!(
            board.handle(&get(&format!("/local/hello?k={KEY}"))).status,
            200
        );
        let header = request("GET", "/local/hello", &[], &[("x-maplesyrup-key", KEY)]);
        assert_eq!(board.handle(&header).status, 200);
        // The right key, but through Cloudflare: not answered.
        let tunneled = request(
            "GET",
            &format!("/local/hello?k={KEY}"),
            &[],
            &[("cf-ray", "8a1b2c3d4e5f-TLV")],
        );
        assert_eq!(board.handle(&tunneled).status, 404);
        assert!(!board.connected(), "hello is not a poll");
    }

    #[test]
    fn a_new_bridge_starts_from_now_and_then_gets_each_event_once() {
        let board = Board::new(KEY.into());
        board.post(
            "death",
            "The character died.",
            "The character died.\n…".into(),
        );
        let first = board.handle(&get(&format!("/local/events?k={KEY}&fresh=1")));
        let first = json_of(&first);
        assert_eq!(first["events"].as_array().unwrap().len(), 0, "old news");
        assert_eq!(first["last"], 1);
        assert_eq!(first["boot"].as_str().unwrap().len(), 8);
        // A bridge is there; Claude takes no events yet.
        assert!(board.bridge_seen_ago().is_some());
        assert!(!board.connected());
        board.handle(&get(&format!("/local/events?k={KEY}&fresh=1&listen=1")));
        assert!(board.connected(), "a Claude Code session listens");
        assert!(!board.quit_requested());
        board.handle(&request(
            "POST",
            &format!("/local/quit?k={KEY}"),
            b"{}",
            &[],
        ));
        assert!(board.quit_requested());
        board.post("map", "New map: Ellinia (was Henesys).", "…".into());
        board.post("hp", "HP low: 28%.", "…".into());
        let next = json_of(&board.handle(&get(&format!("/local/events?k={KEY}&since=1"))));
        let kinds: Vec<&str> = next["events"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["kind"].as_str().unwrap())
            .collect();
        assert_eq!(kinds, ["map", "hp"]);
        assert_eq!(next["last"], 3);
        // Nothing newer: a poll without waiting comes back empty.
        let none = json_of(&board.handle(&get(&format!("/local/events?k={KEY}&since=3"))));
        assert!(none["events"].as_array().unwrap().is_empty());
        assert_eq!(none["last"], 3);
    }

    #[test]
    fn a_poll_waits_for_the_next_event() {
        let board = Board::new(KEY.into());
        // Nothing was ever posted: the first poll after a fresh start is
        // `since=0`, and it waits like any other.
        let got = json_of(&board.handle(&get(&format!("/local/events?k={KEY}&fresh=1"))));
        let since = got["last"].as_u64().unwrap();
        assert_eq!(since, 0);
        // (Posted only now: a test thread held up longer than the poster's
        // nap — a busy Windows runner does that — must not see it early.)
        let poster = Arc::clone(&board);
        let started = Instant::now();
        let handle = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(150));
            poster.post("heard", "He said: hi", "He said: hi".into());
        });
        let got = json_of(&board.handle(&get(&format!(
            "/local/events?k={KEY}&since={since}&wait=5000"
        ))));
        handle.join().unwrap();
        assert_eq!(got["events"][0]["kind"], "heard");
        assert!(
            started.elapsed() < Duration::from_secs(4),
            "it came when posted"
        );
    }

    #[test]
    fn says_are_kept_for_the_main_loop_and_status_says_the_reading_and_recent_news() {
        let board = Board::new(KEY.into());
        let say = |text: &str| {
            board.handle(&request(
                "POST",
                &format!("/local/say?k={KEY}"),
                json!({"text": text}).to_string().as_bytes(),
                &[],
            ))
        };
        assert_eq!(say("   ").status, 400);
        assert_eq!(say(" שלום מיכאל ").status, 200);
        assert_eq!(board.take_says(), vec!["שלום מיכאל".to_string()]);
        assert!(board.take_says().is_empty());
        board.set_status(json!({"hp": 62}), "HP 62%.".into());
        board.post("heard", "He said: hi", "…".into());
        board.post("level_up", "Level up: 16 → 17.", "…".into());
        let status = json_of(&board.handle(&get(&format!("/local/status?k={KEY}"))));
        assert_eq!(status["text"], "HP 62%.");
        assert_eq!(status["status"]["hp"], 62);
        assert_eq!(status["recent"][0]["what"], "Level up: 16 → 17.");
        assert_eq!(
            status["recent"].as_array().unwrap().len(),
            1,
            "what he said is not news"
        );
    }

    #[test]
    fn a_line_for_words_he_has_moved_on_from_is_not_said() {
        let board = Board::new(KEY.into());
        let say = |body: Value| {
            board.handle(&request(
                "POST",
                &format!("/local/say?k={KEY}"),
                body.to_string().as_bytes(),
                &[],
            ))
        };
        let take = |since: u64| {
            json_of(&board.handle(&get(&format!(
                "/local/events?k={KEY}&since={since}&listen=1"
            ))))
        };
        // He asks for a joke; the bridge takes it to Claude, which answers.
        let joke = board.post("heard", "He said: \"tell me a joke\"", "…".into());
        take(0);
        assert_eq!(
            say(json!({"text": "Why did the slime cross the road?", "answers": joke})).status,
            200
        );
        // He asks something else before the rest of the joke.
        let hp = board.post("heard", "He said: \"how much HP do I have\"", "…".into());
        take(joke);
        let late = say(json!({"text": "To get to Henesys!", "answers": joke}));
        assert_eq!(late.status, 409, "the rest of the joke is no longer wanted");
        let why = json_of(&late)["error"].as_str().unwrap().to_string();
        assert!(
            why.contains("how much HP do I have") && why.contains(&format!("heard_id {hp}")),
            "{why}"
        );
        // A line naming nothing, the moment his words went to Claude, was
        // written before Claude read them.
        assert_eq!(say(json!({"text": "Nice level!"})).status, 409);
        // The answer to his newest is said (a number in a string too); a
        // line naming nothing too, once Claude had time to read them.
        assert_eq!(
            say(json!({"text": "62%.", "answers": hp.to_string()})).status,
            200
        );
        board.lock().newest.as_mut().unwrap().taken = Instant::now().checked_sub(SAY_GRACE);
        assert_eq!(say(json!({"text": "Nice level!"})).status, 200);
        assert_eq!(
            board.take_says(),
            vec!["Why did the slime cross the road?", "62%.", "Nice level!"]
        );
        assert_eq!(
            board.take_refused(),
            vec!["To get to Henesys!", "Nice level!"]
        );
        // He talks over Claude's voice: its lines wait for what he says…
        board.talked_over();
        let waiting = say(json!({"text": "As I was saying…", "answers": hp}));
        assert_eq!(waiting.status, 409);
        assert!(
            json_of(&waiting)["error"]
                .as_str()
                .unwrap()
                .contains("talked over")
        );
        // …which comes, and is answered.
        let what = board.post("heard", "He said: \"what was that\"", "…".into());
        take(hp);
        assert_eq!(
            say(json!({"text": "A slime.", "answers": what})).status,
            200
        );
        assert_eq!(board.take_says(), vec!["A slime."]);
    }

    #[test]
    fn his_words_wait_for_claude_until_a_line_is_said_and_then_maplesyrup_may_answer_them() {
        let board = Board::new(KEY.into());
        let say = |body: Value| {
            board.handle(&request(
                "POST",
                &format!("/local/say?k={KEY}"),
                body.to_string().as_bytes(),
                &[],
            ))
        };
        // Posted: they wait, from when they were posted (no bridge took them).
        let first = board.post("heard", "He said: \"where do I level\"", "…".into());
        assert!(board.waiting_since(first).is_some());
        // Claude's answer is said: they wait no more.
        assert_eq!(
            say(json!({"text": "Ant Tunnel.", "answers": first})).status,
            200
        );
        assert_eq!(board.waiting_since(first), None);
        // Quiet on purpose ("shut up", or words for someone else): answered,
        // and nothing to say; without his words named, nothing to do.
        let hush = board.post("heard", "He said: \"shut up\"", "…".into());
        assert_eq!(say(json!({"text": "", "answers": hush})).status, 200);
        assert_eq!(board.waiting_since(hush), None);
        assert_eq!(say(json!({"text": "  "})).status, 400);
        // Newer words take the place of older ones.
        let second = board.post("heard", "He said: \"how much HP\"", "…".into());
        let third = board.post("heard", "He said: \"how much HP do I have\"", "…".into());
        assert_eq!(board.waiting_since(second), None);
        assert!(board.waiting_since(third).is_some());
        // Claude took too long: MapleSyrup answers them, and Claude's line
        // for them is refused, with why; a line of its own still goes.
        board.answered_here(third);
        assert_eq!(board.waiting_since(third), None);
        let late = say(json!({"text": "62%.", "answers": third}));
        assert_eq!(late.status, 409);
        assert!(
            json_of(&late)["error"]
                .as_str()
                .unwrap()
                .contains("MapleSyrup answered him itself")
        );
        board.lock().newest.as_mut().unwrap().taken = Instant::now().checked_sub(SAY_GRACE);
        assert_eq!(say(json!({"text": "Nice level!"})).status, 200);
        assert_eq!(board.take_says(), vec!["Ant Tunnel.", "Nice level!"]);
    }

    #[test]
    fn a_line_of_its_own_about_an_event_is_not_said_once_the_game_changed() {
        let board = Board::new(KEY.into());
        let say = |body: Value| {
            board.handle(&request(
                "POST",
                &format!("/local/say?k={KEY}"),
                body.to_string().as_bytes(),
                &[],
            ))
        };
        board.set_status(
            json!({"dead": true, "map": "Snail Hunting Ground I"}),
            "…".into(),
        );
        board.post("death", "The character died.", "…".into());
        // Still dead: said.
        assert_eq!(say(json!({"text": "Ouch. Revive in town."})).status, 200);
        // Up again before the line came: not said, and Claude is told why.
        board.set_status(
            json!({"dead": false, "map": "Snail Hunting Ground I"}),
            "…".into(),
        );
        let late = say(json!({"text": "You died, click Revive in Town."}));
        assert_eq!(late.status, 409);
        assert!(
            json_of(&late)["error"]
                .as_str()
                .unwrap()
                .contains("alive again")
        );
        // A new map since the event, the same.
        board.post("map", "New map: Henesys.", "…".into());
        board.set_status(json!({"dead": false, "map": "Ellinia"}), "…".into());
        assert_eq!(say(json!({"text": "Henesys has a shop."})).status, 409);
        // An answer to his words is said whatever changed.
        let hp = board.post("heard", "He said: \"how much HP\"", "…".into());
        assert_eq!(say(json!({"text": "HP {hp}.", "answers": hp})).status, 200);
        assert_eq!(board.take_says(), vec!["Ouch. Revive in town.", "HP {hp}."]);
    }

    #[test]
    fn claude_moves_the_bar_to_a_level_it_has_for_the_main_loop_to_take() {
        let board = Board::new(KEY.into());
        let tone = |tone: &str| {
            board.handle(&request(
                "POST",
                &format!("/local/tone?k={KEY}"),
                json!({"tone": tone}).to_string().as_bytes(),
                &[],
            ))
        };
        let moved = tone(" Unhinged ");
        assert_eq!(moved.status, 200);
        assert_eq!(json_of(&moved)["tone"], "unhinged");
        assert_eq!(board.take_tone().as_deref(), Some("unhinged"));
        assert_eq!(board.take_tone(), None);
        // A level it doesn't have: refused, with the ones it has.
        let refused = tone("wild");
        assert_eq!(refused.status, 400);
        assert!(
            json_of(&refused)["error"]
                .as_str()
                .unwrap()
                .contains("kids, friendly, blunt, savage, unhinged")
        );
        assert_eq!(board.take_tone(), None);
    }

    #[test]
    fn look_gives_the_screen_or_part_of_it_while_the_game_is_in_front() {
        let board = Board::new(KEY.into());
        assert_eq!(
            board.handle(&get(&format!("/local/look?k={KEY}"))).status,
            404
        );
        let frame = RgbaImage::from_pixel(3840, 2160, image::Rgba([10, 120, 200, 255]));
        board.set_frame(Some(Arc::new(frame)));
        let full = json_of(&board.handle(&get(&format!("/local/look?k={KEY}"))));
        assert_eq!(full["mime"], "image/jpeg");
        assert_eq!(full["width"], 1568);
        assert!(full["data"].as_str().unwrap().len() > 100);
        let corner = json_of(&board.handle(&get(&format!("/local/look?k={KEY}&region=top-left"))));
        assert_eq!(corner["region"], "top-left");
        assert_eq!(corner["width"], 1536, "a 1536-wide corner is shown whole");
        let custom = board.handle(&get(&format!("/local/look?k={KEY}&region=0.4,0.4,0.6,0.6")));
        assert_eq!(custom.status, 200);
        assert_eq!(
            board
                .handle(&get(&format!("/local/look?k={KEY}&region=nowhere")))
                .status,
            400
        );
    }

    #[test]
    fn a_picture_of_the_game_is_saved_whole_for_claude_to_edit() {
        let dir = std::env::temp_dir().join(format!(
            "ms-claude-pictures-{}",
            crate::phone::tls::random_hex(4)
        ));
        let board = Board::new(KEY.into());
        let take =
            |region: &str| board.handle(&get(&format!("/local/picture?k={KEY}&region={region}")));
        assert_eq!(take("full").status, 503, "nowhere to keep it");
        board.set_pictures_dir(dir.clone());
        assert_eq!(take("full").status, 404, "the game isn't in front");
        let frame = RgbaImage::from_pixel(3840, 2160, image::Rgba([200, 40, 90, 255]));
        board.set_frame(Some(Arc::new(frame)));
        let saved = json_of(&take("center"));
        let path = std::path::PathBuf::from(saved["path"].as_str().unwrap());
        assert!(path.starts_with(&dir), "{path:?}");
        assert_eq!(saved["width"], 1920, "at full size");
        let picture = image::open(&path).unwrap();
        assert_eq!((picture.width(), picture.height()), (1920, 1404));
        assert_eq!(take("nowhere").status, 400);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_wiki_and_the_players_file_are_kept_and_found() {
        let dir = std::env::temp_dir().join(format!(
            "ms-claude-board-{}",
            crate::phone::tls::random_hex(4)
        ));
        let board = Board::new(KEY.into());
        // Nothing kept here: said so.
        assert_eq!(
            board
                .handle(&get(&format!("/local/wiki?k={KEY}&q=slime")))
                .status,
            503
        );
        board.set_learning(crate::ai::Learning::load(&dir));
        let post = |path: &str, body: Value| {
            board.handle(&request(
                "POST",
                &format!("{path}?k={KEY}"),
                body.to_string().as_bytes(),
                &[],
            ))
        };
        assert_eq!(
            post("/local/wiki", json!({"about": "", "answer": "x"})).status,
            400
        );
        let saved = post(
            "/local/wiki",
            json!({
                "about": "Where to hunt Blue Mushrooms in Classic World",
                "answer": "Henesys Hunting Ground I, south of Henesys.",
                "source": "https://example.org/blue-mushroom",
            }),
        );
        assert_eq!(saved.status, 200);
        let found = json_of(&board.handle(&get(&format!(
            "/local/wiki?k={KEY}&q=blue%20mushrooms%20hunt"
        ))));
        assert_eq!(found["size"], 1);
        let answer = found["found"][0]["answer"].as_str().unwrap();
        assert!(answer.contains("Henesys Hunting Ground I"), "{answer}");
        assert!(answer.contains("(source: https://example.org/blue-mushroom)"));
        let none = json_of(&board.handle(&get(&format!("/local/wiki?k={KEY}&q=zakum"))));
        assert!(none["found"].as_array().unwrap().is_empty());
        // The player's file: a fact kept once, and read back with his
        // character as read now.
        assert_eq!(post("/local/player", json!({"fact": "  "})).status, 400);
        let first = json_of(&post(
            "/local/player",
            json!({"fact": "Wants to reach level 30 this week."}),
        ));
        assert_eq!(first["new"], true);
        let again = json_of(&post(
            "/local/player",
            json!({"fact": "Wants to reach level 30 this week."}),
        ));
        assert_eq!(again["new"], false);
        board.set_status(
            json!({"name": "WANWANBUJIO", "job": "Magician", "level": 18, "classic": true}),
            String::new(),
        );
        let player = json_of(&board.handle(&get(&format!("/local/player?k={KEY}"))));
        let text = player["text"].as_str().unwrap();
        assert!(
            text.contains("WANWANBUJIO, Magician, level 18, MapleStory Classic World"),
            "{text}"
        );
        assert!(
            text.contains("Wants to reach level 30 this week."),
            "{text}"
        );
        // And how he set it to talk to him: the level the bar is at.
        let level = crate::companion::spectrum::Spectrum::get().of_voice(Default::default());
        assert!(text.ends_with(&crate::claude::tone(level)), "{text}");
        let file = std::fs::read_to_string(dir.join("about-me.txt")).unwrap();
        assert_eq!(file.matches("level 30").count(), 1, "{file}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn regions_by_name_and_by_numbers() {
        assert!(region("full").is_some());
        assert!(region("Top-Left").is_some());
        assert!(region("0.1,0.1,0.3,0.3").is_some());
        assert!(region("0.1,0.1,0.11,0.3").is_none(), "too thin");
        assert!(region("0.1,0.1,1.3,0.3").is_none(), "off the screen");
        assert!(region("1,2,3").is_none());
        assert!(region("somewhere").is_none());
    }
}
