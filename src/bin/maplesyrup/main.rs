//! MapleSyrup: the MapleStory companion, as one program you start.
//!
//! It finds the MapleStory window and watches it through the vision engine,
//! and you talk with it through your phone like you would with a friend:
//! with an OpenAI key it answers like ChatGPT, knowing what is on your
//! screen, in a natural voice (the game turned down while it talks). It
//! speaks up on its own when HP or MP runs low or you level up. Yohai's
//! dog shows it all, on a panel over the game and on the phone.
//!
//! ```text
//!  MapleStory window ─ syrup capture ─▶ vision engine ─▶ Observation ─┐
//!                                                                      ▼
//!  phone page ── what you said / buttons / audio ──▶ phone link ──▶ Companion ─┐
//!       ▲                                                   │      (warnings)   │
//!       │                                                   ▼                   ▼
//!       └──── replies, voice clips, the dog ◀──── OpenAI (reply + voice) ──▶ PC speakers
//! ```

mod selftest;

use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use image::RgbaImage;
use ms::ai::teaching::{self, Latest, News};
use ms::ai::{self, AiError, Brain, Done, Eyes, Job, OpenAi, Toolbox};
use ms::app::dog::Dog;
use ms::app::panel;
use ms::app::screen::{self, LogLine};
use ms::app::session::{self, Session};
use ms::capture::{Captured, GameCapture};
use ms::companion::commands::{self, Heard};
use ms::companion::{Action, Command, Companion, GameView, Kind, Observation, Settings};
use ms::observe::frame_result::{FrameTimings, VisionFrameResult};
use ms::observe::preview::Preview;
use ms::phone::{self, Hub, Inbound, VoiceOn, qr, tls, tunnel};
use ms::platform::overlay::{self, Overlay};
use ms::platform::sound::Player;
use ms::platform::{self, voice::Voice};
use ms::sight::Sight;
use ms::sight::things::Fired;
use ms::util::timing::FPSCounter;
use ms::vision::snapshot::PerceptionPipeline;
use serde_json::json;

const USAGE: &str = "\
MapleSyrup — the MapleStory companion.

Start it, start MapleStory (either order), and scan the QR code with your
phone. Then just talk to it. With an OpenAI API key it talks like ChatGPT
and sounds like a person; without one it answers simple questions in the
Windows voice. Close this window or press Ctrl+C to stop.

USAGE
  MapleSyrup [options] [IMAGE | FOLDER]

  IMAGE | FOLDER        watch a screenshot, or a folder of frames, instead of
                        the game (to try MapleSyrup without MapleStory open)

OPTIONS
  --window TEXT         watch the window whose title contains TEXT
                        (a private server, a renamed client)
  --tunnel              link the phone through a Cloudflare tunnel instead of
                        the local network: no certificate warning, works on
                        mobile data, needs the internet
  --port N              the phone link's port (default 8443)
  --no-phone            no phone link
  --record-mic          keep the phone's microphone in the session's mic.wav
  --replies WHERE       where replies are spoken: pc, phone, both, off (default pc)
  --wake-word           answer only sentences that say \"syrup\" (for streams)
  --no-ai               no OpenAI, even with a key
  --no-web              don't let it search the web for MapleStory facts
  --forget-key          delete the saved OpenAI key and ask again
  --model NAME          the OpenAI model (default: the fastest the key can use)
  --voice NAME          the OpenAI voice: cedar, marin, ash, coral, sage… (default cedar)
  --no-voice            never speak on the PC
  --rate N              the Windows voice's speed, -10 to 10 (default 1)
  --hp-low N            warn below N% HP (default 30)
  --mp-low N            warn below N% MP (default 15)
  --fps N               frames watched per second (default 10)
  --preview             also open a window showing what the engine sees
  --no-overlay          no panel over the game window
  --overlay-on-stream   let OBS and screenshots see the panel (by default it
                        keeps out of captures)
  --plain               no colours or redrawing in the console
  --self-test           check this PC: the engine, the phone link, the voice
  --help

The OpenAI key is read from OPENAI_API_KEY, or from openai-key.txt next to
MapleSyrup (moved into %APPDATA%\\MapleSyrup on first use), or asked for.
";

struct Options {
    window: Option<String>,
    input: Option<PathBuf>,
    phone: bool,
    tunnel: bool,
    port: u16,
    record_mic: bool,
    replies: VoiceOn,
    wake_word: bool,
    ai: bool,
    web: bool,
    forget_key: bool,
    model: Option<String>,
    voice_name: String,
    openai_base: String,
    voice: bool,
    rate: i32,
    hp_low: f32,
    mp_low: f32,
    fps: f64,
    preview: bool,
    overlay: bool,
    overlay_on_stream: bool,
    plain: bool,
    self_test: bool,
}

