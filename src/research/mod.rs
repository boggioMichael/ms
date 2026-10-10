//! The data program's P1 slice: consented, rights-checked, provenance-tracked **episodes**, built
//! and exported on this machine only. See `docs/data-program/DATA_CONTRACTS.md` (this code is its
//! source of truth) and the owner's directive of 2026-10-10 (§4, §7, §10, §14, §16 P1, §17).
//!
//! **Isolated.** Nothing the player runs calls this module in P1: it is not wired into the app's
//! main loop, it reaches nothing else in the crate, and it has no network code at all — no
//! sockets, no subprocesses, no HTTP. What it writes, it writes to folders the caller names.
//!
//! The path, in order:
//!
//! 1. [`consent`] — purposes × data types × recipients, a [`consent::ConsentReceipt`] per grant
//!    (text version, time, source, scope, epoch), withdrawals and deletions in a separate, minimal
//!    ledger; a [`consent::RightsManifest`] per title (MapleStory and MapleStory Worlds:
//!    *requires title-specific review*; `synthetic`: local demonstration only). Both are checked
//!    on every recording, every flush and every exported row.
//! 2. [`recorder`] — exists only when the gate allows it (`Option`); sanitizes before anything is
//!    kept ([`sanitize`]); holds a bounded batch that a withdrawal cancels.
//! 3. [`store`] — the research folder: the sanitized event spool, the built episodes, the export
//!    lineage; deletion of a research subject through every layer.
//! 4. [`episode`] — the builder: dedupe by `event_id`, order by `sequence_no`, no later observation
//!    as an input to earlier advice, outcomes that a lost window or a closed Syrup do not turn into
//!    failures, corrections kept as claims.
//! 5. [`export`] — a local export: sanitized `events.jsonl` and `episodes.jsonl` under
//!    recipient-scoped pseudonyms, `REPORT.md`, `DATA_CARD.md` and a `manifest.json` with a SHA-256
//!    per file; [`export::verify`] (and `tools/research_loader.py`) check it.
//! 6. [`report`] — the first report: a few metrics with their numerators, denominators and
//!    participant counts.
//! 7. [`synthetic`] — a deterministic generator of synthetic participants and sessions, and the
//!    whole slice run end to end (`examples/research_slice.rs`). Never real data.

pub mod consent;
pub mod contracts;
pub mod episode;
pub mod export;
pub mod recorder;
pub mod report;
pub mod sanitize;
pub mod store;
pub mod synthetic;

pub use contracts::SCHEMA_VERSION;

/// `bytes` as lowercase hex.
pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The SHA-256 of `bytes`, as lowercase hex.
pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    hex(ring::digest::digest(&ring::digest::SHA256, bytes).as_ref())
}

/// `n` random bytes from the operating system, as hex (None if the system has none to give).
pub(crate) fn random_hex(n: usize) -> Option<String> {
    use ring::rand::{SecureRandom, SystemRandom};
    let mut bytes = vec![0u8; n];
    SystemRandom::new().fill(&mut bytes).ok()?;
    Some(hex(&bytes))
}

/// Write `text` to `path` whole or not at all: beside it first, then put in its place.
pub(crate) fn write_whole(path: &std::path::Path, text: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let suffix = random_hex(4).unwrap_or_else(|| "0".into());
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let partial = path.with_file_name(format!("{name}.{suffix}.partial"));
    let written = std::fs::write(&partial, text).and_then(|()| std::fs::rename(&partial, path));
    if written.is_err() {
        let _ = std::fs::remove_file(&partial);
    }
    written
}
