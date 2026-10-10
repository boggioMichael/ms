//! The bridge Claude Code starts: `MapleSyrup --claude-channel`.
//!
//! An MCP server on stdio (one JSON-RPC message per line) that declares
//! Claude Code's `claude/channel` capability, so the session listens for
//! it, and then pushes every event the running companion posts — what the
//! player said, a death, a level-up, a new map, HP in danger, a dialog, the
//! state of things — as a `notifications/claude/channel`, which reaches
//! Claude as `<channel source="maplesyrup" kind="…">`. Its tools let Claude
//! talk back and look: `say` (MapleSyrup's voice says it), `game_status`
//! (the reading now) and `look_at_screen` (a picture of the screen).
//!
//! It finds the companion through [`super::link`], and keeps finding it: a
//! companion started after Claude, or restarted, is picked up; one that
//! stopped is told to Claude once. Only the protocol goes to stdout; what
//! it has to say for itself goes to stderr (Claude Code's debug log).

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

/// What Claude is told about the channel when the server connects.
pub const INSTRUCTIONS: &str = "\
MapleSyrup watches the player's MapleStory screen live (it runs on his PC) and pushes what it reads into this \
session as <channel source=\"maplesyrup\" kind=\"...\"> events. He is playing: he can't read this terminal, \
and he hears you only through the say tool.

kind=\"heard\": he just said this out loud (speech recognition from his phone; may have recognition \
mistakes). Answer him with the say tool, short and spoken, in his language, at once.
kind=\"hello\": the link to MapleSyrup just came up; say hi in one short line.
kind=\"death\" | \"level_up\" | \"map\" | \"hp\" | \"mp\" | \"dialog\" | \"thing\" | \"window\" | \"state\": something \
changed in the game. Speak (say) only when one short sentence helps him right now — danger, a real milestone, \
a quest or dialog you can help with, something he asked you to watch. Otherwise stay silent: don't call say.
kind=\"offline\": MapleSyrup stopped; there is no live reading until it starts again.

Every event ends with the whole reading at that moment: HP/MP/EXP with how old each number is, level, map, \
what moves on the screen and where (x, y as percent of the screen from its top left; MapleSyrup doesn't say \
what each thing is), the action, an open dialog and its text, buff icons, the things he taught MapleSyrup, \
and the session. game_status gives the latest at any time; look_at_screen shows you the screen itself — use \
it for names of monsters, NPCs, items, quests and windows. Never give a number older than about ten seconds \
as the current one.";

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
    fn events(&self, since: Option<u64>, wait: Duration) -> Result<Batch, String>;
    /// The reading now: `{status, text, recent}`.
    fn status(&self) -> Result<Value, String>;
    fn say(&self, text: &str) -> Result<(), String>;
    /// `{mime, data, width, height, region, frame}`.
    fn look(&self, region: &str) -> Result<Value, String>;
}

/// The companion on this PC, found through its link file each time (it
/// may have started after the bridge, or again on another port).
pub struct Local {
    settings: PathBuf,
}

impl Local {
    pub fn new(settings: &Path) -> Local {
        Local {
            settings: settings.to_path_buf(),
        }
    }

