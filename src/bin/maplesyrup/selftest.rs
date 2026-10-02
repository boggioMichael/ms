//! `MapleSyrup --self-test`: checks the parts of MapleSyrup that depend on
//! this PC, one by one, and says plainly what works. CI runs it on a fresh
//! Windows machine after every build.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use ms::capture::{Captured, GameCapture};
use ms::companion::{Action, Companion, Kind, Observation, Settings};
use ms::phone::{self, Hub, Inbound, VoiceOn, client, qr, tls};
use ms::platform::{self, voice::Voice};
use ms::vision::snapshot::PerceptionPipeline;

struct Report {
    failed: usize,
}

impl Report {
    fn ok(&mut self, what: &str, detail: impl AsRef<str>) {
        println!("  ok    {what}: {}", detail.as_ref());
    }
    fn note(&mut self, what: &str, detail: impl AsRef<str>) {
        println!("  note  {what}: {}", detail.as_ref());
    }
    fn fail(&mut self, what: &str, detail: impl AsRef<str>) {
        println!("  FAIL  {what}: {}", detail.as_ref());
        self.failed += 1;
    }
}

pub fn run() -> i32 {
    let mut r = Report { failed: 0 };
    println!("MapleSyrup self-test (v{})", env!("CARGO_PKG_VERSION"));

    let console = platform::init("MapleSyrup self-test");
    r.ok(
        "console",
        format!(
            "escape codes {}, DPI aware {}",
            if console.ansi {
                "on"
            } else {
                "off (output is not a console)"
            },
            console.dpi_aware
        ),
    );

    // The vision engine on the committed screenshot, when it is here.
    let fixture = Path::new("resources/maplestory.png");
    if fixture.exists() {
        match image::open(fixture) {
            Ok(img) => {
                let img = img.to_rgba8();
                let started = Instant::now();
                let mut pipeline = PerceptionPipeline::new();
                let world = pipeline.detect(&img);
                let obs = Observation::from_world("fixture", &world);
                let took = started.elapsed();
                let seen = [obs.hp.is_some(), obs.mp.is_some(), obs.exp.is_some()];
                if seen.iter().all(|s| *s) {
                    r.ok(
                        "vision",
                        format!(
                            "HP {:.0}%, MP {:.0}%, EXP {:.1}% found on the screenshot in {:.0} ms",
                            obs.hp.map(|g| g.percent).unwrap_or_default(),
                            obs.mp.map(|g| g.percent).unwrap_or_default(),
                            obs.exp.map(|g| g.percent).unwrap_or_default(),
                            took.as_secs_f64() * 1000.0
                        ),
                    );
                } else {
                    r.fail("vision", format!("bars found (HP, MP, EXP): {seen:?}"));
                }
                let mut companion = Companion::new(Settings::default());
                companion.observe(0.0, obs);
                let said = said(companion.heard(1.0, "syrup status"));
                if said.iter().any(|l| l.contains("HP")) {
                    r.ok(
                        "companion",
                        format!("\"syrup status\" → {}", said.join(" ")),
                    );
                } else {
                    r.fail("companion", format!("\"syrup status\" → {said:?}"));
                }
            }
            Err(e) => r.fail(
                "vision",
                format!("could not open {}: {e}", fixture.display()),
            ),
        }
    } else {
        r.note("vision", "resources/maplestory.png is not here; skipped");
    }

    // Text on the HUD (level, name, job, the printed HP/MP numbers).
    match ms::vision::ocr::engine() {
        Some(ms::vision::ocr::Engine::Tesseract) => r.ok("text reading", "Tesseract"),
        Some(ms::vision::ocr::Engine::Windows) => r.ok(
            "text reading",
            "the OCR engine built into Windows (Tesseract is not installed)",
        ),
        None => r.note(
            "text reading",
            "no OCR engine: HP/MP/EXP come from the bars, level, name and job stay unknown",
        ),
    }

    // The conversation and the natural voice (OpenAI), when there is a key.
    let settings = tls::settings_dir();
    match ms::ai::load_key(&settings) {
        Some(key) => {
            let client = ms::ai::OpenAi::new(&key, "https://api.openai.com/v1", "cedar", None);
            match client.check() {
                Ok(()) => match client.respond(
                    "Reply with exactly: ready",
                    &[ms::ai::openai::Turn {
                        role: "user",
                        text: "Are you there?".into(),
                    }],
                ) {
                    Ok(text) => r.ok(
                        "OpenAI",
                        format!(
                            "the key works; {} answered \"{text}\"",
                            client.model().unwrap_or_default()
                        ),
                    ),
                    Err(e) => r.fail("OpenAI", e.to_string()),
                },
                Err(e) => r.fail("OpenAI", e.to_string()),
            }
        }
        None => r.note("OpenAI", "no key: simple answers and the Windows voice"),
    }

    // The phone link: a certificate, TLS, the API, and the companion behind it.
    let dir = std::env::temp_dir().join(format!("maplesyrup-selftest-{}", tls::random_hex(4)));
    match phone_link(&dir) {
        Ok(detail) => r.ok("phone link", detail),
        Err(e) => r.fail("phone link", e),
    }
    let _ = std::fs::remove_dir_all(&dir);
    match phone::net::lan_ipv4() {
        Some(ip) => r.ok("network", format!("the phone would open https://{ip}:8443")),
        None => r.note(
            "network",
            "no local network address (the tunnel would still work)",
        ),
    }
    match qr::console("https://192.168.1.100:8443/?k=0123456789abcdef") {
        Some(lines) => r.ok("QR code", format!("{} lines tall", lines.len())),
        None => r.fail("QR code", "could not encode a link"),
    }

    // The voice. A PC without voices installed is not a failure of MapleSyrup.
    match Voice::start(1) {
        Ok(voice) => {
            voice.say("MapleSyrup self test.");
            std::thread::sleep(Duration::from_millis(300));
            r.ok("voice", "the Windows voice started (a line was spoken)");
        }
        Err(e) => r.note("voice", e),
    }

    // Capture: whatever windows are open on this machine.
    let titles = syrup::capture::list_windows();
    match GameCapture::auto().capture() {
        Captured::Frame { title, image } => r.ok(
            "capture",
            format!(
                "captured \"{title}\" ({}x{})",
                image.width(),
                image.height()
            ),
        ),
        Captured::NotFound => r.ok(
            "capture",
            format!("{} windows listed, no MapleStory among them", titles.len()),
        ),
        Captured::Unavailable(why) => r.note("capture", why),
    }

    println!(
        "{}",
        if r.failed == 0 {
            "All checks passed.".to_string()
        } else {
            format!("{} check(s) failed.", r.failed)
        }
    );
    if r.failed == 0 { 0 } else { 1 }
}

