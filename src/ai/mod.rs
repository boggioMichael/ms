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
pub mod eleven;
pub mod images;
pub mod knowledge;
pub mod language;
pub mod live;
pub mod lookup;
pub mod memory;
pub mod openai;
pub mod style;
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
pub use memory::Learning;
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

/// A key for another service: from the environment variable `env`, from
/// `file` next to MapleSyrup (moved into the settings folder, which no
/// desktop sync or screen share shows), or from the settings folder.
fn load_secret(settings: &Path, env: &str, file: &str, prefix: &str) -> Option<String> {
    let valid = |text: &str| {
        let t = clean_key(text);
        t.starts_with(prefix) && t.len() >= 20 && !t.contains(char::is_whitespace)
    };
    if let Ok(key) = std::env::var(env)
        && valid(&key)
    {
        return Some(clean_key(&key).to_string());
    }
    let kept = settings.join(file);
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        let beside = dir.join(file);
        if let Ok(key) = std::fs::read_to_string(&beside)
            && valid(&key)
        {
            let key = clean_key(&key).to_string();
            if std::fs::create_dir_all(settings).is_ok() && std::fs::write(&kept, &key).is_ok() {
                let _ = std::fs::remove_file(&beside);
            }
            return Some(key);
        }
    }
    std::fs::read_to_string(kept)
        .ok()
        .filter(|k| valid(k))
        .map(|k| clean_key(&k).to_string())
}

/// The xAI key (Grok): `XAI_API_KEY`, or `xai-key.txt`.
pub fn load_xai_key(settings: &Path) -> Option<String> {
    load_secret(settings, "XAI_API_KEY", "xai-key.txt", "xai-")
}

/// The ElevenLabs key (voices): `ELEVENLABS_API_KEY`, or
/// `elevenlabs-key.txt`.
pub fn load_elevenlabs_key(settings: &Path) -> Option<String> {
    load_secret(settings, "ELEVENLABS_API_KEY", "elevenlabs-key.txt", "sk_")
}

/// Grok's API, and its fast model (no reasoning: it answers at once).
pub const XAI_BASE: &str = "https://api.x.ai/v1";
pub const GROK_MODEL: &str = "grok-4.3";

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
    /// The picture for the model: the whole frame, small and at low detail
    /// (quick to send and to look at), with rulers to point at things. The
    /// numbers come read already; `look_closer` reads small print.
    pub fn pictures(&self) -> Vec<Value> {
        // (At low detail OpenAI looks at 512 pixels across at most.)
        let frame = images::with_rulers(&images::fit(&self.frame, 640, 400));
        vec![images::input_image(images::jpeg_url(&frame, 60), "low")]
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
    /// as `Shown`, in the player's language, to be shown. An alert's line
    /// (`kind`) is not called off with the rest: a warning must not vanish
    /// because the player spoke over something else.
    Speak {
        text: String,
        language: Option<String>,
        kind: crate::companion::Kind,
        show: bool,
        /// Whether to say it aloud too (a line can be only shown).
        speak: bool,
    },
    /// The last reply was talked over after it was written: only `heard`
    /// of it reached the player, and the conversation keeps only that.
    Cut { heard: String },
    /// The phone just connected: say hi, picking up from what it knows of
    /// the player (shown and spoken, in their language).
    Greet { language: Option<String> },
    /// Say `text` as MapleSyrup (already in the player's language): shown,
    /// spoken, and kept in the conversation (with `heard`, what it answers,
    /// when it answers something).
    Say { heard: Option<String>, text: String },
    /// Nobody said anything: the coach is watching the game and asks
    /// whether there is one line worth saying now (`reason`: why it asks,
    /// `label`: the same in a few words for the log), given what is on
    /// screen and what it said on its own lately (`said`). The line is
    /// said here when `speak` (not on a live call: the call says it).
    Coach {
        reason: String,
        label: String,
        snapshot: String,
        eyes: Option<Eyes>,
        said: Vec<String>,
        language: Option<String>,
        speak: bool,
    },
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
    /// The player asked MapleSyrup to change its own program: the
    /// workshop's job.
    Rewrite { instruction: String },
    /// The player asked to be warned at another HP or MP (`below`: the
    /// percent, 0 for never, None for the usual).
    Warn { what: String, below: Option<f32> },
    /// Look `question` up in the background: `said` was the quick answer,
    /// `asked` whether the player asked for the look-up.
    LookUp {
        question: String,
        said: String,
        asked: bool,
        language: Option<String>,
    },
    /// The coach looked: the line it has to say (None: nothing worth
    /// saying, or `error`), and how long the look took.
    Coached {
        id: u64,
        label: String,
        text: Option<String>,
        error: Option<String>,
        took: Duration,
    },
}

impl Job {
    /// Whether no call-off stops it: an alert's own line.
    fn kept(&self) -> bool {
        matches!(
            self,
            Job::Speak {
                kind: crate::companion::Kind::Alert,
                ..
            }
        )
    }
}

/// How many of the jobs no call-off stops are remembered by number: an
/// alert's voice is over within seconds of its job, so the last few are
/// all that can still be asked about.
const KEPT_REMEMBERED: usize = 64;