fn parse(args: &[String]) -> Result<Options, String> {
    let mut o = Options {
        window: None,
        input: None,
        phone: true,
        tunnel: false,
        port: 8443,
        record_mic: false,
        replies: VoiceOn::Pc,
        wake_word: false,
        ai: true,
        web: true,
        forget_key: false,
        model: None,
        voice_name: "cedar".into(),
        openai_base: std::env::var("OPENAI_BASE_URL")
            .unwrap_or_else(|_| "https://api.openai.com/v1".into()),
        voice: true,
        rate: 1,
        hp_low: 30.0,
        mp_low: 15.0,
        fps: 10.0,
        preview: false,
        overlay: true,
        overlay_on_stream: false,
        plain: false,
        self_test: false,
    };
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        let (flag, inline) = match arg.split_once('=') {
            Some((f, v)) if f.starts_with("--") => (f, Some(v.to_string())),
            _ => (arg.as_str(), None),
        };
        let mut value = |name: &str| {
            inline
                .clone()
                .or_else(|| it.next().cloned())
                .ok_or(format!("{name} needs a value"))
        };
        let number = |name: &str, text: String| {
            text.parse::<f64>()
                .map_err(|_| format!("{name} takes a number, not {text:?}"))
        };
        match flag {
            "--window" => o.window = Some(value("--window")?),
            "--tunnel" => o.tunnel = true,
            "--port" => o.port = number("--port", value("--port")?)? as u16,
            "--no-phone" => o.phone = false,
            "--record-mic" => o.record_mic = true,
            "--replies" => {
                let text = value("--replies")?;
                o.replies = VoiceOn::parse(&text)
                    .ok_or(format!("--replies is pc, phone, both or off, not {text:?}"))?
            }
            "--wake-word" => o.wake_word = true,
            "--no-ai" => o.ai = false,
            "--no-web" => o.web = false,
            "--forget-key" => o.forget_key = true,
            "--model" => o.model = Some(value("--model")?),
            "--voice" => o.voice_name = value("--voice")?,
            "--openai-base" => o.openai_base = value("--openai-base")?,
            "--no-voice" => o.voice = false,
            "--rate" => o.rate = number("--rate", value("--rate")?)? as i32,
            "--hp-low" => o.hp_low = number("--hp-low", value("--hp-low")?)? as f32,
            "--mp-low" => o.mp_low = number("--mp-low", value("--mp-low")?)? as f32,
            "--fps" => o.fps = number("--fps", value("--fps")?)?.clamp(1.0, 30.0),
            "--preview" => o.preview = true,
            "--no-overlay" => o.overlay = false,
            "--overlay-on-stream" => o.overlay_on_stream = true,
            "--plain" => o.plain = true,
            "--self-test" => o.self_test = true,
            "-h" | "--help" | "/?" => return Err(String::new()),
            other if other.starts_with('-') => return Err(format!("unknown option {other}")),
            path => o.input = Some(PathBuf::from(path)),
        }
    }
    Ok(o)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let options = match parse(&args) {
        Ok(o) => o,
        Err(e) => {
            if !e.is_empty() {
                eprintln!("{e}\n");
            }
            print!("{USAGE}");
            std::process::exit(if e.is_empty() { 0 } else { 2 });
        }
    };
    if options.self_test {
        std::process::exit(selftest::run());
    }
    if let Err(e) = run(options) {
        eprintln!("\nMapleSyrup stopped: {e}");
        pause_if_double_clicked();
        std::process::exit(1);
    }
}

/// Started by double-clicking, the console closes with the program: keep
/// an error on screen until it has been read.
fn pause_if_double_clicked() {
    if std::env::var_os("PROMPT").is_none() {
        eprintln!("Press Enter to close.");
        let mut line = String::new();
        let _ = std::io::stdin().read_line(&mut line);
    }
}

/// The phone link once it is up.
struct PhoneLink {
    hub: Arc<Hub>,
    /// What the QR code holds.
    link: String,
    /// The local-network link, also shown when the tunnel is used.
    lan_link: Option<String>,
    tunneled: bool,
    _tunnel: Option<tunnel::Tunnel>,
}

fn start_phone(
    options: &Options,
    settings: &Path,
    session_dir: &Path,
) -> Result<PhoneLink, String> {
    let key = tls::link_key(settings);
    let record_to = options.record_mic.then(|| session_dir.join("mic.wav"));
    let hub = Hub::new(key.clone(), record_to, options.replies);

    let lan_ip = phone::net::lan_ipv4();
    let mut names = vec!["localhost".to_string(), "127.0.0.1".to_string()];
    if let Some(ip) = lan_ip {
        names.push(ip.to_string());
    }
    let identity = tls::load_or_create(settings, &names)?;
    let config = tls::server_config(&identity)?;
    let port = phone::serve_tls(Arc::clone(&hub), config, options.port)?;
    let lan_link = lan_ip.map(|ip| phone::link(&format!("https://{ip}:{port}"), &key));

    if options.tunnel {
        println!("Opening a Cloudflare tunnel for the phone…");
        let binary = match tunnel::find(settings) {
            Some(found) => found,
            None => {
                println!("  downloading cloudflared (once, about 60 MB)…");
                tunnel::download(settings)?
            }
        };
        let local = phone::serve_local(Arc::clone(&hub), 0)?;
        let opened = tunnel::open(&binary, local, &session_dir.join("tunnel.log"))?;
        let link = phone::link(&opened.url, &key);
        return Ok(PhoneLink {
            hub,
            link,
            lan_link,
            tunneled: true,
            _tunnel: Some(opened),
        });
    }
    let link = lan_link
        .clone()
        .unwrap_or_else(|| phone::link(&format!("https://127.0.0.1:{port}"), &key));
    Ok(PhoneLink {
        hub,
        link,
        lan_link,
        tunneled: false,
        _tunnel: None,
    })
}

/// One frame's worth for the main loop.
struct Tick {
    at: f64,
    obs: Observation,
    frame: Option<VisionFrameResult>,
    /// Alerts of things the player taught, that fired on this frame.
    fired: Vec<Fired>,
}

/// The newest tick; an unread one is replaced (the main loop is never
/// more than a frame behind).
#[derive(Default)]
struct TickSlot(Mutex<Option<Tick>>);

impl TickSlot {
    fn put(&self, tick: Tick) {
        *self.0.lock().unwrap_or_else(|e| e.into_inner()) = Some(tick);
    }
    fn take(&self) -> Option<Tick> {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).take()
    }
}

/// Where frames come from.
enum Source {
    Game(GameCapture),
    Still {
        label: String,
        image: RgbaImage,
    },
    Frames {
        label: String,
        paths: Vec<PathBuf>,
        cursor: usize,
    },
}

