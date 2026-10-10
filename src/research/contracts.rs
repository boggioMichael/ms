//! The data contracts: the envelope every research event carries, the eleven event families,
//! provenance, and the closed vocabularies. `docs/data-program/DATA_CONTRACTS.md` describes them;
//! this file is the source of truth, versioned by [`SCHEMA_VERSION`].
//!
//! Rules the types themselves keep:
//!
//! - **Missing is missing.** Anything not known is `None` and is written as JSON `null` — the key
//!   is always there, the value is never a stand-in `0`.
//! - **A confidence needs a calibration.** [`Confidence`] cannot exist without the reference of
//!   the calibration that gives its number a meaning; an uncalibrated score is not recorded.
//! - **No personal field.** The envelope has no name, no account, no address, no device id; the
//!   observed state is a closed list of game components ([`ComponentName`]) — there is no
//!   "character name", "chat" or "party" component to fill.
//! - **Text from the game is untrusted.** It is kept only as [`UntrustedText`] (its key in JSON is
//!   `untrusted_text`), flagged when it reads like an order, and never acted on.
//! - **Provenance cannot be promoted by a claim.** [`Provenance::check`] refuses a label that says
//!   it was reviewed when no reviewer made it, or a model inference that calls itself verified.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::research::consent::{DataType, Purpose};

/// The version of these contracts (semantic versioning: a new optional field is a minor change;
/// a removed or retyped field, or a changed meaning, is a major one).
pub const SCHEMA_VERSION: &str = "0.1.0";

/// The envelope's fields, in the order they are written. Every event carries every one of them
/// (unknown values as `null`), and nothing else but its `payload`.
pub const ENVELOPE_FIELDS: &[&str] = &[
    "event_id",
    "schema_version",
    "session_id",
    "episode_id",
    "research_subject_id",
    "event_type",
    "game_id",
    "game_variant",
    "world_id",
    "game_build",
    "platform",
    "locale",
    "client_version",
    "detector_version",
    "model_version",
    "sequence_no",
    "monotonic_timestamp",
    "ingested_at",
    "observation_coverage",
    "consent_receipt_id",
    "consent_epoch",
    "collection_purpose",
    "rights_policy_id",
    "sampling_policy",
    "sampling_probability",
];

/// Which game an event comes from. Identified only when the identification is reliable; else
/// `unknown`. A title with no rights manifest can be named but never recorded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GameId {
    /// MapleStory (Nexon), the regular client — any world type.
    Maplestory,
    /// MapleStory Worlds (Nexon) — a different product, with its own terms.
    MaplestoryWorlds,
    /// Data made by `research::synthetic`: no game, no player.
    Synthetic,
    /// A known title that has no identifier here yet.
    Other,
    /// Not reliably identified.
    Unknown,
}

/// The family of an event: what its payload is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventType {
    Session,
    Observation,
    Task,
    Help,
    Assistant,
    PlayerAction,
    Correction,
    Feedback,
    Outcome,
    Experiment,
    CaptureQuality,
}

/// How much of the game the system could see when the event was made.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Coverage {
    Full,
    Partial,
    NotVisible,
    Unknown,
}

/// The fields every event carries (see [`ENVELOPE_FIELDS`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Envelope {
    /// Unique per event; the key for deduplication (a retry writes the same id).
    pub event_id: String,
    pub schema_version: String,
    pub session_id: String,
    /// The episode (one attempt at one goal) the event belongs to; `null` for session-wide events.
    pub episode_id: Option<String>,
    /// A pseudonym for the participant, never a name or account. In an export, a recipient-scoped
    /// pseudonym that does not match any other recipient's.
    pub research_subject_id: String,
    pub event_type: EventType,
    pub game_id: GameId,
    /// The game's variant when known (e.g. a world type such as `classic_world`).
    pub game_variant: Option<String>,
    /// The game world or server when known (a game's name for it, not a player's).
    pub world_id: Option<String>,
    pub game_build: Option<String>,
    pub platform: Option<String>,
    /// The interface language, without region (`en`, `he`).
    pub locale: Option<String>,
    pub client_version: String,
    pub detector_version: Option<String>,
    pub model_version: Option<String>,
    /// The recorder's counter within the session: the order of events.
    pub sequence_no: u64,
    /// Milliseconds since the session's start, from a monotonic clock (comparable within a session
    /// only).
    pub monotonic_timestamp: u64,
    /// When the recorder accepted the event (UTC wall clock).
    pub ingested_at: DateTime<Utc>,
    pub observation_coverage: Coverage,
    pub consent_receipt_id: String,
    pub consent_epoch: u32,
    pub collection_purpose: Purpose,
    pub rights_policy_id: String,
    pub sampling_policy: String,
    /// The probability that an event like this one was kept; `null` when not known.
    pub sampling_probability: Option<f64>,
}

