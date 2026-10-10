//! What the conversation model can do besides talking: learn to recognise
//! what the player shows it, take corrections (and keep them for good),
//! remember what the player tells it, change its warnings, look closer at
//! the screen, and search the web.
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

use super::images::{NBox, Thousandths};
use super::knowledge::Source;
use super::memory::Learning;
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
    /// Where the player's corrections are kept.
    pub learning: Option<Learning>,
    /// The workshop, when MapleSyrup may rewrite itself on this PC (the
    /// tool is offered only while it is on).
    pub workshop: Option<Arc<crate::workshop::Workshop>>,
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
    /// Warn about low HP or MP (`what`: "hp" or "mp") below this percent:
    /// 0 never, None the usual.
    Warn { what: String, below: Option<f32> },
    /// Look `question` up in the background (`said`: the quick answer
    /// already given; `asked`: the player asked for the look-up).
    LookUp {
        question: String,
        said: String,
        asked: bool,
    },
    /// Change MapleSyrup's own program on this PC, as the player asked.
    Rewrite(String),
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
/// …and to speak up on its own, or only when asked.
pub const COACH_ON: &str = "coach";
pub const COACH_OFF: &str = "stop coaching";

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
optionally speak up when it appears, disappears, or a bar or number crosses a threshold. Not the player's own \
character (always on screen), and never with an alert about something else (level-ups are watched already).",
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
                "note_correction",
                "The player corrected you (a game fact, a name, how something works, or how you talk or behave): keep the right version for good, so you get it right from now on.",
                json!({
                    "about": {"type": "string", "description": "What it is about, in a few words (\"Easy Zakum level\")."},
                    "right": {"type": "string", "description": "The right version in one short sentence, as the player put it."},
                }),
            ),
            function(
                "set_warnings",
                "When to warn the player about low HP or MP, when they ask (\"warn me at 40%\", \"no more MP warnings\", \"warn me like before\").",
                json!({
                    "what": {"type": "string", "enum": ["hp", "mp"]},
                    "below": {"type": ["number", "null"], "description": "Warn below this percent; 0 for never; null for the usual."},
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
                "set_coaching",
                "Whether you speak up on your own while they play (tips, callouts, what to do next), when they ask you to stop doing that (\"only talk when I ask\", \"no more tips\") or to start again.",
                json!({"on": {"type": "boolean"}}),
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
        if self.workshop.as_ref().is_some_and(|w| w.is_on()) {
            tools.push(function(
                "change_your_code",
                "Change your own program on this PC, when the player asks you to change, fix or improve \
yourself (how you talk, what you warn about, a bug they hit...). A coding agent on this PC rewrites the code, \
it is built and tested, and the new version installs the next time MapleSyrup starts. It takes a few minutes; \
you say so. Pass what they asked for, in full, in their words. Only for changes to MapleSyrup itself.",
                json!({"instruction": {"type": "string", "description": "What to change, as the player put it."}}),
            ));
        }
        if self.web {
            // Never waited for: it answers first, the look-up runs behind.
            tools.push(function(
                "look_it_up",
                "Check a MapleStory fact in the background (the web, and what you were taught). Give your best answer \
FIRST, out loud, then call this with the question and what you said: it never makes the player wait, and you \
speak again only if you were wrong. Also when the player asks you to look something up.",
                json!({
                    "question": {"type": "string", "description": "The question, in English, with the game names."},
                    "said": {"type": "string", "description": "What you just told the player."},
                    "asked": {"type": "boolean", "description": "Whether the player asked you to look it up."},
                }),
            ));
        }
        tools
    }

    /// Run `call` for the player's sentence `heard`: a tool that changes a
    /// setting or the notebook acts only when `heard` plainly asks for it
    /// ([`plainly_asks`]) — never on "don't stop", "23% still owe" or a
    /// name heard once — and no name for the player is ever kept from
    /// speech. Otherwise nothing changes, and the model is told to ask him
    /// to say it plainly (the log gets a line).
    pub fn run_heard(
        &self,
        call: &Call,
        frame: Option<&RgbaImage>,
        heard: &str,
    ) -> (String, Option<Effect>) {
        let args: Value = serde_json::from_str(&call.arguments).unwrap_or(Value::Null);
        if let Some(why) = refused(&call.name, &args, heard) {
            return (
                format!("Not changed: {why}. If he wants it, ask him to say it plainly."),
                Some(Effect::Note(format!(
                    "not changed ({}): {why} — \"{}\"",
                    call.name,
                    heard.trim()
                ))),
            );
        }
        self.run(call, frame)
    }

    /// Run the call `call` on the frame the player was looking at. Returns
    /// what to tell the model, and what changed. (On a live call, where the
    /// player's words are the call's own; replies use [`Toolbox::run_heard`].)
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
            "note_correction" => {
                let (about, right) = (text("about"), text("right"));
                if about.is_empty() || right.is_empty() {
                    return ("Nothing to note.".into(), None);
                }
                if let Some(learning) = &self.learning {
                    learning.knowledge().add(&about, &right, Source::Player);
                }
                (
                    "Kept for good: you'll trust this over what you thought.".into(),
                    Some(Effect::Note(format!("learned: {about}: {right}"))),
                )
            }
            "set_warnings" => {
                let what = text("what");
                if what != "hp" && what != "mp" {
                    return ("What to warn about: hp or mp.".into(), None);
                }
                let below = args["below"]
                    .as_f64()
                    .filter(|b| b.is_finite())
                    .map(|b| (b as f32).clamp(0.0, 95.0));
                let name = what.to_uppercase();
                let done = match below {
                    Some(b) if b <= 0.0 => format!("No more {name} warnings."),
                    Some(b) => format!("You'll be warned when {name} is under {b:.0}%."),
                    None => format!("{name} warnings are back to the usual."),
                };
                (done, Some(Effect::Warn { what, below }))
            }
            "look_it_up" => {
                let question = text("question");
                if question.is_empty() {
                    return ("Nothing to look up.".into(), None);
                }
                let asked = args["asked"].as_bool().unwrap_or(false);
                // Known already: the answer at once.
                let known = self
                    .learning
                    .as_ref()
                    .and_then(|l| l.knowledge().find(&question));
                if let Some(known) = known {
                    let whose = match known.from {
                        Source::Player => "the player taught you this; trust it",
                        Source::Web => "you looked it up before",
                    };
                    return (
                        format!(
                            "Known: {} ({whose}). If that differs from what you said, correct yourself in a few \
words now; otherwise say nothing more.",
                            known.answer
                        ),
                        None,
                    );
                }
                (
                    "Looking it up in the background. Say nothing more about it now; you'll be told if you were \
wrong."
                        .into(),
                    Some(Effect::LookUp {
                        question,
                        said: text("said"),
                        asked,
                    }),
                )
            }
            "mark_moment" => ("Marked.".into(), Some(Effect::Command("mark".into()))),
            "change_your_code" => {
                let instruction = text("instruction");
                let Some(workshop) = self.workshop.as_ref().filter(|w| w.is_on()) else {
                    return (
                        "The workshop is off: MapleSyrup can't change itself right now.".into(),
                        None,
                    );
                };
                if instruction.chars().filter(|c| c.is_alphanumeric()).count() < 6 {
                    return ("Say what to change.".into(), None);
                }
                let _ = workshop;
                (
                    "Queued: the change is being made in the background — the code rewritten, built and tested — \
and installs the next time MapleSyrup starts. Tell the player it takes a few minutes and you'll say when it's ready."
                        .into(),
                    Some(Effect::Rewrite(instruction)),
                )
            }
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
            "set_coaching" => {
                let on = args["on"].as_bool().unwrap_or(true);
                (
                    if on {
                        "Coaching on: you'll speak up on your own again."
                    } else {
                        "Coaching off: from now on you speak only when spoken to (and for low HP or MP)."
                    }
                    .into(),
                    Some(Effect::Command(
                        if on { COACH_ON } else { COACH_OFF }.into(),
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

/// The words of `text`, lower case, letters and digits only ("don't" is
/// "dont", "23%" is "23").
fn plain(text: &str) -> Vec<String> {
    text.to_lowercase()
        .replace(['\'', '’', '‘', '׳'], "")
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(String::from)
        .collect()
}

/// Whether `word` is `part`, or `part` with the one-letter prefixes Hebrew
/// joins to a word ("כשהחיים" is "חיים").
fn is_word(word: &str, part: &str) -> bool {
    if word == part {
        return true;
    }
    let hebrew = part
        .chars()
        .next()
        .is_some_and(|c| ('\u{05d0}'..='\u{05ea}').contains(&c));
    hebrew
        && word.strip_suffix(part).is_some_and(|lead| {
            !lead.is_empty()
                && lead.chars().count() <= 3
                && lead.chars().all(|c| "הובלמשכ".contains(c))
        })
}

/// Where `phrase` (plain words) is in `words`, as whole words, each time.
fn found(words: &[String], phrase: &str) -> Vec<usize> {
    let parts = plain(phrase);
    if parts.is_empty() || parts.len() > words.len() {
        return Vec::new();
    }
    (0..=words.len() - parts.len())
        .filter(|&at| {
            parts
                .iter()
                .enumerate()
                .all(|(k, p)| is_word(&words[at + k], p))
        })
        .collect()
}

/// Words that turn what follows them around ("don't stop", "אל תפסיק").
const NEGATIONS: &[&str] = &["dont", "not", "never", "no", "אל", "לא"];

/// Whether `words` say one of `phrases` — with `unnegated`, not right after
/// a negation ("don't stop talking" asks it to go on; "no more MP
/// warnings" still asks about warnings).
fn says(words: &[String], phrases: &[&str], unnegated: bool) -> bool {
    phrases.iter().any(|p| {
        found(words, p).into_iter().any(|at| {
            !unnegated
                || !words[at.saturating_sub(2)..at]
                    .iter()
                    .any(|w| NEGATIONS.contains(&w.as_str()))
        })
    })
}

/// Words that ask for a warning.
const WARN: &[&str] = &[
    "warn",
    "warning",
    "warnings",
    "alert",
    "alerts",
    "notify",
    "remind",
    "tell me when",
    "let me know when",
    "תזהיר",
    "אזהרה",
    "אזהרות",
    "התראה",
    "התראות",
    "תתריע",
    "תודיע",
];
const HP_WORDS: &[&str] = &["hp", "health", "life", "חיים"];
const MP_WORDS: &[&str] = &["mp", "mana", "מאנה", "מנה"];

/// What asks it to stop speaking up on its own, and to start again.
const ASKS_COACH_OFF: &[&str] = &[
    "stop coaching",
    "no more tips",
    "no tips",
    "only talk when i ask",
    "only when i ask",
    "only speak when",
    "stop talking",
    "stop commenting",
    "stop the tips",
    "stop giving tips",
    "no more advice",
    "dont talk unless",
    "dont speak unless",
    "be quiet",
    "shut up",
    "keep quiet",
    "quiet",
    "תפסיק לדבר",
    "די לדבר",
    "שקט",
    "תשתוק",
    "רק כשאני שואל",
    "בלי טיפים",
    "תפסיק להעיר",
    "אל תדבר",
];
const ASKS_COACH_ON: &[&str] = &[
    "coach me",
    "start coaching",
    "coaching on",
    "coach again",
    "tips again",
    "give me tips",
    "talk on your own",
    "speak up",
    "you can talk",
    "תחזור לדבר",
    "תתחיל לדבר",
    "טיפים",
];
/// What asks it to be quiet altogether, and to speak again.
const MUTE: &[&str] = &[
    "mute",
    "be quiet",
    "shut up",
    "quiet",
    "silence",
    "stop talking",
    "שקט",
    "תשתוק",
    "השתק",
];
const UNMUTE: &[&str] = &[
    "unmute",
    "talk again",
    "speak again",
    "you can talk",
    "תדבר שוב",
    "תחזור לדבר",
    "בטל השתקה",
];
const RECORD: &[&str] = &["record", "recording", "הקלט", "תקליט", "הקלטה"];
/// What asks it to remember something for good.
const REMEMBER: &[&str] = &[
    "remember",
    "keep in mind",
    "dont forget",
    "note that",
    "write down",
    "write that",
    "save that",
    "תזכור",
    "זכור",
    "תרשום",
    "אל תשכח",
];
/// What says it got something wrong.
const CORRECTING: &[&str] = &[
    "no",
    "not",
    "nope",
    "wrong",
    "incorrect",
    "actually",
    "mistake",
    "isnt",
    "arent",
    "wasnt",
    "werent",
    "dont",
    "doesnt",
    "didnt",
    "cant",
    "wont",
    "wouldnt",
    "never",
    "stop",
    "remember",
    "לא",
    "אין",
    "טעית",
    "טעות",
    "תזכור",
    "אל",
    "די",
    "תפסיק",
];
/// Numbers said as words.
const NUMBER_WORDS: &[(&str, i64)] = &[
    ("five", 5),
    ("ten", 10),
    ("fifteen", 15),
    ("twenty", 20),
    ("thirty", 30),
    ("forty", 40),
    ("fifty", 50),
    ("sixty", 60),
    ("seventy", 70),
    ("eighty", 80),
    ("ninety", 90),
    ("חמש", 5),
    ("עשר", 10),
    ("עשרה", 10),
    ("עשרים", 20),
    ("שלושים", 30),
    ("ארבעים", 40),
    ("חמישים", 50),
    ("שישים", 60),
    ("שבעים", 70),
    ("שמונים", 80),
    ("תשעים", 90),
];

/// What says a value of his own ("I'm level 61", "my HP is 500").
const OWN: &[&str] = &["im", "i am", "my", "its", "אני", "שלי"];

/// Whether `heard` plainly asks for what `name` (a tool that changes a
/// setting or the notebook) would do with `args`: a verb and its subject —
/// "warn me when MP is under 30%", "stop talking", "remember that…", "no,
/// I'm level 61". The reason it doesn't, when it doesn't. Tools that change
/// nothing (look closer, look it up) are never refused.
pub fn refused(name: &str, args: &Value, heard: &str) -> Option<String> {
    let words = plain(heard);
    // (A request to stop or start: not after a negation.)
    let any = |list: &[&str]| says(&words, list, false);
    let asks = |list: &[&str]| says(&words, list, true);
    // (A number said as digits, or as a word: "thirty", "ארבעים".)
    let has_number = |n: f64| {
        let n = n.round() as i64;
        words.iter().any(|w| {
            *w == n.to_string()
                || NUMBER_WORDS
                    .iter()
                    .any(|(word, value)| *value == n && is_word(w, word))
        })
    };
    match name {
        "set_warnings" => {
            let what = args["what"].as_str().unwrap_or("");
            let bar = if what == "mp" { MP_WORDS } else { HP_WORDS };
            if !any(WARN) || !any(bar) {
                return Some(format!(
                    "his sentence doesn't ask to be warned about {}",
                    what.to_uppercase()
                ));
            }
            match args["below"].as_f64() {
                Some(b) if b > 0.0 && !has_number(b) => {
                    Some(format!("he didn't say {b:.0}% for the warning"))
                }
                _ => None,
            }
        }
        "set_coaching" => {
            let on = args["on"].as_bool().unwrap_or(true);
            // A phrase ("only talk when I ask"), or a verb and the tips
            // ("stop giving me tips", "start the tips again").
            let tips = any(&[
                "tips",
                "tip",
                "advice",
                "coaching",
                "commentary",
                "טיפים",
                "עצות",
            ]);
            let verb = if on {
                asks(&["start", "again", "back", "resume", "תחזור", "תתחיל"])
            } else {
                asks(&["stop", "no", "quit", "enough", "תפסיק", "די", "בלי"])
            };
            let asked = asks(if on { ASKS_COACH_ON } else { ASKS_COACH_OFF }) || (tips && verb);
            (!asked).then(|| {
                format!(
                    "his sentence doesn't ask you to {} speaking up on your own",
                    if on { "start" } else { "stop" }
                )
            })
        }
        "set_muted" => {
            let muted = args["muted"].as_bool().unwrap_or(true);
            (!asks(if muted { MUTE } else { UNMUTE })).then(|| {
                format!(
                    "his sentence doesn't ask you to {}",
                    if muted { "be quiet" } else { "talk again" }
                )
            })
        }
        "set_recording" => (!asks(RECORD)).then(|| "his sentence doesn't ask to record".into()),
        "remember_fact" => {
            let fact = args["fact"].as_str().unwrap_or("");
            if super::memory::names_the_player(fact) {
                Some("his name comes only from his own file, never from what you hear".into())
            } else {
                (!asks(REMEMBER)).then(|| "his sentence doesn't ask you to remember it".into())
            }
        }
        "note_correction" => {
            let about = args["about"].as_str().unwrap_or("");
            let right = args["right"].as_str().unwrap_or("");
            if super::memory::names_the_player(about) || super::memory::names_the_player(right) {
                Some("his name comes only from his own file, never from what you hear".into())
            } else {
                (!any(CORRECTING)).then(|| "his sentence doesn't correct you".into())
            }
        }
        "correct_reading" => {
            let what = args["what"].as_str().unwrap_or("");
            let value = args["value"].as_str().unwrap_or("");
            if !any(CORRECTING) && !any(OWN) {
                return Some(format!("his sentence doesn't correct your {what}"));
            }
            let number = value
                .split(|c: char| !c.is_ascii_digit())
                .find(|n| !n.is_empty());
            match number {
                Some(n)
                    if matches!(what, "level" | "hp" | "mp" | "exp")
                        && !words.iter().any(|w| w == n) =>
                {
                    Some(format!("he didn't say {n}"))
                }
                _ => None,
            }
        }
        "forget_thing" => (!asks(&["forget", "delete", "remove", "תשכח", "מחק", "תמחק"]))
            .then(|| "his sentence doesn't ask you to forget it".into()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toolbox(dir: &std::path::Path) -> Toolbox {
        Toolbox {
            workshop: None,
            sight: Arc::new(Mutex::new(Sight::load(&dir.join("learned")))),
            eyes: Arc::new(OpenAi::new(
                "sk-test-key-0123456789abcdef",
                "http://127.0.0.1:9/v1",
                "cedar",
                None,
            )),
            settings: dir.to_path_buf(),
            web: false,
            learning: Some(Learning::load(dir)),
        }
    }

    fn call(name: &str, arguments: Value) -> Call {
        Call {
            call_id: "c1".into(),
            name: name.into(),
            arguments: arguments.to_string(),
        }
    }

    #[test]
    fn a_correction_is_kept_for_good() {
        let dir = std::env::temp_dir().join(format!("ms-tools-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let tools = toolbox(&dir);
        let names: Vec<String> = tools
            .definitions()
            .iter()
            .filter_map(|t| t["name"].as_str().map(String::from))
            .collect();
        assert!(names.contains(&"note_correction".to_string()));
        assert!(names.contains(&"set_warnings".to_string()));
        let (said, effect) = tools.run(
            &call(
                "note_correction",
                json!({"about": "Easy Zakum level", "right": "Easy Zakum needs level 50."}),
            ),
            None,
        );
        assert!(said.starts_with("Kept for good"));
        assert_eq!(
            effect,
            Some(Effect::Note(
                "learned: Easy Zakum level: Easy Zakum needs level 50.".into()
            ))
        );
        let learning = tools.learning.as_ref().unwrap();
        assert_eq!(
            learning.knowledge().lessons(5)[0].answer,
            "Easy Zakum needs level 50."
        );
        // Kept on disk too.
        assert_eq!(Learning::load(&dir).knowledge().lessons(5).len(), 1);
        assert_eq!(
            tools
                .run(
                    &call("note_correction", json!({"about": "", "right": "x"})),
                    None
                )
                .1,
            None
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn warnings_move_when_the_player_asks() {
        let dir = std::env::temp_dir().join(format!("ms-tools-warn-{}", std::process::id()));
        let tools = toolbox(&dir);
        let warn = |what: &str, below: Value| {
            tools.run(
                &call("set_warnings", json!({"what": what, "below": below})),
                None,
            )
        };
        assert_eq!(
            warn("hp", json!(40)),
            (
                "You'll be warned when HP is under 40%.".into(),
                Some(Effect::Warn {
                    what: "hp".into(),
                    below: Some(40.0)
                })
            )
        );
        assert_eq!(warn("mp", json!(0)).0, "No more MP warnings.");
        assert_eq!(
            warn("hp", Value::Null).1,
            Some(Effect::Warn {
                what: "hp".into(),
                below: None
            })
        );
        assert_eq!(
            warn("hp", json!(400)).1,
            Some(Effect::Warn {
                what: "hp".into(),
                below: Some(95.0)
            })
        );
        assert_eq!(warn("exp", json!(10)).1, None);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// The owner's real session (2026-10-10): what the model did on his
    /// misheard or echoed words, and what plainly asks.
    #[test]
    fn settings_and_the_notebook_change_only_when_he_plainly_asks() {
        let refused_on = |name: &str, args: Value, heard: &str| refused(name, &args, heard);
        // 12:21:22 "23% still owe" (its own warning heard back) became "MP
        // warnings below 23%"; 12:14:33 "Don't stop… until I say stop"
        // turned coaching off.
        for (name, args, heard) in [
            (
                "set_warnings",
                json!({"what": "mp", "below": 23}),
                "23% still owe",
            ),
            ("set_warnings", json!({"what": "mp", "below": 23}), "MP 23%"),
            (
                "set_coaching",
                json!({"on": false}),
                "Don't stop don't stop don't stop congratulate me until I say stop",
            ),
            ("set_coaching", json!({"on": false}), "don't stop talking"),
            (
                "set_muted",
                json!({"muted": true}),
                "Continue without stopping until I say stop",
            ),
            // Names heard are never kept.
            (
                "note_correction",
                json!({"about": "Player name", "right": "The player said their name is Armani."}),
                "Armani",
            ),
            (
                "note_correction",
                json!({"about": "Player's name", "right": "The player's name is Mikael."}),
                "No, my name is Mikael",
            ),
            (
                "remember_fact",
                json!({"fact": "The player's name is Miha, not Armani."}),
                "I'll Miha",
            ),
            // Not asked to remember, nor corrected.
            (
                "remember_fact",
                json!({"fact": "Armani wants to earn money in MapleStory."}),
                "I need money",
            ),
            (
                "remember_fact",
                json!({"fact": "The player's character is named WANWANBUJIO."}),
                "that's me",
            ),
            (
                "note_correction",
                json!({"about": "Slime Shoes crafting materials", "right": "Slime Shoes need 20 Slime Bubbles."}),
                "Bubble shell",
            ),
            (
                "correct_reading",
                json!({"what": "level", "value": "16"}),
                "How do I get to level 20 from quest fastest",
            ),
        ] {
            assert!(
                refused_on(name, args.clone(), heard).is_some(),
                "{name} {args} on {heard:?}"
            );
        }
        for (name, args, heard) in [
            (
                "set_warnings",
                json!({"what": "mp", "below": 30}),
                "warn me when my MP is under 30%",
            ),
            (
                "set_warnings",
                json!({"what": "mp", "below": 0}),
                "no more MP warnings",
            ),
            (
                "set_warnings",
                json!({"what": "hp", "below": null}),
                "HP warnings like before",
            ),
            (
                "set_warnings",
                json!({"what": "hp", "below": 40}),
                "תזהיר אותי כשהחיים מתחת ל-40",
            ),
            ("set_coaching", json!({"on": false}), "only talk when I ask"),
            (
                "set_coaching",
                json!({"on": false}),
                "stop talking unless I ask you something",
            ),
            ("set_coaching", json!({"on": false}), "תפסיק לדבר"),
            (
                "set_coaching",
                json!({"on": false}),
                "you can stop giving me tips",
            ),
            (
                "set_coaching",
                json!({"on": true}),
                "start giving me tips again",
            ),
            ("set_muted", json!({"muted": true}), "shut up for a bit"),
            (
                "remember_fact",
                json!({"fact": "They use lemons to restore MP."}),
                "I don't have I use lemons remember that",
            ),
            (
                "note_correction",
                json!({"about": "level-up announcements", "right": "Do not announce level-ups."}),
                "don't tell me about my level nothing OK from now on remember to don't do not tell me that",
            ),
            (
                "correct_reading",
                json!({"what": "level", "value": "61"}),
                "no I'm level 61",
            ),
            (
                "look_closer",
                json!({"box": [0, 0, 10, 10], "question": "map"}),
                "where am I",
            ),
        ] {
            assert_eq!(
                refused_on(name, args.clone(), heard),
                None,
                "{name} {args} on {heard:?}"
            );
        }
        // Refused, nothing changes and the log says so.
        let dir = std::env::temp_dir().join(format!("ms-tools-plain-{}", std::process::id()));
        let tools = toolbox(&dir);
        let (said, effect) = tools.run_heard(
            &call("set_coaching", json!({"on": false})),
            None,
            "Don't stop don't stop don't stop congratulate me until I say stop",
        );
        assert!(said.starts_with("Not changed"), "{said}");
        assert!(said.contains("ask him to say it plainly"));
        assert!(
            matches!(effect, Some(Effect::Note(line)) if line.starts_with("not changed (set_coaching)"))
        );
        let (_, effect) = tools.run_heard(
            &call("set_warnings", json!({"what": "mp", "below": 23})),
            None,
            "23% still owe",
        );
        assert!(!matches!(effect, Some(Effect::Warn { .. })));
        // Plainly asked: done.
        let (_, effect) = tools.run_heard(
            &call("set_coaching", json!({"on": false})),
            None,
            "only talk when I ask",
        );
        assert_eq!(effect, Some(Effect::Command(COACH_OFF.into())));
        let _ = std::fs::remove_dir_all(dir);
    }
}
