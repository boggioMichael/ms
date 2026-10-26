//! What the player said over MapleSyrup's voice on the phone, written down
//! from the phone's own microphone.
//!
//! ```text
//!   phone: a clip plays, its speech recognition is off (it would write down
//!          its own voice) — "די!" over it: loud ─▶ the clip pauses ─▶ /api/interrupt
//!   PC:    the phone's microphone (echo-cancelled, sent all along) around
//!          that moment ─▶ Listener: OpenAI's transcriber ─▶ "די"
//!          ─▶ the player's words in it? stop for good : the clip goes on (go_on)
//! ```
//!
//! While MapleSyrup speaks on the phone, the phone does not listen, so a
//! word said over its voice was lost: by the time
//! recognition was on again it was over, the PC heard no words, and the
//! clip played on. The microphone did hear it, and it reaches the PC all
//! the time; the few seconds around the loud moment are written down here,
//! to tell the player's words from a noise (its own echo, the game, a
//! laugh). Only those seconds go to OpenAI, and only when a loud sound
//! paused its voice on the phone. What the transcriber wrote while it was
//! guessing (as it does at a sound with no words in it) is nothing said.

use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

use super::OpenAi;
use super::live::TRANSCRIBE_MODEL;
use super::openai::Transcript;

/// The longest the transcriber is given: the clip waits paused meanwhile.
const TIMEOUT: Duration = Duration::from_secs(6);

/// Below this mean log-probability of the tokens it wrote (about one in
/// three per token), the transcriber was guessing — as it does at a sound
/// with no words in it — and nothing was said.
const GUESSING: f64 = -1.0;

/// A loud moment over its voice, written down.
#[derive(Debug, Clone, PartialEq)]
pub struct Written {
    /// The moment (the PC heard of it then).
    pub at: Instant,
    /// What was said ("" for nothing), or why it couldn't be written down.
    pub words: Result<String, String>,
    /// What the transcriber wrote while it was guessing (not taken as
    /// said), for the log.
    pub guessed: Option<String>,
}

/// What was said, of what the transcriber wrote: nothing when it was
/// guessing (then what it guessed, second).
fn said(transcript: Transcript) -> (String, Option<String>) {
    match transcript.logprob {
        Some(sure) if sure < GUESSING && !transcript.text.is_empty() => {
            (String::new(), Some(transcript.text))
        }
        _ => (transcript.text, None),
    }
}

/// Writes down, in the background, what the phone's microphone heard
/// around a loud moment over MapleSyrup's voice.
pub struct Listener {
    openai: Arc<OpenAi>,
    tx: Sender<Written>,
}

impl Listener {
    pub fn new(openai: Arc<OpenAi>) -> (Listener, Receiver<Written>) {
        let (tx, rx) = channel();
        (Listener { openai, tx }, rx)
    }

    /// Write down `samples` (mono, `rate` Hz), heard around the moment
    /// `at`, in `language` (a locale such as `he-IL`) when known.
    pub fn listen(&self, at: Instant, samples: Vec<i16>, rate: u32, language: Option<String>) {
        let (openai, tx) = (Arc::clone(&self.openai), self.tx.clone());
        let _ = std::thread::Builder::new()
            .name("talk-over".into())
            .spawn(move || {
                let wav = super::wav_bytes(&samples, rate);
                let (words, guessed) =
                    match openai.transcribe(TRANSCRIBE_MODEL, &wav, language.as_deref(), TIMEOUT) {
                        Ok(transcript) => {
                            let (words, guessed) = said(transcript);
                            (Ok(words), guessed)
                        }
                        Err(e) => (Err(e.detail()), None),
                    };
                let _ = tx.send(Written { at, words, guessed });
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_the_transcriber_wrote_while_guessing_is_nothing_said() {
        let written = |text: &str, logprob| Transcript {
            text: text.into(),
            logprob,
        };
        // Sure of it: said.
        assert_eq!(
            said(written("words it heard", Some(-0.05))),
            ("words it heard".into(), None)
        );
        // Guessing, as at a sound with no words: nothing said (and what it
        // guessed, for the log).
        assert_eq!(
            said(written("words it guessed", Some(-1.6))),
            (String::new(), Some("words it guessed".into()))
        );
        // A transcriber that doesn't say how sure it was is taken at its
        // word; nothing written is nothing either way.
        assert_eq!(
            said(written("words it heard", None)),
            ("words it heard".into(), None)
        );
        assert_eq!(said(written("", Some(-3.0))), (String::new(), None));
    }
}
