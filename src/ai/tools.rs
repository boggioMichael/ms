//! What the conversation model can do besides talking: learn to recognise
//! what the player shows it, take corrections, remember what the player
//! tells it, look closer at the screen, and search the web.
//!
//! Each tool is a function the model calls with JSON arguments (strict
//! schemas, so the arguments are always well formed); MapleSyrup runs it
//! and hands the result back, and the model says what happened in its own
//! words.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use image::RgbaImage;
use serde_json::{Value, json};

use super::images::NBox;
use super::openai::{Ask, Call, OpenAi};
use crate::sight::Sight;
use crate::sight::teacher::{self, Look};
use crate::sight::things::{Alert, Kind, Teach, When};

/// What the tools work with.
pub struct Toolbox {
    pub sight: Arc<Mutex<Sight>>,
    /// The vision model: pins down what the player points at, looks closer.
    pub eyes: Arc<OpenAi>,
    /// The settings folder (`about-me.txt` lives there).
    pub settings: PathBuf,
    /// Whether the model may search the web.
    pub web: bool,
}

/// What running a tool changed, for the rest of MapleSyrup.
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    /// A fact about the player to keep in mind from now on.
    Fact(String),
    /// Something learned, forgotten or corrected: a line for the log.
    Note(String),
    /// A command for the main loop: "mark", "mute" or "unmute".
    Command(String),
}

/// Ask the vision model one of the teacher's questions.
pub fn look(eyes: &OpenAi, look: &Look) -> Result<String, String> {
    let ask = Ask {
        instructions: look.instructions.clone(),
        input: vec![json!({"role": "user", "content": look.content})],
        schema: look.schema.clone().map(|s| (look.name.to_string(), s)),
        max_output_tokens: look.max_output_tokens,
        timeout: Duration::from_secs(45),
        needs_images: true,
        ..Default::default()
    };
    eyes.ask(&ask, None)
        .map(|a| a.text)
        .map_err(|e| e.to_string())
}

/// The commands the recording tool hands the main loop.
pub const RECORD_ON: &str = "record";
pub const RECORD_OFF: &str = "stop recording";

fn function(name: &str, description: &str, properties: Value) -> Value {
    let required: Vec<String> = properties
        .as_object()
        .map(|o| o.keys().cloned().collect())
        .unwrap_or_default();
    json!({
        "type": "function",
        "name": name,
        "description": description,
        "strict": true,
        "parameters": {
            "type": "object",
            "properties": properties,
            "required": required,
            "additionalProperties": false,
        },
    })
}

fn a_box(what: &str) -> Value {
    json!({"type": "array", "items": {"type": "number"},
           "description": format!("{what}: [x0, y0, x1, y1] in the first picture's 0–1000 coordinates (rulers on its edges)")})
}

impl Toolbox {
    /// The tools, as the Responses API takes them.
    pub fn definitions(&self) -> Vec<Value> {
        let mut tools = vec![
            function(
                "learn_thing",
                "Learn to recognise something on the game screen that the player shows you or names (a monster, \
NPC, item, portal, icon, the boss's HP bar, a counter...), so you can notice it yourself from now on, and \
optionally speak up when it appears, disappears, or a bar or number crosses a threshold.",
                json!({
                    "name": {"type": "string", "description": "What the player calls it."},
                    "kind": {"type": "string", "enum": ["object", "indicator", "gauge", "number", "text"],
                             "description": "object: can be anywhere (monster, NPC, item, portal); indicator: an icon in one fixed place that is there or not (a buff, a warning); gauge: a bar that fills and empties (boss HP, a timer); number or text: written in one fixed place."},
                    "box": a_box("A tight box around it"),
                    "describe": {"type": "string", "description": "What it looks like, to find it again (colours, shape)."},
                    "alert": {"type": "string", "enum": ["none", "appears", "disappears", "below", "above", "changes"]},
                    "threshold": {"type": ["number", "null"], "description": "For below/above: the percent or number."},
                    "say": {"type": ["string", "null"], "description": "What to say aloud when the alert fires, short, in the player's language."},
                }),
            ),
            function(
                "forget_thing",
                "Forget something you learned to recognise.",
                json!({"name": {"type": "string"}}),
            ),
            function(
                "correct_reading",
                "The player says something you read from their screen is wrong: their level, HP, MP, EXP, map, name or job.",
                json!({
                    "what": {"type": "string", "enum": ["level", "hp", "mp", "exp", "map", "name", "job"]},
                    "value": {"type": "string", "description": "The right value, as the player said it."},
                }),
            ),
            function(
                "remember_fact",
                "Remember something the player told you about themselves or their game for good (their class, a key binding, a goal, a preference), or that they asked you to remember.",
                json!({"fact": {"type": "string", "description": "One short sentence, in the third person (\"Their boss menu key is F10\")."}}),
            ),
            function(
                "mark_moment",
                "Mark this moment: MapleSyrup saves the screen and the time, for the player's video later.",
                json!({}),
            ),
            function(
                "set_muted",
                "Stop speaking aloud (muted: true) or speak again (muted: false), when the player asks you to be quiet or to talk again.",
                json!({"muted": {"type": "boolean"}}),
            ),
            function(
                "set_recording",
                "Start (on: true) or stop (on: false) recording the session as a video on the player's PC: the whole screen, the game's sound, your voice and theirs. Only when the player asks.",
                json!({"on": {"type": "boolean"}}),
            ),
            function(
                "look_closer",
                "Look closely at part of the screen to read small text or make out a detail.",
                json!({
                    "box": a_box("The part to look at"),
                    "question": {"type": "string"},
                }),
            ),
        ];
        if self.web {
            // Low context: quicker, and a sentence or two is all it says.
            tools.push(json!({"type": "web_search", "search_context_size": "low"}));
        }
        tools
    }

