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
//! The worker is two lanes. The thinking lane owns the conversation and
//! does what needs it: replies, the coach's looks, the hello. The mouth
//! lane says MapleSyrup's own lines (`Job::Speak`: a warning, a level-up)
//! as they come, so a warning never waits behind a look's model call; it
//! tells the thinking lane what it said, and the conversation keeps it
//! like a coach's line. The voice is one: a line is made whole before the
//! next starts, on whichever lane.
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
pub mod pronounce;
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
pub use openai::{AiError, Delivery, OpenAi};
pub use tools::{Effect, Toolbox};

use crate::companion::{Attitude, Kind};
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

/// The parts of the screen a question about it is answered from, close up
/// ([`Eyes::close_pictures`]): where each is on the screen (fractions of the
/// frame), and what the model is told it is. On MapleStory the minimap
/// with the map's name is at the top left (Classic World's too), the HUD
/// with the level, job, name, HP, MP and EXP at the bottom (Classic: its
/// left part, the chat line above it), and the camera follows the
/// character: he is around the middle, his name tag under him — where an
/// open window or dialog usually is too.
pub const CLOSE_UPS: [(&str, [f32; 4]); 3] = [
    (
        "the minimap, top left of the screen: the map's name is written at its top",
        [0.0, 0.0, 0.24, 0.24],
    ),
    (
        "the HUD at the bottom of the screen: the level (LV.), job and character name, HP[now/max], \
MP[now/max] and EXP, with the chat line above them",
        [0.2, 0.9, 0.64, 1.0],
    ),
    (
        "the middle of the screen: his character is usually here (the name tag under him is his character's name, \
as on the HUD; at a map's edge he can be off to a side), and any window or dialog open there",
        [0.25, 0.2, 0.75, 0.85],
    ),
];

/// The Quest Helper (top right on Classic World): what each of his quests
/// still needs — close up when he asks about quests ("What's the next
/// quest I should go to" got "follow the yellow markers").
pub const QUEST_CLOSE_UP: (&str, [f32; 4]) = (
    "the Quest Helper, top right of the screen: his quests and what each still needs",
    [0.75, 0.0, 1.0, 0.3],
);

/// How wide the whole screen goes with a question about it: game text is
/// readable at this size (the owner's game draws its UI for 1280 across).
pub const OVERVIEW_WIDE: u32 = 1280;

impl Eyes {
    /// The picture for the model when the sentence is not about the screen:
    /// the whole frame, small and at low detail (quick to send and to look
    /// at), with rulers to point at things. The numbers come read already;
    /// `look_closer` reads small print.
    pub fn pictures(&self) -> Vec<Value> {
        // (At low detail OpenAI looks at 512 pixels across at most.)
        let frame = images::with_rulers(&images::fit(&self.frame, 640, 400));
        vec![images::input_image(images::jpeg_url(&frame, 60), "low")]
    }

    /// The pictures for a sentence about the screen ("where am I", "what
    /// level", "what's equipped", "what do you see"): the whole screen at
    /// [`OVERVIEW_WIDE`] and high detail, rulers on it, then close-ups of
    /// [`CLOSE_UPS`] at the frame's own resolution and high detail, each
    /// with a word on what it is — the map's name, the HUD's numbers and
    /// his character could not be read from 640 pixels at low detail, so
    /// the model guessed them. (Each close-up is no bigger than what OpenAI
    /// looks at in high detail — 2048 across, 768 on its short side — so
    /// nothing it would see is lost on the way; small frames' close-ups are
    /// enlarged so the game's small print is big enough to read.)
    pub fn close_pictures(&self) -> Vec<Value> {
        self.close_pictures_for("")
    }

    /// [`Eyes::close_pictures`] for the sentence `heard`: asked about his
    /// quests, the Quest Helper close up too ([`QUEST_CLOSE_UP`]).
    pub fn close_pictures_for(&self, heard: &str) -> Vec<Value> {
        let overview = images::with_rulers(&images::fit(&self.frame, OVERVIEW_WIDE, 900));
        let mut out = vec![
            json!({"type": "input_text", "text": format!(
                "[The whole screen, {} across, high detail; rulers 0–1000 for pointing (look_closer, boxes)]",
                overview.width().saturating_sub(44)
            )}),
            images::input_image(images::jpeg_url(&overview, 75), "high"),
        ];
        let (w, _) = self.frame.dimensions();
        // A small frame's small print, enlarged (a 1280-wide window: twice).
        let times = (2560.0 / w.max(1) as f32).round().clamp(1.0, 3.0) as u32;
        // What each close-up is, where, and whether it is small print.
        let mut parts: Vec<(&str, NBox, bool)> = Vec::new();
        for (i, (what, [x0, y0, x1, y1])) in CLOSE_UPS.iter().enumerate() {
            let mut part = NBox::new(*x0, *y0, *x1, *y1);
            // The HUD where the vision engine found it, too (another
            // layout than Classic World's keeps its numbers elsewhere along
            // the bottom).
            if i == 1
                && let Some(found) = self.status.filter(|s| s.y0 >= 0.6)
            {
                part = NBox::new(
                    part.x0.min(found.x0),
                    part.y0.min(found.y0),
                    part.x1.max(found.x1),
                    part.y1.max(found.y1),
                );
            }
            parts.push((what, part, i != 2));
        }
        if brain::asks_about_quests(heard) {
            let (what, [x0, y0, x1, y1]) = QUEST_CLOSE_UP;
            parts.push((what, NBox::new(x0, y0, x1, y1), true));
        }
        let n = parts.len();
        for (i, (what, part, small_print)) in parts.into_iter().enumerate() {
            let (x0, y0, x1, y1) = (part.x0, part.y0, part.x1, part.y1);
            let mut crop = images::crop(&self.frame, &part);
            if times > 1 {
                crop = images::enlarged(&crop, times);
            }
            let crop = as_seen_in_high_detail(&crop);
            out.push(json!({"type": "input_text", "text": format!(
                "[Close-up {} of {n}, full resolution: {what} (x {:.0}–{:.0}, y {:.0}–{:.0} on the rulers)]",
                i + 1,
                x0 * 1000.0,
                x1 * 1000.0,
                y0 * 1000.0,
                y1 * 1000.0
            )}));
            // (Small print sharp; the middle a touch lighter.)
            let quality = if small_print { 88 } else { 82 };
            out.push(images::input_image(
                images::jpeg_url(&crop, quality),
                "high",
            ));
        }
        out
    }
}