pub struct Worker {
    jobs: Sender<(u64, Job)>,
    pub done: Receiver<Done>,
    busy: Arc<AtomicBool>,
    pub model: Arc<std::sync::Mutex<Option<String>>>,
    /// The number the next job gets (from 1).
    next: AtomicU64,
    /// Jobs numbered up to this are called off.
    mark: Arc<AtomicU64>,
    /// The jobs the mark does not stop (`Job::kept`), by number.
    kept: Arc<std::sync::Mutex<Vec<u64>>>,
}

impl Worker {
    /// Hand it a job. Returns the job's number, which its `Done`s carry.
    pub fn send(&self, job: Job) -> u64 {
        let id = self.next.fetch_add(1, Ordering::SeqCst);
        if job.kept()
            && let Ok(mut kept) = self.kept.lock()
        {
            kept.push(id);
            if kept.len() > KEPT_REMEMBERED {
                kept.remove(0);
            }
        }
        let _ = self.jobs.send((id, job));
        id
    }

    /// Call off job `id` and every job before it: a request in flight is
    /// stopped, speech being made stops, what waits is skipped. (An
    /// alert's line goes on regardless.)
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
        self.mark.load(Ordering::SeqCst) >= id && !is_kept(&self.kept, id)
    }

    /// Whether it is working on something (the phone shows "thinking").
    pub fn busy(&self) -> bool {
        self.busy.load(Ordering::Relaxed)
    }
}

/// Whether job `id` is one no call-off stops.
fn is_kept(kept: &std::sync::Mutex<Vec<u64>>, id: u64) -> bool {
    kept.lock().is_ok_and(|kept| kept.contains(&id))
}

/// After this many failures in a row, the fast brain is given up on.
const FAST_GIVE_UP: u32 = 3;
/// How long the fast brain rests after a reply that was mostly things it
/// had said before.
const FAST_REST: Duration = Duration::from_secs(10 * 60);

/// Start the worker thread (no tools).
pub fn spawn(openai: OpenAi, brain: Brain) -> Worker {
    spawn_with(openai, brain, None)
}

/// Start the worker thread, with the tools the model may use.
pub fn spawn_with(openai: OpenAi, brain: Brain, toolbox: Option<Toolbox>) -> Worker {
    spawn_hybrid(openai, None, brain, toolbox)
}

/// Start the worker thread: `openai` speaks (and answers when there is no
/// `fast` brain, or when it fails); `fast` (Grok) answers the conversation.
pub fn spawn_hybrid(
    openai: OpenAi,
    fast: Option<OpenAi>,
    brain: Brain,
    toolbox: Option<Toolbox>,
) -> Worker {
    spawn_brains(
        Brains {
            openai,
            fast,
            eleven: None,
        },
        brain,
        toolbox,
    )
}

/// What answers and what speaks.
pub struct Brains {
    /// Speaks, translates, and answers when there is no `fast` brain or it
    /// fails.
    pub openai: OpenAi,
    /// Answers the conversation (Grok).
    pub fast: Option<OpenAi>,
    /// Speaks in the voice the player picked (ElevenLabs).
    pub eleven: Option<eleven::Eleven>,
}

