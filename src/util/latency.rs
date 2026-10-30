//! How long each stage of a voice turn took, for the session log. Off
//! unless asked for ([`FLAG`] on the command line, or [`ENV`] set), and
//! then it only writes down: nothing here changes what MapleSyrup does.
//!
//! A turn is one sentence of the player's and the answer to it. Its stages
//! are stamped where they happen, each with the [`Instant`] it happened at:
//! on the phone (when its microphone last heard a voice, when its
//! recogniser last changed the words, when it closed the sentence, when the
//! sentence was sent, when a clip began to play — on the PC's clock: its
//! uptime as the page last heard it, plus half a trip), in the phone link,
//! in the main loop, on Claude's board, in the worker and in the voice.
//! Once its first sound was heard and its voice is all made — or it ended
//! without one — the turn goes into the session log as one line: every
//! stage in milliseconds from when the phone's recogniser last heard words
//! (or, without the phone's stamps, from when the PC took the sentence).
//! `tools/latency_table.py` makes a table of the lines.
//!
//! ```text
//! 21:04:44  [latency] turn=3 mode=claude game=front at=21:04:43.512 voice=-260 spoke=0 sent=566
//!           arrived=690 heard=703 bridge=705 say=2830 queued=2841 lane=2842 tts_sent=2842
//!           tts_first=3390 tts_done=4120 clip=4122 played=4610 spans=look_at_screen@1210+380
//!           settle=550 rtt=96 chars=74 tts=elevenlabs end=played
//! ```

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use serde_json::Value;

/// The command-line flag that turns it on…
pub const FLAG: &str = "--latency";
/// …or this environment variable, set to anything but nothing or [`ENV_OFF`].
pub const ENV: &str = "MAPLESYRUP_LATENCY";
/// The value of [`ENV`] that leaves it off.
pub const ENV_OFF: &str = "0";
/// The session log's kind for its lines (`[latency] …`).
pub const LOG_KIND: &str = "latency";
/// The session log's first line of it, saying how to read the rest.
pub const LOG_HEADER: &str = "on: one line per voice turn, each stage in milliseconds from when the \
phone's recogniser last heard words (spoke=0; without the phone's stamps, from when the PC took the \
sentence: heard=0; on a live call, from when OpenAI heard him stop: live_stop=0); at= is that moment";
/// The field the phone's status has (true) while it is on, and the field
/// of a sentence the phone sends (`/api/heard`) with the phone's stamps.
pub const STATUS_FIELD: &str = "latency";
/// Where the phone sends the rest of its stamps: a clip it began to play,
/// a turn of its live call.
pub const PHONE_PATH: &str = "/api/latency";

/// What the phone sends, by name (`page.html` writes the same names).
pub mod field {
    /// Its microphone last heard a voice (louder than the room).
    pub const VOICE: &str = "voice";
    /// Its recogniser last changed the words.
    pub const SPOKE: &str = "spoke";
    /// Its recogniser closed the sentence (absent: the pause did).
    pub const FINAL: &str = "final";
    /// It sent the sentence.
    pub const SENT: &str = "sent";
    /// How long it waits for more words before it sends them (ms).
    pub const SETTLE: &str = "settle";
    /// How long a request takes it to the PC and back (ms).
    pub const RTT: &str = "rtt";
    /// The clip that began to play, and when.
    pub const CLIP: &str = "clip";
    pub const PLAYED: &str = "played";
    /// A turn of its live call: when OpenAI heard the player start and
    /// stop talking, its response begin, and its first sound.
    pub const LIVE: &str = "live";
    pub const LIVE_START: &str = "start";
    pub const LIVE_STOP: &str = "stop";
    pub const LIVE_RESPONSE: &str = "response";
    pub const LIVE_SOUND: &str = "sound";
    /// The tools the call ran before it answered, and from when to when.
    pub const LIVE_TOOLS: &str = "tools";
    pub const LIVE_TOOLS_FROM: &str = "tools_from";
    pub const LIVE_TOOLS_TO: &str = "tools_to";
}

/// What a span of a turn is, when it is not one of Claude's tools or the
/// model's: the screen made into pictures for the model, sharp and close
/// up (a sentence about the screen) or small.
pub const PICTURES_CLOSE: &str = "pictures_close";
pub const PICTURES_SMALL: &str = "pictures_small";
/// Which voice spoke a turn's first line.
pub const TTS_ELEVENLABS: &str = "elevenlabs";
pub const TTS_OPENAI: &str = "openai";
/// Who answered a turn of Claude's that Claude took too long for.
pub const ANSWERED_HERE: &str = "maplesyrup";

/// How long a turn may stay open: past it, it is written with what it
/// has (a voice request may take 40 s before curl gives it up).
pub const TURN_DEADLINE: Duration = Duration::from_secs(60);
/// How long the phone may take to say a clip began to play before the
/// turn is written without it.
pub const PLAYED_WAIT: Duration = Duration::from_secs(15);
/// How long a stamp for a turn not open yet waits for it: the worker may
/// begin a job, or the bridge take his words for Claude, a moment before
/// the main loop has opened their turn.
const EARLY_FOR: Duration = Duration::from_secs(2);
/// The most turns open at once, the most stamps waiting for a turn, and
/// the most stamps come early.
const OPEN_MOST: usize = 32;
const WAITING_MOST: usize = 16;
const EARLY_MOST: usize = 64;

