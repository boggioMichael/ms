//! The consent and rights gate.
//!
//! **Consent** is given per purpose × data type × recipient — never one switch — and kept as a
//! [`ConsentReceipt`]: which text (id, version, language, SHA-256), when, from where, what scope,
//! which epoch, and how adulthood was assured. Receipts, withdrawals, deletions and exports go to
//! a separate, minimal [`ConsentLedger`] (one JSON line each). Nothing is written to it unless a
//! person gives, changes or withdraws consent, or data is deleted or exported: with consent off it
//! does not exist.
//!
//! **Rights** are per title and per deliverable: a [`RightsManifest`] says who holds the rights,
//! on what basis, for which purposes, recipients and uses, where, from when and until when. With
//! no approved basis there is no recording and no export: MapleStory and MapleStory Worlds are
//! *requires title-specific review* (Nexon restricts commercial use of gameplay footage; the
//! sources are listed in DATA_CONTRACTS.md); `synthetic` is approved for local demonstration only.
//!
//! Both are checked when a recorder opens, on every event it records, on every flush, and on every
//! row an export takes — each time with the time of the check.
//!
//! Not legal advice: what the purposes, the texts and the manifests must say is for counsel.

use std::collections::BTreeMap;
use std::fmt;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use chrono::{DateTime, TimeZone, Utc};
use serde::{Deserialize, Serialize};

use crate::research::contracts::{EventType, GameId, is_token};

/// Why data is processed. Each is its own consent; the commercial ones are off unless given.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Purpose {
    /// Running the service the player asked for (inference). Never a source for research.
    ServiceOperation,
    /// Improving Syrup's own detectors, help and timing.
    ImproveSyrup,
    /// Aggregate analytics shared outside (thresholded counts, never rows).
    AggregateAnalytics,
    /// External research, training or evaluation by others.
    ExternalResearchTraining,
    /// Donating media (screens, clips). No path for it exists in P1.
    MediaDonation,
}

impl Purpose {
    pub const ALL: [Purpose; 5] = [
        Purpose::ServiceOperation,
        Purpose::ImproveSyrup,
        Purpose::AggregateAnalytics,
        Purpose::ExternalResearchTraining,
        Purpose::MediaDonation,
    ];

    pub fn code(self) -> &'static str {
        match self {
            Purpose::ServiceOperation => "service_operation",
            Purpose::ImproveSyrup => "improve_syrup",
            Purpose::AggregateAnalytics => "aggregate_analytics",
            Purpose::ExternalResearchTraining => "external_research_training",
            Purpose::MediaDonation => "media_donation",
        }
    }
}

/// What kind of data a consent covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DataType {
    SessionMetadata,
    GameplayState,
    Goals,
    HelpRequests,
    AssistantAdvice,
    PlayerActions,
    Corrections,
    Feedback,
    Outcomes,
    ExperimentAssignments,
    CaptureQuality,
    /// Screens and clips (no path in P1).
    Media,
    /// Voice audio (no path in P1).
    Voice,
}

impl DataType {
    /// The data types of the eleven event families.
    pub const EVENTS: [DataType; 11] = [
        DataType::SessionMetadata,
        DataType::GameplayState,
        DataType::Goals,
        DataType::HelpRequests,
        DataType::AssistantAdvice,
        DataType::PlayerActions,
        DataType::Corrections,
        DataType::Feedback,
        DataType::Outcomes,
        DataType::ExperimentAssignments,
        DataType::CaptureQuality,
    ];

    pub fn of(event_type: EventType) -> DataType {
        match event_type {
            EventType::Session => DataType::SessionMetadata,
            EventType::Observation => DataType::GameplayState,
            EventType::Task => DataType::Goals,
            EventType::Help => DataType::HelpRequests,
            EventType::Assistant => DataType::AssistantAdvice,
            EventType::PlayerAction => DataType::PlayerActions,
            EventType::Correction => DataType::Corrections,
            EventType::Feedback => DataType::Feedback,
            EventType::Outcome => DataType::Outcomes,
            EventType::Experiment => DataType::ExperimentAssignments,
            EventType::CaptureQuality => DataType::CaptureQuality,
        }
    }
}

