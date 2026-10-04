//! The whole way of an update, against a release laid out as the pipeline
//! lays it out, served from a folder (`file://`): the manifest fetched and
//! verified, the program fetched and staged, activated at the next start,
//! and the previous program kept beside it.

use ms::update::{Start, Updater, at_start, commit, sha256_of};
use ring::rand::SystemRandom;
use ring::signature::{Ed25519KeyPair, KeyPair};
use std::fs;
use std::path::{Path, PathBuf};

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ms-channel-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn file_url(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    if text.starts_with('/') {
        format!("file://{text}")
    } else {
        format!("file:///{text}")
    }
}

/// A release folder the way the pipeline makes one: the program under its
/// version's name, a manifest naming it, and the manifest's signature.
fn release(dir: &Path, version: &str, body: &[u8], pair: &Ed25519KeyPair) {
    let name = format!("MapleSyrup-{version}.exe");
    let path = dir.join(&name);
    fs::write(&path, body).unwrap();
    let manifest = serde_json::json!({
        "name": "MapleSyrup",
        "version": version,
        "published": "2026-10-04",
        "commit": "abc",
        "notes": "the dog learned a trick",
        "files": [{
            "name": name,
            "kind": "exe",
            "size": body.len(),
            "sha256": sha256_of(&path).unwrap(),
            "url": file_url(&path),
        }],
    });
    let bytes = serde_json::to_vec_pretty(&manifest).unwrap();
    fs::write(dir.join("manifest.json.sig"), pair.sign(&bytes).as_ref()).unwrap();
    fs::write(dir.join("manifest.json"), bytes).unwrap();
}

#[test]
fn a_release_on_the_channel_is_fetched_staged_and_activated_at_the_next_start() {
    let dir = temp("whole-way");
    let site = dir.join("site");
    fs::create_dir_all(&site).unwrap();
    let pkcs8 = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).unwrap();
    let pair = Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).unwrap();
    let public = pair.public_key().as_ref().to_vec();
    release(&site, "0.9.0", b"MZ..the new program", &pair);

    let settings = dir.join("settings");
    let exe = dir.join("MapleSyrup.exe");
    fs::write(&exe, b"MZ..the old program").unwrap();
    let channel = file_url(&site.join("manifest.json"));
    let updater = Updater::with(&settings, "0.8.0", true, &channel, &public);
    assert_eq!(updater.status().to_json()["state"], "up-to-date");

    // One look at the channel: fetched, verified, staged.
    updater.check_once().expect("the release is taken");
    let status = updater.status().to_json();
    assert_eq!(status["state"], "staged", "{status}");
    assert_eq!(status["latest"], "0.9.0");
    assert_eq!(status["notes"], "the dog learned a trick");
    let staged = updater.store().staged().expect("staged");
    assert_eq!(staged.version, "0.9.0");
    assert_eq!(fs::read(&staged.file).unwrap(), b"MZ..the new program");
    assert!(
        !updater
            .store()
            .dir()
            .join("MapleSyrup-0.9.0.exe.part")
            .exists()
    );

    // Looking again fetches nothing: it is already staged.
    updater.check_once().unwrap();
    assert_eq!(updater.status().to_json()["state"], "staged");

    // The next start puts it in place and keeps the old one beside it.
    assert_eq!(
        at_start(&settings, &exe, "0.8.0"),
        Start::Relaunch(exe.clone())
    );
    assert_eq!(fs::read(&exe).unwrap(), b"MZ..the new program");
    assert_eq!(
        fs::read(dir.join("MapleSyrup.old.exe")).unwrap(),
        b"MZ..the old program"
    );
    // The new program starts, runs well, is committed.
    assert_eq!(at_start(&settings, &exe, "0.9.0"), Start::CarryOn);
    assert_eq!(commit(&settings).as_deref(), Some("0.9.0"));
    assert!(!dir.join("MapleSyrup.old.exe").exists());

    // The new program sees the same release: up to date.
    let updater = Updater::with(&settings, "0.9.0", true, &channel, &public);
    updater.check_once().unwrap();
    assert_eq!(updater.status().to_json()["state"], "up-to-date");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_release_signed_by_another_key_or_tampered_with_is_refused() {
    let dir = temp("refused");
    let site = dir.join("site");
    fs::create_dir_all(&site).unwrap();
    let pkcs8 = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).unwrap();
    let pair = Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).unwrap();
    release(&site, "0.9.0", b"MZ..the new program", &pair);
    let settings = dir.join("settings");
    let channel = file_url(&site.join("manifest.json"));

    // Not the key built in.
    let other = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).unwrap();
    let other = Ed25519KeyPair::from_pkcs8(other.as_ref()).unwrap();
    let updater = Updater::with(
        &settings,
        "0.8.0",
        true,
        &channel,
        other.public_key().as_ref(),
    );
    let why = updater.check_once().unwrap_err();
    assert!(why.contains("signature"), "{why}");
    assert!(updater.store().staged().is_none());

    // The right key, but the program on the channel is not the one the
    // manifest describes.
    let public = pair.public_key().as_ref().to_vec();
    fs::write(site.join("MapleSyrup-0.9.0.exe"), b"MZ..something else!").unwrap();
    let updater = Updater::with(&settings, "0.8.0", true, &channel, &public);
    let why = updater.check_once().unwrap_err();
    assert!(why.contains("hash"), "{why}");
    assert!(updater.store().staged().is_none());
    assert!(
        !updater
            .store()
            .dir()
            .join("MapleSyrup-0.9.0.exe.part")
            .exists()
    );
    let _ = fs::remove_dir_all(&dir);
}
