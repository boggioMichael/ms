//! The voice turn's timing, on (`ms::util::latency`): through the real
//! worker against a stand-in OpenAI, Claude's board and the phone link, a
//! turn of MapleSyrup's own brain and one of Claude's each come to one line
//! with their stages in order. (A test binary of its own: the program's
//! tracer is turned on here, and every stamp of this process goes to it.)

use std::net::TcpListener;
use std::time::{Duration, Instant};

use ms::ai::{Brain, Done, Job, OpenAi, Worker};
use ms::claude::board::Board;
use ms::companion::Kind;
use ms::phone::http::{Conn, Request, Response, parse_query};
use ms::phone::{Hub, VoiceOn};
use ms::util::latency::{self, Key, Mode, Stage, TRACER};
use serde_json::json;

const KEY: &str = "sk-test-key-0123456789abcdef";
const BOARD_KEY: &str = "0123456789abcdef0123456789abcdef";
const LINK_KEY: &str = "k1";

/// A stand-in OpenAI: a reply of two sentences, streamed, and speech.
fn fake() -> String {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            std::thread::spawn(move || {
                let mut conn = Conn::new(stream);
                while let Ok(Some(req)) = conn.read_request(16 << 20) {
                    let response = match req.path.as_str() {
                        "/v1/responses" => {
                            let mut events = String::new();
                            for delta in ["You're on ", "Maple Island. ", "Hunt ", "the snails."] {
                                let event =
                                    json!({"type": "response.output_text.delta", "delta": delta});
                                events.push_str(&format!("data: {event}\n\n"));
                            }
                            events.push_str(
                                "data: {\"type\":\"response.completed\",\"response\":{}}\n\n",
                            );
                            Response::new(200, "text/event-stream", events.into_bytes())
                        }
                        "/v1/audio/speech" => {
                            Response::new(200, "application/octet-stream", vec![0u8; 4800])
                        }
                        _ => Response::json(200, &json!({"data": []})),
                    };
                    if conn.write_response(&response, true).is_err() {
                        return;
                    }
                }
            });
        }
    });
    format!("http://127.0.0.1:{port}/v1")
}

