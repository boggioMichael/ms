//! MapleSyrup: the MapleStory companion, as one program you start.
//!
//! It finds the MapleStory window and watches it through the vision engine,
//! speaks up when HP or MP runs low or you level up, and answers when asked
//! — by voice through your phone, or from the phone's buttons. The phone
//! joins by scanning the QR code in this window: its page is the
//! companion's microphone and a second screen.
//!
//! ```text
//!  MapleStory window ─ syrup capture ─▶ vision engine ─▶ Observation ─┐
//!                                                                      ▼
//!  phone page ── heard / buttons / audio ──▶ phone link ──▶  Companion ──▶ voice, console,
//!       ▲                                        │                       phone, session files
//!       └──────────── status and replies ◀───────┘
//! ```

mod selftest;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use image::RgbaImage;
use ms::app::panel;
use ms::app::screen::{self, LogLine};
use ms::app::session::{self, Session};
use ms::capture::{Captured, GameCapture};
use ms::companion::{Action, Command, Companion, GameView, Kind, Observation, Settings};
use ms::observe::frame_result::{FrameTimings, VisionFrameResult};
use ms::observe::preview::Preview;
use ms::phone::{self, Hub, Inbound, VoiceOn, qr, tls, tunnel};
use ms::platform::overlay::{self, Overlay};
use ms::platform::{self, voice::Voice};
use ms::util::timing::FPSCounter;
use ms::vision::snapshot::PerceptionPipeline;
use serde_json::json;

const USAGE: &str = "\
MapleSyrup — the MapleStory companion.

Start it, start MapleStory (either order), and scan the QR code with your
phone. Say \"syrup\" and then a command: status, hp, mp, exp, rate, level,
time, mark, mute, unmute, help. Close this window or press Ctrl+C to stop.

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
  --no-voice            never speak on the PC
  --rate N              the PC voice's speed, -10 to 10 (default 1)
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
";

struct Options {
    window: Option<String>,
    input: Option<PathBuf>,
    phone: bool,
    tunnel: bool,
    port: u16,
    record_mic: bool,
    replies: VoiceOn,
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

/// Capture and the vision engine, on a thread of their own.
fn watch(
    mut source: Source,
    slot: Arc<TickSlot>,
    running: Arc<AtomicBool>,
    start: Instant,
    fps: f64,
) {
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
                let vision = vision_start.elapsed();
                let obs = Observation::from_world(&title, &world);
                let now = Instant::now();
                let interval = now.duration_since(previous);
                previous = now;
                let fps = counter.add_frame_seconds(interval.as_secs_f64());
                let frame = VisionFrameResult {
                    frame_id,
                    elapsed_ms: start.elapsed().as_millis() as u64,
                    source: title,
                    image: Arc::new(image),
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
                });
                if let Some(rest) = period.checked_sub(began.elapsed()) {
                    std::thread::sleep(rest);
                }
            }
            Captured::NotFound => {
                slot.put(Tick {
                    at: start.elapsed().as_secs_f64(),
                    obs: Observation::unseen(GameView::NotFound),
                    frame: None,
                });
                std::thread::sleep(Duration::from_millis(500));
            }
            Captured::Unavailable(why) => {
                slot.put(Tick {
                    at: start.elapsed().as_secs_f64(),
                    obs: Observation::unseen(GameView::Unavailable(why)),
                    frame: None,
                });
                std::thread::sleep(Duration::from_millis(500));
            }
        }
    }
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

/// Everything an action can touch.
struct Outputs {
    voice: Option<Voice>,
    phone: Option<Arc<Hub>>,
    session: Session,
    log: Vec<LogLine>,
    plain: bool,
    start: Instant,
}

impl Outputs {
    fn voice_on(&self, fallback: VoiceOn) -> VoiceOn {
        self.phone
            .as_ref()
            .map(|h| h.voice_on())
            .unwrap_or(fallback)
    }

