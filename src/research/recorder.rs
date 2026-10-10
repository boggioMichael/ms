//! The recorder: the only way events enter the research store, and one that exists only when the
//! gate allows it.
//!
//! [`Recorder::open`] returns `None` — and writes nothing, creates no folder — unless the subject's
//! consent in force covers the purpose (an adult's, for research: never "service operation", whose
//! inference path does not feed research), and the title's rights manifest is approved, in force
//! and licenses capture and storage for that purpose. Then every [`Recorder::record`] and every
//! [`Recorder::flush`] asks again, with the time of the call: a withdrawal, a narrowed consent or
//! a rights policy that ran out cancels the pending batch (nothing of it is written) and ends the
//! recorder.
//!
//! Each event is sanitized before it is kept (see [`crate::research::sanitize`]), stamped with the
//! whole envelope, checked against the contract, and held in a bounded batch until a flush appends
//! it to the spool. The recorder never reads the screen, the microphone or the keyboard: callers
//! hand it drafts.

use std::path::PathBuf;

use chrono::{DateTime, Utc};

use crate::research::consent::{ConsentLedger, Purpose, Refusal, RightsRegistry};
use crate::research::contracts::{
    ClientInfo, Coverage, Envelope, Event, GameIdentity, Payload, SCHEMA_VERSION,
};
use crate::research::sanitize::{Sanitizer, Tally};
use crate::research::store::ResearchStore;

/// How event ids are made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdSource {
    /// Random (UUID v4 from the operating system): for anything real.
    Random,
    /// `<prefix>-e0001`, `<prefix>-e0002`, …: for synthetic data, so that it is reproducible.
    Sequential { prefix: String },
}

/// What a recorder records for: who, which session, which purpose, which game and client.
#[derive(Debug, Clone)]
pub struct RecorderConfig {
    /// The research folder (created only when the first batch is written).
    pub research_root: PathBuf,
    pub research_subject_id: String,
    pub session_id: String,
    pub purpose: Purpose,
    pub game: GameIdentity,
    pub client: ClientInfo,
    pub sampling_policy: String,
    pub sampling_probability: Option<f64>,
    /// Names the sanitizer removes (the player's own characters). Never written.
    pub known_names: Vec<String>,
    /// Events held before a flush; reaching it flushes.
    pub max_pending: usize,
    pub ids: IdSource,
}

/// An event before the recorder: its payload (texts raw), its place in the session.
#[derive(Debug, Clone)]
pub struct Draft {
    pub episode_id: Option<String>,
    /// Milliseconds since the session started (monotonic).
    pub at_ms: u64,
    pub coverage: Coverage,
    /// The model behind this event, when it differs from the client's.
    pub model_version: Option<String>,
    pub payload: Payload,
}

impl Draft {
    pub fn new(at_ms: u64, payload: Payload) -> Draft {
        Draft {
            episode_id: None,
            at_ms,
            coverage: Coverage::Full,
            model_version: None,
            payload,
        }
    }

    pub fn episode(mut self, episode_id: &str) -> Draft {
        self.episode_id = Some(episode_id.into());
        self
    }

    pub fn coverage(mut self, coverage: Coverage) -> Draft {
        self.coverage = coverage;
        self
    }

    pub fn model(mut self, model_version: &str) -> Draft {
        self.model_version = Some(model_version.into());
        self
    }
}

/// A recorder for one subject's session (see the module's documentation).
#[derive(Debug)]
pub struct Recorder {
    config: RecorderConfig,
    ledger: ConsentLedger,
    rights: RightsRegistry,
    store: ResearchStore,
    sanitizer: Sanitizer,
    pending: Vec<Event>,
    next_sequence: u64,
    last_ms: Option<u64>,
    cancelled: usize,
    written: usize,
    closed: Option<Refusal>,
    tally: Tally,
}

impl Recorder {
    /// A recorder, if the gate allows one now; else `None` (and nothing written).
    pub fn open(
        config: RecorderConfig,
        ledger: &ConsentLedger,
        rights: &RightsRegistry,
        now: DateTime<Utc>,
    ) -> Option<Recorder> {
        Recorder::try_open(config, ledger, rights, now).ok()
    }

    /// A recorder, or why not.
    pub fn try_open(
        config: RecorderConfig,
        ledger: &ConsentLedger,
        rights: &RightsRegistry,
        now: DateTime<Utc>,
    ) -> Result<Recorder, Refusal> {
        match config.purpose {
            Purpose::ServiceOperation => {
                return Err(Refusal::NotAResearchPurpose(config.purpose));
            }
            Purpose::MediaDonation => return Err(Refusal::MediaPathNotBuilt),
            _ => {}
        }
        let state = ledger.state().map_err(|e| Refusal::Io(e.to_string()))?;
        state.check_collection(&config.research_subject_id, config.purpose, None)?;
        rights.check_collection(config.game.game_id, config.purpose, now)?;
        let sanitizer = Sanitizer::new(&config.known_names);
        Ok(Recorder {
            store: ResearchStore::at(&config.research_root),
            ledger: ledger.clone(),
            rights: rights.clone(),
            sanitizer,
            config,
            pending: Vec::new(),
            next_sequence: 1,
            last_ms: None,
            cancelled: 0,
            written: 0,
            closed: None,
            tally: Tally::new(),
        })
    }

