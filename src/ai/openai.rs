//! OpenAI, reached through `curl` — the one Windows 10 and 11 ship in
//! System32. It uses the system's own certificate store, so it works behind
//! antivirus HTTPS scanning and corporate proxies, and it adds no TLS
//! client to MapleSyrup. Requests go in on stdin, answers come back on
//! stdout, and the HTTP status on stderr.
//!
//! Two calls: a reply to the player (the Responses API, streamed, so the
//! first sentence can be spoken while the rest is still being written) and
//! that reply as speech (`/audio/speech`, raw 24 kHz 16-bit PCM).

use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Value, json};

/// Models tried for conversation, fastest first; the first one the key can
/// use is kept. (As of October 2026, gpt-6-luna is OpenAI's efficient model.)
pub const CHAT_MODELS: &[&str] = &[
    "gpt-6-luna",
    "gpt-6.1-sol",
    "gpt-5-mini",
    "gpt-4.1-mini",
    "gpt-4o-mini",
];
/// Models tried for looking at the screen (finding the HUD, reading small
/// print): the sharpest eyes first; these calls are few.
pub const VISION_MODELS: &[&str] = &[
    "gpt-6.1-sol",
    "gpt-6-luna",
    "gpt-5-mini",
    "gpt-4.1-mini",
    "gpt-4o-mini",
];
pub const SPEECH_MODEL: &str = "gpt-4o-mini-tts";
/// Raw PCM from `/audio/speech` is 24 kHz, 16-bit, mono.
pub const SPEECH_RATE: u32 = 24_000;

#[derive(Debug, Clone, PartialEq)]
pub enum AiError {
    /// curl could not be run.
    NoCurl(String),
    /// The network or OpenAI could not be reached.
    Network(String),
    /// OpenAI answered with an error: the status and its message.
    Http(u16, String),
    /// The answer could not be read.
    Parse(String),
    /// Called off before it was done (the player talked over it).
    Cancelled,
}

impl std::fmt::Display for AiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AiError::NoCurl(e) => write!(f, "curl could not run ({e})"),
            AiError::Network(e) => write!(f, "could not reach OpenAI ({e})"),
            AiError::Http(401, _) => write!(f, "OpenAI refused the API key"),
            AiError::Http(429, m) => write!(f, "OpenAI says slow down or add credit ({m})"),
            AiError::Http(code, m) => write!(f, "OpenAI error {code}: {m}"),
            AiError::Parse(e) => write!(f, "unexpected answer from OpenAI ({e})"),
            AiError::Cancelled => write!(f, "called off"),
        }
    }
}

/// Calls a piece of work off from another thread: everything numbered up
/// to the shared mark is called off, and a request running for it has its
/// curl stopped at once.
#[derive(Debug, Clone)]
pub struct Stop {
    mark: Arc<AtomicU64>,
    id: u64,
}

impl Stop {
    /// Work number `id`, called off once `mark` reaches it.
    pub fn new(mark: Arc<AtomicU64>, id: u64) -> Stop {
        Stop { mark, id }
    }

    pub fn stopped(&self) -> bool {
        self.mark.load(Ordering::SeqCst) >= self.id
    }
}

/// How a request is made.
#[derive(Default, Clone, Copy)]
struct Via<'a> {
    stop: Option<&'a Stop>,
    /// The status line and headers come before the body (curl's `-i`), so a
    /// streamed body can be told from an error before it is used.
    headers: bool,
}

/// The status and headers in front of a body (curl's `-i`), skipping an
/// interim answer (100 Continue) or a proxy's.
#[derive(Default)]
struct Head {
    buf: Vec<u8>,
    status: Option<u16>,
}

