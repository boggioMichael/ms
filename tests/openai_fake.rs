//! The OpenAI client against a stand-in server on this machine, through the
//! same `curl` the real calls use: model fallback, the reasoning retry,
//! streamed replies, speech (streamed too), refused keys, calling a request
//! off, and the worker speaking a reply line by line.

use std::io::Write;
use std::net::TcpListener;
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ms::ai::openai::Turn;
use ms::ai::{AiError, Brain, Done, Job, OpenAi, Stop};
use ms::phone::http::{Conn, Response};
use serde_json::{Value, json};

/// A fake OpenAI: `gpt-6-luna` does not exist for this key, `gpt-6.1-sol`
/// refuses `reasoning`, and every request is recorded.
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
                    let auth = req.header("authorization").unwrap_or_default().to_string();
                    let body: Value = serde_json::from_slice(&req.body).unwrap_or(Value::Null);
                    log.lock()
                        .unwrap()
                        .push(json!({"path": req.path, "body": body}));
                    let input = body["input"].as_str().unwrap_or_default().to_string();
                    // Speech made slowly: sent as it is made, in odd-sized
                    // pieces (a sample split between two of them).
                    if req.path == "/v1/audio/speech" && input.contains("slowly") {
                        let out = conn.get_mut();
                        let _ = out.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nTransfer-Encoding: chunked\r\n\r\n");
                        let mut pcm = Vec::new();
                        for i in 0..6000i16 {
                            pcm.extend_from_slice(&(i % 300).to_le_bytes());
                        }
                        for piece in pcm.chunks(2001) {
                            let _ = write!(out, "{:x}\r\n", piece.len());
                            let _ = out.write_all(piece);
                            let _ = out.write_all(b"\r\n");
                            let _ = out.flush();
                            std::thread::sleep(Duration::from_millis(60));
                        }
                        let _ = out.write_all(b"0\r\n\r\n");
                        let _ = out.flush();
                        continue;
                    }
                    // A request that takes forever (to be called off).
                    let last_said = body["input"]
                        .as_array()
                        .and_then(|a| a.last())
                        .map(|l| l["content"].to_string())
                        .unwrap_or_default();
                    if input.contains("forever") || last_said.contains("forever") {
                        std::thread::sleep(Duration::from_secs(20));
                        return;
                    }
                    let response = if auth != "Bearer sk-test-key-0123456789abcdef" {
                        Response::json(
                            401,
                            &json!({"error": {"message": "Incorrect API key provided"}}),
                        )
                    } else {
                        match req.path.as_str() {
                            "/v1/models" => Response::json(200, &json!({"data": []})),
                            "/v1/responses" => match body["model"].as_str() {
                                Some("gpt-6-luna") => Response::json(
                                    404,
                                    &json!({"error": {"message": "The model `gpt-6-luna` does not exist or you do not have access to it.", "code": "model_not_found"}}),
                                ),
                                Some("gpt-6.1-sol") if body.get("reasoning").is_some() => {
                                    Response::json(
                                        400,
                                        &json!({"error": {"message": "Unsupported parameter: 'reasoning.effort' is not supported with this model."}}),
                                    )
                                }
                                Some(model) if body["stream"] == true => {
                                    let last = body["input"]
                                        .as_array()
                                        .and_then(|a| a.last())
                                        .cloned()
                                        .unwrap_or_default();
                                    // The words, or the last text part (after
                                    // what is on screen).
                                    let said = last["content"].as_str().unwrap_or_else(|| {
                                        last["content"]
                                            .as_array()
                                            .and_then(|parts| {
                                                parts.iter().rev().find_map(|p| p["text"].as_str())
                                            })
                                            .unwrap_or("")
                                    });
                                    let reply = format!(
                                        "Hello there, my friend! ({model}) you said: {said}."
                                    );
                                    // Server-sent events, the text a few characters at a time.
                                    let mut events = String::from(
                                        "event: response.created\ndata: {\"type\":\"response.created\"}\n\n",
                                    );
                                    let chars: Vec<char> = reply.chars().collect();
                                    for piece in chars.chunks(5) {
                                        let delta: String = piece.iter().collect();
                                        events.push_str(&format!(
                                            "event: response.output_text.delta\ndata: {}\n\n",
                                            json!({"type": "response.output_text.delta", "delta": delta})
                                        ));
                                    }
                                    events.push_str("event: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{}}\n\n");
                                    Response::new(200, "text/event-stream", events.into_bytes())
                                }
                                Some(model) => {
                                    let last = body["input"]
                                        .as_array()
                                        .and_then(|a| a.last())
                                        .cloned()
                                        .unwrap_or_default();
                                    Response::json(
                                        200,
                                        &json!({"output": [
                                            {"type": "reasoning", "summary": []},
                                            {"type": "message", "role": "assistant", "content": [
                                                {"type": "output_text", "text": format!("({model}) you said: {}", last["content"].as_str().unwrap_or(""))}
                                            ]}
                                        ]}),
                                    )
                                }
                                None => {
                                    Response::json(400, &json!({"error": {"message": "no model"}}))
                                }
                            },
                            "/v1/realtime/client_secrets" => {
                                match body["session"]["model"].as_str() {
                                    Some("gpt-realtime") => Response::json(
                                        404,
                                        &json!({"error": {"message": "The model `gpt-realtime` does not exist or you do not have access to it."}}),
                                    ),
                                    Some(_) => Response::json(
                                        200,
                                        &json!({"value": "ek_test_live", "expires_at": 1, "session": body["session"]}),
                                    ),
                                    None => Response::json(
                                        400,
                                        &json!({"error": {"message": "no session"}}),
                                    ),
                                }
                            }
                            "/v1/audio/speech" => {
                                let mut pcm = Vec::new();
                                for i in 0..2400i16 {
                                    pcm.extend_from_slice(&(i % 100).to_le_bytes());
                                }
                                Response::new(200, "application/octet-stream", pcm)
                            }
                            _ => {
                                Response::json(404, &json!({"error": {"message": "no such path"}}))
                            }
                        }
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

#[test]
fn replies_fall_back_through_the_models_and_drop_reasoning_when_refused() {
    if !have_curl() {
        eprintln!("curl is not installed here; skipped");
        return;
    }
    let (base, seen) = fake();
    let ai = OpenAi::new("sk-test-key-0123456789abcdef", &base, "cedar", None);
    ai.check().unwrap();
    let turns = vec![Turn {
        role: "user",
        text: "can you see my game".into(),
    }];
    let reply = ai.respond("be a dog", &turns).unwrap();
    assert_eq!(reply, "(gpt-6.1-sol) you said: can you see my game");
    assert_eq!(ai.model().as_deref(), Some("gpt-6.1-sol"));
    // The chosen model is reused, without asking again.
    let before = seen.lock().unwrap().len();
    ai.respond("be a dog", &turns).unwrap();
    assert_eq!(seen.lock().unwrap().len(), before + 1);
    let last = seen.lock().unwrap().last().cloned().unwrap();
    assert_eq!(last["body"]["model"], "gpt-6.1-sol");
    assert!(last["body"].get("reasoning").is_none());
    assert_eq!(last["body"]["store"], false);
    assert_eq!(last["body"]["instructions"], "be a dog");
}

#[test]
fn a_streamed_reply_arrives_in_pieces_after_the_same_fallbacks() {
    if !have_curl() {
        return;
    }
    let (base, seen) = fake();
    let ai = OpenAi::new("sk-test-key-0123456789abcdef", &base, "cedar", None);
    let turns = vec![Turn {
        role: "user",
        text: "how am I doing".into(),
    }];
    let mut pieces = Vec::new();
    let reply = ai
        .respond_stream("be a dog", &turns, &mut |piece| {
            pieces.push(piece.to_string())
        })
        .unwrap();
    assert_eq!(
        reply,
        "Hello there, my friend! (gpt-6.1-sol) you said: how am I doing."
    );
    assert!(pieces.len() > 5);
    assert_eq!(pieces.concat(), reply);
    let last = seen.lock().unwrap().last().cloned().unwrap();
    assert_eq!(last["body"]["stream"], true);
    assert_eq!(ai.model().as_deref(), Some("gpt-6.1-sol"));
}

#[test]
fn the_worker_speaks_a_reply_line_by_line_as_the_voice_is_made() {
    if !have_curl() {
        return;
    }
    let (base, seen) = fake();
    let ai = OpenAi::new("sk-test-key-0123456789abcdef", &base, "cedar", None);
    let worker = ms::ai::spawn(ai, Brain::new());
    let id = worker.send(Job::Converse {
        heard: "can you see my game".into(),
        snapshot: "HP is about 80%.".into(),
        speak: true,
        eyes: None,
        language: None,
    });
    let mut reply = None;
    // Each line: its words, whether it was the first, its samples, closed.
    let mut lines: Vec<(String, bool, usize, bool)> = Vec::new();
    while reply.is_none() || lines.len() < 2 || !lines.iter().all(|l| l.3) {
        match worker.done.recv_timeout(Duration::from_secs(30)) {
            Ok(Done::Reply { id: of, text, .. }) => {
                assert_eq!(of, id);
                reply = Some(text)
            }
            Ok(Done::Audio {
                id: of,
                text,
                samples,
                first,
                start,
                end,
                ..
            }) => {
                assert_eq!(of, id);
                if start {
                    lines.push((text, first, 0, false));
                }
                let line = lines.last_mut().expect("a piece before its line");
                line.2 += samples.len();
                if end {
                    line.3 = true;
                }
            }
            Ok(Done::Failed { error, .. }) => panic!("{error}"),
            Ok(Done::Silent { heard, .. }) => panic!("silent: {heard}"),
            Ok(Done::Noted { line }) => panic!("noted: {line}"),
            Ok(Done::Shown { text, .. }) => panic!("shown: {text}"),
            Ok(Done::Command { word }) => panic!("command: {word}"),
            Err(e) => panic!("{e}: {reply:?} {lines:?}"),
        }
    }
    assert_eq!(
        reply.as_deref(),
        Some("Hello there, my friend! (gpt-6.1-sol) you said: can you see my game.")
    );
    // The first sentence alone (heard soonest), then the rest together.
    assert_eq!(
        lines,
        [
            ("Hello there, my friend!".to_string(), true, 2400, true),
            (
                "(gpt-6.1-sol) you said: can you see my game.".to_string(),
                false,
                2400,
                true
            )
        ]
    );
    // The model was told what is on screen, with the player's words (the
    // instructions stay the same from reply to reply, for the cache).
    let asked = seen
        .lock()
        .unwrap()
        .iter()
        .rev()
        .find(|r| r["path"] == "/v1/responses")
        .cloned()
        .unwrap();
    let last = asked["body"]["input"].as_array().unwrap().last().unwrap();
    let parts: Vec<&str> = last["content"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|p| p["text"].as_str())
        .collect();
    assert!(parts[0].contains("HP is about 80%."), "{parts:?}");
    assert_eq!(parts.last(), Some(&"can you see my game"));
    assert!(
        !asked["body"]["instructions"]
            .as_str()
            .unwrap()
            .contains("HP is about 80%.")
    );
}

#[test]
fn speech_arrives_in_pieces_while_it_is_made() {
    if !have_curl() {
        return;
    }
    let (base, _) = fake();
    let ai = OpenAi::new("sk-test-key-0123456789abcdef", &base, "cedar", None);
    let started = Instant::now();
    let mut pieces: Vec<(Duration, Vec<i16>)> = Vec::new();
    let count = ai
        .speech_stream("say this slowly", "warm", None, &mut |samples| {
            pieces.push((started.elapsed(), samples.to_vec()))
        })
        .unwrap();
    assert_eq!(count, 6000);
    // Several pieces, the first long before the last: it can play already.
    assert!(pieces.len() >= 3, "{}", pieces.len());
    let (first, last) = (pieces[0].0, pieces.last().unwrap().0);
    assert!(
        last - first >= Duration::from_millis(200),
        "{first:?} {last:?}"
    );
    // Samples split between pieces come out whole and in order.
    let all: Vec<i16> = pieces.into_iter().flat_map(|(_, s)| s).collect();
    assert!(all.iter().enumerate().all(|(i, s)| *s == (i % 300) as i16));
}

#[test]
fn a_request_is_called_off_at_once() {
    if !have_curl() {
        return;
    }
    let (base, _) = fake();
    let ai = OpenAi::new("sk-test-key-0123456789abcdef", &base, "cedar", None);
    let mark = Arc::new(AtomicU64::new(0));
    let stop = Stop::new(Arc::clone(&mark), 7);
    let caller = {
        let mark = Arc::clone(&mark);
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            mark.store(7, std::sync::atomic::Ordering::SeqCst);
        })
    };
    let started = Instant::now();
    let result = ai.speech_stream("take forever", "warm", Some(&stop), &mut |_| {});
    assert_eq!(result, Err(AiError::Cancelled));
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "{:?}",
        started.elapsed()
    );
    caller.join().unwrap();
    // Already called off: not even sent.
    assert_eq!(
        ai.speech_stream("hello", "warm", Some(&stop), &mut |_| {}),
        Err(AiError::Cancelled)
    );
}