/// One research event: the envelope and its family's payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    #[serde(flatten)]
    pub envelope: Envelope,
    pub payload: Payload,
}

/// The payload of each event family.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Payload {
    Session(SessionEvent),
    Observation(ObservationEvent),
    Task(TaskEvent),
    Help(HelpEvent),
    Assistant(AssistantEvent),
    PlayerAction(PlayerActionEvent),
    Correction(CorrectionEvent),
    Feedback(FeedbackEvent),
    Outcome(OutcomeEvent),
    Experiment(ExperimentEvent),
    CaptureQuality(CaptureQualityEvent),
}

// ---------------------------------------------------------------------------------------------
// Provenance

/// Where a value came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceType {
    /// From the game's publisher (an SDK in a research build, licensed replay data).
    PublisherGroundTruth,
    /// Read from the screen by a detector (still not absolute truth).
    DirectObservation,
    /// Said or chosen by the player (a claim, not a fact).
    HumanAsserted,
    /// Checked by a human reviewer against evidence.
    HumanReviewed,
    /// Inferred by a model or a rule.
    ModelInferred,
    /// Generated.
    Synthetic,
    Unknown,
}

/// Who or what produced a label.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnnotatorType {
    Publisher,
    Detector,
    Player,
    HumanReviewer,
    Model,
    Rule,
    Generator,
    Unknown,
}

/// How far a label has been checked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationStatus {
    Unverified,
    /// A reviewer checked it against its evidence.
    ReviewerVerified,
    /// Adjudicated into a gold set.
    Gold,
    /// Found wrong by a reviewer (kept, marked).
    Rejected,
}

/// A probability with the calibration that gives it a meaning. There is no confidence without one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Confidence {
    /// In `[0, 1]`.
    pub value: f64,
    /// The calibration study or set this number was calibrated against.
    pub calibration_ref: String,
}

/// Where an observation, action, label or outcome came from, and how far it was checked.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Provenance {
    pub source_type: SourceType,
    /// Present only when defined and calibrated; else `null` (never a made-up number).
    pub confidence: Option<Confidence>,
    /// What the value can be checked against (a frame reference, a document, an event).
    pub evidence_ref: Option<String>,
    pub annotator_type: AnnotatorType,
    pub verification_status: VerificationStatus,
    /// The producing process and its version (`name/version`).
    pub producer_version: String,
}

impl Provenance {
    /// A plain provenance: unverified, no confidence, no evidence.
    pub fn new(source_type: SourceType, annotator_type: AnnotatorType, producer: &str) -> Self {
        Provenance {
            source_type,
            confidence: None,
            evidence_ref: None,
            annotator_type,
            verification_status: VerificationStatus::Unverified,
            producer_version: producer.into(),
        }
    }

    pub fn with_evidence(mut self, evidence: &str) -> Self {
        self.evidence_ref = Some(evidence.into());
        self
    }

    pub fn with_confidence(mut self, value: f64, calibration_ref: &str) -> Self {
        self.confidence = Some(Confidence {
            value,
            calibration_ref: calibration_ref.into(),
        });
        self
    }