/// What a stamp belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    /// A job of the AI worker (its number).
    Job(u64),
    /// The player's words posted for Claude (their `heard` event's number).
    Heard(u64),
    /// A clip handed to the phone (its number).
    Clip(u64),
}

/// Who answers a turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Claude Code, through the channel (`ms::claude`).
    Claude,
    /// MapleSyrup's own brain (OpenAI's model, or Grok).
    Model,
    /// MapleSyrup's own words, with no model (his numbers, a hello).
    Instant,
    /// The phone's live call (OpenAI's realtime voice).
    Live,
}

impl Mode {
    pub fn name(self) -> &'static str {
        match self {
            Mode::Claude => "claude",
            Mode::Model => "model",
            Mode::Instant => "instant",
            Mode::Live => "live",
        }
    }
}

/// The game as the turn began.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Game {
    /// Open, the window in front: its screen is read.
    Front,
    /// Open behind another window, its screen still read (the capture gets
    /// it as it is drawn).
    Covered,
    /// Open behind another window, its screen not read.
    Behind,
    /// Open, but it can't be captured.
    Unavailable,
    /// Not open.
    #[default]
    Closed,
}

impl Game {
    pub fn name(self) -> &'static str {
        match self {
            Game::Front => "front",
            Game::Covered => "covered",
            Game::Behind => "behind",
            Game::Unavailable => "unavailable",
            Game::Closed => "closed",
        }
    }
}

/// A moment in a turn, in the order they come.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// The phone's microphone last heard a voice.
    Voice,
    /// The phone's recogniser last changed the words.
    Spoke,
    /// The phone's recogniser closed the sentence.
    Final,
    /// The phone sent it.
    Sent,
    /// It reached the PC.
    Arrived,
    /// The main loop took it, and handed it on.
    Heard,
    /// The bridge took it for Claude Code.
    Bridge,
    /// Claude's `say` reached the board.
    Say,
    /// The main loop handed the job to the worker.
    Queued,
    /// The worker's thinking lane began it.
    Lane,
    /// The first request to the model went out.
    Request,
    /// The model's first words came.
    FirstToken,
    /// The model began a web search.
    Search,
    /// The first sentence went to the voice.
    FirstSentence,
    /// The model's last words came.
    LlmDone,
    /// The voice was asked for the first line.
    TtsSent,
    /// The first line's first sound came back.
    TtsFirst,
    /// The first line's voice was all made.
    TtsDone,
    /// Heard from on the PC's speakers (as reckoned: what played before
    /// it, not the sound card's own delay).
    PcPlay,
    /// The first line went to the phone, a clip.
    Clip,
    /// The phone began to play it.
    Played,
    /// On a live call: OpenAI heard the player start talking…
    LiveStart,
    /// …and stop.
    LiveStop,
    /// The call's response began.
    LiveResponse,
    /// The call's first sound.
    LiveSound,
}

impl Stage {
    /// Every stage, in the order the line gives them.
    pub const ALL: [Stage; 25] = [
        Stage::Voice,
        Stage::Spoke,
        Stage::Final,
        Stage::Sent,
        Stage::Arrived,
        Stage::Heard,
        Stage::Bridge,
        Stage::Say,
        Stage::Queued,
        Stage::Lane,
        Stage::Request,
        Stage::FirstToken,
        Stage::Search,
        Stage::FirstSentence,
        Stage::LlmDone,
        Stage::TtsSent,
        Stage::TtsFirst,
        Stage::TtsDone,
        Stage::PcPlay,
        Stage::Clip,
        Stage::Played,
        Stage::LiveStart,
        Stage::LiveStop,
        Stage::LiveResponse,
        Stage::LiveSound,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Stage::Voice => "voice",
            Stage::Spoke => "spoke",
            Stage::Final => "final",
            Stage::Sent => "sent",
            Stage::Arrived => "arrived",
            Stage::Heard => "heard",
            Stage::Bridge => "bridge",
            Stage::Say => "say",
            Stage::Queued => "queued",
            Stage::Lane => "lane",
            Stage::Request => "request",
            Stage::FirstToken => "first_token",
            Stage::Search => "search",
            Stage::FirstSentence => "first_sentence",
            Stage::LlmDone => "llm_done",
            Stage::TtsSent => "tts_sent",
            Stage::TtsFirst => "tts_first",
            Stage::TtsDone => "tts_done",
            Stage::PcPlay => "pc_play",
            Stage::Clip => "clip",
            Stage::Played => "played",
            Stage::LiveStart => "live_start",
            Stage::LiveStop => "live_stop",
            Stage::LiveResponse => "live_response",
            Stage::LiveSound => "live_sound",
        }
    }
}

/// A fact about a turn that is not a moment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Note {
    /// How long the phone waited for more words (ms).
    Settle,
    /// The phone's round trip to the PC (ms).
    Rtt,
    /// How long the first line said is, in characters.
    Chars,
    /// Which voice said it.
    Tts,
    /// Which model answered.
    Model,
    /// Who answered a turn of Claude's, when not Claude ([`ANSWERED_HERE`]).
    AnsweredBy,
    /// How many of Claude's lines for it the board refused (he had moved on).
    Refused,
}

impl Note {
    /// The notes kept as text, in the order the line gives them (the
    /// refusals are counted).
    const ALL: [Note; 6] = [
        Note::Settle,
        Note::Rtt,
        Note::Chars,
        Note::Tts,
        Note::Model,
        Note::AnsweredBy,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Note::Settle => "settle",
            Note::Rtt => "rtt",
            Note::Chars => "chars",
            Note::Tts => "tts",
            Note::Model => "model",
            Note::AnsweredBy => "answered_by",
            Note::Refused => "refused",
        }
    }
}

