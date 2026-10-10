//! The MCP server the Claude app runs: `MapleSyrup --mcp` (also started as
//! `--claude-channel`).
//!
//! MapleSyrup works in the background and Claude talks with the player: the
//! Claude desktop app (or Claude Code) starts this server, which starts the
//! companion itself when it is not running (`--background`: no window, no
//! voice, no phone) and gives Claude its tools on stdio (one JSON-RPC
//! message per line):
//!
//! - `game_status` — everything read off the screen now, with each number's
//!   age, and the last events; `look_at_screen` — the screen or a part of it;
//! - `maple_wiki_search` / `maple_wiki_save` — MapleSyrup's local MapleStory
//!   wiki (`knowledge.json`): what it knows about the game, and facts Claude
//!   checked, kept for next time;
//! - `player_profile` / `player_remember` — what MapleSyrup knows about the
//!   player and his game (`about-me.txt`, its notebook), and a lasting fact
//!   about him;
//! - `say` (Claude Code only) — MapleSyrup's voice says it.
//!
//! In a Claude Code session it is also a channel: it declares Claude Code's
//! `claude/channel` capability and pushes every event the companion posts —
//! a death, a level-up, a new map, HP in danger, a dialog, the state of
//! things — as a `notifications/claude/channel`, which reaches Claude as
//! `<channel source="maplesyrup" kind="…">`. Other clients get the tools
//! only.
//!
//! It finds the companion through [`super::link`], and keeps finding it: a
//! companion started after Claude, or restarted, is picked up; one that
//! stopped is told to Claude Code once. Only the protocol goes to stdout;
//! what it has to say for itself goes to stderr (the client's log).

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::link;
use crate::phone::client;

