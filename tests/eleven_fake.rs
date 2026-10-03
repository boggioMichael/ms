//! ElevenLabs voices against a stand-in server on this machine, through the
//! same `curl` the real calls use: the voices on the account, speech
//! streamed as it is made, a model the account can't use skipped from then
//! on, voice settings a model won't take dropped, Hebrew only to the model
//! that speaks it, and OpenAI's voice standing in (said once) whenever
//! ElevenLabs can't, with ElevenLabs resting a while when it keeps failing.

use std::io::Write;
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ms::ai::eleven::{Eleven, default_voice};
use ms::ai::{AiError, Brain, Brains, Done, Job, OpenAi};
use ms::phone::http::{Conn, Response};
use serde_json::{Value, json};

const KEY: &str = "sk_0123456789abcdef0123456789abcdef0123456789abcdef";
const OPENAI_KEY: &str = "sk-test-key-0123456789abcdef";

/// ElevenLabs's voice is all 7s, OpenAI's all 3s, to tell them apart.
const ELEVEN_SAMPLE: i16 = 7;
const OPENAI_SAMPLE: i16 = 3;

/// A fake ElevenLabs (and OpenAI's speech, to stand in). The voice says how
/// it behaves: `v-ok` speaks with every model, `v-nov4` has no Eleven v4,
/// `v-plain` won't take voice settings, `v-down` always fails. Every request
/// is recorded.
fn fake() -> (String, Arc<Mutex<Vec<Value>>>) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&seen);
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let log = Arc::clone(&log);
            std::thread::spawn(move || {
                let mut conn = Conn::new(stream);
                while let Ok(Some(req)) = conn.read_request(1 << 20) {
                    let body: Value = serde_json::from_slice(&req.body).unwrap_or(Value::Null);
                    let key = req.header("xi-api-key").unwrap_or_default().to_string();
                    let bearer = req.header("authorization").unwrap_or_default().to_string();
                    log.lock().unwrap().push(json!({
                        "path": req.path,
                        "format": req.param("output_format"),
                        "body": body,
                    }));
                    let voice = req
                        .path
                        .strip_prefix("/v1/text-to-speech/")
                        .and_then(|p| p.strip_suffix("/stream"))
                        .map(String::from);
                    let model = body["model_id"].as_str().unwrap_or_default();
                    let response = match (req.path.as_str(), voice.as_deref()) {
                        ("/v1/audio/speech", _) if bearer == format!("Bearer {OPENAI_KEY}") => {
                            let pcm: Vec<u8> = std::iter::repeat_n(OPENAI_SAMPLE, 2400)
                                .flat_map(i16::to_le_bytes)
                                .collect();
                            Response::new(200, "application/octet-stream", pcm)
                        }
                        (_, _) if req.path != "/v1/audio/speech" && key != KEY => Response::json(
                            401,
                            &json!({"detail": {"status": "quota_exceeded", "message": "This request exceeds your quota of 10000."}}),
                        ),
                        ("/v1/voices", _) => Response::json(
                            200,
                            &json!({"voices": [
                                {"voice_id": "v-ok", "name": "Zed", "labels": {"accent": "american", "age": "young", "descriptive": "energetic", "use_case": "social_media"}},
                                {"voice_id": "v-calm", "name": "anna", "labels": {"age": "middle_aged", "descriptive": "calm"}},
                                {"voice_id": "v-broken"},
                            ]}),
                        ),
                        (_, Some("v-nov4")) if model == "eleven_v4_turbo" => Response::json(
                            400,
                            &json!({"detail": {"status": "model_not_found", "message": "A model with model ID eleven_v4_turbo does not exist or is not available to you."}}),
                        ),
                        (_, Some("v-plain")) if body.get("voice_settings").is_some() => {
                            Response::json(
                                422,
                                &json!({"detail": [{"loc": ["body", "voice_settings", "speed"], "msg": "Extra inputs are not permitted", "type": "extra_forbidden"}]}),
                            )
                        }
                        (_, Some("v-down")) => {
                            Response::json(500, &json!({"detail": "Internal Server Error"}))
                        }
                        (_, Some(_)) => {
                            // Sent as it is made, in odd-sized pieces (a
                            // sample split between two of them).
                            let out = conn.get_mut();
                            let _ = out.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: audio/pcm\r\nTransfer-Encoding: chunked\r\n\r\n");
                            let pcm: Vec<u8> = std::iter::repeat_n(ELEVEN_SAMPLE, 4800)
                                .flat_map(i16::to_le_bytes)
                                .collect();
                            for piece in pcm.chunks(3001) {
                                let _ = write!(out, "{:x}\r\n", piece.len());
                                let _ = out.write_all(piece);
                                let _ = out.write_all(b"\r\n");
                                let _ = out.flush();
                                std::thread::sleep(Duration::from_millis(20));
                            }
                            let _ = out.write_all(b"0\r\n\r\n");
                            let _ = out.flush();
                            continue;
                        }
                        _ => Response::json(404, &json!({"detail": "Not Found"})),
                    };
                    if conn.write_response(&response, !req.wants_close()).is_err() {
                        return;
                    }
                }
            });
        }
    });
    (format!("http://127.0.0.1:{port}/v1"), seen)
}