impl Head {
    /// Bytes as they arrive: the part of them that is the body.
    fn feed(&mut self, chunk: &[u8]) -> Vec<u8> {
        if self.status.is_some() {
            return chunk.to_vec();
        }
        self.buf.extend_from_slice(chunk);
        loop {
            let Some(end) = self.buf.windows(4).position(|w| w == b"\r\n\r\n") else {
                return Vec::new();
            };
            let head = String::from_utf8_lossy(&self.buf[..end]).to_ascii_lowercase();
            self.buf.drain(..end + 4);
            let first = head.lines().next().unwrap_or_default();
            let code = first
                .split_whitespace()
                .nth(1)
                .and_then(|c| c.parse::<u16>().ok())
                .unwrap_or(0);
            if (100..200).contains(&code) || first.contains("connection established") {
                continue;
            }
            self.status = Some(code);
            return std::mem::take(&mut self.buf);
        }
    }
}

/// One turn of the conversation.
#[derive(Debug, Clone, PartialEq)]
pub struct Turn {
    /// "user" or "assistant".
    pub role: &'static str,
    pub text: String,
}

/// The chosen model, and what it turned out to take.
#[derive(Debug, Clone)]
struct Choice {
    model: String,
    /// `reasoning.effort` (newer models).
    reasoning: bool,
    /// Pictures in the input.
    images: bool,
    /// The hosted web search tool.
    web: bool,
}

impl Choice {
    fn new(model: &str) -> Choice {
        Choice {
            model: model.to_string(),
            reasoning: true,
            images: true,
            web: true,
        }
    }
}

/// One request to the Responses API.
#[derive(Debug, Clone, Default)]
pub struct Ask {
    pub instructions: String,
    /// Input items: messages (text and pictures), function calls and their
    /// outputs.
    pub input: Vec<Value>,
    /// Function tools, and hosted ones (`{"type": "web_search"}`).
    pub tools: Vec<Value>,
    /// A JSON schema the answer must follow: its name and the schema.
    pub schema: Option<(String, Value)>,
    pub max_output_tokens: u32,
    pub timeout: Duration,
    /// Pictures are the point of the question: a model that cannot take
    /// them is skipped rather than asked without them.
    pub needs_images: bool,
    /// Lets it be called off (the player talked over the reply).
    pub stop: Option<Stop>,
}

/// A function the model wants called.
#[derive(Debug, Clone, PartialEq)]
pub struct Call {
    pub call_id: String,
    pub name: String,
    pub arguments: String,
}

/// What came back from a request.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Answer {
    pub text: String,
    pub calls: Vec<Call>,
    /// The answer's output items, to send back with the calls' outputs.
    pub items: Vec<Value>,
}

/// What arrives while a reply streams in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Piece<'a> {
    Text(&'a str),
    /// The model started searching the web.
    Searching,
}

pub struct OpenAi {
    key: String,
    base: String,
    pub voice: String,
    curl: String,
    choice: Mutex<Option<Choice>>,
    /// Models to try, in order (`CHAT_MODELS`, or the one asked for).
    models: Vec<String>,
}

impl OpenAi {
    /// `base`: `https://api.openai.com/v1`, or a stand-in for tests.
    pub fn new(key: &str, base: &str, voice: &str, model: Option<&str>) -> Self {
        Self::with_models(
            key,
            base,
            voice,
            match model {
                Some(m) => vec![m.to_string()],
                None => CHAT_MODELS.iter().map(|m| m.to_string()).collect(),
            },
        )
    }

    /// A client that tries `models` in order.
    pub fn with_models(key: &str, base: &str, voice: &str, models: Vec<String>) -> Self {
        Self {
            key: key.trim().to_string(),
            base: base.trim_end_matches('/').to_string(),
            voice: voice.to_string(),
            curl: if cfg!(windows) {
                "curl.exe".into()
            } else {
                "curl".into()
            },
            choice: Mutex::new(None),
            models,
        }
    }

    pub fn model(&self) -> Option<String> {
        self.choice.lock().ok()?.as_ref().map(|c| c.model.clone())
    }

