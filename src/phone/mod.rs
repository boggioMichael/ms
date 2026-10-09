//! The phone link: a page the phone opens from a QR code, which turns it
//! into MapleSyrup's microphone and second screen.
//!
//! ```text
//!   phone (Safari / Chrome)                      this PC
//!   ─────────────────────                        ───────
//!   microphone ── 16 kHz samples ─ POST /api/audio ──▶ Mic: level, speaking, WAV
//!   speech recognition ─ sentence ─ POST /api/heard ──▶ inbox ─▶ companion
//!          └── words so far ─ POST /api/hearing ───────▶ inbox ─▶ talked over?
//!   buttons ───────────── command ─ POST /api/command ▶ inbox ─▶ companion
//!   screen ◀──────── HP/MP/EXP, replies ─ GET /api/state ◀── status, messages
//! ```
//!
//! `/api/state` can wait (`wait=` milliseconds) until there is something
//! new — a line, a spoken clip, a reply cut short — so the phone hears of
//! it at once instead of at its next look. It lists the clips the phone can
//! still fetch with what each is (`kind`) and how old (`age_ms`): a phone
//! that could play none before a tap plays no stale news at the tap.
//!
//! On a live call (`crate::ai::live`) the phone talks to OpenAI itself and
//! asks the PC for a short-lived key (`/api/live`), the screen
//! (`/api/eyes`), MapleSyrup's tools (`/api/tool`) and, when the attitude
//! changes mid-call, the call's instructions again (`/api/instructions`);
//! it tells the PC what was said (`/api/said`) and when MapleSyrup's voice
//! is playing (`/api/talking`, to turn the game down). MapleSyrup's own
//! lines for the call come as messages with `speak` and, behind a
//! watcher's line, the reading (`fact`) and whether it is said however
//! late (`urgent`: a death, a level-up); a line the call never said (it
//! waited too long, or the call ended) is reported back (`/api/turn`).
//!
//! While the session is recorded ([`Recording`]), the phone's sound goes
//! into the recording too: its microphone and what it plays (a live call's
//! voice in a second channel of `/api/audio`, spoken lines told by
//! `/api/playing`), each placed by how long ago it was heard.
//!
//! The page is served over HTTPS because phone browsers give the microphone
//! only to secure pages: on the local network with a certificate made on
//! this PC ([`tls`]), or through a Cloudflare quick tunnel ([`tunnel`]),
//! which brings its own. Every API call carries the link's key, so only a
//! phone that scanned the code can use it.

pub mod audio;
pub mod client;
pub mod http;
pub mod net;
pub mod qr;
pub mod tls;
pub mod tunnel;

use std::collections::VecDeque;
use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::companion::Kind;
use audio::{Mic, MicSummary};
use http::{Conn, HttpError, Request, Response};
use image::RgbaImage;

/// The phone page, with its script and styles.
pub const PAGE: &str = include_str!("page.html");
/// The dog on the page: brought to life there (dog.js), from the
/// mascot's parts (dog-parts.png).
pub const DOG_JS: &str = include_str!("dog.js");
pub const DOG_PARTS: &[u8] = crate::app::dog::PARTS_PNG;

/// Audio chunks larger than this are refused (a quarter second is ~8 kB).
const MAX_AUDIO_BODY: usize = 256 * 1024;
const MAX_JSON_BODY: usize = 16 * 1024;
/// Messages kept for the page.
const KEEP_MESSAGES: usize = 100;
/// A page opened fresh gets this many of the latest messages.
const FIRST_MESSAGES: usize = 20;
/// The phone counts as connected while it asked for something this recently.
const PHONE_STALE: Duration = Duration::from_secs(5);
const MAX_CONNECTIONS: usize = 64;
const IDLE_TIMEOUT: Duration = Duration::from_secs(30);

/// Where the companion's replies are spoken.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VoiceOn {
    /// The PC's voice (heard by the room, and by a stream capturing desktop audio).
    Pc,
    /// The phone speaks them (in your ear, not on the stream).
    Phone,
    Both,
    /// Nowhere: only written.
    Off,
}

impl VoiceOn {
    pub fn pc(self) -> bool {
        matches!(self, VoiceOn::Pc | VoiceOn::Both)
    }
    pub fn phone(self) -> bool {
        matches!(self, VoiceOn::Phone | VoiceOn::Both)
    }
    pub fn parse(text: &str) -> Option<VoiceOn> {
        match text.trim().to_ascii_lowercase().as_str() {
            "pc" | "computer" => Some(VoiceOn::Pc),
            "phone" => Some(VoiceOn::Phone),
            "both" => Some(VoiceOn::Both),
            "off" | "none" => Some(VoiceOn::Off),
            _ => None,
        }
    }
}

/// Something the phone sent, for the main loop to act on.
#[derive(Debug, Clone, PartialEq)]
pub enum Inbound {
    /// A sentence its speech recognition heard.
    Heard(String),
    /// What it is hearing right now, not yet a whole sentence (the words
    /// since the last sentence it sent): whether the player is talking over
    /// MapleSyrup, or still talking.
    Hearing(String),
    /// The player talked over MapleSyrup's voice on the phone (heard by the
    /// phone itself): stop talking.
    Interrupt,
    /// A button: a command's word.
    Command(String),
    /// The page opened, from this browser (`agent`), with its live-call
    /// toggle on or off (`live`: whether it will open a call once the
    /// player taps Listen), after the phone had been gone this long
    /// (`away`; None: the link had not seen a phone before).
    Hello {
        agent: String,
        live: bool,
        away: Option<Duration>,
    },
    /// Where replies should be spoken now.
    Voice(VoiceOn),
    /// Answer everything said (true), or only after "syrup" (false).
    Listen(bool),
    /// Speak up on its own while they play (true), or only when asked.
    Coach(bool),
    /// Forget a thing it was taught (by its id).
    Forget(String),
    /// The player's language (a locale such as `he-IL`).
    Language(String),
    /// On a live call: something said, by the player ("player") or by
    /// MapleSyrup ("maplesyrup"), for the log.
    Said { who: String, text: String },
    /// A live call started (true) or ended (false): MapleSyrup's own lines
    /// go to the call to be said, not to the PC's voice.
    Live(bool),
    /// MapleSyrup's voice on the phone started (true) or stopped (false).
    Talking(bool),
    /// The player started (true) or stopped (false) talking on a live call
    /// (the call hears them directly; the PC learns of it from this).
    PlayerTalking(bool),
    /// A tool run for the call changed something: a line to show, or a
    /// command (mark, mute, unmute).
    Effect(crate::ai::Effect),
    /// Start (true) or stop (false) recording the session.
    Record(bool),
    /// How a turn went on a live call ("jumped in": MapleSyrup answered
    /// before the player had finished), for the log it learns from.
    Turn(String),
    /// A line handed to the call that the call never said: it waited too
    /// long behind the call's own voice, or the call ended. For the log,
    /// which must not say it was said.
    NotSaid(String),
    /// How MapleSyrup should talk to the player from now on.
    Attitude(crate::companion::Attitude),
    /// The voice to speak in: an ElevenLabs voice's id, or "openai".
    Speaker(String),
    /// About updates: look now, install what is staged, or whether to
    /// update on its own.
    Update(UpdateAsk),
    /// About the workshop (MapleSyrup rewriting itself on this PC).
    Workshop(WorkshopAsk),
}

/// What the phone asks of the workshop.
#[derive(Debug, Clone, PartialEq)]
pub enum WorkshopAsk {
    On(bool),
    /// Make this change.
    Change(String),
    Undo,
    /// Use this coding agent ("claude" or "codex").
    Coder(String),
}

/// What the phone asks of the updater.
#[derive(Debug, Clone, PartialEq)]
pub enum UpdateAsk {
    Check,
    Install,
    Auto(bool),
}

/// What the phone's live call asks the PC for.
pub trait Service: Send + Sync {
    /// A short-lived key for the call, and where to connect: `{key, url,
    /// model, attitude}`. `recent`: the last things said (a call picked up
    /// again); `language`: the phone's language.
    fn live(&self, recent: &[String], language: Option<&str>) -> Result<Value, String>;
    /// The call's instructions as they are now: `{instructions, attitude}`.
    /// The phone asks for them when the attitude changes during a call
    /// (the picker, or the player objecting to the tone) and hands them to
    /// the call, which goes on in the new tone without starting over.
    fn instructions(&self, language: Option<&str>) -> Value;
    /// Run one of MapleSyrup's tools the call's model asked for, on the
    /// frame the player is looking at. Returns what to tell the model.
    fn tool(
        &self,
        name: &str,
        arguments: &str,
        frame: Option<&RgbaImage>,
    ) -> (String, Option<crate::ai::Effect>);
}

