//! The local export: what a recipient would get, written to a folder on this machine.
//!
//! ```text
//! events.jsonl     sanitized events, every row eligible for this purpose and recipient
//! episodes.jsonl   episodes built from those rows only
//! REPORT.md        the first report on those episodes
//! DATA_CARD.md     what it is, intended and prohibited uses, limits, retention, deletion
//! manifest.json    SHA-256, bytes and rows of each file; schema; counts; consent and rights summary
//! ```
//!
//! Every row is checked, at the time of the export: the title's rights (approved, in force,
//! licensing this purpose for this recipient class), then the subject's consent (in force, and the
//! receipt it was collected under, both covering the purpose, the data type and the recipient).
//! A row that fails is left out and counted by reason; when none passes, there is no export and
//! no folder. Texts are sanitized again. Identifiers are replaced by pseudonyms keyed to the
//! recipient (HMAC-SHA-256 with the recipient's own key), so that two recipients' copies share no
//! key to join them; the link back lives only in the producer's lineage. The manifest names no
//! participant and no path.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::research::consent::{
    ConsentLedger, LedgerEntry, Purpose, Recipient, RecipientClass, Refusal, RightsRegistry,
    RightsStatus,
};
use crate::research::contracts::{Event, GameId, SCHEMA_VERSION};
use crate::research::episode::{self, Episode};
use crate::research::report::{self, Metrics, ReportContext};
use crate::research::sanitize::{Sanitizer, Tally};
use crate::research::store::{ExportLineage, ResearchStore};
use crate::research::{hex, sha256_hex, write_whole};

/// The manifest's own format.
pub const MANIFEST_VERSION: u32 = 1;

/// How the recipient's pseudonym key is chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyChoice {
    /// Random, kept in the producer's lineage folder: for anything real.
    Random,
    /// Derived from the recipient's id (guessable): synthetic data only, for reproducible output.
    SyntheticFixed,
}

#[derive(Debug, Clone)]
pub struct ExportRequest {
    pub export_id: String,
    pub purpose: Purpose,
    pub recipient: Recipient,
    pub keys: KeyChoice,
}

impl ExportRequest {
    pub fn new(export_id: &str, purpose: Purpose, recipient: Recipient) -> ExportRequest {
        ExportRequest {
            export_id: export_id.into(),
            purpose,
            recipient,
            keys: KeyChoice::Random,
        }
    }