    /// One HTTP request: `body` as JSON on stdin (or none for GET). Returns the
    /// status and the raw body.
    fn call(
        &self,
        method: &str,
        path: &str,
        body: Option<&Value>,
        timeout: Duration,
    ) -> Result<(u16, Vec<u8>), AiError> {
        let mut all = Vec::new();
        let status = self.call_with(method, path, body, timeout, Via::default(), &mut |chunk| {
            all.extend_from_slice(chunk)
        })?;
        Ok((status, all))
    }

    /// One HTTP request, its answer handed to `on_body` piece by piece as it
    /// arrives (curl's `--no-buffer`). Returns the HTTP status. A request
    /// with a `stop` that is called off ends at once, as `Cancelled`.
    fn call_with(
        &self,
        method: &str,
        path: &str,
        body: Option<&Value>,
        timeout: Duration,
        via: Via,
        on_body: &mut dyn FnMut(&[u8]),
    ) -> Result<u16, AiError> {
        if via.stop.is_some_and(Stop::stopped) {
            return Err(AiError::Cancelled);
        }
        let url = format!("{}{path}", self.base);
        let mut command = Command::new(&self.curl);
        command
            .args([
                "--silent",
                "--show-error",
                "--no-buffer",
                "--connect-timeout",
                "8",
                "--max-time",
            ])
            .arg(timeout.as_secs().max(1).to_string())
            .args(["-X", method])
            .args(["-H", &format!("Authorization: Bearer {}", self.key)])
            .args(["-w", "%{stderr}HTTPSTATUS:%{http_code}"]);
        if via.headers {
            command.args(["-i", "--suppress-connect-headers"]);
        }
        if body.is_some() {
            command.args([
                "-H",
                "Content-Type: application/json",
                "--data-binary",
                "@-",
            ]);
        }
        command.arg(&url);
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            // No console window of its own.
            command.creation_flags(0x0800_0000);
        }
        let mut child = command
            .spawn()
            .map_err(|e| AiError::NoCurl(e.to_string()))?;
        // The body goes in on stdin; closing it (the drop) ends the request.
        if let Some(mut stdin) = child.stdin.take()
            && let Some(body) = body
        {
            let bytes = serde_json::to_vec(body).map_err(|e| AiError::Parse(e.to_string()))?;
            stdin
                .write_all(&bytes)
                .map_err(|e| AiError::Network(e.to_string()))?;
        }
        // curl's errors and the status, read on the side so neither pipe can
        // fill up and stall it.
        let mut errors = child.stderr.take();
        let error_reader = std::thread::spawn(move || {
            let mut bytes = Vec::new();
            if let Some(stderr) = errors.as_mut() {
                let _ = stderr.read_to_end(&mut bytes);
            }
            bytes
        });
        let stdout = child.stdout.take();
        // Called off: curl is stopped from the side, which ends the reading.
        let child = Arc::new(Mutex::new(child));
        let finished = Arc::new(AtomicBool::new(false));
        let watcher = via.stop.cloned().map(|stop| {
            let child = Arc::clone(&child);
            let finished = Arc::clone(&finished);
            std::thread::spawn(move || {
                while !finished.load(Ordering::SeqCst) {
                    if stop.stopped() {
                        if let Ok(mut child) = child.lock() {
                            let _ = child.kill();
                        }
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(15));
                }
            })
        });
        if let Some(mut stdout) = stdout {
            let mut buffer = [0u8; 8192];
            loop {
                match stdout.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(n) => {
                        if via.stop.is_some_and(Stop::stopped) {
                            break;
                        }
                        on_body(&buffer[..n])
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(_) => break,
                }
            }
        }
        finished.store(true, Ordering::SeqCst);
        if let Some(watcher) = watcher {
            let _ = watcher.join();
        }
        if via.stop.is_some_and(Stop::stopped)
            && let Ok(mut child) = child.lock()
        {
            let _ = child.kill();
        }
        let stderr_bytes = error_reader.join().unwrap_or_default();
        if let Ok(mut child) = child.lock() {
            let _ = child.wait();
        }
        if via.stop.is_some_and(Stop::stopped) {
            return Err(AiError::Cancelled);
        }
        let stderr = String::from_utf8_lossy(&stderr_bytes);
        let status = stderr
            .rsplit("HTTPSTATUS:")
            .next()
            .and_then(|s| s.trim().parse::<u16>().ok())
            .unwrap_or(0);
        if status == 0 {
            let message = stderr
                .split("HTTPSTATUS:")
                .next()
                .unwrap_or_default()
                .trim()
                .to_string();
            return Err(AiError::Network(if message.is_empty() {
                "no answer".into()
            } else {
                message
            }));
        }
        Ok(status)
    }

    fn error_of(status: u16, body: &[u8]) -> AiError {
        let message = serde_json::from_slice::<Value>(body)
            .ok()
            .and_then(|v| v["error"]["message"].as_str().map(String::from))
            .unwrap_or_else(|| String::from_utf8_lossy(body).chars().take(200).collect());
        AiError::Http(status, message)
    }

    /// A GET that answers JSON.
    pub fn get_json(&self, path: &str) -> Result<Value, AiError> {
        let (status, body) = self.call("GET", path, None, Duration::from_secs(15))?;
        if status != 200 {
            return Err(Self::error_of(status, &body));
        }
        serde_json::from_slice(&body).map_err(|e| AiError::Parse(e.to_string()))
    }

    /// A POST of JSON that answers JSON.
    pub fn post_json(&self, path: &str, body: &Value, timeout: Duration) -> Result<Value, AiError> {
        let (status, raw) = self.call("POST", path, Some(body), timeout)?;
        if !(200..300).contains(&status) {
            return Err(Self::error_of(status, &raw));
        }
        serde_json::from_slice(&raw).map_err(|e| AiError::Parse(e.to_string()))
    }

    /// The models this key may use, with when each was made.
    pub fn models(&self) -> Result<Vec<(String, i64)>, AiError> {
        let list = self.get_json("/models")?;
        Ok(list["data"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|m| {
                Some((
                    m["id"].as_str()?.to_string(),
                    m["created"].as_i64().unwrap_or(0),
                ))
            })
            .collect())
    }

    /// Where requests go (`https://api.openai.com/v1`, or a stand-in).
    pub fn base(&self) -> &str {
        &self.base
    }

    /// Whether the key works (lists the models it may use).
    pub fn check(&self) -> Result<(), AiError> {
        let (status, body) = self.call("GET", "/models", None, Duration::from_secs(15))?;
        if status == 200 {
            Ok(())
        } else {
            Err(Self::error_of(status, &body))
        }
    }

    /// The model's reply to the conversation so far.
    pub fn respond(&self, instructions: &str, turns: &[Turn]) -> Result<String, AiError> {
        Ok(self
            .ask(&Ask::conversation(instructions, turns), None)?
            .text)
    }

    /// The model's reply, streamed: `on_text` gets each piece as it is
    /// written. Returns the whole reply.
    pub fn respond_stream(
        &self,
        instructions: &str,
        turns: &[Turn],
        on_text: &mut dyn FnMut(&str),
    ) -> Result<String, AiError> {
        let mut on_piece = |piece: Piece| {
            if let Piece::Text(t) = piece {
                on_text(t)
            }
        };
        Ok(self
            .ask(&Ask::conversation(instructions, turns), Some(&mut on_piece))?
            .text)
    }

    /// One request, streamed to `on_piece` when given. The first model the
    /// key can use is kept; what a model turns out not to take (reasoning
    /// effort, pictures, web search) is left out and the request sent again.
    pub fn ask(
        &self,
        ask: &Ask,
        mut on_piece: Option<&mut dyn FnMut(Piece)>,
    ) -> Result<Answer, AiError> {
        let known = self.choice.lock().ok().and_then(|c| c.clone());
        let candidates: Vec<Choice> = match known {
            Some(choice) => vec![choice],
            None => self.models.iter().map(|m| Choice::new(m)).collect(),
        };
        let has_images = ask.input.iter().any(has_image);
        let has_web = ask.tools.iter().any(|t| t["type"] != "function");
        let mut last = AiError::Parse("no model to try".into());
        // A hiccup on OpenAI's side (a 5xx) is tried once more.
        let mut retried = false;
        for mut choice in candidates {
            if ask.needs_images && !choice.images {
                continue;
            }
            loop {
                let input: Vec<Value> = if choice.images {
                    ask.input.clone()
                } else {
                    ask.input.iter().map(without_images).collect()
                };
                let tools: Vec<&Value> = ask
                    .tools
                    .iter()
                    .filter(|t| choice.web || t["type"] == "function")
                    .collect();
                let mut body = json!({
                    "model": choice.model,
                    "instructions": ask.instructions,
                    "input": input,
                    "max_output_tokens": ask.max_output_tokens.max(16),
                    "store": false,
                });
                if !tools.is_empty() {
                    body["tools"] = json!(tools);
                }
                if let Some((name, schema)) = &ask.schema {
                    body["text"] = json!({"format": {
                        "type": "json_schema", "name": name, "schema": schema, "strict": true
                    }});
                }
                if choice.reasoning {
                    body["reasoning"] = json!({"effort": "none"});
                }
                let timeout = if ask.timeout.is_zero() {
                    Duration::from_secs(40)
                } else {
                    ask.timeout
                };
                let mut events = EventStream::default();
                let via = Via {
                    stop: ask.stop.as_ref(),
                    headers: false,
                };
                let status = match on_piece.as_mut() {
                    Some(on_piece) => {
                        body["stream"] = json!(true);
                        self.call_with(
                            "POST",
                            "/responses",
                            Some(&body),
                            timeout,
                            via,
                            &mut |chunk| {
                                for piece in events.feed(chunk) {
                                    match piece {
                                        Event::Text(t) => on_piece(Piece::Text(&t)),
                                        Event::Searching => on_piece(Piece::Searching),
                                    }
                                }
                            },
                        )?
                    }
                    None => self.call_with(
                        "POST",
                        "/responses",
                        Some(&body),
                        timeout,
                        via,
                        &mut |chunk| events.keep(chunk),
                    )?,
                };
                if status == 200 {
                    if let Some(message) = events.error {
                        return Err(AiError::Http(500, message));
                    }
                    let answer = if events.items.is_empty() && events.text.trim().is_empty() {
                        // A whole answer (not streamed, or buffered on the way).
                        let answer = match &events.completed {
                            Some(response) => answer_of(response),
                            None => answer_of(
                                &serde_json::from_slice::<Value>(&events.raw)
                                    .map_err(|e| AiError::Parse(e.to_string()))?,
                            ),
                        };
                        if let Some(on_piece) = on_piece.as_mut()
                            && !answer.text.is_empty()
                        {
                            on_piece(Piece::Text(&answer.text));
                        }
                        answer
                    } else {
                        Answer {
                            text: events.text.trim().to_string(),
                            calls: events.calls,
                            items: events.items,
                        }
                    };
                    if answer.text.trim().is_empty() && answer.calls.is_empty() {
                        return Err(AiError::Parse("the answer had no text".into()));
                    }
                    if let Ok(mut slot) = self.choice.lock() {
                        *slot = Some(choice.clone());
                    }
                    return Ok(answer);
                }
                let error = Self::error_of(status, &events.raw);
                let message = match &error {
                    AiError::Http(_, m) => m.to_ascii_lowercase(),
                    _ => String::new(),
                };
                if matches!(status, 500 | 502 | 503 | 504) && !retried {
                    retried = true;
                    std::thread::sleep(Duration::from_millis(700));
                    if ask.stop.as_ref().is_some_and(Stop::stopped) {
                        return Err(AiError::Cancelled);
                    }
                    continue;
                }
                if status == 400 {
                    // An older model without reasoning efforts: ask again without.
                    if choice.reasoning
                        && (message.contains("reasoning") || message.contains("effort"))
                    {
                        choice.reasoning = false;
                        continue;
                    }
                    // No web search for this model or key.
                    if has_web
                        && choice.web
                        && (message.contains("web_search") || message.contains("tool"))
                    {
                        choice.web = false;
                        continue;
                    }
                    // No pictures for this model.
                    if has_images && choice.images && message.contains("image") {
                        choice.images = false;
                        if ask.needs_images {
                            last = error;
                            break;
                        }
                        continue;
                    }
                }
                last = error;
                // A model this key cannot use: try the next one.
                if matches!(status, 400 | 403 | 404) && (message.contains("model") || status == 404)
                {
                    break;
                }
                return Err(last);
            }
        }
        Err(last)
    }

    /// `text` spoken in the chosen voice, as 24 kHz mono samples. `style`
    /// tells the voice how to sound.
    pub fn speech(&self, text: &str, style: &str) -> Result<Vec<i16>, AiError> {
        let mut all = Vec::new();
        self.speech_stream(text, style, None, &mut |samples| {
            all.extend_from_slice(samples)
        })?;
        Ok(all)
    }

    /// `text` spoken, handed to `on_samples` a piece at a time as the voice
    /// is made (24 kHz mono), so it can be played before it is complete.
    /// Returns how many samples there were.
    pub fn speech_stream(
        &self,
        text: &str,
        style: &str,
        stop: Option<&Stop>,
        on_samples: &mut dyn FnMut(&[i16]),
    ) -> Result<usize, AiError> {
        let mut voice = self.voice.clone();
        for attempt in 0..2 {
            let body = json!({
                "model": SPEECH_MODEL,
                "voice": voice,
                "input": text,
                "instructions": style,
                "response_format": "pcm",
            });
            let mut head = Head::default();
            let mut error = Vec::new();
            let mut odd: Option<u8> = None;
            let mut count = 0;
            let status = self.call_with(
                "POST",
                "/audio/speech",
                Some(&body),
                Duration::from_secs(40),
                Via {
                    stop,
                    headers: true,
                },
                &mut |chunk| {
                    let bytes = head.feed(chunk);
                    if bytes.is_empty() {
                        return;
                    }
                    if head.status != Some(200) {
                        if error.len() < 4096 {
                            error.extend_from_slice(&bytes);
                        }
                        return;
                    }
                    // A sample can be split between two pieces.
                    let mut joined = Vec::with_capacity(bytes.len() + 1);
                    joined.extend(odd.take());
                    joined.extend_from_slice(&bytes);
                    if joined.len() % 2 == 1 {
                        odd = joined.pop();
                    }
                    let samples: Vec<i16> = joined
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .map(|b| i16::from_le_bytes(*b))
                        .collect();
                    count += samples.len();
                    if !samples.is_empty() {
                        on_samples(&samples);
                    }
                },
            )?;
            if status == 200 && head.status.is_none_or(|s| s == 200) {
                return Ok(count);
            }
            let error = Self::error_of(status, &error);
            // A voice this account does not have: fall back to a standard one.
            if attempt == 0 && status == 400 && format!("{error}").to_lowercase().contains("voice")
            {
                voice = "alloy".into();
                continue;
            }
            return Err(error);
        }
        Err(AiError::Parse("no speech".into()))
    }
}