#[test]
fn a_reply_talked_over_stops_and_the_next_is_answered() {
    if !have_curl() {
        return;
    }
    let (base, _) = fake();
    let ai = OpenAi::new("sk-test-key-0123456789abcdef", &base, "cedar", None);
    let worker = ms::ai::spawn(ai, Brain::new());
    let slow = worker.send(Job::Converse {
        heard: "think about this forever".into(),
        snapshot: String::new(),
        speak: true,
        eyes: None,
        language: None,
    });
    std::thread::sleep(Duration::from_millis(400));
    assert!(worker.busy());
    let started = Instant::now();
    worker.cancel_all();
    assert!(worker.cancelled(slow));
    let next = worker.send(Job::Converse {
        heard: "how am I doing".into(),
        snapshot: String::new(),
        speak: false,
        eyes: None,
        language: None,
    });
    assert!(!worker.cancelled(next));
    loop {
        match worker.done.recv_timeout(Duration::from_secs(10)) {
            Ok(Done::Reply { id, text, .. }) => {
                assert_eq!(id, next, "the call-off reply came back: {text}");
                assert!(text.ends_with("you said: how am I doing."), "{text}");
                break;
            }
            Ok(Done::Failed { error, .. }) => panic!("{error}"),
            Ok(_) => {}
            Err(e) => panic!("{e}"),
        }
    }
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "{:?}",
        started.elapsed()
    );
}