    /// Run the call `call` on the frame the player was looking at. Returns
    /// what to tell the model, and what changed.
    pub fn run(&self, call: &Call, frame: Option<&RgbaImage>) -> (String, Option<Effect>) {
        let args: Value = serde_json::from_str(&call.arguments).unwrap_or(Value::Null);
        let text = |key: &str| args[key].as_str().unwrap_or("").trim().to_string();
        let lock = || self.sight.lock().unwrap_or_else(|e| e.into_inner());
        match call.name.as_str() {
            "learn_thing" => {
                let Some(frame) = frame else {
                    return (
                        "The game can't be seen right now, so nothing could be learned.".into(),
                        None,
                    );
                };
                let name = text("name");
                let Some(kind) = Kind::parse(&text("kind")) else {
                    return ("Unknown kind.".into(), None);
                };
                let values: Vec<f64> = args["box"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_f64)
                    .collect();
                let Some(rough) = NBox::from_thousandths(&values) else {
                    return ("The box must be four numbers from 0 to 1000.".into(), None);
                };
                let describe = text("describe");
                // A close-up to pin it down; the rough box if that fails.
                let (question, around) = teacher::refine(frame, &rough, &name, &describe);
                let place = look(&self.eyes, &question)
                    .ok()
                    .and_then(|answer| teacher::parse_refined(&answer, &around))
                    .filter(|b| {
                        // It must be about where the model said.
                        let g = rough.grown(1.0, 1.0);
                        let (cx, cy) = b.center();
                        cx >= g.x0 && cx <= g.x1 && cy >= g.y0 && cy <= g.y1
                    })
                    .unwrap_or(rough);
                let alert = When::parse(&text("alert")).map(|when| Alert {
                    when,
                    threshold: args["threshold"].as_f64().map(|t| t as f32),
                    say: {
                        let say = text("say");
                        if say.is_empty() {
                            format!("{name}!")
                        } else {
                            say
                        }
                    },
                });
                let result = lock().things.learn(
                    frame,
                    Teach {
                        name: name.clone(),
                        kind,
                        place,
                        describe,
                        alert,
                    },
                );
                match result {
                    Ok(line) => (line.clone(), Some(Effect::Note(line))),
                    Err(why) => (format!("Couldn't learn {name}: {why}."), None),
                }
            }
            "forget_thing" => {
                let name = text("name");
                match lock().things.forget(&name) {
                    Some(forgotten) => (
                        format!("Forgot {forgotten}."),
                        Some(Effect::Note(format!("forgot \"{forgotten}\""))),
                    ),
                    None => (format!("Nothing called {name} was learned."), None),
                }
            }
            "correct_reading" => match lock().correct(&text("what"), &text("value")) {
                Ok(done) => (
                    done.clone(),
                    Some(Effect::Note(format!("corrected: {done}"))),
                ),
                Err(why) => (why, None),
            },
            "remember_fact" => {
                let fact = text("fact");
                if fact.is_empty() {
                    return ("Nothing to remember.".into(), None);
                }
                let file = self.settings.join("about-me.txt");
                let mut about = std::fs::read_to_string(&file).unwrap_or_default();
                if !about
                    .lines()
                    .any(|l| l.trim_start_matches("- ").trim() == fact)
                {
                    if !about.is_empty() && !about.ends_with('\n') {
                        about.push('\n');
                    }
                    about.push_str(&format!("- {fact}\n"));
                    let _ = std::fs::create_dir_all(&self.settings);
                    let _ = std::fs::write(&file, about);
                }
                ("Remembered for good.".into(), Some(Effect::Fact(fact)))
            }
            "mark_moment" => ("Marked.".into(), Some(Effect::Command("mark".into()))),
            "set_recording" => {
                let on = args["on"].as_bool().unwrap_or(true);
                (
                    if on {
                        "Starting the recording (the very first time it fetches the recorder, which takes a minute)."
                    } else {
                        "Stopping the recording; the video is saved in the session folder."
                    }
                    .into(),
                    Some(Effect::Command(
                        if on { RECORD_ON } else { RECORD_OFF }.into(),
                    )),
                )
            }
            "set_muted" => {
                let muted = args["muted"].as_bool().unwrap_or(true);
                (
                    if muted { "Muted." } else { "Speaking again." }.into(),
                    Some(Effect::Command(
                        if muted { "mute" } else { "unmute" }.into(),
                    )),
                )
            }
            "look_closer" => {
                let Some(frame) = frame else {
                    return ("The game can't be seen right now.".into(), None);
                };
                let values: Vec<f64> = args["box"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_f64)
                    .collect();
                let Some(place) = NBox::from_thousandths(&values) else {
                    return ("The box must be four numbers from 0 to 1000.".into(), None);
                };
                match look(
                    &self.eyes,
                    &teacher::closer(frame, &place, &text("question")),
                ) {
                    Ok(answer) => (answer, None),
                    Err(why) => (format!("Couldn't look closer: {why}"), None),
                }
            }
            other => (format!("There is no tool called {other}."), None),
        }
    }
}