    fn call(&self, method: &str, path: &str, body: &[u8], wait: Duration) -> Result<Value, String> {
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
}

impl Api for Local {
    fn events(&self, since: Option<u64>, wait: Duration) -> Result<Batch, String> {
        let path = match since {
            None => "/local/events?fresh=1".to_string(),
            Some(since) => format!("/local/events?since={since}&wait={}", wait.as_millis()),
        };
        let value = self.call("GET", &path, &[], wait + Duration::from_secs(10))?;
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

pub struct Bridge<A: Api + 'static> {
    api: Arc<A>,
    out: Out,
    feeding: bool,
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
        // The session is ready for the channel's events.
        if method == "notifications/initialized" && !self.feeding {
            self.feeding = true;
            let (api, out, stop, timing) = (
                Arc::clone(&self.api),
                Arc::clone(&self.out),
                Arc::clone(&self.stop),
                self.timing,
            );
            let _ = std::thread::Builder::new()
                .name("channel-feed".into())
                .spawn(move || feed(api, out, stop, timing));
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
                Ok(json!({
                    "protocolVersion": version,
                    "capabilities": {
                        "experimental": {"claude/channel": {}},
                        "tools": {},
                    },
                    "serverInfo": {"name": "maplesyrup", "title": "MapleSyrup", "version": env!("CARGO_PKG_VERSION")},
                    "instructions": INSTRUCTIONS,
                }))
            }
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({ "tools": tools() })),
            "tools/call" => Ok(self.call(params)),
            "resources/list" => Ok(json!({"resources": []})),
            "resources/templates/list" => Ok(json!({"resourceTemplates": []})),
            "prompts/list" => Ok(json!({"prompts": []})),
            other => Err((-32601, format!("Method not found: {other}"))),
        }
    }

    fn call(&self, params: &Value) -> Value {
        let arguments = &params["arguments"];
        match params["name"].as_str().unwrap_or_default() {
            "say" => {
                let text = arguments["text"].as_str().unwrap_or_default().trim();
                if text.is_empty() {
                    return failed("Nothing to say: give the text.");
                }
                match self.api.say(text) {
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

fn tools() -> Value {
    json!([
        {
            "name": "say",
            "title": "Say it to him",
            "description": "Say this out loud to the player in MapleSyrup's voice (his speakers or his phone). It is the only \
    way he hears you while he plays, so every answer to something he said goes through here. Spoken style: one or two \
    short sentences unless he asked for more, no lists or markdown, in the language he spoke (usually Hebrew), \
    addressing him in the masculine.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "text": {"type": "string", "description": "What to say, exactly as it should be spoken."}
                },
                "required": ["text"],
                "additionalProperties": false,
            },
            "annotations": {"title": "Say it to him", "readOnlyHint": false, "destructiveHint": false, "idempotentHint": false, "openWorldHint": false},
        },
        {
            "name": "game_status",
            "title": "MapleStory, now",
            "description": "Everything MapleSyrup reads on the player's MapleStory screen right now: the window, the \
    character and level, HP/MP/EXP with how old each number is, the map, what moves on the screen and where, the \
    action, an open dialog and its text, buff icons, the things he taught it, the session so far, and the last \
    events. The events already carry the reading; call this for the latest at any moment.",
            "inputSchema": {"type": "object", "properties": {}, "additionalProperties": false},
            "annotations": {"title": "MapleStory, now", "readOnlyHint": true, "destructiveHint": false, "idempotentHint": true, "openWorldHint": false},
        },
        {
            "name": "look_at_screen",
            "title": "Look at his screen",
            "description": "A picture of the player's screen right now (only while MapleStory is the window in front), \
    or part of it, at a size where the game's text is readable. Use it for what the reading doesn't name: monsters, \
    NPCs, items, quest windows, the map name on the minimap, his character.",
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
            "annotations": {"title": "Look at his screen", "readOnlyHint": true, "destructiveHint": false, "idempotentHint": true, "openWorldHint": false},
        }
    ])
}

/// The events, as they come, to Claude — and the companion coming and going.
fn feed<A: Api>(api: Arc<A>, out: Out, stop: Arc<AtomicBool>, timing: Timing) {
    // Where the events are up to (None: start afresh), and whose they are.
    let mut since: Option<u64> = None;
    let mut boot: Option<String> = None;
    // Whether Claude was last told the companion is there.
    let mut told: Option<bool> = None;
    let mut failing: Option<Instant> = None;
    while !stop.load(Ordering::Relaxed) {
        match api.events(since, timing.poll) {
            Ok(batch) => {
                failing = None;
                // A companion that started again numbers its events from
                // one: start over with it.
                let new_run = boot.as_deref() != Some(batch.boot.as_str());
                if told != Some(true) || new_run {
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
                    told = Some(true);
                    boot = Some(batch.boot.clone());
                    if since.is_none() || new_run {
                        // What came before the hello is in its reading.
                        since = Some(batch.last);
                        continue;
                    }
                }
                for event in &batch.events {
                    notify(
                        &out,
                        event["kind"].as_str().unwrap_or("event"),
                        event["text"].as_str().unwrap_or_default(),
                    );
                }
                since = Some(batch.last);
            }
            Err(why) => {
                let first = *failing.get_or_insert_with(Instant::now);
                if told != Some(false) && first.elapsed() >= timing.offline_after {
                    eprintln!("maplesyrup channel: {why}");
                    notify(
                        &out,
                        "offline",
                        "MapleSyrup isn't running (or stopped), so there is no live reading of the game until it \
is started again.",
                    );
                    told = Some(false);
                }
                since = None;
                std::thread::sleep(timing.retry);
            }
        }
    }
}

/// Run the bridge on stdin and stdout until the client goes. Returns the
/// process's exit code.
pub fn run(settings: &Path) -> i32 {
    let out: Out = Arc::new(Mutex::new(Box::new(std::io::stdout())));
    let mut bridge = Bridge::new(Arc::new(Local::new(settings)), out);
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
        fn events(&self, since: Option<u64>, _wait: Duration) -> Result<Batch, String> {
            if self.down.load(Ordering::Relaxed) {
                return Err("MapleSyrup isn't running".into());
            }
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

    #[test]
    fn it_declares_itself_a_claude_channel_with_its_tools() {
        let (mut bridge, _, sink) = bridge();
        bridge.on_line(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"claude-code","version":"2.1.296"}}}"#,
        );
        let init = &sink.lines()[0];
        assert_eq!(init["id"], 1);
        assert_eq!(init["result"]["protocolVersion"], "2025-06-18");
        assert_eq!(
            init["result"]["capabilities"]["experimental"]["claude/channel"],
            json!({})
        );
        assert!(init["result"]["capabilities"]["tools"].is_object());
        assert!(
            init["result"]["instructions"]
                .as_str()
                .unwrap()
                .contains("kind=\"heard\"")
        );
        // A revision it does not know (the one channels skip) is answered
        // with its own.
        bridge.on_line(
            r#"{"jsonrpc":"2.0","id":2,"method":"initialize","params":{"protocolVersion":"2026-07-28"}}"#,
        );
        assert_eq!(sink.lines()[1]["result"]["protocolVersion"], PROTOCOL);
        bridge.on_line(r#"{"jsonrpc":"2.0","id":3,"method":"tools/list"}"#);
        let tools = sink.lines()[2]["result"]["tools"].clone();
        let names: Vec<&str> = tools
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, ["say", "game_status", "look_at_screen"]);
        assert_eq!(tools[1]["annotations"]["readOnlyHint"], true);
        bridge.on_line(r#"{"jsonrpc":"2.0","id":4,"method":"nonsense"}"#);
        assert_eq!(sink.lines()[3]["error"]["code"], -32601);
        bridge.on_line("{not json");
        assert_eq!(sink.lines()[4]["error"]["code"], -32700);
        // A notification gets no answer; a ping does.
        bridge.on_line(r#"{"jsonrpc":"2.0","method":"notifications/cancelled","params":{}}"#);
        bridge.on_line(r#"{"jsonrpc":"2.0","id":"p","method":"ping"}"#);
        let lines = sink.lines();
        assert_eq!(lines.len(), 6);
        assert_eq!(lines[5]["id"], "p");
        bridge.stop();
    }

    #[test]
    fn the_tools_say_read_and_look() {
        let (mut bridge, fake, sink) = bridge();
        bridge.on_line(
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"say","arguments":{"text":" בוא נלך לאליניה "}}}"#,
        );
        assert_eq!(
            *fake.said.lock().unwrap(),
            vec!["בוא נלך לאליניה".to_string()]
        );
        assert_eq!(sink.lines()[0]["result"]["content"][0]["text"], "Said.");
        bridge.on_line(
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"say","arguments":{"text":""}}}"#,
        );
        assert_eq!(sink.lines()[1]["result"]["isError"], true);
        bridge.on_line(
            r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"game_status","arguments":{}}}"#,
        );
        let status = sink.lines()[2]["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .to_string();
        assert!(status.contains("HP 416/671 (62%)"), "{status}");
        assert!(
            status.contains("- Level up: 16 → 17. (30 s ago)"),
            "{status}"
        );
        bridge.on_line(
            r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"look_at_screen","arguments":{"region":"top-left"}}}"#,
        );
        let look = &sink.lines()[3]["result"]["content"];
        assert_eq!(look[0]["type"], "image");
        assert_eq!(look[0]["mimeType"], "image/jpeg");
        assert_eq!(look[0]["data"], "AAAA");
        assert!(look[1]["text"].as_str().unwrap().contains("top-left"));
        bridge.stop();
    }

    #[test]
    fn events_flow_to_claude_after_initialized_with_a_hello_first() {
        let (mut bridge, fake, sink) = bridge();
        fake.post("death", "too early");
        bridge.on_line(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#);
        std::thread::sleep(Duration::from_millis(150));
        assert!(
            channel_events(&sink.lines()).is_empty(),
            "not before initialized"
        );
        bridge.on_line(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#);
        let lines = wait_for(&sink, |l| !channel_events(l).is_empty());
        let events = channel_events(&lines);
        assert_eq!(events[0].0, "hello");
        assert!(events[0].1.contains("HP 416/671"), "{}", events[0].1);
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
    fn a_companion_that_stops_is_told_once_and_one_that_comes_back_says_hello() {
        let (mut bridge, fake, sink) = bridge();
        fake.down.store(true, Ordering::Relaxed);
        bridge.on_line(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#);
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
        bridge.on_line(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#);
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
}
