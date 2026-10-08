//! MapleSyrup: the MapleStory companion, as one program you start.
//!
//! It finds the MapleStory window and watches it through the vision engine,
//! and you talk with it through your phone like you would with a friend:
//! with an OpenAI key it answers like ChatGPT, knowing what is on your
//! screen, in a natural voice (the game turned down while it talks). It
//! speaks up on its own when HP or MP runs low or you level up. Its dog
//! (the chow chow from the logo) shows it all, on a panel over the game,
//! and lives in a box of its own on the phone.
//!
//! ```text
//!  MapleStory window ─ syrup capture ─▶ vision engine ─▶ Observation ─┐
//!                                                                      ▼
//!  phone page ── what you said / buttons / audio ──▶ phone link ──▶ Companion ─┐
//!       ▲                                                   │      (warnings)   │
//!       │                                                   ▼                   ▼
//!       └──── replies, voice clips, the dog ◀──── OpenAI (reply + voice) ──▶ PC speakers
//! ```

mod recording;
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
use ms::perceive::{Look, Perceived, perceive};
use ms::phone::{self, Hub, Inbound, VoiceOn, qr, tls, tunnel};
use ms::platform::overlay::{self, Overlay};
use ms::platform::sound::Player;
use ms::platform::{self, voice::Voice};
use ms::sight::Sight;
use ms::sight::things::Fired;
use ms::vision::snapshot::{Detectors, PerceptionPipeline};
use serde_json::json;
use syrup::timing::FPSCounter;

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
  --record              record the session from the start: a video of the whole
                        screen with every sound (the phone's Record button does
                        the same any time)
  --record-mic          keep the phone's microphone in the session's mic.wav
  --replies WHERE       where replies are spoken: phone (a live call, like a
                        phone call: any language, talk over it), pc (the PC's
                        speakers), both, off (default phone with an OpenAI
                        key, else pc)
  --no-live             no live call on the phone: replies on the PC instead
  --attitude HOW        how it talks to you: friendly, blunt or savage (default:
                        what you picked on the phone, else blunt)
  --no-grok             don't use Grok even with an xAI key (xai-key.txt)
  --grok-model NAME     the Grok model (default grok-4.3, without reasoning)
  --wake-word           answer only sentences that say \"syrup\" (for streams)
  --no-ai               no OpenAI, even with a key
  --no-web              don't let it search the web for MapleStory facts
  --forget-key          delete the saved OpenAI key and ask again
  --model NAME          the OpenAI model (default: the fastest the key can use)
  --voice NAME          the OpenAI voice: cedar, marin, ash, coral, sage… (default cedar)
  --no-voice            never speak on the PC
  --rate N              the Windows voice's speed, -10 to 10 (default 1)
  --hp-low N            warn below N% HP (default 30, or what you asked it for
                        or it learned)
  --mp-low N            warn below N% MP (default 15, or what you asked it for)
  --fps N               frames watched per second (default 10)
  --preview             also open a window showing what the engine sees
  --no-overlay          no panel over the game window
  --overlay-on-stream   let OBS and screenshots see the panel (by default it
                        keeps out of captures)
  --plain               no colours or redrawing in the console
  --no-update           never look for a new version (it updates itself
                        otherwise: fetched in the background, installed at
                        the next start, rolled back if it does not come up)
  --workshop            let it rewrite itself on this PC when asked (\"change
                        yourself: ...\"): a coding agent installed here (Claude
                        Code or Codex) changes the source in --repo, it is
                        built and tested, and the new program installs at the
                        next start. Also a switch on the phone.
  --repo PATH           the checkout of MapleSyrup's source the workshop works
                        in (default %USERPROFILE%\\GitHub\\ms, or MAPLESYRUP_REPO)
  --self-test           check this PC: the engine, the phone link, the voice
  --record-test         check recording on this PC: a few seconds of the screen
                        with a flash and a tone, which must line up
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
    record: bool,
    record_mic: bool,
    /// Where replies are spoken (None: a live call when there is a key).
    replies: Option<VoiceOn>,
    wake_word: bool,
    ai: bool,
    /// Live calls on the phone (OpenAI's realtime voice).
    live: bool,
    web: bool,
    forget_key: bool,
    model: Option<String>,
    voice_name: String,
    openai_base: String,
    voice: bool,
    rate: i32,
    /// Warn below these percents (None: what the player asked for or it
    /// learned, else the usual).
    hp_low: Option<f32>,
    mp_low: Option<f32>,
    fps: f64,
    preview: bool,
    overlay: bool,
    overlay_on_stream: bool,
    plain: bool,
    /// Looks for a new version of itself and installs it.
    update: bool,
    /// Rewrites itself on this PC when asked (None: as set on the phone).
    workshop: Option<bool>,
    /// The checkout the workshop works in (None: the usual place).
    repo: Option<PathBuf>,
    self_test: bool,
    record_test: bool,
    /// How it talks (None: what the player picked on the phone).
    attitude: Option<ms::companion::Attitude>,
    /// Grok answers the conversation when there is an xAI key.
    grok: bool,
    grok_model: Option<String>,
}

fn parse(args: &[String]) -> Result<Options, String> {
    let mut o = Options {
        window: None,
        input: None,
        phone: true,
        tunnel: false,
        port: 8443,
        record: false,
        record_mic: false,
        replies: None,
        wake_word: false,
        ai: true,
        live: true,
        web: true,
        forget_key: false,
        model: None,
        voice_name: "cedar".into(),
        openai_base: std::env::var("OPENAI_BASE_URL")
            .unwrap_or_else(|_| "https://api.openai.com/v1".into()),
        voice: true,
        rate: 1,
        hp_low: None,
        mp_low: None,
        fps: 10.0,
        preview: false,
        overlay: true,
        overlay_on_stream: false,
        plain: false,
        update: std::env::var_os("MAPLESYRUP_NO_UPDATE").is_none(),
        workshop: None,
        repo: None,
        self_test: false,
        record_test: false,
        attitude: None,
        grok: true,
        grok_model: None,
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
            "--record" => o.record = true,
            "--record-mic" => o.record_mic = true,
            "--replies" => {
                let text = value("--replies")?;
                o.replies = Some(
                    VoiceOn::parse(&text)
                        .ok_or(format!("--replies is pc, phone, both or off, not {text:?}"))?,
                )
            }
            "--wake-word" => o.wake_word = true,
            "--no-ai" => o.ai = false,
            "--no-web" => o.web = false,
            "--no-live" => o.live = false,
            "--forget-key" => o.forget_key = true,
            "--model" => o.model = Some(value("--model")?),
            "--voice" => o.voice_name = value("--voice")?,
            "--openai-base" => o.openai_base = value("--openai-base")?,
            "--no-voice" => o.voice = false,
            "--rate" => o.rate = number("--rate", value("--rate")?)? as i32,
            "--hp-low" => o.hp_low = Some(number("--hp-low", value("--hp-low")?)? as f32),
            "--mp-low" => o.mp_low = Some(number("--mp-low", value("--mp-low")?)? as f32),
            "--fps" => o.fps = number("--fps", value("--fps")?)?.clamp(1.0, 30.0),
            "--preview" => o.preview = true,
            "--no-overlay" => o.overlay = false,
            "--overlay-on-stream" => o.overlay_on_stream = true,
            "--plain" => o.plain = true,
            "--no-update" => o.update = false,
            "--workshop" => o.workshop = Some(true),
            "--repo" => o.repo = Some(PathBuf::from(value("--repo")?)),
            "--self-test" => o.self_test = true,
            "--record-test" => o.record_test = true,
            "--attitude" => {
                let text = value("--attitude")?;
                o.attitude = Some(ms::companion::Attitude::parse(&text).ok_or(format!(
                    "--attitude is friendly, blunt or savage, not {text:?}"
                ))?)
            }
            "--no-grok" => o.grok = false,
            "--grok-model" => o.grok_model = Some(value("--grok-model")?),
            "-h" | "--help" | "/?" => return Err(String::new()),
            other if other.starts_with('-') => return Err(format!("unknown option {other}")),
            path => o.input = Some(PathBuf::from(path)),
        }
    }
    Ok(o)
}

