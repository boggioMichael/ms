//! What the player said over MapleSyrup's voice, written down from the
//! phone's own microphone.
//!
//! ```text
//!   phone: a clip plays, its speech recognition is off (it would write down
//!          its own voice) — "די!" over it: loud ─▶ the clip pauses ─▶ /api/interrupt
//!          (or its voice plays on the PC: louder than that voice here ─▶ /api/interrupt,
//!          and the PC holds its voice)
//!   PC:    the phone's microphone (sent all along) around that moment, from
//!          where it grew louder than its voice (his_part) ─▶ Listener:
//!          OpenAI's transcriber ─▶ "די"
//!          ─▶ the player's words in it? stop for good : its voice goes on (go_on)
//! ```
//!
//! While MapleSyrup speaks on the phone, the phone does not listen, so a
//! word said over its voice was lost: by the time
//! recognition was on again it was over, the PC heard no words, and the
//! clip played on. The microphone did hear it, and it reaches the PC all
//! the time; the moment is written down here, to tell the player's words
//! from a noise (its own echo, the game, a laugh). Over its voice on the
//! PC, which nothing cancels at the phone, the phone's recognition writes
//! that voice down with his, and his one word can't be told from its own
//! there; so the moment is written down the same way — from where his
//! voice grows louder than its own ([`his_part`]), its voice held. Only
//! that part goes to OpenAI, and only when a loud sound came over its
//! voice. What the transcriber wrote while it was guessing (as it does at
//! a sound with no words in it) is nothing said.

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

/// How long before the phone told of a loud moment his voice may have
/// begun: it tells once a few pieces of sound were loud, and the word
/// reaches the PC after that.
pub const ONSET_WITHIN: Duration = Duration::from_millis(1000);

/// How much louder than its own voice (its loudest moments before he could
/// have spoken) his voice is where it begins.
const OVER_ITS_VOICE_DB: f32 = 4.0;

/// How far before his voice grew loud the part written down starts (a word
/// grows loud over its first moments).
const LEAD: Duration = Duration::from_millis(60);

/// The part of `samples` (mono, `rate` Hz; the phone's microphone around a
/// loud moment, the phone telling of it at sample `at`) that is the
/// player's voice, by how loud it is: from a moment before it grows louder
/// than its own voice as heard before he could have spoken (the loudest of
/// it, more than [`ONSET_WITHIN`] before `at`) to the end. Its own voice
/// before him, the middle of one of its lines, is left out: written down
/// with him, its words cut off at the edge came back as words of his
/// ("Zakim" for "Zakum"). With too little of its voice before him to go
/// by, or no part louder than it, from [`ONSET_WITHIN`] before `at`.
pub fn his_part(samples: &[i16], rate: u32, at: usize) -> std::ops::Range<usize> {
    let frame = (rate as usize / 50).max(1);
    let level = |i: usize| {
        let from = (i * frame).min(samples.len());
        crate::phone::audio::rms_db(&samples[from..((i + 1) * frame).min(samples.len())])
    };
    let at = at.min(samples.len());
    let within = (ONSET_WITHIN.as_secs_f64() * rate as f64) as usize;
    let fallback = at.saturating_sub(within)..samples.len();
    let before = at.saturating_sub(within) / frame;
    // (A quarter of a second of its voice, at least.)
    if before < 12 {
        return fallback;
    }
    let mut its_voice: Vec<f32> = (0..before).map(level).collect();
    its_voice.sort_by(f32::total_cmp);
    let loudest = its_voice[its_voice.len() * 9 / 10];
    let frames = samples.len().div_ceil(frame);
    let lead = (LEAD.as_secs_f64() * rate as f64) as usize;
    match (before..frames).find(|&i| level(i) > loudest + OVER_ITS_VOICE_DB) {
        Some(i) => (i * frame).saturating_sub(lead).min(at)..samples.len(),
        None => fallback,
    }
}

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

    /// `seconds` of a voice's worth of sound at 16 kHz: loud for a fifth
    /// of a second, a moment quieter, over and over.
    fn voice(seconds: f64, amplitude: i16) -> Vec<i16> {
        (0..(seconds * 16_000.0) as usize)
            .map(|i| {
                let loud = (i % 4_320) < 3_200;
                let a = if loud { amplitude } else { amplitude / 4 };
                if (i / 20) % 2 == 0 { a } else { -a }
            })
            .collect()
    }

    #[test]
    fn what_is_written_down_of_a_loud_moment_starts_where_his_voice_does() {
        // Its voice, and his louder over it 1.6 s in; the phone tells of it
        // a third of a second later (at 1.93 s), and the window goes on to
        // 2.6 s.
        let mut samples = voice(1.6, 2_000);
        samples.extend(voice(0.3, 12_000));
        samples.extend(voice(0.7, 2_000));
        let at = (1.93 * 16_000.0) as usize;
        let part = his_part(&samples, 16_000, at);
        let begins = part.start as f64 / 16_000.0;
        assert!((1.5..=1.6).contains(&begins), "{begins}");
        assert_eq!(part.end, samples.len());
        // No part louder than its voice (the phone was wrong, or the game):
        // from a second before the moment.
        let flat = voice(2.6, 2_000);
        assert_eq!(his_part(&flat, 16_000, at), at - 16_000..flat.len());
        // Too little of its voice before him to go by: the same.
        assert_eq!(his_part(&samples[..20_000], 16_000, 19_000), 3_000..20_000);
        // Echo cancelled (a clip on the phone): silence, then his voice.
        let mut clean = vec![0i16; (1.6 * 16_000.0) as usize];
        clean.extend(voice(0.5, 3_000));
        let part = his_part(&clean, 16_000, at);
        assert!(
            (1.5..=1.6).contains(&(part.start as f64 / 16_000.0)),
            "{part:?}"
        );
    }

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