    /// Events held, not yet written.
    pub fn pending(&self) -> usize {
        self.pending.len()
    }

    /// Events dropped from a batch because the gate closed before it was written.
    pub fn cancelled(&self) -> usize {
        self.cancelled
    }

    pub fn written(&self) -> usize {
        self.written
    }

    /// Why this recorder ended, if it has.
    pub fn closed(&self) -> Option<&Refusal> {
        self.closed.as_ref()
    }

    /// The redactions made so far, by kind (counts only).
    pub fn redactions(&self) -> &Tally {
        &self.tally
    }

    /// Ask the gate again; a refusal that ends the recorder cancels what is pending.
    fn gate(
        &mut self,
        data_type: Option<crate::research::consent::DataType>,
        now: DateTime<Utc>,
    ) -> Result<(String, u32, String), Refusal> {
        if let Some(refusal) = &self.closed {
            return Err(refusal.clone());
        }
        let checked = self
            .ledger
            .state()
            .map_err(|e| Refusal::Io(e.to_string()))
            .and_then(|state| {
                let receipt = state.check_collection(
                    &self.config.research_subject_id,
                    self.config.purpose,
                    data_type,
                )?;
                let manifest = self.rights.check_collection(
                    self.config.game.game_id,
                    self.config.purpose,
                    now,
                )?;
                Ok((
                    receipt.receipt_id.clone(),
                    receipt.epoch,
                    manifest.rights_policy_id.clone(),
                ))
            });
        if let Err(refusal) = &checked
            && refusal.closes_recorder()
        {
            self.cancelled += self.pending.len();
            self.pending.clear();
            self.closed = Some(refusal.clone());
        }
        checked
    }

    fn next_event_id(&self) -> Result<String, Refusal> {
        match &self.config.ids {
            IdSource::Random => crate::research::random_hex(16)
                .map(|hex| {
                    format!(
                        "{}-{}-4{}-{}-{}",
                        &hex[0..8],
                        &hex[8..12],
                        &hex[13..16],
                        &hex[16..20],
                        &hex[20..32]
                    )
                })
                .ok_or_else(|| Refusal::Io("no randomness for an event id".into())),
            IdSource::Sequential { prefix } => Ok(format!("{prefix}-e{:04}", self.next_sequence)),
        }
    }

    /// Sanitize, stamp, check and hold one event; its `event_id`, or why it was refused.
    pub fn record(&mut self, draft: Draft, now: DateTime<Utc>) -> Result<String, Refusal> {
        let (receipt_id, epoch, rights_policy_id) =
            self.gate(Some(draft.payload.data_type()), now)?;
        if self.last_ms.is_some_and(|last| draft.at_ms < last) {
            return Err(Refusal::InvalidEvent(
                "the session's monotonic clock went back".into(),
            ));
        }
        let mut payload = draft.payload;
        self.sanitizer.payload(&mut payload, &mut self.tally);
        let c = &self.config;
        let event = Event {
            envelope: Envelope {
                event_id: self.next_event_id()?,
                schema_version: SCHEMA_VERSION.into(),
                session_id: c.session_id.clone(),
                episode_id: draft.episode_id,
                research_subject_id: c.research_subject_id.clone(),
                event_type: payload.event_type(),
                game_id: c.game.game_id,
                game_variant: c.game.game_variant.clone(),
                world_id: c.game.world_id.clone(),
                game_build: c.game.game_build.clone(),
                platform: c.client.platform.clone(),
                locale: c.client.locale.clone(),
                client_version: c.client.client_version.clone(),
                detector_version: c.client.detector_version.clone(),
                model_version: draft
                    .model_version
                    .or_else(|| c.client.model_version.clone()),
                sequence_no: self.next_sequence,
                monotonic_timestamp: draft.at_ms,
                ingested_at: now,
                observation_coverage: draft.coverage,
                consent_receipt_id: receipt_id,
                consent_epoch: epoch,
                collection_purpose: c.purpose,
                rights_policy_id,
                sampling_policy: c.sampling_policy.clone(),
                sampling_probability: c.sampling_probability,
            },
            payload,
        };
        event.validate().map_err(Refusal::InvalidEvent)?;
        if event
            .envelope_tokens()
            .iter()
            .any(|token| self.sanitizer.holds_name(token))
        {
            return Err(Refusal::InvalidEvent(
                "an identifier holds a character's name".into(),
            ));
        }
        let id = event.envelope.event_id.clone();
        self.next_sequence += 1;
        self.last_ms = Some(event.envelope.monotonic_timestamp);
        self.pending.push(event);
        if self.pending.len() >= self.config.max_pending.max(1) {
            self.flush(now)?;
        }
        Ok(id)
    }

    /// Write the pending batch to the spool — if the gate still allows it now. How many were
    /// written; a refusal that ends the recorder cancels the batch instead.
    pub fn flush(&mut self, now: DateTime<Utc>) -> Result<usize, Refusal> {
        self.gate(None, now)?;
        if self.pending.is_empty() {
            return Ok(0);
        }
        self.store
            .append_events(&self.pending)
            .map_err(|e| Refusal::Io(e.to_string()))?;
        let n = self.pending.len();
        self.written += n;
        self.pending.clear();
        Ok(n)
    }
}
