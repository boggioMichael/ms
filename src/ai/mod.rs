//! Talking like a person: replies come from an OpenAI model that is given
//! what the vision engine sees, and are spoken in OpenAI's natural voice.
//!
//! ```text
//!   phone: "can you see my game?"
//!      │
//!      ▼            snapshot of the game (HP, MP, EXP, level, EXP/h)
//!   Worker: ask(persona + snapshot + conversation + the screen)
//!      │  the reply arrives a few words at a time and is cut into sentences
//!      ├──▶ the first sentence ──▶ speech, streamed ──▶ the PC's speakers
//!      ├──▶ the rest, together ──▶ speech, streamed     (the game ducked)
//!      ▼                                                and/or the phone
//!   Text ──▶ phone screen, console (once the reply is complete)
//! ```
//!
//! The first words are heard while the rest is still being written, and
//! the voice plays while it is still being made. Every job has a number;
//! the player talking over MapleSyrup calls its work off (`Worker::cancel`):
//! the request in flight is stopped at once, and only what was already
//! said stays in the conversation.
//!
//! Without a key, or when OpenAI cannot be reached, MapleSyrup falls back
//! to its own answers and the Windows voice.

pub mod brain;
pub mod images;
pub mod language;
pub mod live;
pub mod openai;
pub mod teaching;
pub mod tools;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use image::RgbaImage;
use serde_json::{Value, json};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

pub use brain::Brain;
pub use images::NBox;
pub use openai::{AiError, OpenAi};
pub use tools::{Effect, Toolbox};

pub use openai::Stop;
use openai::{Ask, Piece, Turn};

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
        /// The player's language setting (a locale such as `he-IL`).
        language: Option<String>,
    },
    /// Say one of MapleSyrup's own lines (a warning, a greeting) in the
    /// natural voice, translated first when the player's language is not
    /// English. With `show`, the line has not been shown yet: it comes back
    /// as `Shown`, in the player's language, to be shown.
    Speak {
        text: String,
        language: Option<String>,
        show: Option<crate::companion::Kind>,
        /// Whether to say it aloud too (a line can be only shown).
        speak: bool,
    },
    /// The last reply was talked over after it was written: only `heard`
    /// of it reached the player, and the conversation keeps only that.
    Cut { heard: String },
}

/// What the worker did. Each carries the number of the job it came from
/// (`Worker::send`), so the work of a job called off can be dropped.
pub enum Done {
    /// The reply's text (its audio comes as `Audio`).
    Reply {
        id: u64,
        heard: String,
        text: String,
        took: Duration,
    },
    /// The model judged the sentence was not for it.
    Silent { id: u64, heard: String },
    /// Speech, a piece at a time as it is made: 24 kHz mono samples. A
    /// reply is spoken in a few lines (the first sentence alone, so it
    /// starts soon; then what was written meanwhile, together, so it
    /// flows). `start` opens a line (its words are in `text`), `end`
    /// closes it (no samples). `after`: since the job was handed over;
    /// `first`: the first line of a reply.
    Audio {
        id: u64,
        text: String,
        samples: Vec<i16>,
        after: Duration,
        first: bool,
        start: bool,
        end: bool,
    },
    Failed {
        id: u64,
        heard: Option<String>,
        error: AiError,
    },
    /// A tool changed something: learned, forgot or corrected (a line for
    /// the log and the phone).
    Noted { line: String },
    /// One of MapleSyrup's own lines, in the player's language, to show.
    Shown {
        kind: crate::companion::Kind,
        text: String,
    },
    /// The model asked for a command (mark, mute, unmute) the main loop runs.
    Command { word: String },
}

pub struct Worker {
    jobs: Sender<(u64, Job)>,
    pub done: Receiver<Done>,
    busy: Arc<AtomicBool>,
    pub model: Arc<std::sync::Mutex<Option<String>>>,
    /// The number the next job gets (from 1).
    next: AtomicU64,
    /// Jobs numbered up to this are called off.
    mark: Arc<AtomicU64>,
}

