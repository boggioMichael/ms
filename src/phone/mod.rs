//! The phone link: a page the phone opens from a QR code, which turns it
//! into MapleSyrup's microphone and second screen.
//!
//! ```text
//!   phone (Safari / Chrome)                      this PC
//!   ─────────────────────                        ───────
//!   microphone ── 16 kHz samples ─ POST /api/audio ──▶ Mic: level, speaking, WAV
//!   speech recognition ─ sentence ─ POST /api/heard ──▶ inbox ─▶ companion
//!   buttons ───────────── command ─ POST /api/command ▶ inbox ─▶ companion
//!   screen ◀──────── HP/MP/EXP, replies ─ GET /api/state ◀── status, messages
//! ```
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
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::companion::Kind;
use audio::{Mic, MicSummary};
use http::{Conn, HttpError, Request, Response};

/// The phone page, with its script and styles.
pub const PAGE: &str = include_str!("page.html");

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
    /// A button: a command's word.
    Command(String),
    /// The page opened, from this browser.
    Hello(String),
    /// Where replies should be spoken now.
    Voice(VoiceOn),
    /// Answer everything said (true), or only after "syrup" (false).
    Listen(bool),
    /// Forget a thing it was taught (by its id).
    Forget(String),
    /// The player's language (a locale such as `he-IL`).
    Language(String),
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
    /// The latest spoken lines as WAVs, for the phone to play in turn, with
    /// their numbers.
    clips: VecDeque<(u64, Arc<Vec<u8>>)>,
    last_clip: u64,
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

/// Everything the phone link shares between the server's threads and the
/// main loop.
pub struct Hub {
    key: String,
    started: Instant,
    state: Mutex<State>,
}

