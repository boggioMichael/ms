//! The certificate the phone link serves.
//!
//! A phone's browser opens the microphone only for a secure page, and a
//! page on a computer in the same room cannot get a certificate from a
//! public authority, so MapleSyrup is its own: the first time it runs it
//! makes a certificate authority of its own on this PC ([`Authority`]),
//! and signs the phone link's certificate with it. Until the phone trusts
//! that authority it warns ("not private") and the player chooses to visit
//! the page anyway; trusted once (the page's "Trust this PC": a profile
//! installed and turned on), the link opens with no warning from then on,
//! whatever address the PC gets on the home network. The authority can
//! sign for this network only — private addresses and local names (name
//! constraints) — so trusting it trusts nothing else. Everything is kept,
//! with the link's secret key, next to MapleSyrup's other settings.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use rcgen::{
    BasicConstraints, CertificateParams, CidrSubnet, DnType, ExtendedKeyUsagePurpose,
    GeneralSubtree, IsCa, KeyPair, KeyUsagePurpose, NameConstraints,
};
use ring::rand::{SecureRandom, SystemRandom};
use rustls::ServerConfig;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};

/// A day as a certificate's time (its start, UTC).
macro_rules! day {
    ($day:expr) => {{
        use chrono::Datelike;
        let day: chrono::NaiveDate = $day;
        rcgen::date_time_ymd(day.year(), day.month() as u8, day.day() as u8)
    }};
}

/// A certificate and its private key, for the names it covers, signed by
/// MapleSyrup's authority on this PC.
#[derive(Clone)]
pub struct Identity {
    pub cert_der: Vec<u8>,
    pub key_der: Vec<u8>,
    /// The host names and addresses the certificate was made for.
    pub names: Vec<String>,
    /// The authority's certificate: what the phone trusts once.
    pub ca_der: Vec<u8>,
}

/// MapleSyrup's own certificate authority on this PC.
pub struct Authority {
    pub cert_der: Vec<u8>,
    key: KeyPair,
    /// Its name ("MapleSyrup on DESKTOP-…"), as the phone shows it.
    pub name: String,
}

/// How long the link's certificate is valid: phones refuse server
/// certificates valid for more than 825 days (398 for the public ones).
const CERT_DAYS: u64 = 397;
/// A certificate older than this is made again (well before it ends).
const CERT_RENEW_DAYS: i64 = 300;

/// The authority's description: what it may sign for — private addresses
/// and local names only.
fn authority_params(name: &str) -> CertificateParams {
    let mut params = CertificateParams::default();
    params.distinguished_name.push(DnType::CommonName, name);
    params
        .distinguished_name
        .push(DnType::OrganizationName, "MapleSyrup");
    params.is_ca = IsCa::Ca(BasicConstraints::Constrained(0));
    params.key_usages = vec![
        KeyUsagePurpose::KeyCertSign,
        KeyUsagePurpose::CrlSign,
        KeyUsagePurpose::DigitalSignature,
    ];
    let net = |a, b, c, d, prefix| {
        GeneralSubtree::IpAddress(CidrSubnet::from_v4_prefix([a, b, c, d], prefix))
    };
    params.name_constraints = Some(NameConstraints {
        permitted_subtrees: vec![
            net(10, 0, 0, 0, 8),
            net(172, 16, 0, 0, 12),
            net(192, 168, 0, 0, 16),
            net(169, 254, 0, 0, 16),
            net(100, 64, 0, 0, 10),
            net(127, 0, 0, 0, 8),
            GeneralSubtree::DnsName("localhost".into()),
            GeneralSubtree::DnsName("local".into()),
        ],
        excluded_subtrees: Vec::new(),
    });
    let today = chrono::Utc::now().date_naive();
    params.not_before = day!(today - chrono::Days::new(1));
    params.not_after = day!(today + chrono::Days::new(3650));
    params
}

/// Whether the authority may sign for `name` (see [`authority_params`]):
/// a private or local address, `localhost`, or a `.local` name. A
/// certificate with any other name in it is refused whole by a phone that
/// checks the authority's limits, so others are left out of it.
pub fn permitted(name: &str) -> bool {
    if let Ok(ip) = name.parse::<std::net::Ipv4Addr>() {
        let [a, b, ..] = ip.octets();
        return a == 10
            || a == 127
            || (a == 172 && (16..32).contains(&b))
            || (a == 192 && b == 168)
            || (a == 169 && b == 254)
            || (a == 100 && (64..128).contains(&b));
    }
    let name = name.to_ascii_lowercase();
    name == "localhost" || name == "local" || name.ends_with(".local")
}