/// Who receives data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecipientClass {
    /// A folder on this machine (the participant's own copy, a local demonstration).
    ThisDevice,
    /// Syrup's own team, to improve Syrup.
    SyrupTeam,
    /// An outside researcher under a research agreement.
    ExternalResearcher,
    /// A buyer under a licence.
    LicensedBuyer,
}

impl RecipientClass {
    pub const ALL: [RecipientClass; 4] = [
        RecipientClass::ThisDevice,
        RecipientClass::SyrupTeam,
        RecipientClass::ExternalResearcher,
        RecipientClass::LicensedBuyer,
    ];

    pub fn code(self) -> &'static str {
        match self {
            RecipientClass::ThisDevice => "this_device",
            RecipientClass::SyrupTeam => "syrup_team",
            RecipientClass::ExternalResearcher => "external_researcher",
            RecipientClass::LicensedBuyer => "licensed_buyer",
        }
    }
}

/// A recipient: its class (what consent and rights speak of) and its own identifier (what its
/// pseudonyms are scoped to).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Recipient {
    pub class: RecipientClass,
    pub id: String,
}

impl Recipient {
    pub fn new(class: RecipientClass, id: &str) -> Recipient {
        Recipient {
            class,
            id: id.into(),
        }
    }
}

/// One purpose, the data types and the recipients it is given for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScopeGrant {
    pub purpose: Purpose,
    pub data_types: Vec<DataType>,
    pub recipients: Vec<RecipientClass>,
}

/// How the participant's adulthood was assured. Research participation is for adults only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgeAssurance {
    /// "I am 18 or older", kept with the receipt. Whether it is proportionate is for counsel.
    SelfDeclaredAdult,
    NotAssured,
}

/// The text a participant was shown. Its SHA-256 goes into the receipt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConsentText {
    pub text_id: &'static str,
    pub version: &'static str,
    pub language: &'static str,
    pub body: &'static str,
}

impl ConsentText {
    pub fn sha256(&self) -> String {
        crate::research::sha256_hex(self.body.as_bytes())
    }
}

/// A draft of the research consent text (English). A draft for the slice and its synthetic
/// participants — not reviewed by counsel, not shown to anyone.
pub const RESEARCH_CONSENT_DRAFT_EN: ConsentText = ConsentText {
    text_id: "research-participation",
    version: "0.1-draft",
    language: "en",
    body: "DRAFT - not reviewed by counsel. Taking part in Syrup research is separate from using \
           Syrup and is off unless you turn it on. You choose each purpose, each kind of data and \
           each kind of recipient. Syrup keeps what the game shows about your goal, your requests \
           for help, its advice, what happened, and your corrections - after removing names, chat, \
           messages, links, addresses and anything that looks like a password. You can see it, \
           stop it, and delete it at any time; stopping cancels anything not yet saved. You must \
           be 18 or older.",
};

/// A record of consent given: what was shown, when, where, for what, and in which epoch.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConsentReceipt {
    pub receipt_id: String,
    pub research_subject_id: String,
    pub text_id: String,
    pub text_version: String,
    pub text_language: String,
    pub text_sha256: String,
    pub given_at: DateTime<Utc>,
    /// Where it was given: the surface and its version (`phone-settings/0.9.0`).
    pub source: String,
    pub scope: Vec<ScopeGrant>,
    /// Set by the ledger: one more than the subject's last epoch.
    pub epoch: u32,
    pub age_assurance: AgeAssurance,
}