/// The protocol revision answered when the client asks for one this does
/// not know. (Claude Code does not register a channel on 2026-07-28.)
pub const PROTOCOL: &str = "2025-06-18";
/// Revisions answered as asked: this server's few methods are the same in
/// each.
const KNOWN: &[&str] = &["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

/// What Claude is told about MapleSyrup when the server connects.
pub const INSTRUCTIONS: &str = "\
MapleSyrup runs in the background on the player's PC and watches his MapleStory screen live: the game window, \
the HUD's numbers, what moves on the screen, dialogs, and things he taught it to recognise. Use it whenever the \
conversation is about his game.

- game_status: the live reading — HP/MP/EXP with how old each number is, level, map, what moves on the screen \
and where (x, y as percent of the screen from its top left; MapleSyrup doesn't say what each thing is), an open \
dialog and its text, the things he taught it, the session so far and the last events. Call it before answering \
anything about his game as it is now; it is always fresh. Never give a number older than about ten seconds as the \
current one.
- look_at_screen: a picture of his screen, or a part of it (center = around his character, top-left = the minimap \
and the map's name, bottom = the HUD). Use it for names — monsters, NPCs, items, quests, windows — and whatever \
the reading doesn't say.
- maple_wiki_search / maple_wiki_save: MapleSyrup's own MapleStory wiki, kept on his PC. Look there first for \
game facts. When you have checked a fact (on the web, or he corrected you), save it, short and specific, with its \
source, so it is known next time. When he plays MapleStory Classic World (the reading says so), a fact must hold \
for Classic World: modern systems (the Maple Guide, world-map travel, Arcane River, Fafnir, modern jobs, Drop \
Coupons) are not there.
- player_profile / player_remember: what MapleSyrup knows about him and his MapleStory — his name, his language, \
his character, his goals, how he likes to be helped. Read it when a conversation about his game starts; save a \
lasting thing he tells you (a goal, a preference, progress worth remembering).

Never invent maps, NPCs, quests, drops or routes: check, or say you're not sure.";

/// What a Claude Code session is told besides: the events it is pushed.
pub const CHANNEL_INSTRUCTIONS: &str = "\
In this session MapleSyrup also pushes what changes in the game as <channel source=\"maplesyrup\" kind=\"...\"> \
events, each ending with the whole reading at that moment:
kind=\"hello\": the link to MapleSyrup came up. kind=\"heard\": he said this out loud through MapleSyrup's phone \
page (speech recognition; may be misheard) — about anything, not only the game: answer him as you would any \
message, with say when he can't read the screen. \
kind=\"death\" | \"level_up\" | \"map\" | \"hp\" | \"mp\" | \"dialog\" | \"thing\" | \"window\" | \"state\": something \
changed. Speak up only when it helps him right now — danger, a real milestone, a quest or dialog you can help \
with, something he asked you to watch; otherwise stay silent. kind=\"offline\": MapleSyrup stopped.";

/// One poll's worth: the events, the number of the last, and the
/// companion's run (a new `boot` is a companion started again).
#[derive(Debug, Clone, Default)]
pub struct Batch {
    pub events: Vec<Value>,
    pub last: u64,
    pub boot: String,
}

/// What the bridge asks of the running companion.
pub trait Api: Send + Sync {
    /// The events after `since`, waiting up to `wait` for one; `None`
    /// starts afresh (no events, only the number to go on from).
    /// `listening`: Claude takes the events (MapleSyrup then leaves the
    /// talking to it).
    fn events(&self, since: Option<u64>, wait: Duration, listening: bool) -> Result<Batch, String>;
    /// The reading now: `{status, text, recent}`.
    fn status(&self) -> Result<Value, String>;
    fn say(&self, text: &str) -> Result<(), String>;
    /// `{mime, data, width, height, region, frame}`.
    fn look(&self, region: &str) -> Result<Value, String>;
    /// `{found: [{about, answer, from, when}], size}`.
    fn wiki(&self, query: &str) -> Result<Value, String>;
    fn wiki_save(&self, about: &str, answer: &str, source: &str) -> Result<(), String>;
    /// `{text}`.
    fn player(&self) -> Result<Value, String>;
    /// Whether it was new.
    fn remember(&self, fact: &str) -> Result<bool, String>;
}

/// The companion on this PC, found through its link file each time (it
/// may have started after the bridge, or again on another port).
pub struct Local {
    settings: PathBuf,
    /// Until when a companion this bridge started may still be coming up:
    /// a call meanwhile waits for it rather than failing.
    starting: Mutex<Option<Instant>>,
}

/// How long a companion takes to come up, at most.
const COMING_UP: Duration = Duration::from_secs(25);

impl Local {
    pub fn new(settings: &Path) -> Local {
        Local {
            settings: settings.to_path_buf(),
            starting: Mutex::new(None),
        }
    }

    fn once(&self, method: &str, path: &str, body: &[u8], wait: Duration) -> Result<Value, String> {
        let link = link::load(&self.settings).ok_or("MapleSyrup isn't running")?;
        let sep = if path.contains('?') { '&' } else { '?' };
        let target = format!("{path}{sep}k={}", link.key);
        let reply = client::request_plain_waiting(link.port, method, &target, body, wait)
            .map_err(|e| format!("MapleSyrup can't be reached ({e})"))?;
        let value: Value = serde_json::from_slice(&reply.body)
            .map_err(|_| format!("MapleSyrup answered {}", reply.status))?;
        if reply.status != 200 {
            return Err(value["error"]
                .as_str()
                .map(str::to_string)
                .unwrap_or_else(|| format!("MapleSyrup answered {}", reply.status)));
        }
        Ok(value)
    }

    /// One request; while a companion this bridge started is coming up,
    /// again until it answers.
    fn call(&self, method: &str, path: &str, body: &[u8], wait: Duration) -> Result<Value, String> {
        loop {
            match self.once(method, path, body, wait) {
                Err(why) if self.coming_up() => {
                    let _ = why;
                    std::thread::sleep(Duration::from_millis(500));
                }
                done => return done,
            }
        }
    }

    fn coming_up(&self) -> bool {
        self.starting
            .lock()
            .ok()
            .and_then(|s| *s)
            .is_some_and(|until| Instant::now() < until)
    }

    /// Whether a companion answers now.
    pub fn up(&self) -> bool {
        self.once("GET", "/local/hello", &[], Duration::from_secs(3))
            .is_ok()
    }

    /// Start the companion in the background (no window, no voice, no
    /// phone) unless one answers already. Returns the one started.
    pub fn start(&self) -> Option<std::process::Child> {
        if self.up() {
            return None;
        }
        let exe = std::env::current_exe().ok()?;
        let mut command = std::process::Command::new(exe);
        command
            .arg("--background")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            // CREATE_NO_WINDOW: no console of its own.
            command.creation_flags(0x0800_0000);
        }
        match command.spawn() {
            Ok(child) => {
                eprintln!("maplesyrup: started the companion in the background");
                if let Ok(mut starting) = self.starting.lock() {
                    *starting = Some(Instant::now() + COMING_UP);
                }
                Some(child)
            }
            Err(e) => {
                eprintln!("maplesyrup: could not start the companion: {e}");
                None
            }
        }
    }

    /// Stop the companion this bridge started: asked to finish, then made to.
    pub fn stop(&self, mut child: std::process::Child) {
        if let Ok(mut starting) = self.starting.lock() {
            *starting = None;
        }
        let _ = self.once("POST", "/local/quit", b"{}", Duration::from_secs(3));
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if let Ok(Some(_)) = child.try_wait() {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        let _ = child.kill();
        let _ = child.wait();
    }
}

impl Api for Local {
    fn events(&self, since: Option<u64>, wait: Duration, listening: bool) -> Result<Batch, String> {
        let listen = if listening { "&listen=1" } else { "" };
        let path = match since {
            None => format!("/local/events?fresh=1{listen}"),
            Some(since) => format!(
                "/local/events?since={since}&wait={}{listen}",
                wait.as_millis()
            ),
        };
        let value = self.once("GET", &path, &[], wait + Duration::from_secs(10))?;
        Ok(Batch {
            events: value["events"].as_array().cloned().unwrap_or_default(),
            last: value["last"].as_u64().unwrap_or(since.unwrap_or(0)),
            boot: value["boot"].as_str().unwrap_or_default().to_string(),
        })
    }

    fn status(&self) -> Result<Value, String> {
        self.call("GET", "/local/status", &[], Duration::from_secs(10))
    }

    fn say(&self, text: &str) -> Result<(), String> {
        let body = json!({ "text": text }).to_string();
        self.call(
            "POST",
            "/local/say",
            body.as_bytes(),
            Duration::from_secs(10),
        )
        .map(|_| ())
    }

    fn look(&self, region: &str) -> Result<Value, String> {
        let region: String = region
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | ',' | '.'))
            .collect();
        self.call(
            "GET",
            &format!("/local/look?region={region}"),
            &[],
            Duration::from_secs(15),
        )
    }

    fn wiki(&self, query: &str) -> Result<Value, String> {
        self.call(
            "GET",
            &format!("/local/wiki?q={}", escape(query)),
            &[],
            Duration::from_secs(10),
        )
    }

    fn wiki_save(&self, about: &str, answer: &str, source: &str) -> Result<(), String> {
        let body = json!({"about": about, "answer": answer, "source": source}).to_string();
        self.call(
            "POST",
            "/local/wiki",
            body.as_bytes(),
            Duration::from_secs(10),
        )
        .map(|_| ())
    }

    fn player(&self) -> Result<Value, String> {
        self.call("GET", "/local/player", &[], Duration::from_secs(10))
    }

    fn remember(&self, fact: &str) -> Result<bool, String> {
        let body = json!({ "fact": fact }).to_string();
        self.call(
            "POST",
            "/local/player",
            body.as_bytes(),
            Duration::from_secs(10),
        )
        .map(|v| v["new"].as_bool().unwrap_or(true))
    }
}

/// `text` for a query string: letters and digits as they are, everything
/// else (spaces, Hebrew, punctuation) percent-encoded as UTF-8.
fn escape(text: &str) -> String {
    let mut out = String::new();
    for b in text.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Where the protocol goes (stdout; a buffer in the tests). Each message is
/// one line, written whole.
pub type Out = Arc<Mutex<Box<dyn Write + Send>>>;

fn send(out: &Out, message: &Value) {
    let mut out = out.lock().unwrap_or_else(|e| e.into_inner());
    let _ = writeln!(out, "{message}");
    let _ = out.flush();
}

/// One event for Claude.
fn notify(out: &Out, kind: &str, content: &str) {
    send(
        out,
        &json!({
            "jsonrpc": "2.0",
            "method": "notifications/claude/channel",
            "params": {"content": content, "meta": {"kind": kind}},
        }),
    );
}

/// Whether the client is Claude Code (which takes channel events), from
/// the name it gives itself.
fn is_claude_code(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name.contains("claude-code") || name.contains("claude code") || name == "claude_code"
}

pub struct Bridge<A: Api + 'static> {
    api: Arc<A>,
    out: Out,
    feeding: bool,
    /// The client takes channel events (Claude Code): the feed passes them
    /// on, and `say` is offered.
    channel: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    /// How long the events wait for news per poll, and how long the
    /// companion must be unreachable before Claude hears it stopped
    /// (shorter in the tests).
    pub timing: Timing,
}

/// The feed's pace.
#[derive(Debug, Clone, Copy)]
pub struct Timing {
    pub poll: Duration,
    /// (A restart is quicker than this.)
    pub offline_after: Duration,
    /// Between tries while the companion can't be reached.
    pub retry: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Timing {
            poll: Duration::from_secs(20),
            offline_after: Duration::from_secs(8),
            retry: Duration::from_millis(1500),
        }
    }
}