impl Ask {
    /// A plain conversation: the turns as messages.
    pub fn conversation(instructions: &str, turns: &[Turn]) -> Ask {
        Ask {
            instructions: instructions.to_string(),
            input: turns
                .iter()
                .map(|t| json!({"role": t.role, "content": t.text}))
                .collect(),
            max_output_tokens: 400,
            timeout: Duration::from_secs(40),
            ..Default::default()
        }
    }
}

fn has_image(item: &Value) -> bool {
    item["content"]
        .as_array()
        .is_some_and(|parts| parts.iter().any(|p| p["type"] == "input_image"))
}

fn without_images(item: &Value) -> Value {
    let mut item = item.clone();
    if let Some(parts) = item["content"].as_array_mut() {
        parts.retain(|p| p["type"] != "input_image");
    }
    item
}

/// The answer's items as they go back in, with the outputs of the calls it
/// asked for: what the model said, the calls, and its reasoning when it is
/// carried along (encrypted).
pub fn follow_up(answer: &Answer) -> Vec<Value> {
    let mut out = Vec::new();
    for item in &answer.items {
        match item["type"].as_str() {
            Some("function_call") => out.push(json!({
                "type": "function_call",
                "call_id": item["call_id"],
                "name": item["name"],
                "arguments": item["arguments"],
            })),
            Some("message") => {
                let text: String = item["content"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|p| p["text"].as_str())
                    .collect();
                if !text.is_empty() {
                    out.push(json!({"role": "assistant", "content": text}));
                }
            }
            Some("reasoning") if item.get("encrypted_content").is_some_and(|e| e.is_string()) => {
                out.push(item.clone())
            }
            _ => {}
        }
    }
    out
}