impl ConsentReceipt {
    pub fn new(
        receipt_id: &str,
        research_subject_id: &str,
        text: &ConsentText,
        given_at: DateTime<Utc>,
        source: &str,
        scope: Vec<ScopeGrant>,
        age_assurance: AgeAssurance,
    ) -> ConsentReceipt {
        ConsentReceipt {
            receipt_id: receipt_id.into(),
            research_subject_id: research_subject_id.into(),
            text_id: text.text_id.into(),
            text_version: text.version.into(),
            text_language: text.language.into(),
            text_sha256: text.sha256(),
            given_at,
            source: source.into(),
            scope,
            epoch: 0,
            age_assurance,
        }
    }

    /// Whether this receipt covers `purpose` for `data_type` (and, when given, `recipient`) — or
    /// the first thing it lacks.
    pub fn covers(
        &self,
        purpose: Purpose,
        data_type: Option<DataType>,
        recipient: Option<RecipientClass>,
    ) -> Result<(), Refusal> {
        let for_purpose: Vec<&ScopeGrant> =
            self.scope.iter().filter(|g| g.purpose == purpose).collect();
        if for_purpose.is_empty() {
            return Err(Refusal::PurposeNotConsented(purpose));
        }
        let for_type: Vec<&&ScopeGrant> = match data_type {
            Some(data_type) => for_purpose
                .iter()
                .filter(|g| g.data_types.contains(&data_type))
                .collect(),
            None => for_purpose.iter().collect(),
        };
        if for_type.is_empty() {
            return Err(Refusal::DataTypeNotConsented(
                data_type.unwrap_or(DataType::Media),
            ));
        }
        if let Some(recipient) = recipient
            && !for_type.iter().any(|g| g.recipients.contains(&recipient))
        {
            return Err(Refusal::RecipientNotConsented(recipient));
        }
        Ok(())
    }
}

/// One line of the consent and security ledger.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "entry", rename_all = "snake_case")]
pub enum LedgerEntry {
    Granted(ConsentReceipt),
    Withdrawn {
        receipt_id: String,
        research_subject_id: String,
        at: DateTime<Utc>,
        epoch: u32,
    },
    /// The minimum that proves a deletion was done: counts and the exports it reached.
    SubjectDeleted {
        research_subject_id: String,
        at: DateTime<Utc>,
        events_removed: usize,
        episodes_removed: usize,
        exports_affected: Vec<String>,
    },
    ExportWritten {
        export_id: String,
        revision: u32,
        at: DateTime<Utc>,
        purpose: Purpose,
        recipient_class: RecipientClass,
        recipient_id: String,
        rows: usize,
    },
}

/// A subject's consent as the ledger has it now.
#[derive(Debug, Clone, PartialEq)]
pub struct SubjectConsent {
    pub epoch: u32,
    /// The receipt in force, or none (never given, or withdrawn).
    pub active: Option<String>,
    pub withdrawn: bool,
}

/// The ledger read: every receipt by id, and each subject's consent now.
#[derive(Debug, Clone, Default)]
pub struct LedgerState {
    pub receipts: BTreeMap<String, ConsentReceipt>,
    pub subjects: BTreeMap<String, SubjectConsent>,
}

impl LedgerState {
    /// The receipt in force for `subject`.
    pub fn active(&self, subject: &str) -> Result<&ConsentReceipt, Refusal> {
        let state = self.subjects.get(subject).ok_or(Refusal::NoConsent)?;
        match &state.active {
            Some(id) => self.receipts.get(id).ok_or(Refusal::NoConsent),
            None if state.withdrawn => Err(Refusal::ConsentWithdrawn),
            None => Err(Refusal::NoConsent),
        }
    }

    /// May data of `data_type` (or of any type, when `None`) be collected from `subject` for
    /// `purpose` now?
    pub fn check_collection(
        &self,
        subject: &str,
        purpose: Purpose,
        data_type: Option<DataType>,
    ) -> Result<&ConsentReceipt, Refusal> {
        let receipt = self.active(subject)?;
        if receipt.age_assurance != AgeAssurance::SelfDeclaredAdult {
            return Err(Refusal::NotAdult);
        }
        receipt.covers(purpose, data_type, None)?;
        Ok(receipt)
    }

