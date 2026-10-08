//! Talking in real time, like a call (OpenAI's Realtime voice model, as in
//! ChatGPT's voice mode).
//!
//! ```text
//!   phone ── microphone ──────────── WebRTC ─────────▶ OpenAI Realtime
//!     ▲  ◀── its voice ──────────────────────────────── (hears and speaks
//!     │  ◀── events (what was said, tool calls) ─────── any language)
//!     │
//!     ├── POST /api/live  ──▶ PC: a short-lived key, made with the
//!     │                        player's own key (which stays on the PC)
//!     ├── GET  /api/eyes  ──▶ PC: the screen and what is read off it,
//!     │                        while the game is the window in front
//!     └── POST /api/tool  ──▶ PC: learn this, look closer, search the web…
//! ```
//!
//! The model hears the player directly: it understands whatever language
//! they speak, follows them when they switch or mix languages, hears them
//! while it talks (the phone cancels its own voice from its microphone), and
//! stops and answers when talked over. MapleSyrup's own lines (low HP, a
//! level-up) are handed to it to say, so they come in the same voice and
//! language: the watcher's at most once per 20 s and with the reading
//! behind them (the PC's `Relay`), so the call passes a number on for HP
//! and MP, and the thing itself for anything else, instead of restating
//! the watcher every few seconds. When the attitude changes mid-call the
//! phone fetches the instructions again (`/api/instructions`) and hands
//! them to the call (`session.update`).

use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Value, json};

use super::openai::{AiError, OpenAi};

/// Realtime models, best first, when the list of the key's models can't be
/// read.
pub const LIVE_MODELS: &[&str] = &[
    "gpt-realtime",
    "gpt-realtime-mini",
    "gpt-4o-realtime-preview",
];
/// What the player says is also written down (for the screen and the log),
/// in whatever language it is.
pub const TRANSCRIBE_MODEL: &str = "gpt-4o-mini-transcribe";
/// Where the phone sends its WebRTC offer.
pub const CALLS_URL: &str = "https://api.openai.com/v1/realtime/calls";

/// Who MapleSyrup is on a call: the same buddy, speaking rather than
/// writing, in any language.
const LIVE_PERSONA: &str = "You are MapleSyrup: a fluffy cream-colored dog in a pancake-and-syrup hat, \
and the player's buddy while they play MapleStory (the current global version). You're on a live voice call with \
them while they play, like a friend on voice chat.";