/// A whole Responses API answer (the JSON of a response).
pub fn answer_of(response: &Value) -> Answer {
    let mut answer = Answer::default();
    for item in response["output"].as_array().into_iter().flatten() {
        match item["type"].as_str() {
            Some("message") => {
                for part in item["content"].as_array().into_iter().flatten() {
                    if matches!(part["type"].as_str(), Some("output_text" | "refusal"))
                        && let Some(t) = part["text"].as_str().or(part["refusal"].as_str())
                    {
                        answer.text.push_str(t);
                    }
                }
            }
            Some("function_call") => answer.calls.push(Call {
                call_id: item["call_id"].as_str().unwrap_or_default().to_string(),
                name: item["name"].as_str().unwrap_or_default().to_string(),
                arguments: item["arguments"].as_str().unwrap_or("{}").to_string(),
            }),
            _ => {}
        }
        answer.items.push(item.clone());
    }
    if answer.text.trim().is_empty()
        && let Some(t) = response["output_text"].as_str()
    {
        answer.text = t.to_string();
    }
    answer.text = answer.text.trim().to_string();
    answer
}

/// One thing a stream said.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    Text(String),
    Searching,
}

/// A Responses API answer as it arrives: the events of a stream
/// (server-sent events, one `data:` line each), or a whole JSON answer kept
/// as it is.
#[derive(Default)]
pub struct EventStream {
    /// The bytes received (up to a limit), for an answer that is not a stream.
    pub raw: Vec<u8>,
    /// The reply's text so far.
    pub text: String,
    /// Output items as they completed.
    pub items: Vec<Value>,
    /// Functions the model asked for.
    pub calls: Vec<Call>,
    /// The whole response, from the stream's last event.
    pub completed: Option<Value>,
    /// An error reported in the stream.
    pub error: Option<String>,
    searching: bool,
    line: Vec<u8>,
}