impl Source {
    fn open(options: &Options) -> Result<Source, String> {
        if let Some(input) = &options.input {
            if input.is_dir() {
                let mut paths: Vec<PathBuf> = std::fs::read_dir(input)
                    .map_err(|e| format!("{}: {e}", input.display()))?
                    .flatten()
                    .map(|e| e.path())
                    .filter(|p| {
                        p.extension().and_then(|e| e.to_str()).is_some_and(|e| {
                            matches!(
                                e.to_ascii_lowercase().as_str(),
                                "png" | "jpg" | "jpeg" | "bmp"
                            )
                        })
                    })
                    .collect();
                paths.sort();
                if paths.is_empty() {
                    return Err(format!("{} has no .png or .jpg frames", input.display()));
                }
                return Ok(Source::Frames {
                    label: input.display().to_string(),
                    paths,
                    cursor: 0,
                });
            }
            let image = image::open(input)
                .map_err(|e| format!("{}: {e}", input.display()))?
                .to_rgba8();
            return Ok(Source::Still {
                label: input
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "image".into()),
                image,
            });
        }
        Ok(Source::Game(match &options.window {
            Some(title) => GameCapture::titled(title),
            None => GameCapture::auto(),
        }))
    }

    fn next(&mut self) -> Captured {
        match self {
            Source::Game(capture) => capture.capture(),
            Source::Still { label, image } => Captured::Frame {
                title: label.clone(),
                image: image.clone(),
            },
            Source::Frames {
                label,
                paths,
                cursor,
            } => {
                let path = &paths[*cursor % paths.len()];
                *cursor += 1;
                match image::open(path) {
                    Ok(img) => Captured::Frame {
                        title: format!(
                            "{label} [{}/{}]",
                            (*cursor - 1) % paths.len() + 1,
                            paths.len()
                        ),
                        image: img.to_rgba8(),
                    },
                    Err(e) => Captured::Unavailable(format!("{}: {e}", path.display())),
                }
            }
        }
    }
}

/// What the vision thread shares with the rest: the learned sight, the
/// newest frame (for the teacher), and whether the game is in front.
struct Shared {
    sight: Option<Arc<Mutex<Sight>>>,
    latest: Arc<Latest>,
    in_front: Arc<AtomicBool>,
}

/// Capture and the vision engine, on a thread of their own.
fn watch(
    mut source: Source,
    slot: Arc<TickSlot>,
    running: Arc<AtomicBool>,
    start: Instant,
    fps: f64,
    shared: Shared,
) {
    let Shared {
        sight,
        latest,
        in_front,
    } = shared;
    let mut pipeline = PerceptionPipeline::new();
    let mut counter = FPSCounter::new(30);
    let mut frame_id: u64 = 0;
    let mut previous = Instant::now();
    let period = Duration::from_secs_f64(1.0 / fps);
    while running.load(Ordering::Relaxed) {
        let began = Instant::now();
        match source.next() {
            Captured::Frame { title, image } => {
                let capture = began.elapsed();
                frame_id += 1;
                let vision_start = Instant::now();
                let world = pipeline.detect_frame(&image, frame_id);
                let image = Arc::new(image);
                let mut obs = Observation::from_world(&title, &world);
                // What MapleSyrup learned about this screen replaces the
                // old HUD reader's guesses. Only while the game is the window
                // in front: the capture is of the screen where the game is,
                // so another window over it would be measured (and sent to
                // OpenAI) instead.
                let mut fired = Vec::new();
                let in_view = in_front.load(Ordering::Relaxed);
                if in_view {
                    latest.put(Arc::clone(&image));
                } else {
                    latest.clear();
                }
                if let Some(sight) = &sight {
                    let mut sight = sight.lock().unwrap_or_else(|e| e.into_inner());
                    let seen = if in_view {
                        sight.observe(&image, Instant::now())
                    } else {
                        Default::default()
                    };
                    sight.apply(&mut obs, &seen);
                    fired = seen.fired;
                }
                let vision = vision_start.elapsed();
                let now = Instant::now();
                let interval = now.duration_since(previous);
                previous = now;
                let fps = counter.add_frame_seconds(interval.as_secs_f64());
                let frame = VisionFrameResult {
                    frame_id,
                    elapsed_ms: start.elapsed().as_millis() as u64,
                    source: title,
                    image,
                    world: Arc::new(world),
                    timings: FrameTimings {
                        capture,
                        vision,
                        frame_interval: interval,
                    },
                    fps,
                };
                slot.put(Tick {
                    at: start.elapsed().as_secs_f64(),
                    obs,
                    frame: Some(frame),
                    fired,
                });
                if let Some(rest) = period.checked_sub(began.elapsed()) {
                    std::thread::sleep(rest);
                }
            }
            Captured::NotFound => {
                latest.clear();
                slot.put(Tick {
                    at: start.elapsed().as_secs_f64(),
                    obs: Observation::unseen(GameView::NotFound),
                    frame: None,
                    fired: Vec::new(),
                });
                std::thread::sleep(Duration::from_millis(500));
            }
            Captured::Unavailable(why) => {
                latest.clear();
                slot.put(Tick {
                    at: start.elapsed().as_secs_f64(),
                    obs: Observation::unseen(GameView::Unavailable(why)),
                    frame: None,
                    fired: Vec::new(),
                });
                std::thread::sleep(Duration::from_millis(500));
            }
        }
    }
}

/// What the player taught, for the phone: each thing and what it shows
/// now (its picture handed to the phone link when new), and what is known
/// about the HUD.
fn learned_status(sight: Option<&Arc<Mutex<Sight>>>, hub: &Hub) -> serde_json::Value {
    let Some(sight) = sight else {
        return serde_json::Value::Null;
    };
    let sight = sight.lock().unwrap_or_else(|e| e.into_inner());
    let things: Vec<serde_json::Value> = sight
        .things
        .list
        .iter()
        .map(|t| {
            let tag = t.pictures.first().cloned().unwrap_or_default();
            if !tag.is_empty()
                && hub.thumb_tag(&t.id).as_deref() != Some(tag.as_str())
                && let Some(png) = sight.things.thumbnail(&t.id)
            {
                hub.set_thumb(&t.id, &tag, png);
            }
            json!({
                "id": t.id,
                "name": t.name,
                "kind": t.kind.label(),
                "now": t.live.reading.as_ref().map(|r| r.describe()),
                "alert": t.alert.as_ref().map(|a| a.say.clone()),
                "picture": !tag.is_empty(),
            })
        })
        .collect();
    json!({
        "things": things,
        "hud": sight.layout.is_some(),
        "level": sight.facts.level,
        "level_from": sight.facts.level_from,
    })
}