/// How a turn ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum End {
    /// Its first sound was heard.
    Played,
    /// A newer sentence came before any of its voice was made.
    Superseded,
    /// The model judged it was not for it, or had nothing to say.
    Silent,
    /// Its answer failed.
    Failed,
    /// Its first line went to the phone, which never said it began to
    /// play it (within [`PLAYED_WAIT`]).
    Handed,
    /// Its job is over and none of its voice went anywhere to be heard
    /// (muted, its voice failed, nothing to say aloud).
    Unheard,
    /// It was still open at [`TURN_DEADLINE`].
    Timeout,
}

impl End {
    pub fn name(self) -> &'static str {
        match self {
            End::Played => "played",
            End::Superseded => "superseded",
            End::Silent => "silent",
            End::Failed => "failed",
            End::Handed => "handed",
            End::Unheard => "unheard",
            End::Timeout => "timeout",
        }
    }
}

/// A stretch of a turn: a tool, the pictures made for the model.
#[derive(Debug, Clone, PartialEq)]
struct Span {
    name: String,
    from: Instant,
    to: Instant,
}

/// One turn, while it is open.
#[derive(Debug, Clone)]
struct Turn {
    number: u64,
    mode: Mode,
    game: Game,
    keys: Vec<Key>,
    /// The first time each stage was reached.
    stamps: Vec<(Stage, Instant)>,
    spans: Vec<Span>,
    notes: Vec<(Note, String)>,
    /// Claude's lines for it the board refused.
    refused: u32,
    opened: Instant,
    /// Its job is over, its voice all made.
    voiced: bool,
    end: Option<End>,
}

impl Turn {
    fn new(number: u64, mode: Mode, game: Game, keys: Vec<Key>, opened: Instant) -> Turn {
        Turn {
            number,
            mode,
            game,
            keys,
            stamps: Vec::new(),
            spans: Vec::new(),
            notes: Vec::new(),
            refused: 0,
            opened,
            voiced: false,
            end: None,
        }
    }

    fn at(&self, stage: Stage) -> Option<Instant> {
        self.stamps
            .iter()
            .find(|(s, _)| *s == stage)
            .map(|(_, at)| *at)
    }

    fn has(&self, stage: Stage) -> bool {
        self.at(stage).is_some()
    }

    /// The first time only: a stage reached again (a reply's second line)
    /// is not its start.
    fn stamp(&mut self, stage: Stage, at: Instant) {
        if !self.has(stage) {
            self.stamps.push((stage, at));
        }
    }

    fn note(&mut self, note: Note, value: String) {
        if !self.notes.iter().any(|(n, _)| *n == note) {
            self.notes.push((note, value));
        }
    }

    /// Its first sound was heard: on the phone once the phone said so (a
    /// clip it never says began is given up on after [`PLAYED_WAIT`]),
    /// else on the PC.
    fn sounded(&self, now: Instant) -> bool {
        match self.at(Stage::Clip) {
            Some(clip) => {
                self.has(Stage::Played) || now.saturating_duration_since(clip) >= PLAYED_WAIT
            }
            None => self.has(Stage::PcPlay) || self.has(Stage::LiveSound),
        }
    }

    /// Its voice went somewhere to be heard: the PC's speakers, or the
    /// phone as a clip.
    fn sent_out(&self) -> bool {
        self.has(Stage::PcPlay) || self.has(Stage::Clip)
    }

    /// Whether it is ready for the log: ended, past [`TURN_DEADLINE`], or
    /// its voice all made and its first sound heard (or none to hear).
    fn due(&self, now: Instant) -> bool {
        self.end.is_some()
            || now.saturating_duration_since(self.opened) >= TURN_DEADLINE
            || (self.voiced && (self.sounded(now) || !self.sent_out()))
    }

    /// How it ended, once due.
    fn outcome(&self) -> End {
        if let Some(end) = self.end {
            return end;
        }
        if self.has(Stage::Played) || self.has(Stage::PcPlay) || self.has(Stage::LiveSound) {
            End::Played
        } else if self.has(Stage::Clip) {
            End::Handed
        } else if self.voiced {
            End::Unheard
        } else {
            End::Timeout
        }
    }

    /// Where its milliseconds count from: when the phone's recogniser last
    /// heard words, else when the PC took the sentence, else (a live call)
    /// when OpenAI heard the player stop.
    fn origin(&self) -> Instant {
        [Stage::Spoke, Stage::Heard, Stage::LiveStop]
            .iter()
            .find_map(|s| self.at(*s))
            .or_else(|| self.stamps.iter().map(|(_, at)| *at).min())
            .unwrap_or(self.opened)
    }