impl Worker {
    /// Hand it a job. Returns the job's number, which its `Done`s carry.
    pub fn send(&self, job: Job) -> u64 {
        let id = self.next.fetch_add(1, Ordering::SeqCst);
        let _ = self.jobs.send((id, job));
        id
    }

    /// Call off job `id` and every job before it: a request in flight is
    /// stopped, speech being made stops, what waits is skipped.
    pub fn cancel(&self, id: u64) {
        self.mark.fetch_max(id, Ordering::SeqCst);
    }

    /// Call off everything handed over so far.
    pub fn cancel_all(&self) {
        let last = self.next.load(Ordering::SeqCst).saturating_sub(1);
        self.cancel(last);
    }

    /// Whether job `id` was called off.
    pub fn cancelled(&self, id: u64) -> bool {
        self.mark.load(Ordering::SeqCst) >= id
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
    let (jobs, rx) = channel::<(u64, Job)>();
    let (tx, done) = channel::<Done>();
    let busy = Arc::new(AtomicBool::new(false));
    let model = Arc::new(std::sync::Mutex::new(None));
    let mark = Arc::new(AtomicU64::new(0));
    let (busy_flag, model_slot, marks) = (Arc::clone(&busy), Arc::clone(&model), Arc::clone(&mark));
    let _ = std::thread::Builder::new()
        .name("ai".into())
        .spawn(move || {
            // MapleSyrup's own lines come back often ("Level up!"): each is
            // translated once.
            let mut translations = std::collections::HashMap::new();
            while let Ok(first) = rx.recv() {
                busy_flag.store(true, Ordering::Relaxed);
                let mut queue = vec![first];
                while let Ok(more) = rx.try_recv() {
                    queue.push(more);
                }
                // Only the newest of the player's sentences is answered: the
                // main loop folds what came before into it.
                let newest = queue
                    .iter()
                    .rposition(|(_, job)| matches!(job, Job::Converse { .. }));
                for (i, (id, job)) in queue.into_iter().enumerate() {
                    let stop = Stop::new(Arc::clone(&marks), id);
                    if stop.stopped() {
                        continue;
                    }
                    match job {
                        Job::Speak {
                            text,
                            language,
                            show,
                            speak,
                        } => say_line(
                            &openai,
                            id,
                            &stop,
                            &text,
                            language.as_deref(),
                            show,
                            speak,
                            &mut translations,
                            &tx,
                        ),
                        Job::Cut { heard } => brain.cut_short(&heard),
                        Job::Converse { .. } if Some(i) != newest => {}
                        Job::Converse {
                            heard,
                            mut snapshot,
                            speak,
                            eyes,
                            language,
                        } => {
                            if let Some(l) =
                                language.as_deref().filter(|l| !language::is_english(l))
                            {
                                let name = language::name(l);
                                snapshot.push_str(&format!(
                                    "\n(The player's language setting is {name}: answer in the language they speak to you; when it isn't clear, in {name}.)"
                                ));
                            }
                            converse(
                                &openai,
                                toolbox.as_ref(),
                                &mut brain,
                                Talk {
                                    id,
                                    stop: &stop,
                                    heard,
                                    snapshot: &snapshot,
                                    eyes: eyes.as_ref(),
                                    speak,
                                    language: language.as_deref(),
                                },
                                &tx,
                                &busy_flag,
                            );
                            if let Ok(mut slot) = model_slot.lock() {
                                *slot = openai.model();
                            }
                        }
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
        next: AtomicU64::new(1),
        mark,
    }
}

/// One of MapleSyrup's own lines: translated into the player's language
/// when it is not English, shown when `show`, said when `aloud`.
#[allow(clippy::too_many_arguments)]
fn say_line(
    openai: &OpenAi,
    id: u64,
    stop: &Stop,
    text: &str,
    language: Option<&str>,
    show: Option<crate::companion::Kind>,
    aloud: bool,
    translations: &mut std::collections::HashMap<(String, String), String>,
    tx: &Sender<Done>,
) {
    let asked = Instant::now();
    let text = match language {
        Some(l) if !language::is_english(l) => translate(openai, text, l, stop, translations),
        _ => text.to_string(),
    };
    if stop.stopped() {
        return;
    }
    if let Some(kind) = show {
        let _ = tx.send(Done::Shown {
            kind,
            text: text.clone(),
        });
    }
    if !aloud {
        return;
    }
    if let Err(error) = speak_line(openai, id, stop, &text, asked, false, tx) {
        let _ = tx.send(Done::Failed {
            id,
            heard: None,
            error,
        });
    }
}

/// Say `text` in the natural voice, handing it over a piece at a time as it
/// is made. Returns whether any of it was made (it may be called off).
fn speak_line(
    openai: &OpenAi,
    id: u64,
    stop: &Stop,
    text: &str,
    asked: Instant,
    first: bool,
    tx: &Sender<Done>,
) -> Result<bool, AiError> {
    let mut start = true;
    let result = openai.speech_stream(text, brain::VOICE_STYLE, Some(stop), &mut |samples| {
        let _ = tx.send(Done::Audio {
            id,
            text: if start {
                text.to_string()
            } else {
                String::new()
            },
            samples: samples.to_vec(),
            after: asked.elapsed(),
            first: first && start,
            start,
            end: false,
        });
        start = false;
    });
    let made = !start;
    if made {
        let _ = tx.send(Done::Audio {
            id,
            text: String::new(),
            samples: Vec::new(),
            after: asked.elapsed(),
            first: false,
            start: false,
            end: true,
        });
    }
    match result {
        Ok(_) | Err(AiError::Cancelled) => Ok(made),
        Err(error) => Err(error),
    }
}

/// How the model is told it can see the game.
const EYES_GUIDE: &str = "\n\nWith the player's words comes what your vision engine reads off the game right now \
(not said by the player) and, while the game is in view, two pictures: the whole game window as it is now, with \
rulers on its edges (0 to 1000 across and down) for pointing at things, and the HUD at full size, for reading \
small numbers. Use what you see, like a friend looking at the same screen. If the numbers disagree with the \
pictures, trust the pictures (and say so if it matters). Without pictures you can't see the game right now.";

/// How the model is told about its tools.
const TOOLS_GUIDE: &str = "\n\nYou get better the more the player teaches you:
- Only when the player shows or tells you what something on screen is (\"this is...\", \"that's my...\", \"see that? it's...\") or asks you to watch for something, call learn_thing with a tight box around it in the first picture's 0-1000 coordinates. Never learn things on your own. If they want a heads-up (\"tell me when a rune shows up\", \"warn me when the boss is under 20%\"), set alert, threshold and say (what you'll say then, in their language).
- When the player says a value you have is wrong (their level, HP, MP, EXP, map, name, job), call correct_reading.
- When the player tells you something about themselves or their game worth keeping (their class, a key binding, a goal), or asks you to remember something, call remember_fact.
- forget_thing when asked to forget something you learned; look_closer to read small text or details you can't make out.
- mark_moment when the player asks you to mark or save the moment (for their video); set_muted when they ask you to be quiet, or to talk again.
After using a tool, confirm briefly in your own words.";

/// How the model is told it can search the web.
const WEB_GUIDE: &str = "\n- Search the web (once) before answering a MapleStory question you aren't sure of and can't see \
on screen (how to get somewhere, boss or level requirements, job advancements, key bindings, quests, events, \
training spots for their level): the current global version (GMS) changes often. Prefer maplestorywiki.net and \
maplestory.nexon.net. Don't search for what is on screen or for small talk: searching takes the player's time. \
Never put links, sources or citations in your answer: it is spoken aloud.";

/// One of MapleSyrup's own lines in the player's language (as it is, when
/// it can't be translated).
fn translate(
    openai: &OpenAi,
    text: &str,
    locale: &str,
    stop: &Stop,
    cache: &mut std::collections::HashMap<(String, String), String>,
) -> String {
    // Already written in another script (a line the model made in the
    // player's language, such as an alert they asked for).
    if !text.chars().any(|c| c.is_ascii_alphabetic()) {
        return text.to_string();
    }
    let key = (locale.to_string(), text.to_string());
    if let Some(done) = cache.get(&key) {
        return done.clone();
    }
    let name = language::name(locale);
    let ask = Ask {
        instructions: format!(
            "Translate what a friendly companion app says out loud to someone playing MapleStory into {name}. \
Keep it short, casual and natural, as a friend would say it; keep the numbers, and game words the way players \
say them. Reply with the translation only."
        ),
        input: vec![json!({"role": "user", "content": text})],
        max_output_tokens: 150,
        timeout: Duration::from_secs(20),
        stop: Some(stop.clone()),
        ..Default::default()
    };
    match openai.ask(&ask, None) {
        Ok(answer) if !answer.text.trim().is_empty() => {
            let done = brain::for_speech(&answer.text);
            if cache.len() > 200 {
                cache.clear();
            }
            cache.insert(key, done.clone());
            done
        }
        _ => text.to_string(),
    }
}

/// What to say while the web is searched, in the player's language (the
/// one they spoke in, else their setting).
fn searching_line(heard: &str, language: Option<&str>) -> &'static str {
    let script = |range: std::ops::RangeInclusive<char>| heard.chars().any(|c| range.contains(&c));
    let code = if script('\u{0590}'..='\u{05FF}') {
        "he"
    } else if script('\u{AC00}'..='\u{D7AF}') {
        "ko"
    } else if script('\u{3040}'..='\u{30FF}') {
        "ja"
    } else if script('\u{0E00}'..='\u{0E7F}') {
        "th"
    } else if script('\u{0400}'..='\u{04FF}') {
        "ru"
    } else if script('\u{4E00}'..='\u{9FFF}') {
        match language {
            Some(l) if l.starts_with("ja") => "ja",
            Some(l) if l.contains("TW") || l.contains("HK") || l.contains("Hant") => "zh-Hant",
            _ => "zh",
        }
    } else {
        match language.map(|l| l.split(['-', '_']).next().unwrap_or("")) {
            Some("es") => "es",
            Some("pt") => "pt",
            Some("fr") => "fr",
            Some("de") => "de",
            Some("vi") => "vi",
            Some("id" | "in") => "id",
            _ => "en",
        }
    };
    match code {
        "he" => "רגע, בודק.",
        "ko" => "잠깐, 찾아볼게.",
        "ja" => "ちょっと調べるね。",
        "th" => "แป๊บนึง ขอเช็กก่อนนะ",
        "ru" => "Секунду, гляну.",
        "zh" => "等一下，我查查。",
        "zh-Hant" => "等一下，我查查。",
        "es" => "Espera, lo busco.",
        "pt" => "Peraí, vou ver.",
        "fr" => "Attends, je regarde.",
        "de" => "Moment, ich schau nach.",
        "vi" => "Đợi chút, để mình xem.",
        "id" => "Bentar, aku cek dulu.",
        _ => "Hang on, let me check.",
    }
}

/// The conversation as input items. With the player's last sentence goes
/// what is on screen now (and the screen itself), so everything before it
/// stays the same from one reply to the next and OpenAI keeps it cached.
fn input_of(turns: &[openai::Turn], snapshot: &str, eyes: Option<&Eyes>) -> Vec<Value> {
    let last_user = turns.iter().rposition(|t| t.role == "user");
    turns
        .iter()
        .enumerate()
        .map(|(i, t)| {
            if Some(i) != last_user {
                return json!({"role": t.role, "content": t.text});
            }
            let mut content = vec![json!({
                "type": "input_text",
                "text": format!("[The game right now, read by your vision engine — not said by the player]\n{snapshot}"),
            })];
            if let Some(eyes) = eyes {
                content.extend(eyes.pictures());
            }
            content.push(json!({"type": "input_text", "text": t.text}));
            json!({"role": "user", "content": content})
        })
        .collect()
}

/// What the player said, and what goes with it.
struct Talk<'a> {
    id: u64,
    stop: &'a Stop,
    heard: String,
    snapshot: &'a str,
    eyes: Option<&'a Eyes>,
    speak: bool,
    language: Option<&'a str>,
}

/// Answer the player. The reply is streamed and cut into sentences; they
/// are turned into speech (on a second thread) as soon as they are complete:
/// the first sentence alone, so it is heard soon, then whatever was written
/// meanwhile in one piece, so it flows. When the model calls tools, they
/// are run and their results handed back for it to go on, up to a few
/// rounds. Called off (the player talked over it), it stops at once and
/// keeps in the conversation only what was said.
fn converse(
    openai: &OpenAi,
    toolbox: Option<&Toolbox>,
    brain: &mut Brain,
    talk: Talk,
    tx: &Sender<Done>,
    busy: &AtomicBool,
) {
    let Talk {
        id,
        stop,
        heard,
        snapshot,
        eyes,
        speak,
        language,
    } = talk;
    let started = Instant::now();
    let mut instructions = brain.persona();
    instructions.push_str(EYES_GUIDE);
    if let Some(toolbox) = toolbox {
        instructions.push_str(TOOLS_GUIDE);
        if toolbox.web {
            instructions.push_str(WEB_GUIDE);
        }
    }
    // The player's sentence joins the conversation once it is answered (or
    // was talked over after part of the answer was said).
    let mut turns = brain.turns();
    turns.push(Turn {
        role: "user",
        text: heard.clone(),
    });
    let mut input = input_of(&turns, snapshot, eyes);
    let tools = toolbox.map(|t| t.definitions()).unwrap_or_default();
    std::thread::scope(|scope| {
        let (lines, to_say) = channel::<String>();
        let voice = speak.then(|| {
            let tx = tx.clone();
            scope.spawn(move || {
                // What was said aloud, for the conversation.
                let mut spoken = String::new();
                let mut first = true;
                while let Ok(line) = to_say.recv() {
                    // The first sentence goes alone, so it is heard soon;
                    // after it, what was written while the last line was
                    // being said goes in one piece, so it flows.
                    let mut text = line;
                    while !first && let Ok(more) = to_say.try_recv() {
                        text.push(' ');
                        text.push_str(&more);
                    }
                    if stop.stopped() {
                        break;
                    }
                    match speak_line(openai, id, stop, &text, started, first, &tx) {
                        Ok(true) => {
                            first = false;
                            if !spoken.is_empty() {
                                spoken.push(' ');
                            }
                            spoken.push_str(&text);
                        }
                        Ok(false) => {}
                        Err(error) => {
                            let _ = tx.send(Done::Failed {
                                id,
                                heard: None,
                                error,
                            });
                            break;
                        }
                    }
                }
                spoken
            })
        });
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
                stop: Some(stop.clone()),
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
                            let _ = lines.send(searching_line(&heard, language).to_string());
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
                    Some(Effect::Command(word)) => {
                        let _ = tx.send(Done::Command { word });
                    }
                    None => {}
                }
                outputs.push(json!({"type": "function_call_output", "call_id": call.call_id, "output": output}));
            }
            input.extend(openai::follow_up(&answer));
            input.extend(outputs);
            if stop.stopped() {
                failed = Some(AiError::Cancelled);
                break;
            }
        }
        if stop.stopped() {
            failed = Some(AiError::Cancelled);
        }
        match failed {
            Some(AiError::Cancelled) => {
                // Talked over: what was said aloud stays, cut off where it was.
                drop(lines);
                let spoken = voice
                    .map(|v| v.join().unwrap_or_default())
                    .unwrap_or_default();
                if !spoken.trim().is_empty() {
                    brain.heard(&heard);
                    brain.said(&format!("{}…", spoken.trim_end_matches(['.', ' '])));
                }
            }
            Some(error) => {
                brain.heard(&heard);
                let _ = tx.send(Done::Failed {
                    id,
                    heard: Some(heard),
                    error,
                });
            }
            None if brain::is_silent(&said) => {
                brain.heard(&heard);
                let _ = tx.send(Done::Silent { id, heard });
            }
            None => {
                if speak && let Some(rest) = sentences.finish() {
                    let _ = lines.send(brain::for_speech(&rest));
                }
                let text = brain::for_speech(&said);
                brain.heard(&heard);
                brain.said(&text);
                let _ = tx.send(Done::Reply {
                    id,
                    heard,
                    text,
                    took: started.elapsed(),
                });
            }
        }
        // The words are out; the voice may still be on its last lines.
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