fn have_curl() -> bool {
    std::process::Command::new(if cfg!(windows) { "curl.exe" } else { "curl" })
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

fn request(method: &str, target: &str, body: serde_json::Value) -> Request {
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    Request {
        method: method.into(),
        path: path.into(),
        query: parse_query(query),
        headers: Vec::new(),
        body: body.to_string().into_bytes(),
    }
}

/// The worker's news of job `id` until it is over, the main loop's part
/// played by `on_audio` (a piece of its voice: its first, its last).
fn until_voiced(worker: &Worker, id: u64, on_audio: &mut dyn FnMut(bool, bool)) {
    loop {
        match worker.done.recv_timeout(Duration::from_secs(30)) {
            Ok(Done::Audio {
                id: of, start, end, ..
            }) if of == id => on_audio(start, end),
            Ok(Done::Voiced { id: of }) if of == id => return,
            Ok(Done::Failed { error, .. }) => panic!("{error}"),
            Ok(_) => {}
            Err(e) => panic!("{e}"),
        }
    }
}

/// Each of `stages` is in `line`, and none comes before the one before it.
fn in_order(line: &str, stages: &[Stage]) {
    let ms = |stage: &Stage| -> i64 {
        line.split(' ')
            .find_map(|part| part.strip_prefix(stage.name())?.strip_prefix('='))
            .unwrap_or_else(|| panic!("{} in {line}", stage.name()))
            .parse()
            .unwrap()
    };
    for pair in stages.windows(2) {
        assert!(ms(&pair[0]) <= ms(&pair[1]), "{:?} in {line}", pair);
    }
}

#[test]
fn a_turn_of_its_own_brain_and_one_of_claudes_are_a_line_each() {
    if !have_curl() {
        eprintln!("curl is not installed here; skipped");
        return;
    }
    TRACER.enable(true);
    let worker = ms::ai::spawn(OpenAi::new(KEY, &fake(), "cedar", None), Brain::new());

    // MapleSyrup's own brain, its voice on the PC.
    let heard = Instant::now();
    let id = worker.send(Job::Converse {
        heard: "where do I go".into(),
        snapshot: "HP is about 80%.".into(),
        facts: Default::default(),
        speak: true,
        eyes: None,
        language: None,
    });
    TRACER.open(Key::Job(id), Mode::Model, "where do I go", heard);
    until_voiced(&worker, id, &mut |start, _| {
        if start {
            TRACER.stamp(Key::Job(id), Stage::PcPlay);
        }
    });
    TRACER.voiced(id);
    let lines = TRACER.due(Instant::now());
    assert_eq!(lines.len(), 1, "{lines:?}");
    let line = &lines[0];
    assert!(line.contains("mode=model"), "{line}");
    in_order(
        line,
        &[
            Stage::Heard,
            Stage::Queued,
            Stage::Lane,
            Stage::Request,
            Stage::FirstToken,
            Stage::FirstSentence,
            Stage::TtsSent,
            Stage::TtsFirst,
            Stage::TtsDone,
        ],
    );
    in_order(line, &[Stage::FirstToken, Stage::LlmDone]);
    in_order(line, &[Stage::TtsFirst, Stage::PcPlay]);
    assert!(
        line.contains(&format!(" tts={}", latency::TTS_OPENAI)),
        "{line}"
    );
    assert!(line.contains(" model="), "{line}");
    assert!(line.ends_with(" end=played"), "{line}");

    // Claude's, its voice on the phone: the phone's stamps with the
    // sentence, the bridge, a tool, the line, its clip, the phone playing it.
    let hub = Hub::new(LINK_KEY.into(), None, VoiceOn::Phone);
    let up = Instant::now();
    let uptime = || up.elapsed().as_secs_f64();
    let spoke = uptime();
    std::thread::sleep(Duration::from_millis(20));
    let sent = uptime();
    let stamps =
        json!({"text": "what now", "latency": {"spoke": spoke, "sent": sent, "settle": 550}});
    hub.handle(&request(
        "POST",
        &format!("/api/heard?k={LINK_KEY}"),
        stamps,
    ));
    assert!(!hub.take_inbox().is_empty());
    let board = Board::new(BOARD_KEY.into());
    let seq = board.post(
        "heard",
        "He said: \"what now\"",
        "He said: \"what now\"".into(),
    );
    TRACER.open(Key::Heard(seq), Mode::Claude, "what now", Instant::now());
    let since = seq - 1;
    let poll = format!("/local/events?since={since}&wait=0&listen=1&k={BOARD_KEY}");
    assert_eq!(
        board.handle(&request("GET", &poll, json!(null))).status,
        200
    );
    let status = format!("/local/status?k={BOARD_KEY}");
    assert_eq!(
        board.handle(&request("GET", &status, json!(null))).status,
        200
    );
    let say = json!({"text": "Go left.", "answers": seq});
    let said = board.handle(&request("POST", &format!("/local/say?k={BOARD_KEY}"), say));
    assert_eq!(said.status, 200);
    assert_eq!(board.take_says(), vec!["Go left.".to_string()]);
    let job = worker.send(Job::Say {
        heard: None,
        text: "Go left.".into(),
    });
    TRACER.said("Go left.", job);
    let mut clip = None;
    until_voiced(&worker, job, &mut |_, end| {
        if end {
            let seq = hub.set_clip(Kind::Reply, ms::ai::wav_bytes(&[0; 240], 24_000));
            TRACER.clip(job, seq);
            clip = Some(seq);
        }
    });
    TRACER.voiced(job);
    // Not before the phone says its clip began to play.
    assert!(TRACER.due(Instant::now()).is_empty());
    let played = json!({"clip": clip.unwrap(), "played": uptime()});
    let path = format!("{}?k={LINK_KEY}", latency::PHONE_PATH);
    assert_eq!(hub.handle(&request("POST", &path, played)).status, 200);
    let lines = TRACER.due(Instant::now());
    TRACER.enable(false);
    assert_eq!(lines.len(), 1, "{lines:?}");
    let line = &lines[0];
    assert!(line.contains("mode=claude"), "{line}");
    in_order(
        line,
        &[
            Stage::Spoke,
            Stage::Sent,
            Stage::Arrived,
            Stage::Heard,
            Stage::Bridge,
            Stage::Say,
            Stage::Queued,
            Stage::Lane,
            Stage::TtsSent,
            Stage::TtsFirst,
            Stage::TtsDone,
            Stage::Clip,
            Stage::Played,
        ],
    );
    assert!(line.contains(" spans=game_status@"), "{line}");
    assert!(line.contains(" settle=550"), "{line}");
    assert!(line.ends_with(" end=played"), "{line}");
}