/// The authority kept in `dir`, or a new one (saved there).
pub fn authority(dir: &Path) -> Result<Authority, String> {
    let saved = (|| {
        let cert_der = fs::read(dir.join("ca.der")).ok()?;
        let key = KeyPair::try_from(fs::read(dir.join("ca-key.der")).ok()?).ok()?;
        let name = fs::read_to_string(dir.join("ca-name.txt")).ok()?;
        Some(Authority {
            cert_der,
            key,
            name: name.trim().to_string(),
        })
    })();
    if let Some(authority) = saved.filter(|a| !a.name.is_empty()) {
        return Ok(authority);
    }
    let pc: String = std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_default()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .take(30)
        .collect();
    let name = if pc.is_empty() {
        format!("MapleSyrup {}", random_hex(3))
    } else {
        format!("MapleSyrup on {pc}")
    };
    let key = KeyPair::generate().map_err(|e| format!("key: {e}"))?;
    let cert = authority_params(&name)
        .self_signed(&key)
        .map_err(|e| format!("authority: {e}"))?;
    let authority = Authority {
        cert_der: cert.der().to_vec(),
        key,
        name,
    };
    // Not being able to save only means a new one (to trust again) next time.
    if fs::create_dir_all(dir).is_ok() {
        let _ = fs::write(dir.join("ca-key.der"), authority.key.serialize_der());
        let _ = fs::write(dir.join("ca-name.txt"), &authority.name);
        let _ = fs::write(dir.join("ca.der"), &authority.cert_der);
    }
    Ok(authority)
}

/// The authority's fingerprint: which one signed a saved certificate.
fn fingerprint(der: &[u8]) -> String {
    ring::digest::digest(&ring::digest::SHA256, der)
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
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

/// The certificate in `dir` when it covers every one of `names`, was
/// signed by the authority there and is not old, otherwise a new one (saved
/// there for the next run).
pub fn load_or_create(dir: &Path, names: &[String]) -> Result<Identity, String> {
    let authority = authority(dir)?;
    if let Some(identity) = load(dir, &authority)
        && names
            .iter()
            .filter(|n| permitted(n))
            .all(|n| identity.names.contains(n))
    {
        return Ok(identity);
    }
    let identity = create(names, &authority)?;
    // Not being able to save only means a new certificate next time.
    if fs::create_dir_all(dir).is_ok() {
        let _ = fs::write(dir.join("cert.der"), &identity.cert_der);
        let _ = fs::write(dir.join("key.der"), &identity.key_der);
        let _ = fs::write(dir.join("names.txt"), identity.names.join("\n"));
        let today = chrono::Utc::now().date_naive();
        let _ = fs::write(
            dir.join("cert-ca.txt"),
            format!("{}\n{today}\n", fingerprint(&authority.cert_der)),
        );
    }
    Ok(identity)
}

fn load(dir: &Path, authority: &Authority) -> Option<Identity> {
    // Signed by this authority, and not old.
    let made = fs::read_to_string(dir.join("cert-ca.txt")).ok()?;
    let mut lines = made.lines();
    if lines.next()? != fingerprint(&authority.cert_der) {
        return None;
    }
    let day: chrono::NaiveDate = lines.next()?.trim().parse().ok()?;
    if (chrono::Utc::now().date_naive() - day).num_days() > CERT_RENEW_DAYS {
        return None;
    }
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
        ca_der: authority.cert_der.clone(),
    };
    server_config(&identity).ok().map(|_| identity)
}

/// A new certificate for `names` (host names or IP addresses), signed by
/// `authority`, valid for [`CERT_DAYS`].
pub fn create(names: &[String], authority: &Authority) -> Result<Identity, String> {
    let mut names: Vec<String> = names.iter().filter(|n| permitted(n)).cloned().collect();
    if names.is_empty() {
        names.push("localhost".to_string());
    }
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
    params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
    params.use_authority_key_identifier_extension = true;
    let today = chrono::Utc::now().date_naive();
    params.not_before = day!(today - chrono::Days::new(1));
    params.not_after = day!(today + chrono::Days::new(CERT_DAYS - 1));
    let key = KeyPair::generate().map_err(|e| format!("key: {e}"))?;
    // The authority as an issuer: its name and key (as it was made).
    let issuer = authority_params(&authority.name)
        .self_signed(&authority.key)
        .map_err(|e| format!("authority: {e}"))?;
    let cert = params
        .signed_by(&key, &issuer, &authority.key)
        .map_err(|e| format!("certificate: {e}"))?;
    Ok(Identity {
        cert_der: cert.der().to_vec(),
        key_der: key.serialize_der(),
        names,
        ca_der: authority.cert_der.clone(),
    })
}