/// Start the worker thread with these brains.
pub fn spawn_brains(brains: Brains, mut brain: Brain, toolbox: Option<Toolbox>) -> Worker {
    let Brains {
        openai,
        fast,
        eleven,
    } = brains;
    let (jobs, rx) = channel::<(u64, Job)>();
    let (tx, done) = channel::<Done>();
    let busy = Arc::new(AtomicBool::new(false));
    let model = Arc::new(std::sync::Mutex::new(None));
    let mark = Arc::new(AtomicU64::new(0));
    let kept = Arc::new(std::sync::Mutex::new(Vec::new()));
    let (busy_flag, model_slot, marks, kept_jobs) = (
        Arc::clone(&busy),
        Arc::clone(&model),
        Arc::clone(&mark),
        Arc::clone(&kept),
    );
    let _ = std::thread::Builder::new()
        .name("ai".into())
        .spawn(move || {
            // MapleSyrup's own lines come back often ("Level up!"): each is
            // translated once.
            let mut translations = std::collections::HashMap::new();
            // The fast brain failing again and again is given up on; one
            // going round in circles rests a while.
            let mut fast_failures = 0u32;
            let mut fast_paused_until: Option<Instant> = None;
            // ElevenLabs failing is said once.
            let eleven_failed = AtomicBool::new(false);
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
                    let stop = if is_kept(&kept_jobs, id) {
                        Stop::never()
                    } else {
                        Stop::new(Arc::clone(&marks), id)
                    };
                    if stop.stopped() {
                        // (The coach waits to hear back from every look.)
                        if let Job::Coach { label, .. } = job {
                            let _ = tx.send(Done::Coached {
                                id,
                                label,
                                text: None,
                                error: None,
                                took: Duration::ZERO,
                            });
                        }
                        continue;
                    }
                    // The voice the player picked, when it's ElevenLabs's.
                    let voice_id = brain.voice_id();
                    let mouth = Mouth {
                        openai: &openai,
                        eleven: eleven.as_ref().zip(voice_id.as_deref()),
                        failed: &eleven_failed,
                    };
                    match job {
                        Job::Speak {
                            text,
                            language,
                            kind,
                            show,
                            speak,
                        } => say_line(
                            mouth,
                            id,
                            &stop,
                            Line {
                                text: &text,
                                language: language.as_deref(),
                                show: show.then_some(kind),
                                aloud: speak,
                                style: brain::voice_style(brain.attitude()),
                            },
                            &mut translations,
                            &tx,
                        ),
                        Job::Cut { heard } => brain.cut_short(&heard),
                        Job::Greet { language } => greet(
                            fast.as_ref().unwrap_or(&openai),
                            mouth,
                            &mut brain,
                            id,
                            &stop,
                            language.as_deref(),
                            &tx,
                        ),
                        Job::Say { heard, text } => {
                            let asked = Instant::now();
                            let text = brain::for_speech(&text);
                            if text.is_empty() {
                                continue;
                            }
                            if let Some(heard) = &heard {
                                brain.heard(heard);
                            }
                            brain.said(&text);
                            // Said now: the model saying it again would be twice.
                            for sentence in brain::sentences_of(&text) {
                                brain.recent.fresh(&sentence);
                            }
                            // An answer is a reply like the model's (its turn
                            // ends when it is done); a line of its own is shown.
                            let _ = tx.send(match heard {
                                Some(heard) => Done::Reply {
                                    id,
                                    heard,
                                    text: text.clone(),
                                    took: asked.elapsed(),
                                },
                                None => Done::Shown {
                                    kind: crate::companion::Kind::Reply,
                                    text: text.clone(),
                                },
                            });
                            let style = brain::voice_style(brain.attitude());
                            if let Err(error) =
                                speak_line(mouth, id, &stop, &text, style, Instant::now(), false, &tx)
                            {
                                let _ = tx.send(Done::Failed {
                                    id,
                                    heard: None,
                                    error,
                                });
                            }
                        }
                        Job::Coach {
                            reason,
                            label,
                            snapshot,
                            eyes,
                            said,
                            language,
                            speak,
                        } => {
                            let fast_rested = fast_paused_until.is_none_or(|t| Instant::now() >= t);
                            let chat = fast
                                .as_ref()
                                .filter(|_| fast_failures < FAST_GIVE_UP && fast_rested)
                                .unwrap_or(&openai);
                            coach(
                                chat,
                                mouth,
                                &mut brain,
                                Watch {
                                    id,
                                    stop: &stop,
                                    reason: &reason,
                                    label,
                                    snapshot: &snapshot,
                                    eyes: eyes.as_ref(),
                                    said: &said,
                                    language: language.as_deref(),
                                    speak,
                                },
                                &tx,
                            );
                        }
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
                            let fast_rested = fast_paused_until.is_none_or(|t| Instant::now() >= t);
                            let Replied {
                                used,
                                fast_failed: fell_back,
                                looped,
                            } = converse(
                                mouth,
                                fast.as_ref()
                                    .filter(|_| fast_failures < FAST_GIVE_UP && fast_rested),
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
                                    translations: &mut translations,
                                },
                                &tx,
                                &busy_flag,
                            );
                            if let Ok(mut slot) = model_slot.lock() {
                                *slot = used.model();
                            }
                            // A fast brain going round in circles (the same
                            // lines for every question) rests a while;
                            // OpenAI answers meanwhile.
                            if looped && !std::ptr::eq(used, &openai) {
                                fast_paused_until = Some(Instant::now() + FAST_REST);
                                let _ = tx.send(Done::Noted {
                                    line: format!(
                                        "Grok is repeating itself: OpenAI answers for the next {} minutes",
                                        FAST_REST.as_secs() / 60
                                    ),
                                });
                            }
                            match fell_back {
                                Some(why) => {
                                    fast_failures += 1;
                                    if fast_failures == 1 || fast_failures == FAST_GIVE_UP {
                                        let _ = tx.send(Done::Noted {
                                            line: if fast_failures == FAST_GIVE_UP {
                                                format!("Grok keeps failing ({why}): OpenAI answers from now on")
                                            } else {
                                                format!("Grok didn't answer ({why}): OpenAI did")
                                            },
                                        });
                                    }
                                }
                                None if !std::ptr::eq(used, &openai) => fast_failures = 0,
                                None => {}
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
        kept,
    }
}

/// What speaks: OpenAI's voice, or the ElevenLabs voice the player picked
/// (with OpenAI's standing in when it can't).
#[derive(Clone, Copy)]
struct Mouth<'a> {
    openai: &'a OpenAi,
    eleven: Option<(&'a eleven::Eleven, &'a str)>,
    /// ElevenLabs failed already (said once).
    failed: &'a AtomicBool,
}

/// One of MapleSyrup's own lines, and how to say it.
struct Line<'a> {
    text: &'a str,
    /// The player's language: translated into it when it isn't English.
    language: Option<&'a str>,
    /// Shown (it comes back as `Shown`, translated).
    show: Option<crate::companion::Kind>,
    /// Said aloud too.
    aloud: bool,
    /// How the voice sounds.
    style: &'a str,
}