impl<A: Api + 'static> Bridge<A> {
    pub fn new(api: Arc<A>, out: Out) -> Bridge<A> {
        Bridge {
            api,
            out,
            feeding: false,
            channel: Arc::new(AtomicBool::new(false)),
            stop: Arc::new(AtomicBool::new(false)),
            timing: Timing::default(),
        }
    }

    /// One line from the client.
    pub fn on_line(&mut self, line: &str) {
        let line = line.trim();
        if line.is_empty() {
            return;
        }
        let Ok(message) = serde_json::from_str::<Value>(line) else {
            send(
                &self.out,
                &json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32700, "message": "Parse error"}}),
            );
            return;
        };
        match message {
            Value::Array(batch) => {
                let answers: Vec<Value> = batch.iter().filter_map(|m| self.respond(m)).collect();
                if !answers.is_empty() {
                    send(&self.out, &Value::Array(answers));
                }
            }
            message => {
                if let Some(answer) = self.respond(&message) {
                    send(&self.out, &answer);
                }
            }
        }
    }

    /// The answer to one message (None for a notification).
    pub fn respond(&mut self, message: &Value) -> Option<Value> {
        let method = message["method"].as_str()?;
        let Some(id) = message.get("id").filter(|id| !id.is_null()).cloned() else {
            self.notified(method);
            return None;
        };
        Some(match self.request(method, &message["params"]) {
            Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
            Err((code, text)) => {
                json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": text}})
            }
        })
    }

    fn notified(&mut self, method: &str) {
        // The client is ready: the companion is followed from now on (its
        // events passed on to Claude Code; for any client, the companion
        // knows Claude is there).
        if method == "notifications/initialized" && !self.feeding {
            self.feeding = true;
            let (api, out, channel, stop, timing) = (
                Arc::clone(&self.api),
                Arc::clone(&self.out),
                Arc::clone(&self.channel),
                Arc::clone(&self.stop),
                self.timing,
            );
            let _ = std::thread::Builder::new()
                .name("channel-feed".into())
                .spawn(move || feed(api, out, channel, stop, timing));
        }
    }

    fn request(&mut self, method: &str, params: &Value) -> Result<Value, (i64, String)> {
        match method {
            "initialize" => {
                let asked = params["protocolVersion"].as_str().unwrap_or(PROTOCOL);
                let version = if KNOWN.contains(&asked) {
                    asked
                } else {
                    PROTOCOL
                };
                let code =
                    is_claude_code(params["clientInfo"]["name"].as_str().unwrap_or_default());
                self.channel.store(code, Ordering::Relaxed);
                let instructions = if code {
                    format!("{INSTRUCTIONS}\n\n{CHANNEL_INSTRUCTIONS}")
                } else {
                    INSTRUCTIONS.to_string()
                };
                Ok(json!({
                    "protocolVersion": version,
                    "capabilities": {
                        "experimental": {"claude/channel": {}},
                        "tools": {},
                    },
                    "serverInfo": {"name": "maplesyrup", "title": "MapleSyrup", "version": env!("CARGO_PKG_VERSION")},
                    "instructions": instructions,
                }))
            }
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({ "tools": tools(self.channel.load(Ordering::Relaxed)) })),
            "tools/call" => Ok(self.call(params)),
            "resources/list" => Ok(json!({"resources": []})),
            "resources/templates/list" => Ok(json!({"resourceTemplates": []})),
            "prompts/list" => Ok(json!({"prompts": []})),
            other => Err((-32601, format!("Method not found: {other}"))),
        }
    }

    fn call(&self, params: &Value) -> Value {
        let arguments = &params["arguments"];
        let text = |name: &str| {
            arguments[name]
                .as_str()
                .unwrap_or_default()
                .trim()
                .to_string()
        };
        match params["name"].as_str().unwrap_or_default() {
            "say" => {
                let line = text("text");
                if line.is_empty() {
                    return failed("Nothing to say: give the text.");
                }
                match self.api.say(&line) {
                    Ok(()) => said("Said."),
                    Err(why) => failed(&format!("Not said: {why}.")),
                }
            }
            "game_status" => match self.api.status() {
                Ok(status) => said(&status_text(&status)),
                Err(why) => failed(&format!("No reading: {why}.")),
            },
            "look_at_screen" => {
                let region = arguments["region"].as_str().unwrap_or("full");
                match self.api.look(region) {
                    Ok(picture) => json!({
                        "content": [
                            {
                                "type": "image",
                                "data": picture["data"],
                                "mimeType": picture["mime"].as_str().unwrap_or("image/jpeg"),
                            },
                            {
                                "type": "text",
                                "text": format!(
                                    "The {} of his screen now, {}×{} (the screen is {}×{}).",
                                    picture["region"].as_str().unwrap_or(region),
                                    picture["width"],
                                    picture["height"],
                                    picture["frame"][0],
                                    picture["frame"][1],
                                ),
                            },
                        ],
                    }),
                    Err(why) => failed(&format!("No picture: {why}.")),
                }
            }
            "maple_wiki_search" => {
                let query = text("query");
                if query.is_empty() {
                    return failed("Say what to look for.");
                }
                match self.api.wiki(&query) {
                    Ok(found) => said(&wiki_text(&query, &found)),
                    Err(why) => failed(&format!("The wiki can't be read: {why}.")),
                }
            }
            "maple_wiki_save" => {
                let (about, fact) = (text("about"), text("fact"));
                if about.is_empty() || fact.is_empty() {
                    return failed("Give what it is about and the fact.");
                }
                match self.api.wiki_save(&about, &fact, &text("source")) {
                    Ok(()) => said("Saved in MapleSyrup's wiki."),
                    Err(why) => failed(&format!("Not saved: {why}.")),
                }
            }
            "player_profile" => match self.api.player() {
                Ok(player) => said(player["text"].as_str().unwrap_or_default()),
                Err(why) => failed(&format!("Nothing to read: {why}.")),
            },
            "player_remember" => {
                let fact = text("fact");
                if fact.is_empty() {
                    return failed("Give the fact to remember.");
                }
                match self.api.remember(&fact) {
                    Ok(true) => said("Remembered for good."),
                    Ok(false) => said("Already known."),
                    Err(why) => failed(&format!("Not kept: {why}.")),
                }
            }
            other => failed(&format!("There is no tool called {other}.")),
        }
    }

    /// The client went away: the feed stops with it.
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn said(text: &str) -> Value {
    json!({"content": [{"type": "text", "text": text}]})
}

