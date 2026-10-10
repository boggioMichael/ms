//! The research folder, on this machine:
//!
//! ```text
//! <root>/spool/events.jsonl          sanitized events, as recorded (layer 1)
//! <root>/episodes/episodes.jsonl     episodes built from them (layer 2)
//! <root>/lineage/exports.jsonl       each export: where, for what, to whom, and whose rows
//! <root>/lineage/recipient-keys.json the pseudonym key of each recipient (never exported)
//! ```
//!
//! The folder is created by the first batch a recorder writes — never by reading, never with
//! consent off. The lineage is the dependency graph from a subject to every export that holds
//! their rows, so that [`ResearchStore::delete_subject`] reaches every layer: the spool, the
//! episodes, and the rows of each export (whose report, data card, counts and checksums are made
//! again from what is left). Deleting a file does not undo a model already trained on it; the
//! lineage says which exports to follow up with their recipients.

use std::collections::BTreeMap;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::research::consent::{ConsentLedger, LedgerEntry, Purpose, Recipient};
use crate::research::contracts::Event;
use crate::research::episode::{self, Built, Episode};
use crate::research::{export, write_whole};

/// One export, as its producer keeps it (never shipped: it holds the link from subjects to the
/// export's pseudonyms).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExportLineage {
    pub export_id: String,
    pub revision: u32,
    pub dir: PathBuf,
    pub created_at: DateTime<Utc>,
    pub purpose: Purpose,
    pub recipient: Recipient,
    /// research_subject_id → the pseudonym it has in this export.
    pub subjects: BTreeMap<String, String>,
    pub rows: usize,
}

/// An export a deletion reached.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AffectedExport {
    pub export_id: String,
    pub dir: PathBuf,
    pub rows_removed: usize,
    /// The export's revision after the deletion.
    pub revision: u32,
    /// The export's folder was not there (moved or deleted): its rows could not be removed here.
    pub missing: bool,
}

/// What a deletion removed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Deletion {
    pub events_removed: usize,
    pub episodes_removed: usize,
    pub exports_affected: Vec<AffectedExport>,
}

#[derive(Debug, Clone)]
pub struct ResearchStore {
    root: PathBuf,
}

fn read_lines(path: &Path) -> std::io::Result<Option<String>> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

fn to_jsonl<T: Serialize>(rows: &[T]) -> std::io::Result<String> {
    let mut text = String::new();
    for row in rows {
        text.push_str(
            &serde_json::to_string(row)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?,
        );
        text.push('\n');
    }
    Ok(text)
}

