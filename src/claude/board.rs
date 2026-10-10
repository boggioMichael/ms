//! The running companion's side of the channel: what it posts for Claude
//! (events, the reading now, the screen) and what Claude asks to have said,
//! served under `/local/` on this PC only.
//!
//! - `GET /local/events?since=N&wait=MS` — the events after `N`, waiting up
//!   to `wait` milliseconds for one; `?fresh=1` instead starts afresh,
//!   with nothing from before the bridge came (only the number to go on
//!   from). Each answer carries the board's `boot`, which a companion
//!   started again changes, and each poll says the bridge is there
//!   ([`Board::connected`]).
//! - `GET /local/status` — the reading now, as text and as fields, with the
//!   last few events.
//! - `POST /local/say` `{"text": …}` — a line for MapleSyrup's voice.
//! - `GET /local/look?region=…` — the screen (or part of it) as a JPEG,
//!   while the game is the window in front.
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
/// A picture's longest side by default: what Claude takes in whole.
const LOOK_SIZE: u32 = 1568;

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
    says: Vec<String>,
    frame: Option<Arc<RgbaImage>>,
}

pub struct Board {
    key: String,
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
            boot: crate::phone::tls::random_hex(4),
            started: Instant::now(),
            inner: Mutex::new(Inner {
                events: VecDeque::new(),
                next: 1,
                status: json!({}),
                text: String::new(),
                bridge_seen: None,
                says: Vec::new(),
                frame: None,
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

    /// The screen now (None while the game is not the window in front).
    pub fn set_frame(&self, frame: Option<Arc<RgbaImage>>) {
        self.lock().frame = frame;
    }

    /// Whether a bridge is polling: Claude is listening.
    pub fn connected(&self) -> bool {
        self.lock()
            .bridge_seen
            .is_some_and(|at| at.elapsed() < CONNECTED_FOR)
    }

    /// What Claude asked to have said since the last call.
    pub fn take_says(&self) -> Vec<String> {
        std::mem::take(&mut self.lock().says)
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
        match (request.method.as_str(), request.path.as_str()) {
            ("GET", "/local/hello") => {
                Response::json(200, &json!({"ok": true, "pid": std::process::id()}))
            }
            ("GET", "/local/events") => self.events(request),
            ("GET", "/local/status") => self.status(),
            ("POST", "/local/say") => {
                let body = serde_json::from_slice::<Value>(&request.body).unwrap_or(Value::Null);
                let text = body["text"].as_str().map(str::trim).unwrap_or_default();
                if text.is_empty() {
                    return Response::json(400, &json!({"error": "nothing to say"}));
                }
                let text: String = text.chars().take(MAX_SAY).collect();
                let mut inner = self.lock();
                inner.bridge_seen = Some(Instant::now());
                inner.says.push(text);
                Response::json(200, &json!({"ok": true}))
            }
            ("GET", "/local/look") => self.look(request),
            _ => Response::json(404, &json!({"error": "not found"})),
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
        let mut inner = self.lock();
        inner.bridge_seen = Some(Instant::now());
        if request.param("fresh").is_some() {
            // A bridge that just came: from here on.
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
        let events: Vec<&Posted> = inner
            .events
            .iter()
            .filter(|e| e.seq > since)
            .take(PER_POLL)
            .collect();
        let last = events.last().map(|e| e.seq).unwrap_or(inner.next - 1);
        Response::json(
            200,
            &json!({"events": events, "last": last, "boot": self.boot}),
        )
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
                &json!({"error": "MapleStory isn't the window in front, so there is no picture of it"}),
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
        assert!(board.connected());
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
        let poster = Arc::clone(&board);
        let started = Instant::now();
        let handle = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(150));
            poster.post("heard", "He said: hi", "He said: hi".into());
        });
        // Nothing was ever posted: the first poll after a fresh start is
        // `since=0`, and it waits like any other.
        let got = json_of(&board.handle(&get(&format!("/local/events?k={KEY}&fresh=1"))));
        let since = got["last"].as_u64().unwrap();
        assert_eq!(since, 0);
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