/// Where the phone's sound goes while the session is recorded
/// (`crate::app::recorder`). `age`: how many seconds ago the end of the
/// sound (or the moment) was, as near as the phone can tell.
pub trait Recording: Send + Sync {
    /// The phone's microphone, and what the phone played meanwhile (a live
    /// call's voice), as many samples of each.
    fn phone(&self, rate: u32, mic: &[i16], played: Option<&[i16]>, age: f64);
    /// A spoken line the phone started playing `age` ago.
    fn played(&self, rate: u32, samples: &[i16], age: f64);
    /// The phone stopped what it was playing, `age` ago.
    fn stopped(&self, age: f64);
}

/// What the call's model can see: the frame (only while the game is the
/// window in front), and what is read off it.
#[derive(Default, Clone)]
struct Sight {
    frame: Option<Arc<RgbaImage>>,
    snapshot: String,
}

/// The picture for a call: the frame with rulers, as a small JPEG (quick for
/// the call to take in, and within its data channel's messages of up to
/// about 64 kB).
fn eyes_picture(frame: &RgbaImage) -> String {
    use crate::ai::images;
    let mut size = (640u32, 400u32);
    let mut quality = 55u8;
    loop {
        let picture = images::with_rulers(&images::fit(frame, size.0, size.1));
        let url = images::jpeg_url(&picture, quality);
        if url.len() <= 56_000 || size.0 <= 400 {
            return url;
        }
        if quality > 40 {
            quality -= 10;
        } else {
            size = (size.0 * 4 / 5, size.1 * 4 / 5);
        }
    }
}

/// A line on the phone's screen.
#[derive(Debug, Clone, Serialize)]
pub struct Message {
    pub id: u64,
    /// Seconds since MapleSyrup started.
    pub t: f64,
    pub kind: Kind,
    pub text: String,
    /// Whether it is meant to be spoken (when replies are spoken on the phone).
    pub speak: bool,
    /// On a live call, the reading behind one of MapleSyrup's own lines
    /// ("HP 11% (read 0 s ago), MP 40% (read 0 s ago)"): the call passes
    /// the number on, and knows it is newer than any picture it has.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fact: Option<String>,
    /// On a live call, the one line that matters (a death, a level-up):
    /// the call says it however long it waited behind the call's own
    /// voice, where a warning that waited too long is dropped as stale.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub urgent: bool,
}

/// What the console shows about the phone.
#[derive(Debug, Clone, Default)]
pub struct PhoneSummary {
    pub connected: bool,
    pub browser: Option<String>,
    pub mic: MicSummary,
    pub requests: u64,
}

struct State {
    status: Value,
    messages: VecDeque<Message>,
    next_id: u64,
    inbox: Vec<Inbound>,
    mic: Mic,
    voice_on: VoiceOn,
    phone_seen: Option<Instant>,
    browser: Option<String>,
    requests: u64,
    /// The latest spoken lines, for the phone to play in turn.
    clips: VecDeque<Clip>,
    /// How loud each clip is as it goes (for the dog's mouth).
    mouths: VecDeque<(u64, Vec<u8>)>,
    last_clip: u64,
    /// How many times a reply was cut short: the phone stops its clips.
    cut: u64,
    /// What a live call's model can see.
    sight: Sight,
    /// Pictures of the things it was taught: by id, with a tag that changes
    /// with the picture.
    thumbs: std::collections::HashMap<String, (String, Arc<Vec<u8>>)>,
}