    /// A request whose pseudonyms are reproducible: refused unless every row is synthetic.
    pub fn synthetic(export_id: &str, purpose: Purpose, recipient: Recipient) -> ExportRequest {
        ExportRequest {
            keys: KeyChoice::SyntheticFixed,
            ..ExportRequest::new(export_id, purpose, recipient)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileEntry {
    pub path: String,
    pub sha256: String,
    pub bytes: u64,
    /// Lines, for a `.jsonl` file.
    pub rows: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Counts {
    pub participants: usize,
    pub sessions: usize,
    pub episodes: usize,
    pub events: usize,
    /// Hours between a session's start and end, for the sessions where both are in the export.
    pub observed_hours: f64,
    pub sessions_without_known_duration: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RightsSummary {
    pub rights_policy_id: String,
    pub game_id: GameId,
    pub status: RightsStatus,
    pub rights_holder: String,
    pub recipients_licensed: Vec<RecipientClass>,
    pub valid_until: DateTime<Utc>,
}

/// What a buyer's procurement needs to know of consent and rights — without any participant.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConsentRightsSummary {
    /// `text_id version language sha256:…` of each consent text the rows were collected under.
    pub consent_texts: Vec<String>,
    pub age_assurance: Vec<String>,
    pub rights: Vec<RightsSummary>,
    pub checked_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    pub manifest_version: u32,
    pub schema_version: String,
    pub export_id: String,
    pub revision: u32,
    pub created_at: DateTime<Utc>,
    pub synthetic: bool,
    pub purpose: Purpose,
    pub recipient_class: RecipientClass,
    pub recipient_id: String,
    pub files: Vec<FileEntry>,
    pub counts: Counts,
    pub consent_and_rights: ConsentRightsSummary,
    /// Rows left out, by reason (counts only).
    pub excluded: BTreeMap<String, usize>,
    /// What the episode builder saw (duplicates, quarantine, gaps…).
    pub quality: BTreeMap<String, usize>,
    pub deletions_applied: u32,
    pub loader: String,
}

/// An export made.
#[derive(Debug, Clone)]
pub struct Exported {
    pub dir: PathBuf,
    pub manifest: Manifest,
    pub metrics: Metrics,
    pub redactions: Tally,
}

/// The pseudonym of `id` (of `kind`) under `key`.
fn pseudonym(key: &[u8], kind: &str, id: &str) -> String {
    let key = ring::hmac::Key::new(ring::hmac::HMAC_SHA256, key);
    let tag = ring::hmac::sign(&key, format!("{kind}\u{0}{id}").as_bytes());
    format!("{kind}-{}", &hex(tag.as_ref())[..20])
}

/// The identifier keys of the contract and the pseudonym kind of each.
fn id_kind(key: &str) -> Option<&'static str> {
    Some(match key {
        "research_subject_id" => "p",
        "session_id" | "session_ids" => "s",
        "episode_id" => "ep",
        "consent_receipt_id" | "consent_receipt_ids" => "c",
        "event_id" | "event_ids" | "in_reply_to" | "based_on" | "follows_advice"
        | "target_event_id" | "about_event_id" | "evidence_event_id" => "e",
        _ => return None,
    })
}

/// Every identifier in `value`, replaced by its pseudonym.
fn pseudonymize(value: &mut Value, key: &[u8]) {
    match value {
        Value::Object(map) => {
            for (k, v) in map.iter_mut() {
                match (id_kind(k), v) {
                    (Some(kind), Value::String(s)) => *s = pseudonym(key, kind, s),
                    (Some(kind), Value::Array(items)) => {
                        for item in items.iter_mut() {
                            if let Value::String(s) = item {
                                *s = pseudonym(key, kind, s);
                            }
                        }
                    }
                    (_, v) => pseudonymize(v, key),
                }
            }
        }
        Value::Array(items) => items.iter_mut().for_each(|v| pseudonymize(v, key)),
        _ => {}
    }
}

fn jsonl(rows: &[Value]) -> Result<String, Refusal> {
    let mut text = String::new();
    for row in rows {
        text.push_str(&serde_json::to_string(row).map_err(|e| Refusal::Io(e.to_string()))?);
        text.push('\n');
    }
    Ok(text)
}

fn counts(events: &[Value], episodes: &[Value]) -> Counts {
    let set = |rows: &[Value], key: &str| {
        rows.iter()
            .filter_map(|r| r.get(key).and_then(Value::as_str).map(String::from))
            .collect::<BTreeSet<String>>()
    };
    let mut starts: BTreeMap<String, u64> = BTreeMap::new();
    let mut ends: BTreeMap<String, u64> = BTreeMap::new();
    for event in events {
        let session = event["session_id"].as_str().unwrap_or("").to_string();
        let ms = event["monotonic_timestamp"].as_u64();
        if let (Some(action), Some(ms)) = (event["payload"]["session"]["action"].as_str(), ms) {
            match action {
                "start" => {
                    starts.insert(session, ms);
                }
                "end" => {
                    ends.insert(session, ms);
                }
                _ => {}
            }
        }
    }
    let sessions = set(events, "session_id");
    let mut known_ms = 0u64;
    let mut unknown = 0;
    for session in &sessions {
        match (starts.get(session), ends.get(session)) {
            (Some(s), Some(e)) if e >= s => known_ms += e - s,
            _ => unknown += 1,
        }
    }
    let mut participants = set(events, "research_subject_id");
    participants.extend(set(episodes, "research_subject_id"));
    Counts {
        participants: participants.len(),
        sessions: sessions.len(),
        episodes: episodes.len(),
        events: events.len(),
        observed_hours: (known_ms as f64 / 3_600_000.0 * 10_000.0).round() / 10_000.0,
        sessions_without_known_duration: unknown,
    }
}

/// A value's name in the contract (its serialized form).
fn code<T: Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|v| v.as_str().map(String::from))
        .unwrap_or_default()
}

fn data_card(manifest: &Manifest, metrics: &Metrics) -> String {
    let c = &manifest.counts;
    let mut out = String::new();
    let label = if manifest.synthetic {
        " (SYNTHETIC)"
    } else {
        ""
    };
    out.push_str(&format!(
        "# Data card — `{}`{label}\n\n",
        manifest.export_id
    ));
    if manifest.synthetic {
        out.push_str("> **SYNTHETIC.** Generated by `research::synthetic` for local demonstration: no game footage, \
                      no player, no real participant. Not for training, evaluation or any claim about a game.\n\n");
    }
    out.push_str(&format!(
        "Revision {} · schema {} · created {} · purpose `{}` · recipient class `{}`\n\n",
        manifest.revision,
        manifest.schema_version,
        manifest.created_at.to_rfc3339(),
        manifest.purpose.code(),
        manifest.recipient_class.code()
    ));
    out.push_str("## What it is\n\nEpisodes of people using a game companion: a goal, the observed state, requests for help, \
                  the assistant's advice (with its sources, model version and display time), the player's actions as \
                  far as they are known, the outcome and how it was determined, and corrections kept as claims. Every \
                  observation, action, label and outcome carries its provenance (`source_type`, `confidence` only when \
                  calibrated, `evidence_ref`, `annotator_type`, `verification_status`, `producer_version`). Unknown \
                  values are `null`, never 0. Field definitions: `docs/data-program/DATA_CONTRACTS.md`.\n\n");
    out.push_str("## Contents\n\n| File | Rows | SHA-256 |\n|---|---|---|\n");
    for file in &manifest.files {
        out.push_str(&format!(
            "| `{}` | {} | `{}` |\n",
            file.path,
            file.rows.map_or("—".into(), |r| r.to_string()),
            file.sha256
        ));
    }
    out.push_str("\n(This card's own checksum is in `manifest.json`.)\n\n");
    out.push_str(&format!(
        "## Counts\n\n{} participants · {} sessions · {} episodes · {} events · {} observed hours \
         ({} sessions without a known duration).\nOutcomes: {} success, {} failure, {} aborted, {} unobserved, {} censored.\n\n",
        c.participants,
        c.sessions,
        c.episodes,
        c.events,
        c.observed_hours,
        c.sessions_without_known_duration,
        metrics.successes,
        metrics.failures,
        metrics.aborted,
        metrics.unobserved_rate.numerator,
        metrics.censored_rate.numerator
    ));
    out.push_str("## Consent and rights\n\n");
    for text in &manifest.consent_and_rights.consent_texts {
        out.push_str(&format!("- Consent text: `{text}`\n"));
    }
    for rights in &manifest.consent_and_rights.rights {
        out.push_str(&format!(
            "- Rights `{}` (`{}`, `{}`): {}; licensed to {}; valid until {}\n",
            rights.rights_policy_id,
            code(&rights.game_id),
            code(&rights.status),
            rights.rights_holder,
            rights
                .recipients_licensed
                .iter()
                .map(|r| format!("`{}`", r.code()))
                .collect::<Vec<_>>()
                .join(", "),
            rights.valid_until.to_rfc3339()
        ));
    }
    out.push_str(&format!(
        "- Checked for every row at {}. Rows left out: {}.\n\n",
        manifest.consent_and_rights.checked_at.to_rfc3339(),
        if manifest.excluded.is_empty() {
            "none".to_string()
        } else {
            manifest
                .excluded
                .iter()
                .map(|(k, v)| format!("{k} {v}"))
                .collect::<Vec<_>>()
                .join(", ")
        }
    ));
    out.push_str("## Intended uses\n\n");
    if manifest.synthetic {
        out.push_str("- Testing loaders, schemas and pipelines; demonstrating the format to a prospective recipient.\n\n");
    } else {
        out.push_str(&format!(
            "- The purpose consented to and licensed: `{}`, by the recipient named in the agreement.\n\n",
            manifest.purpose.code()
        ));
    }
    out.push_str("## Prohibited uses\n\n- Re-identifying anyone, or linking these rows with any other data about a person.\n\
                  - Profiling a person (spending, ability to pay, \"whales\", mood or traits inferred from face, voice or play).\n\
                  - Any purpose or recipient other than the above; passing the data on.\n\
                  - Treating `model_inferred` or `human_asserted` labels as ground truth, or a synthetic row as a real one.\n\
                  - Training or evaluating on episodes whose outcome is `unobserved` or `censored` as if they were failures.\n\n");
    out.push_str("## Limits and known biases\n\n- Participants are companion users who opted in: not a game's population.\n\
                  - No input is captured: player actions are self-reported or inferred, never a recording of input.\n\
                  - Advice shown is not advice heard or followed; assisted and unassisted attempts differ in who asked.\n\
                  - Identifiers are pseudonyms scoped to this recipient: pseudonymous, not anonymous.\n");
    if metrics.insufficient_evidence {
        out.push_str(&format!(
            "- {} participant(s): insufficient for inference (see `REPORT.md`).\n",
            metrics.participants
        ));
    }
    out.push_str("\n## Retention and deletion\n\nA participant who withdraws stops all collection at once and cancels \
                  what was pending. A deletion removes their events, episodes and rows of every export the producer's \
                  lineage lists; the export's revision goes up and its files, counts and checksums are made again. \
                  Copies already delivered are deleted by the recipient under the agreement. Deleting rows does not \
                  undo a model already trained on them. Retention periods are proposals for the owner and counsel \
                  (`docs/data-program/CONSENT_AND_RIGHTS.md`).\n\n");
    out.push_str(&format!(
        "## Loading\n\n`python3 {} <this folder>` verifies every checksum and refuses on a mismatch, then loads the rows \
         (standard library only). Keep `null` as missing.\n",
        manifest.loader
    ));
    out
}

/// Write the export's files into `dir` and return its manifest (written last).
fn write_files(
    dir: &Path,
    events: &[Value],
    episodes: &[Value],
    template: &Manifest,
    metrics: &Metrics,
) -> Result<Manifest, Refusal> {
    let io = |e: std::io::Error| Refusal::Io(e.to_string());
    let events_text = jsonl(events)?;
    let episodes_text = jsonl(episodes)?;
    let report_text = report::render(
        metrics,
        &ReportContext {
            synthetic: template.synthetic,
            export_id: template.export_id.clone(),
            revision: template.revision,
            purpose: template.purpose,
            as_of: template.created_at,
            schema_version: template.schema_version.clone(),
        },
    );
    let entry = |path: &str, text: &str, rows: Option<usize>| FileEntry {
        path: path.into(),
        sha256: sha256_hex(text.as_bytes()),
        bytes: text.len() as u64,
        rows,
    };
    let mut manifest = template.clone();
    manifest.counts = counts(events, episodes);
    manifest.files = vec![
        entry("events.jsonl", &events_text, Some(events.len())),
        entry("episodes.jsonl", &episodes_text, Some(episodes.len())),
        entry("REPORT.md", &report_text, None),
    ];
    let card_text = data_card(&manifest, metrics);
    manifest.files.push(entry("DATA_CARD.md", &card_text, None));
    std::fs::create_dir_all(dir).map_err(io)?;
    for (name, text) in [
        ("events.jsonl", &events_text),
        ("episodes.jsonl", &episodes_text),
        ("REPORT.md", &report_text),
        ("DATA_CARD.md", &card_text),
    ] {
        write_whole(&dir.join(name), text).map_err(io)?;
    }
    let mut manifest_text =
        serde_json::to_string_pretty(&manifest).map_err(|e| Refusal::Io(e.to_string()))?;
    manifest_text.push('\n');
    write_whole(&dir.join("manifest.json"), &manifest_text).map_err(io)?;
    Ok(manifest)
}

/// Export what may go to `request.recipient` for `request.purpose`, into `out_dir` (which must
/// not exist yet, or be empty).
pub fn export_local(
    store: &ResearchStore,
    ledger: &ConsentLedger,
    rights: &RightsRegistry,
    request: &ExportRequest,
    out_dir: &Path,
    now: DateTime<Utc>,
) -> Result<Exported, Refusal> {
    let io = |e: std::io::Error| Refusal::Io(e.to_string());
    if std::fs::read_dir(out_dir).is_ok_and(|mut d| d.next().is_some()) {
        return Err(Refusal::ExportExists(out_dir.to_path_buf()));
    }
    let (events, quality) = episode::prepare(&store.read_events().map_err(io)?);
    let state = ledger.state().map_err(io)?;
    let class = request.recipient.class;
    let mut excluded: BTreeMap<String, usize> = BTreeMap::new();
    let mut eligible: Vec<Event> = Vec::new();
    for event in events {
        let e = &event.envelope;
        let verdict = rights
            .check_export(e.game_id, request.purpose, class, now)
            .map(|_| ())
            .and_then(|()| {
                state.check_export(
                    &e.research_subject_id,
                    &e.consent_receipt_id,
                    request.purpose,
                    event.payload.data_type(),
                    class,
                )
            });
        match verdict {
            Ok(()) => eligible.push(event),
            Err(refusal) => *excluded.entry(refusal.code().into()).or_insert(0) += 1,
        }
    }
    if eligible.is_empty() {
        return Err(Refusal::NothingEligible(excluded));
    }
    let synthetic = eligible
        .iter()
        .all(|e| e.envelope.game_id == GameId::Synthetic);
    let key = match request.keys {
        KeyChoice::SyntheticFixed if !synthetic => return Err(Refusal::RealDataNeedsRandomKeys),
        KeyChoice::SyntheticFixed => ring::digest::digest(
            &ring::digest::SHA256,
            format!("synthetic-recipient-key/{}", request.recipient.id).as_bytes(),
        )
        .as_ref()
        .to_vec(),
        KeyChoice::Random => store.recipient_key(&request.recipient.id).map_err(io)?,
    };

    // Sanitize again (the last net before anything leaves the research folder).
    let mut redactions = Tally::new();
    let last_net = Sanitizer::default();
    for event in &mut eligible {
        last_net.payload(&mut event.payload, &mut redactions);
    }
    let built = episode::build(&eligible);
    let metrics = report::metrics(&built.episodes);

    let mut subjects: BTreeMap<String, String> = BTreeMap::new();
    let mut texts = BTreeSet::new();
    let mut ages = BTreeSet::new();
    let mut policies = BTreeSet::new();
    for event in &eligible {
        let subject = &event.envelope.research_subject_id;
        subjects
            .entry(subject.clone())
            .or_insert_with(|| pseudonym(&key, "p", subject));
        if let Some(receipt) = state.receipts.get(&event.envelope.consent_receipt_id) {
            texts.insert(format!(
                "{} {} {} sha256:{}",
                receipt.text_id, receipt.text_version, receipt.text_language, receipt.text_sha256
            ));
            ages.insert(code(&receipt.age_assurance));
        }
        policies.insert(event.envelope.game_id);
    }
    let to_values = |rows: Vec<Value>| -> Vec<Value> {
        rows.into_iter()
            .map(|mut v| {
                pseudonymize(&mut v, &key);
                v
            })
            .collect()
    };
    let event_rows = to_values(
        eligible
            .iter()
            .map(|e| serde_json::to_value(e).unwrap_or(Value::Null))
            .collect(),
    );
    let episode_rows = to_values(
        built
            .episodes
            .iter()
            .map(|e| serde_json::to_value(e).unwrap_or(Value::Null))
            .collect(),
    );
    let mut quality_counts: BTreeMap<String, usize> = BTreeMap::from([
        ("events_in".into(), quality.events_in),
        ("duplicates_dropped".into(), quality.duplicates_dropped),
        (
            "conflicting_duplicates".into(),
            quality.conflicting_duplicates,
        ),
        ("sequence_gaps".into(), quality.sequence_gaps),
        ("clock_anomalies".into(), quality.clock_anomalies),
        (
            "future_inputs_rejected".into(),
            built.quality.future_inputs_rejected,
        ),
        ("dangling_refs".into(), built.quality.dangling_refs),
        ("orphan_events".into(), built.quality.orphan_events),
    ]);
    for (reason, n) in &quality.quarantined {
        quality_counts.insert(format!("quarantined_{reason}"), *n);
    }
    let template = Manifest {
        manifest_version: MANIFEST_VERSION,
        schema_version: SCHEMA_VERSION.into(),
        export_id: request.export_id.clone(),
        revision: 1,
        created_at: now,
        synthetic,
        purpose: request.purpose,
        recipient_class: class,
        recipient_id: request.recipient.id.clone(),
        files: Vec::new(),
        counts: counts(&[], &[]),
        consent_and_rights: ConsentRightsSummary {
            consent_texts: texts.into_iter().collect(),
            age_assurance: ages.into_iter().collect(),
            rights: policies
                .into_iter()
                .filter_map(|game| rights.get(game))
                .map(|m| RightsSummary {
                    rights_policy_id: m.rights_policy_id.clone(),
                    game_id: m.game_id,
                    status: m.status,
                    rights_holder: m.rights_holder.clone(),
                    recipients_licensed: m
                        .grants
                        .iter()
                        .filter(|g| g.purpose == request.purpose)
                        .flat_map(|g| g.recipients.iter().copied())
                        .collect::<BTreeSet<_>>()
                        .into_iter()
                        .collect(),
                    valid_until: m.valid_until,
                })
                .collect(),
            checked_at: now,
        },
        excluded: excluded.clone(),
        quality: quality_counts,
        deletions_applied: 0,
        loader: "tools/research_loader.py".into(),
    };

    // Written beside the target, then put in its place: a half-made export is never one.
    let staging = out_dir.with_file_name(format!(
        "{}.{}.partial",
        out_dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
        crate::research::random_hex(4).unwrap_or_else(|| "0".into())
    ));
    let manifest = match write_files(&staging, &event_rows, &episode_rows, &template, &metrics) {
        Ok(manifest) => manifest,
        Err(refusal) => {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(refusal);
        }
    };
    let _ = std::fs::remove_dir(out_dir);
    if let Err(e) = std::fs::rename(&staging, out_dir) {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(io(e));
    }
    store
        .record_export(&ExportLineage {
            export_id: request.export_id.clone(),
            revision: 1,
            dir: out_dir.to_path_buf(),
            created_at: now,
            purpose: request.purpose,
            recipient: request.recipient.clone(),
            subjects,
            rows: event_rows.len() + episode_rows.len(),
        })
        .map_err(io)?;
    ledger
        .append(&LedgerEntry::ExportWritten {
            export_id: request.export_id.clone(),
            revision: 1,
            at: now,
            purpose: request.purpose,
            recipient_class: class,
            recipient_id: request.recipient.id.clone(),
            rows: event_rows.len() + episode_rows.len(),
        })
        .map_err(io)?;
    Ok(Exported {
        dir: out_dir.to_path_buf(),
        manifest,
        metrics,
        redactions,
    })
}

fn read_rows(path: &Path) -> std::io::Result<Vec<Value>> {
    std::fs::read_to_string(path)?
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            serde_json::from_str(line)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
        })
        .collect()
}

