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
pub mod openai;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

pub use brain::Brain;
pub use openai::{AiError, OpenAi};

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

/// Something for the worker to do.
pub enum Job {
    /// Answer what the player said, given what is on screen.
    Converse {
        heard: String,
        snapshot: String,
        speak: bool,
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

/// Start the worker thread.
pub fn spawn(openai: OpenAi, mut brain: Brain) -> Worker {
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
                for job in queue {
                    match job {
                        Job::Converse {
                            heard: h,
                            snapshot: s,
                            speak: sp,
                        } => {
                            heard.push(h);
                            snapshot = s;
                            speak |= sp;
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
                        &openai, &mut brain, heard, &snapshot, speak, &tx, &busy_flag,
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

/// Answer the player. The reply is streamed and cut into sentences; each
/// sentence is turned into speech (on a second thread) as soon as it is
/// complete, so the first is heard while the rest is still being written.
fn converse(
    openai: &OpenAi,
    brain: &mut Brain,
    heard: String,
    snapshot: &str,
    speak: bool,
    tx: &Sender<Done>,
    busy: &AtomicBool,
) {
    let started = Instant::now();
    let instructions = brain.instructions(snapshot);
    let turns = brain.turns();
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
        let result = openai.respond_stream(&instructions, &turns, &mut |piece| {
            if speak {
                for sentence in sentences.push(piece) {
                    let _ = lines.send(brain::for_speech(&sentence));
                }
            }
        });
        match result {
            Ok(reply) if brain::is_silent(&reply) => {
                let _ = tx.send(Done::Silent { heard });
            }
            Ok(reply) => {
                if speak && let Some(rest) = sentences.finish() {
                    let _ = lines.send(brain::for_speech(&rest));
                }
                let text = brain::for_speech(&reply);
                brain.said(&text);
                let _ = tx.send(Done::Reply {
                    heard,
                    text,
                    took: started.elapsed(),
                });
            }
            Err(error) => {
                let _ = tx.send(Done::Failed {
                    heard: Some(heard),
                    error,
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