#[test]
fn its_own_lines_are_translated_shown_and_spoken() {
    if !have_curl() {
        return;
    }
    let (base, seen) = fake();
    let ai = OpenAi::new("sk-test-key-0123456789abcdef", &base, "cedar", None);
    let worker = ms::ai::spawn(ai, Brain::new());
    worker.send(Job::Speak {
        text: "Level up! Nice.".into(),
        language: Some("he-IL".into()),
        show: Some(ms::companion::Kind::Alert),
        speak: true,
    });
    let mut shown = None;
    let mut spoken = None;
    while shown.is_none() || spoken.is_none() {
        match worker.done.recv_timeout(Duration::from_secs(30)) {
            Ok(Done::Shown { text, kind }) => {
                assert_eq!(kind, ms::companion::Kind::Alert);
                shown = Some(text);
            }
            Ok(Done::Audio {
                text, start: true, ..
            }) => spoken = Some(text),
            Ok(Done::Failed { error, .. }) => panic!("{error}"),
            Ok(_) => {}
            Err(e) => panic!("{e}"),
        }
    }
    // The stand-in "translates" by saying it back; the plumbing is the point.
    assert_eq!(
        shown.as_deref(),
        Some("(gpt-6.1-sol) you said: Level up! Nice.")
    );
    assert_eq!(spoken, shown);
    let asked = seen
        .lock()
        .unwrap()
        .iter()
        .find(|r| r["path"] == "/v1/responses")
        .cloned()
        .unwrap();
    assert!(
        asked["body"]["instructions"]
            .as_str()
            .unwrap()
            .contains("Hebrew")
    );
    // English needs no translation: straight to speech.
    let before = seen.lock().unwrap().len();
    worker.send(Job::Speak {
        text: "Level up! Nice.".into(),
        language: Some("en-US".into()),
        show: None,
        speak: true,
    });
    loop {
        if let Ok(Done::Audio {
            text, start: true, ..
        }) = worker.done.recv_timeout(Duration::from_secs(30))
        {
            assert_eq!(text, "Level up! Nice.");
            break;
        }
    }
    let requests = seen.lock().unwrap();
    assert!(
        requests[before..]
            .iter()
            .all(|r| r["path"] == "/v1/audio/speech")
    );
}