    /// May a row collected under `collected_under` be exported for `purpose` to `recipient`? Both
    /// the consent in force now and the one it was collected under must cover it: consent is not
    /// widened backwards.
    pub fn check_export(
        &self,
        subject: &str,
        collected_under: &str,
        purpose: Purpose,
        data_type: DataType,
        recipient: RecipientClass,
    ) -> Result<(), Refusal> {
        let now = self.check_collection(subject, purpose, Some(data_type))?;
        now.covers(purpose, Some(data_type), Some(recipient))?;
        let then = self
            .receipts
            .get(collected_under)
            .filter(|r| r.research_subject_id == subject)
            .ok_or(Refusal::ReceiptMismatch)?;
        then.covers(purpose, Some(data_type), Some(recipient))
    }
}

/// The consent and security ledger: an append-only JSON-lines file at a path of its own, apart
/// from the research data. Reading a ledger that does not exist creates nothing.
#[derive(Debug, Clone)]
pub struct ConsentLedger {
    path: PathBuf,
}

impl ConsentLedger {
    pub fn at(path: impl Into<PathBuf>) -> ConsentLedger {
        ConsentLedger { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Every entry, oldest first (none when there is no file). A line that cannot be read is an
    /// error: a ledger is not guessed at.
    pub fn entries(&self) -> std::io::Result<Vec<LedgerEntry>> {
        let text = match std::fs::read_to_string(&self.path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e),
        };
        text.lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| {
                serde_json::from_str(line)
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
            })
            .collect()
    }

    pub fn state(&self) -> std::io::Result<LedgerState> {
        let mut state = LedgerState::default();
        for entry in self.entries()? {
            match entry {
                LedgerEntry::Granted(receipt) => {
                    let subject = state
                        .subjects
                        .entry(receipt.research_subject_id.clone())
                        .or_insert(SubjectConsent {
                            epoch: 0,
                            active: None,
                            withdrawn: false,
                        });
                    subject.epoch = subject.epoch.max(receipt.epoch);
                    subject.active = Some(receipt.receipt_id.clone());
                    subject.withdrawn = false;
                    state.receipts.insert(receipt.receipt_id.clone(), receipt);
                }
                LedgerEntry::Withdrawn {
                    research_subject_id,
                    epoch,
                    ..
                } => {
                    let subject =
                        state
                            .subjects
                            .entry(research_subject_id)
                            .or_insert(SubjectConsent {
                                epoch: 0,
                                active: None,
                                withdrawn: false,
                            });
                    subject.epoch = subject.epoch.max(epoch);
                    subject.active = None;
                    subject.withdrawn = true;
                }
                LedgerEntry::SubjectDeleted { .. } | LedgerEntry::ExportWritten { .. } => {}
            }
        }
        Ok(state)
    }

    /// Record consent given: the receipt gets the subject's next epoch and replaces any receipt
    /// in force. Refused (and not written) for anyone not assured to be an adult, and for a
    /// receipt whose identifiers are not identifiers.
    pub fn grant(&self, mut receipt: ConsentReceipt) -> std::io::Result<ConsentReceipt> {
        let invalid =
            |why: &str| std::io::Error::new(std::io::ErrorKind::InvalidInput, why.to_string());
        if receipt.age_assurance != AgeAssurance::SelfDeclaredAdult {
            return Err(invalid("research participation is for adults only"));
        }
        if ![
            &receipt.receipt_id,
            &receipt.research_subject_id,
            &receipt.text_id,
            &receipt.text_version,
            &receipt.source,
        ]
        .iter()
        .all(|t| is_token(t))
        {
            return Err(invalid("a receipt identifier is not an identifier"));
        }
        let state = self.state()?;
        if state.receipts.contains_key(&receipt.receipt_id) {
            return Err(invalid("a receipt with this id exists"));
        }
        receipt.epoch = state
            .subjects
            .get(&receipt.research_subject_id)
            .map_or(0, |s| s.epoch)
            + 1;
        self.append(&LedgerEntry::Granted(receipt.clone()))?;
        Ok(receipt)
    }

