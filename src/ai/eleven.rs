//! ElevenLabs voices: with an ElevenLabs key, MapleSyrup speaks in a voice
//! the player picks on the phone (from the voices on their account), made
//! as it is needed and played as it comes, like OpenAI's voice. OpenAI's
//! voice stands in whenever ElevenLabs can't: a line in a language no
//! model on the account speaks, an error, or no credits left (then it rests
//! a while instead of being asked again for every line).

use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::{Value, json};

use super::openai::{AiError, OpenAi, Stop};

pub const BASE: &str = "https://api.elevenlabs.io/v1";
/// Models tried, quickest first. Eleven v4 Turbo speaks Hebrew and Thai
/// too; the others don't, so a line in those goes to it alone.
pub const MODELS: &[&str] = &[
    "eleven_v4_turbo",
    "eleven_flash_v2_5",
    "eleven_multilingual_v2",
];

/// A voice on the player's account.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Voice {
    pub id: String,
    pub name: String,
    /// A few words about it (accent, age, gender, what it's for).
    pub about: String,
}

pub struct Eleven {
    api: OpenAi,
    /// Models the account can't use, found out as it went.
    skip: Mutex<Vec<&'static str>>,
    /// Models that refused the voice settings: asked without them.
    plain: Mutex<Vec<&'static str>>,
    /// Failing: OpenAI's voice speaks until then.
    rest: Mutex<Option<Instant>>,
    /// Failures in a row.
    failures: AtomicU32,
}

impl Eleven {
    /// `base`: `BASE`, or a stand-in for tests.
    pub fn new(key: &str, base: &str) -> Eleven {
        Eleven {
            api: OpenAi::with_models(key, base, "", Vec::new()).with_key_header("xi-api-key"),
            skip: Mutex::new(Vec::new()),
            plain: Mutex::new(Vec::new()),
            rest: Mutex::new(None),
            failures: AtomicU32::new(0),
        }
    }

    /// The voices on the account, by name.
    pub fn voices(&self) -> Result<Vec<Voice>, AiError> {
        let list = self.api.get_json("/voices")?;
        let mut voices: Vec<Voice> = list["voices"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|v| {
                let labels: Vec<String> = v["labels"]
                    .as_object()
                    .into_iter()
                    .flatten()
                    .filter_map(|(_, l)| l.as_str().map(|l| l.trim().replace('_', " ")))
                    .filter(|l| !l.is_empty())
                    .collect();
                Some(Voice {
                    id: v["voice_id"].as_str()?.to_string(),
                    name: v["name"].as_str()?.trim().to_string(),
                    about: labels.join(", "),
                })
            })
            .collect();
        voices.sort_by_key(|v| v.name.to_lowercase());
        Ok(voices)
    }

    /// Resting after failing (OpenAI's voice speaks meanwhile).
    pub fn resting(&self) -> bool {
        lock(&self.rest).is_some_and(|until| Instant::now() < until)
    }

    /// `text` said in voice `voice` (24 kHz mono), handed to `on_samples` a
    /// piece at a time as it is made. A model the account can't use is
    /// skipped (from then on), and one that won't take the voice settings
    /// is asked without them.
    pub fn speech_stream(
        &self,
        text: &str,
        voice: &str,
        stop: Option<&Stop>,
        on_samples: &mut dyn FnMut(&[i16]),
    ) -> Result<usize, AiError> {
        let result = self.try_models(text, voice, stop, on_samples);
        match &result {
            Ok(_) | Err(AiError::Cancelled) | Err(AiError::Unsupported(_)) => {
                self.failures.store(0, Ordering::Relaxed);
            }
            Err(error) => {
                let failures = self.failures.fetch_add(1, Ordering::Relaxed) + 1;
                let rest = match error {
                    // A key that doesn't work, or no credits left.
                    AiError::Http(401 | 402, _) => Some(Duration::from_secs(600)),
                    // Too many at once.
                    AiError::Http(429, _) => Some(Duration::from_secs(20)),
                    // Down, or the line is out: twice in a row, a minute off.
                    _ if failures >= 2 => Some(Duration::from_secs(60)),
                    _ => None,
                };
                if let Some(rest) = rest {
                    *lock(&self.rest) = Some(Instant::now() + rest);
                    self.failures.store(0, Ordering::Relaxed);
                }
            }
        }
        result
    }