impl ResearchStore {
    /// The store at `root`. Creates nothing.
    pub fn at(root: impl Into<PathBuf>) -> ResearchStore {
        ResearchStore { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn spool_path(&self) -> PathBuf {
        self.root.join("spool").join("events.jsonl")
    }

    pub fn episodes_path(&self) -> PathBuf {
        self.root.join("episodes").join("episodes.jsonl")
    }

    pub fn lineage_path(&self) -> PathBuf {
        self.root.join("lineage").join("exports.jsonl")
    }

    pub fn keys_path(&self) -> PathBuf {
        self.root.join("lineage").join("recipient-keys.json")
    }

    /// Append `events` to the spool, in one write.
    pub(crate) fn append_events(&self, events: &[Event]) -> std::io::Result<()> {
        let path = self.spool_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = to_jsonl(events)?;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;
        file.write_all(text.as_bytes())?;
        file.sync_all()
    }

    /// The spool's events as written (duplicates and all); a line that cannot be read is
    /// skipped. None when nothing was ever recorded.
    pub fn read_events(&self) -> std::io::Result<Vec<Event>> {
        Ok(read_lines(&self.spool_path())?
            .map(|text| {
                text.lines()
                    .filter_map(|line| serde_json::from_str(line).ok())
                    .collect()
            })
            .unwrap_or_default())
    }

    /// Build the episodes from the spool and keep them (layer 2).
    pub fn build_episodes(&self) -> std::io::Result<Built> {
        let built = episode::build(&self.read_events()?);
        write_whole(&self.episodes_path(), &to_jsonl(&built.episodes)?)?;
        Ok(built)
    }

    pub fn read_episodes(&self) -> std::io::Result<Vec<Episode>> {
        Ok(read_lines(&self.episodes_path())?
            .map(|text| {
                text.lines()
                    .filter_map(|line| serde_json::from_str(line).ok())
                    .collect()
            })
            .unwrap_or_default())
    }

    pub fn exports(&self) -> std::io::Result<Vec<ExportLineage>> {
        let Some(text) = read_lines(&self.lineage_path())? else {
            return Ok(Vec::new());
        };
        text.lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| {
                serde_json::from_str(line)
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
            })
            .collect()
    }

    /// Keep `lineage`, in place of any earlier record of the same export.
    pub(crate) fn record_export(&self, lineage: &ExportLineage) -> std::io::Result<()> {
        let mut all: Vec<ExportLineage> = self
            .exports()?
            .into_iter()
            .filter(|l| l.export_id != lineage.export_id)
            .collect();
        all.push(lineage.clone());
        write_whole(&self.lineage_path(), &to_jsonl(&all)?)
    }

    /// The pseudonym key of `recipient_id`: made at random on first use and kept here.
    pub(crate) fn recipient_key(&self, recipient_id: &str) -> std::io::Result<Vec<u8>> {
        let path = self.keys_path();
        let mut keys: BTreeMap<String, String> = match read_lines(&path)? {
            Some(text) => serde_json::from_str(&text)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?,
            None => BTreeMap::new(),
        };
        if !keys.contains_key(recipient_id) {
            let key = crate::research::random_hex(32)
                .ok_or_else(|| std::io::Error::other("no randomness for a recipient key"))?;
            keys.insert(recipient_id.into(), key);
            let text = serde_json::to_string_pretty(&keys)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
            write_whole(&path, &text)?;
        }
        let hex = &keys[recipient_id];
        Ok((0..hex.len() / 2)
            .filter_map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).ok())
            .collect())
    }

    /// Delete everything of `subject`: their events, their episodes, and their rows in every
    /// export the lineage knows (each export made whole again, its revision one higher). The
    /// ledger keeps the minimum that proves it: when, how much, which exports.
    pub fn delete_subject(
        &self,
        subject: &str,
        ledger: &ConsentLedger,
        at: DateTime<Utc>,
    ) -> std::io::Result<Deletion> {
        let events = self.read_events()?;
        let kept: Vec<&Event> = events
            .iter()
            .filter(|e| e.envelope.research_subject_id != subject)
            .collect();
        let events_removed = events.len() - kept.len();
        if events_removed > 0 {
            write_whole(&self.spool_path(), &to_jsonl(&kept)?)?;
        }
        let episodes = self.read_episodes()?;
        let kept_episodes: Vec<&Episode> = episodes
            .iter()
            .filter(|e| e.research_subject_id != subject)
            .collect();
        let episodes_removed = episodes.len() - kept_episodes.len();
        if episodes_removed > 0 {
            write_whole(&self.episodes_path(), &to_jsonl(&kept_episodes)?)?;
        }
        let mut exports_affected = Vec::new();
        for mut lineage in self.exports()? {
            let Some(pseudonym) = lineage.subjects.get(subject).cloned() else {
                continue;
            };
            let (rows_removed, missing) = if lineage.dir.is_dir() {
                (
                    export::remove_subject_rows(&lineage.dir, &pseudonym)?,
                    false,
                )
            } else {
                (0, true)
            };
            lineage.subjects.remove(subject);
            if !missing {
                lineage.revision += 1;
                lineage.rows = lineage.rows.saturating_sub(rows_removed);
            }
            self.record_export(&lineage)?;
            exports_affected.push(AffectedExport {
                export_id: lineage.export_id.clone(),
                dir: lineage.dir.clone(),
                rows_removed,
                revision: lineage.revision,
                missing,
            });
        }
        ledger.append(&LedgerEntry::SubjectDeleted {
            research_subject_id: subject.into(),
            at,
            events_removed,
            episodes_removed,
            exports_affected: exports_affected
                .iter()
                .map(|a| a.export_id.clone())
                .collect(),
        })?;
        Ok(Deletion {
            events_removed,
            episodes_removed,
            exports_affected,
        })
    }
}
