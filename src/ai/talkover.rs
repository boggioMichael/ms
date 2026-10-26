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
//! word said over its voice ("די", "stop", "שקט") was lost: by the time
//! recognition was on again it was over, the PC heard no words, and the
//! clip played on. The microphone did hear it, and it reaches the PC all
//! the time; the few seconds around the loud moment are written down here,
//! to tell the player's words from a noise (its own echo, the game, a
//! laugh). Only those seconds go to OpenAI, and only when a loud sound
//! paused its voice on the phone.

use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

use super::OpenAi;
use super::live::TRANSCRIBE_MODEL;

/// The longest the transcriber is given: the clip waits paused meanwhile.
const TIMEOUT: Duration = Duration::from_secs(6);

/// A loud moment over its voice, written down.
#[derive(Debug, Clone, PartialEq)]
pub struct Written {
    /// The moment (the PC heard of it then).
    pub at: Instant,
    /// What was said ("" for nothing), or why it couldn't be written down.
    pub words: Result<String, String>,
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
                let words = openai
                    .transcribe(TRANSCRIBE_MODEL, &wav, language.as_deref(), TIMEOUT)
                    .map(|text| said(&text))
                    .map_err(|e| e.detail());
                let _ = tx.send(Written { at, words });
            });
    }
}

/// What transcribers write for a sound with no words in it: nobody said it.
const NOTHING_SAID: &[&str] = &[
    "thank you",
    "thanks",
    "thank you for watching",
    "thanks for watching",
    "you",
    "bye",
    "subtitles",
    "תודה",
    "תודה רבה",
    "תודה שצפיתם",
    "תודה על הצפייה",
    "כתוביות",
    "תרגום",
];

/// `text` as written down, less what a transcriber writes for a sound
/// with no words in it ("Thank you.", "תודה רבה."): then nothing.
pub fn said(text: &str) -> String {
    let normal = crate::companion::commands::normalize(text);
    if NOTHING_SAID.contains(&normal.as_str()) {
        return String::new();
    }
    text.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_a_transcriber_writes_for_a_noise_is_nothing() {
        assert_eq!(said(" די! "), "די!");
        assert_eq!(said("Thank you."), "");
        assert_eq!(said("תודה רבה."), "");
        assert_eq!(said("תודה, כמה HP יש לי?"), "תודה, כמה HP יש לי?");
        assert_eq!(said(""), "");
    }
}