    /// Record a withdrawal: from now on nothing more is collected or exported for `subject`, and a
    /// recorder's pending batch is cancelled at its next step. The epoch of the withdrawal, or
    /// `None` when there was nothing in force.
    pub fn withdraw(&self, subject: &str, at: DateTime<Utc>) -> std::io::Result<Option<u32>> {
        let state = self.state()?;
        let Some(current) = state.subjects.get(subject) else {
            return Ok(None);
        };
        let Some(receipt_id) = current.active.clone() else {
            return Ok(None);
        };
        let epoch = current.epoch + 1;
        self.append(&LedgerEntry::Withdrawn {
            receipt_id,
            research_subject_id: subject.into(),
            at,
            epoch,
        })?;
        Ok(Some(epoch))
    }

    /// Append one entry (one line, one write).
    pub(crate) fn append(&self, entry: &LedgerEntry) -> std::io::Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut line = serde_json::to_string(entry)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        line.push('\n');
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        file.write_all(line.as_bytes())?;
        file.sync_all()
    }
}

// ---------------------------------------------------------------------------------------------
// Rights

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RightsStatus {
    Approved,
    RequiresTitleSpecificReview,
    Denied,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RightsUse {
    Capture,
    Store,
    Annotate,
    TrainInternal,
    TrainExternal,
    Evaluate,
    /// Handing data to a recipient (an export), even to a folder on this machine.
    Transfer,
}

/// What the rights allow for one purpose: to whom, and which uses.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RightsGrant {
    pub purpose: Purpose,
    pub recipients: Vec<RecipientClass>,
    pub uses: Vec<RightsUse>,
}

/// What a rights manifest covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Deliverable {
    /// Events and episodes (metadata, sanitized text) — no pictures, no sound.
    EventMetadata,
    /// Screens, clips, audio.
    Media,
}

/// The rights basis for one title and one kind of deliverable.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RightsManifest {
    pub rights_policy_id: String,
    pub game_id: GameId,
    pub deliverable: Deliverable,
    pub rights_holder: String,
    pub status: RightsStatus,
    /// The evidence of permission, or of an approved analysis; `null` when there is none.
    pub basis: Option<String>,
    /// Versions and worlds covered (empty: none named).
    pub applies_to: Vec<String>,
    pub grants: Vec<RightsGrant>,
    /// Territories (recorded; not enforced in P1, which has no recipient off this machine).
    pub territory: Vec<String>,
    pub valid_from: DateTime<Utc>,
    pub valid_until: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
    /// The role that reviewed it (a role, not a person).
    pub reviewed_by_role: Option<String>,
    pub notes: String,
}

impl RightsManifest {
    fn in_force(&self, at: DateTime<Utc>) -> Result<(), Refusal> {
        if self.status != RightsStatus::Approved {
            return Err(Refusal::RightsNotApproved(self.game_id, self.status));
        }
        if self.revoked_at.is_some_and(|r| r <= at) {
            return Err(Refusal::RightsRevoked(self.rights_policy_id.clone()));
        }
        if at < self.valid_from {
            return Err(Refusal::RightsNotYetValid(self.rights_policy_id.clone()));
        }
        if at >= self.valid_until {
            return Err(Refusal::RightsExpired(self.rights_policy_id.clone()));
        }
        Ok(())
    }
}

/// The rights manifests known to this build, one per title.
#[derive(Debug, Clone)]
pub struct RightsRegistry {
    manifests: BTreeMap<GameId, RightsManifest>,
}

fn utc(y: i32, m: u32, d: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(y, m, d, 0, 0, 0)
        .single()
        .unwrap_or(DateTime::<Utc>::MIN_UTC)
}