    /// The log line: `turn=… mode=… game=… at=… <stage>=<ms> … spans=…
    /// <note>=… end=…`, every moment in milliseconds from [`Turn::origin`];
    /// `at` is the origin's wall-clock time, to the millisecond.
    fn line(&self, now: Instant, wall_now: chrono::DateTime<chrono::Local>) -> String {
        let origin = self.origin();
        let ms = |at: Instant| -> i64 {
            if at >= origin {
                at.duration_since(origin).as_millis() as i64
            } else {
                -(origin.duration_since(at).as_millis() as i64)
            }
        };
        let age =
            chrono::Duration::from_std(now.saturating_duration_since(origin)).unwrap_or_default();
        let mut parts = vec![
            format!("turn={}", self.number),
            format!("mode={}", self.mode.name()),
            format!("game={}", self.game.name()),
            format!("at={}", (wall_now - age).format("%H:%M:%S%.3f")),
        ];
        for stage in Stage::ALL {
            if let Some(at) = self.at(stage) {
                parts.push(format!("{}={}", stage.name(), ms(at)));
            }
        }
        if !self.spans.is_empty() {
            let spans: Vec<String> = self
                .spans
                .iter()
                .map(|s| {
                    format!(
                        "{}@{}+{}",
                        s.name,
                        ms(s.from),
                        s.to.saturating_duration_since(s.from).as_millis()
                    )
                })
                .collect();
            parts.push(format!("spans={}", spans.join(",")));
        }
        for note in Note::ALL {
            if let Some((_, value)) = self.notes.iter().find(|(n, _)| *n == note) {
                parts.push(format!("{}={value}", note.name()));
            }
        }
        if self.refused > 0 {
            parts.push(format!("{}={}", Note::Refused.name(), self.refused));
        }
        parts.push(format!("end={}", self.outcome().name()));
        parts.join(" ")
    }
}

/// The phone's stamps for a sentence, waiting for the main loop to make it
/// a turn.
#[derive(Debug, Clone)]
struct Waiting {
    text: String,
    stamps: Vec<(Stage, Instant)>,
    notes: Vec<(Note, String)>,
}

/// A line of Claude's the board took for a turn, until the main loop hands
/// it to the voice.
#[derive(Debug, Clone)]
struct Said {
    text: String,
    heard: u64,
}

#[derive(Debug)]
struct Inner {
    turns: Vec<Turn>,
    waiting: Vec<Waiting>,
    says: Vec<Said>,
    /// Stamps for a turn not open yet ([`EARLY_FOR`]).
    early: Vec<(Key, Stage, Instant)>,
    /// The game as the main loop last saw it.
    game: Game,
}

/// Every turn's stages, as they are stamped. One for the program
/// ([`TRACER`]); the tests make their own.
#[derive(Debug)]
pub struct Tracer {
    on: AtomicBool,
    next: AtomicU64,
    inner: Mutex<Inner>,
}

/// The program's.
pub static TRACER: Tracer = Tracer::new();

/// Whether [`ENV`] asks for it.
pub fn asked_by_env() -> bool {
    asked_by(std::env::var_os(ENV).as_deref())
}

/// Whether a value of [`ENV`] asks for it: anything but nothing or
/// [`ENV_OFF`].
fn asked_by(value: Option<&std::ffi::OsStr>) -> bool {
    value.is_some_and(|v| !v.is_empty() && v != ENV_OFF)
}

impl Default for Tracer {
    fn default() -> Self {
        Self::new()
    }
}

impl Tracer {
    pub const fn new() -> Tracer {
        Tracer {
            on: AtomicBool::new(false),
            next: AtomicU64::new(1),
            inner: Mutex::new(Inner {
                turns: Vec::new(),
                waiting: Vec::new(),
                says: Vec::new(),
                early: Vec::new(),
                game: Game::Closed,
            }),
        }
    }

    pub fn enable(&self, on: bool) {
        self.on.store(on, Ordering::Relaxed);
    }

    pub fn enabled(&self) -> bool {
        self.on.load(Ordering::Relaxed)
    }