/// One of MapleSyrup's own lines: translated into the player's language
/// when it is not English, shown, said.
fn say_line(
    mouth: Mouth,
    id: u64,
    stop: &Stop,
    line: Line,
    translations: &mut std::collections::HashMap<(String, String), String>,
    tx: &Sender<Done>,
) {
    let asked = Instant::now();
    let text = match line.language {
        Some(l) if !language::is_english(l) => {
            translate(mouth.openai, line.text, l, stop, translations)
        }
        _ => line.text.to_string(),
    };
    if stop.stopped() {
        return;
    }
    if let Some(kind) = line.show {
        let _ = tx.send(Done::Shown {
            kind,
            text: text.clone(),
        });
    }
    if !line.aloud {
        return;
    }
    if let Err(error) = speak_line(mouth, id, stop, &text, line.style, asked, false, tx) {
        let _ = tx.send(Done::Failed {
            id,
            heard: None,
            error,
        });
    }
}

/// Say `text` in the natural voice, handing it over a piece at a time as it
/// is made. Returns whether any of it was made (it may be called off, and
/// there is nothing to say for an empty line).
#[allow(clippy::too_many_arguments)]
fn speak_line(
    mouth: Mouth,
    id: u64,
    stop: &Stop,
    text: &str,
    style: &str,
    asked: Instant,
    first: bool,
    tx: &Sender<Done>,
) -> Result<bool, AiError> {
    if !text.chars().any(char::is_alphanumeric) {
        return Ok(false);
    }
    let start = std::cell::Cell::new(true);
    let mut send = |samples: &[i16]| {
        let opens = start.replace(false);
        let _ = tx.send(Done::Audio {
            id,
            text: if opens {
                text.to_string()
            } else {
                String::new()
            },
            samples: samples.to_vec(),
            after: asked.elapsed(),
            first: first && opens,
            start: opens,
            end: false,
        });
    };
    let result = match mouth.eleven {
        // ElevenLabs, unless it is resting after failing.
        Some((eleven, voice)) if !eleven.resting() => {
            match eleven.speech_stream(text, voice, Some(stop), &mut send) {
                // ElevenLabs couldn't: OpenAI's voice says it.
                Err(error) if start.get() && !matches!(error, AiError::Cancelled) => {
                    if !mouth.failed.swap(true, Ordering::Relaxed) {
                        let _ = tx.send(Done::Noted {
                            line: format!(
                                "ElevenLabs didn't speak ({}): OpenAI's voice did",
                                error.detail()
                            ),
                        });
                    }
                    mouth
                        .openai
                        .speech_stream(text, style, Some(stop), &mut send)
                }
                other => other,
            }
        }
        _ => mouth
            .openai
            .speech_stream(text, style, Some(stop), &mut send),
    };
    let made = !start.get();
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
(not said by the player) and, while the game is in view, a small picture of the game window as it is now, with \
rulers on its edges (0 to 1000 across and down) for pointing at things. The picture is low detail: take the \
numbers from what your vision engine read, and use look_closer when a small detail really matters. Use what you \
see, like a friend looking at the same screen. Without a picture you can't see the game right now.";

/// How the model is told about its tools.
const TOOLS_GUIDE: &str = "\n\nYou get better the more the player teaches you:
- Only when the player shows or tells you what something on screen is (\"this is...\", \"that's my...\", \"see that? it's...\") or asks you to watch for something, call learn_thing with a tight box around it in the picture's 0-1000 coordinates. Never learn things on your own. If they want a heads-up (\"tell me when a rune shows up\", \"warn me when the boss is under 20%\"), set alert, threshold and say (what you'll say then, in their language). The alert is about that thing appearing, disappearing or crossing a value — never attach an unrelated announcement to it (a level-up is watched by MapleSyrup itself; their own character is always on screen and is never a thing to learn).
- When the player says a value you have is wrong (their level, HP, MP, EXP, map, name, job), call correct_reading.
- When the player corrects you on anything else (a game fact, a name, how something works, or how you talk or behave), call note_correction with the right version, then go on with it.
- When the player tells you something about themselves or their game worth keeping (their class, a key binding, a goal), or asks you to remember something, call remember_fact.
- set_warnings when they want low HP or MP warnings at another percent, or no more of them, or back to the usual.
- forget_thing when asked to forget something you learned; look_closer to read small text or details you can't make out.
- mark_moment when the player asks you to mark or save the moment (for their video); set_muted when they ask you to be quiet, or to talk again.
- set_recording when they ask you to start or stop recording (a video of the screen with all the sound).
- set_coaching when they ask you to stop speaking up on your own (\"only talk when I ask\", \"no more tips\"), or to start again.
Never announce a tool before using it; after one, a few words at most.";

/// How the model is told about looking things up.
const LOOKUP_GUIDE: &str = "\n- look_it_up never makes the player wait: when you're not sure of a MapleStory fact (or they \
ask you to look something up), say your best answer first, then call look_it_up with the question and what you \
said. It checks in the background; you'll speak again only if you were wrong. Never mention it.";

/// How the model is told it is watching on its own: it reacts to the one
/// thing the watcher brings it, in the attitude's voice, or keeps quiet.
/// (The reasons' own wording, with the example lines, is in `coach`.)
const COACH_GUIDE: &str = "\n\nRight now nobody said anything to you. You're watching them play, like a friend \
on voice chat glancing at their screen, and your watcher says why it's asking — what just happened — with a few \
lines a friend would say in that spot, in your attitude: the pattern, not a script. React, don't report: one \
specific thing that just happened or that you just saw, the way a friend blurts it out (\"That's the wrong \
portal.\" \"Rebuff.\" \"Oof, that hit.\"), never a run-down of the screen (\"you're on a map with four \
characters…\"). One short line, in your attitude's voice; an instruction when there is one. Most of the time \
there is nothing worth saying: then reply with exactly [silent]. Never narrate or list what's on screen, never \
comment for the sake of it, never ask them anything, never repeat what you said lately, and never greet.";

/// How the model is told what it learned is there.
const LEARNED_GUIDE: &str =
    "\n\nWhat you learned from playing together before (use it naturally; never recite it):";

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
            "Translate what a gaming buddy app says out loud to someone playing MapleStory into {name}. \
Keep it short, casual and natural, as a friend would say it, and keep its tone: bossy stays bossy, rude stays \
rude, swearing stays swearing. Keep the numbers, and game words the way players say them. Reply with the \
translation only."
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

/// The conversation as input items. With the player's last sentence goes
/// what is on screen now (and the screen itself), so everything before it
/// stays the same from one reply to the next and OpenAI keeps it cached.
fn input_of(
    turns: &[openai::Turn],
    snapshot: &str,
    eyes: Option<&Eyes>,
    helps: &str,
) -> Vec<Value> {
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
            if !helps.trim().is_empty() {
                content.push(json!({
                    "type": "input_text",
                    "text": format!("[What you learned before that may help — not said by the player]\n{helps}"),
                }));
            }
            content.push(json!({"type": "input_text", "text": t.text}));
            json!({"role": "user", "content": content})
        })
        .collect()
}

/// The coach's look: why, and what goes with it.
struct Watch<'a> {
    id: u64,
    stop: &'a Stop,
    reason: &'a str,
    label: String,
    snapshot: &'a str,
    eyes: Option<&'a Eyes>,
    said: &'a [String],
    language: Option<&'a str>,
    speak: bool,
}