    fn apply(
        &mut self,
        actions: Vec<Action>,
        companion: &Companion,
        frame: Option<Arc<RgbaImage>>,
        replies: VoiceOn,
    ) {
        let voice_on = self.voice_on(replies);
        for action in actions {
            match action {
                Action::Say(say) => {
                    let who = match say.kind {
                        Kind::Heard => "heard",
                        Kind::Alert => "alert",
                        Kind::Reply => "reply",
                        Kind::Info => "info",
                    };
                    self.session.line(who, &say.text);
                    if let Some(hub) = &self.phone {
                        hub.post(say.kind, &say.text, say.speak);
                    }
                    if say.speak
                        && !companion.muted()
                        && voice_on.pc()
                        && let Some(voice) = &self.voice
                    {
                        voice.say(&say.text);
                    }
                    self.push(say.kind, say.text);
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
                Action::SetMuted(true) => {
                    if let Some(voice) = &self.voice {
                        voice.hush();
                    }
                }
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

fn run(options: Options) -> Result<(), String> {
    let console = platform::init("MapleSyrup (close this window or press Ctrl+C to stop)");
    let ansi = console.ansi && !options.plain;
    let start = Instant::now();
    let settings_dir = tls::settings_dir();
    let session_dir = session::new_dir(&session::sessions_base(&settings_dir));
    let session = Session::open(session_dir.clone());

    let (voice, voice_label) = if options.voice {
        match Voice::start(options.rate) {
            Ok(voice) => (Some(voice), "PC voice".to_string()),
            Err(e) => (None, format!("no PC voice ({e})")),
        }
    } else {
        (None, "PC voice off".to_string())
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
        "{b}Maple{s}Syrup{r}{b} · the MapleStory companion{r} {d}v{}{r}",
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
    let worker = {
        let (slot, running, fps) = (Arc::clone(&slot), Arc::clone(&running), options.fps);
        std::thread::Builder::new()
            .name("vision".into())
            .spawn(move || watch(source, slot, running, start, fps))
            .map_err(|e| e.to_string())?
    };

    let mut companion = Companion::new(Settings {
        hp_low: options.hp_low,
        hp_rearm: (options.hp_low + 15.0).min(95.0),
        mp_low: options.mp_low,
        mp_rearm: (options.mp_low + 15.0).min(95.0),
        ..Settings::default()
    });
    let mut out = Outputs {
        voice,
        phone: phone.as_ref().map(|p| Arc::clone(&p.hub)),
        session,
        log: Vec::new(),
        plain: !ansi,
        start,
    };
    out.apply(companion.hello(), &companion, None, options.replies);

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

    let mut preview: Option<Preview> = None;
    let mut preview_failed = false;
    let mut latest_image: Option<Arc<RgbaImage>> = None;
    let mut frame_size = None;
    let mut fps = 0.0;
    let mut drawn_lines = 0usize;
    let mut last_draw = Instant::now() - Duration::from_secs(1);
    let mut last_plain_status = Instant::now();
    let block_height = qr_lines.len().max(screen::HEIGHT);

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
            out.apply(actions, &companion, latest_image.clone(), options.replies);
        } else if let Some(p) = preview.as_mut() {
            p.pump();
        }
        if let Some(window) = &panel_window {
            window.pump();
        }

        if let Some(hub) = out.phone.clone() {
            for inbound in hub.take_inbox() {
                let actions = match inbound {
                    Inbound::Heard(text) => companion.heard(now, &text),
                    Inbound::Command(word) => match Command::from_word(&word) {
                        Some(command) => companion.command(now, command),
                        None => Vec::new(),
                    },
                    Inbound::Hello(agent) => {
                        let device = device_of(&agent);
                        out.push(Kind::Info, format!("{device} connected"));
                        hub.post(
                            Kind::Info,
                            &format!("Connected to MapleSyrup on this {device}."),
                            false,
                        );
                        if out.voice_on(options.replies).pc()
                            && !companion.muted()
                            && let Some(voice) = &out.voice
                        {
                            voice.say("Phone connected.");
                        }
                        Vec::new()
                    }
                    Inbound::Voice(on) => {
                        let place = match on {
                            VoiceOn::Pc => "the PC",
                            VoiceOn::Phone => "the phone",
                            VoiceOn::Both => "the PC and the phone",
                            VoiceOn::Off => "nowhere (written only)",
                        };
                        out.push(Kind::Info, format!("replies are now spoken on {place}"));
                        if !on.pc()
                            && let Some(voice) = &out.voice
                        {
                            voice.hush();
                        }
                        Vec::new()
                    }
                };
                out.apply(actions, &companion, latest_image.clone(), options.replies);
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
                }));
            }
            if let Some(window) = panel_window.as_mut() {
                let obs = companion.last();
                let area = match obs.map(|o| &o.game) {
                    Some(GameView::Seen(title)) => overlay::game_area(title),
                    _ => None,
                };
                match area {
                    Some(area) if area.foreground && area.width > 200 && area.height > 150 => {
                        let scale = panel::scale_for(area.height);
                        let (pw, _) = panel::size(scale);
                        let margin = (12.0 * scale) as i32;
                        let recent = out
                            .log
                            .iter()
                            .rev()
                            .find(|l| matches!(l.kind, Kind::Reply | Kind::Alert))
                            .filter(|l| l.at.elapsed() < Duration::from_secs(12))
                            .map(|l| l.text.as_str());
                        let content = panel::Content {
                            obs,
                            exp_per_hour: progress.exp_per_hour,
                            phone_connected: summary.as_ref().map(|s| s.connected),
                            speaking: summary.as_ref().is_some_and(|s| s.mic.speaking),
                            muted: companion.muted(),
                            last_line: recent,
                        };
                        let painted = panel::paint(&content, scale);
                        let at = (
                            area.left + area.width - pw as i32 - margin,
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
            if ansi {
                let view = screen::View {
                    obs: companion.last(),
                    fps,
                    frame_size,
                    progress: &progress,
                    phone: summary.as_ref(),
                    voice: &voice_label,
                    voice_on: out.voice_on(options.replies),
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
    let _ = worker.join();
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
