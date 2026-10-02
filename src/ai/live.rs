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
//! language.

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
and the player's buddy while they play MapleStory (the current version of the game). You're on a live voice \
call with them while they play, like a friend sitting next to them.

How you talk:
- Like a real friend: warm, casual, quick, a little playful, genuinely interested. Talk like a person, not an \
assistant. Keep your turns short, usually a sentence or two, and let them talk.
- Speak the language the player speaks. When they switch languages, switch with them at once; when they mix \
languages in one sentence (Hebrew and English, say), answer naturally the same way. Say game words the way \
players say them.
- If they talk over you, stop and go with what they just said; don't repeat what you had said.
- React first when something happens (\"Ooh, nice!\"), lead with the answer, don't repeat their question back, \
don't open with filler, and don't end every turn with a question.
- When they speak you may also get a message that is not from them: what your vision engine reads off the game \
right now (level, HP, MP, EXP; values marked \"about\" are estimates) and a picture of the screen with rulers \
on its edges (0 to 1000 across and down, for pointing at things). Use it like a friend looking at the same \
screen; never ask them to read the screen to you. If what they say clearly disagrees with what you see, say \
what you see.
- Don't guess MapleStory facts (where a place is, level requirements, quests, bosses, events): look them up \
with search_web, or say plainly you're not sure. Never read links out.
- MapleSyrup's game watcher sometimes tells you something to say (low HP or MP, a level-up, something they \
asked you to watch for): say it right away, briefly, in your own words and in the language you're speaking \
with them.
- You can't press keys or play for them; you watch and talk.

You get better the more the player teaches you:
- Only when the player shows or tells you what something on screen is (\"this is...\", \"that's my...\") or asks \
you to watch for something, call learn_thing with a tight box around it in the picture's 0-1000 coordinates. \
Never learn things on your own. If they want a heads-up (\"tell me when a rune shows up\", \"warn me when the \
boss is under 20%\"), set alert, threshold and say (what to say then, in their language).
- When the player says a value you have is wrong (their level, HP, MP, EXP, map, name, job), call correct_reading.
- When the player tells you something worth keeping (their class, a key binding, a goal) or asks you to \
remember something, call remember_fact.
- forget_thing when asked to forget something you learned; look_closer to read small text or details.
- mark_moment when they ask you to mark or save the moment; set_muted when they ask you to be quiet or to \
talk again.
- set_recording when they ask you to start or stop recording (a video of the screen with all the sound).
After using a tool, say what happened in a few words.";

/// The instructions for a call: who it is, what the player told it about
/// themselves, and the last things said (so a call picked up again keeps
/// its thread).
pub fn instructions(about: &str, recent: &[String], language: Option<&str>) -> String {
    let mut text = LIVE_PERSONA.to_string();
    if let Some(name) = language
        .filter(|l| !l.trim().is_empty())
        .map(super::language::name)
    {
        text.push_str(&format!(
            "\n\nThe player's phone is set to {name}: start in {name} until they speak; then follow them."
        ));
    }
    if !about.trim().is_empty() {
        text.push_str("\n\nAbout the player (they told you this):\n");
        text.push_str(about.trim());
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

/// MapleSyrup's tools as a call takes them: functions only (no hosted
/// tools, no strict flag), with a web search the PC runs.
pub fn tools(definitions: Vec<Value>, web: bool) -> Vec<Value> {
    let mut tools: Vec<Value> = definitions
        .into_iter()
        .filter(|t| t["type"] == "function")
        .map(|mut t| {
            if let Some(object) = t.as_object_mut() {
                object.remove("strict");
            }
            t
        })
        .collect();
    if web {
        tools.push(json!({
            "type": "function",
            "name": "search_web",
            "description": "Look a MapleStory fact up on the web (how to get somewhere, requirements, quests, \
bosses, events, training spots) when you aren't sure. Not for what is on screen.",
            "parameters": {
                "type": "object",
                "properties": {"query": {"type": "string", "description": "What to look up, in English."}},
                "required": ["query"],
            },
        }));
    }
    tools
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
    pub fn session(&self, instructions: &str, tools: &[Value]) -> Result<Value, AiError> {
        let mut last = AiError::Parse("no realtime model to try".into());
        for model in self.candidates() {
            let body = json!({
                "expires_after": {"anchor": "created_at", "seconds": 600},
                "session": {
                    "type": "realtime",
                    "model": model,
                    "instructions": instructions,
                    "audio": {
                        "input": {
                            "noise_reduction": {"type": "near_field"},
                            "transcription": {"model": TRANSCRIBE_MODEL},
                            "turn_detection": {
                                "type": "semantic_vad",
                                "eagerness": "high",
                                "create_response": true,
                                "interrupt_response": true,
                            },
                        },
                        "output": {"voice": self.voice},
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
                    return Ok(
                        json!({"key": key, "url": self.calls_url, "model": model, "api": "ga"}),
                    );
                }
                // A model this key can't use: the next one.
                Err(AiError::Http(status @ (400 | 403 | 404), message))
                    if message.to_lowercase().contains("model") =>
                {
                    last = AiError::Http(status, message);
                }
                // An account still on the first version of the API.
                Err(AiError::Http(404, _)) => {
                    match self.beta_session(&model, instructions, tools) {
                        Ok(call) => {
                            self.remember(&model);
                            return Ok(call);
                        }
                        Err(e) => last = e,
                    }
                }
                Err(e) => return Err(e),
            }
        }
        Err(last)
    }

    fn beta_session(
        &self,
        model: &str,
        instructions: &str,
        tools: &[Value],
    ) -> Result<Value, AiError> {
        let body = json!({
            "model": model,
            "voice": self.voice,
            "instructions": instructions,
            "modalities": ["audio", "text"],
            "input_audio_transcription": {"model": TRANSCRIBE_MODEL},
            "input_audio_noise_reduction": {"type": "near_field"},
            "turn_detection": {"type": "semantic_vad", "eagerness": "high", "create_response": true, "interrupt_response": true},
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

/// A short answer from the web, for the call's search_web (the call's model
/// can't search itself).
pub fn search(openai: &OpenAi, query: &str) -> String {
    let ask = super::openai::Ask {
        instructions: "Look this MapleStory question up (the current global version, GMS; prefer \
maplestorywiki.net and maplestory.nexon.net) and answer in two or three short sentences that will be read \
out loud: the facts only, no links."
            .into(),
        input: vec![json!({"role": "user", "content": query})],
        tools: vec![json!({"type": "web_search", "search_context_size": "low"})],
        max_output_tokens: 300,
        timeout: Duration::from_secs(45),
        ..Default::default()
    };
    match openai.ask(&ask, None) {
        Ok(answer) => super::brain::for_speech(&answer.text),
        Err(e) => format!("The search didn't work ({e})."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_call_is_told_who_it_is_what_it_knows_and_what_was_said() {
        let text = instructions(
            "- Their class is Night Lord.",
            &["Player: hey".into(), "MapleSyrup: Hey! Ready?".into()],
            Some("he-IL"),
        );
        assert!(text.contains("switch with them"));
        assert!(text.contains("set to Hebrew"));
        assert!(text.contains("Night Lord"));
        assert!(text.contains("MapleSyrup: Hey! Ready?"));
        let fresh = instructions("", &[], None);
        assert!(!fresh.contains("conversation so far"));
    }

    #[test]
    fn a_call_takes_functions_only_and_a_search_the_pc_runs() {
        let defs = vec![
            json!({"type": "function", "name": "learn_thing", "strict": true, "parameters": {}}),
            json!({"type": "web_search", "search_context_size": "low"}),
        ];
        let tools = tools(defs, true);
        assert_eq!(tools.len(), 2);
        assert!(tools[0].get("strict").is_none());
        assert_eq!(tools[1]["name"], "search_web");
        assert_eq!(super::tools(vec![], false).len(), 0);
    }
}