fn said(actions: Vec<Action>) -> Vec<String> {
    actions
        .into_iter()
        .filter_map(|a| match a {
            Action::Say(s) if s.kind != Kind::Heard => Some(s.text),
            _ => None,
        })
        .collect()
}

fn phone_link(dir: &Path) -> Result<String, String> {
    let identity = tls::load_or_create(dir, &["127.0.0.1".into(), "localhost".into()])?;
    let config = tls::server_config(&identity)?;
    let hub = Hub::new("selftest".into(), None, VoiceOn::Pc);
    let port = phone::serve_tls(Arc::clone(&hub), config, 48443)?;
    let cert = &identity.cert_der;

    let page = client::request(port, cert, "GET", "/?k=selftest", b"")?;
    if page.status != 200 || !page.text().contains("MapleSyrup") {
        return Err(format!("the page answered {}", page.status));
    }
    let heard = client::request(
        port,
        cert,
        "POST",
        "/api/heard?k=selftest",
        br#"{"text":"syrup help"}"#,
    )?;
    if heard.status != 200 {
        return Err(format!("/api/heard answered {}", heard.status));
    }
    let inbox = hub.take_inbox();
    let Some(Inbound::Heard(text)) = inbox.first() else {
        return Err(format!("nothing reached the companion: {inbox:?}"));
    };
    let mut companion = Companion::new(Settings::default());
    for line in said(companion.heard(0.0, text)) {
        hub.post(Kind::Reply, &line, true);
    }
    let state = client::request(port, cert, "GET", "/api/state?k=selftest&since=0", b"")?;
    let json: serde_json::Value = serde_json::from_slice(&state.body).map_err(|e| e.to_string())?;
    let reply = json["messages"][0]["text"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    if !(reply.starts_with("Just talk to me") || reply.starts_with("Say syrup")) {
        return Err(format!("the phone would have shown {reply:?}"));
    }
    let redirect = client::request_plain(port, "GET", "/", b"")?;
    if redirect.status != 301 {
        return Err(format!(
            "plain http on the port answered {}",
            redirect.status
        ));
    }
    Ok(format!(
        "HTTPS on port {port} with a certificate made here; \"syrup help\" from the phone → \"{reply}\""
    ))
}

/// `MapleSyrup --record-test`: records a few seconds of the screen while a
/// window over it turns from black to white and a tone plays at the same
/// moment, then checks the file: a picture thirty frames a second, the
/// sound, and the flash and the tone together. CI runs it on Windows.
pub fn record_test() -> i32 {
    use ms::app::recorder::{self, Picture, Recorder};
    use ms::phone::Recording;

    let mut r = Report { failed: 0 };
    println!("MapleSyrup recording test (v{})", env!("CARGO_PKG_VERSION"));
    let _ = platform::init("MapleSyrup recording test");
    let settings = tls::settings_dir();
    let ffmpeg = match recorder::find_ffmpeg(&settings) {
        Some(found) => {
            r.ok("ffmpeg", found.display().to_string());
            found
        }
        None => {
            println!("  ...   downloading ffmpeg (once, about 150 MB)");
            match recorder::download_ffmpeg(&settings) {
                Ok(found) => {
                    r.ok("ffmpeg", format!("downloaded to {}", found.display()));
                    found
                }
                Err(e) => {
                    r.fail("ffmpeg", e);
                    return 1;
                }
            }
        }
    };
    // A folder named in Hebrew, like a user folder can be.
    let dir = std::env::temp_dir().join(format!(
        "maplesyrup-record-test-מבחן-{}",
        std::process::id()
    ));
    let file = dir.join("record-test.mp4");
    let mut screen = flash::Screen::black();
    if let Err(e) = &screen {
        r.note("flash", format!("no window to flash ({e})"));
    }
    let picture = if cfg!(windows) || std::env::var_os("DISPLAY").is_some() {
        Picture::Screen
    } else {
        Picture::Test
    };
    let recorder = match Recorder::start(&ffmpeg, picture, &file) {
        Ok(recorder) => recorder,
        Err(e) => {
            r.fail("recording", e);
            if let Ok(log) = std::fs::read_to_string(file.with_extension("log")) {
                println!("ffmpeg said:\n{log}");
            }
            return 1;
        }
    };
    r.ok(
        "recording",
        format!(
            "the screen through {}, {}",
            recorder.grab,
            match &recorder.pc_sound {
                Ok(()) => "with the PC's sound".to_string(),
                Err(e) => format!("without the PC's sound ({e})"),
            }
        ),
    );
    let taps = recorder.taps();
    let wait = |screen: &Result<flash::Screen, String>, seconds: f64| {
        let until = Instant::now() + Duration::from_secs_f64(seconds);
        while Instant::now() < until {
            if let Ok(screen) = screen {
                screen.pump();
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    };
    wait(&screen, 2.5);
    // White, and the tone, at once.
    if let Ok(screen) = screen.as_mut()
        && let Err(e) = screen.white()
    {
        r.note("flash", e);
    }
    taps.played(48_000, &recorder::beep(0.5), 0.0);
    wait(&screen, 1.5);
    drop(screen);
    std::thread::sleep(Duration::from_millis(500));
    let finished = recorder.stop();
    let file = match finished {
        Ok(file) => file,
        Err(e) => {
            r.fail("saving", e);
            return 1;
        }
    };
    match recorder::measure(&ffmpeg, &file) {
        Err(e) => r.fail("the file", e),
        Ok(m) => {
            if m.has_video && m.has_audio {
                r.ok(
                    "the file",
                    format!("{:.1} s with a picture and sound", m.duration),
                );
            } else {
                r.fail(
                    "the file",
                    format!("picture {}, sound {}", m.has_video, m.has_audio),
                );
            }
            let fps = m.frames as f64 / m.duration.max(0.1);
            if (25.0..=35.0).contains(&fps) {
                r.ok("frames", format!("{} ({fps:.1} a second)", m.frames));
            } else {
                r.fail("frames", format!("{} in {:.1} s", m.frames, m.duration));
            }
            match (m.bright_at, m.loud_at) {
                (Some(bright), Some(loud)) => {
                    let off = loud - bright;
                    if off.abs() < 0.1 {
                        r.ok(
                            "in sync",
                            format!("the tone {:+.0} ms from the flash", off * 1000.0),
                        );
                    } else {
                        r.fail(
                            "in sync",
                            format!("the tone {:+.0} ms from the flash", off * 1000.0),
                        );
                    }
                }
                (None, Some(_)) => r.note(
                    "in sync",
                    "the flash was not seen in the recording (no desktop to show it on?)",
                ),
                (_, None) => r.fail("in sync", "the tone is not in the recording"),
            }
        }
    }
    if r.failed == 0 {
        let _ = std::fs::remove_dir_all(&dir);
        println!("Recording works.");
        0
    } else {
        println!("Kept for a look: {}", file.display());
        1
    }
}

/// A window over the whole screen, black, then white (Windows).
#[cfg(windows)]
mod flash {
    use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM};
    use windows::Win32::Graphics::Dwm::DwmFlush;
    use windows::Win32::Graphics::Gdi::{
        BLACK_BRUSH, FillRect, GET_STOCK_OBJECT_FLAGS, GdiFlush, GetDC, GetStockObject, HBRUSH,
        ReleaseDC, UpdateWindow, WHITE_BRUSH,
    };
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GCLP_HBRBACKGROUND,
        GetClientRect, GetSystemMetrics, MSG, PM_REMOVE, PeekMessageW, RegisterClassExW,
        SM_CXSCREEN, SM_CYSCREEN, SW_SHOW, SetClassLongPtrW, ShowWindow, TranslateMessage,
        WNDCLASSEXW, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
    };
    use windows::core::{PCWSTR, w};

    unsafe extern "system" fn window_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
    }

    fn window(class: PCWSTR, brush: GET_STOCK_OBJECT_FLAGS) -> Result<HWND, String> {
        unsafe {
            let instance: HINSTANCE = GetModuleHandleW(PCWSTR::null())
                .map_err(|e| e.to_string())?
                .into();
            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(window_proc),
                hInstance: instance,
                lpszClassName: class,
                hbrBackground: HBRUSH(GetStockObject(brush).0),
                ..Default::default()
            };
            RegisterClassExW(&wc);
            let (width, height) = (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN));
            let hwnd = CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
                class,
                w!("MapleSyrup recording test"),
                WS_POPUP,
                0,
                0,
                width,
                height,
                None,
                None,
                Some(instance),
                None,
            )
            .map_err(|e| e.to_string())?;
            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = UpdateWindow(hwnd);
            Ok(hwnd)
        }
    }

    pub struct Screen {
        windows: Vec<HWND>,
    }

    impl Screen {
        pub fn black() -> Result<Screen, String> {
            Ok(Screen {
                windows: vec![window(w!("MapleSyrupTestFlash"), BLACK_BRUSH)?],
            })
        }

        /// White at once (painted over, no window opening animation), and
        /// on the screen when this returns.
        pub fn white(&mut self) -> Result<(), String> {
            let hwnd = *self.windows.first().ok_or("no window")?;
            unsafe {
                let white = HBRUSH(GetStockObject(WHITE_BRUSH).0);
                SetClassLongPtrW(hwnd, GCLP_HBRBACKGROUND, white.0 as isize);
                let mut rect = RECT::default();
                GetClientRect(hwnd, &mut rect).map_err(|e| e.to_string())?;
                let dc = GetDC(Some(hwnd));
                FillRect(dc, &rect, white);
                ReleaseDC(Some(hwnd), dc);
                let _ = GdiFlush();
                // Until the screen has been put together with it.
                let _ = DwmFlush();
            }
            Ok(())
        }

        pub fn pump(&self) {
            unsafe {
                let mut msg = MSG::default();
                while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                    let _ = TranslateMessage(&msg);
                    let _ = DispatchMessageW(&msg);
                }
            }
        }
    }

    impl Drop for Screen {
        fn drop(&mut self) {
            for hwnd in self.windows.drain(..) {
                unsafe {
                    let _ = DestroyWindow(hwnd);
                }
            }
        }
    }
}

#[cfg(not(windows))]
mod flash {
    pub struct Screen;

    impl Screen {
        pub fn black() -> Result<Screen, String> {
            Err("only on Windows".into())
        }
        pub fn white(&mut self) -> Result<(), String> {
            Ok(())
        }
        pub fn pump(&self) {}
    }
}