/// "iPhone", "Android phone", "iPad"… from a browser's user agent.
fn device_of(agent: &str) -> &'static str {
    if agent.contains("iPhone") {
        "iPhone"
    } else if agent.contains("iPad") {
        "iPad"
    } else if agent.contains("Android") {
        "Android phone"
    } else if agent.contains("Macintosh") || agent.contains("Windows") || agent.contains("Linux") {
        "computer's browser"
    } else {
        "phone"
    }
}

/// How MapleSyrup's words get out: OpenAI's natural voice (played here,
/// or handed to the phone), or the Windows voice when there is no key.
struct Mouth {
    ai: Option<ai::Worker>,
    sapi: Option<Voice>,
    player: Player,
    /// When the Windows voice or the phone is expected to finish speaking.
    sapi_until: Instant,
    phone_until: Instant,
    /// When the phone is expected to finish the clips it was handed.
    phone_end: Instant,
    /// The game's process, turned down while the PC speaks.
    game_pid: Option<u32>,
}

impl Mouth {
    fn speaking(&self) -> bool {
        let now = Instant::now();
        self.player.speaking() || now < self.sapi_until || now < self.phone_until
    }

    fn hush(&mut self) {
        self.player.stop();
        if let Some(voice) = &self.sapi {
            voice.hush();
        }
        self.sapi_until = Instant::now();
    }

    /// Roughly how long the Windows voice takes to say `text`.
    fn estimate(text: &str) -> Duration {
        Duration::from_secs_f64(0.6 + text.chars().count() as f64 / 14.0)
    }
}

/// Everything an action can touch.
struct Outputs {
    mouth: Mouth,
    phone: Option<Arc<Hub>>,
    session: Session,
    log: Vec<LogLine>,
    plain: bool,
    start: Instant,
    replies: VoiceOn,
}

impl Outputs {
    fn voice_on(&self) -> VoiceOn {
        self.phone
            .as_ref()
            .map(|h| h.voice_on())
            .unwrap_or(self.replies)
    }

    /// Show a line everywhere (console, phone, session log).
    fn show(&mut self, kind: Kind, text: &str) {
        let who = match kind {
            Kind::Heard => "heard",
            Kind::Alert => "alert",
            Kind::Reply => "reply",
            Kind::Info => "info",
        };
        self.session.line(who, text);
        if let Some(hub) = &self.phone {
            // The phone speaks a line itself only without a natural voice.
            let phone_speaks = self.mouth.ai.is_none() && matches!(kind, Kind::Alert | Kind::Reply);
            hub.post(kind, text, phone_speaks);
        }
        self.push(kind, text.to_string());
    }

    /// Say `text` out loud, where replies are spoken.
    fn speak(&mut self, text: &str, companion: &mut Companion) {
        if companion.muted() {
            return;
        }
        let now = self.start.elapsed().as_secs_f64();
        companion.remember_spoken(now, text);
        match &self.mouth.ai {
            Some(worker) => worker.send(Job::Speak {
                text: text.to_string(),
            }),
            None => {
                if self.voice_on().pc()
                    && let Some(voice) = &self.mouth.sapi
                {
                    voice.say(text);
                    self.mouth.sapi_until = Instant::now() + Mouth::estimate(text);
                }
            }
        }
    }

    /// Play a natural-voice clip where replies are spoken, after the ones
    /// before it (a reply comes a sentence at a time).
    fn play(&mut self, samples: Vec<i16>, companion: &Companion) {
        if companion.muted() {
            return;
        }
        let voice_on = self.voice_on();
        if voice_on.phone()
            && let Some(hub) = &self.phone
        {
            hub.set_clip(ai::wav_bytes(&samples, ai::openai::SPEECH_RATE));
            let length =
                Duration::from_secs_f64(samples.len() as f64 / ai::openai::SPEECH_RATE as f64);
            // The phone fetches it within a poll and plays it after the last.
            let begins = self
                .mouth
                .phone_end
                .max(Instant::now() + Duration::from_millis(500));
            self.mouth.phone_end = begins + length;
            self.mouth.phone_until = self.mouth.phone_end + Duration::from_millis(800);
        }
        if voice_on.pc() {
            self.mouth
                .player
                .enqueue(samples, ai::openai::SPEECH_RATE, self.mouth.game_pid);
        }
    }

    fn apply(
        &mut self,
        actions: Vec<Action>,
        companion: &mut Companion,
        frame: Option<Arc<RgbaImage>>,
    ) {
        for action in actions {
            match action {
                Action::Say(say) => {
                    self.show(say.kind, &say.text);
                    if say.speak {
                        self.speak(&say.text, companion);
                    }
                }
                Action::Mark => {
                    let elapsed = self.start.elapsed().as_secs_f64();
                    match self.session.mark(elapsed, companion.last(), frame.clone()) {
                        Some(path) => self.push(
                            Kind::Info,
                            format!(
                                "saved {}",
                                path.file_name()
                                    .map(|f| f.to_string_lossy())
                                    .unwrap_or_default()
                            ),
                        ),
                        None => self.push(Kind::Info, "marked (no game frame to save)".into()),
                    }
                }
                Action::SetMuted(true) => self.mouth.hush(),
                Action::SetMuted(false) => {}
            }
        }
    }

    fn push(&mut self, kind: Kind, text: String) {
        let time = chrono::Local::now().format("%H:%M:%S").to_string();
        if self.plain {
            let who = match kind {
                Kind::Heard => "you",
                Kind::Alert => "syrup!",
                Kind::Reply => "syrup",
                Kind::Info => "·",
            };
            println!("{time}  {who:<7} {text}");
        }
        self.log.push(LogLine {
            time,
            kind,
            text,
            at: Instant::now(),
        });
        if self.log.len() > 200 {
            self.log.drain(..100);
        }
    }
}

