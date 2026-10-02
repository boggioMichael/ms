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
use std::sync::Mutex;
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

/// The chosen model, and whether it takes `reasoning.effort`.
#[derive(Debug, Clone)]
struct Choice {
    model: String,
    reasoning: bool,
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
            models: match model {
                Some(m) => vec![m.to_string()],
                None => CHAT_MODELS.iter().map(|m| m.to_string()).collect(),
            },
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
        let status = self.call_with(method, path, body, timeout, &mut |chunk| {
            all.extend_from_slice(chunk)
        })?;
        Ok((status, all))
    }

    /// One HTTP request, its answer handed to `on_body` piece by piece as it
    /// arrives (curl's `--no-buffer`). Returns the HTTP status.
    fn call_with(
        &self,
        method: &str,
        path: &str,
        body: Option<&Value>,
        timeout: Duration,
        on_body: &mut dyn FnMut(&[u8]),
    ) -> Result<u16, AiError> {
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
        if let Some(mut stdout) = child.stdout.take() {
            let mut buffer = [0u8; 8192];
            loop {
                match stdout.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(n) => on_body(&buffer[..n]),
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(_) => break,
                }
            }
        }
        let stderr_bytes = error_reader.join().unwrap_or_default();
        let _ = child.wait();
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
        self.converse(instructions, turns, None)
    }

    /// The model's reply, streamed: `on_text` gets each piece as it is
    /// written. Returns the whole reply.
    pub fn respond_stream(
        &self,
        instructions: &str,
        turns: &[Turn],
        on_text: &mut dyn FnMut(&str),
    ) -> Result<String, AiError> {
        self.converse(instructions, turns, Some(on_text))
    }

    fn converse(
        &self,
        instructions: &str,
        turns: &[Turn],
        mut on_text: Option<&mut dyn FnMut(&str)>,
    ) -> Result<String, AiError> {
        let input: Vec<Value> = turns
            .iter()
            .map(|t| json!({"role": t.role, "content": t.text}))
            .collect();
        let known = self.choice.lock().ok().and_then(|c| c.clone());
        let candidates: Vec<Choice> = match known {
            Some(choice) => vec![choice],
            None => self
                .models
                .iter()
                .map(|m| Choice {
                    model: m.clone(),
                    reasoning: true,
                })
                .collect(),
        };
        let mut last = AiError::Parse("no model to try".into());
        for mut choice in candidates {
            loop {
                let mut body = json!({
                    "model": choice.model,
                    "instructions": instructions,
                    "input": input,
                    "max_output_tokens": 400,
                    "store": false,
                });
                if choice.reasoning {
                    body["reasoning"] = json!({"effort": "none"});
                }
                let mut events = EventStream::default();
                let status = match on_text.as_mut() {
                    Some(on_text) => {
                        body["stream"] = json!(true);
                        self.call_with(
                            "POST",
                            "/responses",
                            Some(&body),
                            Duration::from_secs(40),
                            &mut |chunk| {
                                for piece in events.feed(chunk) {
                                    on_text(&piece);
                                }
                            },
                        )?
                    }
                    None => self.call_with(
                        "POST",
                        "/responses",
                        Some(&body),
                        Duration::from_secs(40),
                        &mut |chunk| events.keep(chunk),
                    )?,
                };
                let raw = &events.raw;
                if status == 200 {
                    if let Some(message) = events.error {
                        return Err(AiError::Http(500, message));
                    }
                    let text = if events.text.trim().is_empty() {
                        // A whole answer (not streamed, or buffered on the way).
                        let text = output_text(raw)?;
                        if let Some(on_text) = on_text.as_mut() {
                            on_text(&text);
                        }
                        text
                    } else {
                        events.text.trim().to_string()
                    };
                    if let Ok(mut slot) = self.choice.lock() {
                        *slot = Some(choice.clone());
                    }
                    return Ok(text);
                }
                let error = Self::error_of(status, raw);
                let message = match &error {
                    AiError::Http(_, m) => m.to_ascii_lowercase(),
                    _ => String::new(),
                };
                // An older model without reasoning efforts: ask again without.
                if status == 400
                    && choice.reasoning
                    && (message.contains("reasoning") || message.contains("effort"))
                {
                    choice.reasoning = false;
                    continue;
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
        let mut voice = self.voice.clone();
        for attempt in 0..2 {
            let body = json!({
                "model": SPEECH_MODEL,
                "voice": voice,
                "input": text,
                "instructions": style,
                "response_format": "pcm",
            });
            let (status, raw) = self.call(
                "POST",
                "/audio/speech",
                Some(&body),
                Duration::from_secs(40),
            )?;
            if status == 200 {
                return Ok(raw
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|b| i16::from_le_bytes(*b))
                    .collect());
            }
            let error = Self::error_of(status, &raw);
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

/// A Responses API answer as it arrives: the events of a stream
/// (server-sent events, one `data:` line each), or a whole JSON answer kept
/// as it is.
#[derive(Default)]
pub struct EventStream {
    /// The bytes received (up to a limit), for an answer that is not a stream.
    pub raw: Vec<u8>,
    /// The reply's text so far.
    pub text: String,
    /// An error reported in the stream.
    pub error: Option<String>,
    line: Vec<u8>,
}

impl EventStream {
    fn keep(&mut self, chunk: &[u8]) {
        if self.raw.len() < (1 << 20) {
            self.raw.extend_from_slice(chunk);
        }
    }

    /// Bytes as they arrive. Returns the pieces of text in the lines they
    /// completed.
    pub fn feed(&mut self, chunk: &[u8]) -> Vec<String> {
        self.keep(chunk);
        let mut pieces = Vec::new();
        for &byte in chunk {
            if byte == b'\n' {
                let line = std::mem::take(&mut self.line);
                if let Some(piece) = self.event(&line) {
                    pieces.push(piece);
                }
            } else {
                self.line.push(byte);
            }
        }
        pieces
    }

    fn event(&mut self, line: &[u8]) -> Option<String> {
        let line = String::from_utf8_lossy(line);
        let data = line
            .trim_end_matches('\r')
            .strip_prefix("data:")?
            .trim_start();
        let event: Value = serde_json::from_str(data).ok()?;
        match event["type"].as_str()? {
            "response.output_text.delta" | "response.refusal.delta" => {
                let piece = event["delta"].as_str()?.to_string();
                self.text.push_str(&piece);
                Some(piece)
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
            assert_eq!(pieces, ["Hey! ", "עלית רמה 🎉"], "{size}");
            assert_eq!(events.text, "Hey! עלית רמה 🎉");
            assert!(events.error.is_none());
        }
        let mut failed = EventStream::default();
        failed.feed(b"data: {\"type\":\"response.failed\",\"response\":{\"error\":{\"message\":\"server busy\"}}}\n");
        assert_eq!(failed.error.as_deref(), Some("server busy"));
    }
}
