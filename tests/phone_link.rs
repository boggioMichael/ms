//! The phone link end to end, over real sockets: TLS with the certificate
//! MapleSyrup makes, the API the page calls, the redirect for a plain
//! `http://` link, and the local plain-HTTP door a tunnel uses.

use ms::companion::Kind;
use ms::phone::{self, Hub, Inbound, VoiceOn, client, tls};

fn setup() -> (std::sync::Arc<Hub>, tls::Identity, u16) {
    let dir = std::env::temp_dir().join(format!("ms-link-{}", tls::random_hex(4)));
    let identity = tls::load_or_create(&dir, &["127.0.0.1".into(), "localhost".into()]).unwrap();
    let hub = Hub::new("testkey".into(), None, VoiceOn::Pc);
    let config = tls::server_config(&identity).unwrap();
    // A port unlikely to be taken, and the next free one if it is.
    let port = phone::serve_tls(hub.clone(), config, 38443).unwrap();
    (hub, identity, port)
}

#[test]
fn the_page_and_the_api_work_over_tls() {
    let (hub, identity, port) = setup();
    let cert = &identity.cert_der;

    let page = client::request(port, cert, "GET", "/?k=testkey", b"").unwrap();
    assert_eq!(page.status, 200);
    assert!(page.text().contains("Start listening"));

    let health = client::request(port, cert, "GET", "/health", b"").unwrap();
    assert_eq!(health.text(), "ok maplesyrup");

    let refused = client::request(port, cert, "GET", "/api/state?k=wrong", b"").unwrap();
    assert_eq!(refused.status, 403);

    let heard = client::request(
        port,
        cert,
        "POST",
        "/api/heard?k=testkey",
        br#"{"text":"syrup status"}"#,
    )
    .unwrap();
    assert_eq!(heard.status, 200);
    assert_eq!(
        hub.take_inbox(),
        vec![Inbound::Heard("syrup status".into())]
    );

    let samples: Vec<u8> = (0..4000i32)
        .flat_map(|i| (((i as f32 * 0.25).sin() * 9000.0) as i16).to_le_bytes())
        .collect();
    let audio = client::request(
        port,
        cert,
        "POST",
        "/api/audio?k=testkey&rate=16000",
        &samples,
    )
    .unwrap();
    assert_eq!(audio.status, 204);
    assert!(hub.summary().mic.live);

    hub.post(Kind::Reply, "Level 57, HP 82 percent.", true);
    let state = client::request(port, cert, "GET", "/api/state?k=testkey&since=0", b"").unwrap();
    let json: serde_json::Value = serde_json::from_slice(&state.body).unwrap();
    assert_eq!(json["messages"][0]["text"], "Level 57, HP 82 percent.");
    assert_eq!(json["mic"]["live"], true);
}

#[test]
fn a_certificate_other_than_ours_is_refused_by_the_pinned_client() {
    let (_hub, _identity, port) = setup();
    let other = tls::create(&["127.0.0.1".into()]).unwrap();
    assert!(client::request(port, &other.cert_der, "GET", "/health", b"").is_err());
}

#[test]
fn plain_http_on_the_tls_port_is_sent_to_https() {
    let (_hub, _identity, port) = setup();
    let reply = client::request_plain(port, "GET", "/?k=testkey", b"").unwrap();
    assert_eq!(reply.status, 301);
    assert_eq!(
        reply.header("location"),
        Some(format!("https://127.0.0.1:{port}/?k=testkey").as_str())
    );
}

#[test]
fn the_local_door_for_a_tunnel_serves_plain_http() {
    let hub = Hub::new("k".into(), None, VoiceOn::Pc);
    let port = phone::serve_local(hub.clone(), 0).unwrap();
    let reply =
        client::request_plain(port, "POST", "/api/command?k=k", br#"{"command":"mark"}"#).unwrap();
    assert_eq!(reply.status, 200);
    assert_eq!(hub.take_inbox(), vec![Inbound::Command("mark".into())]);
}