/// What else a call needs to know, after the rules and the attitude.
const LIVE_MORE: &str = "On the call:
- Speak fast, like a gamer on comms: no pauses, no drawn-out words. Short turns; let them talk.
- Speak the language the player speaks, every time. When they switch languages, switch with them at once; when \
they mix languages in one sentence (Hebrew and English, say), answer the same way. A language you were told to use \
\"by default\" is for when their words have no language: it never overrides the one they are speaking now. Say \
game words the way players say them.
- Never say again what you said in your last two turns unless they ask again, and never open with where they are \
unless they asked where they are. If what you heard makes no sense, say in a few words that you didn't catch it.
- If they talk over you, stop and go with what they just said; don't repeat what you had said.
- When they speak you may also get a message that is not from them: what your vision engine reads off the game \
right now (level, HP, MP, EXP; values marked \"about\" are estimates) and, now and then, a small picture of the \
screen with rulers on its edges (0 to 1000 across and down, for pointing at things). Use it like a friend looking \
at the same screen; never ask them to read the screen to you (look_closer reads small print). If what they say \
clearly disagrees with what you see, say what you see.
- Now and then a message comes from MapleSyrup's game watcher, never from the player: a line it wants said (low \
HP or MP, a death, a level-up, something they asked you to watch for, a tip, a correction from a look-up) and \
the game as read right then; a warning about HP or MP comes with its reading of them at that moment. That \
reading is newer than any picture you have. Pass the line on in one short clause, in the language you're \
speaking with them and in your attitude: \
when the line is about HP or MP, say the number (\"HP's at 11, pot now\"), not the watcher's words; otherwise say \
the thing, in your words, short (\"Rebuff.\" is \"rebuff\", not \"HP's at 96, rebuff\"), and leave the numbers \
out. Never both. Never restate the whole line, never argue with it, never answer it with what your side shows. \
A line that comes late (\"N s ago\") is still said, as late news (\"you died a moment ago\"). If the player was \
talking, their words come first: answer them, then the watcher in a few words.
- Presence: greet only when your watcher says the phone just connected, never on your own; never ask whether \
they're still there — your watcher does, when the game idles. When the session facts say they had been quiet for \
a long while until just now, one short \"welcome back\" is fine, once. Those facts (how long, deaths, level-ups, \
when they last spoke, the lowest HP) are for you, not for them: never recite them; one comes up only when it \
changes what you'd say.
- What you know about them from before comes in only when it bears on what they just said, as a clause, never \
as a list: \"that boss again?\", not \"I remember you fought Zakum, wanted a Fafnir and play Mu Lung Dojo\".
- You can't press keys or play for them; you watch and talk.

Tools (never announce one before using it; after one, a few words at most):
- Only when the player shows or tells you what something on screen is (\"this is...\", \"that's my...\") or asks \
you to watch for something, call learn_thing with a tight box around it in the picture's 0-1000 coordinates. \
Never learn things on your own. If they want a heads-up (\"tell me when a rune shows up\", \"warn me when the \
boss is under 20%\"), set alert, threshold and say (what to say then, in their language). The alert is about that \
thing appearing, disappearing or crossing a value — never attach an unrelated announcement to it (a level-up is \
watched by MapleSyrup itself; their own character is always on screen and is never a thing to learn).
- When the player says a value you have is wrong (their level, HP, MP, EXP, map, name, job), call correct_reading.
- When they correct you on anything else (a game fact, a name, how something works, how you talk), call \
note_correction with the right version and go on with it. What they corrected you on before beats what you think \
you know.
- When the player tells you something worth keeping (their class, a key binding, a goal) or asks you to \
remember something, call remember_fact.
- look_it_up never makes them wait: when you're not sure of a MapleStory fact (or they ask you to look something \
up), say your best answer first, then call it with the question and what you said; you'll be told only if you \
were wrong. Never mention it.
- set_warnings when they want low HP or MP warnings at another percent, or no more of them, or back to the usual.
- forget_thing when asked to forget something you learned; look_closer to read small text or details.
- mark_moment when they ask you to mark or save the moment; set_muted when they ask you to be quiet or to \
talk again.
- set_recording when they ask you to start or stop recording (a video of the screen with all the sound).
- set_coaching when they ask you to stop speaking up on your own (\"only talk when I ask\", \"no more tips\"), or \
to start again.";

/// The instructions for a call: who it is, its rules and the attitude the
/// player picked, what it learned about the player (`learned`: what they
/// told it, the notebook, their corrections), and the last things said (so a
/// call picked up again keeps its thread).
pub fn instructions(
    learned: &str,
    recent: &[String],
    language: Option<&str>,
    attitude: super::style::Attitude,
) -> String {
    let mut text = format!(
        "{LIVE_PERSONA}\n\n{}\n\n{LIVE_MORE}",
        super::style::rules(attitude)
    );
    if let Some(name) = language
        .filter(|l| !l.trim().is_empty())
        .map(super::language::name)
    {
        text.push_str(&format!(
            "\n\nThe player's phone is set to {name}: start in {name} until they speak; then follow them."
        ));
    }
    if !learned.trim().is_empty() {
        text.push_str(
            "\n\nWhat you learned from playing together before (use it naturally; never recite it):\n",
        );
        text.push_str(learned.trim());
    }
    let recent: Vec<&String> = recent.iter().filter(|l| !l.trim().is_empty()).collect();
    if !recent.is_empty() {
        text.push_str("\n\nThe conversation so far (the call was just picked up again):\n");
        for line in recent.iter().rev().take(12).rev() {
            let line: String = line.chars().take(300).collect();
            text.push_str(&format!("{line}\n"));
        }
    }
    text
}

/// How a call listens, as MapleSyrup adapted it to the player.
#[derive(Debug, Clone, PartialEq)]
pub struct Tuning {
    /// How soon it answers when they pause: "high", "medium" or "low".
    pub eagerness: String,
    /// Names and words they use, so what they say is written down right.
    pub words: Option<String>,
    /// How fast it speaks (1 is its usual pace; a quick talker is snappier).
    pub speed: f64,
}

/// A call speaks a little faster than the model's usual pace: it's a game.
pub const SPEED: f64 = 1.15;

impl Default for Tuning {
    fn default() -> Self {
        Tuning {
            eagerness: "high".into(),
            words: None,
            speed: SPEED,
        }
    }
}

impl Tuning {
    fn eagerness(&self) -> &str {
        match self.eagerness.as_str() {
            e @ ("low" | "medium" | "high" | "auto") => e,
            _ => "high",
        }
    }

    fn speed(&self) -> f64 {
        if self.speed.is_finite() {
            self.speed.clamp(0.8, 1.5)
        } else {
            1.0
        }
    }

    fn transcription(&self) -> Value {
        let mut transcription = json!({"model": TRANSCRIBE_MODEL});
        if let Some(words) = self.words.as_deref().filter(|w| !w.trim().is_empty()) {
            let words: String = words.chars().take(800).collect();
            transcription["prompt"] = json!(words);
        }
        transcription
    }
}

/// MapleSyrup's tools as a call takes them: functions only (no hosted
/// tools, no strict flag).
pub fn tools(definitions: Vec<Value>) -> Vec<Value> {
    definitions
        .into_iter()
        .filter(|t| t["type"] == "function")
        .map(|mut t| {
            if let Some(object) = t.as_object_mut() {
                object.remove("strict");
            }
            t
        })
        .collect()
}

/// Makes calls: a short-lived key for the phone to talk to OpenAI with.
pub struct Live {
    openai: Arc<OpenAi>,
    voice: String,
    /// Where the phone sends its offer (OpenAI's, or a stand-in for tests).
    calls_url: String,
    /// The model that worked last time.
    chosen: Mutex<Option<String>>,
}

impl Live {
    pub fn new(openai: Arc<OpenAi>, voice: &str) -> Live {
        Live {
            openai,
            voice: voice.to_string(),
            calls_url: std::env::var("OPENAI_REALTIME_URL").unwrap_or_else(|_| CALLS_URL.into()),
            chosen: Mutex::new(None),
        }
    }

    /// The realtime models to try, best first: the newest full model the
    /// key has, then the smaller and older ones.
    fn candidates(&self) -> Vec<String> {
        if let Some(model) = self.chosen.lock().ok().and_then(|c| c.clone()) {
            return vec![model];
        }
        let Ok(mut models) = self.openai.models() else {
            return LIVE_MODELS.iter().map(|m| m.to_string()).collect();
        };
        models.retain(|(id, _)| {
            id.contains("realtime") && !id.contains("transcri") && !id.contains("translat")
        });
        let rank = |id: &str| {
            if id.starts_with("gpt-realtime") && !id.contains("mini") {
                0
            } else if id.starts_with("gpt-realtime") {
                1
            } else if !id.contains("mini") {
                2
            } else {
                3
            }
        };
        models.sort_by(|(a, made_a), (b, made_b)| rank(a).cmp(&rank(b)).then(made_b.cmp(made_a)));
        let mut out: Vec<String> = models.into_iter().map(|(id, _)| id).collect();
        for known in LIVE_MODELS {
            if !out.iter().any(|m| m == known) {
                out.push(known.to_string());
            }
        }
        out
    }

    /// A call for the phone: `{key, url, model}`. The key works for a few
    /// minutes and only for this; the player's own key stays here.
    pub fn session(
        &self,
        instructions: &str,
        tools: &[Value],
        tuning: &Tuning,
    ) -> Result<Value, AiError> {
        let mut last = AiError::Parse("no realtime model to try".into());
        // (A model that won't take the speed is asked again without it.)
        let mut speed = true;
        let candidates = self.candidates();
        let mut index = 0;
        while let Some(model) = candidates.get(index).cloned() {
            let mut output = json!({"voice": self.voice});
            if speed {
                output["speed"] = json!(tuning.speed());
            }
            let body = json!({
                "expires_after": {"anchor": "created_at", "seconds": 600},
                "session": {
                    "type": "realtime",
                    "model": model,
                    "instructions": instructions,
                    "audio": {
                        "input": {
                            "noise_reduction": {"type": "near_field"},
                            "transcription": tuning.transcription(),
                            "turn_detection": {
                                "type": "semantic_vad",
                                "eagerness": tuning.eagerness(),
                                "create_response": true,
                                "interrupt_response": true,
                            },
                        },
                        "output": output,
                    },
                    "tools": tools,
                    "tool_choice": "auto",
                },
            });
            match self
                .openai
                .post_json("/realtime/client_secrets", &body, Duration::from_secs(20))
            {
                Ok(answer) => {
                    let key = answer["value"]
                        .as_str()
                        .or(answer["client_secret"]["value"].as_str())
                        .ok_or_else(|| AiError::Parse("no key in the answer".into()))?;
                    self.remember(&model);
                    // The hint goes to the phone too: on silence the
                    // transcriber sometimes returns its own hint as what
                    // the player said, and the phone drops those.
                    return Ok(json!({
                        "key": key,
                        "url": self.calls_url,
                        "model": model,
                        "api": "ga",
                        "hint": tuning.words.as_deref().unwrap_or(""),
                    }));
                }
                Err(AiError::Http(400, message))
                    if speed && message.to_lowercase().contains("speed") =>
                {
                    speed = false;
                    continue;
                }
                // A model this key can't use: the next one.
                Err(AiError::Http(status @ (400 | 403 | 404), message))
                    if message.to_lowercase().contains("model") =>
                {
                    last = AiError::Http(status, message);
                }
                // An account still on the first version of the API.
                Err(AiError::Http(404, _)) => {
                    match self.beta_session(&model, instructions, tools, tuning) {
                        Ok(call) => {
                            self.remember(&model);
                            return Ok(call);
                        }
                        Err(e) => last = e,
                    }
                }
                Err(e) => return Err(e),
            }
            index += 1;
        }
        Err(last)
    }

    fn beta_session(
        &self,
        model: &str,
        instructions: &str,
        tools: &[Value],
        tuning: &Tuning,
    ) -> Result<Value, AiError> {
        let body = json!({
            "model": model,
            "voice": self.voice,
            "instructions": instructions,
            "modalities": ["audio", "text"],
            "input_audio_transcription": tuning.transcription(),
            "input_audio_noise_reduction": {"type": "near_field"},
            "turn_detection": {"type": "semantic_vad", "eagerness": tuning.eagerness(), "create_response": true, "interrupt_response": true},
            "tools": tools,
            "tool_choice": "auto",
        });
        let answer = self
            .openai
            .post_json("/realtime/sessions", &body, Duration::from_secs(20))?;
        let key = answer["client_secret"]["value"]
            .as_str()
            .ok_or_else(|| AiError::Parse("no key in the answer".into()))?;
        let url = self.calls_url.replace("/realtime/calls", "/realtime");
        Ok(
            json!({"key": key, "url": format!("{url}?model={model}"), "model": model, "api": "beta"}),
        )
    }

    fn remember(&self, model: &str) {
        if let Ok(mut chosen) = self.chosen.lock() {
            *chosen = Some(model.to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_call_is_told_who_it_is_what_it_knows_and_what_was_said() {
        let text = instructions(
            "About the player (they told you this):\n- Their class is Night Lord.",
            &["Player: hey".into(), "MapleSyrup: Hey! Ready?".into()],
            Some("he-IL"),
            crate::companion::Attitude::Savage,
        );
        assert!(text.contains("MapleSyrup's rules"));
        assert!(text.contains("Your attitude: savage"));
        assert!(text.contains("look_it_up never makes them wait"));
        // The watcher's line: the number when it is about HP or MP, the
        // thing itself otherwise (never both), never argued with; late is
        // still said. Only a warning comes with the reading (a death,
        // "Rebuff." come without).
        assert!(text.contains("newer than any picture you have"));
        assert!(
            text.contains("a warning about HP or MP comes with its reading of them at that moment")
        );
        assert!(!text.contains("with its reading of HP and MP at that moment and the game"));
        assert!(text.contains("when the line is about HP or MP, say the number"));
        assert!(text.contains("otherwise say the thing, in your words, short"));
        assert!(text.contains("Never both."));
        assert!(
            !text.contains(
                "with the number (\"HP's at 11, pot now\") rather than the watcher's words"
            )
        );
        assert!(text.contains("A line that comes late (\"N s ago\") is still said"));
        assert!(text.contains("never argue with it"));
        assert!(text.contains("their words come first"));
        assert!(!text.contains("say it right away"));
        assert!(text.contains("switch with them"));
        assert!(text.contains("set to Hebrew"));
        assert!(text.contains("learned from playing together"));
        assert!(text.contains("Night Lord"));
        assert!(text.contains("MapleSyrup: Hey! Ready?"));
        let fresh = instructions("", &[], None, Default::default());
        assert!(fresh.contains("Your attitude: blunt"));
        assert!(!fresh.contains("conversation so far"));
        assert!(!fresh.contains("learned from playing together"));
    }

    /// The rules a call shares with the conversation word for word: how to
    /// be present (greet when told the phone connected, "welcome back" once
    /// after a long quiet, never ask after them, never recite the session
    /// facts) and how what it knows about the
    /// player comes up (a clause when it bears on what they said, never a
    /// list). (They are copied: the conversation's live in its own
    /// module, out of reach of a shared constant.)
    fn shared_rules() -> Vec<&'static str> {
        LIVE_MORE
            .lines()
            .filter(|l| l.starts_with("- Presence:") || l.starts_with("- What you know about them"))
            .collect()
    }

    #[test]
    fn a_call_keeps_the_conversations_rules_on_presence_and_on_what_it_knows() {
        let rules = shared_rules();
        assert_eq!(rules.len(), 2, "{rules:?}");
        // Each, word for word, is the conversation's rule too.
        let persona = super::super::Brain::new().persona();
        for rule in &rules {
            assert!(rule.split_whitespace().count() > 20, "{rule}");
            assert!(persona.contains(rule), "the conversation lacks: {rule}");
        }
        // And every call gets them, whatever the attitude.
        for attitude in crate::companion::Attitude::ALL {
            let text = instructions("", &[], None, attitude);
            assert!(text.contains(
                "greet only when your watcher says the phone just connected, never on your own"
            ));
            assert!(text.contains(
                "never ask whether they're still there — your watcher does, when the game idles"
            ));
            assert!(text.contains(
                "they had been quiet for a long while until just now, one short \"welcome back\" \
is fine, once"
            ));
            assert!(
                text.contains(
                    "never recite them; one comes up only when it changes what you'd say"
                )
            );
            assert!(text.contains(
                "only when it bears on what they just said, as a clause, never as a list: \"that boss again?\""
            ));
        }
    }

    #[test]
    fn a_call_listens_the_way_it_adapted_to() {
        let usual = Tuning::default();
        assert_eq!(usual.eagerness(), "high");
        assert!(usual.transcription().get("prompt").is_none());
        let tuned = Tuning {
            eagerness: "medium".into(),
            words: Some("MapleStory. Names and words the player uses: Zakum, MoonWalker77.".into()),
            speed: 9.0,
        };
        assert_eq!(tuned.speed(), 1.5);
        assert_eq!(usual.speed(), SPEED);
        assert_eq!(tuned.eagerness(), "medium");
        assert_eq!(tuned.transcription()["model"], TRANSCRIBE_MODEL);
        assert!(
            tuned.transcription()["prompt"]
                .as_str()
                .unwrap()
                .contains("MoonWalker77")
        );
        let odd = Tuning {
            eagerness: "very".into(),
            words: None,
            speed: f64::NAN,
        };
        assert_eq!(odd.speed(), 1.0);
        assert_eq!(odd.eagerness(), "high");
    }

    #[test]
    fn a_call_takes_functions_only() {
        let defs = vec![
            json!({"type": "function", "name": "learn_thing", "strict": true, "parameters": {}}),
            json!({"type": "web_search", "search_context_size": "low"}),
        ];
        let tools = tools(defs);
        assert_eq!(tools.len(), 1);
        assert!(tools[0].get("strict").is_none());
        assert_eq!(super::tools(vec![]).len(), 0);
    }
}
