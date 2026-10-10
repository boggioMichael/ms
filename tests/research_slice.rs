//! The data program's P1 slice (`ms::research`), held to the acceptance rows of the owner's
//! directive (§17) as the w46 brief lists them — one test per row — plus the checks that keep
//! `docs/data-program/DATA_CONTRACTS.md`, the committed fixtures and the code in step.
//!
//! Everything here is synthetic: `game_id` "synthetic", generated participants, a fixed clock
//! (`synthetic::now()`). No real player data is read or written.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use chrono::Duration;
use serde_json::Value;

use ms::research::consent::{
    ConsentLedger, LedgerEntry, Purpose, Recipient, RecipientClass, RightsRegistry, RightsStatus,
};
use ms::research::contracts::{
    ENVELOPE_FIELDS, Event, GameId, Payload, SCHEMA_VERSION, SourceType,
};
use ms::research::episode::{self, ClaimStatus, Episode, OutcomeKind};
use ms::research::export::{self, ExportRequest};
use ms::research::recorder::Recorder;
use ms::research::store::ResearchStore;
use ms::research::synthetic;

// ---------------------------------------------------------------------------------------------
// Helpers

fn temp(name: &str) -> PathBuf {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static N: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "ms-research-{name}-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

/// Every file under `dir`, recursively (none when `dir` does not exist).
fn files_under(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(next) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&next) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}

fn jsonl(path: &Path) -> Vec<Value> {
    std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

/// Every value under `key`, anywhere in `value`.
fn values_of<'a>(value: &'a Value, key: &str, out: &mut Vec<&'a Value>) {
    match value {
        Value::Object(map) => {
            for (k, v) in map {
                if k == key {
                    out.push(v);
                }
                values_of(v, key, out);
            }
        }
        Value::Array(items) => items.iter().for_each(|v| values_of(v, key, out)),
        _ => {}
    }
}

fn the_episode<'a>(episodes: &'a [Episode], goal_note: &str) -> &'a Episode {
    episodes
        .iter()
        .find(|e| e.episode_id.ends_with(goal_note))
        .unwrap_or_else(|| panic!("no episode {goal_note} in {:?}", ids(episodes)))
}

fn ids(episodes: &[Episode]) -> Vec<&str> {
    episodes.iter().map(|e| e.episode_id.as_str()).collect()
}

// ---------------------------------------------------------------------------------------------
// Row 1 — consent off: the recorder is not made, nothing is written, no research folder; and the
// module has no network code.