impl RightsRegistry {
    /// The manifests this build ships: MapleStory and MapleStory Worlds under review (nothing
    /// allowed), the synthetic title approved for local demonstration only.
    pub fn builtin() -> RightsRegistry {
        let under_review = |game: GameId, id: &str, title: &str| RightsManifest {
            rights_policy_id: id.into(),
            game_id: game,
            deliverable: Deliverable::EventMetadata,
            rights_holder: "Nexon (title not reviewed)".into(),
            status: RightsStatus::RequiresTitleSpecificReview,
            basis: None,
            applies_to: Vec::new(),
            grants: Vec::new(),
            territory: Vec::new(),
            valid_from: utc(2026, 10, 10),
            valid_until: utc(2027, 10, 10),
            revoked_at: None,
            reviewed_by_role: None,
            notes: format!(
                "{title}: requires a title-specific review. Nexon publishes restrictions on \
                 commercial use of gameplay footage (owner directive §10; sources in \
                 DATA_CONTRACTS.md). A player's consent or the code's licence grants no rights in \
                 the game's images, music, third-party content or other players' data; derived \
                 data is not exempt. No capture or export until an approved basis is recorded here."
            ),
        };
        let synthetic = RightsManifest {
            rights_policy_id: "rights-synthetic-local-demo-v1".into(),
            game_id: GameId::Synthetic,
            deliverable: Deliverable::EventMetadata,
            rights_holder: "MapleSyrup (generated by research::synthetic)".into(),
            status: RightsStatus::Approved,
            basis: Some("generated-data-no-game-content-no-player-data".into()),
            applies_to: vec!["synthetic-generator/0.1".into()],
            grants: [
                Purpose::ImproveSyrup,
                Purpose::AggregateAnalytics,
                Purpose::ExternalResearchTraining,
            ]
            .into_iter()
            .map(|purpose| RightsGrant {
                purpose,
                recipients: vec![RecipientClass::ThisDevice],
                uses: vec![
                    RightsUse::Capture,
                    RightsUse::Store,
                    RightsUse::Annotate,
                    RightsUse::Evaluate,
                    RightsUse::Transfer,
                ],
            })
            .collect(),
            territory: vec!["local".into()],
            valid_from: utc(2026, 10, 1),
            valid_until: utc(2027, 10, 1),
            revoked_at: None,
            reviewed_by_role: Some("data-engineering".into()),
            notes: "Approved for local demonstration only: generated participants, no game \
                    footage, no player. Not a basis for any real title."
                .into(),
        };
        let mut manifests = BTreeMap::new();
        for manifest in [
            under_review(
                GameId::Maplestory,
                "rights-maplestory-review-required-v1",
                "MapleStory",
            ),
            under_review(
                GameId::MaplestoryWorlds,
                "rights-maplestory-worlds-review-required-v1",
                "MapleStory Worlds",
            ),
            synthetic,
        ] {
            manifests.insert(manifest.game_id, manifest);
        }
        RightsRegistry { manifests }
    }

    /// This registry with `manifest` in place of its title's.
    pub fn with(mut self, manifest: RightsManifest) -> RightsRegistry {
        self.manifests.insert(manifest.game_id, manifest);
        self
    }

    pub fn get(&self, game: GameId) -> Option<&RightsManifest> {
        self.manifests.get(&game)
    }

    pub fn all(&self) -> impl Iterator<Item = &RightsManifest> {
        self.manifests.values()
    }

    /// May `game` be captured and stored for `purpose` at `at`?
    pub fn check_collection(
        &self,
        game: GameId,
        purpose: Purpose,
        at: DateTime<Utc>,
    ) -> Result<&RightsManifest, Refusal> {
        let manifest = self.get(game).ok_or(Refusal::NoRightsManifest(game))?;
        manifest.in_force(at)?;
        let licensed = manifest.grants.iter().any(|g| {
            g.purpose == purpose
                && g.uses.contains(&RightsUse::Capture)
                && g.uses.contains(&RightsUse::Store)
        });
        if !licensed {
            return Err(Refusal::UseNotLicensed {
                purpose,
                recipient: None,
            });
        }
        Ok(manifest)
    }

