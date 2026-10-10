//! A small HTTPS client that trusts exactly one certificate — the one
//! MapleSyrup made — for the self-test and the tests: it talks to the phone
//! link the way the phone does, over the same TLS.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::Arc;
use std::time::Duration;

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{CryptoProvider, verify_tls12_signature, verify_tls13_signature};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{ClientConfig, DigitallySignedStruct, SignatureScheme};

/// Accepts the one certificate it was given and nothing else.
#[derive(Debug)]
struct Pinned {
    cert: Vec<u8>,
    provider: Arc<CryptoProvider>,
}

impl ServerCertVerifier for Pinned {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        if end_entity.as_ref() == self.cert.as_slice() {
            Ok(ServerCertVerified::assertion())
        } else {
            Err(rustls::Error::General(
                "not MapleSyrup's certificate".into(),
            ))
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls12_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}

/// A response: status and body.
#[derive(Debug, Clone)]
pub struct Reply {
    pub status: u16,
    pub body: Vec<u8>,
    pub headers: Vec<(String, String)>,
}

impl Reply {
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

/// One request over a fresh TLS connection to `127.0.0.1:port`, trusting
/// only `cert`.
pub fn request(
    port: u16,
    cert: &[u8],
    method: &str,
    target: &str,
    body: &[u8],
) -> Result<Reply, String> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = ClientConfig::builder_with_provider(Arc::clone(&provider))
        .with_safe_default_protocol_versions()
        .map_err(|e| e.to_string())?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(Pinned {
            cert: cert.to_vec(),
            provider,
        }))
        .with_no_client_auth();
    let name = ServerName::try_from("localhost").map_err(|e| e.to_string())?;
    let conn = rustls::ClientConnection::new(Arc::new(config), name).map_err(|e| e.to_string())?;
    let tcp = TcpStream::connect(("127.0.0.1", port)).map_err(|e| e.to_string())?;
    tcp.set_read_timeout(Some(Duration::from_secs(10))).ok();
    let mut tls = rustls::StreamOwned::new(conn, tcp);
    exchange(&mut tls, method, target, body)
}

/// One request over plain HTTP to `127.0.0.1:port`.
pub fn request_plain(port: u16, method: &str, target: &str, body: &[u8]) -> Result<Reply, String> {
    request_plain_waiting(port, method, target, body, Duration::from_secs(10))
}

/// One request over plain HTTP to `127.0.0.1:port`, waiting up to `wait`
/// for the answer (a long poll waits for its news).
pub fn request_plain_waiting(
    port: u16,
    method: &str,
    target: &str,
    body: &[u8],
    wait: Duration,
) -> Result<Reply, String> {
    let address = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let mut tcp =
        TcpStream::connect_timeout(&address, Duration::from_secs(3)).map_err(|e| e.to_string())?;
    tcp.set_read_timeout(Some(wait)).ok();
    tcp.set_write_timeout(Some(Duration::from_secs(5))).ok();
    exchange(&mut tcp, method, target, body)
}

fn exchange<S: Read + Write>(
    stream: &mut S,
    method: &str,
    target: &str,
    body: &[u8],
) -> Result<Reply, String> {
    let head = format!(
        "{method} {target} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream
        .write_all(head.as_bytes())
        .map_err(|e| e.to_string())?;
    stream.write_all(body).map_err(|e| e.to_string())?;
    stream.flush().map_err(|e| e.to_string())?;
    let mut raw = Vec::new();
    // rustls reports the peer closing without close_notify as an error;
    // whatever arrived before it is still the response.
    let _ = stream.read_to_end(&mut raw);
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or("no response")?;
    let head = String::from_utf8_lossy(&raw[..split]).into_owned();
    let mut lines = head.split("\r\n");
    let status = lines
        .next()
        .and_then(|l| l.split(' ').nth(1))
        .and_then(|s| s.parse().ok())
        .ok_or("bad status line")?;
    let headers = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(n, v)| (n.trim().to_string(), v.trim().to_string()))
        .collect();
    Ok(Reply {
        status,
        body: raw[split + 4..].to_vec(),
        headers,
    })
}