fn failed(text: &str) -> Value {
    json!({"content": [{"type": "text", "text": text}], "isError": true})
}

/// The reading for Claude, with the last few events under it.
fn status_text(status: &Value) -> String {
    let mut text = status["text"].as_str().unwrap_or_default().to_string();
    if text.is_empty() {
        text = "MapleSyrup has no reading yet.".into();
    }
    let recent: Vec<String> = status["recent"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|e| {
            format!(
                "- {} ({} s ago)",
                e["what"].as_str().unwrap_or_default(),
                e["seconds_ago"]
            )
        })
        .collect();
    if !recent.is_empty() {
        text.push_str("\n\nLately:\n");
        text.push_str(&recent.join("\n"));
    }
    text
}

/// What the wiki has on `query`, for Claude.
fn wiki_text(query: &str, found: &Value) -> String {
    let entries = found["found"].as_array().cloned().unwrap_or_default();
    if entries.is_empty() {
        return format!(
            "MapleSyrup's wiki has nothing on \"{query}\" yet ({} entries in all). Check elsewhere, and save \
what you find with maple_wiki_save.",
            found["size"]
        );
    }
    let lines: Vec<String> = entries
        .iter()
        .map(|e| {
            format!(
                "- {}: {} [{}, {}]",
                e["about"].as_str().unwrap_or_default(),
                e["answer"].as_str().unwrap_or_default(),
                e["from"].as_str().unwrap_or_default(),
                e["when"].as_str().unwrap_or_default(),
            )
        })
        .collect();
    format!("From MapleSyrup's wiki:\n{}", lines.join("\n"))
}