    /// May data of `game` be handed to `recipient` for `purpose` at `at`?
    pub fn check_export(
        &self,
        game: GameId,
        purpose: Purpose,
        recipient: RecipientClass,
        at: DateTime<Utc>,
    ) -> Result<&RightsManifest, Refusal> {
        let manifest = self.get(game).ok_or(Refusal::NoRightsManifest(game))?;
        manifest.in_force(at)?;
        let licensed = manifest.grants.iter().any(|g| {
            g.purpose == purpose
                && g.recipients.contains(&recipient)
                && g.uses.contains(&RightsUse::Transfer)
        });
        if !licensed {
            return Err(Refusal::UseNotLicensed {
                purpose,
                recipient: Some(recipient),
            });
        }
        Ok(manifest)
    }
}

// ---------------------------------------------------------------------------------------------
// Refusals

/// Why the gate said no. [`Refusal::code`] is stable (it is what manifests count).
#[derive(Debug, Clone, PartialEq)]
pub enum Refusal {
    NoConsent,
    ConsentWithdrawn,
    PurposeNotConsented(Purpose),
    DataTypeNotConsented(DataType),
    RecipientNotConsented(RecipientClass),
    NotAdult,
    /// The receipt a row was collected under is not the subject's.
    ReceiptMismatch,
    /// Service operation (inference) never feeds the research store.
    NotAResearchPurpose(Purpose),
    /// Media donation has no path in P1.
    MediaPathNotBuilt,
    NoRightsManifest(GameId),
    RightsNotApproved(GameId, RightsStatus),
    RightsRevoked(String),
    RightsNotYetValid(String),
    RightsExpired(String),
    UseNotLicensed {
        purpose: Purpose,
        recipient: Option<RecipientClass>,
    },
    InvalidEvent(String),
    Io(String),
    /// No row may go: each reason, with how many rows it kept out.
    NothingEligible(BTreeMap<String, usize>),
    ExportExists(PathBuf),
    /// Fixed (guessable) pseudonym keys are for synthetic data only.
    RealDataNeedsRandomKeys,
}

impl Refusal {
    pub fn code(&self) -> &'static str {
        match self {
            Refusal::NoConsent => "no_consent",
            Refusal::ConsentWithdrawn => "consent_withdrawn",
            Refusal::PurposeNotConsented(_) => "purpose_not_consented",
            Refusal::DataTypeNotConsented(_) => "data_type_not_consented",
            Refusal::RecipientNotConsented(_) => "recipient_not_consented",
            Refusal::NotAdult => "not_adult",
            Refusal::ReceiptMismatch => "receipt_mismatch",
            Refusal::NotAResearchPurpose(_) => "not_a_research_purpose",
            Refusal::MediaPathNotBuilt => "media_path_not_built",
            Refusal::NoRightsManifest(_) => "no_rights_manifest",
            Refusal::RightsNotApproved(..) => "rights_not_approved",
            Refusal::RightsRevoked(_) => "rights_revoked",
            Refusal::RightsNotYetValid(_) => "rights_not_yet_valid",
            Refusal::RightsExpired(_) => "rights_expired",
            Refusal::UseNotLicensed { .. } => "use_not_licensed",
            Refusal::InvalidEvent(_) => "invalid_event",
            Refusal::Io(_) => "io",
            Refusal::NothingEligible(_) => "nothing_eligible",
            Refusal::ExportExists(_) => "export_exists",
            Refusal::RealDataNeedsRandomKeys => "real_data_needs_random_keys",
        }
    }

    /// For [`Refusal::NothingEligible`], the reasons and their counts; else this refusal alone.
    pub fn reasons(&self) -> BTreeMap<String, usize> {
        match self {
            Refusal::NothingEligible(reasons) => reasons.clone(),
            other => BTreeMap::from([(other.code().to_string(), 1)]),
        }
    }

    /// Whether this refusal ends a recorder (rather than refusing one event).
    pub fn closes_recorder(&self) -> bool {
        matches!(
            self,
            Refusal::NoConsent
                | Refusal::ConsentWithdrawn
                | Refusal::NotAdult
                | Refusal::PurposeNotConsented(_)
                | Refusal::NoRightsManifest(_)
                | Refusal::RightsNotApproved(..)
                | Refusal::RightsRevoked(_)
                | Refusal::RightsNotYetValid(_)
                | Refusal::RightsExpired(_)
                | Refusal::UseNotLicensed { .. }
        )
    }
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Refusal::NothingEligible(reasons) => {
                write!(f, "nothing_eligible:")?;
                for (reason, n) in reasons {
                    write!(f, " {reason}={n}")?;
                }
                Ok(())
            }
            Refusal::InvalidEvent(why) | Refusal::Io(why) => write!(f, "{}: {why}", self.code()),
            Refusal::ExportExists(path) => write!(f, "{}: {}", self.code(), path.display()),
            other => write!(f, "{} ({other:?})", other.code()),
        }
    }
}