    /// The turns and what waits for them, when it is on (None when off:
    /// nothing is kept).
    fn inner(&self) -> Option<std::sync::MutexGuard<'_, Inner>> {
        self.enabled()
            .then(|| self.inner.lock().unwrap_or_else(|e| e.into_inner()))
    }

    /// The game as the main loop sees it now: the turns that begin take it.
    pub fn game(&self, game: Game) {
        if let Some(mut inner) = self.inner() {
            inner.game = game;
        }
    }

    /// What the phone sent with a sentence (`/api/heard`'s
    /// [`STATUS_FIELD`]): its moments in the PC's uptime in seconds, which
    /// `at` turns into instants. Kept for the turn the sentence becomes.
    pub fn phone_heard(&self, text: &str, stamps: &Value, at: impl Fn(f64) -> Instant) {
        let Some(mut inner) = self.inner() else {
            return;
        };
        let moment = |name: &str| stamps.get(name).and_then(Value::as_f64).map(&at);
        let mut waiting = Waiting {
            text: text.to_string(),
            stamps: Vec::new(),
            notes: Vec::new(),
        };
        for (name, stage) in [
            (field::VOICE, Stage::Voice),
            (field::SPOKE, Stage::Spoke),
            (field::FINAL, Stage::Final),
            (field::SENT, Stage::Sent),
        ] {
            if let Some(at) = moment(name) {
                waiting.stamps.push((stage, at));
            }
        }
        waiting.stamps.push((Stage::Arrived, Instant::now()));
        for (name, note) in [(field::SETTLE, Note::Settle), (field::RTT, Note::Rtt)] {
            if let Some(value) = stamps.get(name).and_then(Value::as_f64) {
                waiting.notes.push((note, format!("{value:.0}")));
            }
        }
        inner.waiting.push(waiting);
        if inner.waiting.len() > WAITING_MOST {
            inner.waiting.remove(0);
        }
    }

    /// The rest of what the phone sends ([`PHONE_PATH`]): a clip that began
    /// to play, or a turn of its live call (written at once).
    pub fn phone(&self, body: &Value, at: impl Fn(f64) -> Instant) {
        let Some(mut inner) = self.inner() else {
            return;
        };
        let moment = |value: &Value, name: &str| value.get(name).and_then(Value::as_f64).map(&at);
        if let (Some(clip), Some(played)) = (
            body.get(field::CLIP).and_then(Value::as_u64),
            moment(body, field::PLAYED),
        ) {
            if let Some(turn) = find(&mut inner.turns, Key::Clip(clip)) {
                turn.stamp(Stage::Played, played);
            }
            return;
        }
        let Some(live) = body.get(field::LIVE) else {
            return;
        };
        let Some(stop) = moment(live, field::LIVE_STOP) else {
            return;
        };
        let number = self.next.fetch_add(1, Ordering::Relaxed);
        let game = inner.game;
        let mut turn = Turn::new(number, Mode::Live, game, Vec::new(), stop);
        for (name, stage) in [
            (field::VOICE, Stage::Voice),
            (field::LIVE_START, Stage::LiveStart),
            (field::LIVE_STOP, Stage::LiveStop),
            (field::LIVE_RESPONSE, Stage::LiveResponse),
            (field::LIVE_SOUND, Stage::LiveSound),
        ] {
            if let Some(at) = moment(live, name) {
                turn.stamp(stage, at);
            }
        }
        if let (Some(names), Some(from), Some(to)) = (
            live.get(field::LIVE_TOOLS).and_then(Value::as_str),
            moment(live, field::LIVE_TOOLS_FROM),
            moment(live, field::LIVE_TOOLS_TO),
        ) {
            turn.spans.push(Span {
                name: names.to_string(),
                from,
                to,
            });
        }
        turn.end = Some(if turn.has(Stage::LiveSound) {
            End::Played
        } else {
            End::Timeout
        });
        push(&mut inner.turns, turn);
    }

    /// The player's sentence `text`, which the main loop took at `heard`,
    /// became a turn that `key` finds, answered as `mode`: with the phone's
    /// stamps for it, if it sent any, the stamps come for `key` just before,
    /// and the game as it is. The turns before it that have no voice made
    /// yet are over: he moved on. Returns its number (None when off).
    pub fn open(&self, key: Key, mode: Mode, text: &str, heard: Instant) -> Option<u64> {
        let mut inner = self.inner()?;
        let now = Instant::now();
        for turn in inner.turns.iter_mut() {
            if turn.end.is_none() && !turn.has(Stage::TtsFirst) {
                turn.end = Some(End::Superseded);
            }
        }
        let number = self.next.fetch_add(1, Ordering::Relaxed);
        let mut turn = Turn::new(number, mode, inner.game, vec![key], now);
        if let Some(i) = inner.waiting.iter().position(|w| w.text == text) {
            let waiting = inner.waiting.remove(i);
            for (stage, at) in waiting.stamps {
                turn.stamp(stage, at);
            }
            turn.notes = waiting.notes;
        }
        turn.stamp(Stage::Heard, heard);
        if matches!(key, Key::Job(_)) {
            turn.stamp(Stage::Queued, now);
        }
        adopt(&mut turn, &mut inner.early, key, now);
        push(&mut inner.turns, turn);
        Some(number)
    }

    /// The turn `key` finds reached `stage` now.
    pub fn stamp(&self, key: Key, stage: Stage) {
        self.stamp_at(key, stage, Instant::now());
    }

    /// The turn `key` finds reached (or is to reach) `stage` at `at`. A job
    /// or words of his with no turn yet keep it a moment, for the turn the
    /// main loop is about to open ([`EARLY_FOR`]).
    pub fn stamp_at(&self, key: Key, stage: Stage, at: Instant) {
        let Some(mut inner) = self.inner() else {
            return;
        };
        match find(&mut inner.turns, key) {
            Some(turn) => turn.stamp(stage, at),
            None if !matches!(key, Key::Clip(_)) => {
                inner.early.push((key, stage, at));
                if inner.early.len() > EARLY_MOST {
                    inner.early.remove(0);
                }
            }
            None => {}
        }
    }

    /// A fact about the turn `key` finds (the first one stays).
    pub fn note(&self, key: Key, note: Note, value: &str) {
        if let Some(mut inner) = self.inner()
            && let Some(turn) = find(&mut inner.turns, key)
        {
            turn.note(note, value.to_string());
        }
    }

    /// A stretch of the turn `key` finds: a tool, the pictures.
    pub fn span(&self, key: Key, name: &str, from: Instant, to: Instant) {
        if let Some(mut inner) = self.inner()
            && let Some(turn) = find(&mut inner.turns, key)
        {
            turn.spans.push(Span {
                name: name.to_string(),
                from,
                to,
            });
        }
    }

    /// Claude's board took the line `text` for his words number `heard`
    /// (Claude's `say`): stamped, and kept until the main loop hands it to
    /// the voice ([`Tracer::said`]).
    pub fn say(&self, heard: u64, text: &str) {
        let Some(mut inner) = self.inner() else {
            return;
        };
        let Some(turn) = find(&mut inner.turns, Key::Heard(heard)) else {
            return;
        };
        turn.stamp(Stage::Say, Instant::now());
        inner.says.push(Said {
            text: text.to_string(),
            heard,
        });
        if inner.says.len() > WAITING_MOST {
            inner.says.remove(0);
        }
    }

    /// Claude's board refused a line for his words number `heard`: he had
    /// moved on.
    pub fn refused(&self, heard: u64) {
        if let Some(mut inner) = self.inner()
            && let Some(turn) = find(&mut inner.turns, Key::Heard(heard))
        {
            turn.refused += 1;
        }
    }

    /// The main loop handed Claude's line `text` to the voice as job `job`:
    /// the job is the turn's from now on (its first line's only: a second
    /// line of Claude's for the same words is not when he first heard the
    /// answer).
    pub fn said(&self, text: &str, job: u64) {
        let Some(mut inner) = self.inner() else {
            return;
        };
        let Some(i) = inner.says.iter().position(|s| s.text == text) else {
            return;
        };
        let said = inner.says.remove(i);
        link(&mut inner, said.heard, job);
    }

    /// Claude took too long for his words number `heard`, and MapleSyrup's
    /// own brain answers them as job `job`.
    pub fn answered_here(&self, heard: u64, job: u64) {
        let Some(mut inner) = self.inner() else {
            return;
        };
        if let Some(turn) = link(&mut inner, heard, job) {
            turn.note(Note::AnsweredBy, ANSWERED_HERE.to_string());
        }
    }

    /// Job `job`'s first line went to the phone as clip `clip`.
    pub fn clip(&self, job: u64, clip: u64) {
        if let Some(mut inner) = self.inner()
            && let Some(turn) = find(&mut inner.turns, Key::Job(job))
            && !turn.has(Stage::Clip)
        {
            turn.stamp(Stage::Clip, Instant::now());
            turn.keys.push(Key::Clip(clip));
        }
    }

    /// Job `job` is over, its voice all made.
    pub fn voiced(&self, job: u64) {
        if let Some(mut inner) = self.inner()
            && let Some(turn) = find(&mut inner.turns, Key::Job(job))
        {
            turn.voiced = true;
        }
    }

    /// The turn `key` finds ended so (silent, failed) — unless it was heard
    /// already: then its line says what was.
    pub fn ended(&self, key: Key, end: End) {
        if let Some(mut inner) = self.inner()
            && let Some(turn) = find(&mut inner.turns, key)
            && turn.end.is_none()
            && !turn.has(Stage::TtsFirst)
        {
            turn.end = Some(end);
        }
    }

    /// The turns ready for the log, as their lines (none when off).
    pub fn due(&self, now: Instant) -> Vec<String> {
        self.due_at(now, chrono::Local::now())
    }

    fn due_at(&self, now: Instant, wall_now: chrono::DateTime<chrono::Local>) -> Vec<String> {
        let Some(mut inner) = self.inner() else {
            return Vec::new();
        };
        let (due, open): (Vec<Turn>, Vec<Turn>) = std::mem::take(&mut inner.turns)
            .into_iter()
            .partition(|t| t.due(now));
        inner.turns = open;
        due.iter().map(|t| t.line(now, wall_now)).collect()
    }
}

