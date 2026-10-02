//! Talking like a person: replies come from an OpenAI model that is given
//! what the vision engine sees, and are spoken in OpenAI's natural voice.
//!
//! ```text
//!   phone: "can you see my game?"
//!      │
//!      ▼            snapshot of the game (HP, MP, EXP, level, EXP/h)
//!   Worker: respond_stream(persona + snapshot + conversation)
//!      │  the reply arrives a few words at a time and is cut into sentences
//!      ├──▶ each sentence ──▶ speech ──▶ the PC's speakers (the game
//!      │                                  ducked) and/or the phone, in turn
//!      ▼
//!   Text ──▶ phone screen, console (once the reply is complete)
//! ```
//!
//! The first sentence is spoken while the rest is still being written.
//!
//! Without a key, or when OpenAI cannot be reached, MapleSyrup falls back
//! to its own answers and the Windows voice.

pub mod brain;
pub mod images;
pub mod openai;
pub mod teaching;
pub mod tools;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use image::RgbaImage;
use serde_json::{Value, json};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

pub use brain::Brain;
pub use images::NBox;
pub use openai::{AiError, OpenAi};
pub use tools::{Effect, Toolbox};

use openai::{Ask, Piece};

/// The API key's file in MapleSyrup's settings folder.
pub fn key_file(settings: &Path) -> PathBuf {
    settings.join("openai-key.txt")
}

/// Whether `text` looks like an OpenAI API key.
pub fn looks_like_key(text: &str) -> bool {
    let t = clean_key(text);
    t.starts_with("sk-") && t.len() >= 20 && !t.contains(char::is_whitespace)
}

/// A key as pasted or saved: without the byte-order mark Notepad may put in
/// front, quotes around it, or lines after it.
pub fn clean_key(text: &str) -> &str {
    text.trim_start_matches('\u{feff}')
        .trim()
        .lines()
        .next()
        .unwrap_or("")
        .trim()
        .trim_matches(|c| c == '"' || c == '\'')
        .trim()
}

/// The key from `OPENAI_API_KEY`, the settings folder, or `openai-key.txt`
/// next to MapleSyrup. A key found next to MapleSyrup is moved into the
/// settings folder (which no desktop sync or screen share shows).
pub fn load_key(settings: &Path) -> Option<String> {
    if let Ok(key) = std::env::var("OPENAI_API_KEY")
        && looks_like_key(&key)
    {
        return Some(clean_key(&key).to_string());
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        let beside = dir.join("openai-key.txt");
        if let Ok(key) = std::fs::read_to_string(&beside)
            && looks_like_key(&key)
        {
            let key = clean_key(&key).to_string();
            if save_key(settings, &key).is_ok() {
                let _ = std::fs::remove_file(&beside);
            }
            return Some(key);
        }
    }
    std::fs::read_to_string(key_file(settings))
        .ok()
        .filter(|k| looks_like_key(k))
        .map(|k| clean_key(&k).to_string())
}

pub fn save_key(settings: &Path, key: &str) -> std::io::Result<()> {
    std::fs::create_dir_all(settings)?;
    std::fs::write(key_file(settings), clean_key(key))
}

/// What the model sees with a sentence: the frame the player was looking
/// at, and where its HUD is (when known).
#[derive(Clone)]
pub struct Eyes {
    pub frame: Arc<RgbaImage>,
    pub status: Option<NBox>,
}

impl Eyes {
    /// The pictures for the model: the whole frame with rulers (to point at
    /// things), and the HUD at full size (to read small numbers).
    pub fn pictures(&self) -> Vec<Value> {
        let frame = images::with_rulers(&images::fit(&self.frame, 1280, 800));
        let status = self
            .status
            .map(|s| s.grown(0.02, 0.3))
            .unwrap_or(NBox::new(0.0, 0.78, 1.0, 1.0));
        let mut hud = images::crop(&self.frame, &status);
        if hud.height() < 90 {
            hud = images::enlarged(&hud, 2);
        }
        let hud = images::fit(&hud, 1600, 400);
        vec![
            images::input_image(images::jpeg_url(&frame, 80), "high"),
            images::input_image(images::png_url(&hud), "high"),
        ]
    }
}

