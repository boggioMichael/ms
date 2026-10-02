//! The OpenAI client against a stand-in server on this machine, through the
//! same `curl` the real calls use: model fallback, the reasoning retry,
//! streamed replies, speech, refused keys, and the worker speaking a reply
//! a sentence at a time.

use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ms::ai::openai::Turn;
use ms::ai::{AiError, Brain, Done, Job, OpenAi};
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
                                    let said = last["content"].as_str().unwrap_or("");
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
fn the_worker_speaks_a_reply_a_sentence_at_a_time() {
    if !have_curl() {
        return;
    }
    let (base, seen) = fake();
    let ai = OpenAi::new("sk-test-key-0123456789abcdef", &base, "cedar", None);
    let worker = ms::ai::spawn(ai, Brain::new());
    worker.send(Job::Converse {
        heard: "can you see my game".into(),
        snapshot: "HP is about 80%.".into(),
        speak: true,
    });
    let mut reply = None;
    let mut spoken = Vec::new();
    while reply.is_none() || spoken.len() < 2 {
        match worker.done.recv_timeout(Duration::from_secs(30)) {
            Ok(Done::Reply { text, .. }) => reply = Some(text),
            Ok(Done::Audio {
                text,
                samples,
                first,
                ..
            }) => {
                assert_eq!(samples.len(), 2400);
                spoken.push((text, first));
            }
            Ok(Done::Failed { error, .. }) => panic!("{error}"),
            Ok(Done::Silent { heard }) => panic!("silent: {heard}"),
            Err(e) => panic!("{e}: {reply:?} {spoken:?}"),
        }
    }
    assert_eq!(
        reply.as_deref(),
        Some("Hello there, my friend! (gpt-6.1-sol) you said: can you see my game.")
    );
    assert_eq!(
        spoken,
        [
            ("Hello there, my friend!".to_string(), true),
            (
                "(gpt-6.1-sol) you said: can you see my game.".to_string(),
                false
            )
        ]
    );
    // The model was told what is on screen.
    let asked = seen
        .lock()
        .unwrap()
        .iter()
        .rev()
        .find(|r| r["path"] == "/v1/responses")
        .cloned()
        .unwrap();
    assert!(
        asked["body"]["instructions"]
            .as_str()
            .unwrap()
            .contains("HP is about 80%.")
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