#[test]
fn speech_comes_back_as_samples() {
    if !have_curl() {
        return;
    }
    let (base, seen) = fake();
    let ai = OpenAi::new("sk-test-key-0123456789abcdef", &base, "cedar", None);
    let samples = ai.speech("Hey there!", "warm").unwrap();
    assert_eq!(samples.len(), 2400);
    assert_eq!(samples[5], 5);
    let request = seen.lock().unwrap().last().cloned().unwrap();
    assert_eq!(request["body"]["model"], "gpt-4o-mini-tts");
    assert_eq!(request["body"]["voice"], "cedar");
    assert_eq!(request["body"]["response_format"], "pcm");
    assert_eq!(request["body"]["instructions"], "warm");
}

#[test]
fn a_refused_key_says_so() {
    if !have_curl() {
        return;
    }
    let (base, _) = fake();
    let ai = OpenAi::new("sk-wrong-key-0000000000000000", &base, "cedar", None);
    match ai.check() {
        Err(AiError::Http(401, message)) => assert!(message.contains("Incorrect API key")),
        other => panic!("{other:?}"),
    }
    assert_eq!(
        format!("{}", AiError::Http(401, String::new())),
        "OpenAI refused the API key"
    );
}

#[test]
fn nothing_listening_is_a_network_error() {
    if !have_curl() {
        return;
    }
    let ai = OpenAi::new(
        "sk-test-key-0123456789abcdef",
        "http://127.0.0.1:9/v1",
        "cedar",
        None,
    );
    assert!(matches!(ai.check(), Err(AiError::Network(_))));
}