/// Job `job` is the voice of the turn of his words number `heard` from
/// now on (its first job only). Returns the turn, when it took the job.
fn link(inner: &mut Inner, heard: u64, job: u64) -> Option<&mut Turn> {
    let now = Instant::now();
    let Inner { turns, early, .. } = inner;
    let turn = find(turns, Key::Heard(heard))?;
    if turn.keys.iter().any(|k| matches!(k, Key::Job(_))) {
        return None;
    }
    let key = Key::Job(job);
    turn.keys.push(key);
    turn.stamp(Stage::Queued, now);
    adopt(turn, early, key, now);
    Some(turn)
}

/// The stamps that came early for `key` ([`EARLY_FOR`]) are `turn`'s (the
/// rest that are too old go). The job was handed over just before it was
/// stamped queued, and the lane may have begun it meanwhile: queued is
/// then when the lane began it.
fn adopt(turn: &mut Turn, early: &mut Vec<(Key, Stage, Instant)>, key: Key, now: Instant) {
    for (_, stage, at) in early.iter().filter(|(k, _, _)| *k == key) {
        turn.stamp(*stage, *at);
    }
    early.retain(|(k, _, at)| *k != key && now.saturating_duration_since(*at) < EARLY_FOR);
    if let (Some(lane), Some(queued)) = (turn.at(Stage::Lane), turn.at(Stage::Queued))
        && lane < queued
        && let Some(stamp) = turn.stamps.iter_mut().find(|(s, _)| *s == Stage::Queued)
    {
        stamp.1 = lane;
    }
}

/// The open turn `key` finds (the newest, if several).
fn find(turns: &mut [Turn], key: Key) -> Option<&mut Turn> {
    turns.iter_mut().rev().find(|t| t.keys.contains(&key))
}