impl Hub {
    /// `key`: the link's secret. `record_to`: keep the phone's audio in this WAV file.
    pub fn new(key: String, record_to: Option<PathBuf>, voice_on: VoiceOn) -> Arc<Hub> {
        Arc::new(Hub {
            key,
            started: Instant::now(),
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
                last_clip: 0,
                thumbs: std::collections::HashMap::new(),
            }),
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
        });
        while state.messages.len() > KEEP_MESSAGES {
            state.messages.pop_front();
        }
        id
    }

    /// What the phone sent since the last call.
    pub fn take_inbox(&self) -> Vec<Inbound> {
        std::mem::take(&mut self.lock().inbox)
    }

    /// Hand the phone a spoken line (a WAV) to play after the ones before
    /// it. Returns its number.
    pub fn set_clip(&self, wav: Vec<u8>) -> u64 {
        let mut state = self.lock();
        state.last_clip += 1;
        let seq = state.last_clip;
        state.clips.push_back((seq, Arc::new(wav)));
        while state.clips.len() > CLIPS_KEPT {
            state.clips.pop_front();
        }
        seq
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
            ("GET", "/dog.png") => Response::new(200, "image/png", crate::app::dog::SHEET_PNG)
                .with_header("Cache-Control", "max-age=86400"),
            ("GET", "/dog.json") => {
                Response::new(200, "application/json", crate::app::dog::SHEET_JSON)
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
                {
                    let mut state = self.lock();
                    state.phone_seen = Some(Instant::now());
                    state.requests += 1;
                }
                self.api(method, path, request)
            }
            _ => Response::text(404, "not found"),
        }
    }

    fn api(&self, method: &str, path: &str, request: &Request) -> Response {
        let body = || serde_json::from_slice::<Value>(&request.body).unwrap_or(Value::Null);
        let text_field = |name: &str| {
            body()
                .get(name)
                .and_then(Value::as_str)
                .map(|s| s.chars().take(500).collect::<String>())
        };
        match (method, path) {
            ("GET", "/api/state") => {
                let since: u64 = request
                    .param("since")
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0);
                let now = Instant::now();
                let state = self.lock();
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
                Response::json(
                    200,
                    &json!({
                        "status": state.status,
                        "mic": state.mic.summary(now),
                        "voice_on": state.voice_on,
                        "messages": messages,
                        "last_id": state.next_id - 1,
                        "uptime": self.started.elapsed().as_secs_f64(),
                        "clip": state.last_clip,
                    }),
                )
            }
            ("GET", "/api/clip") => {
                // The clip asked for by number, or else the latest.
                let wanted: Option<u64> = request.param("seq").and_then(|s| s.parse().ok());
                let clip = {
                    let state = self.lock();
                    match wanted {
                        Some(seq) => state.clips.iter().find(|(n, _)| *n == seq).cloned(),
                        None => state.clips.back().cloned(),
                    }
                };
                match clip {
                    Some((_, wav)) => Response::new(200, "audio/wav", wav.as_slice()),
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
            ("POST", "/api/audio") => {
                let rate: u32 = request
                    .param("rate")
                    .and_then(|r| r.parse().ok())
                    .unwrap_or(16_000);
                if !(4_000..=96_000).contains(&rate) {
                    return Response::json(400, &json!({"error": "unsupported sample rate"}));
                }
                let samples = audio::samples_from_bytes(&request.body);
                self.lock().mic.ingest(&samples, rate, Instant::now());
                Response::empty(204)
            }
            ("POST", "/api/heard") => match text_field("text") {
                Some(text) if !text.trim().is_empty() => {
                    self.lock()
                        .inbox
                        .push(Inbound::Heard(text.trim().to_string()));
                    Response::json(200, &json!({"ok": true}))
                }
                _ => Response::json(400, &json!({"error": "no text"})),
            },
            ("POST", "/api/command") => match text_field("command") {
                Some(command) => {
                    self.lock().inbox.push(Inbound::Command(command));
                    Response::json(200, &json!({"ok": true}))
                }
                None => Response::json(400, &json!({"error": "no command"})),
            },
            ("POST", "/api/hello") => {
                let browser = text_field("agent").unwrap_or_else(|| "a browser".into());
                let lang = text_field("lang").filter(|l| plausible_locale(l));
                let mut state = self.lock();
                state.browser = Some(browser.clone());
                // The language first, so the greeting is in it.
                if let Some(lang) = lang {
                    state.inbox.push(Inbound::Language(lang));
                }
                state.inbox.push(Inbound::Hello(browser));
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
            r#"{"agent":"iPhone Safari"}"#,
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
                Inbound::Hello("iPhone Safari".into()),
                Inbound::Heard("syrup status".into()),
                Inbound::Command("mark".into()),
            ]
        );
        assert!(hub.take_inbox().is_empty());
        let summary = hub.summary();
        assert!(summary.connected);
        assert_eq!(summary.browser.as_deref(), Some("iPhone Safari"));
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

    #[test]
    fn the_phone_gets_spoken_clips_the_dog_and_the_listen_switch() {
        let hub = hub();
        assert_eq!(
            hub.handle(&request("GET", "/api/clip?k=k1", "")).status,
            404
        );
        assert_eq!(hub.set_clip(b"RIFF....WAVE".to_vec()), 1);
        assert_eq!(hub.set_clip(b"RIFF....WAVE2".to_vec()), 2);
        let state = body(&hub.handle(&request("GET", "/api/state?k=k1", "")));
        assert_eq!(state["clip"], 2);
        let clip = hub.handle(&request("GET", "/api/clip?k=k1&seq=2", ""));
        assert_eq!((clip.status, clip.content_type), (200, "audio/wav"));
        assert_eq!(clip.body, b"RIFF....WAVE2");
        // Earlier lines can still be fetched, in turn; long gone ones cannot.
        let first = hub.handle(&request("GET", "/api/clip?k=k1&seq=1", ""));
        assert_eq!(first.body, b"RIFF....WAVE");
        for _ in 0..super::CLIPS_KEPT {
            hub.set_clip(b"RIFF....MORE".to_vec());
        }
        assert_eq!(
            hub.handle(&request("GET", "/api/clip?k=k1&seq=1", ""))
                .status,
            404
        );
        let dog = hub.handle(&request("GET", "/dog.png", ""));
        assert_eq!(dog.status, 200);
        assert_eq!(&dog.body[1..4], b"PNG");
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
        assert_eq!(
            hub.take_inbox(),
            vec![
                Inbound::Language("ko-KR".into()),
                Inbound::Hello("iPhone".into())
            ]
        );
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