    fn try_models(
        &self,
        text: &str,
        voice: &str,
        stop: Option<&Stop>,
        on_samples: &mut dyn FnMut(&[i16]),
    ) -> Result<usize, AiError> {
        let path = format!("/text-to-speech/{voice}/stream?output_format=pcm_24000");
        let only_v4 = needs_v4(text);
        let mut last = None;
        for &model in MODELS {
            if (only_v4 && !is_v4(model)) || lock(&self.skip).contains(&model) {
                continue;
            }
            let mut plain = lock(&self.plain).contains(&model);
            loop {
                let body = request(text, model, plain);
                let mut made = false;
                let result = self
                    .api
                    .audio_stream(&path, &body, stop, &mut |samples: &[i16]| {
                        made = true;
                        on_samples(samples)
                    });
                let refused = match &result {
                    Err(AiError::Http(400 | 403 | 404 | 422, message)) if !made => {
                        Some(message.to_lowercase())
                    }
                    _ => None,
                };
                match refused {
                    // A setting it won't take: once more, without them.
                    Some(message) if !plain && about_settings(&message) => {
                        lock(&self.plain).push(model);
                        plain = true;
                    }
                    // A model the account can't use: the next one.
                    Some(message) if message.contains("model") => {
                        lock(&self.skip).push(model);
                        last = result.err();
                        break;
                    }
                    _ => return result,
                }
            }
        }
        Err(last.unwrap_or_else(|| {
            AiError::Unsupported(if only_v4 {
                "no ElevenLabs model on this account speaks this language".into()
            } else {
                "no ElevenLabs model works on this account".into()
            })
        }))
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Eleven v4 (and v4 Turbo): its own voice settings, and every language.
fn is_v4(model: &str) -> bool {
    model.starts_with("eleven_v4")
}

/// Written in a script only Eleven v4 speaks of the ones the phone offers
/// (Hebrew, Thai).
fn needs_v4(text: &str) -> bool {
    text.chars()
        .any(|c| matches!(c, '\u{0590}'..='\u{05FF}' | '\u{0E00}'..='\u{0E7F}'))
}

/// An error about the voice settings rather than the model.
fn about_settings(message: &str) -> bool {
    [
        "voice_settings",
        "stability",
        "similarity",
        "style",
        "speed",
        "speaker_boost",
    ]
    .iter()
    .any(|w| message.contains(w))
}

/// What ElevenLabs is asked: the words, the model, and a lively delivery
/// (v4 takes only stability and similarity; `plain`: the voice as it is).
fn request(text: &str, model: &str, plain: bool) -> Value {
    let mut body = json!({"text": text, "model_id": model});
    if !plain {
        body["voice_settings"] = if is_v4(model) {
            json!({"stability": 0.5, "similarity_boost": 0.8})
        } else {
            json!({
                "stability": 0.4,
                "similarity_boost": 0.8,
                "style": 0.35,
                "use_speaker_boost": true,
                "speed": 1.1,
            })
        };
    }
    body
}

/// A voice to start with, when the player hasn't picked one: a young,
/// lively one if there is one, else the first.
pub fn default_voice(voices: &[Voice]) -> Option<&Voice> {
    let score = |v: &&Voice| {
        let about = v.about.to_lowercase();
        [
            "young",
            "energetic",
            "playful",
            "excited",
            "casual",
            "upbeat",
            "conversational",
        ]
        .iter()
        .filter(|w| about.contains(*w))
        .count()
    };
    voices
        .iter()
        .max_by_key(|v| (score(v), std::cmp::Reverse(v.name.to_lowercase())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_lively_voice_is_picked_to_start_with() {
        let voice = |name: &str, about: &str| Voice {
            id: name.to_lowercase(),
            name: name.into(),
            about: about.into(),
        };
        let voices = [
            voice("Rachel", "american, calm, young, female"),
            voice("Leo", "british, young, energetic, casual, male"),
            voice("Bob", "deep, old"),
        ];
        assert_eq!(default_voice(&voices).unwrap().name, "Leo");
        assert_eq!(default_voice(&voices[2..]).unwrap().name, "Bob");
        assert!(default_voice(&[]).is_none());
    }

    #[test]
    fn a_line_is_asked_for_lively_and_quick() {
        let body = request("Pot now!", "eleven_flash_v2_5", false);
        assert_eq!(body["model_id"], "eleven_flash_v2_5");
        assert_eq!(body["text"], "Pot now!");
        assert!(body["voice_settings"]["speed"].as_f64().unwrap() > 1.0);
        // v4 takes no style or speed.
        let body = request("היי!", "eleven_v4_turbo", false);
        assert!(body["voice_settings"].get("speed").is_none());
        assert!(body["voice_settings"].get("style").is_none());
        assert!(
            request("היי!", "eleven_v4_turbo", true)
                .get("voice_settings")
                .is_none()
        );
    }

    #[test]
    fn hebrew_and_thai_go_to_the_model_that_speaks_them() {
        assert!(needs_v4("תשתה שיקוי!"));
        assert!(needs_v4("HP 30%. ดื่มยา!"));
        assert!(!needs_v4("Pot now! 30% HP."));
        assert!(!needs_v4("Пей зелье!"));
        assert!(about_settings(
            "invalid voice_settings: speed must be between 0.7 and 1.2"
        ));
        assert!(!about_settings("model_not_found: the model does not exist"));
    }
}