/// The coach looks at the game and says one line if there is one worth
/// saying (`Done::Coached`): nobody asked anything. The line is said here
/// when `speak`, and joins the conversation either way, so a "why?" after
/// it makes sense.
fn coach(chat: &OpenAi, mouth: Mouth, brain: &mut Brain, watch: Watch, tx: &Sender<Done>) {
    let Watch {
        id,
        stop,
        reason,
        label,
        snapshot,
        eyes,
        said,
        language,
        speak,
    } = watch;
    let started = Instant::now();
    let mut instructions = brain.persona();
    instructions.push_str(COACH_GUIDE);
    let learned = brain.learned();
    if !learned.is_empty() {
        instructions.push_str(LEARNED_GUIDE);
        instructions.push('\n');
        instructions.push_str(&learned);
    }
    let mut text = format!(
        "[Not the player: your game watcher. Nobody said anything.]\nThe game right now, read by your vision \
engine:\n{snapshot}\n\n{reason}"
    );
    if !said.is_empty() {
        text.push_str("\n\nWhat you said on your own lately (don't repeat it, don't nag):");
        for line in said {
            text.push_str(&format!("\n- {line}"));
        }
    }
    if let Some(l) = language.filter(|l| !language::is_english(l)) {
        let name = language::name(l);
        text.push_str(&format!(
            "\n\n(The player's language setting is {name}: speak the language of the conversation; when there is none yet, {name}.)"
        ));
    }
    let mut content = vec![json!({"type": "input_text", "text": text})];
    if let Some(eyes) = eyes {
        content.extend(eyes.pictures());
    }
    // The conversation so far, so it knows what was talked about (the
    // player said they're bossing; it was told to shut up about potions).
    let mut input: Vec<Value> = brain
        .turns()
        .iter()
        .map(|t| json!({"role": t.role, "content": t.text}))
        .collect();
    input.push(json!({"role": "user", "content": content}));
    let ask = Ask {
        instructions,
        input,
        max_output_tokens: 60,
        timeout: Duration::from_secs(if std::ptr::eq(chat, mouth.openai) {
            20
        } else {
            12
        }),
        stop: Some(stop.clone()),
        ..Default::default()
    };
    let answer = chat.ask(&ask, None).or_else(|e| {
        // The fast brain couldn't: OpenAI looks instead.
        if std::ptr::eq(chat, mouth.openai) || matches!(e, AiError::Cancelled) {
            Err(e)
        } else {
            mouth.openai.ask(&ask, None)
        }
    });
    let mut done = Done::Coached {
        id,
        label: label.clone(),
        text: None,
        error: None,
        took: started.elapsed(),
    };
    match answer {
        Err(AiError::Cancelled) => {}
        Err(error) => {
            done = Done::Coached {
                id,
                label,
                text: None,
                error: Some(error.detail()),
                took: started.elapsed(),
            };
        }
        Ok(answer) if brain::is_silent(&answer.text) => {}
        Ok(answer) => {
            // Nothing it said lately is said again: a coach that keeps
            // calling the same thing out is cut to what is new.
            let filtered = brain.recent.filter(&brain::for_speech(&answer.text));
            if filtered.dropped > 0 {
                let _ = tx.send(Done::Noted {
                    line: format!(
                        "{label}: {} of {} sentences said before, left out",
                        filtered.dropped, filtered.total
                    ),
                });
            }
            let line = filtered.text;
            if !line.is_empty() && !stop.stopped() {
                brain.heard(&format!("[Your game watcher, not the player: {label}.]"));
                brain.said(&line);
                if speak {
                    let _ = tx.send(Done::Shown {
                        kind: crate::companion::Kind::Alert,
                        text: line.clone(),
                    });
                    let style = brain::voice_style(brain.attitude());
                    if let Err(error) = speak_line(mouth, id, stop, &line, style, started, true, tx)
                    {
                        let _ = tx.send(Done::Failed {
                            id,
                            heard: None,
                            error,
                        });
                    }
                }
                done = Done::Coached {
                    id,
                    label,
                    text: Some(line),
                    error: None,
                    took: started.elapsed(),
                };
            }
        }
    }
    let _ = tx.send(done);
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
    /// MapleSyrup's own lines in the player's language, translated once.
    translations: &'a mut std::collections::HashMap<(String, String), String>,
}