/// Something for the worker to do.
pub enum Job {
    /// Answer what the player said, given what is on screen.
    Converse {
        heard: String,
        snapshot: String,
        speak: bool,
        /// The screen, when the game is in view.
        eyes: Option<Eyes>,
    },
    /// Say this line in the natural voice (a warning, a quick reply).
    Speak { text: String },
}

/// What the worker did.
pub enum Done {
    /// The reply's text (its audio follows as `Audio`).
    Reply {
        heard: String,
        text: String,
        took: Duration,
    },
    /// The model judged the sentence was not for it.
    Silent { heard: String },
    /// Speech for a line (a reply comes a sentence at a time): 24 kHz mono
    /// samples. `after`: since the player's sentence came in (or since the
    /// line was asked for); `first`: the first sentence of a reply.
    Audio {
        text: String,
        samples: Vec<i16>,
        after: Duration,
        first: bool,
    },
    Failed {
        heard: Option<String>,
        error: AiError,
    },
    /// A tool changed something: learned, forgot or corrected (a line for
    /// the log and the phone).
    Noted { line: String },
}

pub struct Worker {
    jobs: Sender<Job>,
    pub done: Receiver<Done>,
    busy: Arc<AtomicBool>,
    pub model: Arc<std::sync::Mutex<Option<String>>>,
}

impl Worker {
    pub fn send(&self, job: Job) {
        let _ = self.jobs.send(job);
    }

    /// Whether it is working on something (the phone shows "thinking").
    pub fn busy(&self) -> bool {
        self.busy.load(Ordering::Relaxed)
    }
}

/// Start the worker thread (no tools).
pub fn spawn(openai: OpenAi, brain: Brain) -> Worker {
    spawn_with(openai, brain, None)
}

/// Start the worker thread, with the tools the model may use.
pub fn spawn_with(openai: OpenAi, mut brain: Brain, toolbox: Option<Toolbox>) -> Worker {
    let (jobs, rx) = channel::<Job>();
    let (tx, done) = channel::<Done>();
    let busy = Arc::new(AtomicBool::new(false));
    let model = Arc::new(std::sync::Mutex::new(None));
    let (busy_flag, model_slot) = (Arc::clone(&busy), Arc::clone(&model));
    let _ = std::thread::Builder::new()
        .name("ai".into())
        .spawn(move || {
            while let Ok(first) = rx.recv() {
                busy_flag.store(true, Ordering::Relaxed);
                // Sentences that arrived while it was busy are answered together.
                let mut queue = vec![first];
                while let Ok(more) = rx.try_recv() {
                    queue.push(more);
                }
                let mut heard = Vec::new();
                let mut snapshot = String::new();
                let mut speak = false;
                let mut eyes = None;
                for job in queue {
                    match job {
                        Job::Converse {
                            heard: h,
                            snapshot: s,
                            speak: sp,
                            eyes: e,
                        } => {
                            heard.push(h);
                            snapshot = s;
                            speak |= sp;
                            if e.is_some() {
                                eyes = e;
                            }
                        }
                        Job::Speak { text } => {
                            let asked = Instant::now();
                            match openai.speech(&text, brain::VOICE_STYLE) {
                                Ok(samples) => {
                                    let _ = tx.send(Done::Audio {
                                        text,
                                        samples,
                                        after: asked.elapsed(),
                                        first: false,
                                    });
                                }
                                Err(error) => {
                                    let _ = tx.send(Done::Failed { heard: None, error });
                                }
                            }
                        }
                    }
                }
                if !heard.is_empty() {
                    let heard = heard.join(" ");
                    brain.heard(&heard);
                    converse(
                        &openai,
                        toolbox.as_ref(),
                        &mut brain,
                        heard,
                        &snapshot,
                        eyes.as_ref(),
                        speak,
                        &tx,
                        &busy_flag,
                    );
                    if let Ok(mut slot) = model_slot.lock() {
                        *slot = openai.model();
                    }
                }
                busy_flag.store(false, Ordering::Relaxed);
            }
        });
    Worker {
        jobs,
        done,
        busy,
        model,
    }
}

/// How the model is told it can see the game.
const EYES_GUIDE: &str = "\n\nYou can see the game: the first picture is the whole game window as it is now, with \
rulers on its edges (0 to 1000 across and down) for pointing at things; the second is the HUD at full size, for \
reading small numbers. Use what you see, like a friend looking at the same screen. If the numbers listed above \
disagree with the pictures, trust the pictures (and say so if it matters).";

