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
    if !reply.starts_with("Say syrup") {
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