impl EventStream {
    fn keep(&mut self, chunk: &[u8]) {
        if self.raw.len() < (16 << 20) {
            self.raw.extend_from_slice(chunk);
        }
    }

    /// Bytes as they arrive. Returns what the lines they completed said.
    pub fn feed(&mut self, chunk: &[u8]) -> Vec<Event> {
        self.keep(chunk);
        let mut events = Vec::new();
        for &byte in chunk {
            if byte == b'\n' {
                let line = std::mem::take(&mut self.line);
                if let Some(event) = self.event(&line) {
                    events.push(event);
                }
            } else {
                self.line.push(byte);
            }
        }
        events
    }

    fn event(&mut self, line: &[u8]) -> Option<Event> {
        let line = String::from_utf8_lossy(line);
        let data = line
            .trim_end_matches('\r')
            .strip_prefix("data:")?
            .trim_start();
        let event: Value = serde_json::from_str(data).ok()?;
        let kind = event["type"].as_str()?;
        match kind {
            "response.output_text.delta" | "response.refusal.delta" => {
                let piece = event["delta"].as_str()?.to_string();
                self.text.push_str(&piece);
                Some(Event::Text(piece))
            }
            "response.output_item.done" => {
                let item = event["item"].clone();
                if item["type"] == "function_call" {
                    self.calls.push(Call {
                        call_id: item["call_id"].as_str().unwrap_or_default().to_string(),
                        name: item["name"].as_str().unwrap_or_default().to_string(),
                        arguments: item["arguments"].as_str().unwrap_or("{}").to_string(),
                    });
                }
                self.items.push(item);
                None
            }
            "response.completed" | "response.incomplete" => {
                self.completed = Some(event["response"].clone());
                None
            }
            "response.failed" => {
                self.error = Some(
                    event["response"]["error"]["message"]
                        .as_str()
                        .unwrap_or("the reply failed")
                        .to_string(),
                );
                None
            }
            "error" => {
                self.error = Some(
                    event["message"]
                        .as_str()
                        .or(event["error"]["message"].as_str())
                        .unwrap_or("the reply failed")
                        .to_string(),
                );
                None
            }
            k if k.starts_with("response.web_search_call.") && !self.searching => {
                self.searching = true;
                Some(Event::Searching)
            }
            _ => None,
        }
    }
}