impl std::error::Error for Refusal {}

#[cfg(test)]
mod tests {
    use super::*;

    fn receipt(scope: Vec<ScopeGrant>) -> ConsentReceipt {
        ConsentReceipt::new(
            "r-1",
            "s-1",
            &RESEARCH_CONSENT_DRAFT_EN,
            utc(2026, 10, 2),
            "test/1",
            scope,
            AgeAssurance::SelfDeclaredAdult,
        )
    }

    #[test]
    fn a_receipt_names_what_it_lacks() {
        let r = receipt(vec![ScopeGrant {
            purpose: Purpose::ImproveSyrup,
            data_types: vec![DataType::Goals],
            recipients: vec![RecipientClass::SyrupTeam],
        }]);
        assert!(
            r.covers(
                Purpose::ImproveSyrup,
                Some(DataType::Goals),
                Some(RecipientClass::SyrupTeam)
            )
            .is_ok()
        );
        assert_eq!(
            r.covers(Purpose::AggregateAnalytics, None, None)
                .unwrap_err()
                .code(),
            "purpose_not_consented"
        );
        assert_eq!(
            r.covers(Purpose::ImproveSyrup, Some(DataType::HelpRequests), None)
                .unwrap_err()
                .code(),
            "data_type_not_consented"
        );
        assert_eq!(
            r.covers(
                Purpose::ImproveSyrup,
                Some(DataType::Goals),
                Some(RecipientClass::LicensedBuyer)
            )
            .unwrap_err()
            .code(),
            "recipient_not_consented"
        );
    }

    #[test]
    fn epochs_count_up_and_a_withdrawal_ends_what_was_in_force() {
        let dir = std::env::temp_dir().join(format!(
            "ms-research-ledger-{}",
            crate::research::random_hex(4).unwrap()
        ));
        let ledger = ConsentLedger::at(dir.join("ledger.jsonl"));
        assert_eq!(ledger.withdraw("s-1", utc(2026, 10, 3)).unwrap(), None);
        assert!(!dir.exists(), "withdrawing nothing wrote something");
        let first = ledger.grant(receipt(Vec::new())).unwrap();
        assert_eq!(first.epoch, 1);
        assert_eq!(ledger.withdraw("s-1", utc(2026, 10, 3)).unwrap(), Some(2));
        let state = ledger.state().unwrap();
        assert_eq!(state.active("s-1").unwrap_err().code(), "consent_withdrawn");
        let mut again = receipt(Vec::new());
        again.receipt_id = "r-2".into();
        assert_eq!(ledger.grant(again).unwrap().epoch, 3);
        let mut child = receipt(Vec::new());
        child.receipt_id = "r-3".into();
        child.age_assurance = AgeAssurance::NotAssured;
        assert!(ledger.grant(child).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