/// What it says when everything it had to say, it said lately (the player
/// asked the same thing twice; the model is going round in circles): a
/// word rather than nothing, so they know they were heard.
const SAME_AS_BEFORE: &str = "Same as before.";

/// Answer the player. The reply is streamed and cut into sentences; they
/// are turned into speech (on a second thread) as soon as they are complete:
/// the first sentence alone, so it is heard soon, then whatever was written
/// meanwhile in one piece, so it flows. The `fast` brain (Grok) answers when
/// there is one, and OpenAI when it fails before a word of its answer. When
/// the model calls tools, they are run and their results handed back for it
/// to go on, up to a few rounds; a look-up never holds the answer up (it runs
/// in the background, `Done::LookUp`). Called off (the player talked over
/// it), it stops at once and keeps in the conversation only what was said.
/// Returns the brain that answered, and why the fast one didn't, if it
/// failed.
#[allow(clippy::too_many_arguments)]
/// How a reply went: which brain answered, why the fast one didn't (when
/// it didn't), and whether the reply was mostly things said before (the
/// brain going round in circles).
struct Replied<'a> {
    used: &'a OpenAi,
    fast_failed: Option<String>,
    looped: bool,
}

fn converse<'a>(
    mouth: Mouth<'a>,
    fast: Option<&'a OpenAi>,
    toolbox: Option<&Toolbox>,
    brain: &mut Brain,
    talk: Talk,
    tx: &Sender<Done>,
    busy: &AtomicBool,
) -> Replied<'a> {
    let Talk {
        id,
        stop,
        heard,
        snapshot,
        eyes,
        speak,
        language,
        translations,
    } = talk;
    let openai = mouth.openai;
    let started = Instant::now();
    // What never changes first, what changes now and then last: the model's
    // service keeps the start cached, and answers sooner.
    let mut instructions = brain.persona();
    instructions.push_str(EYES_GUIDE);
    if let Some(toolbox) = toolbox {
        instructions.push_str(TOOLS_GUIDE);
        if toolbox.web {
            instructions.push_str(LOOKUP_GUIDE);
        }
    }
    let learned = brain.learned();
    if !learned.is_empty() {
        instructions.push_str(LEARNED_GUIDE);
        instructions.push('\n');
        instructions.push_str(&learned);
    }
    let style = brain::voice_style(brain.attitude());
    // The player's sentence joins the conversation once it is answered (or
    // was talked over after part of the answer was said).
    let mut turns = brain.turns();
    turns.push(Turn {
        role: "user",
        text: heard.clone(),
    });
    let helps = brain
        .learning
        .as_ref()
        .map(|l| l.helps(&heard))
        .unwrap_or_default();
    let mut input = input_of(&turns, snapshot, eyes, &helps);
    let tools = toolbox.map(|t| t.definitions()).unwrap_or_default();
    // The brain for this reply.
    let mut chat = fast.unwrap_or(openai);
    let mut fast_failed = None;
    // Nothing said lately is said again (unless they asked to hear it
    // again): the sentences kept, and how many went.
    let again = brain::asks_again(&heard);
    let mut kept: Vec<String> = Vec::new();
    let (mut total, mut dropped) = (0usize, 0usize);
    let mut looped = false;
    // Where the record of what was said lately stands: this reply's
    // sentences are checked (and recorded) as they are written, and settled
    // against what the voice really said once it is done.
    let taken = brain.recent.taken();
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
                    match speak_line(mouth, id, stop, &text, style, started, first, &tx) {
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
        let mut round = 0;
        while round < 4 {
            let ask = Ask {
                instructions: instructions.clone(),
                input: input.clone(),
                tools: if round < 3 { tools.clone() } else { Vec::new() },
                // One or two short sentences; a cap keeps a ramble short.
                max_output_tokens: 150,
                // A brain that hangs gives way to the other soon.
                timeout: Duration::from_secs(if std::ptr::eq(chat, openai) { 40 } else { 12 }),
                stop: Some(stop.clone()),
                ..Default::default()
            };
            let answer = chat.ask(
                &ask,
                Some(&mut |piece| {
                    if let Piece::Text(t) = piece {
                        said.push_str(t);
                        if speak {
                            for sentence in sentences.push(t) {
                                total += 1;
                                if again || brain.recent.fresh(&sentence) {
                                    kept.push(sentence.clone());
                                    let _ = lines.send(brain::for_speech(&sentence));
                                } else {
                                    dropped += 1;
                                }
                            }
                        }
                    }
                }),
            );
            let answer = match answer {
                Ok(answer) => answer,
                // The fast brain failed before a word of its answer: OpenAI
                // answers instead.
                Err(error)
                    if !matches!(error, AiError::Cancelled)
                        && !std::ptr::eq(chat, openai)
                        && said.trim().is_empty() =>
                {
                    fast_failed = Some(error.detail());
                    chat = openai;
                    continue;
                }
                Err(error) => {
                    failed = Some(error);
                    break;
                }
            };
            round += 1;
            if answer.calls.is_empty() {
                break;
            }
            // What was said so far ends a sentence before the next round.
            if !said.trim().is_empty() && !said.ends_with(' ') {
                said.push(' ');
                if speak {
                    for sentence in sentences.push(" ") {
                        total += 1;
                        if again || brain.recent.fresh(&sentence) {
                            kept.push(sentence.clone());
                            let _ = lines.send(brain::for_speech(&sentence));
                        } else {
                            dropped += 1;
                        }
                    }
                }
            }
            // Only look-ups, after the answer was said: nothing to wait for.
            let mut go_on = false;
            let mut outputs = Vec::new();
            for call in &answer.calls {
                let (mut output, effect) = match toolbox {
                    Some(toolbox) => toolbox.run(call, eyes.map(|e| e.frame.as_ref())),
                    None => ("No tools here.".to_string(), None),
                };
                if call.name != "look_it_up" {
                    go_on = true;
                }
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
                    Some(Effect::Rewrite(instruction)) => {
                        let _ = tx.send(Done::Rewrite { instruction });
                    }
                    Some(Effect::Warn { what, below }) => {
                        let _ = tx.send(Done::Warn { what, below });
                    }
                    Some(Effect::LookUp {
                        question,
                        said: quick,
                        asked,
                    }) => {
                        let quick = if quick.trim().is_empty() {
                            said.trim().to_string()
                        } else {
                            quick
                        };
                        let _ = tx.send(Done::LookUp {
                            question,
                            said: quick,
                            asked,
                            language: language.map(String::from),
                        });
                        if said.trim().is_empty() {
                            // Asked before answering: the answer comes now.
                            output = "It's being looked up in the background. Say your best answer now, in a few \
words (\"probably\" if you're not sure)."
                                .into();
                            go_on = true;
                        }
                    }
                    None => {
                        // Known already (it answered from what it was taught).
                        if call.name == "look_it_up" && output.starts_with("Known:") {
                            go_on = true;
                        }
                    }
                }
                outputs.push(json!({"type": "function_call_output", "call_id": call.call_id, "output": output}));
            }
            if !go_on {
                break;
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
        // Talked over: the voice is waited for below, and what it said
        // aloud stays in the conversation, cut off where it was.
        let talked_over = matches!(failed, Some(AiError::Cancelled)).then(|| heard.clone());
        match failed {
            Some(AiError::Cancelled) => {}
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
                let text = if speak {
                    if let Some(rest) = sentences.finish() {
                        total += 1;
                        if again || brain.recent.fresh(&rest) {
                            kept.push(rest.clone());
                            let _ = lines.send(brain::for_speech(&rest));
                        } else {
                            dropped += 1;
                        }
                    }
                    brain::for_speech(&brain::without_announcement(&kept.join(" ")))
                } else {
                    let whole = brain::for_speech(&brain::without_announcement(&said));
                    if again {
                        whole
                    } else {
                        let filtered = brain.recent.filter(&whole);
                        total = filtered.total;
                        dropped = filtered.dropped;
                        filtered.text
                    }
                };
                looped = total >= 2 && dropped * 2 >= total;
                if dropped > 0 {
                    let _ = tx.send(Done::Noted {
                        line: format!("{dropped} of {total} sentences said before, left out"),
                    });
                }
                brain.heard(&heard);
                if text.is_empty() && dropped > 0 {
                    // Nothing new in it: not the same again, but not
                    // silence either — they asked, and hear that they were
                    // heard. The conversation keeps none of what was
                    // dropped, so the model has no loop of its own to
                    // follow.
                    let line = match language {
                        Some(l) if !language::is_english(l) => {
                            translate(openai, SAME_AS_BEFORE, l, stop, translations)
                        }
                        _ => SAME_AS_BEFORE.to_string(),
                    };
                    if !stop.stopped() {
                        brain.said(&line);
                        if speak {
                            let _ = lines.send(line.clone());
                        }
                        let _ = tx.send(Done::Reply {
                            id,
                            heard,
                            text: line,
                            took: started.elapsed(),
                        });
                    }
                } else if text.is_empty() {
                    // Nothing in it to say (a link, an emoji): quiet.
                    let _ = tx.send(Done::Silent { id, heard });
                } else {
                    brain.said(&text);
                    let _ = tx.send(Done::Reply {
                        id,
                        heard,
                        text,
                        took: started.elapsed(),
                    });
                }
            }
        }
        // The words are out; the voice may still be on its last lines.
        busy.store(false, Ordering::Relaxed);
        // Once it is done, what it said is what was said lately — and no
        // more: the sentences went into the record as they were written,
        // and the ones the voice never got to (the player talked over it)
        // would otherwise be "said before" when they ask again.
        drop(lines);
        if let Some(voice) = voice {
            let spoken = voice.join().unwrap_or_default();
            brain.recent.settle(taken, &spoken);
            if let Some(heard) = talked_over
                && !spoken.trim().is_empty()
            {
                brain.heard(&heard);
                brain.said(&format!("{}…", spoken.trim_end_matches(['.', ' '])));
            }
        }
    });
    Replied {
        used: chat,
        fast_failed,
        looped,
    }
}