#[test]
fn consent_off_makes_no_recorder_writes_nothing_and_creates_no_folder() {
    let dir = temp("consent-off");
    std::fs::create_dir_all(&dir).unwrap();
    let ledger = ConsentLedger::at(dir.join("consent").join("ledger.jsonl"));
    let rights = RightsRegistry::builtin();
    let root = dir.join("research");
    let now = synthetic::now();

    // No consent at all: no recorder, for any research purpose.
    for purpose in [
        Purpose::ImproveSyrup,
        Purpose::AggregateAnalytics,
        Purpose::ExternalResearchTraining,
    ] {
        let config =
            synthetic::recorder_config(&root, "syn-subject-off", "syn-session-off", purpose);
        let refused = Recorder::try_open(config.clone(), &ledger, &rights, now).unwrap_err();
        assert_eq!(refused.code(), "no_consent", "{purpose:?}");
        assert!(Recorder::open(config, &ledger, &rights, now).is_none());
    }
    // Nothing was written anywhere: not the research folder, not the ledger, not a lock.
    assert!(!root.exists(), "the research folder was created");
    assert_eq!(files_under(&dir), Vec::<PathBuf>::new());

    // Consent to one purpose opens no recorder for another, and service operation (the
    // inference service) never feeds the research store, even with consent to it.
    ledger
        .grant(synthetic::receipt(
            "syn-subject-some",
            &[Purpose::AggregateAnalytics, Purpose::ServiceOperation],
            &[RecipientClass::ThisDevice],
        ))
        .unwrap();
    for (purpose, code) in [
        (Purpose::ExternalResearchTraining, "purpose_not_consented"),
        (Purpose::ServiceOperation, "not_a_research_purpose"),
        (Purpose::MediaDonation, "media_path_not_built"),
    ] {
        let config =
            synthetic::recorder_config(&root, "syn-subject-some", "syn-session-x", purpose);
        let refused = Recorder::try_open(config, &ledger, &rights, now).unwrap_err();
        assert_eq!(refused.code(), code, "{purpose:?}");
    }
    assert!(
        !root.exists(),
        "a refused recorder created the research folder"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_research_module_has_no_network_code_and_reaches_nothing_outside_itself() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("research");
    let sources = files_under(&dir);
    assert!(sources.len() >= 5, "the module's files: {sources:?}");
    // Sockets, subprocesses (the app sends with a subprocess), HTTP clients, TLS, addresses.
    let forbidden = [
        "std::net",
        "TcpStream",
        "TcpListener",
        "UdpSocket",
        "ToSocketAddrs",
        "std::process",
        "Command",
        "curl",
        "reqwest",
        "ureq",
        "hyper",
        "rustls",
        "http://",
        "https://",
    ];
    for source in &sources {
        let text = std::fs::read_to_string(source).unwrap();
        for word in forbidden {
            assert!(
                !text.contains(word),
                "{} contains {word:?}",
                source.display()
            );
        }
        // Nothing of the app (its AI, phone link, updater, recorder) is reachable from here.
        for line in text.lines().filter(|line| line.contains("crate::")) {
            assert!(
                line.contains("crate::research"),
                "{} reaches outside the module: {line}",
                source.display()
            );
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Row 2 — withdrawal with a batch pending cancels it.

#[test]
fn withdrawal_with_a_batch_pending_cancels_it() {
    let dir = temp("withdraw");
    let ledger_path = dir.join("consent").join("ledger.jsonl");
    let ledger = ConsentLedger::at(&ledger_path);
    let rights = RightsRegistry::builtin();
    let root = dir.join("research");
    let now = synthetic::now();
    let subject = "syn-subject-w";
    ledger
        .grant(synthetic::receipt(
            subject,
            &[Purpose::ImproveSyrup],
            &[RecipientClass::ThisDevice],
        ))
        .unwrap();

    let config = synthetic::recorder_config(&root, subject, "syn-session-w", Purpose::ImproveSyrup);
    let mut recorder = Recorder::open(config, &ledger, &rights, now).expect("consented");
    recorder.record(synthetic::session_start(0), now).unwrap();
    recorder.record(synthetic::observation(1_000), now).unwrap();
    assert_eq!(recorder.flush(now).unwrap(), 2);

    recorder
        .record(synthetic::help(2_000, "where is the next npc"), now)
        .unwrap();
    recorder
        .record(synthetic::help(3_000, "and after that?"), now)
        .unwrap();
    assert_eq!(recorder.pending(), 2);

    // The player withdraws — on the phone: another handle on the same ledger.
    ConsentLedger::at(&ledger_path)
        .withdraw(subject, now + Duration::seconds(5))
        .unwrap();
    let later = now + Duration::seconds(6);
    let refused = recorder.flush(later).unwrap_err();
    assert_eq!(refused.code(), "consent_withdrawn");
    assert_eq!(recorder.pending(), 0, "the batch is still pending");
    assert_eq!(recorder.cancelled(), 2);

    // Only what was written before the withdrawal is on disk; the batch never reached it.
    let events = ResearchStore::at(&root).read_events().unwrap();
    assert_eq!(events.len(), 2);
    assert!(
        events
            .iter()
            .all(|e| !matches!(e.payload, Payload::Help(_)))
    );

    // And nothing more is taken.
    let refused = recorder
        .record(synthetic::help(7_000, "one more"), later)
        .unwrap_err();
    assert_eq!(refused.code(), "consent_withdrawn");
    assert_eq!(recorder.pending(), 0);
    assert_eq!(ResearchStore::at(&root).read_events().unwrap().len(), 2);

    // The withdrawal is a receipt in the ledger, beside the grant (not instead of it).
    let entries = ledger.entries().unwrap();
    assert!(matches!(entries[0], LedgerEntry::Granted(_)));
    assert!(matches!(entries[1], LedgerEntry::Withdrawn { .. }));
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------------------------
// Row 3 — an expired rights policy refuses (recording and export, checked every time).

#[test]
fn an_expired_rights_policy_refuses_recording_and_export() {
    let dir = temp("expired");
    let run = synthetic::run_slice(&dir).unwrap();
    let now = synthetic::now();
    let ledger = ConsentLedger::at(&run.ledger_path);
    let store = ResearchStore::at(&run.research_root);

    let synthetic_policy = RightsRegistry::builtin()
        .get(GameId::Synthetic)
        .cloned()
        .unwrap();
    let mut expired_policy = synthetic_policy.clone();
    expired_policy.valid_until = now - Duration::days(1);
    let expired = RightsRegistry::builtin().with(expired_policy);

    // Recording: refused at the door.
    let config = synthetic::recorder_config(
        &run.research_root,
        synthetic::SUBJECT_ONE,
        "syn-session-late",
        Purpose::ExternalResearchTraining,
    );
    let refused = Recorder::try_open(config.clone(), &ledger, &expired, now).unwrap_err();
    assert_eq!(refused.code(), "rights_expired");

    // A recorder opened while the policy held refuses once it has run out — checked every time,
    // and what was pending is not written.
    let mut ending = synthetic_policy;
    ending.valid_until = now + Duration::minutes(10);
    let ending = RightsRegistry::builtin().with(ending);
    let before = store.read_events().unwrap().len();
    let mut recorder = Recorder::open(config, &ledger, &ending, now).expect("valid now");
    recorder.record(synthetic::session_start(0), now).unwrap();
    let after_expiry = now + Duration::minutes(11);
    assert_eq!(
        recorder.flush(after_expiry).unwrap_err().code(),
        "rights_expired"
    );
    assert_eq!(recorder.pending(), 0);
    assert_eq!(store.read_events().unwrap().len(), before);

    // Export: refused, and no folder is made.
    let out = dir.join("export-after-expiry");
    let request = ExportRequest::new(
        "syn-export-expired",
        Purpose::ExternalResearchTraining,
        Recipient::new(RecipientClass::ThisDevice, "local-demo"),
    );
    let refused = export::export_local(&store, &ledger, &expired, &request, &out, now).unwrap_err();
    assert_eq!(refused.code(), "nothing_eligible");
    assert!(
        refused.reasons().contains_key("rights_expired"),
        "{refused}"
    );
    assert!(!out.exists());
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------------------------
// Row 4 — an export for a purpose or recipient not consented or not licensed refuses; MapleStory
// data is refused even with consent.

#[test]
fn an_export_for_a_purpose_or_recipient_not_consented_or_not_licensed_refuses() {
    let dir = temp("export-refusals");
    let run = synthetic::run_slice(&dir).unwrap();
    let now = synthetic::now();
    let ledger = ConsentLedger::at(&run.ledger_path);
    let store = ResearchStore::at(&run.research_root);
    let rights = RightsRegistry::builtin();
    let local = Recipient::new(RecipientClass::ThisDevice, "local-demo");

    let cases = [
        // Licensed for the synthetic title, but no participant consented to it.
        (
            "agg",
            Purpose::AggregateAnalytics,
            local.clone(),
            "purpose_not_consented",
        ),
        // Consented by some, but the synthetic title is licensed for this device only.
        (
            "lab",
            Purpose::ExternalResearchTraining,
            Recipient::new(RecipientClass::ExternalResearcher, "lab-a"),
            "use_not_licensed",
        ),
        (
            "buyer",
            Purpose::ExternalResearchTraining,
            Recipient::new(RecipientClass::LicensedBuyer, "buyer-a"),
            "use_not_licensed",
        ),
        // Neither licensed nor consented: media has no path in P1.
        (
            "media",
            Purpose::MediaDonation,
            local.clone(),
            "use_not_licensed",
        ),
    ];
    for (name, purpose, recipient, reason) in cases {
        let out = dir.join(format!("refused-{name}"));
        let request = ExportRequest::new(&format!("syn-export-{name}"), purpose, recipient);
        let refused =
            export::export_local(&store, &ledger, &rights, &request, &out, now).expect_err(name);
        assert_eq!(refused.code(), "nothing_eligible", "{name}");
        assert!(refused.reasons().contains_key(reason), "{name}: {refused}");
        assert!(!out.exists(), "{name}: a refused export made its folder");
    }

    // An export that is allowed leaves out, row by row, whoever did not consent to its purpose
    // or recipient — and says how many, never who.
    let manifest = &run.exported.manifest;
    assert!(
        manifest
            .excluded
            .get("purpose_not_consented")
            .copied()
            .unwrap_or(0)
            > 0
    );
    let manifest_text = std::fs::read_to_string(run.export_dir.join("manifest.json")).unwrap();
    assert!(
        !manifest_text.contains("syn-subject"),
        "the manifest names a subject"
    );

    // The refusals the slice itself tried are on record with their reasons.
    let codes: Vec<&str> = run.refused.iter().map(|(_, r)| r.code()).collect();
    assert!(codes.iter().all(|c| *c == "nothing_eligible"), "{codes:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn maplestory_data_is_refused_even_with_full_consent() {
    let dir = temp("maplestory");
    let now = synthetic::now();
    let ledger = ConsentLedger::at(dir.join("consent").join("ledger.jsonl"));
    let rights = RightsRegistry::builtin();
    for game in [GameId::Maplestory, GameId::MaplestoryWorlds] {
        let manifest = rights.get(game).unwrap();
        assert_eq!(manifest.status, RightsStatus::RequiresTitleSpecificReview);
    }
    let subject = "syn-subject-all-in";
    ledger
        .grant(synthetic::receipt(
            subject,
            &Purpose::ALL,
            &RecipientClass::ALL,
        ))
        .unwrap();

    // Recording: refused for both titles, whatever the purpose.
    let root = dir.join("research");
    for game in [GameId::Maplestory, GameId::MaplestoryWorlds] {
        for purpose in [Purpose::ImproveSyrup, Purpose::ExternalResearchTraining] {
            let mut config = synthetic::recorder_config(&root, subject, "syn-session-ms", purpose);
            config.game.game_id = game;
            let refused = Recorder::try_open(config, &ledger, &rights, now).unwrap_err();
            assert_eq!(
                refused.code(),
                "rights_not_approved",
                "{game:?} {purpose:?}"
            );
        }
    }
    assert!(!root.exists());

    // Export: a spool holding MapleStory events (as if they came from elsewhere) is refused, for
    // every purpose and recipient, though the subject consented to everything.
    let store = ResearchStore::at(&root);
    std::fs::create_dir_all(store.spool_path().parent().unwrap()).unwrap();
    let mut lines = String::new();
    for (i, mut event) in synthetic::sample_events(subject).into_iter().enumerate() {
        event.envelope.game_id = if i % 2 == 0 {
            GameId::Maplestory
        } else {
            GameId::MaplestoryWorlds
        };
        lines.push_str(&serde_json::to_string(&event).unwrap());
        lines.push('\n');
    }
    std::fs::write(store.spool_path(), lines).unwrap();
    for purpose in Purpose::ALL {
        for class in RecipientClass::ALL {
            let out = dir.join("never");
            let request =
                ExportRequest::new("syn-export-ms", purpose, Recipient::new(class, "any"));
            let refused =
                export::export_local(&store, &ledger, &rights, &request, &out, now).unwrap_err();
            assert_eq!(refused.code(), "nothing_eligible");
            assert_eq!(
                refused.reasons().keys().collect::<Vec<_>>(),
                vec!["rights_not_approved"],
                "{purpose:?} {class:?}"
            );
            assert!(!out.exists());
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------------------------
// Row 5 — a name, a chat line or a credential never reaches an exported (or any written) file.

#[test]
fn a_name_a_chat_line_or_a_credential_never_reaches_a_written_file() {
    // The generator really puts them in (else this test would prove nothing).
    let raw = format!("{:?}", synthetic::participants());
    for needle in synthetic::NEEDLES {
        assert!(raw.contains(needle), "the generator lacks {needle:?}");
    }

    let dir = temp("leaks");
    let run = synthetic::run_slice(&dir).unwrap();
    let files = files_under(&dir);
    assert!(files.len() >= 8, "{files:?}");
    for file in &files {
        let text = std::fs::read_to_string(file).unwrap().to_lowercase();
        for needle in synthetic::NEEDLES {
            assert!(
                !text.contains(&needle.to_lowercase()),
                "{needle:?} reached {}",
                file.display()
            );
        }
    }

    // And the sanitizer did see each kind: the redactions are counted (kinds, never content).
    let mut kinds = Vec::new();
    for event in jsonl(&run.export_dir.join("events.jsonl")) {
        let mut found = Vec::new();
        values_of(&event, "redactions", &mut found);
        for list in found {
            for kind in list.as_array().unwrap() {
                kinds.push(kind.as_str().unwrap().to_string());
            }
        }
    }
    for kind in ["name", "credential", "email", "url", "handle"] {
        assert!(
            kinds.iter().any(|k| k == kind),
            "no {kind} redaction in {kinds:?}"
        );
    }
    // Chat lines and whispers are dropped whole, before anything is written.
    assert!(run.recorded.redactions.get("chat").copied().unwrap_or(0) >= 2);

    // No identifier of the research folder reaches the export: every one is a pseudonym there.
    let mut originals = std::collections::BTreeSet::new();
    for event in jsonl(&ResearchStore::at(&run.research_root).spool_path()) {
        for key in [
            "event_id",
            "session_id",
            "episode_id",
            "research_subject_id",
            "consent_receipt_id",
        ] {
            if let Some(id) = event[key].as_str() {
                originals.insert(id.to_string());
            }
        }
    }
    assert!(originals.len() > 40);
    for file in files_under(&run.export_dir) {
        let text = std::fs::read_to_string(&file).unwrap();
        for id in &originals {
            assert!(
                !text.contains(&format!("\"{id}\"")),
                "{id} in {}",
                file.display()
            );
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------------------------
// Row 6 — duplicate and out-of-order events build the same episodes.

#[test]
fn duplicate_and_out_of_order_events_build_the_same_episodes() {
    let clean_dir = temp("order-clean");
    let clean = synthetic::run_slice_with(
        &clean_dir,
        synthetic::SliceOptions {
            transport_faults: false,
        },
    )
    .unwrap();
    let faulty_dir = temp("order-faulty");
    let faulty = synthetic::run_slice(&faulty_dir).unwrap();

    let clean_events = ResearchStore::at(&clean.research_root)
        .read_events()
        .unwrap();
    let faulty_events = ResearchStore::at(&faulty.research_root)
        .read_events()
        .unwrap();
    // The faults are really there: one more line, and not in order.
    assert_eq!(faulty_events.len(), clean_events.len() + 1);
    assert_ne!(
        faulty_events
            .iter()
            .map(|e| e.envelope.event_id.clone())
            .collect::<Vec<_>>(),
        clean_events
            .iter()
            .map(|e| e.envelope.event_id.clone())
            .collect::<Vec<_>>()
    );

    let a = episode::build(&clean_events);
    let b = episode::build(&faulty_events);
    assert_eq!(b.quality.duplicates_dropped, 1);
    assert_eq!(
        serde_json::to_value(&a.episodes).unwrap(),
        serde_json::to_value(&b.episodes).unwrap()
    );

    // Any order, any number of exact copies: the same episodes.
    let mut shuffled: Vec<Event> = clean_events.iter().rev().cloned().collect();
    shuffled.extend(clean_events.iter().step_by(3).cloned());
    let c = episode::build(&shuffled);
    assert_eq!(
        serde_json::to_value(&a.episodes).unwrap(),
        serde_json::to_value(&c.episodes).unwrap()
    );

    // Two different events under one id are not "a duplicate": neither is believed.
    let mut conflicting = clean_events.clone();
    let mut twin = conflicting[3].clone();
    twin.envelope.monotonic_timestamp += 1;
    conflicting.push(twin);
    let d = episode::build(&conflicting);
    assert_eq!(d.quality.conflicting_duplicates, 1);

    // The committed fixture (the spool as a retry left it) builds the same episodes.
    let fixture =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/research/spool-events.jsonl");
    let committed: Vec<Event> = std::fs::read_to_string(fixture)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let e = episode::build(&committed);
    assert_eq!(
        serde_json::to_value(&a.episodes).unwrap(),
        serde_json::to_value(&e.episodes).unwrap()
    );
    let _ = std::fs::remove_dir_all(&clean_dir);
    let _ = std::fs::remove_dir_all(&faulty_dir);
}

// ---------------------------------------------------------------------------------------------
// Row 7 — `unknown` never becomes 0.

#[test]
fn unknown_never_becomes_zero() {
    let dir = temp("unknown");
    let run = synthetic::run_slice(&dir).unwrap();

    // In the spool: the level the HUD could not read is null — present, and not 0.
    let spool = jsonl(&ResearchStore::at(&run.research_root).spool_path());
    let mut unknown_levels = 0;
    for event in &spool {
        let mut components = Vec::new();
        values_of(event, "components", &mut components);
        for list in components {
            for component in list.as_array().unwrap() {
                if component["name"] == "level" && component["status"] == "unknown" {
                    assert!(component.get("value").is_some(), "the key is dropped");
                    assert!(component["value"].is_null(), "{component}");
                    unknown_levels += 1;
                }
            }
        }
        // A probability nobody knows stays unknown.
        assert!(event.get("sampling_probability").is_some());
    }
    assert!(unknown_levels >= 1, "the fixture has no unknown level");

    // In the export: the same nulls; an episode with no success has no time to success (not 0),
    // and a confidence nobody calibrated is absent, never a number.
    let episodes = jsonl(&run.export_dir.join("episodes.jsonl"));
    let mut nulls = 0;
    for episode in &episodes {
        let ttfs = &episode["time_to_first_success_ms"];
        if episode["outcome"]["result"] != "success" {
            assert!(ttfs.is_null(), "{}", episode["episode_id"]);
            nulls += 1;
        } else {
            assert!(ttfs.as_u64().is_some_and(|ms| ms > 0));
        }
    }
    assert!(nulls >= 2);
    for event in jsonl(&run.export_dir.join("events.jsonl")) {
        let mut confidences = Vec::new();
        values_of(&event, "confidence", &mut confidences);
        for confidence in confidences {
            assert!(
                confidence.is_null() || confidence.get("calibration_ref").is_some(),
                "an uncalibrated confidence: {confidence}"
            );
        }
    }

    // In the report: the unobserved and censored attempts are counted as such, outside the
    // success rates' denominators — not as failures, not as zeros; and an attempt whose advice
    // may or may not have been shown is in neither arm.
    let metrics = &run.exported.metrics;
    assert!(metrics.unobserved_rate.numerator >= 1);
    assert!(metrics.censored_rate.numerator >= 1);
    let observed = metrics.success_assisted.denominator + metrics.success_unassisted.denominator;
    assert_eq!(
        observed
            + metrics.unobserved_rate.numerator
            + metrics.censored_rate.numerator
            + metrics.success_assistance_unknown,
        metrics.attempts
    );
    assert_eq!(metrics.help_per_attempt.denominator, metrics.attempts);
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------------------------
// Row 8 — `model_inferred` never becomes `human_reviewed` / gold.

#[test]
fn model_inferred_never_becomes_human_reviewed_or_gold() {
    let dir = temp("provenance");
    let run = synthetic::run_slice(&dir).unwrap();

    // A label that claims a review it never had is refused at the door.
    let forged: Vec<&str> = run
        .recorded
        .rejected
        .iter()
        .map(|(_, code)| code.as_str())
        .collect();
    assert!(forged.contains(&"invalid_event"), "{forged:?}");

    // What was model_inferred in the spool is model_inferred in the export, as often.
    let count = |rows: &[Value], source: &str| {
        let mut n = 0;
        for row in rows {
            let mut found = Vec::new();
            values_of(row, "source_type", &mut found);
            n += found.iter().filter(|v| **v == source).count();
        }
        n
    };
    let spool = jsonl(&ResearchStore::at(&run.research_root).spool_path());
    let exported_subjects: Vec<&str> = vec![synthetic::SUBJECT_ONE, synthetic::SUBJECT_THREE];
    let spool_of_exported: Vec<Value> = spool
        .into_iter()
        .filter(|e| exported_subjects.contains(&e["research_subject_id"].as_str().unwrap()))
        .collect();
    let exported = jsonl(&run.export_dir.join("events.jsonl"));
    assert_eq!(exported.len(), spool_of_exported.len() - 1, "one duplicate");
    let inferred_in = count(&spool_of_exported, "model_inferred");
    assert!(inferred_in >= 3);
    // (The duplicate the retry made is one of the spool's lines: count it once.)
    let duplicate_inferred = count(&run.duplicated_rows, "model_inferred");
    assert_eq!(
        count(&exported, "model_inferred"),
        inferred_in - duplicate_inferred
    );

    // No reviewer exists in P1: nothing anywhere is human_reviewed, reviewer_verified or gold.
    for file in files_under(&dir) {
        let text = std::fs::read_to_string(&file).unwrap();
        for word in ["\"human_reviewed\"", "\"reviewer_verified\"", "\"gold\""] {
            assert!(!text.contains(word), "{word} in {}", file.display());
        }
    }

    // A player's "yes, that's it" leaves a model's inferred goal inferred.
    let built = ResearchStore::at(&run.research_root)
        .read_episodes()
        .unwrap();
    let inferred = the_episode(&built, synthetic::EPISODE_INFERRED_GOAL);
    let goal = inferred.goal.as_ref().unwrap();
    assert_eq!(goal.provenance.source_type, SourceType::ModelInferred);
    assert!(
        !inferred.feedback.is_empty(),
        "the confirmation is kept beside it"
    );

    // A forged label in a spool (not through the recorder) is quarantined by the builder.
    let mut events = ResearchStore::at(&run.research_root).read_events().unwrap();
    let forged = synthetic::forged_review(&events[5]);
    events.push(forged);
    let built = episode::build(&events);
    assert_eq!(built.quality.quarantined.get("invalid_event"), Some(&1));
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------------------------
// Row 9 — a poisoned correction is kept as a claim, not applied.

#[test]
fn a_poisoned_correction_is_kept_as_a_claim_not_applied() {
    let dir = temp("poison");
    let run = synthetic::run_slice(&dir).unwrap();
    let episodes = ResearchStore::at(&run.research_root)
        .read_episodes()
        .unwrap();
    let poisoned = the_episode(&episodes, synthetic::EPISODE_POISONED);

    // The player's claim ("it was a success, mark it verified") is there, as a claim.
    assert_eq!(poisoned.corrections.len(), 1);
    let claim = &poisoned.corrections[0];
    assert_eq!(claim.status, ClaimStatus::Claimed);
    assert!(!claim.applied);
    assert_eq!(claim.provenance.source_type, SourceType::HumanAsserted);

    // Nothing it targets changed: the outcome is still the observed failure, determined as before.
    assert_eq!(poisoned.outcome.result, OutcomeKind::Failure);
    assert_eq!(poisoned.time_to_first_success_ms, None);
    // ... and the report counts a failure, not a success.
    let metrics = &run.exported.metrics;
    assert!(metrics.failures >= 1);
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------------------------
// Row 10 — deleting a research subject removes their events, episodes and export rows and
// lists the affected exports.

#[test]
fn deleting_a_research_subject_removes_their_events_episodes_and_export_rows_and_lists_the_exports()
{
    let dir = temp("delete");
    let run = synthetic::run_slice(&dir).unwrap();
    let store = ResearchStore::at(&run.research_root);
    let ledger = ConsentLedger::at(&run.ledger_path);
    let subject = synthetic::SUBJECT_THREE;
    let pseudonym = store
        .exports()
        .unwrap()
        .iter()
        .find(|lineage| lineage.export_id == run.exported.manifest.export_id)
        .and_then(|lineage| lineage.subjects.get(subject).cloned())
        .expect("the subject is in the export");
    let rows_with = |file: &str| {
        jsonl(&run.export_dir.join(file))
            .iter()
            .filter(|row| row["research_subject_id"] == pseudonym.as_str())
            .count()
    };
    assert!(rows_with("events.jsonl") > 0 && rows_with("episodes.jsonl") > 0);
    let participants_before = run.exported.manifest.counts.participants;

    let deletion = store
        .delete_subject(subject, &ledger, synthetic::now() + Duration::hours(1))
        .unwrap();
    assert!(deletion.events_removed > 0);
    assert!(deletion.episodes_removed > 0);
    assert_eq!(deletion.exports_affected.len(), 1);
    assert_eq!(
        deletion.exports_affected[0].export_id,
        run.exported.manifest.export_id
    );
    assert!(deletion.exports_affected[0].rows_removed > 0);

    // Gone from every layer: the spool, the episodes, the export's rows.
    assert!(
        store
            .read_events()
            .unwrap()
            .iter()
            .all(|e| e.envelope.research_subject_id != subject)
    );
    assert!(
        store
            .read_episodes()
            .unwrap()
            .iter()
            .all(|e| e.research_subject_id != subject)
    );
    assert_eq!(rows_with("events.jsonl"), 0);
    assert_eq!(rows_with("episodes.jsonl"), 0);
    // The export is whole again: counts, report and checksums made from what is left.
    let manifest = export::verify(&run.export_dir).unwrap();
    assert_eq!(manifest.revision, 2);
    assert_eq!(manifest.counts.participants, participants_before - 1);
    // The ledger keeps the minimum that proves it was done, and which exports it reached.
    let last = ledger.entries().unwrap().pop().unwrap();
    match last {
        LedgerEntry::SubjectDeleted {
            exports_affected, ..
        } => assert_eq!(
            exports_affected,
            vec![run.exported.manifest.export_id.clone()]
        ),
        other => panic!("the last entry is {other:?}"),
    }
    // Again: nothing left to remove, nothing affected.
    let again = store
        .delete_subject(subject, &ledger, synthetic::now() + Duration::hours(2))
        .unwrap();
    assert_eq!(
        (
            again.events_removed,
            again.episodes_removed,
            again.exports_affected.len()
        ),
        (0, 0, 0)
    );
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------------------------
// Row 11 — the manifest's checksums match, and the loader verifies them.

#[test]
fn the_manifest_checksums_match_and_the_loader_verifies_them() {
    let dir = temp("checksums");
    let run = synthetic::run_slice(&dir).unwrap();
    let manifest = export::verify(&run.export_dir).unwrap();
    assert_eq!(manifest.schema_version, SCHEMA_VERSION);
    assert!(manifest.synthetic);
    let names: Vec<&str> = manifest.files.iter().map(|f| f.path.as_str()).collect();
    for name in [
        "events.jsonl",
        "episodes.jsonl",
        "DATA_CARD.md",
        "REPORT.md",
    ] {
        assert!(names.contains(&name), "{names:?}");
    }
    for file in &manifest.files {
        assert_eq!(file.sha256.len(), 64);
    }

    let loader = Path::new(env!("CARGO_MANIFEST_DIR")).join("tools/research_loader.py");
    let python = ["python3", "python"].into_iter().find(|p| {
        std::process::Command::new(p)
            .arg("--version")
            .output()
            .is_ok_and(|o| o.status.success())
    });
    let run_loader = |python: &str| {
        std::process::Command::new(python)
            .arg(&loader)
            .arg(&run.export_dir)
            .output()
            .unwrap()
    };
    if let Some(python) = python {
        let ok = run_loader(python);
        assert!(
            ok.status.success(),
            "{}",
            String::from_utf8_lossy(&ok.stderr)
        );
        assert!(String::from_utf8_lossy(&ok.stdout).contains("verified"));
    } else {
        eprintln!("SKIPPED the Python half: no python3/python on this machine");
    }

    // One byte changed: both refuse.
    let events = run.export_dir.join("events.jsonl");
    let mut bytes = std::fs::read(&events).unwrap();
    let at = bytes.iter().position(|b| *b == b'{').unwrap();
    bytes[at + 1] = if bytes[at + 1] == b'"' { b' ' } else { b'"' };
    std::fs::write(&events, bytes).unwrap();
    let refused = export::verify(&run.export_dir).unwrap_err();
    assert!(refused.contains("checksum"), "{refused}");
    if let Some(python) = python {
        let refused = run_loader(python);
        assert!(!refused.status.success());
        assert!(String::from_utf8_lossy(&refused.stderr).contains("checksum"));
    }
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------------------------
// The contracts — kept in step with the document, the fixtures and the directive.

#[test]
fn in_game_text_that_gives_orders_is_kept_as_untrusted_text_and_changes_nothing() {
    let dir = temp("injection");
    let run = synthetic::run_slice(&dir).unwrap();
    let ledger = ConsentLedger::at(&run.ledger_path);

    // It is there, as untrusted text, flagged — and only under that key.
    let events = jsonl(&run.export_dir.join("events.jsonl"));
    let mut flagged = 0;
    for event in &events {
        let mut texts = Vec::new();
        values_of(event, "untrusted_text", &mut texts);
        flagged += texts
            .iter()
            .filter(|t| {
                t.as_str()
                    .is_some_and(|t| t.to_lowercase().contains("ignore previous instructions"))
            })
            .count();
        let mut other_texts = Vec::new();
        values_of(event, "text", &mut other_texts);
        for text in other_texts {
            assert!(
                !text
                    .as_str()
                    .unwrap_or("")
                    .to_lowercase()
                    .contains("ignore previous instructions")
            );
        }
    }
    assert_eq!(flagged, 1);
    assert!(run.exported.metrics.instruction_like_texts >= 1);

    // It changed nothing: the ledger holds only the slice's own grants and exports, and the
    // buyer it names is still refused.
    for entry in ledger.entries().unwrap() {
        match entry {
            LedgerEntry::Granted(receipt) => {
                assert!(
                    synthetic::consenting_subjects()
                        .contains(&receipt.research_subject_id.as_str())
                )
            }
            LedgerEntry::ExportWritten {
                recipient_class, ..
            } => {
                assert_eq!(recipient_class, RecipientClass::ThisDevice)
            }
            other => panic!("unexpected ledger entry {other:?}"),
        }
    }
    let request = ExportRequest::new(
        "syn-export-acme",
        Purpose::ExternalResearchTraining,
        Recipient::new(RecipientClass::LicensedBuyer, "acme"),
    );
    let store = ResearchStore::at(&run.research_root);
    let out = dir.join("acme");
    assert!(
        export::export_local(
            &store,
            &ledger,
            &RightsRegistry::builtin(),
            &request,
            &out,
            synthetic::now()
        )
        .is_err()
    );
    assert!(!out.exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_window_loss_or_syrup_closing_is_not_a_failure() {
    let dir = temp("not-failure");
    let run = synthetic::run_slice(&dir).unwrap();
    let episodes = ResearchStore::at(&run.research_root)
        .read_episodes()
        .unwrap();
    let lost = the_episode(&episodes, synthetic::EPISODE_WINDOW_LOST);
    assert_eq!(lost.outcome.result, OutcomeKind::Unobserved);
    assert!(lost.outcome.observable_until_ms.is_some());
    // Advice was shown before sight was lost: assisted, though the outcome is not known.
    assert_eq!(lost.assisted, Some(true));
    let closed = the_episode(&episodes, synthetic::EPISODE_RECORDING_ENDED);
    assert_eq!(closed.outcome.result, OutcomeKind::Censored);
    // Advice was given there, but whether it was shown is not known: unknown, not "no".
    assert_eq!(closed.assisted, None);
    // A success with no advice before it is independent.
    let alone = the_episode(&episodes, synthetic::EPISODE_INFERRED_GOAL);
    assert_eq!(alone.assisted, Some(false));
    // A failure, then help, then success: one episode, two attempts, ending in success.
    let recovered = the_episode(&episodes, synthetic::EPISODE_RECOVERY);
    let results: Vec<OutcomeKind> = recovered.attempts.iter().map(|a| a.result).collect();
    assert_eq!(results, vec![OutcomeKind::Failure, OutcomeKind::Success]);
    assert_eq!(recovered.assisted, Some(true));
    assert!(recovered.time_to_first_success_ms.is_some_and(|ms| ms > 0));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn no_later_observation_is_an_input_to_earlier_advice() {
    let dir = temp("future");
    let run = synthetic::run_slice(&dir).unwrap();
    let events = ResearchStore::at(&run.research_root).read_events().unwrap();
    let built = episode::build(&events);
    // The generator links one piece of advice to an observation made after it was shown.
    assert_eq!(built.quality.future_inputs_rejected, 1);
    let by_id: BTreeMap<&str, &Event> = events
        .iter()
        .map(|e| (e.envelope.event_id.as_str(), e))
        .collect();
    for episode in &built.episodes {
        for advice in &episode.advice {
            for input in &advice.based_on {
                let input = by_id[input.as_str()];
                assert!(
                    input.envelope.sequence_no
                        < by_id[advice.event_id.as_str()].envelope.sequence_no
                );
            }
        }
        if let Some(before) = &episode.state_before {
            let first = episode.lineage.event_ids.first().unwrap();
            assert!(
                by_id[before.event_id.as_str()].envelope.sequence_no
                    <= by_id[first.as_str()].envelope.sequence_no
                    || by_id[before.event_id.as_str()].envelope.session_id
                        != by_id[first.as_str()].envelope.session_id
            );
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn every_event_carries_the_whole_envelope_and_the_contract_document_lists_it() {
    let dir = temp("envelope");
    let run = synthetic::run_slice(&dir).unwrap();
    for event in jsonl(&ResearchStore::at(&run.research_root).spool_path()) {
        let keys: Vec<&str> = event
            .as_object()
            .unwrap()
            .keys()
            .map(|k| k.as_str())
            .collect();
        for field in ENVELOPE_FIELDS {
            assert!(keys.contains(field), "{field} missing from {event}");
        }
        assert_eq!(keys.len(), ENVELOPE_FIELDS.len() + 1, "{keys:?}");
        assert_eq!(event["schema_version"], SCHEMA_VERSION);
        assert_eq!(event["game_id"], "synthetic");
    }
    let doc = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/data-program/DATA_CONTRACTS.md"),
    )
    .unwrap();
    for field in ENVELOPE_FIELDS {
        assert!(
            doc.contains(&format!("`{field}`")),
            "DATA_CONTRACTS.md lacks `{field}`"
        );
    }
    assert!(
        doc.contains(SCHEMA_VERSION),
        "DATA_CONTRACTS.md names another schema version"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_committed_fixtures_are_what_the_generator_makes() {
    // `RESEARCH_FIXTURES=write cargo test --release --offline --test research_slice` rewrites them.
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/research");
    let dir = temp("fixtures");
    let run = synthetic::run_slice(&dir).unwrap();
    let made: Vec<(PathBuf, PathBuf)> = vec![
        (
            ResearchStore::at(&run.research_root).spool_path(),
            fixtures.join("spool-events.jsonl"),
        ),
        (
            run.export_dir.join("events.jsonl"),
            fixtures.join("sample-export/events.jsonl"),
        ),
        (
            run.export_dir.join("episodes.jsonl"),
            fixtures.join("sample-export/episodes.jsonl"),
        ),
        (
            run.export_dir.join("manifest.json"),
            fixtures.join("sample-export/manifest.json"),
        ),
        (
            run.export_dir.join("DATA_CARD.md"),
            fixtures.join("sample-export/DATA_CARD.md"),
        ),
        (
            run.export_dir.join("REPORT.md"),
            fixtures.join("sample-export/REPORT.md"),
        ),
    ];
    let write = std::env::var("RESEARCH_FIXTURES").is_ok_and(|v| v == "write");
    for (fresh, committed) in &made {
        let fresh_text = std::fs::read_to_string(fresh).unwrap();
        if write {
            std::fs::create_dir_all(committed.parent().unwrap()).unwrap();
            std::fs::write(committed, &fresh_text).unwrap();
        }
        let committed_text = std::fs::read_to_string(committed)
            .unwrap_or_else(|_| panic!("{} is not committed", committed.display()));
        assert!(
            fresh_text == committed_text,
            "{} is not what the generator makes (RESEARCH_FIXTURES=write to rewrite)",
            committed.display()
        );
    }
    // The sample export verifies as committed.
    export::verify(&fixtures.join("sample-export")).unwrap();
    // Small, as fixtures in Git must be.
    let total: u64 = files_under(&fixtures)
        .iter()
        .map(|f| std::fs::metadata(f).unwrap().len())
        .sum();
    assert!(total < 400_000, "the fixtures are {total} bytes");
    let _ = std::fs::remove_dir_all(&dir);
}