/// `image` as OpenAI looks at it in high detail: within 2048 by 2048, then
/// no more than 768 on its short side (anything bigger is made that small
/// on their side: sending it bigger only takes longer).
pub fn as_seen_in_high_detail(image: &RgbaImage) -> RgbaImage {
    let fitted = images::fit(image, 2048, 2048);
    let (w, h) = fitted.dimensions();
    let short = w.min(h);
    if short <= 768 {
        return fitted;
    }
    let scale = 768.0 / short as f32;
    images::fit(
        &fitted,
        (w as f32 * scale).round() as u32,
        (h as f32 * scale).round() as u32,
    )
}

/// Something for the worker to do.
pub enum Job {
    /// Answer what the player said, given what is on screen.
    Converse {
        heard: String,
        snapshot: String,
        /// What the snapshot says of the game (the level, the map…), to
        /// tell a reply from a status line nobody asked for.
        facts: brain::Facts,
        speak: bool,
        /// The screen, when the game is in view.
        eyes: Option<Eyes>,
        /// The player's language setting (a locale such as `he-IL`).
        language: Option<String>,
    },
    /// Say one of MapleSyrup's own lines (a warning, a greeting) in the
    /// natural voice, translated first when the player's language is not
    /// English. With `show`, the line has not been shown yet: it comes back
    /// as `Shown`, in the player's language, to be shown. A warning's line
    /// and news (`kind`) are not called off with the rest: a warning must
    /// not vanish because the player spoke over something else. Said on
    /// the mouth lane, it never waits for a reply or a look; said aloud,
    /// it joins the conversation (the next reply knows its own last
    /// words).
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
    /// closes it (no samples). `kind`: what kind of line it is (a
    /// warning's clip is the one a cut keeps); `after`: since the job was
    /// handed over; `first`: the first line of a reply.
    Audio {
        id: u64,
        kind: crate::companion::Kind,
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
    /// saying, or `error`, or `called_off` — the player spoke before the
    /// look was done, so it never answered: not "nothing to say", and no
    /// reason to look less often), and how long the look took.
    Coached {
        id: u64,
        label: String,
        text: Option<String>,
        error: Option<String>,
        called_off: bool,
        took: Duration,
    },
}

impl Job {
    /// Whether no call-off stops it: a warning's own line, or news (a
    /// death, a level-up) — what is still true after the player's words
    /// ([`Kind::kept`]).
    fn kept(&self) -> bool {
        matches!(self, Job::Speak { kind, .. } if kind.kept())
    }
}

/// How many of the jobs no call-off stops are remembered by number: an
/// alert's voice is over within seconds of its job, so the last few are
/// all that can still be asked about.
const KEPT_REMEMBERED: usize = 64;

/// What the thinking lane is handed: a job, or word from the mouth lane.
enum Work {
    Job(u64, Job),
    /// The mouth lane handed one of MapleSyrup's own lines to the voice
    /// (`kind`; `text` as said, in the player's language): the
    /// conversation keeps it, so the next reply knows its own last words.
    Said {
        kind: Kind,
        text: String,
    },
}

pub struct Worker {
    /// The thinking lane: replies, the coach's looks, the hello.
    jobs: Sender<Work>,
    /// The mouth lane: its own lines (`Job::Speak`), said as they come,
    /// whatever the thinking lane is on — a warning never waits for a look.
    lines: Sender<(u64, Job)>,
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
        let _ = match job {
            Job::Speak { .. } => self.lines.send((id, job)).is_ok(),
            job => self.jobs.send(Work::Job(id, job)).is_ok(),
        };
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

/// Start the worker's two lanes with these brains: the thinking lane
/// (replies, the coach's looks, the hello: everything that needs the
/// conversation) and the mouth lane (its own lines, `Job::Speak`). The
/// voice is one: a line is made whole before the next starts
/// (`Mouth::floor`), on whichever lane — so what can overlap is a warning
/// being made while the other lane waits on a model, never two lines'
/// audio.
pub fn spawn_brains(brains: Brains, mut brain: Brain, toolbox: Option<Toolbox>) -> Worker {
    let Brains {
        openai,
        fast,
        eleven,
    } = brains;
    let openai = Arc::new(openai);
    let eleven = eleven.map(Arc::new);
    let (jobs, rx) = channel::<Work>();
    let (lines, lines_rx) = channel::<(u64, Job)>();
    let (tx, done) = channel::<Done>();
    let busy = Arc::new(AtomicBool::new(false));
    let model = Arc::new(std::sync::Mutex::new(None));
    let mark = Arc::new(AtomicU64::new(0));
    let kept = Arc::new(std::sync::Mutex::new(Vec::new()));
    // One voice for both lanes; ElevenLabs failing is said once.
    let floor = Arc::new(std::sync::Mutex::new(()));
    let eleven_failed = Arc::new(AtomicBool::new(false));
    {
        let (openai, eleven, tx, marks, kept_jobs, floor, failed, notes) = (
            Arc::clone(&openai),
            eleven.clone(),
            tx.clone(),
            Arc::clone(&mark),
            Arc::clone(&kept),
            Arc::clone(&floor),
            Arc::clone(&eleven_failed),
            jobs.clone(),
        );
        let tuning = brain.tuning();
        let _ = std::thread::Builder::new()
            .name("mouth".into())
            .spawn(move || {
                // Its own lines come back often ("Level up!"): each is
                // translated once.
                let mut translations = Translations::default();
                while let Ok((id, job)) = lines_rx.recv() {
                    let Job::Speak {
                        text,
                        language,
                        kind,
                        show,
                        speak,
                    } = job
                    else {
                        continue;
                    };
                    let stop = if is_kept(&kept_jobs, id) {
                        Stop::never()
                    } else {
                        Stop::new(Arc::clone(&marks), id)
                    };
                    if stop.stopped() {
                        continue;
                    }
                    let voice_id = tuning.voice_id();
                    let mouth = Mouth {
                        openai: &openai,
                        eleven: eleven.as_deref().zip(voice_id.as_deref()),
                        failed: &failed,
                        floor: &floor,
                    };
                    say_line(
                        mouth,
                        id,
                        &stop,
                        Line {
                            text: &text,
                            language: language.as_deref(),
                            kind,
                            show,
                            aloud: speak,
                            attitude: tuning.attitude(),
                        },
                        &mut translations,
                        &tx,
                        &mut |said| {
                            let _ = notes.send(Work::Said {
                                kind,
                                text: said.to_string(),
                            });
                        },
                    );
                }
            });
    }
    let (busy_flag, model_slot, marks, kept_jobs) = (
        Arc::clone(&busy),
        Arc::clone(&model),
        Arc::clone(&mark),
        Arc::clone(&kept),
    );
    let _ = std::thread::Builder::new()
        .name("ai".into())
        .spawn(move || {
            let openai: &OpenAi = &openai;
            // The line it says when a reply had nothing new in it, in the
            // player's language: translated once.
            let mut translations = Translations::default();
            // The fast brain failing again and again is given up on; one
            // going round in circles rests a while.
            let mut fast_failures = 0u32;
            let mut fast_paused_until: Option<Instant> = None;
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
                    .rposition(|work| matches!(work, Work::Job(_, Job::Converse { .. })));
                for (i, work) in queue.into_iter().enumerate() {
                    let (id, job) = match work {
                        Work::Job(id, job) => (id, job),
                        Work::Said { kind, text } => {
                            brain.watched(label_of(kind), &text);
                            continue;
                        }
                    };
                    let stop = if is_kept(&kept_jobs, id) {
                        Stop::never()
                    } else {
                        Stop::new(Arc::clone(&marks), id)
                    };
                    if stop.stopped() {
                        // (The coach waits to hear back from every look:
                        // this one was called off before it began.)
                        if let Job::Coach { label, .. } = job {
                            let _ = tx.send(Done::Coached {
                                id,
                                label,
                                text: None,
                                error: None,
                                called_off: true,
                                took: Duration::ZERO,
                            });
                        }
                        continue;
                    }
                    // The voice the player picked, when it's ElevenLabs's.
                    let voice_id = brain.voice_id();
                    let mouth = Mouth {
                        openai,
                        eleven: eleven.as_deref().zip(voice_id.as_deref()),
                        failed: &eleven_failed,
                        floor: &floor,
                    };
                    match job {
                        // (Its own lines go to the mouth lane: `Worker::send`
                        // routes them there. One that gets here is said all
                        // the same.)
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
                                kind,
                                show,
                                aloud: speak,
                                attitude: brain.attitude(),
                            },
                            &mut translations,
                            &tx,
                            &mut |said| brain.watched(label_of(kind), said),
                        ),
                        Job::Cut { heard } => brain.cut_short(&heard),
                        Job::Greet { language } => greet(
                            fast.as_ref().unwrap_or(openai),
                            mouth,
                            &mut brain,
                            id,
                            &stop,
                            language.as_deref(),
                            &tx,
                        ),
                        // A look-up's finding is shown on the phone, never
                        // said: one answer, not two.
                        Job::Say { heard: None, text } if lookup::shown_only(&text) => {
                            let text = brain::for_speech(&text);
                            if !text.is_empty() {
                                let _ = tx.send(Done::Shown {
                                    kind: Kind::Info,
                                    text,
                                });
                            }
                        }
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
                            // Said now: the model saying it again would be
                            // twice — settled, once the voice is done, against
                            // what it did say (a line called off before a
                            // sound was made was not said).
                            let taken = brain.recent.taken();
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
                            let delivery = Delivery::of(brain.attitude(), Kind::Reply, &text);
                            match speak_line(
                                mouth,
                                id,
                                &stop,
                                &text,
                                delivery,
                                Instant::now(),
                                false,
                                &tx,
                            ) {
                                Ok(made) => brain.recent.settle(taken, if made { &text } else { "" }),
                                Err(error) => {
                                    brain.recent.settle(taken, "");
                                    let _ = tx.send(Done::Failed {
                                        id,
                                        heard: None,
                                        error,
                                    });
                                }
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
                                .unwrap_or(openai);
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
                            snapshot,
                            facts,
                            speak,
                            eyes,
                            language,
                        } => {
                            // (The language to answer in goes with his
                            // words: `brain::answer_note`.)
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
                                    facts: &facts,
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
                            if looped && !std::ptr::eq(used, openai) {
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
                                None if !std::ptr::eq(used, openai) => fast_failures = 0,
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
        lines,
        done,
        busy,
        model,
        next: AtomicU64::new(1),
        mark,
        kept,
    }
}

/// The watcher's word for one of its own lines of this `kind`, for the
/// conversation (`Brain::watched`).
fn label_of(kind: Kind) -> &'static str {
    match kind {
        Kind::Warning => "a warning",
        Kind::Alert => "news",
        Kind::Info => "a note",
        Kind::Reply | Kind::Heard => "a line",
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
    /// Held while a line is made: the two lanes share one voice, and a
    /// line's pieces reach the main loop whole, never shuffled with
    /// another's.
    floor: &'a std::sync::Mutex<()>,
}

/// One of MapleSyrup's own lines, and how to say it.
struct Line<'a> {
    text: &'a str,
    /// The player's language: translated into it when it isn't English.
    language: Option<&'a str>,
    /// What kind of line it is (a warning is said faster and sharper).
    kind: Kind,
    /// Shown (it comes back as `Shown`, translated).
    show: bool,
    /// Said aloud too.
    aloud: bool,
    /// How it talks now: the voice sounds like it.
    attitude: Attitude,
}

/// One of MapleSyrup's own lines: translated into the player's language
/// when it is not English, shown, said. `on_say` gets the line as it goes
/// to the voice (for the conversation: it said this).
#[allow(clippy::too_many_arguments)]
fn say_line(
    mouth: Mouth,
    id: u64,
    stop: &Stop,
    line: Line,
    translations: &mut Translations,
    tx: &Sender<Done>,
    on_say: &mut dyn FnMut(&str),
) {
    let asked = Instant::now();
    let text = match line.language {
        Some(l) if !language::is_english(l) => {
            let wait = match line.kind {
                Kind::Warning => WARNING_TRANSLATION,
                _ => LINE_TRANSLATION,
            };
            translate(mouth.openai, line.text, l, stop, translations, wait)
        }
        _ => line.text.to_string(),
    };
    if stop.stopped() {
        return;
    }
    if line.show {
        let _ = tx.send(Done::Shown {
            kind: line.kind,
            text: text.clone(),
        });
    }
    if !line.aloud {
        return;
    }
    on_say(&text);
    let delivery = Delivery::of(line.attitude, line.kind, &text);
    if let Err(error) = speak_line(mouth, id, stop, &text, delivery, asked, false, tx) {
        let _ = tx.send(Done::Failed {
            id,
            heard: None,
            error,
        });
    }
}

/// Say `text` in the natural voice, delivered as `delivery` says, handing
/// it over a piece at a time as it is made. Returns whether any of it was
/// made (it may be called off, and there is nothing to say for an empty
/// line). One line at a time, across both lanes (`Mouth::floor`): a
/// warning waits for the line being made, never for a look or a reply
/// being written.
#[allow(clippy::too_many_arguments)]
fn speak_line(
    mouth: Mouth,
    id: u64,
    stop: &Stop,
    text: &str,
    delivery: Delivery,
    asked: Instant,
    first: bool,
    tx: &Sender<Done>,
) -> Result<bool, AiError> {
    if !text.chars().any(char::is_alphanumeric) {
        return Ok(false);
    }
    let _floor = mouth.floor.lock().unwrap_or_else(|e| e.into_inner());
    // (Called off while it waited its turn.)
    if stop.stopped() {
        return Ok(false);
    }
    let style = brain::voice_style(delivery);
    let start = std::cell::Cell::new(true);
    let mut send = |samples: &[i16]| {
        let opens = start.replace(false);
        let _ = tx.send(Done::Audio {
            id,
            kind: delivery.kind,
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
            match eleven.speech_stream(text, voice, delivery, Some(stop), &mut send) {
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
                        .speech_stream(text, &style, Some(stop), &mut send)
                }
                other => other,
            }
        }
        _ => mouth
            .openai
            .speech_stream(text, &style, Some(stop), &mut send),
    };
    let made = !start.get();
    if made {
        let _ = tx.send(Done::Audio {
            id,
            kind: delivery.kind,
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
(not said by the player) and, while the game is in view, a picture of the game window as it is now, with rulers on \
its edges (0 to 1000 across and down) for pointing at things. When their words are about the screen (where they \
are, the map, their level, HP or MP, how they look, an NPC, a quest, an item, a window, what you see), the picture \
is sharp and close-ups at full resolution come with it: the minimap (the map's name is at its top), the HUD (LV., \
job, name, HP[now/max], MP[now/max], EXP) and the middle of the screen (their character, his name tag under him, \
and any window or dialog). Read the answer off them, as a person reading the screen would: the map's name off the \
minimap, the numbers off the HUD (or from what your vision engine read, with its age), what they wear off their \
character — the one whose name tag under him is the HUD's name. Use look_closer for anything still too small. \
Never answer about the screen from memory, from an earlier picture or from the small low-detail picture, and \
never guess a map, a level or a look: if you truly can't read it, say so in a few words and say what you can see. Use what you see, like a friend looking at the \
same screen. Without a picture you can't see the game right now.";

/// How the model is told about MapleStory Classic World, when the player
/// plays it ([`brain::classic_world`]): what it does not have, and that a
/// fact it isn't sure holds there is said to be unsure — the owner was sent
/// to the Maple Guide, a world-map search, "Blue Mushroom Forest 2", Drop
/// Coupons and a Slime Shoes recipe, none of which his world has.
const CLASSIC_GUIDE: &str = "\n\nThey play MapleStory Classic World: the game as it was long ago, not today's \
MapleStory. It has none of today's systems and content: no Maple Guide, no world-map search, no Arcane River, no \
Fafnir, no Root Abyss, no Kanna or the other later jobs, no Drop Coupons in the Cash Shop, none of the modern \
events or boosts. If you are not sure a fact holds in Classic World, say you're not sure — never invent maps, \
NPCs, recipes or routes. What you see on their screen beats what you remember.";

/// How the model is told about its tools.
const TOOLS_GUIDE: &str = "\n\nYou get better the more the player teaches you:
- Only when the player shows or tells you what something on screen is (\"this is...\", \"that's my...\", \"see that? it's...\") or asks you to watch for something, call learn_thing with a tight box around it in the picture's 0-1000 coordinates. Never learn things on your own. If they want a heads-up (\"tell me when a rune shows up\", \"warn me when the boss is under 20%\"), set alert, threshold and say (what you'll say then, in their language). The alert is about that thing appearing, disappearing or crossing a value — never attach an unrelated announcement to it (a level-up is watched by MapleSyrup itself; their own character is always on screen and is never a thing to learn).
- When the player says a value you have is wrong (their level, HP, MP, EXP, map, name, job), call correct_reading.
- When the player corrects you on anything else (a game fact, how something works, or how you talk or behave), call note_correction with the right version, then go on with it.
- When the player asks you to remember something (\"remember…\", \"תזכור…\"), call remember_fact.
- set_warnings when they want low HP or MP warnings at another percent, or no more of them, or back to the usual.
- forget_thing when asked to forget something you learned; look_closer to read small text or details you can't make out.
- mark_moment when the player asks you to mark or save the moment (for their video); set_muted when they ask you to be quiet, or to talk again.
- set_recording when they ask you to start or stop recording (a video of the screen with all the sound).
- set_coaching when they ask you to stop speaking up on your own (\"only talk when I ask\", \"no more tips\"), or to start again.
A tool that changes a setting or what you keep acts only when the player's own sentence plainly asks for it — a \
verb and what it is about (\"warn me when MP is under 30%\", \"stop talking\", \"remember that…\", \"no, I'm level \
61\") — never on a few words that may be misheard or your own voice heard back (\"don't stop\", \"23% still \
owe\"); \"Not changed\" from a tool means nothing changed. Never keep a name for the player with a tool.
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

/// How long one of its own lines waits for its translation: a warning is
/// said in English after [`WARNING_TRANSLATION`] (a late warning is no
/// warning), anything else after [`LINE_TRANSLATION`].
const WARNING_TRANSLATION: Duration = Duration::from_secs(2);
const LINE_TRANSLATION: Duration = Duration::from_secs(20);

/// MapleSyrup's own lines in the player's language, each translated once —
/// and a line that comes back with other numbers ("HP 25 percent. Pot
/// now!", then "HP 20 percent. Pot now!") translated once for all of them:
/// kept by the line with its numbers taken out, as the translation with a
/// gap where each number goes. (A translation the numbers can't be found
/// in, once each, is kept for its own line only.)
#[derive(Default)]
struct Translations {
    /// By locale and line.
    lines: std::collections::HashMap<(String, String), String>,
    /// By locale and the line with `{}` for each number.
    shapes: std::collections::HashMap<(String, String), Shape>,
}

/// A translation with its numbers taken out: the text around them, and
/// which of the line's numbers goes in each gap (the translation may put
/// them in another order).
#[derive(Debug, Clone, PartialEq)]
struct Shape {
    pieces: Vec<String>,
    order: Vec<usize>,
}

impl Translations {
    /// The translation into `locale` of `text`, when it is known.
    fn get(&self, locale: &str, text: &str) -> Option<String> {
        let key = |s: &str| (locale.to_string(), s.to_string());
        if let Some(done) = self.lines.get(&key(text)) {
            return Some(done.clone());
        }
        let (shape, numbers) = numbers_of(text);
        let shape = self
            .shapes
            .get(&key(&shape))
            .filter(|_| templated(locale, &numbers))?;
        let mut out = shape.pieces[0].clone();
        for (gap, piece) in shape.order.iter().zip(&shape.pieces[1..]) {
            out.push_str(numbers.get(*gap)?);
            out.push_str(piece);
        }
        Some(out)
    }

    fn put(&mut self, locale: &str, text: &str, done: &str) {
        if self.lines.len() > 200 {
            self.lines.clear();
            self.shapes.clear();
        }
        let (shape, numbers) = numbers_of(text);
        if templated(locale, &numbers)
            && let Some(found) = shape_of(done, &numbers)
        {
            self.shapes.insert((locale.to_string(), shape), found);
        }
        self.lines
            .insert((locale.to_string(), text.to_string()), done.to_string());
    }
}

/// Languages whose nouns take more forms by the number before them than
/// one and many ("21 процент", "22 процента", "25 процентов"; Arabic's dual
/// and its plurals; Romanian's "20 de"): their lines are kept one by one,
/// never as a template.
const INFLECTS_BY_NUMBER: &[&str] = &[
    "ru", "uk", "be", "pl", "cs", "sk", "sl", "hr", "sr", "bs", "lt", "lv", "ro", "ar", "ga", "cy",
    "is",
];

/// Whether a line with `numbers` in it is translated into `locale` once
/// for all its numbers ([`Translations`]): when every number is two or
/// more — a template learned from "25" said "1 אחוז", "Te quedan 1 de PM";
/// one learned from "1" would say "Te queda 20" — and the language's nouns
/// have one plural for them all.
fn templated(locale: &str, numbers: &[String]) -> bool {
    let language = locale
        .split(['-', '_'])
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    !numbers.is_empty()
        && !INFLECTS_BY_NUMBER.contains(&language.as_str())
        && numbers
            .iter()
            .all(|n| n.replace(',', "").parse::<f64>().is_ok_and(|v| v >= 2.0))
}

/// `text` with `{}` for each number in it, and the numbers ("HP 25.5
/// percent" is "HP {} percent" and "25.5").
fn numbers_of(text: &str) -> (String, Vec<String>) {
    let (mut shape, mut numbers) = (String::new(), Vec::new());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if !c.is_ascii_digit() {
            shape.push(c);
            continue;
        }
        let mut number = c.to_string();
        // Its digits, and a point or a comma between two of them.
        while let Some(&next) = chars.peek() {
            let inside = matches!(next, '.' | ',')
                && chars.clone().nth(1).is_some_and(|d| d.is_ascii_digit());
            if !next.is_ascii_digit() && !inside {
                break;
            }
            number.push(next);
            chars.next();
        }
        shape.push_str("{}");
        numbers.push(number);
    }
    (shape, numbers)
}

/// `done`, a translation of a line with `numbers` in it, cut around them:
/// when each is in it exactly once (and no two are the same), so that it
/// can be told where each goes.
fn shape_of(done: &str, numbers: &[String]) -> Option<Shape> {
    if numbers.is_empty() {
        return None;
    }
    let (_, found) = numbers_of(done);
    let mut at = Vec::new();
    for (i, number) in numbers.iter().enumerate() {
        let places: Vec<usize> = found
            .iter()
            .enumerate()
            .filter(|(_, f)| *f == number)
            .map(|(p, _)| p)
            .collect();
        if numbers.iter().filter(|n| *n == number).count() > 1 || places.len() != 1 {
            return None;
        }
        at.push((places[0], i));
    }
    // The translation's own pieces, and its numbers: the line's go into
    // gaps, any other stays as it was.
    let (shape, _) = numbers_of(done);
    let mut pieces = vec![String::new()];
    let mut order = Vec::new();
    for (p, piece) in shape.split("{}").enumerate() {
        if p > 0 {
            match at.iter().find(|(place, _)| *place == p - 1) {
                Some((_, i)) => {
                    order.push(*i);
                    pieces.push(String::new());
                }
                None => pieces.last_mut()?.push_str(&found[p - 1]),
            }
        }
        pieces.last_mut()?.push_str(piece);
    }
    Some(Shape { pieces, order })
}

/// One of MapleSyrup's own lines in the player's language (as it is, when
/// it can't be translated within `wait`).
fn translate(
    openai: &OpenAi,
    text: &str,
    locale: &str,
    stop: &Stop,
    cache: &mut Translations,
    wait: Duration,
) -> String {
    // Already written in another script (a line the model made in the
    // player's language, such as an alert they asked for).
    if !text.chars().any(|c| c.is_ascii_alphabetic()) {
        return text.to_string();
    }
    if let Some(done) = cache.get(locale, text) {
        return done;
    }
    let name = language::name(locale);
    let ask = Ask {
        instructions: format!(
            "Translate what a gaming buddy app says out loud to someone playing MapleStory into {name}. \
Keep it short, casual and natural, as a friend would say it, and keep its tone: bossy stays bossy, rude stays \
rude, swearing stays swearing. Keep the numbers, and game words the way players say them. The player is a man: \
speak to him in the masculine (in Hebrew אתה, תשתה, תזוז). Reply with the translation only."
        ),
        input: vec![json!({"role": "user", "content": text})],
        max_output_tokens: 150,
        timeout: wait,
        stop: Some(stop.clone()),
        ..Default::default()
    };
    match openai.ask(&ask, None) {
        Ok(answer) if !answer.text.trim().is_empty() => {
            let done = brain::for_speech(&answer.text);
            cache.put(locale, text, &done);
            done
        }
        _ => text.to_string(),
    }
}

/// The conversation as input items. With the player's last sentence goes
/// what is on screen now (and the screen itself), so everything before it
/// stays the same from one reply to the next and OpenAI keeps it cached.
/// (`close`: the sentence is about the screen — the screen goes sharp and
/// close up, [`Eyes::close_pictures`]; `note`: how to answer it, just
/// before the player's words.)
fn input_of(
    turns: &[openai::Turn],
    snapshot: &str,
    eyes: Option<&Eyes>,
    helps: &str,
    close: bool,
    note: &str,
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
                if close {
                    content.extend(eyes.close_pictures_for(&t.text));
                } else {
                    content.extend(eyes.pictures());
                }
            }
            if !helps.trim().is_empty() {
                content.push(json!({
                    "type": "input_text",
                    "text": format!("[What you learned before that may help — not said by the player]\n{helps}"),
                }));
            }
            if !note.trim().is_empty() {
                content.push(json!({"type": "input_text", "text": note}));
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
    // What the look came to: a line, nothing, an error — or nothing
    // because the player spoke before it was done (`called_off`), which
    // is not the same as nothing to say: every word of theirs calls off
    // the look in flight, and a chatty hour counted as silent had the
    // looks slowed to their fewest.
    let mut text = None;
    let mut error = None;
    let mut called_off = false;
    match answer {
        Err(AiError::Cancelled) => called_off = true,
        Err(e) => error = Some(e.detail()),
        Ok(answer) if brain::is_silent(&answer.text) => {}
        Ok(answer) => {
            // Nothing it said lately is said again: a coach that keeps
            // calling the same thing out is cut to what is new. What is
            // kept goes into the record now, and is settled against what
            // the voice did say once it is done: a line called off before
            // a sound was made was not said. (On a call the call says it.)
            let taken = brain.recent.taken();
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
                brain.watched(&label, &line);
                if speak {
                    let _ = tx.send(Done::Shown {
                        kind: Kind::Alert,
                        text: line.clone(),
                    });
                    let delivery = Delivery::of(brain.attitude(), Kind::Alert, &line);
                    let made = match speak_line(mouth, id, stop, &line, delivery, started, true, tx)
                    {
                        Ok(made) => made,
                        Err(error) => {
                            let _ = tx.send(Done::Failed {
                                id,
                                heard: None,
                                error,
                            });
                            false
                        }
                    };
                    brain.recent.settle(taken, if made { &line } else { "" });
                }
                text = Some(line);
            } else {
                // (Called off between the answer and the voice: never
                // handed over, so not said — and not silent either.)
                called_off = stop.stopped();
                brain.recent.settle(taken, "");
            }
        }
    }
    let _ = tx.send(Done::Coached {
        id,
        label,
        text,
        error,
        called_off,
        took: started.elapsed(),
    });
}

/// What the player said, and what goes with it.
struct Talk<'a> {
    id: u64,
    stop: &'a Stop,
    heard: String,
    snapshot: &'a str,
    facts: &'a brain::Facts,
    eyes: Option<&'a Eyes>,
    speak: bool,
    language: Option<&'a str>,
    /// MapleSyrup's own lines in the player's language, translated once.
    translations: &'a mut Translations,
}

/// A reply's sentences as they are written: the ones said, how many went as
/// said lately, and the status lines nobody asked for, set aside in case
/// they were all there was.
struct Saying<'a> {
    /// They asked to hear it again: nothing goes as said lately.
    again: bool,
    /// They did not ask about the game: what only says the snapshot back
    /// goes ([`brain::without_status`]).
    unasked: bool,
    facts: &'a brain::Facts,
    /// Whether he asked about the level, or to be congratulated, now or
    /// just before: else a level-up is not said ([`brain::without_level_ups`]).
    level_asked: bool,
    kept: Vec<String>,
    total: usize,
    dropped: usize,
    restated: Vec<String>,
}

impl Saying<'_> {
    /// A sentence of the reply (or a few), as it is written: kept, and
    /// handed to the voice, unless it was said lately, only says the
    /// game's state back to someone who did not ask about it, or announces
    /// a level-up he did not ask about.
    fn take(&mut self, sentence: &str, recent: &mut brain::Recent, voice: &Sender<String>) {
        let (sentence, level_ups) = brain::without_level_ups(sentence, self.level_asked);
        if !level_ups.is_empty() {
            self.restated.extend(level_ups);
            if sentence.trim().is_empty() {
                return;
            }
        }
        let sentence = sentence.as_str();
        let sentence = if self.unasked {
            match brain::without_status(&brain::for_speech(sentence), self.facts) {
                Some(kept) => kept,
                None => {
                    self.restated.push(sentence.to_string());
                    return;
                }
            }
        } else {
            sentence.to_string()
        };
        self.total += 1;
        if self.again || recent.fresh(&sentence) {
            let _ = voice.send(brain::for_speech(&sentence));
            self.kept.push(sentence);
        } else {
            self.dropped += 1;
        }
    }
}

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
        facts,
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
    let learned = brain.learned();
    // Classic World: what it doesn't have, and to say when it's not sure.
    let classic = brain::classic_world(snapshot, &learned);
    if classic {
        instructions.push_str(CLASSIC_GUIDE);
    }
    if let Some(toolbox) = toolbox {
        instructions.push_str(TOOLS_GUIDE);
        if toolbox.web {
            instructions.push_str(LOOKUP_GUIDE);
        }
    }
    if !learned.is_empty() {
        instructions.push_str(LEARNED_GUIDE);
        instructions.push('\n');
        instructions.push_str(&learned);
    }
    // A sentence about the screen gets it sharp and close up; the language
    // of the answer is his sentence's (or the one he asked for), to a man.
    let close = brain::about_the_screen(&heard);
    let note = brain::answer_note(&brain.language_for(&heard, language));
    // The voice sounds like the attitude of the moment.
    let attitude = brain.attitude();
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
    let mut input = input_of(&turns, snapshot, eyes, &helps, close, &note);
    let tools = toolbox.map(|t| t.definitions()).unwrap_or_default();
    // The brain for this reply.
    let mut chat = fast.unwrap_or(openai);
    let mut fast_failed = None;
    // Nothing said lately is said again (unless they asked to hear it
    // again), nor the game's state to someone who did not ask about it:
    // the sentences kept, and how many went.
    let mut saying = Saying {
        again: brain::asks_again(&heard),
        unasked: !brain::asks_about_the_game(&heard),
        facts,
        level_asked: brain::asks_about_the_level(&heard)
            || turns
                .iter()
                .rev()
                .filter(|t| t.role == "user" && !t.text.starts_with(brain::WATCHER))
                .skip(1)
                .take(2)
                .any(|t| brain::asks_about_the_level(&t.text)),
        kept: Vec::new(),
        total: 0,
        dropped: 0,
        restated: Vec::new(),
    };
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
                    // (A long piece of the reply is an explanation: a touch
                    // slower and steadier.)
                    let delivery = Delivery::of(attitude, Kind::Reply, &text);
                    match speak_line(mouth, id, stop, &text, delivery, started, first, &tx) {
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
                                saying.take(&sentence, &mut brain.recent, &lines);
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
                        saying.take(&sentence, &mut brain.recent, &lines);
                    }
                }
            }
            // Only look-ups, after the answer was said: nothing to wait for.
            let mut go_on = false;
            let mut outputs = Vec::new();
            for call in &answer.calls {
                let (mut output, effect) = match toolbox {
                    // (A setting or the notebook changes only when his own
                    // sentence plainly asks for it.)
                    Some(toolbox) => {
                        toolbox.run_heard(call, eyes.map(|e| e.frame.as_ref()), &heard)
                    }
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
                        // (Checked for the world they play in.)
                        let question = if classic && !brain::classic_world(&question, "") {
                            format!("{question} ({})", lookup::CLASSIC)
                        } else {
                            question
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
                        saying.take(&rest, &mut brain.recent, &lines);
                    }
                    // The game's state, said back, was all there was: a
                    // question keeps it (better than nothing); asked to
                    // talk, it is here and listening (a card: the state
                    // stays unsaid); anything else needs no answer ("OK").
                    if saying.kept.is_empty()
                        && saying.dropped == 0
                        && brain::wants_an_answer(&heard)
                    {
                        if brain::asked_to_talk(&heard) {
                            let card = brain.here(brain::is_hebrew(&heard));
                            saying.take(card, &mut brain.recent, &lines);
                        } else {
                            saying.unasked = false;
                            for sentence in std::mem::take(&mut saying.restated) {
                                saying.take(&sentence, &mut brain.recent, &lines);
                            }
                        }
                    }
                    if !saying.restated.is_empty() {
                        let _ = tx.send(Done::Noted {
                            line: format!(
                                "not said, nobody asked: {}",
                                brain::for_speech(&saying.restated.join(" "))
                            ),
                        });
                    }
                    brain::for_speech(&brain::without_announcement(&saying.kept.join(" ")))
                } else {
                    let reply = brain::for_speech(&brain::without_announcement(&said));
                    // (A level-up nobody asked about is not said.)
                    let (reply, level_ups) = brain::without_level_ups(&reply, saying.level_asked);
                    if !level_ups.is_empty() {
                        let _ = tx.send(Done::Noted {
                            line: format!("not said, nobody asked: {}", level_ups.join(" ")),
                        });
                    }
                    // (Asked to talk, and nothing but the game's state:
                    // here and listening, as above.)
                    let whole = if brain::asked_to_talk(&heard)
                        && !brain::asks_about_the_game(&heard)
                        && brain::without_status(&reply, facts).is_none()
                    {
                        brain.here(brain::is_hebrew(&heard)).to_string()
                    } else {
                        brain::unasked(&reply, &heard, facts)
                    };
                    if saying.again {
                        whole
                    } else {
                        let filtered = brain.recent.filter(&whole);
                        saying.total = filtered.total;
                        saying.dropped = filtered.dropped;
                        filtered.text
                    }
                };
                let (total, dropped) = (saying.total, saying.dropped);
                looped = total >= 2 && dropped * 2 >= total;
                if dropped > 0 {
                    let _ = tx.send(Done::Noted {
                        line: format!("{dropped} of {total} sentences said before, left out"),
                    });
                }
                brain.heard(&heard);
                if text.is_empty() && dropped > 0 && !brain::a_word_or_two(&heard) {
                    // Nothing new in it: not the same again, but not
                    // silence either — they asked, and hear that they were
                    // heard (a card in its attitude: "Same as before." the
                    // third time is a machine's). The conversation keeps
                    // none of what was dropped, so the model has no loop
                    // of its own to follow. (To a word or two — "OK",
                    // "Danny" — nothing new is nothing to say: quiet.)
                    let card = brain.same_as_before();
                    let line = match language {
                        Some(l) if !language::is_english(l) => {
                            translate(openai, card, l, stop, translations, LINE_TRANSLATION)
                        }
                        _ => card.to_string(),
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
                    // Nothing in it to say (a link, an emoji, the game's
                    // state nobody asked for, all of it said lately to an
                    // "OK"): quiet.
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
    let attitude = brain.attitude();
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
        let mut translations = Translations::default();
        say_line(
            mouth,
            id,
            stop,
            Line {
                text: HELLO,
                language,
                kind: Kind::Reply,
                show: true,
                aloud: true,
                attitude,
            },
            &mut translations,
            tx,
            &mut |said| brain.said(said),
        );
        return;
    }
    let _ = tx.send(Done::Shown {
        kind: Kind::Reply,
        text: text.clone(),
    });
    brain.said(&text);
    let delivery = Delivery::of(attitude, Kind::Reply, &text);
    if let Err(error) = speak_line(mouth, id, stop, &text, delivery, asked, false, tx) {
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

    #[test]
    fn a_line_with_other_numbers_is_translated_once() {
        assert_eq!(
            numbers_of("HP 25.5 percent, MP 1,200. Pot now!"),
            (
                "HP {} percent, MP {}. Pot now!".to_string(),
                vec!["25.5".to_string(), "1,200".to_string()]
            )
        );
        let mut cache = Translations::default();
        cache.put(
            "he-IL",
            "HP 25 percent. Pot now!",
            "25 אחוז חיים. תשתה עכשיו!",
        );
        // The same line with another number: no call, the number put in.
        assert_eq!(
            cache.get("he-IL", "HP 20 percent. Pot now!").as_deref(),
            Some("20 אחוז חיים. תשתה עכשיו!")
        );
        assert_eq!(cache.get("fr-FR", "HP 20 percent. Pot now!"), None);
        assert_eq!(cache.get("he-IL", "HP 20 percent. Drink!"), None);
        // Two numbers the translation puts the other way round, and a
        // number of its own that stays.
        cache.put("he-IL", "HP 30, MP 40.", "מאנה 40, חיים 30 (v2).");
        assert_eq!(
            cache.get("he-IL", "HP 12, MP 9.").as_deref(),
            Some("מאנה 9, חיים 12 (v2).")
        );
        // A number not found once each (written in words, said twice, two
        // the same): kept for its own line only.
        for (line, done) in [
            ("Level 10!", "רמה עשר!"),
            ("Level 11! 11!", "רמה 11! 11!"),
            ("EXP 50, HP 50.", "ניסיון 50, חיים 50."),
        ] {
            cache.put("he-IL", line, done);
            assert_eq!(cache.get("he-IL", line).as_deref(), Some(done));
        }
        assert_eq!(cache.get("he-IL", "Level 12!"), None);
        assert_eq!(cache.get("he-IL", "Level 12! 12!"), None);
        assert_eq!(cache.get("he-IL", "EXP 60, HP 70."), None);
        // A line without numbers, as before.
        cache.put("he-IL", "Level up! Nice.", "עלית רמה! יפה.");
        assert_eq!(
            cache.get("he-IL", "Level up! Nice.").as_deref(),
            Some("עלית רמה! יפה.")
        );
    }

    #[test]
    fn a_template_never_puts_a_number_in_another_numbers_grammar() {
        // A template learned from 25 said "1 אחוז", "Te quedan 1 de PM",
        // and in Russian "21 процентов", "22 процентов" (процент, процента).
        let mut cache = Translations::default();
        cache.put(
            "he-IL",
            "HP 25 percent. Pot now!",
            "25 אחוז חיים. תשתה עכשיו!",
        );
        assert_eq!(cache.get("he-IL", "HP 1 percent. Pot now!"), None);
        assert_eq!(cache.get("he-IL", "HP 1.5 percent. Pot now!"), None);
        assert_eq!(
            cache.get("he-IL", "HP 2 percent. Pot now!").as_deref(),
            Some("2 אחוז חיים. תשתה עכשיו!")
        );
        cache.put("es-ES", "20 MP left. Drink.", "Te quedan 20 de PM. Bebe.");
        assert_eq!(cache.get("es-ES", "1 MP left. Drink."), None);
        assert_eq!(
            cache.get("es-ES", "21 MP left. Drink.").as_deref(),
            Some("Te quedan 21 de PM. Bebe.")
        );
        // Learned from a one, no template at all ("Te queda 20").
        cache.put("es-ES", "1 HP left. Drink.", "Te queda 1 de HP. Bebe.");
        assert_eq!(cache.get("es-ES", "20 HP left. Drink."), None);
        assert_eq!(
            cache.get("es-ES", "1 HP left. Drink.").as_deref(),
            Some("Te queda 1 de HP. Bebe.")
        );
        // Russian: the noun follows the number's last digits — each line
        // its own.
        cache.put(
            "ru-RU",
            "HP 25 percent. Pot now!",
            "HP 25 процентов. Пей зелье!",
        );
        assert_eq!(cache.get("ru-RU", "HP 22 percent. Pot now!"), None);
        assert_eq!(
            cache.get("ru-RU", "HP 25 percent. Pot now!").as_deref(),
            Some("HP 25 процентов. Пей зелье!")
        );
    }
}