/// How the model is told about its tools.
const TOOLS_GUIDE: &str = "\n\nYou get better the more the player teaches you:
- When the player shows or tells you what something on screen is (\"this is...\", \"that's my...\", \"see that? it's...\") or asks you to watch for something, call learn_thing with a tight box around it in the first picture's 0-1000 coordinates. If they want a heads-up (\"tell me when a rune shows up\", \"warn me when the boss is under 20%\"), set alert, threshold and say (what you'll say then, in their language).
- When the player says a value you have is wrong (their level, HP, MP, EXP, map, name, job), call correct_reading.
- When the player tells you something about themselves or their game worth keeping (their class, a key binding, a goal), or asks you to remember something, call remember_fact.
- forget_thing when asked to forget something you learned; look_closer to read small text or details you can't make out.
After using a tool, confirm briefly in your own words.";

/// How the model is told it can search the web.
const WEB_GUIDE: &str = "\n- Search the web before answering any MapleStory question you aren't sure of (how to get somewhere, \
boss or level requirements, job advancements, key bindings, quests, events, training spots for their level): the \
current global version (GMS) changes often. Prefer maplestory.nexon.net and maplestorywiki.net, and answer in a \
sentence or two.";

/// What to say while the web is searched, in the player's language.
fn searching_line(heard: &str) -> &'static str {
    if heard
        .chars()
        .any(|c| ('\u{0590}'..='\u{05FF}').contains(&c))
    {
        "רגע, אני בודק."
    } else {
        "Let me check that real quick."
    }
}

/// The conversation as input items, the screen with the player's last
/// sentence.
fn input_of(turns: &[openai::Turn], eyes: Option<&Eyes>) -> Vec<Value> {
    let last_user = turns.iter().rposition(|t| t.role == "user");
    turns
        .iter()
        .enumerate()
        .map(|(i, t)| match (eyes, Some(i) == last_user) {
            (Some(eyes), true) => {
                let mut content = vec![json!({"type": "input_text", "text": t.text})];
                content.extend(eyes.pictures());
                json!({"role": "user", "content": content})
            }
            _ => json!({"role": t.role, "content": t.text}),
        })
        .collect()
}