#[test]
fn a_live_call_gets_a_short_lived_key_from_a_realtime_model_the_key_can_use() {
    if !have_curl() {
        return;
    }
    let (base, seen) = fake();
    let ai = Arc::new(OpenAi::new(
        "sk-test-key-0123456789abcdef",
        &base,
        "cedar",
        None,
    ));
    let live = ms::ai::live::Live::new(Arc::clone(&ai), "cedar");
    let instructions =
        ms::ai::live::instructions("- Their class is Night Lord.", &[], Some("he-IL"));
    let tools = ms::ai::live::tools(
        vec![
            json!({"type": "function", "name": "remember_fact", "strict": true, "parameters": {}}),
        ],
        true,
    );
    let call = live.session(&instructions, &tools).unwrap();
    // The full model isn't there for this key: the smaller one is used.
    assert_eq!(call["key"], "ek_test_live");
    assert_eq!(call["model"], "gpt-realtime-mini");
    assert!(
        call["url"]
            .as_str()
            .unwrap()
            .ends_with("/v1/realtime/calls")
    );
    let asked = seen
        .lock()
        .unwrap()
        .iter()
        .rev()
        .find(|r| r["path"] == "/v1/realtime/client_secrets")
        .cloned()
        .unwrap();
    let session = &asked["body"]["session"];
    assert_eq!(session["type"], "realtime");
    assert_eq!(session["audio"]["output"]["voice"], "cedar");
    assert_eq!(
        session["audio"]["input"]["turn_detection"]["type"],
        "semantic_vad"
    );
    assert!(session["audio"]["input"]["transcription"]["language"].is_null());
    assert!(
        session["instructions"]
            .as_str()
            .unwrap()
            .contains("Night Lord")
    );
    let names: Vec<&str> = session["tools"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|t| t["name"].as_str())
        .collect();
    assert_eq!(names, ["remember_fact", "search_web"]);
    // The model that worked is kept.
    let again = live.session(&instructions, &tools).unwrap();
    assert_eq!(again["model"], "gpt-realtime-mini");
}