/// A locale such as `en-US` or `zh-Hant` (letters, digits and dashes).
fn plausible_locale(text: &str) -> bool {
    (2..=16).contains(&text.len())
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// How many spoken lines the phone can still fetch.
const CLIPS_KEPT: usize = 8;

/// A spoken line for the phone: its number, what it is (a warning, news, a
/// reply, a note) and when it was made — the page drops a warning or news
/// that waited too long for the tap — and the WAV.
struct Clip {
    seq: u64,
    kind: Kind,
    made: Instant,
    wav: Arc<Vec<u8>>,
}

/// Everything the phone link shares between the server's threads and the
/// main loop.
pub struct Hub {
    key: String,
    started: Instant,
    /// This run of the program: a page open across a restart (the
    /// self-updater's) sees it change and starts over with it, since the
    /// new program numbers its lines and clips from one again and has
    /// not heard the page's hello.
    boot: String,
    state: Mutex<State>,
    /// Told when there is something new for the phone.
    changed: Condvar,
    /// What answers a live call's requests (with an OpenAI key).
    service: Mutex<Option<Arc<dyn Service>>>,
    /// Where the phone's sound goes while the session is recorded.
    recording: Mutex<Option<Arc<dyn Recording>>>,
}

impl Hub {
    /// `key`: the link's secret. `record_to`: keep the phone's audio in this WAV file.
    pub fn new(key: String, record_to: Option<PathBuf>, voice_on: VoiceOn) -> Arc<Hub> {
        Arc::new(Hub {
            key,
            started: Instant::now(),
            boot: tls::random_hex(4),
            state: Mutex::new(State {
                status: json!({}),
                messages: VecDeque::new(),
                next_id: 1,
                inbox: Vec::new(),
                mic: Mic::new(record_to),
                voice_on,
                phone_seen: None,
                browser: None,
                requests: 0,
                clips: VecDeque::new(),
                mouths: VecDeque::new(),
                last_clip: 0,
                cut: 0,
                sight: Sight::default(),
                thumbs: std::collections::HashMap::new(),
            }),
            changed: Condvar::new(),
            service: Mutex::new(None),
            recording: Mutex::new(None),
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        // A panic in one request must not take the link down for the rest.
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn key(&self) -> &str {
        &self.key
    }

    /// The game and session figures the page shows, replaced every tick.
    pub fn set_status(&self, status: Value) {
        self.lock().status = status;
    }

    /// Add a line to the phone's screen; returns its id.
    pub fn post(&self, kind: Kind, text: &str, speak: bool) -> u64 {
        self.post_with_fact(kind, text, speak, None, false)
    }

    /// Add a line of MapleSyrup's own for a live call to say, with the
    /// reading behind it (`fact`, when there is one), and whether it is
    /// said however late it comes (`urgent`: a death, a level-up).
    pub fn post_with_fact(
        &self,
        kind: Kind,
        text: &str,
        speak: bool,
        fact: Option<&str>,
        urgent: bool,
    ) -> u64 {
        let t = self.started.elapsed().as_secs_f64();
        let mut state = self.lock();
        let id = state.next_id;
        state.next_id += 1;
        state.messages.push_back(Message {
            id,
            t,
            kind,
            text: text.to_string(),
            speak,
            fact: fact.map(str::to_string),
            urgent,
        });
        while state.messages.len() > KEEP_MESSAGES {
            state.messages.pop_front();
        }
        drop(state);
        self.changed.notify_all();
        id
    }

    /// What the phone sent since the last call.
    pub fn take_inbox(&self) -> Vec<Inbound> {
        std::mem::take(&mut self.lock().inbox)
    }

    /// Hand the phone a spoken line (a WAV) of this `kind` to play after
    /// the ones before it. Returns its number.
    pub fn set_clip(&self, kind: Kind, wav: Vec<u8>) -> u64 {
        let mouth = crate::app::dog::mouth_of_wav(&wav);
        let mut state = self.lock();
        state.last_clip += 1;
        let seq = state.last_clip;
        state.clips.push_back(Clip {
            seq,
            kind,
            made: Instant::now(),
            wav: Arc::new(wav),
        });
        state.mouths.push_back((seq, mouth));
        while state.clips.len() > CLIPS_KEPT {
            state.clips.pop_front();
        }
        while state.mouths.len() > CLIPS_KEPT {
            state.mouths.pop_front();
        }
        drop(state);
        self.changed.notify_all();
        seq
    }

    /// Live calls can be made (an OpenAI key): what answers them.
    pub fn set_service(&self, service: Arc<dyn Service>) {
        if let Ok(mut slot) = self.service.lock() {
            *slot = Some(service);
        }
    }

    fn service(&self) -> Option<Arc<dyn Service>> {
        self.service.lock().ok().and_then(|s| s.clone())
    }

    /// The session is being recorded (Some): the phone's sound goes there
    /// too. None: no longer.
    pub fn set_recording(&self, recording: Option<Arc<dyn Recording>>) {
        if let Ok(mut slot) = self.recording.lock() {
            *slot = recording;
        }
    }

    fn recording(&self) -> Option<Arc<dyn Recording>> {
        self.recording.lock().ok().and_then(|r| r.clone())
    }

    /// What a call's model can see now: the frame (None while the game is
    /// not the window in front), and what is read off it.
    pub fn set_sight(&self, frame: Option<Arc<RgbaImage>>, snapshot: String) {
        self.lock().sight = Sight { frame, snapshot };
    }

    /// The reply was cut short (the player talked over it): the phone stops
    /// what it is playing and skips the clips it has not played.
    pub fn cut(&self) {
        let mut state = self.lock();
        state.cut += 1;
        // Clips not fetched yet are not to be played.
        state.clips.clear();
        drop(state);
        self.changed.notify_all();
    }

    /// The picture of a thing it was taught, for the phone's list.
    pub fn set_thumb(&self, id: &str, tag: &str, png: Vec<u8>) {
        self.lock()
            .thumbs
            .insert(id.to_string(), (tag.to_string(), Arc::new(png)));
    }

    pub fn thumb_tag(&self, id: &str) -> Option<String> {
        self.lock().thumbs.get(id).map(|(tag, _)| tag.clone())
    }

    pub fn voice_on(&self) -> VoiceOn {
        self.lock().voice_on
    }

    pub fn set_voice_on(&self, voice_on: VoiceOn) {
        self.lock().voice_on = voice_on;
    }

    pub fn summary(&self) -> PhoneSummary {
        let now = Instant::now();
        let state = self.lock();
        PhoneSummary {
            connected: state
                .phone_seen
                .is_some_and(|at| now.saturating_duration_since(at) < PHONE_STALE),
            browser: state.browser.clone(),
            mic: state.mic.summary(now),
            requests: state.requests,
        }
    }

    /// Answer one request.
    pub fn handle(&self, request: &Request) -> Response {
        let method = request.method.as_str();
        match (method, request.path.as_str()) {
            ("GET" | "HEAD", "/" | "/index.html") => {
                Response::new(200, "text/html; charset=utf-8", PAGE)
                    .with_header("Referrer-Policy", "no-referrer")
            }
            ("GET", "/health") => Response::text(200, "ok maplesyrup"),
            ("GET", "/dog.js") => Response::new(200, "text/javascript; charset=utf-8", DOG_JS)
                .with_header("Cache-Control", "no-cache"),
            ("GET", "/dog-parts.png") => {
                Response::new(200, "image/png", DOG_PARTS).with_header("Cache-Control", "no-cache")
            }
            ("GET", "/favicon.ico") => Response::empty(204),
            (_, path) if path.starts_with("/api/") => {
                let key = request
                    .param("k")
                    .or_else(|| request.header("x-maplesyrup-key"));
                if key != Some(self.key.as_str()) {
                    return Response::json(
                        403,
                        &json!({"error": "This link is out of date. Scan the code on the PC again."}),
                    );
                }
                // (When the phone was last seen before this request: a
                // hello tells how long it had been gone.)
                let seen = {
                    let mut state = self.lock();
                    let seen = state.phone_seen;
                    state.phone_seen = Some(Instant::now());
                    state.requests += 1;
                    seen
                };
                self.api(method, path, request, seen)
            }
            _ => Response::text(404, "not found"),
        }
    }

    fn api(&self, method: &str, path: &str, request: &Request, seen: Option<Instant>) -> Response {
        let body = || serde_json::from_slice::<Value>(&request.body).unwrap_or(Value::Null);
        let text_field = |name: &str| {
            body()
                .get(name)
                .and_then(Value::as_str)
                .map(|s| s.chars().take(500).collect::<String>())
        };
        match (method, path) {
            ("GET", "/api/state") => {
                let number = |name: &str| -> u64 {
                    request
                        .param(name)
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(0)
                };
                let since = number("since");
                let mut state = self.lock();
                // Wait for something new, up to `wait` milliseconds.
                let wait = Duration::from_millis(number("wait").min(1500));
                if !wait.is_zero() {
                    let (clip, cut) = (number("clip"), number("cut"));
                    let deadline = Instant::now() + wait;
                    while state.next_id - 1 <= since && state.last_clip <= clip && state.cut <= cut
                    {
                        let left = deadline.saturating_duration_since(Instant::now());
                        if left.is_zero() {
                            break;
                        }
                        state = match self.changed.wait_timeout(state, left) {
                            Ok((guard, _)) => guard,
                            Err(poisoned) => poisoned.into_inner().0,
                        };
                    }
                }
                let now = Instant::now();
                let skip = if since == 0 {
                    state.messages.len().saturating_sub(FIRST_MESSAGES)
                } else {
                    0
                };
                let messages: Vec<&Message> = state
                    .messages
                    .iter()
                    .skip(skip)
                    .filter(|m| m.id > since)
                    .collect();
                let clips: Vec<Value> = state
                    .clips
                    .iter()
                    .map(|c| {
                        json!({
                            "seq": c.seq,
                            "kind": c.kind,
                            "age_ms": now.saturating_duration_since(c.made).as_millis() as u64,
                        })
                    })
                    .collect();
                Response::json(
                    200,
                    &json!({
                        "status": state.status,
                        "mic": state.mic.summary(now),
                        "voice_on": state.voice_on,
                        "messages": messages,
                        "last_id": state.next_id - 1,
                        "uptime": self.started.elapsed().as_secs_f64(),
                        "boot": self.boot,
                        "clip": state.last_clip,
                        "clips": clips,
                        "cut": state.cut,
                    }),
                )
            }
            ("GET", "/api/clip") => {
                // The clip asked for by number, or else the latest.
                let wanted: Option<u64> = request.param("seq").and_then(|s| s.parse().ok());
                let clip = {
                    let state = self.lock();
                    match wanted {
                        Some(seq) => state.clips.iter().find(|c| c.seq == seq),
                        None => state.clips.back(),
                    }
                    .map(|c| Arc::clone(&c.wav))
                };
                match clip {
                    Some(wav) => Response::new(200, "audio/wav", wav.as_slice()),
                    None => Response::json(404, &json!({"error": "no such clip"})),
                }
            }
            ("GET", "/api/mouth") => {
                // How loud a clip is as it goes: the dog's mouth follows it.
                let wanted: Option<u64> = request.param("seq").and_then(|s| s.parse().ok());
                let mouth = wanted.and_then(|seq| {
                    self.lock()
                        .mouths
                        .iter()
                        .find(|(n, _)| *n == seq)
                        .map(|(_, m)| m.clone())
                });
                match mouth {
                    Some(levels) => Response::json(
                        200,
                        &json!({"step_ms": crate::app::dog::MOUTH_STEP_MS, "levels": levels}),
                    ),
                    None => Response::json(404, &json!({"error": "no such clip"})),
                }
            }
            ("GET", "/api/thumb") => {
                let id = request.param("id").unwrap_or_default();
                let thumb = self.lock().thumbs.get(id).map(|(_, png)| Arc::clone(png));
                match thumb {
                    Some(png) => Response::new(200, "image/png", png.as_slice())
                        .with_header("Cache-Control", "max-age=60"),
                    None => Response::json(404, &json!({"error": "no such picture"})),
                }
            }
            ("POST", "/api/forget") => match body().get("id").and_then(Value::as_str) {
                Some(id) if !id.is_empty() => {
                    self.lock().inbox.push(Inbound::Forget(id.to_string()));
                    Response::json(200, &json!({"ok": true}))
                }
                _ => Response::json(400, &json!({"error": "which thing?"})),
            },
            ("POST", "/api/listen") => match body().get("always").and_then(Value::as_bool) {
                Some(always) => {
                    self.lock().inbox.push(Inbound::Listen(always));
                    Response::json(200, &json!({"ok": true}))
                }
                None => Response::json(400, &json!({"error": "always must be true or false"})),
            },
            ("POST", "/api/coach") => match body().get("on").and_then(Value::as_bool) {
                Some(on) => {
                    self.lock().inbox.push(Inbound::Coach(on));
                    Response::json(200, &json!({"ok": true}))
                }
                None => Response::json(400, &json!({"error": "on must be true or false"})),
            },
            ("POST", "/api/workshop") => {
                let body = body();
                let text = body
                    .get("text")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .unwrap_or("");
                let ask = match body.get("action").and_then(Value::as_str) {
                    Some("on") => Some(WorkshopAsk::On(true)),
                    Some("off") => Some(WorkshopAsk::On(false)),
                    Some("change") if !text.is_empty() && text.len() <= 4000 => {
                        Some(WorkshopAsk::Change(text.to_string()))
                    }
                    Some("undo") => Some(WorkshopAsk::Undo),
                    Some("coder") if !text.is_empty() => Some(WorkshopAsk::Coder(text.to_string())),
                    _ => None,
                };
                match ask {
                    Some(ask) => {
                        self.lock().inbox.push(Inbound::Workshop(ask));
                        Response::json(200, &json!({"ok": true}))
                    }
                    None => Response::json(
                        400,
                        &json!({"error": "action is on, off, change (with text), undo or coder (with text)"}),
                    ),
                }
            }
            ("POST", "/api/update") => {
                let body = body();
                let ask = match body.get("action").and_then(Value::as_str) {
                    Some("check") => Some(UpdateAsk::Check),
                    Some("install") => Some(UpdateAsk::Install),
                    Some("auto") => body.get("on").and_then(Value::as_bool).map(UpdateAsk::Auto),
                    _ => None,
                };
                match ask {
                    Some(ask) => {
                        self.lock().inbox.push(Inbound::Update(ask));
                        Response::json(200, &json!({"ok": true}))
                    }
                    None => Response::json(
                        400,
                        &json!({"error": "action is check, install or auto (with on)"}),
                    ),
                }
            }
            ("POST", "/api/audio") => {
                let rate: u32 = request
                    .param("rate")
                    .and_then(|r| r.parse().ok())
                    .unwrap_or(16_000);
                if !(4_000..=96_000).contains(&rate) {
                    return Response::json(400, &json!({"error": "unsupported sample rate"}));
                }
                // Two channels: the microphone, and what the phone played
                // meanwhile (a live call's voice, for the recording).
                let channels: usize = request
                    .param("channels")
                    .and_then(|c| c.parse().ok())
                    .unwrap_or(1);
                let samples = audio::samples_from_bytes(&request.body);
                let (mic, played) = match channels {
                    2 => {
                        let frames = samples.as_chunks::<2>().0;
                        let mic: Vec<i16> = frames.iter().map(|f| f[0]).collect();
                        let played: Vec<i16> = frames.iter().map(|f| f[1]).collect();
                        (mic, Some(played))
                    }
                    _ => (samples, None),
                };
                self.lock().mic.ingest(&mic, rate, Instant::now());
                if let Some(recording) = self.recording() {
                    recording.phone(rate, &mic, played.as_deref(), age_of(request));
                }
                Response::empty(204)
            }
            ("POST", "/api/playing") => {
                // The phone started (or stopped short) one of the spoken
                // lines it was handed: for the recording.
                let Some(recording) = self.recording() else {
                    return Response::json(200, &json!({"ok": true, "recording": false}));
                };
                let body = body();
                let age = body["age"].as_f64().unwrap_or(0.0) / 1000.0;
                if body["on"].as_bool() == Some(false) {
                    recording.stopped(age);
                    return Response::json(200, &json!({"ok": true}));
                }
                let seq = body["seq"].as_u64().unwrap_or(0);
                let clip = self
                    .lock()
                    .clips
                    .iter()
                    .find(|c| c.seq == seq)
                    .map(|c| Arc::clone(&c.wav));
                match clip.as_deref().and_then(|wav| audio::wav_samples(wav)) {
                    Some((rate, samples)) => {
                        recording.played(rate, &samples, age);
                        Response::json(200, &json!({"ok": true}))
                    }
                    None => Response::json(404, &json!({"error": "no such clip"})),
                }
            }
            ("POST", "/api/record") => match body().get("on").and_then(Value::as_bool) {
                Some(on) => {
                    let mut state = self.lock();
                    state.inbox.retain(|i| !matches!(i, Inbound::Record(_)));
                    state.inbox.push(Inbound::Record(on));
                    Response::json(200, &json!({"ok": true}))
                }
                None => Response::json(400, &json!({"error": "on must be true or false"})),
            },
            ("POST", "/api/heard") => match text_field("text") {
                Some(text) if !text.trim().is_empty() => {
                    self.lock()
                        .inbox
                        .push(Inbound::Heard(text.trim().to_string()));
                    Response::json(200, &json!({"ok": true}))
                }
                _ => Response::json(400, &json!({"error": "no text"})),
            },
            ("POST", "/api/live") => {
                let Some(service) = self.service() else {
                    return Response::json(503, &json!({"error": "live calls need an OpenAI key"}));
                };
                let recent: Vec<String> = body()["recent"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|l| l.as_str().map(|t| t.chars().take(400).collect()))
                    .take(20)
                    .collect();
                let language = text_field("lang").filter(|l| plausible_locale(l));
                match service.live(&recent, language.as_deref()) {
                    Ok(call) => Response::json(200, &call).with_header("Cache-Control", "no-store"),
                    Err(why) => Response::json(502, &json!({"error": why})),
                }
            }
            ("GET", "/api/instructions") => {
                // The call's instructions as they are now (the attitude
                // changed mid-call): the page hands them to the call.
                let Some(service) = self.service() else {
                    return Response::json(503, &json!({"error": "live calls need an OpenAI key"}));
                };
                let language = request
                    .param("lang")
                    .filter(|l| plausible_locale(l))
                    .map(str::to_string);
                Response::json(200, &service.instructions(language.as_deref()))
                    .with_header("Cache-Control", "no-store")
            }
            ("GET", "/api/eyes") => {
                let sight = self.lock().sight.clone();
                // (`image=0`: what is read off it only; the picture went lately.)
                let image = sight
                    .frame
                    .as_deref()
                    .filter(|_| request.param("image") != Some("0"))
                    .map(eyes_picture);
                Response::json(200, &json!({"snapshot": sight.snapshot, "image": image}))
            }
            ("POST", "/api/tool") => {
                let Some(service) = self.service() else {
                    return Response::json(
                        503,
                        &json!({"error": "no tools without an OpenAI key"}),
                    );
                };
                let name = text_field("name").unwrap_or_default();
                let arguments = body()["arguments"].as_str().unwrap_or("{}").to_string();
                let frame = self.lock().sight.frame.clone();
                let (output, effect) = service.tool(&name, &arguments, frame.as_deref());
                if let Some(effect) = effect {
                    self.lock().inbox.push(Inbound::Effect(effect));
                }
                Response::json(200, &json!({"output": output}))
            }
            ("POST", "/api/said") => {
                let who = text_field("who").unwrap_or_default();
                match text_field("text") {
                    Some(text)
                        if !text.trim().is_empty()
                            && (who == "player" || who == "maplesyrup" || who == "timing") =>
                    {
                        self.lock().inbox.push(Inbound::Said {
                            who,
                            text: text.trim().to_string(),
                        });
                        Response::json(200, &json!({"ok": true}))
                    }
                    _ => Response::json(400, &json!({"error": "who and text"})),
                }
            }
            ("POST", "/api/mode") => match body().get("live").and_then(Value::as_bool) {
                Some(on) => {
                    self.lock().inbox.push(Inbound::Live(on));
                    Response::json(200, &json!({"ok": true}))
                }
                None => Response::json(400, &json!({"error": "live must be true or false"})),
            },
            ("POST", "/api/talking") => match body().get("on").and_then(Value::as_bool) {
                Some(on) if body().get("who").and_then(Value::as_str) == Some("player") => {
                    let mut state = self.lock();
                    state
                        .inbox
                        .retain(|i| !matches!(i, Inbound::PlayerTalking(_)));
                    state.inbox.push(Inbound::PlayerTalking(on));
                    Response::json(200, &json!({"ok": true}))
                }
                Some(on) => {
                    let mut state = self.lock();
                    state.inbox.retain(|i| !matches!(i, Inbound::Talking(_)));
                    state.inbox.push(Inbound::Talking(on));
                    Response::json(200, &json!({"ok": true}))
                }
                None => Response::json(400, &json!({"error": "on must be true or false"})),
            },
            ("POST", "/api/hearing") => match text_field("text") {
                Some(text) if !text.trim().is_empty() => {
                    let mut state = self.lock();
                    // Only the latest matters.
                    state.inbox.retain(|i| !matches!(i, Inbound::Hearing(_)));
                    state.inbox.push(Inbound::Hearing(text.trim().to_string()));
                    Response::json(200, &json!({"ok": true}))
                }
                _ => Response::json(400, &json!({"error": "no text"})),
            },
            ("POST", "/api/turn") => match (text_field("what").as_deref(), text_field("text")) {
                (Some(what @ "jumped in"), _) => {
                    self.lock().inbox.push(Inbound::Turn(what.to_string()));
                    Response::json(200, &json!({"ok": true}))
                }
                // A line the call was handed and never said.
                (Some("dropped"), Some(text)) if !text.trim().is_empty() => {
                    self.lock()
                        .inbox
                        .push(Inbound::NotSaid(text.trim().to_string()));
                    Response::json(200, &json!({"ok": true}))
                }
                _ => Response::json(
                    400,
                    &json!({"error": "what is \"jumped in\", or \"dropped\" with the line's text"}),
                ),
            },
            ("POST", "/api/attitude") => match text_field("attitude")
                .as_deref()
                .and_then(crate::companion::Attitude::parse)
            {
                Some(attitude) => {
                    let mut state = self.lock();
                    state.inbox.retain(|i| !matches!(i, Inbound::Attitude(_)));
                    state.inbox.push(Inbound::Attitude(attitude));
                    Response::json(200, &json!({"ok": true, "attitude": attitude}))
                }
                None => Response::json(
                    400,
                    &json!({"error": "attitude is friendly, blunt or savage"}),
                ),
            },
            ("POST", "/api/speaker") => match text_field("id").filter(|id| {
                !id.is_empty()
                    && id.len() <= 64
                    && id
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
            }) {
                Some(id) => {
                    let mut state = self.lock();
                    state.inbox.retain(|i| !matches!(i, Inbound::Speaker(_)));
                    state.inbox.push(Inbound::Speaker(id));
                    Response::json(200, &json!({"ok": true}))
                }
                None => Response::json(400, &json!({"error": "which voice?"})),
            },
            ("POST", "/api/interrupt") => {
                self.lock().inbox.push(Inbound::Interrupt);
                Response::json(200, &json!({"ok": true}))
            }
            ("POST", "/api/command") => match text_field("command") {
                Some(command) => {
                    self.lock().inbox.push(Inbound::Command(command));
                    Response::json(200, &json!({"ok": true}))
                }
                None => Response::json(400, &json!({"error": "no command"})),
            },
            ("POST", "/api/hello") => {
                let body = body();
                let browser = text_field("agent").unwrap_or_else(|| "a browser".into());
                let lang = text_field("lang").filter(|l| plausible_locale(l));
                // (A page that does not say — one from before the toggle
                // came with the hello — is taken as the toggle on, its default.)
                let live = body.get("live").and_then(Value::as_bool).unwrap_or(true);
                let away = seen.map(|at| at.elapsed());
                let mut state = self.lock();
                state.browser = Some(browser.clone());
                // The language first, so the greeting is in it.
                if let Some(lang) = lang {
                    state.inbox.push(Inbound::Language(lang));
                }
                state.inbox.push(Inbound::Hello {
                    agent: browser,
                    live,
                    away,
                });
                Response::json(200, &json!({"ok": true}))
            }
            ("POST", "/api/lang") => match text_field("lang").filter(|l| plausible_locale(l)) {
                Some(lang) => {
                    self.lock().inbox.push(Inbound::Language(lang));
                    Response::json(200, &json!({"ok": true}))
                }
                None => Response::json(400, &json!({"error": "lang is a locale such as en-US"})),
            },
            ("POST", "/api/voice") => match text_field("on").as_deref().and_then(VoiceOn::parse) {
                Some(on) => {
                    let mut state = self.lock();
                    state.voice_on = on;
                    state.inbox.push(Inbound::Voice(on));
                    Response::json(200, &json!({"ok": true, "voice_on": on}))
                }
                None => Response::json(
                    400,
                    &json!({"error": "voice must be pc, phone, both or off"}),
                ),
            },
            _ => Response::json(404, &json!({"error": "no such call"})),
        }
    }
}

/// How long ago the end of a request's sound was heard, in seconds, as the
/// phone tells it (`age=` milliseconds).
fn age_of(request: &Request) -> f64 {
    request
        .param("age")
        .and_then(|a| a.parse::<f64>().ok())
        .filter(|a| a.is_finite())
        .map(|a| a.clamp(0.0, 10_000.0) / 1000.0)
        .unwrap_or(0.05)
}

/// Serve requests on one connection until it closes, errs or idles out.
fn serve_connection<S: Read + Write>(hub: &Hub, stream: S) {
    let mut conn = Conn::new(stream);
    loop {
        let max = MAX_AUDIO_BODY.max(MAX_JSON_BODY);
        match conn.read_request(max) {
            Ok(Some(request)) => {
                let mut response = hub.handle(&request);
                if request.method == "HEAD" {
                    response.body.clear();
                }
                let keep = !request.wants_close();
                if conn.write_response(&response, keep).is_err() || !keep {
                    return;
                }
            }
            Ok(None) => return,
            Err(HttpError::TooLarge) => {
                let _ = conn.write_response(&Response::text(413, "too large"), false);
                return;
            }
            Err(HttpError::Malformed(why)) => {
                let _ = conn.write_response(&Response::text(400, why), false);
                return;
            }
            Err(HttpError::Io(_)) => return,
        }
    }
}

/// A connection that was not TLS on the TLS port: someone typed `http://`.
/// Point them at `https://`.
fn redirect_to_https(stream: TcpStream, port: u16) {
    let mut conn = Conn::new(stream);
    if let Ok(Some(request)) = conn.read_request(0) {
        let host = request
            .header("host")
            .map(|h| h.split(':').next().unwrap_or(h).to_string())
            .unwrap_or_else(|| "localhost".into());
        let query: Vec<String> = request
            .query
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect();
        let target = if query.is_empty() {
            format!("https://{host}:{port}{}", request.path)
        } else {
            format!("https://{host}:{port}{}?{}", request.path, query.join("&"))
        };
        let response =
            Response::text(301, format!("Use {target}\n")).with_header("Location", target);
        let _ = conn.write_response(&response, false);
    }
}

struct Slot(Arc<AtomicUsize>);

impl Drop for Slot {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

fn take_slot(active: &Arc<AtomicUsize>) -> Option<Slot> {
    if active.fetch_add(1, Ordering::SeqCst) >= MAX_CONNECTIONS {
        active.fetch_sub(1, Ordering::SeqCst);
        return None;
    }
    Some(Slot(Arc::clone(active)))
}

/// Bind `port` on every interface, or the next free one of the ten after it.
fn bind(ip: IpAddr, port: u16) -> Result<TcpListener, String> {
    let mut last = None;
    for candidate in port..port.saturating_add(10) {
        match TcpListener::bind(SocketAddr::new(ip, candidate)) {
            Ok(listener) => return Ok(listener),
            Err(e) => last = Some(e),
        }
    }
    Err(format!(
        "could not listen on port {port} or the nine after it: {}",
        last.map(|e| e.to_string()).unwrap_or_default()
    ))
}

/// Serve the page over TLS on every interface. Returns the port it got.
pub fn serve_tls(
    hub: Arc<Hub>,
    config: Arc<rustls::ServerConfig>,
    port: u16,
) -> Result<u16, String> {
    let listener = bind(IpAddr::V4(Ipv4Addr::UNSPECIFIED), port)?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let active = Arc::new(AtomicUsize::new(0));
    std::thread::Builder::new()
        .name("phone-link".into())
        .spawn(move || {
            for stream in listener.incoming().flatten() {
                let Some(slot) = take_slot(&active) else {
                    continue;
                };
                let hub = Arc::clone(&hub);
                let config = Arc::clone(&config);
                let _ = std::thread::Builder::new()
                    .name("phone-conn".into())
                    .spawn(move || {
                        let _slot = slot;
                        let _ = stream.set_read_timeout(Some(IDLE_TIMEOUT));
                        let _ = stream.set_write_timeout(Some(IDLE_TIMEOUT));
                        let _ = stream.set_nodelay(true);
                        let mut first = [0u8; 1];
                        match stream.peek(&mut first) {
                            // 0x16 starts a TLS handshake; anything else is plain HTTP.
                            Ok(1) if first[0] != 0x16 => redirect_to_https(stream, port),
                            Ok(1) => {
                                let Ok(tls) = rustls::ServerConnection::new(config) else {
                                    return;
                                };
                                serve_connection(&hub, rustls::StreamOwned::new(tls, stream));
                            }
                            _ => {}
                        }
                    });
            }
        })
        .map_err(|e| e.to_string())?;
    Ok(port)
}

/// Serve the page as plain HTTP on this PC only (127.0.0.1), for a tunnel
/// and for tests. Returns the port it got (0 picks any).
pub fn serve_local(hub: Arc<Hub>, port: u16) -> Result<u16, String> {
    let listener = if port == 0 {
        TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).map_err(|e| e.to_string())?
    } else {
        bind(IpAddr::V4(Ipv4Addr::LOCALHOST), port)?
    };
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let active = Arc::new(AtomicUsize::new(0));
    std::thread::Builder::new()
        .name("phone-local".into())
        .spawn(move || {
            for stream in listener.incoming().flatten() {
                let Some(slot) = take_slot(&active) else {
                    continue;
                };
                let hub = Arc::clone(&hub);
                let _ = std::thread::Builder::new()
                    .name("phone-local-conn".into())
                    .spawn(move || {
                        let _slot = slot;
                        let _ = stream.set_read_timeout(Some(IDLE_TIMEOUT));
                        let _ = stream.set_nodelay(true);
                        serve_connection(&hub, stream);
                    });
            }
        })
        .map_err(|e| e.to_string())?;
    Ok(port)
}

/// The link to put in the QR code.
pub fn link(base: &str, key: &str) -> String {
    format!("{}/?k={key}", base.trim_end_matches('/'))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stand-in for what answers live calls.
    struct FakeService;

    impl Service for FakeService {
        fn live(&self, recent: &[String], language: Option<&str>) -> Result<Value, String> {
            Ok(
                json!({"key": "ek_1", "url": "https://x/calls", "recent": recent.len(), "lang": language, "attitude": "savage"}),
            )
        }
        fn instructions(&self, language: Option<&str>) -> Value {
            json!({"instructions": format!("Your attitude: friendly ({})", language.unwrap_or("-")), "attitude": "friendly"})
        }
        fn tool(
            &self,
            name: &str,
            arguments: &str,
            frame: Option<&RgbaImage>,
        ) -> (String, Option<crate::ai::Effect>) {
            (
                format!("{name} {arguments} {}", frame.is_some()),
                Some(crate::ai::Effect::Note(format!("ran {name}"))),
            )
        }
    }

    #[test]
    fn a_live_call_gets_a_key_the_screen_and_the_tools_from_the_pc() {
        let hub = Hub::new("k1".into(), None, VoiceOn::Phone);
        // No key: no calls.
        let r = hub.handle(&request("POST", "/api/live?k=k1", "{}"));
        assert_eq!(r.status, 503);
        assert_eq!(
            hub.handle(&request("GET", "/api/instructions?k=k1", ""))
                .status,
            503
        );
        hub.set_service(Arc::new(FakeService));
        let r = hub.handle(&request(
            "POST",
            "/api/live?k=k1",
            r#"{"recent": ["Player: hi", "MapleSyrup: hey"], "lang": "he-IL"}"#,
        ));
        assert_eq!(r.status, 200);
        let call: Value = serde_json::from_slice(&r.body).unwrap();
        assert_eq!(
            (call["key"].as_str(), call["recent"].as_u64()),
            (Some("ek_1"), Some(2))
        );
        assert_eq!(call["lang"], "he-IL");
        assert_eq!(call["attitude"], "savage");
        // The attitude changed mid-call: the instructions as they are now,
        // for the page to hand to the call.
        let r = hub.handle(&request("GET", "/api/instructions?k=k1&lang=he-IL", ""));
        assert_eq!(r.status, 200);
        let now: Value = serde_json::from_slice(&r.body).unwrap();
        assert_eq!(now["instructions"], "Your attitude: friendly (he-IL)");
        assert_eq!(now["attitude"], "friendly");
        assert_eq!(
            hub.handle(&request("GET", "/api/instructions", "")).status,
            403
        );
        // The screen only once there is one (the game in front); always the snapshot.
        let r = hub.handle(&request("GET", "/api/eyes?k=k1", ""));
        let eyes: Value = serde_json::from_slice(&r.body).unwrap();
        assert!(eyes["image"].is_null());
        let frame = RgbaImage::from_fn(1920, 1080, |x, y| {
            image::Rgba([(x % 256) as u8, (y % 256) as u8, ((x ^ y) % 256) as u8, 255])
        });
        hub.set_sight(Some(Arc::new(frame)), "HP about 75%.".into());
        let r = hub.handle(&request("GET", "/api/eyes?k=k1", ""));
        let eyes: Value = serde_json::from_slice(&r.body).unwrap();
        assert_eq!(eyes["snapshot"], "HP about 75%.");
        let image = eyes["image"].as_str().unwrap();
        // Small enough for the call's data channel, even from a screen of noise.
        assert!(
            image.starts_with("data:image/jpeg;base64,") && image.len() <= 56_000,
            "{}",
            image.len()
        );
        // The picture went lately: what is read off it only.
        let r = hub.handle(&request("GET", "/api/eyes?k=k1&image=0", ""));
        let eyes: Value = serde_json::from_slice(&r.body).unwrap();
        assert_eq!(eyes["snapshot"], "HP about 75%.");
        assert!(eyes["image"].is_null());
        // A tool runs on the PC with the frame; what it changed reaches the main loop.
        let r = hub.handle(&request(
            "POST",
            "/api/tool?k=k1",
            r#"{"name": "remember_fact", "arguments": "{\"fact\":\"x\"}"}"#,
        ));
        let out: Value = serde_json::from_slice(&r.body).unwrap();
        assert_eq!(out["output"], r#"remember_fact {"fact":"x"} true"#);
        // What was said, the call starting, the voice playing: for the main loop.
        for (path, body) in [
            (
                "/api/said?k=k1",
                r#"{"who": "player", "text": "מה הרמה שלי"}"#,
            ),
            ("/api/mode?k=k1", r#"{"live": true}"#),
            ("/api/talking?k=k1", r#"{"on": true}"#),
            ("/api/talking?k=k1", r#"{"on": true, "who": "player"}"#),
            ("/api/coach?k=k1", r#"{"on": false}"#),
            ("/api/update?k=k1", r#"{"action": "install"}"#),
            ("/api/update?k=k1", r#"{"action": "auto", "on": false}"#),
            (
                "/api/workshop?k=k1",
                r#"{"action": "change", "text": "shorter HP warning"}"#,
            ),
            ("/api/workshop?k=k1", r#"{"action": "undo"}"#),
            ("/api/turn?k=k1", r#"{"what": "jumped in"}"#),
            ("/api/attitude?k=k1", r#"{"attitude": "savage"}"#),
            ("/api/speaker?k=k1", r#"{"id": "pNInz6obpgDQGcFmaJgB"}"#),
        ] {
            assert_eq!(
                hub.handle(&request("POST", path, body)).status,
                200,
                "{path}"
            );
        }
        assert_eq!(
            hub.handle(&request(
                "POST",
                "/api/said?k=k1",
                r#"{"who": "someone", "text": "x"}"#
            ))
            .status,
            400
        );
        assert_eq!(
            hub.handle(&request(
                "POST",
                "/api/turn?k=k1",
                r#"{"what": "anything"}"#
            ))
            .status,
            400
        );
        assert_eq!(
            hub.take_inbox(),
            vec![
                Inbound::Effect(crate::ai::Effect::Note("ran remember_fact".into())),
                Inbound::Said {
                    who: "player".into(),
                    text: "מה הרמה שלי".into()
                },
                Inbound::Live(true),
                Inbound::Talking(true),
                Inbound::PlayerTalking(true),
                Inbound::Coach(false),
                Inbound::Update(UpdateAsk::Install),
                Inbound::Update(UpdateAsk::Auto(false)),
                Inbound::Workshop(WorkshopAsk::Change("shorter HP warning".into())),
                Inbound::Workshop(WorkshopAsk::Undo),
                Inbound::Turn("jumped in".into()),
                Inbound::Attitude(crate::companion::Attitude::Savage),
                Inbound::Speaker("pNInz6obpgDQGcFmaJgB".into()),
            ]
        );
        assert_eq!(
            hub.handle(&request("POST", "/api/speaker?k=k1", r#"{"id": "../x"}"#))
                .status,
            400
        );
        assert_eq!(
            hub.handle(&request(
                "POST",
                "/api/attitude?k=k1",
                r#"{"attitude": "loud"}"#
            ))
            .status,
            400
        );
        assert_eq!(
            hub.handle(&request("POST", "/api/update?k=k1", r#"{"action": "fly"}"#))
                .status,
            400
        );
        assert_eq!(
            hub.handle(&request(
                "POST",
                "/api/workshop?k=k1",
                r#"{"action": "change", "text": " "}"#
            ))
            .status,
            400
        );
    }

    fn request(method: &str, target: &str, body: &str) -> Request {
        let (path, query) = match target.split_once('?') {
            Some((p, q)) => (p.to_string(), http::parse_query(q)),
            None => (target.to_string(), Vec::new()),
        };
        Request {
            method: method.into(),
            path,
            query,
            headers: Vec::new(),
            body: body.as_bytes().to_vec(),
        }
    }

    fn hub() -> Arc<Hub> {
        Hub::new("k1".into(), None, VoiceOn::Pc)
    }

    #[test]
    fn the_pages_last_word_as_it_goes_reaches_the_inbox() {
        // The page says its call is off as it is suspended (the screen
        // locks), with a beacon: the key in the query (a beacon sets no
        // header), the JSON as a plain-text body.
        let hub = hub();
        let mut beacon = request("POST", "/api/mode?k=k1", r#"{"live":false}"#);
        beacon
            .headers
            .push(("Content-Type".into(), "text/plain;charset=UTF-8".into()));
        assert_eq!(hub.handle(&beacon).status, 200);
        assert_eq!(hub.take_inbox(), vec![Inbound::Live(false)]);
        // Without the key it is nobody's.
        let mut stray = request("POST", "/api/mode", r#"{"live":false}"#);
        stray
            .headers
            .push(("Content-Type".into(), "text/plain;charset=UTF-8".into()));
        assert_eq!(hub.handle(&stray).status, 403);
        assert!(hub.take_inbox().is_empty());
    }

    fn body(response: &Response) -> Value {
        serde_json::from_slice(&response.body).unwrap()
    }

    #[test]
    fn the_page_is_public_and_the_api_needs_the_key() {
        let hub = hub();
        let page = hub.handle(&request("GET", "/", ""));
        assert_eq!(page.status, 200);
        assert!(String::from_utf8_lossy(&page.body).contains("MapleSyrup"));
        assert_eq!(hub.handle(&request("GET", "/api/state", "")).status, 403);
        assert_eq!(
            hub.handle(&request("GET", "/api/state?k=nope", "")).status,
            403
        );
        assert_eq!(
            hub.handle(&request("GET", "/api/state?k=k1", "")).status,
            200
        );
        assert_eq!(hub.handle(&request("GET", "/nothing", "")).status, 404);
    }

    #[test]
    fn sentences_and_buttons_reach_the_inbox() {
        let hub = hub();
        hub.handle(&request(
            "POST",
            "/api/hello?k=k1",
            r#"{"agent":"iPhone Safari", "live": true}"#,
        ));
        hub.handle(&request(
            "POST",
            "/api/heard?k=k1",
            r#"{"text":"  syrup status "}"#,
        ));
        hub.handle(&request(
            "POST",
            "/api/command?k=k1",
            r#"{"command":"mark"}"#,
        ));
        assert_eq!(
            hub.handle(&request("POST", "/api/heard?k=k1", r#"{"text":""}"#))
                .status,
            400
        );
        assert_eq!(
            hub.take_inbox(),
            vec![
                // (The first hello: the link had not seen a phone before.)
                Inbound::Hello {
                    agent: "iPhone Safari".into(),
                    live: true,
                    away: None
                },
                Inbound::Heard("syrup status".into()),
                Inbound::Command("mark".into()),
            ]
        );
        assert!(hub.take_inbox().is_empty());
        let summary = hub.summary();
        assert!(summary.connected);
        assert_eq!(summary.browser.as_deref(), Some("iPhone Safari"));
        // The page opened again (a reload): how long the phone was gone
        // comes with the hello, and the toggle as it is (on when unsaid:
        // the page's default).
        hub.handle(&request(
            "POST",
            "/api/hello?k=k1",
            r#"{"agent":"iPhone Safari", "live": false}"#,
        ));
        match hub.take_inbox().as_slice() {
            [
                Inbound::Hello {
                    agent,
                    live: false,
                    away: Some(away),
                },
            ] => {
                assert_eq!(agent, "iPhone Safari");
                assert!(*away < Duration::from_secs(5), "{away:?}");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn messages_are_delivered_once_and_a_fresh_page_gets_the_latest() {
        let hub = hub();
        for i in 0..30 {
            hub.post(Kind::Reply, &format!("line {i}"), true);
        }
        let first = body(&hub.handle(&request("GET", "/api/state?k=k1&since=0", "")));
        let messages = first["messages"].as_array().unwrap();
        assert_eq!(messages.len(), FIRST_MESSAGES);
        assert_eq!(messages.last().unwrap()["text"], "line 29");
        let last_id = first["last_id"].as_u64().unwrap();
        hub.post(Kind::Alert, "HP low", true);
        let next = body(&hub.handle(&request(
            "GET",
            &format!("/api/state?k=k1&since={last_id}"),
            "",
        )));
        let messages = next["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["text"], "HP low");
        assert_eq!(messages[0]["kind"], "alert");
    }

    #[test]
    fn audio_moves_the_meter() {
        let hub = hub();
        let samples: Vec<u8> = (0..4000)
            .flat_map(|i| (((i as f32 * 0.3).sin() * 8000.0) as i16).to_le_bytes())
            .collect();
        let mut req = request("POST", "/api/audio?k=k1&rate=16000", "");
        req.body = samples;
        assert_eq!(hub.handle(&req).status, 204);
        let mic = hub.summary().mic;
        assert!(mic.live && mic.level > 0.5, "{mic:?}");
        let mut bad = request("POST", "/api/audio?k=k1&rate=5", "");
        bad.body = vec![0, 0];
        assert_eq!(hub.handle(&bad).status, 400);
    }

    /// A stand-in recording: what it was handed.
    #[derive(Default)]
    struct FakeRecording(Mutex<Vec<String>>);

    impl Recording for FakeRecording {
        fn phone(&self, rate: u32, mic: &[i16], played: Option<&[i16]>, age: f64) {
            self.0.lock().unwrap().push(format!(
                "phone {rate} {} {:?} {age:.3}",
                mic.len(),
                played.map(|p| (p.len(), p[0]))
            ));
        }
        fn played(&self, rate: u32, samples: &[i16], age: f64) {
            self.0
                .lock()
                .unwrap()
                .push(format!("played {rate} {} {age:.3}", samples.len()));
        }
        fn stopped(&self, age: f64) {
            self.0.lock().unwrap().push(format!("stopped {age:.3}"));
        }
    }

    #[test]
    fn while_recording_the_phones_sound_goes_into_the_recording() {
        let hub = hub();
        let stereo: Vec<u8> = (0..2_400)
            .flat_map(|i| {
                let (mic, played) = ((i as i16) % 100, 7_000i16);
                [mic.to_le_bytes(), played.to_le_bytes()].concat()
            })
            .collect();
        let mut req = request("POST", "/api/audio?k=k1&rate=24000&channels=2&age=120", "");
        req.body = stereo.clone();
        // Not recording: only the meter.
        assert_eq!(hub.handle(&req).status, 204);
        let recording = Arc::new(FakeRecording::default());
        hub.set_recording(Some(Arc::clone(&recording) as Arc<dyn Recording>));
        assert_eq!(hub.handle(&req).status, 204);
        // A spoken line the phone started playing a moment ago, then stopped.
        let seq = hub.set_clip(Kind::Reply, crate::ai::wav_bytes(&[100; 2_400], 24_000));
        let started = format!(r#"{{"seq": {seq}, "on": true, "age": 80}}"#);
        assert_eq!(
            hub.handle(&request("POST", "/api/playing?k=k1", &started))
                .status,
            200
        );
        assert_eq!(
            hub.handle(&request(
                "POST",
                "/api/playing?k=k1",
                r#"{"seq": 99, "on": true}"#
            ))
            .status,
            404
        );
        hub.handle(&request(
            "POST",
            "/api/playing?k=k1",
            r#"{"on": false, "age": 30}"#,
        ));
        assert_eq!(
            *recording.0.lock().unwrap(),
            [
                "phone 24000 2400 Some((2400, 7000)) 0.120",
                "played 24000 2400 0.080",
                "stopped 0.030"
            ]
        );
        // Stopped: nothing more goes there.
        hub.set_recording(None);
        assert_eq!(hub.handle(&req).status, 204);
        assert_eq!(recording.0.lock().unwrap().len(), 3);
        // The button.
        hub.handle(&request("POST", "/api/record?k=k1", r#"{"on": true}"#));
        hub.handle(&request("POST", "/api/record?k=k1", r#"{"on": false}"#));
        assert_eq!(hub.take_inbox(), [Inbound::Record(false)]);
        assert_eq!(
            hub.handle(&request("POST", "/api/record?k=k1", "{}"))
                .status,
            400
        );
    }

    #[test]
    fn each_clip_comes_with_how_loud_it_is_for_the_dogs_mouth() {
        let hub = hub();
        let mut samples = vec![0i16; 960];
        samples.extend((0..960).map(|i| if i % 2 == 0 { 9_000 } else { -9_000 }));
        let seq = hub.set_clip(Kind::Reply, crate::ai::wav_bytes(&samples, 24_000));
        let mouth = body(&hub.handle(&request("GET", &format!("/api/mouth?k=k1&seq={seq}"), "")));
        assert_eq!(mouth["step_ms"], 40);
        let levels: Vec<u64> = mouth["levels"]
            .as_array()
            .unwrap()
            .iter()
            .map(|l| l.as_u64().unwrap())
            .collect();
        assert_eq!(levels.len(), 2);
        assert!(levels[0] == 0 && levels[1] > 200, "{levels:?}");
        assert_eq!(
            hub.handle(&request("GET", "/api/mouth?k=k1&seq=99", ""))
                .status,
            404
        );
        // Only with the key.
        assert_eq!(
            hub.handle(&request("GET", &format!("/api/mouth?seq={seq}"), ""))
                .status,
            403
        );
    }

    #[test]
    fn the_phone_gets_spoken_clips_the_dog_and_the_listen_switch() {
        let hub = hub();
        assert_eq!(
            hub.handle(&request("GET", "/api/clip?k=k1", "")).status,
            404
        );
        assert_eq!(hub.set_clip(Kind::Info, b"RIFF....WAVE".to_vec()), 1);
        assert_eq!(hub.set_clip(Kind::Warning, b"RIFF....WAVE2".to_vec()), 2);
        let state = body(&hub.handle(&request("GET", "/api/state?k=k1", "")));
        assert_eq!(state["clip"], 2);
        // Each with what it is and how old: the page plays no stale
        // warning at the tap.
        let listed = |state: &Value| -> Vec<(u64, String)> {
            state["clips"]
                .as_array()
                .unwrap()
                .iter()
                .map(|c| {
                    assert!(c["age_ms"].as_u64().unwrap() < 5_000, "{c}");
                    (
                        c["seq"].as_u64().unwrap(),
                        c["kind"].as_str().unwrap().to_string(),
                    )
                })
                .collect()
        };
        assert_eq!(
            listed(&state),
            [(1, "info".to_string()), (2, "warning".to_string())]
        );
        let clip = hub.handle(&request("GET", "/api/clip?k=k1&seq=2", ""));
        assert_eq!((clip.status, clip.content_type), (200, "audio/wav"));
        assert_eq!(clip.body, b"RIFF....WAVE2");
        // Earlier lines can still be fetched, in turn; long gone ones cannot.
        let first = hub.handle(&request("GET", "/api/clip?k=k1&seq=1", ""));
        assert_eq!(first.body, b"RIFF....WAVE");
        for _ in 0..super::CLIPS_KEPT {
            hub.set_clip(Kind::Alert, b"RIFF....MORE".to_vec());
        }
        assert_eq!(
            hub.handle(&request("GET", "/api/clip?k=k1&seq=1", ""))
                .status,
            404
        );
        let state = body(&hub.handle(&request("GET", "/api/state?k=k1", "")));
        assert_eq!(listed(&state).len(), super::CLIPS_KEPT);
        assert_eq!(listed(&state)[0], (3, "alert".to_string()));
        // Cut: none left to fetch, none listed.
        hub.cut();
        let state = body(&hub.handle(&request("GET", "/api/state?k=k1", "")));
        assert!(listed(&state).is_empty());
        let dog = hub.handle(&request("GET", "/dog.js", ""));
        assert_eq!(
            (dog.status, dog.content_type),
            (200, "text/javascript; charset=utf-8")
        );
        assert!(String::from_utf8_lossy(&dog.body).contains("MSDog"));
        let parts = hub.handle(&request("GET", "/dog-parts.png", ""));
        assert_eq!((parts.status, &parts.body[1..4]), (200, &b"PNG"[..]));
        hub.handle(&request("POST", "/api/listen?k=k1", r#"{"always":false}"#));
        assert_eq!(hub.take_inbox(), vec![Inbound::Listen(false)]);
        // Taught things: their pictures, and forgetting one.
        assert_eq!(
            hub.handle(&request("GET", "/api/thumb?k=k1&id=rune", ""))
                .status,
            404
        );
        hub.set_thumb("rune", "rune-1.png", b"\x89PNG....".to_vec());
        assert_eq!(hub.thumb_tag("rune").as_deref(), Some("rune-1.png"));
        let thumb = hub.handle(&request("GET", "/api/thumb?k=k1&id=rune", ""));
        assert_eq!((thumb.status, thumb.content_type), (200, "image/png"));
        hub.handle(&request("POST", "/api/forget?k=k1", r#"{"id":"rune"}"#));
        assert_eq!(hub.take_inbox(), vec![Inbound::Forget("rune".into())]);
        // The player's language, also with the hello (before it).
        hub.handle(&request("POST", "/api/lang?k=k1", r#"{"lang":"he-IL"}"#));
        assert_eq!(hub.take_inbox(), vec![Inbound::Language("he-IL".into())]);
        assert_eq!(
            hub.handle(&request("POST", "/api/lang?k=k1", r#"{"lang":"<script>"}"#))
                .status,
            400
        );
        hub.handle(&request(
            "POST",
            "/api/hello?k=k1",
            r#"{"agent":"iPhone","lang":"ko-KR"}"#,
        ));
        match hub.take_inbox().as_slice() {
            [
                Inbound::Language(lang),
                Inbound::Hello {
                    agent,
                    live: true,
                    away: Some(_),
                },
            ] => assert_eq!((lang.as_str(), agent.as_str()), ("ko-KR", "iPhone")),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_line_for_the_call_reaches_the_page_with_the_reading_behind_it() {
        let hub = hub();
        hub.post(Kind::Reply, "Marked.", true);
        hub.post_with_fact(
            Kind::Warning,
            "Back off, you're getting shredded.",
            true,
            Some("HP 11% (read 0 s ago), MP 40% (read 0 s ago)"),
            false,
        );
        hub.post_with_fact(
            Kind::Alert,
            "You died. Revive and get back in there.",
            true,
            None,
            true,
        );
        let state = body(&hub.handle(&request("GET", "/api/state?k=k1", "")));
        let messages = state["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 3);
        // An ordinary line carries no reading and no urgency at all (not
        // even a null or a false).
        assert!(messages[0].get("fact").is_none(), "{}", messages[0]);
        assert!(messages[0].get("urgent").is_none(), "{}", messages[0]);
        // A warning carries the reading, and the page knows it for one.
        assert_eq!(
            messages[1]["fact"],
            "HP 11% (read 0 s ago), MP 40% (read 0 s ago)"
        );
        assert_eq!(
            (messages[1]["kind"].as_str(), messages[1]["speak"].as_bool()),
            (Some("warning"), Some(true))
        );
        // A warning waits its turn like any; a death is news, said however
        // late, and with no reading.
        assert!(messages[1].get("urgent").is_none(), "{}", messages[1]);
        assert_eq!(messages[2]["kind"], "alert");
        assert_eq!(messages[2]["urgent"], true);
        assert!(messages[2].get("fact").is_none(), "{}", messages[2]);
        // The page says when a line it was handed was never said.
        assert_eq!(
            hub.handle(&request(
                "POST",
                "/api/turn?k=k1",
                r#"{"what": "dropped", "text": "Back off, you're getting shredded."}"#
            ))
            .status,
            200
        );
        assert_eq!(
            hub.handle(&request("POST", "/api/turn?k=k1", r#"{"what": "dropped"}"#))
                .status,
            400
        );
        assert_eq!(
            hub.take_inbox(),
            vec![Inbound::NotSaid(
                "Back off, you're getting shredded.".into()
            )]
        );
    }

    #[test]
    fn a_page_can_tell_a_restarted_pc_by_its_boot() {
        // The same page state against two runs of the program in turn.
        let before = hub();
        let after = hub();
        let state = |hub: &Hub| body(&hub.handle(&request("GET", "/api/state?k=k1", "")));
        let (first, second) = (
            state(&before)["boot"].clone(),
            state(&after)["boot"].clone(),
        );
        assert!(first.as_str().is_some_and(|b| b.len() == 8), "{first}");
        assert_ne!(first, second);
        // One run keeps its boot from poll to poll.
        assert_eq!(state(&before)["boot"], first);
    }

    #[test]
    fn the_voice_setting_round_trips() {
        let hub = hub();
        let r = hub.handle(&request("POST", "/api/voice?k=k1", r#"{"on":"phone"}"#));
        assert_eq!(r.status, 200);
        assert_eq!(hub.voice_on(), VoiceOn::Phone);
        assert_eq!(hub.take_inbox(), vec![Inbound::Voice(VoiceOn::Phone)]);
        assert_eq!(
            hub.handle(&request("POST", "/api/voice?k=k1", r#"{"on":"loud"}"#))
                .status,
            400
        );
    }
}