fn tools(channel: bool) -> Value {
    let read_only = |title: &str| json!({"title": title, "readOnlyHint": true, "destructiveHint": false, "idempotentHint": true, "openWorldHint": false});
    let keeps = |title: &str| json!({"title": title, "readOnlyHint": false, "destructiveHint": false, "idempotentHint": true, "openWorldHint": false});
    let mut tools = vec![
        json!({
            "name": "game_status",
            "title": "MapleStory, now",
            "description": "Everything MapleSyrup reads on the player's MapleStory screen right now: the window, the \
        character and level, HP/MP/EXP with how old each number is, the map, what moves on the screen and where, an open \
        dialog and its text, the things he taught it, the session so far, and the last events. Always fresh: call it \
        before answering about his game as it is now.",
            "inputSchema": {"type": "object", "properties": {}, "additionalProperties": false},
            "annotations": read_only("MapleStory, now"),
        }),
        json!({
            "name": "look_at_screen",
            "title": "Look at his screen",
            "description": "A picture of the player's screen right now (only while MapleStory is the window in \
        front), or a part of it, at a size where the game's text is readable. Use it for what the reading doesn't name: \
        monsters, NPCs, items, quest windows, the map name on the minimap, his character.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "region": {
                        "type": "string",
                        "description": "full (default), center (around his character), top, bottom, left, right, \
        top-left (the minimap and map name), top-right, bottom-left, bottom-right, or x0,y0,x1,y1 as fractions of the \
        screen (0 to 1 from the top left).",
                    }
                },
                "additionalProperties": false,
            },
            "annotations": read_only("Look at his screen"),
        }),
        json!({
            "name": "maple_wiki_search",
            "title": "Search MapleSyrup's wiki",
            "description": "Search MapleSyrup's own MapleStory wiki on the player's PC: facts about the game it \
        looked up or he corrected (his corrections are to be trusted), with when each was learned. Look here first for a \
        game fact.",
            "inputSchema": {
                "type": "object",
                "properties": {"query": {"type": "string", "description": "What to look for, in a few words (any language)."}},
                "required": ["query"],
                "additionalProperties": false,
            },
            "annotations": read_only("Search MapleSyrup's wiki"),
        }),
        json!({
            "name": "maple_wiki_save",
            "title": "Save to MapleSyrup's wiki",
            "description": "Keep a checked MapleStory fact in MapleSyrup's wiki for next time — short and \
        specific, with its source. It replaces what was kept about the same thing (unless that was his correction). \
        Only facts you checked; for Classic World, facts that hold in Classic World.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "about": {"type": "string", "description": "What it is about, as a question or a topic (\"Where to hunt Blue Mushrooms in Classic World\")."},
                    "fact": {"type": "string", "description": "The fact itself, one or two sentences."},
                    "source": {"type": "string", "description": "Where it was checked: a URL, or \"the player\"."}
                },
                "required": ["about", "fact"],
                "additionalProperties": false,
            },
            "annotations": keeps("Save to MapleSyrup's wiki"),
        }),
        json!({
            "name": "player_profile",
            "title": "What MapleSyrup knows about him",
            "description": "What MapleSyrup knows about the player and his MapleStory: his name, his language, his \
        character as read now, his goals, how he likes to be helped, what happened lately. Read it when a conversation \
        about his game starts.",
            "inputSchema": {"type": "object", "properties": {}, "additionalProperties": false},
            "annotations": read_only("What MapleSyrup knows about him"),
        }),
        json!({
            "name": "player_remember",
            "title": "Remember about him",
            "description": "Keep a lasting fact about the player and his MapleStory for good (in his own file, \
        which he can read and edit): a goal, a preference, progress worth remembering. Not passing states (HP now, \
        where he is now).",
            "inputSchema": {
                "type": "object",
                "properties": {"fact": {"type": "string", "description": "The fact, as one short sentence about him."}},
                "required": ["fact"],
                "additionalProperties": false,
            },
            "annotations": keeps("Remember about him"),
        }),
    ];
    if channel {
        tools.push(json!({
            "name": "say",
            "title": "Say it out loud",
            "description": "Say this out loud to the player in MapleSyrup's voice (his speakers or his phone), for \
when he can't read the screen — he is playing, or he spoke to you through MapleSyrup. Spoken style: one or two \
short sentences, no lists or markdown, in his language (in Hebrew, address him in the masculine).",
            "inputSchema": {
                "type": "object",
                "properties": {"text": {"type": "string", "description": "What to say, exactly as it should be spoken."}},
                "required": ["text"],
                "additionalProperties": false,
            },
            "annotations": {"title": "Say it out loud", "readOnlyHint": false, "destructiveHint": false, "idempotentHint": false, "openWorldHint": false},
        }));
    }
    Value::Array(tools)
}

/// The events, as they come — passed on to Claude when it takes them
/// (`channel`) — and the companion coming and going. Polling at all tells
/// the companion Claude is there.
fn feed<A: Api>(
    api: Arc<A>,
    out: Out,
    channel: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    timing: Timing,
) {
    // Where the events are up to (None: start afresh), and whose they are.
    let mut since: Option<u64> = None;
    let mut boot: Option<String> = None;
    // Whether Claude was last told the companion is there.
    let mut told: Option<bool> = None;
    let mut failing: Option<Instant> = None;
    while !stop.load(Ordering::Relaxed) {
        let listening = channel.load(Ordering::Relaxed);
        match api.events(since, timing.poll, listening) {
            Ok(batch) => {
                failing = None;
                // A companion that started again numbers its events from
                // one: start over with it.
                let new_run = boot.as_deref() != Some(batch.boot.as_str());
                if told != Some(true) || new_run {
                    if listening {
                        let reading = api
                            .status()
                            .ok()
                            .and_then(|s| s["text"].as_str().map(str::to_string))
                            .unwrap_or_default();
                        notify(
                            &out,
                            "hello",
                            format!(
                                "Connected to MapleSyrup: from now on you get what it reads off his screen, live.\n\n{reading}"
                            )
                            .trim_end(),
                        );
                    }
                    told = Some(true);
                    boot = Some(batch.boot.clone());
                    if since.is_none() || new_run {
                        // What came before the hello is in its reading.
                        since = Some(batch.last);
                        continue;
                    }
                }
                if listening {
                    for event in &batch.events {
                        notify(
                            &out,
                            event["kind"].as_str().unwrap_or("event"),
                            event["text"].as_str().unwrap_or_default(),
                        );
                    }
                }
                since = Some(batch.last);
            }
            Err(why) => {
                let first = *failing.get_or_insert_with(Instant::now);
                if told != Some(false) && first.elapsed() >= timing.offline_after {
                    eprintln!("maplesyrup channel: {why}");
                    if listening {
                        notify(
                            &out,
                            "offline",
                            "MapleSyrup isn't running (or stopped), so there is no live reading of the game until \
it is started again.",
                        );
                    }
                    told = Some(false);
                }
                since = None;
                std::thread::sleep(timing.retry);
            }
        }
    }
}