    /// Whether this provenance is coherent: a review needs a reviewer and evidence, a model's
    /// inference is never verified by itself, a player's word is a player's, a confidence has a
    /// calibration.
    pub fn check(&self) -> Result<(), String> {
        use AnnotatorType as A;
        use SourceType as S;
        use VerificationStatus as V;
        if !is_token(&self.producer_version) {
            return Err("producer_version is not an identifier".into());
        }
        if let Some(evidence) = &self.evidence_ref
            && !is_token(evidence)
        {
            return Err("evidence_ref is not an identifier".into());
        }
        if let Some(confidence) = &self.confidence
            && (!confidence.value.is_finite()
                || !(0.0..=1.0).contains(&confidence.value)
                || !is_token(&confidence.calibration_ref))
        {
            return Err("a confidence outside [0, 1] or without a calibration".into());
        }
        let annotator_fits = match self.source_type {
            S::PublisherGroundTruth => {
                self.annotator_type == A::Publisher && self.evidence_ref.is_some()
            }
            S::DirectObservation => matches!(self.annotator_type, A::Detector | A::Rule),
            S::HumanAsserted => self.annotator_type == A::Player,
            S::HumanReviewed => {
                self.annotator_type == A::HumanReviewer && self.evidence_ref.is_some()
            }
            S::ModelInferred => matches!(self.annotator_type, A::Model | A::Rule),
            S::Synthetic => self.annotator_type == A::Generator,
            S::Unknown => true,
        };
        if !annotator_fits {
            return Err(format!(
                "{:?} cannot come from {:?} (or lacks its evidence)",
                self.source_type, self.annotator_type
            ));
        }
        if matches!(self.verification_status, V::ReviewerVerified | V::Gold)
            && !matches!(self.source_type, S::HumanReviewed | S::PublisherGroundTruth)
        {
            return Err(format!(
                "{:?} cannot be {:?}: only a reviewer's or the publisher's label is",
                self.source_type, self.verification_status
            ));
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------------------------
// Texts

/// What a text was redacted for (the kind only — never what was removed).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Redaction {
    /// A character name the recorder knew (the player's own).
    Name,
    /// A chat line or a whisper — dropped whole.
    Chat,
    Url,
    Email,
    /// A handle (`@someone`, `name#1234`).
    Handle,
    /// Something that looks like a password, a key or a token.
    Credential,
    /// A phone-like number.
    Number,
    /// Cut to the length limit.
    Truncated,
}

/// Where a text from the game was read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextOrigin {
    GameScreen,
    NpcDialog,
    QuestLog,
    SystemNotice,
    /// The chat window: never kept.
    ChatWindow,
    /// A whisper: never kept.
    Whisper,
}

/// Text read from the game: data, never an instruction. `untrusted_text` is `null` when it was
/// withheld (a chat line, a whisper, nothing left after redaction).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UntrustedText {
    pub untrusted_text: Option<String>,
    pub origin: TextOrigin,
    pub redactions: Vec<Redaction>,
    /// It reads like an order to the system ("ignore previous instructions…"). Flagged, not obeyed.
    pub instruction_like: bool,
}

impl UntrustedText {
    /// Raw text from the game, before the sanitizer.
    pub fn raw(text: &str, origin: TextOrigin) -> Self {
        UntrustedText {
            untrusted_text: Some(text.into()),
            origin,
            redactions: Vec::new(),
            instruction_like: false,
        }
    }
}

/// The player's or the assistant's words, after the sanitizer. `text` is `null` when withheld.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SanitizedText {
    pub text: Option<String>,
    pub redactions: Vec<Redaction>,
}