fn have_curl() -> bool {
    std::process::Command::new(if cfg!(windows) { "curl.exe" } else { "curl" })
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

/// The models asked for since `from`, in order.
fn models_asked(seen: &Mutex<Vec<Value>>, from: usize) -> Vec<String> {
    seen.lock().unwrap()[from..]
        .iter()
        .filter(|r| r["path"].as_str().unwrap_or("").ends_with("/stream"))
        .map(|r| r["body"]["model_id"].as_str().unwrap_or("").to_string())
        .collect()
}

fn say(eleven: &Eleven, text: &str, voice: &str) -> Result<Vec<i16>, AiError> {
    let mut all = Vec::new();
    let mut pieces = 0;
    eleven.speech_stream(text, voice, None, &mut |samples| {
        pieces += 1;
        all.extend_from_slice(samples)
    })?;
    if !all.is_empty() {
        assert!(pieces > 1, "it should come in pieces, as it is made");
    }
    Ok(all)
}

#[test]
fn the_voices_on_the_account_come_by_name_with_what_they_are() {
    if !have_curl() {
        eprintln!("curl is not installed here; skipped");
        return;
    }
    let (base, _) = fake();
    let voices = Eleven::new(KEY, &base).voices().unwrap();
    let names: Vec<&str> = voices.iter().map(|v| v.name.as_str()).collect();
    assert_eq!(names, ["anna", "Zed"]);
    assert_eq!(voices[1].about, "american, young, energetic, social media");
    assert_eq!(voices[0].about, "middle aged, calm");
    // The lively one to start with.
    assert_eq!(default_voice(&voices).unwrap().id, "v-ok");
    // A key ElevenLabs refuses says why.
    let refused = Eleven::new("sk_refused", &base).voices().unwrap_err();
    assert!(
        refused.detail().contains("quota_exceeded"),
        "{}",
        refused.detail()
    );
}

#[test]
fn speech_streams_and_models_the_account_cant_use_are_skipped() {
    if !have_curl() {
        return;
    }
    let (base, seen) = fake();
    let eleven = Eleven::new(KEY, &base);
    // The quickest model first, its words and lively settings, raw 24 kHz.
    let samples = say(&eleven, "Pot now!", "v-ok").unwrap();
    assert_eq!(samples.len(), 4800);
    assert!(samples.iter().all(|&s| s == ELEVEN_SAMPLE));
    let asked = seen.lock().unwrap().last().cloned().unwrap();
    assert_eq!(asked["path"], "/v1/text-to-speech/v-ok/stream");
    assert_eq!(asked["format"], "pcm_24000");
    assert_eq!(asked["body"]["model_id"], "eleven_v4_turbo");
    assert_eq!(asked["body"]["text"], "Pot now!");
    assert!(asked["body"]["voice_settings"]["stability"].is_number());

    // No Eleven v4 on this one: the next model, and from then on straight to it.
    let eleven = Eleven::new(KEY, &base);
    let from = seen.lock().unwrap().len();
    assert_eq!(say(&eleven, "Pot now!", "v-nov4").unwrap().len(), 4800);
    assert_eq!(
        models_asked(&seen, from),
        ["eleven_v4_turbo", "eleven_flash_v2_5"]
    );
    let from = seen.lock().unwrap().len();
    say(&eleven, "Again!", "v-nov4").unwrap();
    assert_eq!(models_asked(&seen, from), ["eleven_flash_v2_5"]);
    let flash = seen.lock().unwrap().last().cloned().unwrap();
    assert!(flash["body"]["voice_settings"]["speed"].as_f64().unwrap() > 1.0);

    // Hebrew only to the model that speaks it, and that one isn't there:
    // nothing is asked, and it says why.
    let from = seen.lock().unwrap().len();
    match say(&eleven, "תשתה שיקוי!", "v-nov4") {
        Err(AiError::Unsupported(why)) => assert!(why.contains("language"), "{why}"),
        other => panic!("{other:?}"),
    }
    assert!(models_asked(&seen, from).is_empty());
    assert!(!eleven.resting());

    // Settings a model won't take: asked again without them, and from then
    // on without them.
    let eleven = Eleven::new(KEY, &base);
    let from = seen.lock().unwrap().len();
    assert_eq!(say(&eleven, "Nice!", "v-plain").unwrap().len(), 4800);
    let asked: Vec<Value> = seen.lock().unwrap()[from..].to_vec();
    assert_eq!(asked.len(), 2);
    assert!(asked[0]["body"].get("voice_settings").is_some());
    assert!(asked[1]["body"].get("voice_settings").is_none());
    assert_eq!(asked[1]["body"]["model_id"], "eleven_v4_turbo");
    let from = seen.lock().unwrap().len();
    say(&eleven, "Nice again!", "v-plain").unwrap();
    let asked: Vec<Value> = seen.lock().unwrap()[from..].to_vec();
    assert_eq!(asked.len(), 1);
    assert!(asked[0]["body"].get("voice_settings").is_none());
}

#[test]
fn failing_again_and_again_it_rests_and_no_credits_rests_longer() {
    if !have_curl() {
        return;
    }
    let (base, _) = fake();
    // Down: once is let go, twice in a row rests a while.
    let eleven = Eleven::new(KEY, &base);
    assert!(matches!(
        say(&eleven, "Hi!", "v-down"),
        Err(AiError::Http(500, _))
    ));
    assert!(!eleven.resting());
    assert!(say(&eleven, "Hi!", "v-down").is_err());
    assert!(eleven.resting());
    // Out of credits: at once.
    let broke = Eleven::new("sk_refused_no_credits_at_all_000000", &base);
    assert!(matches!(
        say(&broke, "Hi!", "v-ok"),
        Err(AiError::Http(401, _))
    ));
    assert!(broke.resting());
}

/// Settings for a test, with the voice picked.
fn picked(name: &str, voice: &str) -> (std::path::PathBuf, ms::ai::Learning) {
    let settings = std::env::temp_dir().join(format!(
        "maplesyrup-eleven-{name}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&settings);
    std::fs::create_dir_all(&settings).unwrap();
    let learning = ms::ai::Learning::load(&settings);
    learning.memory().voice = Some(voice.into());
    (settings, learning)
}

/// A line said by the worker: its samples, and what it noted.
fn worker_says(worker: &ms::ai::Worker, text: &str) -> (Vec<i16>, Vec<String>) {
    worker.send(Job::Say {
        heard: None,
        text: text.into(),
    });
    let (mut samples, mut noted, mut ended) = (Vec::new(), Vec::new(), false);
    while !ended {
        match worker.done.recv_timeout(Duration::from_secs(30)) {
            Ok(Done::Audio {
                samples: s, end, ..
            }) => {
                samples.extend(s);
                ended = end;
            }
            Ok(Done::Noted { line }) => noted.push(line),
            Ok(Done::Failed { error, .. }) => panic!("{error}"),
            Ok(_) => {}
            Err(e) => panic!("{e}: {} samples, {noted:?}", samples.len()),
        }
    }
    (samples, noted)
}

#[test]
fn the_worker_speaks_in_the_picked_voice_and_openai_stands_in_when_it_cant() {
    if !have_curl() {
        return;
    }
    let (base, _) = fake();
    let (settings, learning) = picked("worker", "v-nov4");
    let mut brain = Brain::new();
    brain.learning = Some(learning.clone());
    let worker = ms::ai::spawn_brains(
        Brains {
            openai: OpenAi::new(OPENAI_KEY, &base, "cedar", None),
            fast: None,
            eleven: Some(Eleven::new(KEY, &base)),
        },
        brain,
        None,
    );
    // The picked voice speaks.
    let (samples, noted) = worker_says(&worker, "Pot now!");
    assert_eq!(samples.len(), 4800);
    assert!(samples.iter().all(|&s| s == ELEVEN_SAMPLE));
    assert!(noted.is_empty(), "{noted:?}");
    // Hebrew, with no model on the account that speaks it: OpenAI's voice
    // says it, and it says so once.
    let (samples, noted) = worker_says(&worker, "תשתה שיקוי!");
    assert!(!samples.is_empty() && samples.iter().all(|&s| s == OPENAI_SAMPLE));
    assert_eq!(noted.len(), 1);
    assert!(
        noted[0].starts_with("ElevenLabs didn't speak (no ElevenLabs model"),
        "{}",
        noted[0]
    );
    let (_, noted) = worker_says(&worker, "שוב!");
    assert!(noted.is_empty(), "{noted:?}");
    // The player picks OpenAI's voice: it speaks, ElevenLabs isn't asked.
    learning.memory().voice = Some("openai".into());
    let (samples, _) = worker_says(&worker, "Pot now!");
    assert!(samples.iter().all(|&s| s == OPENAI_SAMPLE));
    let _ = std::fs::remove_dir_all(settings);
}