/// Answer the player. The reply is streamed and cut into sentences; each
/// sentence is turned into speech (on a second thread) as soon as it is
/// complete, so the first is heard while the rest is still being written.
/// When the model calls tools, they are run and their results handed back
/// for it to go on, up to a few rounds.
#[allow(clippy::too_many_arguments)]
fn converse(
    openai: &OpenAi,
    toolbox: Option<&Toolbox>,
    brain: &mut Brain,
    heard: String,
    snapshot: &str,
    eyes: Option<&Eyes>,
    speak: bool,
    tx: &Sender<Done>,
    busy: &AtomicBool,
) {
    let started = Instant::now();
    let mut instructions = brain.instructions(snapshot);
    if eyes.is_some() {
        instructions.push_str(EYES_GUIDE);
    }
    if let Some(toolbox) = toolbox {
        instructions.push_str(TOOLS_GUIDE);
        if toolbox.web {
            instructions.push_str(WEB_GUIDE);
        }
    }
    let mut input = input_of(&brain.turns(), eyes);
    let tools = toolbox.map(|t| t.definitions()).unwrap_or_default();
    std::thread::scope(|scope| {
        let (lines, to_say) = channel::<String>();
        if speak {
            let tx = tx.clone();
            scope.spawn(move || {
                let mut first = true;
                for text in to_say {
                    match openai.speech(&text, brain::VOICE_STYLE) {
                        Ok(samples) => {
                            let _ = tx.send(Done::Audio {
                                text,
                                samples,
                                after: started.elapsed(),
                                first,
                            });
                            first = false;
                        }
                        Err(error) => {
                            let _ = tx.send(Done::Failed { heard: None, error });
                            break;
                        }
                    }
                }
            });
        }
        let mut sentences = brain::Sentences::default();
        let mut said = String::new();
        let mut failed = None;
        for round in 0..4 {
            let ask = Ask {
                instructions: instructions.clone(),
                input: input.clone(),
                tools: if round < 3 { tools.clone() } else { Vec::new() },
                max_output_tokens: 500,
                timeout: Duration::from_secs(60),
                ..Default::default()
            };
            let answer = openai.ask(
                &ask,
                Some(&mut |piece| match piece {
                    Piece::Text(t) => {
                        said.push_str(t);
                        if speak {
                            for sentence in sentences.push(t) {
                                let _ = lines.send(brain::for_speech(&sentence));
                            }
                        }
                    }
                    Piece::Searching => {
                        if speak && said.trim().is_empty() {
                            let _ = lines.send(searching_line(&heard).to_string());
                        }
                    }
                }),
            );
            let answer = match answer {
                Ok(answer) => answer,
                Err(error) => {
                    failed = Some(error);
                    break;
                }
            };
            if answer.calls.is_empty() {
                break;
            }
            // What was said so far ends a sentence before the next round.
            if !said.trim().is_empty() && !said.ends_with(' ') {
                said.push(' ');
                if speak {
                    for sentence in sentences.push(" ") {
                        let _ = lines.send(brain::for_speech(&sentence));
                    }
                }
            }
            let mut outputs = Vec::new();
            for call in &answer.calls {
                let (output, effect) = match toolbox {
                    Some(toolbox) => toolbox.run(call, eyes.map(|e| e.frame.as_ref())),
                    None => ("No tools here.".to_string(), None),
                };
                match effect {
                    Some(Effect::Fact(fact)) => {
                        let about = &mut brain.about_player;
                        if !about.is_empty() && !about.ends_with('\n') {
                            about.push('\n');
                        }
                        about.push_str(&format!("- {fact}"));
                        let _ = tx.send(Done::Noted {
                            line: format!("remembered: {fact}"),
                        });
                    }
                    Some(Effect::Note(line)) => {
                        let _ = tx.send(Done::Noted { line });
                    }
                    None => {}
                }
                outputs.push(json!({"type": "function_call_output", "call_id": call.call_id, "output": output}));
            }
            input.extend(openai::follow_up(&answer));
            input.extend(outputs);
        }
        match failed {
            Some(error) => {
                let _ = tx.send(Done::Failed {
                    heard: Some(heard),
                    error,
                });
            }
            None if brain::is_silent(&said) => {
                let _ = tx.send(Done::Silent { heard });
            }
            None => {
                if speak && let Some(rest) = sentences.finish() {
                    let _ = lines.send(brain::for_speech(&rest));
                }
                let text = brain::for_speech(&said);
                brain.said(&text);
                let _ = tx.send(Done::Reply {
                    heard,
                    text,
                    took: started.elapsed(),
                });
            }
        }
        // The words are out; the voice may still be on its last sentences.
        drop(lines);
        busy.store(false, Ordering::Relaxed);
    });
}

/// 16-bit mono samples as a WAV file in memory.
pub fn wav_bytes(samples: &[i16], rate: u32) -> Vec<u8> {
    let data = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data.to_le_bytes());
    for s in samples {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_recognised() {
        assert!(looks_like_key("sk-proj-abcdefghijklmnopqrstuvwxyz0123"));
        assert!(looks_like_key("  sk-abcdefghijklmnopqrstuvwxyz \n"));
        assert!(!looks_like_key("hello"));
        assert!(!looks_like_key("sk- spaced key that is long enough"));
        // Saved by Notepad with a byte-order mark, quoted, or with a note after.
        let key = "sk-proj-abcdefghijklmnopqrstuvwxyz0123";
        for saved in [
            format!("\u{feff}{key}\r\n"),
            format!("\"{key}\""),
            format!("{key}\nmy key from October"),
        ] {
            assert!(looks_like_key(&saved), "{saved:?}");
            assert_eq!(clean_key(&saved), key);
        }
    }

    #[test]
    fn wav_in_memory_has_a_correct_header() {
        let wav = wav_bytes(&[0, 1, -1, 32767], 24_000);
        assert_eq!(wav.len(), 44 + 8);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(u32::from_le_bytes(wav[24..28].try_into().unwrap()), 24_000);
        assert_eq!(u32::from_le_bytes(wav[40..44].try_into().unwrap()), 8);
    }
}