fn main() {
    // `MS_OCR` chose the text reader before OCR moved into Syrup; it still
    // does, through Syrup's own variable.
    if let Some(engine) = std::env::var_os("MS_OCR")
        && std::env::var_os("SYRUP_OCR").is_none()
    {
        // SAFETY: nothing else is running yet; the variable is read later,
        // on other threads, through the usual lock.
        unsafe { std::env::set_var("SYRUP_OCR", engine) };
    }
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
    if options.record_test {
        std::process::exit(selftest::record_test());
    }
    // A new version fetched last time goes in now (the previous one kept
    // beside it), or the kept one comes back when the new one did not come
    // up: either way the program in place is started, and this one leaves.
    if options.update
        && let Ok(exe) = std::env::current_exe()
        && let ms::update::Start::Relaunch(exe) =
            ms::update::at_start(&tls::settings_dir(), &exe, env!("CARGO_PKG_VERSION"))
    {
        println!("MapleSyrup: starting the version just put in place…");
        match ms::update::relaunch(&exe, &args) {
            Ok(()) => return,
            Err(e) => eprintln!("{e}\nCarrying on with this one."),
        }
    }
    if let Err(e) = run(options, args) {
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
    replies: VoiceOn,
    settings: &Path,
    session_dir: &Path,
) -> Result<PhoneLink, String> {
    let key = tls::link_key(settings);
    let record_to = options.record_mic.then(|| session_dir.join("mic.wav"));
    let hub = Hub::new(key.clone(), record_to, replies);

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
    /// A line worth showing once: where the frames come from.
    note: Option<String>,
    /// What the frame's fingerprint says: how much changed, a new scene.
    scene: Option<ms::coach::scene::Verdict>,
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
        image: Arc<RgbaImage>,
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
                image: Arc::new(image),
            });
        }
        Ok(Source::Game(match &options.window {
            Some(title) => GameCapture::titled(title),
            None => GameCapture::auto(),
        }))
    }

    /// Where the game's frames come from (see [`GameCapture::path`]).
    fn path(&self) -> Option<String> {
        match self {
            Source::Game(capture) => capture.path(),
            _ => None,
        }
    }

    fn next(&mut self) -> Captured {
        match self {
            Source::Game(capture) => capture.capture(),
            Source::Still { label, image } => Captured::Frame {
                title: label.clone(),
                image: Arc::clone(image),
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
                        image: Arc::new(img.to_rgba8()),
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

/// What the capture thread hands the vision thread: one attempt to
/// capture, how long it took, and a line to show if the frames' source
/// changed.
struct Grabbed {
    captured: Captured,
    took: Duration,
    note: Option<String>,
}

/// The newest capture, waiting for the vision thread: a mailbox of one.
/// A frame the vision thread has not taken by the time the next arrives is
/// dropped — the companion wants the latest picture, not a backlog.
#[derive(Default)]
struct Mailbox {
    slot: Mutex<Option<Grabbed>>,
    arrived: std::sync::Condvar,
}

impl Mailbox {
    fn put(&self, grabbed: Grabbed) {
        *self.slot.lock().unwrap_or_else(|e| e.into_inner()) = Some(grabbed);
        self.arrived.notify_one();
    }

    /// The newest capture, waiting up to `timeout` for one.
    fn take(&self, timeout: Duration) -> Option<Grabbed> {
        let guard = self.slot.lock().unwrap_or_else(|e| e.into_inner());
        let (mut guard, _) = self
            .arrived
            .wait_timeout_while(guard, timeout, |slot| slot.is_none())
            .unwrap_or_else(|e| e.into_inner());
        guard.take()
    }
}

/// Capture, on a thread of its own: a frame every `1/fps` seconds (half a
/// second between tries while the game cannot be captured) into the
/// mailbox, so the vision thread never waits on the compositor or on GDI,
/// and a slow frame of vision costs the next capture nothing.
fn grab(mut source: Source, mailbox: Arc<Mailbox>, running: Arc<AtomicBool>, fps: f64) {
    let period = Duration::from_secs_f64(1.0 / fps);
    let mut capture_path: Option<String> = None;
    while running.load(Ordering::Relaxed) {
        let began = Instant::now();
        let captured = source.next();
        let took = began.elapsed();
        let seen = matches!(captured, Captured::Frame { .. });
        // Said once, and again if the path changes (the GPU path giving
        // up, say): where the frames are coming from.
        let note = match &captured {
            Captured::Frame { image, .. } => {
                let path = source.path();
                (path != capture_path).then(|| {
                    capture_path = path;
                    capture_path.as_ref().map(|path| {
                        format!(
                            "capture: {}x{} frames from {path}",
                            image.width(),
                            image.height()
                        )
                    })
                })
            }
            _ => None,
        };
        mailbox.put(Grabbed {
            captured,
            took,
            note: note.flatten(),
        });
        let rest = if seen {
            period.checked_sub(began.elapsed())
        } else {
            Some(Duration::from_millis(500))
        };
        if let Some(rest) = rest {
            std::thread::sleep(rest);
        }
    }
}

/// The vision engine, on a thread of its own: every frame the capture
/// thread puts in the mailbox goes through `perceive` and out as a tick.
///
/// `wanted` says which detectors run on every frame: the HUD alone when
/// nothing shows the rest, everything when the preview window does.
fn watch(
    mailbox: Arc<Mailbox>,
    slot: Arc<TickSlot>,
    running: Arc<AtomicBool>,
    start: Instant,
    wanted: Detectors,
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
    let mut scenes = ms::coach::scene::Scenes::default();
    while running.load(Ordering::Relaxed) {
        let Some(Grabbed {
            captured,
            took: capture,
            note,
        }) = mailbox.take(Duration::from_millis(250))
        else {
            continue;
        };
        match captured {
            Captured::Frame { title, image } => {
                frame_id += 1;
                let vision_start = Instant::now();
                // What MapleSyrup learned about this screen replaces the
                // old HUD reader's guesses. Only while the game is the window
                // in front: the capture is of the screen where the game is,
                // so another window over it would be measured (and sent to
                // OpenAI) instead.
                let in_view = in_front.load(Ordering::Relaxed);
                if in_view {
                    latest.put(Arc::clone(&image));
                } else {
                    latest.clear();
                }
                let Perceived { world, obs, seen } = {
                    let _frame_span = tracing::trace_span!("frame").entered();
                    let mut sight = sight
                        .as_ref()
                        .map(|s| s.lock().unwrap_or_else(|e| e.into_inner()));
                    perceive(
                        &mut pipeline,
                        sight.as_deref_mut(),
                        wanted,
                        &Look {
                            title: &title,
                            frame: &image,
                            frame_id,
                            now: Instant::now(),
                            in_view,
                        },
                    )
                };
                let fired = seen.fired;
                // One scene from the next, for the coach (a few thousand
                // pixels, whatever the frame's size).
                let scene = in_view.then(|| {
                    let _span = tracing::trace_span!("scene").entered();
                    scenes.observe(
                        start.elapsed().as_secs_f64(),
                        ms::coach::scene::Fingerprint::of(&image),
                    )
                });
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
                    note,
                    scene,
                });
            }
            Captured::NotFound => {
                latest.clear();
                slot.put(Tick {
                    at: start.elapsed().as_secs_f64(),
                    obs: Observation::unseen(GameView::NotFound),
                    frame: None,
                    fired: Vec::new(),
                    note: None,
                    scene: None,
                });
            }
            Captured::Unavailable(why) => {
                latest.clear();
                slot.put(Tick {
                    at: start.elapsed().as_secs_f64(),
                    obs: Observation::unseen(GameView::Unavailable(why)),
                    frame: None,
                    fired: Vec::new(),
                    note: None,
                    scene: None,
                });
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
    /// The line under way (the last the voice started making, by job) and
    /// what kind of line it is: a cut lets a warning's play out and hushes
    /// the rest.
    line: Option<(u64, Kind)>,
    /// A line a cut hushed: what was still to come of it is dropped, here
    /// and on the phone (hushed on its first half, a reply went on from
    /// its second).
    hushed: Option<u64>,
    /// The line being made for the phone (it gets a line at a time).
    phone_line: Vec<i16>,
    /// Lines of its own (a warning, a hello) made while the player was
    /// talking or their answer was being made, handed over once there is
    /// room.
    phone_held: Vec<HeldClip>,
    /// When it was last heard speaking (the phone may still hand its last
    /// words back for a moment); None: not yet.
    last_voice: Option<Instant>,
    /// The game was turned down for the phone's live voice.
    phone_ducked: bool,
    /// The game's process, turned down while the PC speaks.
    game_pid: Option<u32>,
}

/// A line of its own as a WAV, waiting for room on the phone: what kind of
/// line it is, how long it plays, and since when it waits.
struct HeldClip {
    since: Instant,
    kind: Kind,
    wav: Vec<u8>,
    length: Duration,
}

/// A piece of a line's voice, as the worker hands it over: which line
/// (the job), what kind of line, its samples, and whether it opens or
/// closes the line.
struct Piece<'a> {
    id: u64,
    kind: Kind,
    samples: &'a [i16],
    start: bool,
    end: bool,
}

impl Mouth {
    fn speaking(&self) -> bool {
        let now = Instant::now();
        self.player.speaking() || now < self.sapi_until || now < self.phone_until
    }

    /// Speaking on the PC's speakers (not on the phone).
    fn pc_speaking(&self) -> bool {
        self.player.speaking() || Instant::now() < self.sapi_until
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

/// Answers the phone's live calls: a short-lived key for OpenAI's realtime
/// voice, MapleSyrup's tools, a web search.
struct LiveService {
    live: ai::live::Live,
    toolbox: Toolbox,
    /// What it learned: told to every call.
    learning: ai::Learning,
}

impl LiveService {
    /// The call's instructions as of now, and the attitude they carry.
    fn instructions_with(
        &self,
        recent: &[String],
        language: Option<&str>,
    ) -> (String, ms::companion::Attitude) {
        let attitude = self.learning.memory().attitude;
        let text = ai::live::instructions(&self.learning.prompt(), recent, language, attitude);
        (text, attitude)
    }
}

impl phone::Service for LiveService {
    fn live(&self, recent: &[String], language: Option<&str>) -> Result<serde_json::Value, String> {
        let (instructions, attitude) = self.instructions_with(recent, language);
        let tools = ai::live::tools(self.toolbox.definitions());
        // How it adapted to the player: how soon to answer, their words.
        let tuning = {
            let memory = self.learning.memory();
            ai::live::Tuning {
                eagerness: memory.adapt.eagerness.clone(),
                words: memory.words_hint(),
                speed: ai::live::SPEED,
            }
        };
        let mut call = self
            .live
            .session(&instructions, &tools, &tuning)
            .map_err(|e| e.to_string())?;
        // The page watches for the attitude to change from this one.
        call["attitude"] = json!(attitude);
        Ok(call)
    }

    fn instructions(&self, language: Option<&str>) -> serde_json::Value {
        // (No "conversation so far": the call has it.)
        let (instructions, attitude) = self.instructions_with(&[], language);
        json!({"instructions": instructions, "attitude": attitude})
    }

    fn tool(
        &self,
        name: &str,
        arguments: &str,
        frame: Option<&RgbaImage>,
    ) -> (String, Option<ai::Effect>) {
        let call = ai::openai::Call {
            call_id: String::new(),
            name: name.to_string(),
            arguments: arguments.to_string(),
        };
        self.toolbox.run(&call, frame)
    }
}

/// Whose turn it is, so talking to MapleSyrup works like talking to a
/// person: talk over it and it stops and listens; keep talking after a
/// pause and it waits for the rest of the sentence instead of answering
/// half of it.
#[derive(Default)]
struct Turns {
    /// The latest reply asked for.
    reply: Option<Asked>,
    /// What the player said before they kept talking (its reply was called
    /// off before a word of it was said): answered together with what they
    /// say next, or alone after a moment.
    held: Option<Held>,
    /// The phone heard a loud sound over its own voice and paused the clip:
    /// a talk-over once the player's words follow, else a noise (its own
    /// echo, the game) and the clip plays on.
    loud: Option<Instant>,
}

struct Asked {
    /// The worker's job.
    id: u64,
    /// What it answers.
    heard: String,
    /// Some of it has been said.
    spoke: bool,
    /// Its text is complete (its voice may still be playing).
    done: bool,
    /// Its lines, and when each starts to be heard.
    lines: Vec<(Instant, String)>,
}

/// Words waiting for the rest of their sentence.
struct Held {
    words: String,
    since: Instant,
    /// The rest as the phone heard it so far: answered with the words when
    /// no whole sentence follows (the phone's recognition stops when a clip
    /// starts, and the sentence it was writing is gone).
    more: Option<String>,
}

/// How long the player's words wait for the rest of their sentence.
const HOLD_FOR: Duration = Duration::from_millis(1600);

/// How long a line of its own waits for the player to stop talking (and
/// their answer to be made) before the phone gets it anyway.
const CLIP_WAITS: Duration = Duration::from_secs(8);

/// How long a loud sound the phone heard over its own voice waits for the
/// player's words before it counts as a talk-over. The phone restarts its
/// recognition the moment it pauses the clip, and a sentence really spoken
/// over it is written within about a second; its own echo and the game
/// never are.
const CORROBORATE_FOR: Duration = Duration::from_millis(1200);

impl Turns {
    /// Stop talking and call off what is being made (the player talked over
    /// it, or kept talking).
    fn interrupt(&mut self, out: &mut Outputs) {
        self.loud = None;
        let talking = out.mouth.speaking();
        out.cut();
        let Some(worker) = &out.mouth.ai else {
            return;
        };
        worker.cancel_all();
        if let Some(reply) = self.reply.as_mut() {
            if !reply.spoke && !reply.done {
                // Nothing of it was said: the player's words still count.
                self.held = Some(Held {
                    words: reply.heard.clone(),
                    since: Instant::now(),
                    more: None,
                });
            } else if reply.done && talking {
                // Written in full but cut off while said: the conversation
                // keeps what was heard.
                let now = Instant::now();
                let heard: Vec<String> = reply
                    .lines
                    .iter()
                    .filter(|(at, _)| *at <= now)
                    .map(|(at, line)| heard_of(line, now.duration_since(*at)))
                    .collect();
                worker.send(Job::Cut {
                    heard: heard.join(" "),
                });
            }
            reply.done = true;
        }
    }

    /// The phone heard a loud sound over its own voice and paused the clip.
    /// Nothing is called off yet: the phone fires on its own echo and on
    /// the game, so only the player's words (`hearing`) make it a
    /// talk-over; otherwise the phone plays on, that much later.
    fn loud(&mut self, out: &mut Outputs) {
        let now = Instant::now();
        self.loud = Some(now);
        // (It is playing, whatever was reckoned.)
        out.mouth.phone_end = out.mouth.phone_end.max(now) + CORROBORATE_FOR;
        out.mouth.phone_until = out.mouth.phone_until.max(now) + CORROBORATE_FOR;
    }

    /// A loud sound the phone heard lately, still waiting for words.
    fn loud_lately(&self) -> bool {
        self.loud.is_some_and(|at| at.elapsed() < CORROBORATE_FOR)
    }

    /// The phone hears the player now. Returns what happened, for the log.
    fn hearing(
        &mut self,
        out: &mut Outputs,
        companion: &Companion,
        now: f64,
        text: &str,
    ) -> Option<String> {
        if out.mouth.speaking() || self.loud_lately() {
            if let Some(words) = companion.barge_in(now, text) {
                self.interrupt(out);
                return Some(format!("talked over: {words}"));
            }
            if out.mouth.speaking() {
                return None;
            }
        }
        let waiting = self.reply.as_ref().is_some_and(|r| !r.spoke && !r.done);
        if waiting {
            // The sentence's own words, posted late (the phone throttles
            // them): not more talking.
            if self.reply.as_ref().is_some_and(|r| within(&r.heard, text)) {
                return None;
            }
            if companion.still_talking(now, text) {
                // Still talking: the answer waits for the rest.
                self.interrupt(out);
                if let Some(held) = self.held.as_mut() {
                    held.more = Some(text.to_string());
                }
                return Some(format!("still talking: {text}"));
            }
        } else if let Some(held) = self.held.as_mut()
            && companion.still_talking(now, text)
        {
            // Still at it: the wait starts over, and this is the rest so far.
            held.since = Instant::now();
            held.more = Some(text.to_string());
            return Some(format!("still talking: {text}"));
        }
        None
    }

    /// The player said `text`: what to answer (with words of theirs still
    /// waiting, if any). Whatever is being said or made is stopped.
    fn heard(&mut self, out: &mut Outputs, text: String) -> String {
        let busy = self.reply.as_ref().is_some_and(|r| !r.done);
        if out.mouth.speaking() || busy {
            self.interrupt(out);
        }
        match self.held.take() {
            Some(held) => fold(&held.words, text),
            None => text,
        }
    }

    /// A reply is being made (or words wait for the rest of a sentence).
    fn busy(&self) -> bool {
        self.held.is_some() || self.reply.as_ref().is_some_and(|r| !r.done)
    }

    /// The job of the reply the player is waiting for (its voice is not
    /// held back for anything).
    fn reply_id(&self) -> Option<u64> {
        self.reply.as_ref().map(|r| r.id)
    }

    fn asked(&mut self, id: u64, heard: &str) {
        self.reply = Some(Asked {
            id,
            heard: heard.to_string(),
            spoke: false,
            done: false,
            lines: Vec::new(),
        });
    }

    /// A line of reply `id` starts to be made, to be heard from `at`.
    fn spoke(&mut self, id: u64, at: Instant, line: &str) {
        if let Some(reply) = self.reply.as_mut().filter(|r| r.id == id) {
            reply.spoke = true;
            reply.lines.push((at, line.to_string()));
        }
    }

    fn finished(&mut self, id: u64) {
        if let Some(reply) = self.reply.as_mut().filter(|r| r.id == id) {
            reply.done = true;
        }
    }

    /// Words that waited long enough for more: to be answered now, with
    /// the rest as the phone heard it.
    fn overdue(&mut self) -> Option<String> {
        if self
            .held
            .as_ref()
            .is_some_and(|held| held.since.elapsed() >= HOLD_FOR)
        {
            let held = self.held.take()?;
            return Some(match held.more {
                Some(more) => fold(&held.words, more),
                None => held.words,
            });
        }
        None
    }
}

/// The start of a sentence and its rest, as one (the phone sometimes sends
/// the whole sentence again instead of the rest).
fn fold(before: &str, text: String) -> String {
    if commands::normalize(&text).starts_with(&commands::normalize(before)) {
        text
    } else {
        format!("{before} {text}")
    }
}

/// The same actions with their lines only shown, not spoken.
fn unspoken(actions: Vec<Action>) -> Vec<Action> {
    actions
        .into_iter()
        .map(|action| match action {
            Action::Say(say) => Action::Say(ms::companion::Say {
                speak: false,
                ..say
            }),
            other => other,
        })
        .collect()
}

/// Whether the player is neither talking (heard this last second) nor
/// waiting for an answer: a line of its own may start on the phone.
fn player_quiet(player_heard: Option<Instant>, turns: &Turns) -> bool {
    player_heard.is_none_or(|at| at.elapsed() >= Duration::from_secs(1)) && !turns.busy()
}

/// Whether `part` is in `sentence`, as whole words ("am I talking to" is
/// in "am I talking to right").
fn within(sentence: &str, part: &str) -> bool {
    let part = commands::normalize(part);
    !part.is_empty()
        && format!(" {} ", commands::normalize(sentence)).contains(&format!(" {part} "))
}

/// How much of `line` was heard after it had played for `played`: speech
/// runs at about fourteen letters a second; cut at a word.
fn heard_of(line: &str, played: Duration) -> String {
    let letters = (played.as_secs_f64() * 14.0) as usize;
    if letters >= line.chars().count() {
        return line.to_string();
    }
    let mut heard = String::new();
    for word in line.split_whitespace() {
        if heard.chars().count() + word.chars().count() > letters {
            break;
        }
        if !heard.is_empty() {
            heard.push(' ');
        }
        heard.push_str(word);
    }
    heard
}

/// Coaching on or off, for good (the player asked, by voice or through the
/// model), said back in a word.
fn set_coaching(
    coach: &mut ms::coach::Coach,
    learning: &ai::Learning,
    out: &mut Outputs,
    on: bool,
) {
    coach.set_on(on);
    {
        let mut memory = learning.memory();
        memory.coach = Some(on);
        memory.save();
    }
    out.session
        .line("coach", if on { "coaching on" } else { "coaching off" });
    out.show(
        Kind::Info,
        if on {
            "Coaching on: I'll speak up on my own when I see something."
        } else {
            "Coaching off: I'll only talk when you ask (and for low HP or MP)."
        },
    );
}

/// The player asked not to be spoken to that way: the attitude goes to
/// friendly, for good (until they pick another on the phone), and they
/// are told so in one line. The learner notes the correction too, but the
/// warnings are MapleSyrup's own lines, which only the setting changes.
fn drop_the_attitude(companion: &mut Companion, learning: &ai::Learning, out: &mut Outputs) {
    use ms::companion::Attitude;
    let was = companion.settings.attitude;
    if was != Attitude::Friendly {
        companion.settings.attitude = Attitude::Friendly;
        let mut memory = learning.memory();
        memory.attitude = Attitude::Friendly;
        memory.save();
        out.session.line(
            "info",
            &format!("attitude: friendly (was {}; the player asked)", was.word()),
        );
    }
    let line = if was == Attitude::Friendly {
        "Okay. I'll keep it respectful."
    } else {
        "Okay, I'll keep it respectful from now on. You can pick another tone on the phone."
    };
    out.tell(Kind::Reply, line, true, companion);
}

/// What is on screen, as a few lines for a model: what the vision engine
/// reads, what a friend in the room would know of the session so far, and
/// what MapleSyrup learned about this screen.
fn snapshot_text(companion: &Companion, sight: Option<&Arc<Mutex<Sight>>>) -> String {
    let mut snapshot =
        ai::brain::snapshot(companion.last(), &companion.progress(), &companion.so_far());
    if let Some(sight) = sight {
        let sight = sight.lock().unwrap_or_else(|e| e.into_inner());
        for line in sight.describe() {
            snapshot.push('\n');
            snapshot.push_str(&line);
        }
    }
    snapshot
}

/// What the player said, for the model: with what is on screen, and the
/// screen itself while the game is the window in front.
fn conversation_job(
    text: String,
    companion: &Companion,
    sight: Option<&Arc<Mutex<Sight>>>,
    frame: Option<Arc<RgbaImage>>,
    in_view: bool,
    language: Option<String>,
) -> Job {
    let snapshot = snapshot_text(companion, sight);
    let status = sight.and_then(|s| {
        s.lock()
            .unwrap_or_else(|e| e.into_inner())
            .layout
            .as_ref()
            .and_then(|l| l.status)
    });
    let eyes = frame
        .filter(|_| in_view && companion.last().is_some_and(|o| o.game.is_seen()))
        .map(|frame| Eyes { frame, status });
    Job::Converse {
        heard: text,
        snapshot,
        speak: true,
        eyes,
        language,
    }
}

/// The end of the session so far, for the workshop's coder: what was
/// heard, said and noticed lately.
fn recent_log(out: &Outputs) -> String {
    out.log
        .iter()
        .rev()
        .take(30)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .map(|l| format!("[{}] {}", kind_label(l.kind), l.text))
        .collect::<Vec<_>>()
        .join("\n")
}

/// One of MapleSyrup's own lines, said in its voice (and shown): through
/// the AI worker when there is one (the voice the player picked), else as
/// the PC says things.
fn say_line(out: &mut Outputs, line: String) {
    match &out.mouth.ai {
        Some(worker) => {
            worker.send(Job::Say {
                heard: None,
                text: line,
            });
        }
        None => out.push(Kind::Reply, line),
    }
}

/// Hand the workshop a task and tell the player what happens now.
fn workshop_ask(workshop: &ms::workshop::Workshop, task: ms::workshop::Task, out: &mut Outputs) {
    match workshop.ask(task) {
        Ok(line) => {
            out.session.line("workshop", &line);
            say_line(out, line);
        }
        Err(why) => {
            out.session.line("workshop", &why);
            say_line(out, format!("I can't change myself right now: {why}"));
        }
    }
}

/// Whether a line of this kind outlives the player talking over it: a
/// warning and news are still true after their words (the worker keeps
/// their jobs too), chat and a note are not.
fn kept(kind: Kind) -> bool {
    matches!(kind, Kind::Warning | Kind::Alert)
}

/// How a line is labelled in the session log.
fn kind_label(kind: Kind) -> &'static str {
    match kind {
        Kind::Heard => "heard",
        Kind::Warning => "warning",
        Kind::Alert => "alert",
        Kind::Reply => "reply",
        Kind::Info => "info",
    }
}

/// MapleSyrup's own lines on a live call, as the call gets them to say.
/// The watcher's lines (a warning, news, the coach's, a look-up's) go at
/// most once per [`RELAY_GAP`]: what comes meanwhile waits, the newest of
/// each kind, and goes together when the gap is up. A death or a level-up
/// goes at once. A warning handed over carries the reading behind it, so
/// the call passes on a number rather than restating the watcher's mood
/// in its own words every few seconds, which is what the player shouted
/// at; the rest ("Rebuff.", "You died.") go without one, so the call does
/// not say "HP's at 95, rebuff".
#[derive(Default)]
struct Relay {
    /// When lines last went to the call.
    sent: Option<Instant>,
    /// The lines waiting for the gap: the newest of each kind (a later
    /// alert is the state of things now; a correction keeps its place).
    waiting: Vec<(Kind, String)>,
}

/// How long the call keeps the watcher to itself after passing a line on.
/// In a fight the warnings come every 12 to 20 s; once in that time the
/// call says the number, and the rest of the time is the player's.
const RELAY_GAP: Duration = Duration::from_secs(20);

impl Relay {
    fn open(&self) -> bool {
        self.sent.is_none_or(|at| at.elapsed() >= RELAY_GAP)
    }

    /// A line for the call: the lines to hand over now (this one with
    /// those that waited, each with whether it is said however late it
    /// comes: only an urgent one is), or none while it waits for the gap.
    /// Parked or going, it takes the place of the line of its kind that
    /// waited: the newest is the state of things.
    fn offer(&mut self, kind: Kind, text: &str, urgent: bool) -> Vec<(Kind, String, bool)> {
        self.waiting.retain(|(k, _)| *k != kind);
        if !urgent && !self.open() {
            self.waiting.push((kind, text.to_string()));
            return Vec::new();
        }
        self.sent = Some(Instant::now());
        let mut lines = self.take_waiting();
        lines.push((kind, text.to_string(), urgent));
        lines
    }

    /// What waited, once the gap is up.
    fn due(&mut self) -> Vec<(Kind, String, bool)> {
        if self.waiting.is_empty() || !self.open() {
            return Vec::new();
        }
        self.sent = Some(Instant::now());
        self.take_waiting()
    }

    /// The lines that waited (none of them urgent: an urgent line never waits).
    fn take_waiting(&mut self) -> Vec<(Kind, String, bool)> {
        std::mem::take(&mut self.waiting)
            .into_iter()
            .map(|(kind, text)| (kind, text, false))
            .collect()
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
    /// The player's language (from the phone), when it is not English:
    /// MapleSyrup's own spoken lines are translated into it.
    language: Option<String>,
    /// Live calls can be made (an OpenAI key).
    live_ok: bool,
    /// A live call is on: MapleSyrup's own lines are handed to the call to
    /// say (in its voice and the language being spoken), not to the PC.
    live: bool,
    /// How they are handed over.
    relay: Relay,
    /// When the phone was last said hello to (None: not yet).
    greeted: Option<Instant>,
    /// The hello was left to the call the page is about to open: the call
    /// says hi when it opens (the page reads it off the status).
    call_greets: bool,
}

/// How long the phone must be gone before it is said hello to again: a
/// page reloaded sooner (iOS Safari does that on its own) is the same
/// visit, and a hello on every reload is a bot's tic.
const HELLO_AGAIN: Duration = Duration::from_secs(30 * 60);

/// Who said hello to the phone that just connected.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Hello {
    /// The call the page is about to open, when it opens.
    Call,
    /// A line of MapleSyrup's own, as a clip.
    Clip,
    /// Nobody: the phone is back from a reload, greeted already.
    Quiet,
}

impl Outputs {
    fn voice_on(&self) -> VoiceOn {
        self.phone
            .as_ref()
            .map(|h| h.voice_on())
            .unwrap_or(self.replies)
    }

    /// The phone's page opened (`agent`: its browser; `live`: its live-call
    /// toggle is on; `away`: how long the phone had been gone, None the
    /// first time): "Connected" on its screen, and a hello in its ear once
    /// per visit, like a person's — the call's own when the page is about
    /// to open one (it reads that off the status), else a clip of
    /// MapleSyrup's own (`knows_player`: picking up from last time). A
    /// reload within [`HELLO_AGAIN`] is the same visit: no second hello.
    fn hello(
        &mut self,
        companion: &mut Companion,
        knows_player: bool,
        agent: &str,
        live: bool,
        away: Option<Duration>,
        language: Option<String>,
    ) -> Hello {
        let device = device_of(agent);
        self.push(Kind::Info, format!("{device} connected"));
        // (In another language the page says it itself.)
        if self.language.is_none()
            && let Some(hub) = &self.phone
        {
            hub.post(
                Kind::Info,
                &format!("Connected to MapleSyrup on this {device}."),
                false,
            );
        }
        let back = self.greeted.is_some() && away.is_some_and(|gone| gone < HELLO_AGAIN);
        let hello = if back {
            Hello::Quiet
        } else if self.live_ok
            && self.voice_on().phone()
            && live
            && companion.settings.always_listen
        {
            Hello::Call
        } else {
            Hello::Clip
        };
        self.session.line(
            "info",
            &format!(
                "{device} connected: {}",
                match hello {
                    Hello::Call => "the call says hello when it opens",
                    Hello::Clip => "saying hello",
                    Hello::Quiet => "back from a reload, no second hello",
                }
            ),
        );
        match hello {
            Hello::Quiet => {}
            Hello::Call => {
                self.greeted = Some(Instant::now());
                self.call_greets = true;
            }
            Hello::Clip => {
                self.greeted = Some(Instant::now());
                match &self.mouth.ai {
                    // Knowing the player: a hello of its own, picking up
                    // from last time.
                    Some(worker) if knows_player && !companion.muted() => {
                        worker.send(Job::Greet { language });
                    }
                    Some(_) => self.speak(Kind::Info, "Hey! I'm here. Just talk to me.", companion),
                    None => self.speak(Kind::Info, "Phone connected.", companion),
                }
            }
        }
        hello
    }

    /// Show a line everywhere (console, phone, session log).
    fn show(&mut self, kind: Kind, text: &str) {
        self.session.line(kind_label(kind), text);
        if let Some(hub) = &self.phone {
            // The phone speaks a line itself only without a natural voice.
            let phone_speaks = self.mouth.ai.is_none()
                && matches!(kind, Kind::Warning | Kind::Alert | Kind::Reply);
            hub.post(kind, text, phone_speaks);
        }
        self.push(kind, text.to_string());
    }

    /// One of MapleSyrup's own lines (not the model's): shown and logged
    /// at once, and spoken when `speak`, in the player's language (the
    /// voice is translated; the line on the screen is not, so that a
    /// warning neither waits for its translation nor goes with it when
    /// that is called off).
    fn tell(&mut self, kind: Kind, text: &str, speak: bool, companion: &mut Companion) {
        if self.live
            && let Some(hub) = self.phone.clone()
        {
            // The call says it, in its voice and the language being spoken.
            self.session.line(kind_label(kind), text);
            self.push(kind, text.to_string());
            if !speak || companion.muted() {
                hub.post(kind, text, false);
            } else if kind == Kind::Reply {
                // The answer to something they did (a button): theirs at once.
                hub.post(kind, text, true);
            } else {
                self.relay(kind, text, companion);
            }
            return;
        }
        self.show(kind, text);
        if speak {
            self.speak(kind, text, companion);
        }
    }

    /// A watcher line for the call to say: handed over now, or when the
    /// call has said one lately, at the next gap (`relay_due`), the newest
    /// of its kind. A death or a level-up goes at once. (While the
    /// character is dead the only alert is the death's; a level-up is the
    /// one announced from the frame being acted on.)
    fn relay(&mut self, kind: Kind, text: &str, companion: &Companion) {
        let dead = companion.dead();
        let urgent = dead || companion.last_level_up() >= companion.progress().seconds;
        // What this line takes the place of: the line of its kind that
        // waited (the newest is the state of things), and, at a death,
        // every warning that waited (about a bar that no longer matters)
        // and every piece of news (about a fight that is over).
        let superseded = |k: Kind| k == kind || (dead && matches!(k, Kind::Warning | Kind::Alert));
        for (_, old) in self.relay.waiting.iter().filter(|(k, _)| superseded(*k)) {
            self.session
                .line("live", &format!("not said, a newer line came: {old}"));
        }
        self.relay.waiting.retain(|(k, _)| !superseded(*k));
        let lines = self.relay.offer(kind, text, urgent);
        if lines.is_empty() {
            self.session
                .line("live", "waits: the call said a line of mine lately");
        }
        self.hand(lines, companion);
    }

    /// The lines that waited for the gap, once it is up.
    fn relay_due(&mut self, companion: &Companion) {
        let lines = self.relay.due();
        self.hand(lines, companion);
    }

    /// Hand lines to the call (shown only, when muted meanwhile), a
    /// warning with the reading behind it — it is about the number, and
    /// the call says the number — and the rest without (a death, a
    /// correction, the coach's "Rebuff." are not about HP); each with
    /// whether it is said however late it comes (the page drops a line
    /// that waited too long behind the call's own voice, and says so; an
    /// urgent one it says when the voice stops).
    fn hand(&mut self, lines: Vec<(Kind, String, bool)>, companion: &Companion) {
        let Some(hub) = &self.phone else {
            return;
        };
        let reading = self.fact(companion);
        for (kind, text, urgent) in lines {
            let fact = reading.as_deref().filter(|_| kind == Kind::Warning);
            self.session.line(
                "live",
                &format!(
                    "to the call{}: {text}{}",
                    if urgent { ", urgent" } else { "" },
                    match (kind, &fact) {
                        (_, Some(fact)) => format!(" [{fact}]"),
                        (Kind::Warning, None) => " [no reading]".to_string(),
                        _ => String::new(),
                    }
                ),
            );
            hub.post_with_fact(kind, &text, !companion.muted(), fact, urgent);
        }
    }

    /// The reading behind a watcher line, for the call: HP and MP as last
    /// seen, each read (the number in the game's font) or estimated (the
    /// bar's fill), and how old the frame is. The call's own picture is
    /// from the last time the player spoke; this is newer, and a number.
    fn fact(&self, companion: &Companion) -> Option<String> {
        let obs = companion.last().filter(|o| o.game.is_seen())?;
        let age = (self.start.elapsed().as_secs_f64() - companion.progress().seconds).max(0.0);
        let gauge = |name: &str, g: ms::companion::Gauge| {
            let pct = if g.percent >= 10.0 {
                format!("{:.0}%", g.percent)
            } else {
                format!("{:.1}%", g.percent)
            };
            if g.read {
                format!("{name} {pct} (read {age:.0} s ago)")
            } else {
                format!("{name} about {pct} (estimated {age:.0} s ago)")
            }
        };
        let parts: Vec<String> = [("HP", obs.hp), ("MP", obs.mp)]
            .into_iter()
            .filter_map(|(name, g)| g.map(|g| gauge(name, g)))
            .collect();
        (!parts.is_empty()).then(|| parts.join(", "))
    }

    /// Say `text`, a line of its own of this `kind`, out loud where replies
    /// are spoken.
    fn speak(&mut self, kind: Kind, text: &str, companion: &mut Companion) {
        if companion.muted() {
            return;
        }
        if self.live && self.phone.is_some() {
            self.relay(Kind::Info, text, companion);
            return;
        }
        let now = self.start.elapsed().as_secs_f64();
        companion.remember_spoken(now, text);
        match &self.mouth.ai {
            Some(worker) => {
                worker.send(Job::Speak {
                    text: text.to_string(),
                    language: self.language.clone(),
                    kind,
                    show: false,
                    speak: true,
                });
            }
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

    /// A piece of natural-voice speech, played as it comes where replies
    /// are spoken: on the PC at once (a new line after a short pause), on
    /// the phone a line at a time, once the line is complete — or, with
    /// `hold`, once the player has stopped talking (`release_clips`): a
    /// clip starting on the phone stops its recognition. A piece of a line
    /// a cut hushed is dropped.
    fn play_piece(&mut self, piece: Piece, hold: bool, companion: &Companion) {
        let Piece {
            id,
            kind,
            samples,
            start,
            end,
        } = piece;
        if companion.muted() || self.mouth.hushed == Some(id) {
            return;
        }
        if start {
            self.mouth.line = Some((id, kind));
        }
        let voice_on = self.voice_on();
        let rate = ai::openai::SPEECH_RATE;
        if voice_on.pc() {
            let pid = self.mouth.game_pid;
            if start {
                self.mouth.player.gap(pid);
            }
            self.mouth.player.push(samples, rate, pid);
            if end {
                self.mouth.player.flush(pid);
            }
        }
        if voice_on.phone() && self.phone.is_some() {
            if start {
                self.mouth.phone_line.clear();
            }
            self.mouth.phone_line.extend_from_slice(samples);
            if end && !self.mouth.phone_line.is_empty() {
                let line = std::mem::take(&mut self.mouth.phone_line);
                let length = Duration::from_secs_f64(line.len() as f64 / rate as f64);
                let wav = ai::wav_bytes(&line, rate);
                if hold {
                    self.mouth.phone_held.push(HeldClip {
                        since: Instant::now(),
                        kind,
                        wav,
                        length,
                    });
                } else {
                    self.hand_clip(wav, length);
                }
            }
        }
    }

    /// Hand the phone a clip, to play after the ones before it.
    fn hand_clip(&mut self, wav: Vec<u8>, length: Duration) {
        let Some(hub) = &self.phone else {
            return;
        };
        hub.set_clip(wav);
        // The phone fetches it at once and plays it after the last.
        let begins = self
            .mouth
            .phone_end
            .max(Instant::now() + Duration::from_millis(300));
        self.mouth.phone_end = begins + length;
        self.mouth.phone_until = self.mouth.phone_end + Duration::from_millis(600);
    }

    /// The clips held back while the player was talking: handed over now
    /// that it is `quiet`, or once one has waited long enough (a warning
    /// cannot wait out a long story).
    fn release_clips(&mut self, quiet: bool) {
        while let Some(clip) = self.mouth.phone_held.first() {
            if !quiet && clip.since.elapsed() < CLIP_WAITS {
                return;
            }
            let HeldClip { wav, length, .. } = self.mouth.phone_held.remove(0);
            self.hand_clip(wav, length);
        }
    }

    /// MapleSyrup's voice on a live call started or stopped: the game is
    /// turned down while it talks, and the dog talks on the PC too.
    fn phone_talking(&mut self, on: bool) {
        if on {
            // Until the phone says it stopped (with a limit, in case it never does).
            self.mouth.phone_until = Instant::now() + Duration::from_secs(20);
            if !self.mouth.phone_ducked
                && let Some(pid) = self.mouth.game_pid
            {
                self.mouth.phone_ducked = ms::platform::sound::duck(pid);
            }
        } else {
            self.mouth.phone_until = Instant::now() + Duration::from_millis(250);
            if self.mouth.phone_ducked {
                ms::platform::sound::restore();
                self.mouth.phone_ducked = false;
            }
        }
    }

    /// Stop talking at once (the player talked over it), here and on the
    /// phone, and drop what was still to come — except a warning's line or
    /// news: a warning the player talked over (or that waited for their
    /// turn) is still true. On the PC's speakers it plays out; on the
    /// phone it is finished whole and goes after their turn, as it would
    /// have. Any other line is hushed, and what was still to come of it
    /// dropped: hushed on its first half, a reply went on from its
    /// second, and a warning was cut like chat. (The PC has one queue: the
    /// line under way is the last one started; a reply's tail still
    /// sounding when a warning starts behind it is let play out with it.)
    fn cut(&mut self) {
        let line_kept = self.mouth.line.is_some_and(|(_, kind)| kept(kind));
        if !line_kept {
            self.mouth.hush();
            self.mouth.phone_line.clear();
            self.mouth.hushed = self.mouth.line.map(|(id, _)| id);
        }
        self.mouth.phone_held.retain(|clip| kept(clip.kind));
        self.mouth.phone_until = Instant::now();
        self.mouth.phone_end = Instant::now();
        if let Some(hub) = &self.phone {
            hub.cut();
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
                Action::Say(say) if say.kind == Kind::Heard => self.show(say.kind, &say.text),
                Action::Say(say) => self.tell(say.kind, &say.text, say.speak, companion),
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
                // (Here and on the phone, the clips queued there too.)
                Action::SetMuted(true) => self.cut(),
                Action::SetMuted(false) => {}
            }
        }
    }

    fn push(&mut self, kind: Kind, text: String) {
        let time = chrono::Local::now().format("%H:%M:%S").to_string();
        if self.plain {
            let who = match kind {
                Kind::Heard => "you",
                Kind::Warning | Kind::Alert => "syrup!",
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

/// The player asked to be warned at another HP or MP (`below`: the percent,
/// 0 for never, None for the usual): from now on, and kept for next time.
/// Returns the line to show.
fn set_warning(
    companion: &mut Companion,
    learning: &ai::Learning,
    options: &Options,
    warn_at: &mut (f32, f32),
    what: &str,
    below: Option<f32>,
) -> String {
    let below = below.filter(|b| b.is_finite()).map(|b| b.clamp(0.0, 95.0));
    let hp = what == "hp";
    let usual = if hp {
        options.hp_low.unwrap_or(30.0)
    } else {
        options.mp_low.unwrap_or(15.0)
    };
    let at = below.unwrap_or(usual);
    let rearm = (at + 15.0).min(95.0);
    {
        let mut memory = learning.memory();
        if hp {
            companion.settings.hp_low = at;
            companion.settings.hp_rearm = rearm;
            warn_at.0 = at;
            memory.adapt.hp_low = below;
        } else {
            companion.settings.mp_low = at;
            companion.settings.mp_rearm = rearm;
            warn_at.1 = at;
            memory.adapt.mp_low = below;
        }
        memory.save();
    }
    let name = if hp { "HP" } else { "MP" };
    if at <= 0.0 {
        format!("No more {name} warnings.")
    } else {
        format!("{name} warnings below {at:.0}% from now on.")
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

fn run(options: Options, args: Vec<String>) -> Result<(), String> {
    let console = platform::init("MapleSyrup (close this window or press Ctrl+C to stop)");
    let ansi = console.ansi && !options.plain;
    let start = Instant::now();
    let settings_dir = tls::settings_dir();
    let session_dir = session::new_dir(&session::sessions_base(&settings_dir));
    let session = Session::open(session_dir.clone());
    // What it learned from playing together before: about the player, their
    // corrections, what it looked up, how they like to talk (all on this PC).
    let learning = ai::Learning::load(&settings_dir);
    let (learned_tx, learned_rx) = mpsc::channel::<String>();
    // How it talks: as asked on the command line, else as picked before.
    if let Some(attitude) = options.attitude {
        learning.memory().attitude = attitude;
    }
    // New versions: looked for in the background and staged for the next
    // start, unless asked not to (here, or on the phone, for good).
    let (update_tx, update_rx) = mpsc::channel::<ms::update::Event>();
    let updater = Arc::new(ms::update::Updater::new(
        &settings_dir,
        env!("CARGO_PKG_VERSION"),
        options.update && learning.memory().updates.unwrap_or(true),
    ));
    if options.update {
        Arc::clone(&updater).spawn(update_tx);
    }
    // The workshop: MapleSyrup rewriting itself on this PC when asked (a
    // coding agent installed here, the local checkout, a build, the
    // updater's staging). While it is on, the channel's releases are not
    // taken: they would wipe the local work.
    let workshop_on = options
        .workshop
        .or(learning.memory().workshop)
        .unwrap_or(false);
    let workshop = Arc::new(ms::workshop::Workshop::new(
        &settings_dir,
        options
            .repo
            .clone()
            .unwrap_or_else(ms::workshop::default_repo),
        ms::workshop::Running {
            version: env!("CARGO_PKG_VERSION").to_string(),
            commit: env!("MS_COMMIT").to_string(),
        },
        workshop_on,
    ));
    if let Some(coder) = learning
        .memory()
        .workshop_coder
        .as_deref()
        .and_then(ms::workshop::Coder::parse)
    {
        let _ = workshop.prefer(coder);
    }
    let (workshop_tx, workshop_rx) = mpsc::channel::<ms::workshop::Event>();
    Arc::clone(&workshop).spawn(workshop_tx);
    if workshop_on {
        updater.set_auto(false);
    }
    // The new version this is, until it has run long enough to be kept.
    let mut update_committed = !options.update;
    // "Update now" from the phone: the staged version put in place, to be
    // started once this one has wound down.
    let mut relaunch_as: Option<String> = None;
    // ElevenLabs voices, with a key: the account's voices are listed on the
    // phone (fetched on the side), and the one picked speaks.
    let elevenlabs_key = ai::load_elevenlabs_key(&settings_dir);
    let eleven_base =
        std::env::var("ELEVENLABS_BASE_URL").unwrap_or_else(|_| ai::eleven::BASE.to_string());
    let voices: Arc<Mutex<Vec<ai::eleven::Voice>>> = Arc::new(Mutex::new(Vec::new()));
    // What it found (for the log and the phone).
    let (voices_tx, voices_rx) = mpsc::channel::<String>();
    if let Some(key) = &elevenlabs_key {
        let (key, base, voices, learning) = (
            key.clone(),
            eleven_base.clone(),
            Arc::clone(&voices),
            learning.clone(),
        );
        let _ = std::thread::Builder::new()
            .name("voices".into())
            .spawn(move || {
                let eleven = ai::eleven::Eleven::new(&key, &base);
                // The network may not be up yet: a few tries.
                let mut tries = 0;
                let list = loop {
                    tries += 1;
                    match eleven.voices() {
                        Ok(list) => break list,
                        Err(ai::AiError::Network(_)) if tries < 4 => {
                            std::thread::sleep(Duration::from_secs(5))
                        }
                        Err(error) => {
                            let _ = voices_tx.send(format!(
                                "ElevenLabs: couldn't list the voices ({})",
                                error.detail()
                            ));
                            return;
                        }
                    }
                };
                // None picked yet: a lively one to start with.
                {
                    let mut memory = learning.memory();
                    if memory.voice.is_none()
                        && let Some(voice) = ai::eleven::default_voice(&list)
                    {
                        memory.voice = Some(voice.id.clone());
                        memory.save();
                    }
                }
                let _ = voices_tx.send(format!(
                    "ElevenLabs: {} voices to pick from on the phone",
                    list.len()
                ));
                *voices.lock().unwrap_or_else(|e| e.into_inner()) = list;
            });
    }

    // The brain: OpenAI when there is a key that works. With it come the
    // eyes: a vision model that teaches MapleSyrup the player's screen; and,
    // with an xAI key, Grok answering the conversation (faster, and freer).
    // Look-ups run in the background.
    let mut ai_note = String::from("no OpenAI key: simple answers, Windows voice");
    let latest = Arc::new(Latest::default());
    let (news_tx, news_rx) = mpsc::channel::<News>();
    // What it learned about this screen: the HUD, found from the pixels
    // and read in the game's own font, with or without a model to ask.
    let learned = Arc::new(Mutex::new(Sight::load(&settings_dir.join("learned"))));
    let sight: Option<Arc<Mutex<Sight>>> = Some(Arc::clone(&learned));
    let mut teacher_started = false;
    let mut live_service: Option<Arc<LiveService>> = None;
    let mut lookups: Option<(ai::lookup::Lookups, mpsc::Receiver<ai::lookup::Found>)> = None;
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
                    brain.learning = Some(learning.clone());
                    // It looks back on the conversation now and then, and
                    // learns (the sessions before this one first).
                    if let Some(sessions) = session_dir.parent() {
                        ai::memory::spawn(
                            Arc::new(OpenAi::new(
                                &key,
                                &options.openai_base,
                                &options.voice_name,
                                options.model.as_deref(),
                            )),
                            learning.clone(),
                            sessions.to_path_buf(),
                            learned_tx.clone(),
                        );
                    }
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
                        Some(Arc::clone(&eyes)),
                        Arc::clone(&learned),
                        Arc::clone(&latest),
                        news_tx.clone(),
                    );
                    teacher_started = true;
                    if options.live {
                        let chat = Arc::new(OpenAi::new(
                            &key,
                            &options.openai_base,
                            &options.voice_name,
                            options.model.as_deref(),
                        ));
                        live_service = Some(Arc::new(LiveService {
                            live: ai::live::Live::new(Arc::clone(&chat), &options.voice_name),
                            toolbox: Toolbox {
                                sight: Arc::clone(&learned),
                                eyes: Arc::clone(&eyes),
                                settings: settings_dir.clone(),
                                web: options.web,
                                learning: Some(learning.clone()),
                                workshop: Some(Arc::clone(&workshop)),
                            },
                            learning: learning.clone(),
                        }));
                    }
                    let toolbox = Toolbox {
                        sight: Arc::clone(&learned),
                        eyes,
                        settings: settings_dir.clone(),
                        web: options.web,
                        learning: Some(learning.clone()),
                        workshop: Some(Arc::clone(&workshop)),
                    };
                    lookups = Some(ai::lookup::Lookups::new(
                        Arc::new(OpenAi::new(
                            &key,
                            &options.openai_base,
                            &options.voice_name,
                            options.model.as_deref(),
                        )),
                        Some(learning.clone()),
                    ));
                    // Grok answers when there is an xAI key that works.
                    let grok = ai::load_xai_key(&settings_dir)
                        .filter(|_| options.grok)
                        .and_then(|xai| {
                            let model = options
                                .grok_model
                                .clone()
                                .unwrap_or_else(|| ai::GROK_MODEL.to_string());
                            let base = std::env::var("XAI_BASE_URL")
                                .unwrap_or_else(|_| ai::XAI_BASE.to_string());
                            let grok = OpenAi::with_models(
                                &xai,
                                &base,
                                &options.voice_name,
                                vec![model.clone()],
                            );
                            match grok.check() {
                                Err(AiError::Http(401 | 403, why)) => {
                                    ai_note.push_str(&format!("; xAI refused the key ({why})"));
                                    None
                                }
                                _ => {
                                    ai_note.push_str(&format!("; Grok ({model}) answers"));
                                    Some(grok)
                                }
                            }
                        });
                    if elevenlabs_key.is_some() {
                        ai_note.push_str("; ElevenLabs voices");
                    }
                    Some(ai::spawn_brains(
                        ai::Brains {
                            openai: client,
                            fast: grok,
                            eleven: elevenlabs_key
                                .as_deref()
                                .map(|key| ai::eleven::Eleven::new(key, &eleven_base)),
                        },
                        brain,
                        Some(toolbox),
                    ))
                }
            }
        }
        None => None,
    };
    // Without a model, the teacher still labels the HUD's font from the OCR
    // engine.
    if !teacher_started {
        teaching::spawn(
            None,
            Arc::clone(&learned),
            Arc::clone(&latest),
            news_tx.clone(),
        );
    }

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

    // A live call on the phone when it can be made, unless asked otherwise.
    let replies = options.replies.unwrap_or(if live_service.is_some() {
        VoiceOn::Phone
    } else {
        VoiceOn::Pc
    });
    let phone = if options.phone {
        match start_phone(&options, replies, &settings_dir, &session_dir) {
            Ok(link) => {
                if let Some(service) = &live_service {
                    link.hub
                        .set_service(Arc::clone(service) as Arc<dyn phone::Service>);
                }
                Some(link)
            }
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
        // Only the HUD reaches the companion; the other detectors are for
        // the preview window, and run only when it was asked for.
        let wanted = if options.preview {
            Detectors::ALL
        } else {
            Detectors::HUD
        };
        let mailbox = Arc::new(Mailbox::default());
        {
            let (mailbox, running) = (Arc::clone(&mailbox), Arc::clone(&running));
            std::thread::Builder::new()
                .name("capture".into())
                .spawn(move || grab(source, mailbox, running, fps))
                .map_err(|e| e.to_string())?;
        }
        std::thread::Builder::new()
            .name("vision".into())
            .spawn(move || watch(mailbox, slot, running, start, wanted, shared))
            .map_err(|e| e.to_string())?
    };

    // Warnings where the player asked for them (or it learned), unless
    // given on the command line.
    let (hp_low, mp_low) = {
        let memory = learning.memory();
        (
            options.hp_low.or(memory.adapt.hp_low).unwrap_or(30.0),
            options.mp_low.or(memory.adapt.mp_low).unwrap_or(15.0),
        )
    };
    let mut companion = Companion::new(Settings {
        hp_low,
        hp_rearm: (hp_low + 15.0).min(95.0),
        mp_low,
        mp_rearm: (mp_low + 15.0).min(95.0),
        always_listen: !options.wake_word,
        ..Settings::default()
    });
    companion.settings.attitude = learning.memory().attitude;
    // The warnings as kept (a death no warning came before moves them).
    let mut warn_at = (hp_low, mp_low);
    // Answers said at once (varied by how many there were).
    let mut instant_count = 0u32;
    // What the phone shows of what it learned (looked at every few seconds;
    // None: at the next chance).
    let mut memory_status = serde_json::Value::Null;
    let mut memory_checked: Option<Instant> = None;
    let mut out = Outputs {
        mouth: Mouth {
            ai: worker,
            sapi,
            player: Player::new(),
            sapi_until: Instant::now(),
            phone_until: Instant::now(),
            phone_end: Instant::now(),
            line: None,
            hushed: None,
            phone_line: Vec::new(),
            phone_held: Vec::new(),
            last_voice: None,
            phone_ducked: false,
            game_pid: None,
        },
        phone: phone.as_ref().map(|p| Arc::clone(&p.hub)),
        session,
        log: Vec::new(),
        plain: !ansi,
        start,
        replies,
        language: None,
        live_ok: live_service.is_some(),
        live: false,
        relay: Relay::default(),
        greeted: None,
        call_greets: false,
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
    // The session's recording (the phone's Record button, or --record).
    let mut recording =
        recording::Recording::new(&settings_dir, &session_dir, options.overlay_on_stream);
    if options.record {
        recording.start(&mut out);
    }
    let mut dog = Dog::load();
    let mut ai_error_shown = String::new();
    let mut model_logged = false;
    let mut player_language: Option<String> = None;
    let mut turns = Turns::default();
    // The coach: MapleSyrup speaking up on its own (off when the player
    // asked for that, for good).
    let mut coach = ms::coach::Coach::new(learning.memory().coach.unwrap_or(true));
    // When the player was last heard (talking, or talked; None: not yet),
    // and the last level-up the companion announced: the coach keeps out of
    // the way.
    let mut player_heard: Option<Instant> = None;
    // Since when the player has been talking on a call (None: they aren't;
    // a start the phone never ends is forgotten after a while).
    let mut player_talking_since: Option<Instant> = None;
    let mut level_up_seen = f64::NEG_INFINITY;
    // The coach's look under way, to call it off when the player talks.
    let mut coach_job: Option<u64> = None;

    let mut preview: Option<Preview> = None;
    let mut preview_failed = false;
    let mut latest_image: Option<Arc<RgbaImage>> = None;
    let mut frame_size = None;
    let mut fps = 0.0;
    let mut drawn_lines = 0usize;
    let mut last_draw: Option<Instant> = None;
    let mut last_panel: Option<Instant> = None;
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
        if relaunch_as.is_some() {
            break;
        }
        let now = start.elapsed().as_secs_f64();
        // What the updater did; and this version, once it has run long
        // enough, is kept for good (the previous one let go).
        while let Ok(event) = update_rx.try_recv() {
            match event {
                ms::update::Event::Said(line) => {
                    out.session.line("update", &line);
                    out.push(Kind::Info, line);
                }
                ms::update::Event::Noted(line) => out.session.line("update", &line),
            }
        }
        while let Ok(event) = workshop_rx.try_recv() {
            match event {
                ms::workshop::Event::Said(line) => {
                    out.session.line("workshop", &line);
                    say_line(&mut out, line);
                }
                ms::workshop::Event::Noted(line) => out.session.line("workshop", &line),
                ms::workshop::Event::Staged => updater.refresh(),
            }
        }
        if !update_committed && start.elapsed() >= ms::update::HEALTHY_AFTER {
            update_committed = true;
            if let Some(version) = ms::update::commit(&settings_dir) {
                let line = format!("MapleSyrup {version} is in for good");
                out.session.line("update", &line);
                out.push(Kind::Info, line);
            }
        }

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
            let actions = companion.observe(tick.at, tick.obs.clone());
            if actions
                .iter()
                .any(|a| matches!(a, Action::Say(s) if s.speak && s.kind != Kind::Heard))
            {
                coach.someone_spoke(now);
            }
            out.apply(actions, &mut companion, latest_image.clone());
            if companion.last_level_up() > level_up_seen {
                level_up_seen = companion.last_level_up();
                coach.leveled(now, companion.level());
            }
            // The coach: is it time for a look at the game, and why?
            if let Some(worker) = &out.mouth.ai {
                let in_view = in_front.load(Ordering::Relaxed);
                let talking = out.mouth.speaking()
                    || worker.busy()
                    || turns.busy()
                    || player_heard.is_some_and(|at| at.elapsed() < Duration::from_secs(3))
                    || player_talking_since.is_some_and(|t| t.elapsed() < Duration::from_secs(30));
                let glance = ms::coach::Glance {
                    now,
                    obs: &tick.obs,
                    scene: tick.scene.as_ref(),
                    in_view,
                    talking,
                    muted: companion.muted(),
                    dead: companion.dead(),
                };
                if let Some(reason) = coach.observe(&glance) {
                    let snapshot = snapshot_text(&companion, sight.as_ref());
                    let status = sight.as_ref().and_then(|s| {
                        s.lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .layout
                            .as_ref()
                            .and_then(|l| l.status)
                    });
                    // (A reaction — a close call, a streak — goes without
                    // the picture: it is about what just happened, and a
                    // second sooner.)
                    let eyes = latest_image
                        .clone()
                        .filter(|_| in_view && reason.wants_picture())
                        .map(|frame| Eyes { frame, status });
                    out.session.line(
                        "coach",
                        &format!(
                            "looking: {} (activity {:.3})",
                            reason.label(),
                            tick.scene.as_ref().map(|s| s.activity).unwrap_or(0.0)
                        ),
                    );
                    coach_job = Some(worker.send(Job::Coach {
                        reason: reason.describe(companion.settings.attitude),
                        label: reason.label(),
                        snapshot,
                        eyes,
                        said: coach.lines(),
                        language: player_language.clone(),
                        speak: !out.live,
                    }));
                }
            }
            // Died without a warning: HP warnings come sooner now, for good.
            if companion.settings.hp_low != warn_at.0 {
                warn_at.0 = companion.settings.hp_low;
                let mut memory = learning.memory();
                memory.adapt.hp_low = Some(warn_at.0);
                memory.save();
                out.session.line(
                    "learned",
                    &format!("HP warnings below {:.0}% from now on", warn_at.0),
                );
            }
            // The things the player taught: their alerts (held with the
            // companion's own while nothing the player does answers them).
            // One past the mark they set ("the boss under 20%") is a
            // warning, shouted; a thing that showed up is news, told.
            for fired in tick.fired {
                let kind = if fired.warning {
                    Kind::Warning
                } else {
                    Kind::Alert
                };
                if companion.alerts_held(now) {
                    out.session
                        .line(kind_label(kind), &format!("(held) {}", fired.say));
                } else {
                    out.tell(kind, &fired.say, true, &mut companion);
                }
                if let Some(waits) = fired.waits {
                    out.session.line(
                        "alert",
                        &format!(
                            "\"{}\" keeps firing: it next waits {} s (forget the thing if it is mis-taught)",
                            fired.name,
                            waits.as_secs()
                        ),
                    );
                }
            }
            if let Some(note) = tick.note {
                out.session.line("capture", &note);
                out.push(Kind::Info, note);
            }
        } else if let Some(p) = preview.as_mut() {
            p.pump();
        }
        if let Some(window) = &panel_window {
            window.pump();
        }
        out.mouth.player.tick();
        if out.mouth.speaking() {
            out.mouth.last_voice = Some(Instant::now());
        }
        // The phone never said its voice stopped (it went away): the game
        // comes back up.
        if out.mouth.phone_ducked && Instant::now() > out.mouth.phone_until {
            out.phone_talking(false);
        }

        // What the phone sent.
        if let Some(hub) = out.phone.clone() {
            for inbound in hub.take_inbox() {
                match inbound {
                    Inbound::Heard(heard) => {
                        player_heard = Some(Instant::now());
                        coach.someone_spoke(now);
                        // A look under way gives way to the player.
                        if let (Some(id), Some(worker)) = (coach_job.take(), &out.mouth.ai) {
                            worker.cancel(id);
                        }
                        // MapleSyrup's own voice, heard back by the phone, is
                        // taken out; what is left is the player's. Heard while
                        // it was talking, there must be enough of the player's
                        // own words in it.
                        let need = if out.mouth.speaking() {
                            2
                        } else if out
                            .mouth
                            .last_voice
                            .is_some_and(|at| at.elapsed() < Duration::from_millis(2500))
                        {
                            1
                        } else {
                            0
                        };
                        // ("mute" or "mark that" said over it still counts.)
                        let own = companion.own_words(now, &heard, need).or_else(|| {
                            companion
                                .own_words(now, &heard, 0)
                                .filter(|t| commands::local_command(t).is_some())
                        });
                        let text = match own {
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
                        companion.player_spoke(now);
                        if let Some(on) = commands::recording_request(&text) {
                            out.show(Kind::Heard, &text);
                            if on {
                                recording.start(&mut out);
                            } else {
                                recording.stop(&mut out, panel_window.as_mut());
                            }
                            continue;
                        }
                        // "Change yourself: …" / "undo the last change": the
                        // workshop's, when it is on.
                        if workshop.is_on() {
                            let task = if ms::workshop::undo_request(&text) {
                                Some(ms::workshop::Task::Undo)
                            } else {
                                ms::workshop::request(&text).map(|instruction| {
                                    ms::workshop::Task::Change {
                                        instruction,
                                        context: recent_log(&out),
                                    }
                                })
                            };
                            if let Some(task) = task {
                                out.show(Kind::Heard, &text);
                                workshop_ask(&workshop, task, &mut out);
                                continue;
                            }
                        }
                        if let Some(on) = commands::coaching_request(&text) {
                            out.show(Kind::Heard, &text);
                            set_coaching(&mut coach, &learning, &mut out, on);
                            continue;
                        }
                        if commands::tone_complaint(&text) {
                            out.show(Kind::Heard, &text);
                            drop_the_attitude(&mut companion, &learning, &mut out);
                            continue;
                        }
                        if out.mouth.ai.is_some() {
                            out.show(Kind::Heard, &text);
                            if let Some(command) = commands::local_command(&text) {
                                let actions = companion.command(now, command);
                                out.apply(actions, &mut companion, latest_image.clone());
                            } else if companion.settings.always_listen
                                || !matches!(commands::interpret(&text, false), Heard::NotForUs)
                            {
                                let text = turns.heard(&mut out, text);
                                // Their own numbers: answered at once, without
                                // a model.
                                let instant = ms::companion::instant::asks(&text).and_then(|ask| {
                                    ms::companion::instant::answer(
                                        ask,
                                        &text,
                                        companion.last(),
                                        &companion.progress(),
                                        companion.settings.attitude,
                                        instant_count,
                                    )
                                });
                                if let Some(line) = instant {
                                    instant_count += 1;
                                    out.session.line("timing", "instant answer");
                                    if let Some(worker) = &out.mouth.ai {
                                        let id = worker.send(Job::Say {
                                            heard: Some(text.clone()),
                                            text: line,
                                        });
                                        // Its turn, like any reply's: talked
                                        // over, it is the one cut short.
                                        turns.asked(id, &text);
                                    }
                                    continue;
                                }
                                let job = conversation_job(
                                    text.clone(),
                                    &companion,
                                    sight.as_ref(),
                                    latest_image.clone(),
                                    in_front.load(Ordering::Relaxed),
                                    player_language.clone(),
                                );
                                if let Some(worker) = &out.mouth.ai {
                                    let id = worker.send(job);
                                    turns.asked(id, &text);
                                }
                            }
                        } else {
                            let actions = companion.heard(now, &text);
                            out.apply(actions, &mut companion, latest_image.clone());
                        }
                    }
                    Inbound::Hearing(text) => {
                        player_heard = Some(Instant::now());
                        // Words as they are said: talked over, or still
                        // talking. (Without a natural voice there is nothing
                        // to stop.)
                        if out.mouth.ai.is_some()
                            && (companion.settings.always_listen
                                || !matches!(commands::interpret(&text, false), Heard::NotForUs)
                                || commands::local_command(&text).is_some())
                            && let Some(what) = turns.hearing(&mut out, &companion, now, &text)
                        {
                            out.session.line("turn", &what);
                        }
                    }
                    Inbound::Interrupt => {
                        // The phone's own detection (a loud sound over its
                        // voice; it only reports one while a clip plays)
                        // is not trusted on its own: it fires on its echo
                        // and on the game. The player's words, if they
                        // follow, make it a talk-over (`Turns::hearing`).
                        turns.loud(&mut out);
                        out.session
                            .line("turn", "loud over the phone's voice: waiting for words");
                    }
                    Inbound::Said { who, text } if who == "timing" => {
                        // Where the time went on the call, from the phone.
                        out.session.line("timing", &text);
                    }
                    Inbound::Said { who, text } => {
                        // On a live call: shown here and kept in the log (the
                        // phone shows it itself).
                        coach.someone_spoke(now);
                        let kind = if who == "player" {
                            player_heard = Some(Instant::now());
                            companion.player_spoke(now);
                            Kind::Heard
                        } else {
                            companion.remember_spoken(now, &text);
                            Kind::Reply
                        };
                        out.session.line(kind_label(kind), &text);
                        out.push(kind, text.clone());
                        // What the player asks of MapleSyrup itself — to
                        // record, to coach or not, to change its code, to
                        // drop the attitude, to mute — is done here, on a
                        // call as off one: the call's model talks, it does
                        // not run MapleSyrup.
                        if who == "player" {
                            if let Some(on) = commands::recording_request(&text) {
                                if on {
                                    recording.start(&mut out);
                                } else {
                                    recording.stop(&mut out, panel_window.as_mut());
                                }
                            } else if let Some(on) = commands::coaching_request(&text) {
                                set_coaching(&mut coach, &learning, &mut out, on);
                            } else if commands::tone_complaint(&text) {
                                drop_the_attitude(&mut companion, &learning, &mut out);
                            } else if workshop.is_on() && ms::workshop::undo_request(&text) {
                                workshop_ask(&workshop, ms::workshop::Task::Undo, &mut out);
                            } else if workshop.is_on()
                                && let Some(instruction) = ms::workshop::request(&text)
                            {
                                let task = ms::workshop::Task::Change {
                                    instruction,
                                    context: recent_log(&out),
                                };
                                workshop_ask(&workshop, task, &mut out);
                            } else if let Some(command) = commands::local_command(&text)
                                && matches!(command, Command::Mute | Command::Unmute)
                            {
                                let actions = companion.command(now, command);
                                out.apply(actions, &mut companion, latest_image.clone());
                            }
                        }
                    }
                    Inbound::Live(on) => {
                        // (The call that opens says the hello left to it.)
                        if on {
                            out.call_greets = false;
                        }
                        if on != out.live {
                            out.live = on;
                            // (Lines waiting for the old call are not for the new.)
                            out.relay = Relay::default();
                            out.session.line(
                                "info",
                                if on {
                                    "live call on the phone: talking in real time"
                                } else {
                                    "live call ended"
                                },
                            );
                            if on {
                                // The PC's own voice gives way to the call.
                                turns.interrupt(&mut out);
                            } else {
                                out.phone_talking(false);
                            }
                        }
                    }
                    Inbound::Talking(on) => out.phone_talking(on),
                    Inbound::PlayerTalking(on) => {
                        // On a call the player's words reach the PC only once
                        // written down; the coach keeps out of the way from
                        // their first sound.
                        if on {
                            player_talking_since = Some(Instant::now());
                        } else {
                            player_talking_since = None;
                            player_heard = Some(Instant::now());
                        }
                        coach.someone_spoke(now);
                    }
                    // (The learner reads it from the log.)
                    Inbound::Turn(what) => out.session.line("turn", &what),
                    // A line the call was handed and never said: the log
                    // must not say it was.
                    Inbound::NotSaid(text) => out
                        .session
                        .line("live", &format!("not said, too late: {text}")),
                    Inbound::Speaker(id) => {
                        let openai = id == "openai";
                        let known = openai
                            || voices
                                .lock()
                                .unwrap_or_else(|e| e.into_inner())
                                .iter()
                                .any(|v| v.id == id);
                        if known {
                            {
                                let mut memory = learning.memory();
                                memory.voice = Some(id.clone());
                                memory.save();
                            }
                            out.session.line("info", &format!("voice: {id}"));
                            // A word in the new voice.
                            if let Some(worker) = &out.mouth.ai
                                && !companion.muted()
                            {
                                worker.send(Job::Speak {
                                    text: "This is my voice now.".into(),
                                    language: out.language.clone(),
                                    kind: Kind::Info,
                                    show: false,
                                    speak: true,
                                });
                            }
                        }
                    }
                    Inbound::Attitude(attitude) => {
                        {
                            let mut memory = learning.memory();
                            memory.attitude = attitude;
                            memory.save();
                        }
                        companion.settings.attitude = attitude;
                        memory_checked = None;
                        out.session
                            .line("info", &format!("attitude: {}", attitude.word()));
                        out.push(Kind::Info, format!("attitude: {}", attitude.word()));
                    }
                    Inbound::Record(true) => recording.start(&mut out),
                    Inbound::Record(false) => recording.stop(&mut out, panel_window.as_mut()),
                    Inbound::Effect(effect) => match effect {
                        ai::Effect::Note(line) => out.show(Kind::Info, &line),
                        ai::Effect::Fact(fact) => {
                            out.show(Kind::Info, &format!("remembered: {fact}"))
                        }
                        ai::Effect::LookUp {
                            question,
                            said,
                            asked,
                        } => {
                            if let Some((runner, _)) = &lookups {
                                runner.start(&question, &said, asked, player_language.as_deref());
                            }
                        }
                        ai::Effect::Warn { what, below } => {
                            let line = set_warning(
                                &mut companion,
                                &learning,
                                &options,
                                &mut warn_at,
                                &what,
                                below,
                            );
                            out.show(Kind::Info, &line);
                        }
                        ai::Effect::Command(word)
                            if word == ai::tools::COACH_ON || word == ai::tools::COACH_OFF =>
                        {
                            let on = word == ai::tools::COACH_ON;
                            set_coaching(&mut coach, &learning, &mut out, on);
                        }
                        ai::Effect::Command(word) if word == ai::tools::RECORD_ON => {
                            recording.start(&mut out)
                        }
                        ai::Effect::Command(word) if word == ai::tools::RECORD_OFF => {
                            recording.stop(&mut out, panel_window.as_mut())
                        }
                        ai::Effect::Command(word) => {
                            if let Some(command) = Command::from_word(&word) {
                                // The call's model tells the player what its
                                // tool did; the companion's word on it is
                                // shown, not said again.
                                let actions = unspoken(companion.command(now, command));
                                out.apply(actions, &mut companion, latest_image.clone());
                            }
                        }
                        ai::Effect::Rewrite(instruction) => {
                            out.session
                                .line("workshop", &format!("asked on the call: {instruction}"));
                            let task = ms::workshop::Task::Change {
                                instruction,
                                context: recent_log(&out),
                            };
                            if let Err(why) = workshop.ask(task) {
                                out.push(Kind::Info, format!("couldn't start the change: {why}"));
                            }
                        }
                    },
                    Inbound::Command(word) => {
                        if let Some(command) = Command::from_word(&word) {
                            let actions = companion.command(now, command);
                            out.apply(actions, &mut companion, latest_image.clone());
                        }
                    }
                    Inbound::Hello { agent, live, away } => {
                        out.hello(
                            &mut companion,
                            learning.knows_player(),
                            &agent,
                            live,
                            away,
                            player_language.clone(),
                        );
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
                    Inbound::Language(locale) => {
                        let english = ai::language::is_english(&locale);
                        let changed = out.language.as_deref() != Some(locale.as_str());
                        out.language = (!english).then(|| locale.clone());
                        player_language = Some(locale.clone());
                        if changed {
                            out.session.line(
                                "info",
                                &format!("language: {} ({locale})", ai::language::name(&locale)),
                            );
                        }
                    }
                    Inbound::Forget(id) => {
                        // What it learned about the player, or a thing on screen.
                        let forgotten = learning.forget(&id).or_else(|| {
                            sight.as_ref().and_then(|sight| {
                                sight
                                    .lock()
                                    .unwrap_or_else(|e| e.into_inner())
                                    .things
                                    .forget(&id)
                            })
                        });
                        if let Some(name) = forgotten {
                            memory_checked = None;
                            out.tell(
                                Kind::Info,
                                &format!("I forgot \"{name}\"."),
                                false,
                                &mut companion,
                            );
                        }
                    }
                    Inbound::Workshop(ask) => match ask {
                        phone::WorkshopAsk::On(on) => {
                            workshop.set_on(on);
                            let mut memory = learning.memory();
                            memory.workshop = Some(on);
                            let updates = memory.updates.unwrap_or(true);
                            memory.save();
                            drop(memory);
                            // The channel's releases would wipe the local
                            // work: off while the workshop is on.
                            updater.set_auto(!on && options.update && updates);
                            let line = if on {
                                match workshop.coders().first() {
                                    Some(coder) => format!(
                                        "Workshop on: say \"change yourself: …\" and {} rewrites me here, on this PC; the site's updates are off meanwhile.",
                                        coder.label()
                                    ),
                                    None => "Workshop on, but no coding agent is installed on this PC (Claude Code or Codex CLI): nothing can be changed until one is.".to_string(),
                                }
                            } else {
                                "Workshop off: the site's updates are back on.".to_string()
                            };
                            out.session.line("workshop", &line);
                            out.push(Kind::Info, line);
                        }
                        phone::WorkshopAsk::Change(instruction) => {
                            out.show(
                                Kind::Heard,
                                &format!("[phone] change yourself: {instruction}"),
                            );
                            workshop_ask(
                                &workshop,
                                ms::workshop::Task::Change {
                                    instruction,
                                    context: recent_log(&out),
                                },
                                &mut out,
                            );
                        }
                        phone::WorkshopAsk::Undo => {
                            out.show(Kind::Heard, "[phone] undo the last change");
                            workshop_ask(&workshop, ms::workshop::Task::Undo, &mut out);
                        }
                        phone::WorkshopAsk::Coder(name) => {
                            match ms::workshop::Coder::parse(&name) {
                                Some(coder) => match workshop.prefer(coder) {
                                    Ok(()) => {
                                        let mut memory = learning.memory();
                                        memory.workshop_coder = Some(name);
                                        memory.save();
                                        out.session
                                            .line("workshop", &format!("coder: {}", coder.label()));
                                    }
                                    Err(e) => out.push(Kind::Info, e),
                                },
                                None => out.push(Kind::Info, format!("no such coder: {name}")),
                            }
                        }
                    },
                    Inbound::Update(ask) => match ask {
                        phone::UpdateAsk::Check => updater.check_now(),
                        phone::UpdateAsk::Auto(on) => {
                            updater.set_auto(on);
                            let mut memory = learning.memory();
                            memory.updates = Some(on);
                            memory.save();
                            out.session.line(
                                "update",
                                if on {
                                    "updates itself again"
                                } else {
                                    "no more updates on its own"
                                },
                            );
                        }
                        phone::UpdateAsk::Install => match std::env::current_exe()
                            .map_err(|e| e.to_string())
                            .and_then(|exe| updater.install_now(&exe))
                        {
                            Ok(version) => {
                                out.push(
                                    Kind::Info,
                                    format!("MapleSyrup {version} is in place: restarting"),
                                );
                                relaunch_as = Some(version);
                            }
                            Err(e) => out.push(Kind::Info, format!("couldn't update now: {e}")),
                        },
                    },
                    Inbound::Coach(on) => {
                        if on != coach.on {
                            set_coaching(&mut coach, &learning, &mut out, on);
                        }
                    }
                    Inbound::Listen(always) => {
                        companion.set_always_listen(always);
                        out.tell(
                            Kind::Info,
                            if always {
                                "I answer everything you say now."
                            } else {
                                "I answer only when you say \"syrup\" now."
                            },
                            false,
                            &mut companion,
                        );
                    }
                }
            }
        }

        recording.tick(&mut out, panel_window.as_mut());
        out.release_clips(player_quiet(player_heard, &turns));
        // On a call, the watcher's lines that waited for their turn.
        if out.live {
            out.relay_due(&companion);
        }

        // Words that waited for the rest of a sentence that never came.
        if let Some(text) = turns.overdue() {
            let job = conversation_job(
                text.clone(),
                &companion,
                sight.as_ref(),
                latest_image.clone(),
                in_front.load(Ordering::Relaxed),
                player_language.clone(),
            );
            if let Some(worker) = &out.mouth.ai {
                let id = worker.send(job);
                turns.asked(id, &text);
            }
        }

        // What the brain came back with.
        let finished: Vec<Done> = match &out.mouth.ai {
            Some(worker) => worker.done.try_iter().collect(),
            None => Vec::new(),
        };
        for done in finished {
            match done {
                Done::Reply { id, text, took, .. } => {
                    turns.finished(id);
                    companion.remember_spoken(now, &text);
                    coach.someone_spoke(now);
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
                Done::Coached {
                    id,
                    label,
                    text,
                    error,
                    called_off,
                    took,
                } => {
                    if coach_job == Some(id) {
                        coach_job = None;
                    }
                    let took = took.as_secs_f64();
                    // A look the player's words called off never answered:
                    // the coach looks again at the next gap, no slower.
                    if called_off {
                        coach.called_off();
                        out.session
                            .line("coach", &format!("{label}: called off ({took:.1} s)"));
                        continue;
                    }
                    coach.answered(now, text.as_deref());
                    match (text, error) {
                        (Some(line), _) => {
                            companion.remember_spoken(now, &line);
                            out.session
                                .line("coach", &format!("{label}: said in {took:.1} s"));
                            if out.live {
                                // The call says it, in its own words.
                                out.tell(Kind::Alert, &line, true, &mut companion);
                            }
                            // (Else the worker showed and said it already.)
                        }
                        (None, Some(error)) => {
                            out.session
                                .line("coach", &format!("{label}: couldn't look ({error})"));
                        }
                        (None, None) => {
                            out.session
                                .line("coach", &format!("{label}: nothing to say ({took:.1} s)"));
                        }
                    }
                }
                Done::Shown { kind, text } => {
                    // Its own line, translated: what the phone may hear back.
                    companion.remember_spoken(now, &text);
                    out.show(kind, &text);
                }
                Done::LookUp {
                    question,
                    said,
                    asked,
                    language,
                } => {
                    if let Some((runner, _)) = &lookups {
                        runner.start(&question, &said, asked, language.as_deref());
                    }
                }
                Done::Warn { what, below } => {
                    let line = set_warning(
                        &mut companion,
                        &learning,
                        &options,
                        &mut warn_at,
                        &what,
                        below,
                    );
                    out.show(Kind::Info, &line);
                }
                Done::Command { word }
                    if word == ai::tools::COACH_ON || word == ai::tools::COACH_OFF =>
                {
                    let on = word == ai::tools::COACH_ON;
                    set_coaching(&mut coach, &learning, &mut out, on);
                }
                Done::Rewrite { instruction } => {
                    out.session.line(
                        "workshop",
                        &format!("asked through the model: {instruction}"),
                    );
                    let task = ms::workshop::Task::Change {
                        instruction,
                        context: recent_log(&out),
                    };
                    // The model already told the player; only a refusal is
                    // worth a line.
                    if let Err(why) = workshop.ask(task) {
                        out.push(Kind::Info, format!("couldn't start the change: {why}"));
                    }
                }
                Done::Command { word } if word == ai::tools::RECORD_ON => recording.start(&mut out),
                Done::Command { word } if word == ai::tools::RECORD_OFF => {
                    recording.stop(&mut out, panel_window.as_mut())
                }
                Done::Command { word } => {
                    if let Some(command) = Command::from_word(&word) {
                        let actions = companion.command(now, command);
                        out.apply(actions, &mut companion, latest_image.clone());
                    }
                }
                Done::Audio {
                    id,
                    kind,
                    text,
                    samples,
                    after,
                    first,
                    start,
                    end,
                } => {
                    // A reply that was talked over: its last pieces are dropped.
                    if out.mouth.ai.as_ref().is_some_and(|w| w.cancelled(id)) {
                        continue;
                    }
                    if start {
                        companion.remember_spoken(now, &text);
                        // When it will be heard: after what is still to play.
                        let at = if out.voice_on().pc() {
                            Instant::now() + out.mouth.player.remaining()
                        } else {
                            out.mouth.phone_end.max(Instant::now())
                        };
                        turns.spoke(id, at, &text);
                        if first {
                            out.session.line(
                                "timing",
                                &format!("first words after {:.1} s", after.as_secs_f64()),
                            );
                        }
                    }
                    // A line of its own (not the answer they are waiting
                    // for) waits while they are talking, and for their
                    // answer: a clip starting on the phone cuts off the
                    // sentence it is writing down.
                    let own = turns.reply_id() != Some(id);
                    let hold = own && !player_quiet(player_heard, &turns);
                    out.play_piece(
                        Piece {
                            id,
                            kind,
                            samples: &samples,
                            start,
                            end,
                        },
                        hold,
                        &companion,
                    );
                }
                Done::Silent { id, heard } => {
                    turns.finished(id);
                    out.session.line("silent", &heard);
                }
                Done::Failed { id, heard, error } => {
                    turns.finished(id);
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
                    // Answer anyway, simply, in the Windows voice (where
                    // the PC is to speak at all).
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
                                    && out.voice_on().pc()
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
                    out.tell(
                        Kind::Info,
                        "I found your HUD: I measure HP, MP and EXP myself now, and check them every couple of minutes.",
                        false,
                        &mut companion,
                    );
                }
                News::Trouble(why) => {
                    out.session.line("sight", &why);
                    out.push(Kind::Info, format!("sight: {why}"));
                }
            }
        }

        // What the look-ups found: said only when the quick answer was
        // wrong, or the player asked for it.
        let found: Vec<ai::lookup::Found> = lookups
            .as_ref()
            .map(|(_, rx)| rx.try_iter().collect())
            .unwrap_or_default();
        for found in found {
            match found {
                ai::lookup::Found::Say(text) => {
                    out.session.line("lookup", &text);
                    if out.live {
                        // The call says it, in its own words.
                        out.tell(Kind::Info, &text, true, &mut companion);
                    } else if let Some(worker) = &out.mouth.ai {
                        worker.send(Job::Say { heard: None, text });
                    }
                }
                ai::lookup::Found::Right => {
                    out.session.line("lookup", "the quick answer was right")
                }
                ai::lookup::Found::Failed(why) => {
                    out.session.line("lookup", &why);
                    out.push(Kind::Info, why);
                }
            }
        }

        // What the learner learned or adapted to.
        while let Ok(line) = voices_rx.try_recv() {
            out.session.line("info", &line);
            out.push(Kind::Info, line);
        }
        while let Ok(line) = learned_rx.try_recv() {
            memory_checked = None;
            if line.starts_with("learned from") || line.starts_with("adapted") {
                out.tell(Kind::Info, &line, false, &mut companion);
            } else {
                out.session.line("learned", &line);
                out.push(Kind::Info, line);
            }
        }

        // Where the game is (for the panel, and whose sound to turn down).
        if last_draw.is_none_or(|at| at.elapsed() >= Duration::from_millis(250)) {
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
        if last_panel.is_none_or(|at| at.elapsed() >= Duration::from_millis(80)) {
            last_panel = Some(Instant::now());
            let speaking = out.mouth.speaking();
            if let Some(window) = panel_window.as_mut() {
                match game_area {
                    Some(area) if area.foreground && area.width > 200 && area.height > 150 => {
                        let scale = panel::scale_for(area.height);
                        let (_, ph) = panel::size(scale);
                        // The dog: its mouth with the PC's voice, its head
                        // tilting while it thinks.
                        let mood = ms::app::dog::Mood {
                            speaking,
                            level: out.mouth.player.loudness(),
                            thinking: out.mouth.ai.as_ref().is_some_and(|w| w.busy()),
                        };
                        let dog_frame = dog.as_mut().map(|d| d.frame(ph, mood));
                        let recent = out
                            .log
                            .iter()
                            .rev()
                            .find(|l| matches!(l.kind, Kind::Reply | Kind::Warning | Kind::Alert))
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

        if last_draw.is_none_or(|at| at.elapsed() >= Duration::from_millis(250)) {
            last_draw = Some(Instant::now());
            let progress = companion.progress();
            let summary = out.phone.as_ref().map(|h| h.summary());
            if let Some(hub) = &out.phone {
                if out.mouth.ai.is_some()
                    && memory_checked.is_none_or(|at| at.elapsed() >= Duration::from_secs(3))
                {
                    memory_checked = Some(Instant::now());
                    memory_status = learning.status();
                }
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
                    // The dog plays dead with the character.
                    "dead": companion.dead(),
                    "fps": fps,
                    "wake": "syrup",
                    "always_listen": companion.settings.always_listen,
                    // Whether it speaks up on its own (the coach).
                    "coach": coach.on,
                    // This version, and whether a newer one is on its way.
                    "update": updater.status().to_json(),
                    // Whether it rewrites itself here, and how that is going.
                    "workshop": workshop.state().to_json(),
                    "speaking": out.mouth.speaking(),
                    // The PC's own voice (the phone keeps listening through it).
                    "speaking_pc": out.mouth.pc_speaking(),
                    "thinking": out.mouth.ai.as_ref().is_some_and(|w| w.busy()),
                    "ai": out.mouth.ai.as_ref().map(|w| w.model.lock().ok().and_then(|m| m.clone()).unwrap_or_else(|| "OpenAI".into())),
                    "learned": learned_status(sight.as_ref(), hub),
                    // Live calls can be made; and the next call to open says
                    // hello (the phone's hello was left to it).
                    "live": out.live_ok,
                    "call_greets": out.call_greets,
                    "recording": recording.status(),
                    // How it talks, and who answers.
                    "attitude": companion.settings.attitude,
                    // The voices to pick from (ElevenLabs), and the one picked.
                    "voices": *voices.lock().unwrap_or_else(|e| e.into_inner()),
                    "voice": learning.memory().voice.clone().unwrap_or_else(|| "openai".into()),
                    // How long the phone waits after the words stop (it adapts).
                    "settle_ms": learning.memory().adapt.settle_ms,
                    // What it learned about the player (with an OpenAI key).
                    "memory": if out.mouth.ai.is_some() { memory_status.clone() } else { serde_json::Value::Null },
                    "warn": {"hp": companion.settings.hp_low, "mp": companion.settings.mp_low},
                }));
                // What a live call can see: the screen only while the game is
                // the window in front.
                if out.live_ok {
                    let in_view = in_front.load(Ordering::Relaxed)
                        && companion.last().is_some_and(|o| o.game.is_seen());
                    hub.set_sight(
                        latest_image.clone().filter(|_| in_view),
                        snapshot_text(&companion, sight.as_ref()),
                    );
                }
            }
            if ansi {
                let recording_label = recording.label();
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
                    recording: recording_label.as_deref(),
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
    recording.finish();
    let progress = companion.progress();
    println!(
        "\nStopped after {}. {} mark{} saved in {}",
        screen::short_duration(progress.seconds),
        progress.marks,
        if progress.marks == 1 { "" } else { "s" },
        session_dir.display()
    );
    if let Some(version) = relaunch_as {
        println!("Starting MapleSyrup {version}…");
        if let Ok(exe) = std::env::current_exe()
            && let Err(e) = ms::update::relaunch(&exe, &args)
        {
            eprintln!("{e}");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_cut_short_keeps_the_words_heard() {
        let line = "That's a great question, let me think about it for a while.";
        assert_eq!(
            heard_of(line, Duration::from_millis(1000)),
            "That's a great"
        );
        assert_eq!(heard_of(line, Duration::from_secs(10)), line);
        assert_eq!(heard_of(line, Duration::ZERO), "");
    }

    /// An instant `d` ago. (`Instant - Duration` panics when the monotonic
    /// clock is younger than `d`, as on a runner that just booted; every
    /// `d` here is seconds, and no runner is that young.)
    fn earlier(d: Duration) -> Instant {
        Instant::now()
            .checked_sub(d)
            .unwrap_or_else(|| panic!("the clock is younger than {d:?}: too soon for this test"))
    }

    fn held(words: &str, since: Instant) -> Held {
        Held {
            words: words.into(),
            since,
            more: None,
        }
    }

    #[test]
    fn words_held_for_the_rest_of_a_sentence_join_it() {
        assert_eq!(
            fold("what should I do", "with the quest".into()),
            "what should I do with the quest"
        );
        assert_eq!(
            fold(
                "what should I do",
                "What should I do with the quest?".into()
            ),
            "What should I do with the quest?"
        );
        let mut turns = Turns {
            held: Some(held("hello", Instant::now())),
            ..Default::default()
        };
        assert!(turns.overdue().is_none());
        turns.held = Some(held("hello", earlier(HOLD_FOR)));
        assert_eq!(turns.overdue().as_deref(), Some("hello"));
        assert!(turns.held.is_none());
    }

    /// The outputs as the turn-taking sees them: a phone link, and a worker
    /// that never reaches OpenAI (nothing here asks it to). Replies are
    /// spoken on the phone.
    fn outputs() -> Outputs {
        let hub = Hub::new("k1".into(), None, VoiceOn::Phone);
        let worker = ai::spawn(
            OpenAi::new("sk-test", "http://127.0.0.1:9/v1", "cedar", None),
            Brain::new(),
        );
        let now = Instant::now();
        // (A session folder of this fixture's own, with a fresh log.)
        static FIXTURES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let session_dir = std::env::temp_dir().join(format!(
            "ms-turns-{}-{:?}-{}",
            std::process::id(),
            std::thread::current().id(),
            FIXTURES.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&session_dir);
        Outputs {
            mouth: Mouth {
                ai: Some(worker),
                sapi: None,
                player: Player::new(),
                sapi_until: now,
                phone_until: now,
                phone_end: now,
                line: None,
                hushed: None,
                phone_line: Vec::new(),
                phone_held: Vec::new(),
                last_voice: None,
                phone_ducked: false,
                game_pid: None,
            },
            phone: Some(hub),
            session: Session::open(session_dir),
            log: Vec::new(),
            plain: true,
            start: now,
            replies: VoiceOn::Phone,
            language: None,
            live_ok: false,
            live: false,
            relay: Relay::default(),
            greeted: None,
            call_greets: false,
        }
    }

    /// The session log so far.
    fn log(out: &Outputs) -> String {
        std::fs::read_to_string(out.session.dir.join("log.txt")).unwrap_or_default()
    }

    /// A job the worker has nothing to do for, numbered like any.
    fn job(out: &Outputs) -> u64 {
        out.mouth.ai.as_ref().unwrap().send(Job::Say {
            heard: None,
            text: String::new(),
        })
    }

    fn cancelled(out: &Outputs, id: u64) -> bool {
        out.mouth.ai.as_ref().unwrap().cancelled(id)
    }

    /// How many times the phone was told to stop its clips.
    fn cuts(out: &Outputs) -> u64 {
        let hub = out.phone.as_ref().unwrap();
        let state = hub.handle(&phone::http::Request {
            method: "GET".into(),
            path: "/api/state".into(),
            query: vec![("k".into(), "k1".into())],
            headers: Vec::new(),
            body: Vec::new(),
        });
        serde_json::from_slice::<serde_json::Value>(&state.body).unwrap()["cut"]
            .as_u64()
            .unwrap()
    }

    /// The phone plays a reply for a while.
    fn playing(out: &mut Outputs, turns: &mut Turns, id: u64, line: &str) {
        turns.asked(id, "what's my level");
        turns.spoke(id, Instant::now(), line);
        turns.finished(id);
        out.mouth.phone_end = Instant::now() + Duration::from_secs(3);
        out.mouth.phone_until = out.mouth.phone_end + Duration::from_millis(600);
    }

    #[test]
    fn the_sentences_own_words_posted_late_are_not_more_talking() {
        let mut out = outputs();
        let companion = Companion::new(Settings::default());
        let mut turns = Turns::default();
        let id = job(&out);
        turns.asked(id, "am I talking to right");
        // The phone's throttled "words so far" landing after the sentence.
        for late in ["am I talking to right", "am I talking to", "talking"] {
            assert_eq!(turns.hearing(&mut out, &companion, 1.0, late), None);
        }
        assert!(!cancelled(&out, id));
        assert!(turns.busy() && turns.held.is_none());
        // Going on is.
        assert_eq!(
            turns
                .hearing(&mut out, &companion, 1.0, "am I talking to right now")
                .as_deref(),
            Some("still talking: am I talking to right now")
        );
        assert!(cancelled(&out, id));
        assert_eq!(
            turns.held.as_ref().map(|h| h.words.as_str()),
            Some("am I talking to right")
        );
    }

    #[test]
    fn words_overdue_are_answered_with_the_rest_the_phone_heard() {
        let mut out = outputs();
        let companion = Companion::new(Settings::default());
        let mut turns = Turns::default();
        turns.asked(job(&out), "No take a look");
        assert!(
            turns
                .hearing(&mut out, &companion, 1.0, "at yourself")
                .is_some()
        );
        // Still at it after the answer was called off: the wait starts over.
        turns.held.as_mut().unwrap().since = earlier(HOLD_FOR);
        assert_eq!(
            turns
                .hearing(
                    &mut out,
                    &companion,
                    2.0,
                    "at yourself where am I right now"
                )
                .as_deref(),
            Some("still talking: at yourself where am I right now")
        );
        assert!(turns.overdue().is_none());
        turns.held.as_mut().unwrap().since = earlier(HOLD_FOR);
        assert_eq!(
            turns.overdue().as_deref(),
            Some("No take a look at yourself where am I right now")
        );
        // A whole sentence meanwhile takes the words with it instead.
        turns.asked(job(&out), "No take a look");
        turns.hearing(&mut out, &companion, 3.0, "at yourself");
        assert_eq!(
            turns.heard(&mut out, "at yourself where am I".into()),
            "No take a look at yourself where am I"
        );
        assert!(turns.overdue().is_none());
    }

    #[test]
    fn a_loud_sound_on_the_phone_cuts_the_reply_only_once_words_follow() {
        let mut out = outputs();
        let companion = Companion::new(Settings::default());
        let mut turns = Turns::default();
        let id = job(&out);
        playing(
            &mut out,
            &mut turns,
            id,
            "You're level 165, with 74 percent.",
        );
        let ends = out.mouth.phone_end;
        // The phone heard something loud and paused: nothing is called off
        // (no cut, so no Job::Cut either), the clip plays on, that much later.
        turns.loud(&mut out);
        assert!(!cancelled(&out, id));
        assert_eq!(cuts(&out), 0);
        assert!(out.mouth.speaking());
        assert_eq!(out.mouth.phone_end, ends + CORROBORATE_FOR);
        // Its own echo, nothing of the player's: still nothing.
        let mut echoing = Companion::new(Settings::default());
        echoing.remember_spoken(1.0, "You're level 165, with 74 percent.");
        assert_eq!(
            turns.hearing(&mut out, &echoing, 1.0, "level 165 percent"),
            None
        );
        assert!(!cancelled(&out, id) && cuts(&out) == 0);
        // The player's words: a talk-over after all.
        assert_eq!(
            turns
                .hearing(&mut out, &companion, 1.5, "wait stop")
                .as_deref(),
            Some("talked over: wait stop")
        );
        assert!(cancelled(&out, id));
        assert_eq!(cuts(&out), 1);
        assert!(turns.loud.is_none());
    }

    #[test]
    fn a_loud_sound_nobody_spoke_over_is_forgotten() {
        let mut out = outputs();
        let companion = Companion::new(Settings::default());
        let mut turns = Turns::default();
        let id = job(&out);
        playing(&mut out, &mut turns, id, "You're level 165.");
        turns.loud(&mut out);
        // Too late to be about that sound: the clip ended meanwhile.
        turns.loud = Some(earlier(CORROBORATE_FOR));
        out.mouth.phone_until = Instant::now();
        assert!(!turns.loud_lately());
        assert_eq!(turns.hearing(&mut out, &companion, 5.0, "wait stop"), None);
        assert!(!cancelled(&out, id));
        assert_eq!(cuts(&out), 0);
    }

    /// The lines on the phone's screen that the call is to say, in order,
    /// with the reading behind each.
    fn said(out: &Outputs) -> Vec<(String, Option<String>)> {
        let hub = out.phone.as_ref().unwrap();
        let state = hub.handle(&phone::http::Request {
            method: "GET".into(),
            path: "/api/state".into(),
            query: vec![("k".into(), "k1".into())],
            headers: Vec::new(),
            body: Vec::new(),
        });
        serde_json::from_slice::<serde_json::Value>(&state.body).unwrap()["messages"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|m| m["speak"] == true)
            .map(|m| {
                (
                    m["text"].as_str().unwrap().to_string(),
                    m["fact"].as_str().map(str::to_string),
                )
            })
            .collect()
    }

    /// The lines on the phone's screen the call is to say however late
    /// they come (a death, a level-up).
    fn urgent(out: &Outputs) -> Vec<String> {
        let hub = out.phone.as_ref().unwrap();
        let state = hub.handle(&phone::http::Request {
            method: "GET".into(),
            path: "/api/state".into(),
            query: vec![("k".into(), "k1".into())],
            headers: Vec::new(),
            body: Vec::new(),
        });
        serde_json::from_slice::<serde_json::Value>(&state.body).unwrap()["messages"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|m| m["urgent"] == true)
            .map(|m| m["text"].as_str().unwrap().to_string())
            .collect()
    }

    fn frame(hp: f32, read: bool, level: u32) -> Observation {
        let gauge = |percent: f32, read: bool| ms::companion::Gauge {
            percent,
            current: None,
            max: None,
            read,
        };
        Observation {
            game: GameView::Seen("MapleStory".into()),
            hp: Some(gauge(hp, read)),
            mp: Some(gauge(40.0, false)),
            exp: None,
            level: Some(level),
            name: None,
            job: None,
        }
    }

    #[test]
    fn on_a_call_the_watcher_speaks_once_per_gap_and_with_a_number() {
        let mut out = outputs();
        out.live = true;
        let mut companion = Companion::new(Settings::default());
        let now = |out: &Outputs| out.start.elapsed().as_secs_f64();
        companion.observe(now(&out), frame(11.0, true, 165));
        // Six beatings in a few seconds: the first goes, with the reading
        // behind it; the rest wait, and only the newest of them.
        for i in 0..6 {
            out.tell(Kind::Warning, &format!("beating {i}"), true, &mut companion);
        }
        assert_eq!(
            said(&out),
            [(
                "beating 0".to_string(),
                Some("HP 11% (read 0 s ago), MP about 40% (estimated 0 s ago)".to_string())
            )]
        );
        // A correction from a look-up waits alongside: a warning does not
        // push it out. Nothing goes before the gap is up.
        out.tell(
            Kind::Info,
            "Actually, Zakum is level 110.",
            true,
            &mut companion,
        );
        out.tell(Kind::Warning, "beating 6", true, &mut companion);
        out.relay_due(&companion);
        assert_eq!(said(&out).len(), 1);
        // The answer to a button is theirs at once, gap or no gap.
        out.tell(Kind::Reply, "Marked.", true, &mut companion);
        assert_eq!(said(&out).len(), 2);
        // The gap up: what waited goes together (one turn for the call),
        // the warning with the reading as it is now — and only the
        // warning: a correction is not about HP, and a call handed "HP
        // 62%" with it said "HP's at 62, and Zakum is level 110".
        out.relay.sent = Some(earlier(RELAY_GAP));
        companion.observe(now(&out), frame(62.0, true, 165));
        out.relay_due(&companion);
        let lines: Vec<String> = said(&out).into_iter().map(|(t, _)| t).collect();
        assert_eq!(
            lines,
            [
                "beating 0",
                "Marked.",
                "Actually, Zakum is level 110.",
                "beating 6"
            ]
        );
        assert_eq!(said(&out)[1].1, None);
        assert_eq!(said(&out)[2].1, None);
        assert_eq!(
            said(&out)[3].1.as_deref(),
            Some("HP 62% (read 0 s ago), MP about 40% (estimated 0 s ago)")
        );
        assert!(out.relay.waiting.is_empty());
        // The gap is closed again: a low-HP line and the coach's word
        // park, and are still parked when the death comes. The death goes
        // at once, alone, and with no reading ("You died" is not about a
        // number): the parked warning was about a bar that no longer
        // matters, the parked news about a fight that is over, and the
        // log says neither was said.
        out.tell(
            Kind::Warning,
            "HP 20 percent. Pot now!",
            true,
            &mut companion,
        );
        out.tell(Kind::Alert, "Rebuff.", true, &mut companion);
        assert_eq!(out.relay.waiting.len(), 2);
        let before = said(&out).len();
        for _ in 0..3 {
            let actions = companion.observe(now(&out), frame(0.0, true, 165));
            out.apply(actions, &mut companion, None);
        }
        assert!(companion.dead());
        let lines = said(&out);
        assert_eq!(lines.len(), before + 1, "{lines:?}");
        let death = lines.last().unwrap().0.clone();
        assert!(death.starts_with("Your HP hit zero"), "{death}");
        assert_eq!(lines.last().unwrap().1, None, "{lines:?}");
        assert!(!lines.iter().any(|(t, _)| t.contains("HP 20 percent")));
        assert!(!lines.iter().any(|(t, _)| t == "Rebuff."));
        assert!(out.relay.waiting.is_empty());
        for dropped in ["HP 20 percent. Pot now!", "Rebuff."] {
            assert!(
                log(&out).contains(&format!("[live] not said, a newer line came: {dropped}")),
                "{}",
                log(&out)
            );
        }
        // The page is told the death is said however late it comes; the
        // warnings before it were not.
        assert_eq!(urgent(&out), std::slice::from_ref(&death));
        assert!(log(&out).contains(&format!("[live] to the call, urgent: {death}\n")));
        // So does a level-up (the number held for a moment, one up), and
        // it carries no reading either.
        let t = now(&out) + 100.0;
        for at in [t, t + 3.5, t + 10.0, t + 13.5] {
            let level = if at < t + 10.0 { 165 } else { 166 };
            let actions = companion.observe(at, frame(80.0, true, level));
            out.apply(actions, &mut companion, None);
        }
        assert_eq!(said(&out).last().unwrap().0, "Level up! You're level 166.");
        assert_eq!(said(&out).last().unwrap().1, None);
        assert_eq!(urgent(&out), [death, "Level up! You're level 166.".into()]);
        // Muted: shown, not said, and nothing waits for the gap.
        let actions = companion.command(t + 20.0, Command::Mute);
        out.apply(actions, &mut companion, None);
        let before = said(&out).len();
        out.tell(Kind::Warning, "beating 7", true, &mut companion);
        assert_eq!((said(&out).len(), out.relay.waiting.len()), (before, 0));
    }

    /// The phone's clip counter.
    fn clip(out: &Outputs) -> u64 {
        let hub = out.phone.as_ref().unwrap();
        let state = hub.handle(&phone::http::Request {
            method: "GET".into(),
            path: "/api/state".into(),
            query: vec![("k".into(), "k1".into())],
            headers: Vec::new(),
            body: Vec::new(),
        });
        serde_json::from_slice::<serde_json::Value>(&state.body).unwrap()["clip"]
            .as_u64()
            .unwrap()
    }

    #[test]
    fn a_line_of_its_own_waits_while_the_player_talks() {
        let mut out = outputs();
        let companion = Companion::new(Settings::default());
        let mut turns = Turns::default();
        let samples = vec![100i16; 2400];
        let piece = |id: u64, kind: Kind, start: bool, end: bool| Piece {
            id,
            kind,
            samples: &samples,
            start,
            end,
        };
        // The answer they are waiting for goes at once.
        let answer = job(&out);
        turns.asked(answer, "what's my level");
        let own = turns.reply_id() != Some(answer);
        assert!(!own);
        out.play_piece(piece(answer, Kind::Reply, true, true), false, &companion);
        assert_eq!(clip(&out), 1);
        // A line of its own while they talk (or while the answer is made): held.
        let heard = Some(Instant::now());
        assert!(!player_quiet(heard, &turns));
        out.play_piece(piece(101, Kind::Info, true, true), true, &companion);
        out.release_clips(player_quiet(heard, &turns));
        assert_eq!((clip(&out), out.mouth.phone_held.len()), (1, 1));
        // Once they are quiet and answered, it goes.
        turns.finished(answer);
        let quiet = Some(earlier(Duration::from_secs(1)));
        assert!(player_quiet(quiet, &turns));
        out.release_clips(player_quiet(quiet, &turns));
        assert_eq!((clip(&out), out.mouth.phone_held.len()), (2, 0));
        // Held too long, it goes anyway; cut, it is dropped.
        out.play_piece(piece(102, Kind::Info, true, true), true, &companion);
        out.mouth.phone_held[0].since = earlier(CLIP_WAITS);
        out.release_clips(false);
        assert_eq!(clip(&out), 3);
        out.play_piece(piece(103, Kind::Info, true, true), true, &companion);
        out.cut();
        assert!(out.mouth.phone_held.is_empty());
        // A warning's clip, held for their turn, is not dropped by their
        // next sentence: the warning is still true. Nor is news (a death,
        // a level-up). They go after their turn, as they would have; the
        // note held with them is dropped.
        out.play_piece(piece(104, Kind::Warning, true, true), true, &companion);
        out.play_piece(piece(105, Kind::Alert, true, true), true, &companion);
        out.play_piece(piece(106, Kind::Info, true, true), true, &companion);
        assert_eq!(out.mouth.phone_held.len(), 3);
        out.cut();
        assert_eq!(
            out.mouth
                .phone_held
                .iter()
                .map(|clip| clip.kind)
                .collect::<Vec<_>>(),
            [Kind::Warning, Kind::Alert],
            "the warning's clip and the news must survive the cut"
        );
        assert_eq!(cuts(&out), 2);
        out.release_clips(true);
        assert_eq!((clip(&out), out.mouth.phone_held.len()), (5, 0));
        // A warning still being made when the cut comes keeps what was
        // made of it: the clip is whole, not its second half.
        out.play_piece(piece(107, Kind::Warning, true, false), false, &companion);
        out.cut();
        out.play_piece(piece(107, Kind::Warning, false, true), false, &companion);
        assert_eq!(clip(&out), 6);
        assert_eq!(clip_bytes(&out, 6), 44 + 2 * 2 * samples.len());
    }

    #[test]
    fn a_cut_hushes_a_reply_and_lets_a_warning_play_out() {
        let mut out = outputs();
        let companion = Companion::new(Settings::default());
        // Replies on the PC's speakers.
        out.phone.as_ref().unwrap().set_voice_on(VoiceOn::Pc);
        let samples = vec![100i16; 24_000];
        let piece = |id: u64, kind: Kind, start: bool, end: bool| Piece {
            id,
            kind,
            samples: &samples,
            start,
            end,
        };
        // A reply under way; the player talks over it: the speakers stop,
        // and the rest of the reply is dropped — hushed on its first half,
        // a reply went on from its second.
        out.play_piece(piece(1, Kind::Reply, true, false), false, &companion);
        assert!(out.mouth.player.speaking());
        out.cut();
        assert!(!out.mouth.player.speaking());
        assert_eq!(out.mouth.hushed, Some(1));
        out.play_piece(piece(1, Kind::Reply, false, true), false, &companion);
        assert!(
            !out.mouth.player.speaking(),
            "the hushed reply's second half must not play"
        );
        // A warning under way is not hushed: it is still true, and plays
        // out, the rest of it too.
        out.play_piece(piece(2, Kind::Warning, true, false), false, &companion);
        out.cut();
        assert!(
            out.mouth.player.speaking(),
            "a warning must play on through the cut"
        );
        let before = out.mouth.player.remaining();
        out.play_piece(piece(2, Kind::Warning, false, true), false, &companion);
        assert!(out.mouth.player.remaining() > before);
        assert_eq!(out.mouth.hushed, Some(1));
        // News (a death, a level-up) the same.
        out.mouth.player.stop();
        out.play_piece(piece(3, Kind::Alert, true, true), false, &companion);
        out.cut();
        assert!(
            out.mouth.player.speaking(),
            "news must play on through the cut"
        );
        // A note of its own (a hello) is chat: hushed.
        out.mouth.player.stop();
        out.play_piece(piece(4, Kind::Info, true, false), false, &companion);
        out.cut();
        assert!(!out.mouth.player.speaking());
        assert_eq!(out.mouth.hushed, Some(4));
        // On the phone the same: a reply hushed on its first half does
        // not come out as its second half once its last piece arrives.
        out.phone.as_ref().unwrap().set_voice_on(VoiceOn::Phone);
        let clips = clip(&out);
        out.play_piece(piece(5, Kind::Reply, true, false), false, &companion);
        out.cut();
        out.play_piece(piece(5, Kind::Reply, false, true), false, &companion);
        assert_eq!(
            clip(&out),
            clips,
            "no clip of the hushed reply's second half"
        );
        // The next line is a new one: heard in full.
        out.play_piece(piece(6, Kind::Reply, true, true), false, &companion);
        assert_eq!(clip(&out), clips + 1);
    }

    #[test]
    fn the_phone_is_said_hello_to_once_per_visit_by_whoever_will_talk() {
        let mut out = outputs();
        let mut companion = Companion::new(Settings::default());
        let hello =
            |out: &mut Outputs, companion: &mut Companion, live: bool, away: Option<Duration>| {
                out.hello(companion, false, "iPhone Safari", live, away, None)
            };
        let shown = |out: &Outputs| {
            let hub = out.phone.as_ref().unwrap();
            let state = hub.handle(&phone::http::Request {
                method: "GET".into(),
                path: "/api/state".into(),
                query: vec![("k".into(), "k1".into())],
                headers: Vec::new(),
                body: Vec::new(),
            });
            serde_json::from_slice::<serde_json::Value>(&state.body).unwrap()["messages"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|m| m["text"] == "Connected to MapleSyrup on this iPhone.")
                .count()
        };
        // No key (no call to come): a hello of its own, said as a clip
        // (one job for the worker, remembered as its own words).
        let first = job(&out);
        assert_eq!(hello(&mut out, &mut companion, true, None), Hello::Clip);
        assert_eq!(shown(&out), 1);
        assert!(
            companion
                .own_words(1.0, "hey I'm here just talk to me", 0)
                .is_none(),
            "the hello was not said"
        );
        // The page reloaded a moment later (the same visit): "Connected"
        // on the screen, no second hello (no second job).
        assert_eq!(
            hello(&mut out, &mut companion, true, Some(Duration::from_secs(2))),
            Hello::Quiet
        );
        assert_eq!(shown(&out), 2);
        assert_eq!(job(&out), first + 2);
        // Gone for half an hour: a visit of its own, hello again.
        let before = job(&out);
        assert_eq!(
            hello(&mut out, &mut companion, true, Some(HELLO_AGAIN)),
            Hello::Clip
        );
        assert_eq!(job(&out), before + 2);
        // Knowing the player, the hello picks up from last time (a job of
        // its own).
        let before = job(&out);
        assert_eq!(
            out.hello(
                &mut companion,
                true,
                "iPhone",
                true,
                Some(HELLO_AGAIN),
                None
            ),
            Hello::Clip
        );
        assert_eq!(job(&out), before + 2);
        // With a key, and the page's toggle on: the call about to open says
        // hello, and nothing is said meanwhile; the page is told so.
        out.live_ok = true;
        let before = job(&out);
        assert!(!out.call_greets);
        assert_eq!(
            hello(&mut out, &mut companion, true, Some(HELLO_AGAIN)),
            Hello::Call
        );
        assert!(out.call_greets);
        assert_eq!(job(&out), before + 1);
        // The toggle off (the owner's phone connected to silence): a clip.
        out.call_greets = false;
        assert_eq!(
            hello(&mut out, &mut companion, false, Some(HELLO_AGAIN)),
            Hello::Clip
        );
        assert!(!out.call_greets);
        // Replies on the PC: no call will open, whatever the toggle.
        out.phone.as_ref().unwrap().set_voice_on(VoiceOn::Pc);
        assert_eq!(
            hello(&mut out, &mut companion, true, Some(HELLO_AGAIN)),
            Hello::Clip
        );
        out.phone.as_ref().unwrap().set_voice_on(VoiceOn::Phone);
        // Only after "syrup": the page opens no call either.
        companion.set_always_listen(false);
        assert_eq!(
            hello(&mut out, &mut companion, true, Some(HELLO_AGAIN)),
            Hello::Clip
        );
        companion.set_always_listen(true);
        // A new session (the PC started again) says hello even though the
        // link saw the phone a moment ago.
        let mut fresh = outputs();
        assert_eq!(
            hello(
                &mut fresh,
                &mut companion,
                true,
                Some(Duration::from_millis(30))
            ),
            Hello::Clip
        );
        assert!(log(&out).contains("[info] iPhone connected: back from a reload, no second hello"));
        assert!(log(&out).contains("[info] iPhone connected: the call says hello when it opens"));
    }

    /// The size of the phone's clip `seq`, as it would fetch it.
    fn clip_bytes(out: &Outputs, seq: u64) -> usize {
        let hub = out.phone.as_ref().unwrap();
        let clip = hub.handle(&phone::http::Request {
            method: "GET".into(),
            path: "/api/clip".into(),
            query: vec![("k".into(), "k1".into()), ("seq".into(), seq.to_string())],
            headers: Vec::new(),
            body: Vec::new(),
        });
        assert_eq!(clip.status, 200);
        clip.body.len()
    }
}