/// The text of a Responses API answer: every `output_text` of every
/// `message` in `output`.
pub fn output_text(raw: &[u8]) -> Result<String, AiError> {
    let value: Value = serde_json::from_slice(raw).map_err(|e| AiError::Parse(e.to_string()))?;
    let mut text = String::new();
    for item in value["output"].as_array().into_iter().flatten() {
        if item["type"] != "message" {
            continue;
        }
        for part in item["content"].as_array().into_iter().flatten() {
            if part["type"] == "output_text"
                && let Some(t) = part["text"].as_str()
            {
                text.push_str(t);
            }
        }
    }
    if text.trim().is_empty() {
        // Some answers carry only the convenience field.
        if let Some(t) = value["output_text"].as_str() {
            return Ok(t.trim().to_string());
        }
        return Err(AiError::Parse("the answer had no text".into()));
    }
    Ok(text.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_text_out_of_a_responses_answer() {
        let raw = br#"{"id":"r1","output":[{"type":"reasoning","summary":[]},{"type":"message","role":"assistant","content":[{"type":"output_text","text":"Hey! HP looks great."}]}]}"#;
        assert_eq!(output_text(raw).unwrap(), "Hey! HP looks great.");
        assert!(output_text(br#"{"output":[]}"#).is_err());
        assert!(output_text(b"not json").is_err());
    }

    #[test]
    fn a_stream_is_read_whatever_pieces_it_arrives_in() {
        let stream = "event: response.created\ndata: {\"type\":\"response.created\"}\n\n\
event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"delta\":\"Hey! \"}\n\n\
event: response.output_text.delta\r\ndata: {\"type\":\"response.output_text.delta\",\"delta\":\"עלית רמה 🎉\"}\r\n\r\n\
event: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{}}\n\n";
        for size in [1, 2, 5, 64, 4096] {
            let mut events = EventStream::default();
            let mut pieces = Vec::new();
            for chunk in stream.as_bytes().chunks(size) {
                pieces.extend(events.feed(chunk));
            }
            assert_eq!(
                pieces,
                [
                    Event::Text("Hey! ".into()),
                    Event::Text("עלית רמה 🎉".into())
                ],
                "{size}"
            );
            assert_eq!(events.text, "Hey! עלית רמה 🎉");
            assert!(events.error.is_none());
        }
        let mut failed = EventStream::default();
        failed.feed(b"data: {\"type\":\"response.failed\",\"response\":{\"error\":{\"message\":\"server busy\"}}}\n");
        assert_eq!(failed.error.as_deref(), Some("server busy"));
    }
}