impl SanitizedText {
    /// Raw words, before the sanitizer.
    pub fn raw(text: &str) -> Self {
        SanitizedText {
            text: Some(text.into()),
            redactions: Vec::new(),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// The families

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionAction {
    Start,
    End,
    Pause,
    Resume,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionReason {
    UserStopped,
    SyrupClosed,
    WindowLost,
    FocusLost,
    RecordingEnded,
    Crashed,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionEvent {
    pub action: SessionAction,
    pub reason: Option<SessionReason>,
}

/// The game components an observation may hold — a closed list. There is no component for a
/// character's name, the chat, the party or the guild.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComponentName {
    HpPercent,
    MpPercent,
    ExpPercent,
    Level,
    JobClass,
    Map,
    Screen,
    QuestState,
    DialogText,
    ScreenText,
    NpcVisible,
    PortalVisible,
    BossHpPercent,
    MenuOpen,
    ItemCount,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComponentStatus {
    /// Seen; `value` holds what was read.
    Observed,
    /// Looked for and not read: `value` is `null`.
    Unknown,
    /// Not on screen: `value` is `null`.
    NotVisible,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComponentValue {
    Number(f64),
    /// A game vocabulary identifier (`ellinia_forest`, `night_lord`).
    Category(String),
    Flag(bool),
    Text(UntrustedText),
}

/// One component of the observed state, with its own certainty and provenance.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Component {
    pub name: ComponentName,
    pub status: ComponentStatus,
    pub value: Option<ComponentValue>,
    pub provenance: Provenance,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ObservationEvent {
    pub components: Vec<Component>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskAction {
    GoalSet,
    GoalChanged,
    ConstraintAdded,
    AttemptStarted,
    GoalAbandoned,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GoalKind {
    CompleteQuest,
    ReachLevel,
    DefeatBoss,
    FindNpc,
    FindPlace,
    GetItem,
    LearnMechanic,
    Other,
}

/// Whether the player said the goal, or a model inferred it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GoalOrigin {
    Explicit,
    Inferred,
}

/// The player's constraints on help.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Constraint {
    NoSpoilers,
    HintOnly,
    FindMyself,
    NoPurchase,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Goal {
    pub kind: GoalKind,
    pub origin: GoalOrigin,
    /// The quest, boss or place, as the game names it.
    pub target: Option<UntrustedText>,
    pub constraints: Vec<Constraint>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskEvent {
    pub action: TaskAction,
    pub goal: Option<Goal>,
    pub constraint: Option<Constraint>,
    pub provenance: Provenance,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HelpKind {
    Question,
    HintRequest,
    Stuck,
    Clarification,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Channel {
    Voice,
    Text,
    Button,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HelpEvent {
    pub kind: HelpKind,
    pub channel: Channel,
    /// The request's words, sanitized; `null` for a button or when withheld.
    pub text: Option<SanitizedText>,
    pub provenance: Provenance,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdviceKind {
    Answer,
    Hint,
    Warning,
    ClarifyingQuestion,
    Refusal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdviceSourceKind {
    KnowledgeBase,
    WebLookup,
    ModelKnowledge,
    ScreenReading,
    PlayerNotes,
}

/// What a piece of advice drew on, with its version (game knowledge has a source and a date).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AdviceSource {
    pub kind: AdviceSourceKind,
    pub reference: Option<String>,
    pub version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AssistantEvent {
    pub kind: AdviceKind,
    /// The help request (its `event_id`) this answers, if any.
    pub in_reply_to: Option<String>,
    /// The observations (their `event_id`s) the advice was made from.
    pub based_on: Vec<String>,
    pub sources: Vec<AdviceSource>,
    pub text: Option<SanitizedText>,
    /// When it was shown or began to be spoken (session milliseconds); `null` = not known to have
    /// been shown. Shown is not heard, and heard is not followed.
    pub displayed_at_ms: Option<u64>,
    pub display_ms: Option<u64>,
    pub constraints_respected: Option<bool>,
    pub provenance: Provenance,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionKind {
    Moved,
    TalkedToNpc,
    UsedItem,
    OpenedMenu,
    Fought,
    FollowedAdvice,
    IgnoredAdvice,
    Other,
}

/// How a player action is known. MapleSyrup captures no input: an action is either said by the
/// player, inferred from the state, or given by a publisher's licensed source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Measurement {
    SelfReport,
    InferredFromState,
    PublisherSdk,
    LicensedReplay,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MeasurementLimit {
    TimingApproximate,
    NotDirectlyObserved,
    PartialView,
    SelfReported,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlayerActionEvent {
    pub kind: ActionKind,
    /// The advice (its `event_id`) the player says or seems to follow.
    pub follows_advice: Option<String>,
    pub measurement: Measurement,
    pub limits: Vec<MeasurementLimit>,
    pub provenance: Provenance,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CorrectionCategory {
    WrongObject,
    WrongFact,
    AmbiguousDirection,
    StaleSource,
    MissingInfo,
    WrongOutcome,
    Other,
}

/// A correction is a claim about another event. It never overwrites that event.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CorrectionEvent {
    pub target_event_id: String,
    pub category: CorrectionCategory,
    pub component: Option<ComponentName>,
    pub proposed_value: Option<ComponentValue>,
    pub proposed_outcome: Option<OutcomeKind>,
    pub note: Option<SanitizedText>,
    pub provenance: Provenance,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FeedbackKind {
    Helped,
    NotHelpful,
    DontInterrupt,
    Wrong,
    GoalReached,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FeedbackEvent {
    pub kind: FeedbackKind,
    pub about_event_id: Option<String>,
    pub provenance: Provenance,
}

/// An attempt's result. `unobserved`: the system lost sight before the result could be seen.
/// `censored`: the observation ended (session, recording) while the attempt was still open.
/// Neither is a failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutcomeKind {
    Success,
    Failure,
    Aborted,
    Unobserved,
    Censored,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutcomeReason {
    GoalReached,
    Died,
    TimedOut,
    GaveUp,
    WindowLost,
    SyrupClosed,
    RecordingEnded,
    Other,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OutcomeEvent {
    pub result: OutcomeKind,
    pub reason: Option<OutcomeReason>,
    /// Until when (session milliseconds) the system could see the result.
    pub observable_until_ms: Option<u64>,
    pub provenance: Provenance,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExperimentEvent {
    pub experiment_id: String,
    pub arm: String,
    pub assignment_probability: Option<f64>,
    pub provenance: Provenance,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureStatus {
    Ok,
    Degraded,
    FocusLost,
    NotInView,
    LoginScreen,
    DetectionFailed,
    FramesDropped,
    ClockDrift,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CaptureQualityEvent {
    pub status: CaptureStatus,
    pub dropped_frames: Option<u32>,
    pub clock_offset_ms: Option<i64>,
}

// ---------------------------------------------------------------------------------------------
// Identity of the game and the client (what a recorder stamps on every envelope)

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GameIdentity {
    pub game_id: GameId,
    pub game_variant: Option<String>,
    pub world_id: Option<String>,
    pub game_build: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClientInfo {
    pub platform: Option<String>,
    pub locale: Option<String>,
    pub client_version: String,
    pub detector_version: Option<String>,
    pub model_version: Option<String>,
}

// ---------------------------------------------------------------------------------------------
// Validation

/// An identifier: 1–96 characters of `A-Z a-z 0-9 . _ : + / -`, no `//` (no link), no `@` (no
/// address), no space (no sentence).
pub fn is_token(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 96
        && !text.contains("//")
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "._:+/-".contains(c))
}

fn token(name: &str, text: &str) -> Result<(), String> {
    if is_token(text) {
        Ok(())
    } else {
        Err(format!("{name} is not an identifier"))
    }
}

fn optional_token(name: &str, text: &Option<String>) -> Result<(), String> {
    text.as_deref().map_or(Ok(()), |t| token(name, t))
}

fn probability(name: &str, value: Option<f64>) -> Result<(), String> {
    match value {
        Some(p) if !(p.is_finite() && p > 0.0 && p <= 1.0) => {
            Err(format!("{name} is not a probability in (0, 1]"))
        }
        _ => Ok(()),
    }
}

impl Payload {
    pub fn event_type(&self) -> EventType {
        match self {
            Payload::Session(_) => EventType::Session,
            Payload::Observation(_) => EventType::Observation,
            Payload::Task(_) => EventType::Task,
            Payload::Help(_) => EventType::Help,
            Payload::Assistant(_) => EventType::Assistant,
            Payload::PlayerAction(_) => EventType::PlayerAction,
            Payload::Correction(_) => EventType::Correction,
            Payload::Feedback(_) => EventType::Feedback,
            Payload::Outcome(_) => EventType::Outcome,
            Payload::Experiment(_) => EventType::Experiment,
            Payload::CaptureQuality(_) => EventType::CaptureQuality,
        }
    }

    /// The consent data type this payload is.
    pub fn data_type(&self) -> DataType {
        DataType::of(self.event_type())
    }

    /// Every provenance in the payload.
    pub fn provenances(&self) -> Vec<&Provenance> {
        match self {
            Payload::Session(_) | Payload::CaptureQuality(_) => Vec::new(),
            Payload::Observation(o) => o.components.iter().map(|c| &c.provenance).collect(),
            Payload::Task(t) => vec![&t.provenance],
            Payload::Help(h) => vec![&h.provenance],
            Payload::Assistant(a) => vec![&a.provenance],
            Payload::PlayerAction(a) => vec![&a.provenance],
            Payload::Correction(c) => vec![&c.provenance],
            Payload::Feedback(f) => vec![&f.provenance],
            Payload::Outcome(o) => vec![&o.provenance],
            Payload::Experiment(e) => vec![&e.provenance],
        }
    }

    /// Every reference to another event (`event_id`s) in the payload.
    pub fn references_mut(&mut self) -> Vec<&mut String> {
        match self {
            Payload::Assistant(a) => {
                let mut refs: Vec<&mut String> = a.in_reply_to.iter_mut().collect();
                refs.extend(a.based_on.iter_mut());
                refs
            }
            Payload::PlayerAction(a) => a.follows_advice.iter_mut().collect(),
            Payload::Correction(c) => vec![&mut c.target_event_id],
            Payload::Feedback(f) => f.about_event_id.iter_mut().collect(),
            _ => Vec::new(),
        }
    }

    /// Every untrusted (game) text in the payload.
    pub fn untrusted_texts(&self) -> Vec<&UntrustedText> {
        let mut out = Vec::new();
        match self {
            Payload::Observation(o) => {
                for component in &o.components {
                    if let Some(ComponentValue::Text(text)) = &component.value {
                        out.push(text);
                    }
                }
            }
            Payload::Task(t) => {
                if let Some(target) = t.goal.as_ref().and_then(|g| g.target.as_ref()) {
                    out.push(target);
                }
            }
            Payload::Correction(c) => {
                if let Some(ComponentValue::Text(text)) = &c.proposed_value {
                    out.push(text);
                }
            }
            _ => {}
        }
        out
    }

    fn check(&self) -> Result<(), String> {
        for provenance in self.provenances() {
            provenance.check()?;
        }
        for text in self.untrusted_texts() {
            if matches!(text.origin, TextOrigin::ChatWindow | TextOrigin::Whisper)
                && text.untrusted_text.is_some()
            {
                return Err("a chat line or a whisper is kept".into());
            }
        }
        let mut refs = self.clone();
        for reference in refs.references_mut() {
            token("a reference", reference)?;
        }
        match self {
            Payload::Observation(o) => {
                for component in &o.components {
                    let fits = match component.status {
                        ComponentStatus::Observed => component.value.is_some(),
                        ComponentStatus::Unknown | ComponentStatus::NotVisible => {
                            component.value.is_none()
                        }
                    };
                    if !fits {
                        return Err(format!(
                            "{:?} is {:?} but its value says otherwise",
                            component.name, component.status
                        ));
                    }
                    match &component.value {
                        Some(ComponentValue::Number(n)) if !n.is_finite() => {
                            return Err("a number that is not finite".into());
                        }
                        Some(ComponentValue::Category(c)) => token("a category", c)?,
                        _ => {}
                    }
                }
            }
            Payload::Task(t) => {
                let needs_goal = matches!(t.action, TaskAction::GoalSet | TaskAction::GoalChanged);
                if needs_goal && t.goal.is_none() {
                    return Err("a goal event without its goal".into());
                }
                if let Some(goal) = &t.goal {
                    let source_fits = match goal.origin {
                        GoalOrigin::Explicit => {
                            t.provenance.source_type == SourceType::HumanAsserted
                        }
                        GoalOrigin::Inferred => {
                            t.provenance.source_type == SourceType::ModelInferred
                        }
                    };
                    if !source_fits {
                        return Err("an inferred goal must say it is inferred, a stated one that it is the player's".into());
                    }
                }
            }
            Payload::PlayerAction(a) => {
                let source_fits = match a.measurement {
                    Measurement::SelfReport => {
                        a.provenance.source_type == SourceType::HumanAsserted
                    }
                    Measurement::InferredFromState => {
                        a.provenance.source_type == SourceType::ModelInferred
                    }
                    Measurement::PublisherSdk | Measurement::LicensedReplay => {
                        a.provenance.source_type == SourceType::PublisherGroundTruth
                    }
                };
                if !source_fits {
                    return Err("an action's source does not match how it was measured".into());
                }
            }
            Payload::Experiment(e) => {
                token("experiment_id", &e.experiment_id)?;
                token("arm", &e.arm)?;
                probability("assignment_probability", e.assignment_probability)?;
            }
            Payload::Assistant(a) => {
                for source in &a.sources {
                    optional_token("a source's reference", &source.reference)?;
                    optional_token("a source's version", &source.version)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
}

impl Event {
    /// Whether the event keeps the contract: this schema, a payload of its family, identifiers that
    /// are identifiers, a probability that is one, provenance that holds together, no chat kept.
    pub fn validate(&self) -> Result<(), String> {
        let e = &self.envelope;
        if e.schema_version != SCHEMA_VERSION {
            return Err(format!(
                "schema {} is not {SCHEMA_VERSION}",
                e.schema_version
            ));
        }
        if e.event_type != self.payload.event_type() {
            return Err("event_type does not match the payload".into());
        }
        token("event_id", &e.event_id)?;
        token("session_id", &e.session_id)?;
        optional_token("episode_id", &e.episode_id)?;
        token("research_subject_id", &e.research_subject_id)?;
        optional_token("game_variant", &e.game_variant)?;
        optional_token("world_id", &e.world_id)?;
        optional_token("game_build", &e.game_build)?;
        optional_token("platform", &e.platform)?;
        optional_token("locale", &e.locale)?;
        token("client_version", &e.client_version)?;
        optional_token("detector_version", &e.detector_version)?;
        optional_token("model_version", &e.model_version)?;
        token("consent_receipt_id", &e.consent_receipt_id)?;
        token("rights_policy_id", &e.rights_policy_id)?;
        token("sampling_policy", &e.sampling_policy)?;
        probability("sampling_probability", e.sampling_probability)?;
        self.payload.check()
    }

    /// The identifier-like strings of the envelope (for checks against known names).
    pub fn envelope_tokens(&self) -> Vec<&str> {
        let e = &self.envelope;
        let mut out = vec![
            e.event_id.as_str(),
            e.session_id.as_str(),
            e.research_subject_id.as_str(),
            e.client_version.as_str(),
            e.consent_receipt_id.as_str(),
            e.rights_policy_id.as_str(),
            e.sampling_policy.as_str(),
        ];
        out.extend(
            [
                &e.episode_id,
                &e.game_variant,
                &e.world_id,
                &e.game_build,
                &e.platform,
                &e.locale,
                &e.detector_version,
                &e.model_version,
            ]
            .into_iter()
            .flatten()
            .map(String::as_str),
        );
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_label_cannot_promote_itself() {
        let model = Provenance::new(SourceType::ModelInferred, AnnotatorType::Model, "m/1");
        assert!(model.check().is_ok());
        let mut gold = model.clone();
        gold.verification_status = VerificationStatus::Gold;
        assert!(gold.check().is_err());
        let player_says_reviewed =
            Provenance::new(SourceType::HumanReviewed, AnnotatorType::Player, "p/1")
                .with_evidence("frame:1");
        assert!(player_says_reviewed.check().is_err());
        let reviewer_without_evidence = Provenance::new(
            SourceType::HumanReviewed,
            AnnotatorType::HumanReviewer,
            "r/1",
        );
        assert!(reviewer_without_evidence.check().is_err());
        let mut reviewed = reviewer_without_evidence.with_evidence("frame:1");
        reviewed.verification_status = VerificationStatus::ReviewerVerified;
        assert!(reviewed.check().is_ok());
        let uncalibrated = Provenance::new(
            SourceType::DirectObservation,
            AnnotatorType::Detector,
            "d/1",
        )
        .with_confidence(1.5, "cal/1");
        assert!(uncalibrated.check().is_err());
    }

    #[test]
    fn identifiers_hold_no_link_no_address_and_no_sentence() {
        assert!(is_token("maplesyrup-sight/0.9.0"));
        assert!(is_token("syn-s01a-e0001"));
        for bad in ["", "a b", "x@y.z", "ftp://x.y", "x".repeat(97).as_str()] {
            assert!(!is_token(bad), "{bad:?}");
        }
    }
}