/// The OpenAI key: saved, or asked for in this window (when there is one to
/// type in). `None` runs MapleSyrup with its own simple answers.
fn openai_key(options: &Options, settings: &Path) -> Option<String> {
    if !options.ai {
        return None;
    }
    if options.forget_key {
        let _ = std::fs::remove_file(ai::key_file(settings));
    }
    if let Some(key) = ai::load_key(settings) {
        return Some(key);
    }
    if !std::io::stdin().is_terminal() {
        return None;
    }
    println!("MapleSyrup talks like ChatGPT, in a natural voice, with an OpenAI API key");
    println!(
        "(platform.openai.com/api-keys). It is kept on this PC only, in {}.",
        settings.display()
    );
    println!("Paste the key and press Enter, or just press Enter to go without:");
    let mut line = String::new();
    let _ = std::io::stdin().read_line(&mut line);
    let key = ai::clean_key(&line).to_string();
    if !ai::looks_like_key(&key) {
        if !key.is_empty() {
            println!("That does not look like an OpenAI key (they start with sk-). Going without.");
        }
        return None;
    }
    if let Err(e) = ai::save_key(settings, &key) {
        println!("Could not save the key ({e}); it is used for this run only.");
    }
    Some(key)
}

fn run(options: Options) -> Result<(), String> {
    let console = platform::init("MapleSyrup (close this window or press Ctrl+C to stop)");
    let ansi = console.ansi && !options.plain;
    let start = Instant::now();
    let settings_dir = tls::settings_dir();
    let session_dir = session::new_dir(&session::sessions_base(&settings_dir));
    let session = Session::open(session_dir.clone());

    // The brain: OpenAI when there is a key that works. With it come the
    // eyes: a vision model that teaches MapleSyrup the player's screen.
    let mut ai_note = String::from("no OpenAI key: simple answers, Windows voice");
    let latest = Arc::new(Latest::default());
    let (news_tx, news_rx) = mpsc::channel::<News>();
    let mut sight: Option<Arc<Mutex<Sight>>> = None;
    let worker = match openai_key(&options, &settings_dir) {
        Some(key) => {
            println!("Connecting to OpenAI…");
            let client = OpenAi::new(
                &key,
                &options.openai_base,
                &options.voice_name,
                options.model.as_deref(),
            );
            match client.check() {
                Err(AiError::Http(401, _)) => {
                    let _ = std::fs::remove_file(ai::key_file(&settings_dir));
                    ai_note =
                        "OpenAI refused the key (removed; start again to paste another)".into();
                    None
                }
                result => {
                    if let Err(e) = &result {
                        ai_note = format!("OpenAI not reachable yet ({e}); will keep trying");
                    } else {
                        ai_note = "OpenAI: conversation and natural voice".into();
                    }
                    let mut brain = Brain::new();
                    if let Ok(about) = std::fs::read_to_string(settings_dir.join("about-me.txt")) {
                        brain.about_player = about;
                    }
                    let learned = Arc::new(Mutex::new(Sight::load(&settings_dir.join("learned"))));
                    let eye_models: Vec<String> = match &options.model {
                        Some(m) => vec![m.clone()],
                        None => ai::openai::VISION_MODELS
                            .iter()
                            .map(|m| m.to_string())
                            .collect(),
                    };
                    let eyes = Arc::new(OpenAi::with_models(
                        &key,
                        &options.openai_base,
                        &options.voice_name,
                        eye_models,
                    ));
                    teaching::spawn(
                        Arc::clone(&eyes),
                        Arc::clone(&learned),
                        Arc::clone(&latest),
                        news_tx.clone(),
                    );
                    let toolbox = Toolbox {
                        sight: Arc::clone(&learned),
                        eyes,
                        settings: settings_dir.clone(),
                        web: options.web,
                    };
                    sight = Some(learned);
                    Some(ai::spawn_with(client, brain, Some(toolbox)))
                }
            }
        }
        None => None,
    };

    let sapi = if options.voice && worker.is_none() {
        Voice::start(options.rate).ok()
    } else if options.voice {
        // Kept for when OpenAI cannot be reached.
        Voice::start(options.rate).ok()
    } else {
        None
    };
    let voice_label = match (&worker, options.voice) {
        (_, false) => "PC voice off".to_string(),
        (Some(_), true) => format!("natural voice ({})", options.voice_name),
        (None, true) if sapi.is_some() => "Windows voice".to_string(),
        (None, true) => "no PC voice".to_string(),
    };

    let phone = if options.phone {
        match start_phone(&options, &settings_dir, &session_dir) {
            Ok(link) => Some(link),
            Err(e) => {
                eprintln!("The phone link could not start: {e}\nMapleSyrup carries on without it.");
                None
            }
        }
    } else {
        None
    };

    // The header, printed once.
    let (b, d, s, r) = if ansi {
        ("\x1b[1m", "\x1b[2m", "\x1b[38;5;214m", "\x1b[0m")
    } else {
        ("", "", "", "")
    };
    if ansi {
        // Clear the screen, and clip long lines instead of wrapping them so
        // the live block can be redrawn in place.
        print!("\x1b[2J\x1b[H\x1b[?7l");
    }
    println!(
        "{b}Maple{s}Syrup{r}{b} · the MapleStory companion{r} {d}v{} · {ai_note}{r}",
        env!("CARGO_PKG_VERSION")
    );
    if let Some(link) = &phone {
        println!("{d}Phone:{r} {}", link.link);
        if link.tunneled {
            println!(
                "{d}  through a Cloudflare tunnel (local network link: {}){r}",
                link.lan_link.as_deref().unwrap_or("none")
            );
        } else {
            println!(
                "{d}  The phone warns the page is \"not private\" (the certificate was made on this PC): on iPhone tap Show Details → visit this website.{r}"
            );
            println!(
                "{d}  Phone on the same Wi-Fi. If Windows asks, allow MapleSyrup on private networks. No luck? Start it with --tunnel.{r}"
            );
        }
    }
    println!("{d}Session files: {}{r}", session_dir.display());
    if !console.dpi_aware {
        println!(
            "{d}(This display is scaled and DPI awareness could not be set: the HUD may read worse.){r}"
        );
    }
    println!();

    let qr_lines: Vec<String> = phone
        .as_ref()
        .and_then(|p| qr::console(&p.link))
        .map(|mut lines| {
            lines.push(String::new());
            lines.push(" Scan with your phone's camera".into());
            lines
        })
        .unwrap_or_default();
    if !ansi {
        for line in &qr_lines {
            println!("{line}");
        }
    }

    let source = Source::open(&options)?;
    let slot = Arc::new(TickSlot::default());
    let running = Arc::new(AtomicBool::new(true));
    // Whether the game is the window in front (a screenshot given on the
    // command line always is).
    let in_front = Arc::new(AtomicBool::new(options.input.is_some()));
    let vision = {
        let (slot, running, fps) = (Arc::clone(&slot), Arc::clone(&running), options.fps);
        let shared = Shared {
            sight: sight.clone(),
            latest: Arc::clone(&latest),
            in_front: Arc::clone(&in_front),
        };
        std::thread::Builder::new()
            .name("vision".into())
            .spawn(move || watch(source, slot, running, start, fps, shared))
            .map_err(|e| e.to_string())?
    };

    let mut companion = Companion::new(Settings {
        hp_low: options.hp_low,
        hp_rearm: (options.hp_low + 15.0).min(95.0),
        mp_low: options.mp_low,
        mp_rearm: (options.mp_low + 15.0).min(95.0),
        always_listen: !options.wake_word,
        ..Settings::default()
    });
    let mut out = Outputs {
        mouth: Mouth {
            ai: worker,
            sapi,
            player: Player::new(),
            sapi_until: Instant::now(),
            phone_until: Instant::now(),
            phone_end: Instant::now(),
            game_pid: None,
        },
        phone: phone.as_ref().map(|p| Arc::clone(&p.hub)),
        session,
        log: Vec::new(),
        plain: !ansi,
        start,
        replies: options.replies,
    };
    let hello = companion.hello();
    out.apply(hello, &mut companion, None);

    // The panel over the game (Windows): only for the live game, not a screenshot.
    let mut panel_window = if options.overlay && options.input.is_none() {
        match Overlay::new(options.overlay_on_stream) {
            Ok(window) => Some(window),
            Err(e) => {
                out.push(Kind::Info, format!("no panel over the game: {e}"));
                None
            }
        }
    } else {
        None
    };
    let mut panel_failed = false;
    let mut dog = Dog::load();
    let mut ai_error_shown = String::new();
    let mut model_logged = false;

    let mut preview: Option<Preview> = None;
    let mut preview_failed = false;
    let mut latest_image: Option<Arc<RgbaImage>> = None;
    let mut frame_size = None;
    let mut fps = 0.0;
    let mut drawn_lines = 0usize;
    let mut last_draw = Instant::now() - Duration::from_secs(1);
    let mut last_panel = Instant::now() - Duration::from_secs(1);
    let mut last_plain_status = Instant::now();
    let block_height = qr_lines.len().max(screen::HEIGHT);
    let mut game_area: Option<overlay::GameArea> = None;

    loop {
        if platform::stop_requested() {
            break;
        }
        if preview.as_ref().is_some_and(|p| !p.is_open()) {
            break;
        }
        let now = start.elapsed().as_secs_f64();

        if let Some(tick) = slot.take() {
            if let Some(frame) = &tick.frame {
                latest_image = Some(Arc::clone(&frame.image));
                frame_size = Some(frame.frame_size());
                fps = frame.fps;
                if options.preview && preview.is_none() && !preview_failed {
                    let (w, h) = frame.frame_size();
                    match Preview::open(w, h) {
                        Ok(p) => preview = Some(p),
                        Err(e) => {
                            preview_failed = true;
                            out.push(
                                Kind::Info,
                                format!("could not open the preview window: {e}"),
                            );
                        }
                    }
                }
                if let Some(p) = preview.as_mut() {
                    p.show(frame);
                }
            }
            let actions = companion.observe(tick.at, tick.obs);
            out.apply(actions, &mut companion, latest_image.clone());
            // The things the player taught: their alerts.
            for fired in tick.fired {
                out.show(Kind::Alert, &fired.say);
                out.speak(&fired.say, &mut companion);
            }
        } else if let Some(p) = preview.as_mut() {
            p.pump();
        }
        if let Some(window) = &panel_window {
            window.pump();
        }
        out.mouth.player.tick();

        // What the phone sent.
        if let Some(hub) = out.phone.clone() {
            for inbound in hub.take_inbox() {
                match inbound {
                    Inbound::Heard(heard) => {
                        // MapleSyrup's own voice, heard back by the phone, is
                        // taken out; what is left is the player's.
                        let text = match companion.strip_echo(now, &heard) {
                            None => {
                                out.session.line("echo", &heard);
                                continue;
                            }
                            Some(rest) => {
                                if rest != heard.trim() {
                                    out.session
                                        .line("echo", &format!("{heard}  (kept: {rest})"));
                                }
                                rest
                            }
                        };
                        if out.mouth.ai.is_some() {
                            out.show(Kind::Heard, &text);
                            if let Some(command) = commands::local_command(&text) {
                                let actions = companion.command(now, command);
                                out.apply(actions, &mut companion, latest_image.clone());
                            } else if (companion.settings.always_listen
                                || !matches!(commands::interpret(&text, false), Heard::NotForUs))
                                && let Some(worker) = &out.mouth.ai
                            {
                                {
                                    let mut snapshot = ai::brain::snapshot(
                                        companion.last(),
                                        &companion.progress(),
                                    );
                                    let mut status = None;
                                    if let Some(sight) = &sight {
                                        let sight = sight.lock().unwrap_or_else(|e| e.into_inner());
                                        for line in sight.describe() {
                                            snapshot.push('\n');
                                            snapshot.push_str(&line);
                                        }
                                        status = sight.layout.as_ref().and_then(|l| l.status);
                                    }
                                    // The screen goes with the sentence while the game is in view.
                                    let eyes = latest_image
                                        .clone()
                                        .filter(|_| {
                                            companion.last().is_some_and(|o| o.game.is_seen())
                                                && in_front.load(Ordering::Relaxed)
                                        })
                                        .map(|frame| Eyes { frame, status });
                                    worker.send(Job::Converse {
                                        heard: text,
                                        snapshot,
                                        speak: true,
                                        eyes,
                                    });
                                }
                            }
                        } else {
                            let actions = companion.heard(now, &text);
                            out.apply(actions, &mut companion, latest_image.clone());
                        }
                    }
                    Inbound::Command(word) => {
                        if let Some(command) = Command::from_word(&word) {
                            let actions = companion.command(now, command);
                            out.apply(actions, &mut companion, latest_image.clone());
                        }
                    }
                    Inbound::Hello(agent) => {
                        let device = device_of(&agent);
                        out.push(Kind::Info, format!("{device} connected"));
                        hub.post(
                            Kind::Info,
                            &format!("Connected to MapleSyrup on this {device}."),
                            false,
                        );
                        let greeting = if out.mouth.ai.is_some() {
                            "Hey! I'm here. Just talk to me."
                        } else {
                            "Phone connected."
                        };
                        out.speak(greeting, &mut companion);
                    }
                    Inbound::Voice(on) => {
                        let place = match on {
                            VoiceOn::Pc => "the PC",
                            VoiceOn::Phone => "the phone",
                            VoiceOn::Both => "the PC and the phone",
                            VoiceOn::Off => "nowhere (written only)",
                        };
                        out.push(Kind::Info, format!("replies are now spoken on {place}"));
                        if !on.pc() {
                            out.mouth.hush();
                        }
                    }
                    Inbound::Forget(id) => {
                        if let Some(sight) = &sight {
                            let forgotten = sight
                                .lock()
                                .unwrap_or_else(|e| e.into_inner())
                                .things
                                .forget(&id);
                            if let Some(name) = forgotten {
                                out.show(Kind::Info, &format!("forgot \"{name}\""));
                            }
                        }
                    }
                    Inbound::Listen(always) => {
                        companion.set_always_listen(always);
                        out.show(
                            Kind::Info,
                            if always {
                                "I answer everything you say now."
                            } else {
                                "I answer only when you say \"syrup\" now."
                            },
                        );
                    }
                }
            }
        }

        // What the brain came back with.
        let finished: Vec<Done> = match &out.mouth.ai {
            Some(worker) => worker.done.try_iter().collect(),
            None => Vec::new(),
        };
        for done in finished {
            match done {
                Done::Reply { text, took, .. } => {
                    companion.remember_spoken(now, &text);
                    out.show(Kind::Reply, &text);
                    out.session
                        .line("timing", &format!("reply in {:.1} s", took.as_secs_f64()));
                    if !model_logged
                        && let Some(model) = out
                            .mouth
                            .ai
                            .as_ref()
                            .and_then(|w| w.model.lock().ok().and_then(|m| m.clone()))
                    {
                        model_logged = true;
                        out.session.line("info", &format!("OpenAI model: {model}"));
                    }
                }
                Done::Noted { line } => out.show(Kind::Info, &line),
                Done::Audio {
                    samples,
                    after,
                    first,
                    ..
                } => {
                    if first {
                        out.session.line(
                            "timing",
                            &format!("first words after {:.1} s", after.as_secs_f64()),
                        );
                    }
                    out.play(samples, &companion);
                }
                Done::Silent { heard } => out.session.line("silent", &heard),
                Done::Failed { heard, error } => {
                    let message = format!("OpenAI: {error}");
                    if message != ai_error_shown {
                        out.show(Kind::Info, &message);
                        ai_error_shown = message;
                    }
                    if let AiError::Http(401, _) = error {
                        let _ = std::fs::remove_file(ai::key_file(&settings_dir));
                        out.mouth.ai = None;
                        out.show(Kind::Info, "The OpenAI key was refused; going on with simple answers. Start MapleSyrup again to paste a new key.");
                    }
                    // Answer anyway, simply, in the Windows voice.
                    if let Some(heard) = heard {
                        let fallback: Vec<Action> = companion
                            .heard(now, &heard)
                            .into_iter()
                            .filter(|a| !matches!(a, Action::Say(s) if s.kind == Kind::Heard))
                            .collect();
                        for action in fallback {
                            if let Action::Say(say) = &action {
                                out.show(say.kind, &say.text);
                                if say.speak
                                    && !companion.muted()
                                    && let Some(voice) = &out.mouth.sapi
                                {
                                    voice.say(&say.text);
                                    out.mouth.sapi_until =
                                        Instant::now() + Mouth::estimate(&say.text);
                                }
                            }
                        }
                    }
                }
            }
        }

        // What the teacher found out about the screen.
        while let Ok(news) = news_rx.try_recv() {
            match news {
                News::Line(line) => {
                    out.session.line("sight", &line);
                    out.push(Kind::Info, format!("sight: {line}"));
                }
                News::Found { line, picture } => {
                    out.session.line("sight", &format!("found the HUD: {line}"));
                    let _ = picture.save(session_dir.join("hud-found.png"));
                    out.show(
                        Kind::Info,
                        "I found your HUD: I measure HP, MP and EXP myself now, and check them every couple of minutes.",
                    );
                }
                News::Trouble(why) => {
                    out.session.line("sight", &why);
                    out.push(Kind::Info, format!("sight: {why}"));
                }
            }
        }

        // Where the game is (for the panel, and whose sound to turn down).
        if last_draw.elapsed() >= Duration::from_millis(250) {
            game_area = match companion.last().map(|o| &o.game) {
                Some(GameView::Seen(title)) if options.input.is_none() => overlay::game_area(title),
                _ => None,
            };
            out.mouth.game_pid = game_area.map(|a| a.pid).filter(|pid| *pid != 0);
            in_front.store(
                options.input.is_some() || game_area.is_some_and(|a| a.foreground),
                Ordering::Relaxed,
            );
        }

        // The panel and the dog, about twelve times a second.
        if last_panel.elapsed() >= Duration::from_millis(80) {
            last_panel = Instant::now();
            let speaking = out.mouth.speaking();
            let frame = dog.as_mut().map(|d| d.advance(speaking));
            if let Some(window) = panel_window.as_mut() {
                match game_area {
                    Some(area) if area.foreground && area.width > 200 && area.height > 150 => {
                        let scale = panel::scale_for(area.height);
                        let (_, ph) = panel::size(scale);
                        let dog_frame = match (dog.as_mut(), frame) {
                            (Some(d), Some(i)) => Some(d.frame(i, ph).clone()),
                            _ => None,
                        };
                        let recent = out
                            .log
                            .iter()
                            .rev()
                            .find(|l| matches!(l.kind, Kind::Reply | Kind::Alert))
                            .filter(|l| l.at.elapsed() < Duration::from_secs(15))
                            .map(|l| l.text.as_str());
                        let summary = out.phone.as_ref().map(|h| h.summary());
                        let progress = companion.progress();
                        let content = panel::Content {
                            obs: companion.last(),
                            exp_per_hour: progress.exp_per_hour,
                            phone_connected: summary.as_ref().map(|s| s.connected),
                            speaking,
                            muted: companion.muted(),
                            last_line: recent,
                            dog: dog_frame.as_ref(),
                        };
                        let painted = panel::paint(&content, scale);
                        let margin = (12.0 * scale) as i32;
                        let at = (
                            area.left + area.width - painted.image.width() as i32 - margin,
                            area.top + margin,
                        );
                        if let Err(e) = window.show(&painted, at)
                            && !panel_failed
                        {
                            panel_failed = true;
                            out.push(Kind::Info, format!("the panel could not be drawn: {e}"));
                        }
                    }
                    _ => window.hide(),
                }
            }
        }

        if last_draw.elapsed() >= Duration::from_millis(250) {
            last_draw = Instant::now();
            let progress = companion.progress();
            let summary = out.phone.as_ref().map(|h| h.summary());
            if let Some(hub) = &out.phone {
                let obs = companion.last();
                hub.set_status(json!({
                    "game": obs.map(|o| o.game.clone()).unwrap_or(GameView::NotFound),
                    "hp": obs.and_then(|o| o.hp),
                    "mp": obs.and_then(|o| o.mp),
                    "exp": obs.and_then(|o| o.exp),
                    "level": obs.and_then(|o| o.level),
                    "name": obs.and_then(|o| o.name.clone()),
                    "job": obs.and_then(|o| o.job.clone()),
                    "progress": progress,
                    "muted": companion.muted(),
                    "fps": fps,
                    "wake": "syrup",
                    "always_listen": companion.settings.always_listen,
                    "speaking": out.mouth.speaking(),
                    "thinking": out.mouth.ai.as_ref().is_some_and(|w| w.busy()),
                    "ai": out.mouth.ai.as_ref().map(|w| w.model.lock().ok().and_then(|m| m.clone()).unwrap_or_else(|| "OpenAI".into())),
                    "learned": learned_status(sight.as_ref(), hub),
                }));
            }
            if ansi {
                let view = screen::View {
                    obs: companion.last(),
                    fps,
                    frame_size,
                    progress: &progress,
                    phone: summary.as_ref(),
                    voice: &voice_label,
                    voice_on: out.voice_on(),
                    muted: companion.muted(),
                    log: &out.log,
                };
                let right = screen::render(&view, true);
                // The code stays until a phone has connected.
                let show_qr = summary
                    .as_ref()
                    .is_some_and(|s| !s.connected && s.requests == 0);
                let (columns, _) = platform::console_size().unwrap_or((120, 30));
                let qr_width = qr_lines
                    .iter()
                    .map(|l| l.chars().count())
                    .max()
                    .unwrap_or(0);
                let lines = if !show_qr {
                    screen::side_by_side(&[], &right, block_height)
                } else if columns >= qr_width + 3 + 64 {
                    screen::side_by_side(&qr_lines, &right, block_height)
                } else {
                    // Too narrow for both: the code alone until the phone is in.
                    screen::side_by_side(&qr_lines, &[], block_height)
                };
                let mut frame = String::new();
                if drawn_lines > 0 {
                    frame.push_str(&format!("\x1b[{drawn_lines}A"));
                }
                for line in &lines {
                    frame.push_str("\x1b[2K");
                    frame.push_str(&screen::fit_width(line, columns.saturating_sub(1)));
                    frame.push('\n');
                }
                // Anything left below from a taller block before.
                frame.push_str("\x1b[J");
                use std::io::Write;
                let mut stdout = std::io::stdout();
                let _ = stdout.write_all(frame.as_bytes());
                let _ = stdout.flush();
                drawn_lines = lines.len();
            } else if last_plain_status.elapsed() >= Duration::from_secs(30) {
                last_plain_status = Instant::now();
                let obs = companion.last();
                let pct = |g: Option<ms::companion::Gauge>| {
                    g.map(|g| format!("{:.0}%", g.percent))
                        .unwrap_or("--".into())
                };
                println!(
                    "[status] game {} · HP {} · MP {} · EXP {} · phone {}",
                    if obs.is_some_and(|o| o.game.is_seen()) {
                        "seen"
                    } else {
                        "not seen"
                    },
                    pct(obs.and_then(|o| o.hp)),
                    pct(obs.and_then(|o| o.mp)),
                    pct(obs.and_then(|o| o.exp)),
                    if summary.as_ref().is_some_and(|s| s.connected) {
                        "connected"
                    } else {
                        "not connected"
                    },
                );
            }
        }

        std::thread::sleep(Duration::from_millis(15));
    }

    running.store(false, Ordering::Relaxed);
    let _ = vision.join();
    out.mouth.hush();
    ms::platform::sound::restore();
    if ansi {
        print!("\x1b[?7h");
    }
    let progress = companion.progress();
    println!(
        "\nStopped after {}. {} mark{} saved in {}",
        screen::short_duration(progress.seconds),
        progress.marks,
        if progress.marks == 1 { "" } else { "s" },
        session_dir.display()
    );
    Ok(())
}