/// The usual hello, when the model has nothing of its own to say.
const HELLO: &str = "Hey! I'm here. Just talk to me.";

/// Say hi when the phone connects: from the model (`chat`) when it knows
/// the player (it may pick up from last time), else the usual line; shown,
/// and said in `voice`.
fn greet(
    chat: &OpenAi,
    mouth: Mouth,
    brain: &mut Brain,
    id: u64,
    stop: &Stop,
    language: Option<&str>,
    tx: &Sender<Done>,
) {
    let asked = Instant::now();
    let learned = brain.learned();
    let style = brain::voice_style(brain.attitude());
    let mut text = String::new();
    if !learned.is_empty() {
        let tongue = language
            .map(language::name)
            .map(|name| format!(" (their phone is set to {name})"))
            .unwrap_or_default();
        let ask = Ask {
            instructions: format!("{}{LEARNED_GUIDE}\n{learned}", brain.persona()),
            input: vec![json!({"role": "user", "content": format!(
                "[The player just connected their phone to talk with you; not said by them.] Say hi in one short, \
            natural line in their language{tongue}, in your attitude. If you know what they were up to lately, you may pick up \
            from there in a few words."
            )})],
            max_output_tokens: 80,
            timeout: Duration::from_secs(15),
            stop: Some(stop.clone()),
            ..Default::default()
        };
        let answer = chat.ask(&ask, None).or_else(|e| {
            if std::ptr::eq(chat, mouth.openai) || matches!(e, AiError::Cancelled) {
                Err(e)
            } else {
                mouth.openai.ask(&ask, None)
            }
        });
        if let Ok(answer) = answer
            && !brain::is_silent(&answer.text)
        {
            text = brain::for_speech(&answer.text);
        }
    }
    if stop.stopped() {
        return;
    }
    if text.is_empty() {
        // The usual line, in their language.
        let mut translations = std::collections::HashMap::new();
        say_line(
            mouth,
            id,
            stop,
            Line {
                text: HELLO,
                language,
                show: Some(crate::companion::Kind::Reply),
                aloud: true,
                style,
            },
            &mut translations,
            tx,
        );
        return;
    }
    let _ = tx.send(Done::Shown {
        kind: crate::companion::Kind::Reply,
        text: text.clone(),
    });
    brain.said(&text);
    if let Err(error) = speak_line(mouth, id, stop, &text, style, asked, false, tx) {
        let _ = tx.send(Done::Failed {
            id,
            heard: None,
            error,
        });
    }
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

    #[test]
    fn the_coach_is_told_to_react_in_its_attitude_and_never_to_narrate() {
        // (The fake server in tests/openai_fake.rs knows a coach's look by
        // its first sentence.)
        assert!(COACH_GUIDE.contains("Right now nobody said anything to you."));
        assert!(COACH_GUIDE.contains("your watcher says why it's asking"));
        assert!(COACH_GUIDE.contains("in your attitude: the pattern, not a script"));
        assert!(COACH_GUIDE.contains("React, don't report: one specific thing"));
        assert!(COACH_GUIDE.contains("the way a friend blurts it out"));
        assert!(COACH_GUIDE.contains(
            "never a run-down of the screen (\"you're on a map with four characters…\")"
        ));
        assert!(COACH_GUIDE.contains("then reply with exactly [silent]"));
        assert!(COACH_GUIDE.contains("Never narrate or list what's on screen"));
        assert!(COACH_GUIDE.contains("never ask them anything"));
        assert!(COACH_GUIDE.contains("never greet"));
    }
}