/// Remove the rows of the participant whose pseudonym in this export is `pseudonym`, and make the
/// export whole again (report, card, counts, checksums; revision + 1). How many rows went.
pub(crate) fn remove_subject_rows(dir: &Path, pseudonym: &str) -> std::io::Result<usize> {
    let invalid = |e: String| std::io::Error::new(std::io::ErrorKind::InvalidData, e);
    let mut manifest = verify(dir).map_err(invalid)?;
    let of_subject = |row: &Value| row["research_subject_id"] == pseudonym;
    let events = read_rows(&dir.join("events.jsonl"))?;
    let episodes = read_rows(&dir.join("episodes.jsonl"))?;
    let removed = events.iter().filter(|r| of_subject(r)).count()
        + episodes.iter().filter(|r| of_subject(r)).count();
    if removed == 0 {
        return Ok(0);
    }
    let events: Vec<Value> = events.into_iter().filter(|r| !of_subject(r)).collect();
    let episodes: Vec<Value> = episodes.into_iter().filter(|r| !of_subject(r)).collect();
    let typed: Vec<Episode> = episodes
        .iter()
        .filter_map(|v| serde_json::from_value(v.clone()).ok())
        .collect();
    let metrics = report::metrics(&typed);
    manifest.revision += 1;
    manifest.deletions_applied += 1;
    write_files(dir, &events, &episodes, &manifest, &metrics)
        .map_err(|r| invalid(r.to_string()))?;
    Ok(removed)
}

