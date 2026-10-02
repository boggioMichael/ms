//! The certificate the phone link serves.
//!
//! A phone's browser opens the microphone only for a secure page, and a
//! page on a computer in the same room cannot get a certificate from a
//! public authority, so MapleSyrup makes its own the first time it runs and
//! keeps it, together with the link's secret key, next to its other
//! settings. The phone warns about it once ("not private"), and after the
//! player chooses to visit the page anyway, it works. Keeping the same
//! certificate between runs keeps that choice valid.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use rcgen::{CertificateParams, DnType, ExtendedKeyUsagePurpose, KeyPair};
use ring::rand::{SecureRandom, SystemRandom};
use rustls::ServerConfig;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};

/// A certificate and its private key, for the names it covers.
#[derive(Clone)]
pub struct Identity {
    pub cert_der: Vec<u8>,
    pub key_der: Vec<u8>,
    /// The host names and addresses the certificate was made for.
    pub names: Vec<String>,
}

/// Where MapleSyrup keeps its certificate and the link key: `%APPDATA%\MapleSyrup`
/// on Windows, `~/.config/maplesyrup` elsewhere, or the temporary directory.
pub fn settings_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("MAPLESYRUP_HOME") {
        return PathBuf::from(dir);
    }
    if cfg!(windows)
        && let Some(appdata) = std::env::var_os("APPDATA")
    {
        return PathBuf::from(appdata).join("MapleSyrup");
    }
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home).join(".config").join("maplesyrup");
    }
    std::env::temp_dir().join("maplesyrup")
}

/// The certificate in `dir` when it covers every one of `names`, otherwise
/// a new one (saved there for the next run).
pub fn load_or_create(dir: &Path, names: &[String]) -> Result<Identity, String> {
    if let Some(identity) = load(dir)
        && names.iter().all(|n| identity.names.contains(n))
    {
        return Ok(identity);
    }
    let identity = create(names)?;
    // Not being able to save only means the phone warns again next time.
    if fs::create_dir_all(dir).is_ok() {
        let _ = fs::write(dir.join("cert.der"), &identity.cert_der);
        let _ = fs::write(dir.join("key.der"), &identity.key_der);
        let _ = fs::write(dir.join("names.txt"), identity.names.join("\n"));
    }
    Ok(identity)
}

fn load(dir: &Path) -> Option<Identity> {
    let cert_der = fs::read(dir.join("cert.der")).ok()?;
    let key_der = fs::read(dir.join("key.der")).ok()?;
    let names = fs::read_to_string(dir.join("names.txt"))
        .ok()?
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(String::from)
        .collect();
    // A certificate that no longer parses as a server config is useless.
    let identity = Identity {
        cert_der,
        key_der,
        names,
    };
    server_config(&identity).ok().map(|_| identity)
}

/// A new self-signed certificate for `names` (host names or IP addresses),
/// valid for a year, as browsers on phones accept for a server.
pub fn create(names: &[String]) -> Result<Identity, String> {
    let mut names: Vec<String> = names.to_vec();
    names.sort();
    names.dedup();
    let mut params =
        CertificateParams::new(names.clone()).map_err(|e| format!("certificate names: {e}"))?;
    params
        .distinguished_name
        .push(DnType::CommonName, "MapleSyrup on this PC");
    params
        .distinguished_name
        .push(DnType::OrganizationName, "MapleSyrup");
    params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    // Phones refuse server certificates valid for more than 825 days.
    let today = chrono::Utc::now().date_naive();
    let start = today - chrono::Days::new(1);
    let end = today + chrono::Days::new(365);
    use chrono::Datelike;
    params.not_before = rcgen::date_time_ymd(start.year(), start.month() as u8, start.day() as u8);
    params.not_after = rcgen::date_time_ymd(end.year(), end.month() as u8, end.day() as u8);
    let key = KeyPair::generate().map_err(|e| format!("key: {e}"))?;
    let cert = params
        .self_signed(&key)
        .map_err(|e| format!("certificate: {e}"))?;
    Ok(Identity {
        cert_der: cert.der().to_vec(),
        key_der: key.serialize_der(),
        names,
    })
}

/// The TLS server configuration for `identity`.
pub fn server_config(identity: &Identity) -> Result<Arc<ServerConfig>, String> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let cert = CertificateDer::from(identity.cert_der.clone());
    let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(identity.key_der.clone()));
    let mut config = ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| format!("TLS: {e}"))?
        .with_no_client_auth()
        .with_single_cert(vec![cert], key)
        .map_err(|e| format!("TLS certificate: {e}"))?;
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    Ok(Arc::new(config))
}

/// A random key for the phone link, as hex: whoever does not have the link
/// cannot use the companion's microphone or commands, on the same network
/// or through a tunnel. Kept in `dir` so a saved link keeps working.
pub fn link_key(dir: &Path) -> String {
    let path = dir.join("link-key.txt");
    if let Ok(saved) = fs::read_to_string(&path) {
        let saved = saved.trim();
        if saved.len() >= 16 && saved.chars().all(|c| c.is_ascii_hexdigit()) {
            return saved.to_string();
        }
    }
    // 64 bits: unguessable, and short enough to keep the QR code small.
    let key = random_hex(8);
    if fs::create_dir_all(dir).is_ok() {
        let _ = fs::write(&path, &key);
    }
    key
}

pub fn random_hex(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    if SystemRandom::new().fill(&mut buf).is_err() {
        // The system's generator is never expected to fail; if it does, a
        // key from the clock is still unguessable enough on a LAN.
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default();
        buf = nanos.to_le_bytes()[..bytes.min(16)].to_vec();
    }
    buf.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ms-tls-{name}-{}", random_hex(4)));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn a_certificate_is_made_saved_and_reused() {
        let dir = temp_dir("reuse");
        let names = vec!["192.168.1.20".to_string(), "localhost".to_string()];
        let first = load_or_create(&dir, &names).unwrap();
        assert!(server_config(&first).is_ok());
        let second = load_or_create(&dir, &names).unwrap();
        assert_eq!(first.cert_der, second.cert_der, "the saved one is reused");
        // A new address needs a new certificate.
        let third = load_or_create(&dir, &["10.0.0.5".to_string()]).unwrap();
        assert_ne!(first.cert_der, third.cert_der);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_link_key_is_random_and_kept() {
        let dir = temp_dir("key");
        let key = link_key(&dir);
        assert_eq!(key.len(), 16);
        assert_eq!(link_key(&dir), key);
        assert_ne!(random_hex(12), random_hex(12));
        let _ = fs::remove_dir_all(&dir);
    }
}