/// The TLS server configuration for `identity`.
pub fn server_config(identity: &Identity) -> Result<Arc<ServerConfig>, String> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    // The certificate with its authority after it.
    let chain = vec![
        CertificateDer::from(identity.cert_der.clone()),
        CertificateDer::from(identity.ca_der.clone()),
    ];
    let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(identity.key_der.clone()));
    let mut config = ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| format!("TLS: {e}"))?
        .with_no_client_auth()
        .with_single_cert(chain, key)
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

    /// Whether a browser that trusts `ca` accepts `cert` for `name` (the
    /// web PKI's own checks: the chain, the dates, the names and the
    /// authority's name constraints).
    fn trusted(ca: &[u8], cert: &[u8], name: &str) -> Result<(), String> {
        use rustls::client::danger::ServerCertVerifier;
        use rustls::pki_types::{ServerName, UnixTime};
        let mut roots = rustls::RootCertStore::empty();
        roots
            .add(CertificateDer::from(ca.to_vec()))
            .map_err(|e| e.to_string())?;
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let verifier =
            rustls::client::WebPkiServerVerifier::builder_with_provider(Arc::new(roots), provider)
                .build()
                .map_err(|e| e.to_string())?;
        let name = ServerName::try_from(name.to_string()).map_err(|e| e.to_string())?;
        verifier
            .verify_server_cert(
                &CertificateDer::from(cert.to_vec()),
                &[],
                &name,
                &[],
                UnixTime::now(),
            )
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    #[test]
    fn a_phone_that_trusts_the_authority_trusts_the_link() {
        let dir = temp_dir("authority");
        let names = vec![
            "192.168.1.20".to_string(),
            "localhost".to_string(),
            "127.0.0.1".to_string(),
        ];
        let identity = load_or_create(&dir, &names).unwrap();
        for name in ["192.168.1.20", "localhost", "127.0.0.1"] {
            trusted(&identity.ca_der, &identity.cert_der, name)
                .unwrap_or_else(|e| panic!("{name}: {e}"));
        }
        assert!(trusted(&identity.ca_der, &identity.cert_der, "192.168.1.21").is_err());
        // The authority is kept: a certificate for a new address (the PC
        // got another one) is trusted as well, with nothing to do again.
        let moved = load_or_create(&dir, &["10.0.0.7".to_string()]).unwrap();
        assert_eq!(moved.ca_der, identity.ca_der);
        trusted(&moved.ca_der, &moved.cert_der, "10.0.0.7").unwrap();
        let authority = authority(&dir).unwrap();
        assert!(
            authority.name.starts_with("MapleSyrup"),
            "{}",
            authority.name
        );
        // It signs for this network only: a certificate it made for a
        // public name or address is not trusted (and it makes none: such a
        // name is left out).
        let issuer = authority_params(&authority.name)
            .self_signed(&authority.key)
            .unwrap();
        for outside in ["example.com", "8.8.8.8"] {
            assert!(!permitted(outside));
            let key = KeyPair::generate().unwrap();
            let mut params = CertificateParams::new(vec![outside.to_string()]).unwrap();
            params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
            let foreign = params.signed_by(&key, &issuer, &authority.key).unwrap();
            assert!(
                trusted(&identity.ca_der, foreign.der(), outside).is_err(),
                "{outside} must be outside what it may sign for"
            );
        }
        let left_out = create(&["8.8.8.8".into(), "192.168.0.9".into()], &authority).unwrap();
        assert_eq!(left_out.names, ["192.168.0.9"]);
        assert!(permitted("10.0.0.3") && permitted("desktop.local") && !permitted("192.0.2.2"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_old_certificate_or_one_from_another_authority_is_made_again() {
        let dir = temp_dir("renew");
        let names = vec!["192.168.1.20".to_string()];
        let first = load_or_create(&dir, &names).unwrap();
        // Made long ago: made again (by the same authority).
        let made = fs::read_to_string(dir.join("cert-ca.txt")).unwrap();
        let print = made.lines().next().unwrap().to_string();
        fs::write(dir.join("cert-ca.txt"), format!("{print}\n2020-01-01\n")).unwrap();
        let renewed = load_or_create(&dir, &names).unwrap();
        assert_ne!(renewed.cert_der, first.cert_der);
        assert_eq!(renewed.ca_der, first.ca_der);
        // One the authority here did not sign (a certificate from before
        // there was one): made again.
        fs::write(dir.join("cert-ca.txt"), "0000\n2099-01-01\n").unwrap();
        let again = load_or_create(&dir, &names).unwrap();
        assert_ne!(again.cert_der, renewed.cert_der);
        trusted(&again.ca_der, &again.cert_der, "192.168.1.20").unwrap();
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