/// A turn opened: the oldest goes if too many are open (it is written,
/// timed out, at the next look).
fn push(turns: &mut Vec<Turn>, turn: Turn) {
    turns.push(turn);
    if turns.len() > OPEN_MOST
        && let Some(oldest) = turns.iter_mut().find(|t| t.end.is_none())
    {
        oldest.end = Some(End::Timeout);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn on() -> Tracer {
        let tracer = Tracer::new();
        tracer.enable(true);
        tracer
    }

    /// `base` and `secs` seconds after it, as the phone link turns the
    /// phone's uptime into instants.
    fn after(base: Instant) -> impl Fn(f64) -> Instant {
        move |secs| base + Duration::from_secs_f64(secs)
    }

    fn field_of<'a>(line: &'a str, name: &str) -> Option<&'a str> {
        line.split(' ')
            .find_map(|part| part.strip_prefix(name)?.strip_prefix('='))
    }

    fn ms_of(line: &str, name: &str) -> i64 {
        field_of(line, name)
            .unwrap_or_else(|| panic!("{name} in {line}"))
            .parse()
            .unwrap()
    }

    fn wall() -> chrono::DateTime<chrono::Local> {
        chrono::Local::now()
    }

    #[test]
    fn off_it_keeps_nothing_and_writes_nothing() {
        let tracer = Tracer::new();
        let base = Instant::now();
        tracer.phone_heard("hello", &json!({"spoke": 1.0}), after(base));
        assert_eq!(
            tracer.open(Key::Job(1), Mode::Model, "hello", Instant::now()),
            None
        );
        tracer.stamp(Key::Job(1), Stage::Lane);
        tracer.voiced(1);
        assert!(tracer.due(Instant::now() + TURN_DEADLINE * 2).is_empty());
        let inner = tracer.inner.lock().unwrap();
        assert!(inner.turns.is_empty() && inner.waiting.is_empty() && inner.says.is_empty());
    }

    #[test]
    fn a_claude_turn_is_one_line_from_the_phone_to_the_phones_first_sound() {
        let tracer = on();
        tracer.game(Game::Front);
        // The phone's clock: its words last changed 1 s into the PC's uptime.
        let base = Instant::now() - Duration::from_secs(5);
        tracer.phone_heard(
            "what level am I",
            &json!({"voice": 0.8, "spoke": 1.0, "sent": 1.55, "settle": 550, "rtt": 96}),
            after(base),
        );
        let seq = 7;
        assert_eq!(
            tracer.open(
                Key::Heard(seq),
                Mode::Claude,
                "what level am I",
                Instant::now()
            ),
            Some(1)
        );
        tracer.stamp(Key::Heard(seq), Stage::Bridge);
        let look = Instant::now();
        tracer.span(
            Key::Heard(seq),
            "look_at_screen",
            look,
            look + Duration::from_millis(300),
        );
        tracer.say(seq, "Level 30.");
        // (A line for words of his it does not know is no turn's.)
        tracer.said("Something else.", 2);
        tracer.said("Level 30.", 3);
        for stage in [Stage::Lane, Stage::TtsSent, Stage::TtsFirst, Stage::TtsDone] {
            tracer.stamp(Key::Job(3), stage);
        }
        tracer.note(Key::Job(3), Note::Tts, TTS_ELEVENLABS);
        tracer.clip(3, 11);
        tracer.voiced(3);
        // Not yet: the phone has not said the clip began.
        assert!(tracer.due(Instant::now()).is_empty());
        tracer.phone(&json!({"clip": 11, "played": 5.4}), after(base));
        let lines = tracer.due_at(Instant::now(), wall());
        assert_eq!(lines.len(), 1, "{lines:?}");
        let line = &lines[0];
        assert!(
            line.starts_with("turn=1 mode=claude game=front at="),
            "{line}"
        );
        // From when its words last changed: the voice before, the rest after,
        // in the order they came.
        assert_eq!(ms_of(line, "spoke"), 0);
        assert_eq!(ms_of(line, "voice"), -200);
        assert_eq!(ms_of(line, "sent"), 550);
        assert_eq!(ms_of(line, "played"), 4400);
        let order = [
            "spoke",
            "sent",
            "arrived",
            "heard",
            "bridge",
            "say",
            "queued",
            "lane",
            "tts_sent",
            "tts_first",
            "tts_done",
            "clip",
            "played",
        ];
        let at: Vec<usize> = order
            .iter()
            .map(|name| line.find(&format!(" {name}=")).expect(name))
            .collect();
        assert!(at.windows(2).all(|w| w[0] < w[1]), "{line}");
        assert!(
            field_of(line, "spans")
                .unwrap()
                .starts_with("look_at_screen@")
        );
        assert!(field_of(line, "spans").unwrap().ends_with("+300"));
        assert_eq!(field_of(line, "settle"), Some("550"));
        assert_eq!(field_of(line, "rtt"), Some("96"));
        assert_eq!(field_of(line, "tts"), Some(TTS_ELEVENLABS));
        assert_eq!(field_of(line, "end"), Some("played"));
        // Written once.
        assert!(tracer.due(Instant::now()).is_empty());
    }

    #[test]
    fn a_model_turn_on_the_pc_waits_for_its_voice_and_one_he_moved_on_from_ends() {
        let tracer = on();
        tracer.open(Key::Job(4), Mode::Model, "hey", Instant::now());
        tracer.stamp(Key::Job(4), Stage::Lane);
        // He said more before any of its voice was made: over.
        tracer.open(
            Key::Job(5),
            Mode::Model,
            "hey, where do I go",
            Instant::now(),
        );
        for stage in [
            Stage::Lane,
            Stage::Request,
            Stage::FirstToken,
            Stage::TtsFirst,
        ] {
            tracer.stamp(Key::Job(5), stage);
        }
        tracer.stamp_at(
            Key::Job(5),
            Stage::PcPlay,
            Instant::now() + Duration::from_millis(150),
        );
        let lines = tracer.due(Instant::now());
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert!(lines[0].contains("mode=model"));
        assert_eq!(field_of(&lines[0], "end"), Some("superseded"));
        // Heard on the PC, but the reply is still being made: not yet.
        assert!(tracer.due(Instant::now()).is_empty());
        tracer.stamp(Key::Job(5), Stage::LlmDone);
        tracer.voiced(5);
        let lines = tracer.due(Instant::now());
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert_eq!(field_of(&lines[0], "end"), Some("played"));
        assert!(ms_of(&lines[0], "pc_play") >= 150);
        // Without the phone's stamps, it counts from when the PC took it.
        assert_eq!(ms_of(&lines[0], "heard"), 0);
        assert!(field_of(&lines[0], "spoke").is_none());
    }

    #[test]
    fn a_stamp_that_comes_just_before_its_turn_opens_is_its() {
        let tracer = on();
        let heard = Instant::now();
        // The worker began the job, and the bridge took his words for
        // Claude, before the main loop had opened their turns.
        tracer.stamp(Key::Job(2), Stage::Lane);
        tracer.stamp(Key::Heard(5), Stage::Bridge);
        // (A clip no turn has is nobody's.)
        tracer.stamp(Key::Clip(1), Stage::Played);
        tracer.open(Key::Job(2), Mode::Model, "where am I", heard);
        tracer.voiced(2);
        let line = tracer.due(Instant::now()).remove(0);
        assert!(field_of(&line, "lane").is_some(), "{line}");
        assert_eq!(ms_of(&line, "heard"), 0);
        tracer.open(Key::Heard(5), Mode::Claude, "what now", heard);
        // Claude's line: the lane began its job before the main loop had
        // made it the turn's.
        tracer.say(5, "Go left.");
        tracer.stamp(Key::Job(6), Stage::Lane);
        tracer.said("Go left.", 6);
        let line = tracer.due(Instant::now() + TURN_DEADLINE).remove(0);
        assert!(field_of(&line, "bridge").is_some(), "{line}");
        assert_eq!(ms_of(&line, "queued"), ms_of(&line, "lane"), "{line}");
        assert!(tracer.inner.lock().unwrap().early.is_empty());
    }

    #[test]
    fn a_turn_nothing_finishes_is_written_at_the_deadline_and_silence_at_once() {
        let tracer = on();
        tracer.open(Key::Heard(1), Mode::Claude, "hm", Instant::now());
        assert!(tracer.due(Instant::now()).is_empty());
        let lines = tracer.due(Instant::now() + TURN_DEADLINE);
        assert_eq!(lines.len(), 1);
        assert_eq!(field_of(&lines[0], "end"), Some("timeout"));
        tracer.open(Key::Job(9), Mode::Model, "ok", Instant::now());
        tracer.ended(Key::Job(9), End::Silent);
        let lines = tracer.due(Instant::now());
        assert_eq!(field_of(&lines[0], "end"), Some("silent"));
    }

    #[test]
    fn a_turn_claude_took_too_long_for_is_answered_here_and_says_so() {
        let tracer = on();
        tracer.open(Key::Heard(8), Mode::Claude, "where now", Instant::now());
        tracer.answered_here(8, 21);
        tracer.stamp(Key::Job(21), Stage::Request);
        tracer.voiced(21);
        let line = tracer.due(Instant::now()).remove(0);
        assert!(line.contains("mode=claude"), "{line}");
        assert!(field_of(&line, "request").is_some(), "{line}");
        assert_eq!(field_of(&line, "answered_by"), Some(ANSWERED_HERE));
    }

    #[test]
    fn claudes_lines_he_moved_on_from_are_counted() {
        let tracer = on();
        tracer.open(Key::Heard(3), Mode::Claude, "a", Instant::now());
        tracer.refused(3);
        tracer.refused(3);
        let lines = tracer.due(Instant::now() + TURN_DEADLINE);
        assert_eq!(field_of(&lines[0], "refused"), Some("2"));
    }

    #[test]
    fn a_live_call_turn_is_written_at_once_from_when_openai_heard_him_stop() {
        let tracer = on();
        tracer.game(Game::Behind);
        let base = Instant::now() - Duration::from_secs(10);
        tracer.phone(
            &json!({"live": {
                "start": 1.0, "stop": 2.5, "response": 2.9, "sound": 3.6,
                "tools": "game_status", "tools_from": 2.95, "tools_to": 3.2,
            }}),
            after(base),
        );
        let lines = tracer.due_at(Instant::now(), wall());
        assert_eq!(lines.len(), 1, "{lines:?}");
        let line = &lines[0];
        assert!(line.contains("mode=live game=behind"), "{line}");
        assert_eq!(ms_of(line, "live_start"), -1500);
        assert_eq!(ms_of(line, "live_stop"), 0);
        assert_eq!(ms_of(line, "live_response"), 400);
        assert_eq!(ms_of(line, "live_sound"), 1100);
        assert_eq!(field_of(line, "spans"), Some("game_status@450+250"));
        assert_eq!(field_of(line, "end"), Some("played"));
    }

    #[test]
    fn the_line_says_when_the_turn_began_to_the_millisecond() {
        let tracer = on();
        tracer.open(Key::Job(1), Mode::Instant, "hp?", Instant::now());
        tracer.voiced(1);
        let wall_now = chrono::Local::now();
        let line = tracer.due_at(Instant::now(), wall_now).remove(0);
        let at = field_of(&line, "at").unwrap();
        // HH:MM:SS.mmm
        assert_eq!(at.len(), 12, "{at}");
        assert_eq!(&at[8..9], ".");
        assert_eq!(field_of(&line, "end"), Some("unheard"));
    }

    #[test]
    fn the_env_switch_is_off_unless_set_to_something_but_off() {
        use std::ffi::OsStr;
        assert!(!asked_by(None));
        for (value, on) in [("", false), (ENV_OFF, false), ("1", true), ("yes", true)] {
            assert_eq!(asked_by(Some(OsStr::new(value))), on, "{value:?}");
        }
    }
}