/// Run the server on stdin and stdout until the client goes; with
/// `autostart`, the companion is started in the background when it is not
/// running (and stopped again at the end). Returns the process's exit code.
pub fn run(settings: &Path, autostart: bool) -> i32 {
    let out: Out = Arc::new(Mutex::new(Box::new(std::io::stdout())));
    let local = Arc::new(Local::new(settings));
    let started = if autostart { local.start() } else { None };
    let mut bridge = Bridge::new(Arc::clone(&local), out);
    let stdin = std::io::stdin();
    let mut line = String::new();
    loop {
        line.clear();
        match stdin.lock().read_line(&mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) => bridge.on_line(&line),
        }
    }
    bridge.stop();
    if let Some(child) = started {
        local.stop(child);
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A companion that posts what it is given, numbering each. (Its
    /// events and its run are kept together: a restart is one step.)
    #[derive(Default)]
    struct Fake {
        run: Mutex<(Vec<Value>, String)>,
        said: Mutex<Vec<String>>,
        wiki: Mutex<Vec<(String, String, String)>>,
        facts: Mutex<Vec<String>>,
        /// Whether the last poll said Claude takes the events.
        listening: AtomicBool,
        down: AtomicBool,
    }

    impl Fake {
        fn post(&self, kind: &str, text: &str) {
            let mut run = self.run.lock().unwrap();
            let seq = run.0.len() as u64 + 1;
            run.0.push(json!({"seq": seq, "kind": kind, "text": text}));
        }

        /// Started again: its numbers start over.
        fn restart(&self) {
            *self.run.lock().unwrap() = (Vec::new(), "second".into());
        }
    }

    impl Api for Fake {
        fn events(
            &self,
            since: Option<u64>,
            _wait: Duration,
            listening: bool,
        ) -> Result<Batch, String> {
            if self.down.load(Ordering::Relaxed) {
                return Err("MapleSyrup isn't running".into());
            }
            self.listening.store(listening, Ordering::Relaxed);
            std::thread::sleep(Duration::from_millis(20));
            let run = self.run.lock().unwrap();
            let (posted, boot) = (&run.0, &run.1);
            let last = posted.len() as u64;
            let events = match since {
                None => Vec::new(),
                Some(since) => posted
                    .iter()
                    .filter(|e| e["seq"].as_u64().unwrap() > since)
                    .cloned()
                    .collect(),
            };
            Ok(Batch {
                events,
                last,
                boot: boot.clone(),
            })
        }
        fn status(&self) -> Result<Value, String> {
            Ok(json!({
                "text": "Now 12:00:00: MapleStory is open, the window in front.\nHP 416/671 (62%), read just now.",
                "recent": [{"what": "Level up: 16 → 17.", "seconds_ago": 30}],
            }))
        }
        fn say(&self, text: &str) -> Result<(), String> {
            self.said.lock().unwrap().push(text.into());
            Ok(())
        }
        fn look(&self, region: &str) -> Result<Value, String> {
            Ok(
                json!({"mime": "image/jpeg", "data": "AAAA", "width": 800, "height": 450, "region": region, "frame": [3840, 2160]}),
            )
        }
        fn wiki(&self, query: &str) -> Result<Value, String> {
            let wiki = self.wiki.lock().unwrap();
            let found: Vec<Value> = wiki
                .iter()
                .filter(|(about, _, _)| about.to_lowercase().contains(&query.to_lowercase()))
                .map(|(about, fact, source)| {
                    json!({"about": about, "answer": format!("{fact} (source: {source})"), "from": "looked up", "when": "2026-10-10"})
                })
                .collect();
            Ok(json!({"found": found, "size": wiki.len()}))
        }
        fn wiki_save(&self, about: &str, answer: &str, source: &str) -> Result<(), String> {
            self.wiki
                .lock()
                .unwrap()
                .push((about.into(), answer.into(), source.into()));
            Ok(())
        }
        fn player(&self) -> Result<Value, String> {
            let facts = self.facts.lock().unwrap().join("\n- ");
            Ok(json!({"text": format!("About the player (they told you this):\n- {facts}")}))
        }
        fn remember(&self, fact: &str) -> Result<bool, String> {
            let mut facts = self.facts.lock().unwrap();
            if facts.iter().any(|f| f == fact) {
                return Ok(false);
            }
            facts.push(fact.into());
            Ok(true)
        }
    }

    /// Writes into a shared buffer.
    #[derive(Clone, Default)]
    struct Sink(Arc<Mutex<Vec<u8>>>);

    impl Write for Sink {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl Sink {
        fn lines(&self) -> Vec<Value> {
            String::from_utf8(self.0.lock().unwrap().clone())
                .unwrap()
                .lines()
                .map(|l| serde_json::from_str(l).expect("each line is one JSON message"))
                .collect()
        }
    }

    fn bridge() -> (Bridge<Fake>, Arc<Fake>, Sink) {
        let fake = Arc::new(Fake::default());
        fake.run.lock().unwrap().1 = "first".into();
        let sink = Sink::default();
        let out: Out = Arc::new(Mutex::new(Box::new(sink.clone())));
        let mut bridge = Bridge::new(Arc::clone(&fake), out);
        bridge.timing = Timing {
            poll: Duration::from_millis(50),
            offline_after: Duration::from_millis(200),
            retry: Duration::from_millis(30),
        };
        (bridge, fake, sink)
    }

    const CODE: &str = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"claude-code","version":"2.1.296"}}}"#;
    const DESKTOP: &str = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"claude-ai","version":"0.1.0"}}}"#;
    const READY: &str = r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;

    fn wait_for(sink: &Sink, what: impl Fn(&[Value]) -> bool) -> Vec<Value> {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let lines = sink.lines();
            if what(&lines) || Instant::now() > deadline {
                return lines;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn channel_events(lines: &[Value]) -> Vec<(String, String)> {
        lines
            .iter()
            .filter(|l| l["method"] == "notifications/claude/channel")
            .map(|l| {
                (
                    l["params"]["meta"]["kind"].as_str().unwrap().to_string(),
                    l["params"]["content"].as_str().unwrap().to_string(),
                )
            })
            .collect()
    }

    fn tool_names(lines: &[Value], id: u64) -> Vec<String> {
        lines.iter().find(|l| l["id"] == id).unwrap()["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap().to_string())
            .collect()
    }

    #[test]
    fn claude_code_gets_the_channel_and_say_the_desktop_app_the_tools() {
        let (mut code, _, sink) = bridge();
        code.on_line(CODE);
        let init = &sink.lines()[0];
        assert_eq!(init["id"], 1);
        assert_eq!(init["result"]["protocolVersion"], "2025-06-18");
        assert_eq!(
            init["result"]["capabilities"]["experimental"]["claude/channel"],
            json!({})
        );
        assert!(init["result"]["capabilities"]["tools"].is_object());
        let instructions = init["result"]["instructions"].as_str().unwrap();
        assert!(instructions.contains("maple_wiki_search"), "{instructions}");
        assert!(instructions.contains("kind=\"heard\""), "{instructions}");
        code.on_line(r#"{"jsonrpc":"2.0","id":3,"method":"tools/list"}"#);
        let names = tool_names(&sink.lines(), 3);
        assert_eq!(
            names,
            [
                "game_status",
                "look_at_screen",
                "maple_wiki_search",
                "maple_wiki_save",
                "player_profile",
                "player_remember",
                "say"
            ]
        );
        code.stop();

        let (mut desktop, _, sink) = bridge();
        desktop.on_line(DESKTOP);
        let init = &sink.lines()[0];
        assert!(
            !init["result"]["instructions"]
                .as_str()
                .unwrap()
                .contains("kind=\"heard\""),
            "no events for a client that takes none"
        );
        desktop.on_line(r#"{"jsonrpc":"2.0","id":3,"method":"tools/list"}"#);
        let names = tool_names(&sink.lines(), 3);
        assert!(!names.contains(&"say".to_string()), "{names:?}");
        assert_eq!(names.len(), 6);
        desktop.stop();
    }

    #[test]
    fn it_answers_the_protocol_and_nothing_else() {
        let (mut bridge, _, sink) = bridge();
        // A revision it does not know (the one channels skip) is answered
        // with its own.
        bridge.on_line(
            r#"{"jsonrpc":"2.0","id":2,"method":"initialize","params":{"protocolVersion":"2026-07-28"}}"#,
        );
        assert_eq!(sink.lines()[0]["result"]["protocolVersion"], PROTOCOL);
        bridge.on_line(r#"{"jsonrpc":"2.0","id":4,"method":"nonsense"}"#);
        assert_eq!(sink.lines()[1]["error"]["code"], -32601);
        bridge.on_line("{not json");
        assert_eq!(sink.lines()[2]["error"]["code"], -32700);
        // A notification gets no answer; a ping does.
        bridge.on_line(r#"{"jsonrpc":"2.0","method":"notifications/cancelled","params":{}}"#);
        bridge.on_line(r#"{"jsonrpc":"2.0","id":"p","method":"ping"}"#);
        let lines = sink.lines();
        assert_eq!(lines.len(), 4);
        assert_eq!(lines[3]["id"], "p");
        bridge.stop();
    }

    #[test]
    fn the_tools_read_look_search_keep_and_say() {
        let (mut bridge, fake, sink) = bridge();
        bridge.on_line(CODE);
        let call = |bridge: &mut Bridge<Fake>, id: u64, name: &str, args: Value| {
            bridge.on_line(
                &json!({"jsonrpc": "2.0", "id": id, "method": "tools/call", "params": {"name": name, "arguments": args}})
                    .to_string(),
            );
            sink.lines().into_iter().find(|l| l["id"] == id).unwrap()["result"].clone()
        };
        let said = call(&mut bridge, 10, "say", json!({"text": " בוא נלך לאליניה "}));
        assert_eq!(said["content"][0]["text"], "Said.");
        assert_eq!(
            *fake.said.lock().unwrap(),
            vec!["בוא נלך לאליניה".to_string()]
        );
        assert_eq!(
            call(&mut bridge, 11, "say", json!({"text": ""}))["isError"],
            true
        );
        let status = call(&mut bridge, 12, "game_status", json!({}));
        let text = status["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("HP 416/671 (62%)"), "{text}");
        assert!(text.contains("- Level up: 16 → 17. (30 s ago)"), "{text}");
        let look = call(
            &mut bridge,
            13,
            "look_at_screen",
            json!({"region": "top-left"}),
        );
        assert_eq!(look["content"][0]["type"], "image");
        assert_eq!(look["content"][0]["mimeType"], "image/jpeg");
        assert!(
            look["content"][1]["text"]
                .as_str()
                .unwrap()
                .contains("top-left")
        );
        // The wiki: nothing yet, then what was saved.
        let none = call(
            &mut bridge,
            14,
            "maple_wiki_search",
            json!({"query": "blue mushroom"}),
        );
        assert!(
            none["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("nothing on \"blue mushroom\"")
        );
        let saved = call(
            &mut bridge,
            15,
            "maple_wiki_save",
            json!({"about": "Blue Mushroom hunting", "fact": "Henesys Hunting Ground I.", "source": "https://example.org"}),
        );
        assert_eq!(saved["content"][0]["text"], "Saved in MapleSyrup's wiki.");
        assert_eq!(
            call(&mut bridge, 16, "maple_wiki_save", json!({"about": "x"}))["isError"],
            true
        );
        let found = call(
            &mut bridge,
            17,
            "maple_wiki_search",
            json!({"query": "blue mushroom"}),
        );
        let text = found["content"][0]["text"].as_str().unwrap();
        assert!(
            text.contains("Henesys Hunting Ground I. (source: https://example.org)"),
            "{text}"
        );
        // The player: kept once, read back.
        let kept = call(
            &mut bridge,
            18,
            "player_remember",
            json!({"fact": "Wants level 30 this week."}),
        );
        assert_eq!(kept["content"][0]["text"], "Remembered for good.");
        let again = call(
            &mut bridge,
            19,
            "player_remember",
            json!({"fact": "Wants level 30 this week."}),
        );
        assert_eq!(again["content"][0]["text"], "Already known.");
        let profile = call(&mut bridge, 20, "player_profile", json!({}));
        assert!(
            profile["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("Wants level 30 this week.")
        );
        bridge.stop();
    }

    #[test]
    fn events_flow_to_claude_code_after_initialized_with_a_hello_first() {
        let (mut bridge, fake, sink) = bridge();
        fake.post("death", "too early");
        bridge.on_line(CODE);
        std::thread::sleep(Duration::from_millis(150));
        assert!(
            channel_events(&sink.lines()).is_empty(),
            "not before initialized"
        );
        bridge.on_line(READY);
        let lines = wait_for(&sink, |l| !channel_events(l).is_empty());
        let events = channel_events(&lines);
        assert_eq!(events[0].0, "hello");
        assert!(events[0].1.contains("HP 416/671"), "{}", events[0].1);
        assert!(
            fake.listening.load(Ordering::Relaxed),
            "the companion hears Claude listens"
        );
        fake.post("heard", "He said: כמה HP יש לי?");
        fake.post("level_up", "Level up: 16 → 17.");
        let lines = wait_for(&sink, |l| channel_events(l).len() >= 3);
        let kinds: Vec<String> = channel_events(&lines).into_iter().map(|(k, _)| k).collect();
        assert_eq!(kinds, ["hello", "heard", "level_up"]);
        // Every line is a whole message (the reader parsed each).
        assert!(lines.iter().all(|l| l["jsonrpc"] == "2.0"));
        bridge.stop();
    }

    #[test]
    fn the_desktop_app_is_sent_no_events_and_the_companion_keeps_talking() {
        let (mut bridge, fake, sink) = bridge();
        bridge.on_line(DESKTOP);
        bridge.on_line(READY);
        fake.post("heard", "He said: hi");
        fake.post("level_up", "Level up: 16 → 17.");
        std::thread::sleep(Duration::from_millis(400));
        assert!(channel_events(&sink.lines()).is_empty());
        assert!(
            !fake.listening.load(Ordering::Relaxed),
            "polled, but not listening"
        );
        bridge.stop();
    }

    #[test]
    fn a_companion_that_stops_is_told_once_and_one_that_comes_back_says_hello() {
        let (mut bridge, fake, sink) = bridge();
        fake.down.store(true, Ordering::Relaxed);
        bridge.on_line(CODE);
        bridge.on_line(READY);
        let lines = wait_for(&sink, |l| !channel_events(l).is_empty());
        let events = channel_events(&lines);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "offline");
        std::thread::sleep(Duration::from_millis(300));
        assert_eq!(channel_events(&sink.lines()).len(), 1, "said once");
        fake.down.store(false, Ordering::Relaxed);
        let lines = wait_for(&sink, |l| channel_events(l).len() >= 2);
        assert_eq!(channel_events(&lines)[1].0, "hello");
        bridge.stop();
    }

    #[test]
    fn a_companion_started_again_is_greeted_again_and_its_events_come_through() {
        let (mut bridge, fake, sink) = bridge();
        bridge.on_line(CODE);
        bridge.on_line(READY);
        wait_for(&sink, |l| !channel_events(l).is_empty());
        fake.post("map", "New map: Ellinia.");
        fake.post("hp", "HP low: 28%.");
        wait_for(&sink, |l| channel_events(l).len() >= 3);
        // Started again, it numbers from one: the bridge, ahead of it,
        // would wait for its third event — the new run says hello instead.
        fake.restart();
        let lines = wait_for(&sink, |l| channel_events(l).len() >= 4);
        assert_eq!(channel_events(&lines)[3].0, "hello");
        fake.post("death", "The character died.");
        let lines = wait_for(&sink, |l| channel_events(l).len() >= 5);
        let kinds: Vec<String> = channel_events(&lines).into_iter().map(|(k, _)| k).collect();
        assert_eq!(kinds, ["hello", "map", "hp", "hello", "death"]);
        bridge.stop();
    }

    #[test]
    fn a_query_is_escaped_for_the_url() {
        assert_eq!(escape("blue mushroom"), "blue%20mushroom");
        assert_eq!(escape("איפה"), "%D7%90%D7%99%D7%A4%D7%94");
        assert_eq!(escape("a&k=b"), "a%26k%3Db");
        assert!(is_claude_code("claude-code"));
        assert!(!is_claude_code("claude-ai"));
    }
}