/// Check an export: every file the manifest lists is there, with its SHA-256, size and rows.
pub fn verify(dir: &Path) -> Result<Manifest, String> {
    let text = std::fs::read_to_string(dir.join("manifest.json"))
        .map_err(|e| format!("manifest.json: {e}"))?;
    let manifest: Manifest =
        serde_json::from_str(&text).map_err(|e| format!("manifest.json: {e}"))?;
    if manifest.manifest_version != MANIFEST_VERSION {
        return Err(format!("manifest version {}", manifest.manifest_version));
    }
    if manifest.schema_version != SCHEMA_VERSION {
        return Err(format!("schema {}", manifest.schema_version));
    }
    for file in &manifest.files {
        if file.path.contains("..") || file.path.contains('/') || file.path.contains('\\') {
            return Err(format!("{}: not a file of this folder", file.path));
        }
        let bytes =
            std::fs::read(dir.join(&file.path)).map_err(|e| format!("{}: {e}", file.path))?;
        let sum = sha256_hex(&bytes);
        if sum != file.sha256 {
            return Err(format!(
                "{}: checksum mismatch (manifest {}, file {sum})",
                file.path, file.sha256
            ));
        }
        if bytes.len() as u64 != file.bytes {
            return Err(format!("{}: size mismatch", file.path));
        }
        if let Some(rows) = file.rows {
            let lines = bytes
                .split(|b| *b == b'\n')
                .filter(|l| !l.is_empty())
                .count();
            if lines != rows {
                return Err(format!("{}: {lines} rows, manifest says {rows}", file.path));
            }
        }
    }
    Ok(manifest)
}
