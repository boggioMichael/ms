//! The episode builder: from the events of a spool (in any order, with retries' copies) to
//! episodes — one attempt at one goal each, with its failures, help, corrections and outcome.
//!
//! - **Validated, then deduplicated by `event_id`.** An event that breaks the contract is
//!   quarantined; exact copies count once; two different events under one id are both kept out
//!   (neither can be believed).
//! - **Ordered by `sequence_no`** within a session (sessions by when they were first ingested), not
//!   by arrival: the same events in any order build the same episodes.
//! - **Nothing later is an input to anything earlier.** The state before an episode is the last
//!   observation before it began; an advice's inputs are observations made before it — a link to a
//!   later one is dropped and counted.
//! - **An outcome is what was seen.** An attempt with no outcome event ends `unobserved` when the
//!   system lost sight of the game before the end (window lost, focus lost), `censored` when the
//!   observation ended while it still saw (Syrup closed, recording ended) — never `failure`. When
//!   the system could last see it is kept.
//! - **Corrections are claims.** They are kept beside what they correct, with their provenance;
//!   nothing is overwritten, and in P1 nothing is applied.

use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub use crate::research::contracts::OutcomeKind;
use crate::research::contracts::{
    AdviceKind, AdviceSource, CaptureStatus, Component, ComponentName, ComponentValue, Constraint,
    CorrectionCategory, Coverage, Event, ExperimentEvent, FeedbackEvent, GameId, Goal, HelpEvent,
    OutcomeReason, Payload, PlayerActionEvent, Provenance, SanitizedText, SessionAction,
    SessionReason, TaskAction, VerificationStatus,
};

/// An event's content with where it sits in its session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stamped<T> {
    pub event_id: String,
    pub at_ms: u64,
    pub data: T,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GoalView {
    pub event_id: String,
    pub at_ms: u64,
    pub goal: Goal,
    /// `human_asserted` for a goal the player stated, `model_inferred` for one inferred.
    pub provenance: Provenance,
}

/// The observed state at one moment, component by component.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StateView {
    pub event_id: String,
    pub session_id: String,
    pub at_ms: u64,
    pub coverage: Coverage,
    pub components: Vec<Component>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AdviceView {
    pub event_id: String,
    pub at_ms: u64,
    pub kind: AdviceKind,
    pub in_reply_to: Option<String>,
    /// The observations it was made from — earlier ones only.
    pub based_on: Vec<String>,
    /// Links to inputs that were dropped (later than the advice, or not observations of its
    /// session).
    pub rejected_inputs: usize,
    pub sources: Vec<AdviceSource>,
    pub text: Option<SanitizedText>,
    pub displayed_at_ms: Option<u64>,
    pub display_ms: Option<u64>,
    pub constraints_respected: Option<bool>,
    pub model_version: Option<String>,
    pub provenance: Provenance,
}

/// How an attempt's result was determined: by an event (with its provenance), or by a rule of the
/// builder (with the event that triggered it).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Determination {
    Event {
        event_id: String,
        provenance: Provenance,
    },
    Rule {
        rule: String,
        evidence_event_id: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AttemptView {
    pub attempt_no: u32,
    pub started_at_ms: Option<u64>,
    pub ended_at_ms: Option<u64>,
    pub result: OutcomeKind,
    pub reason: Option<OutcomeReason>,
    pub determined_by: Determination,
    /// Until when the system could see the result (session milliseconds).
    pub observable_until_ms: Option<u64>,
    pub help_requests: u32,
    pub advice_shown: u32,
    /// Advice given during the attempt whose showing is not known.
    pub advice_display_unknown: u32,
}

/// What became of a correction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaimStatus {
    /// Not checked by anyone: a claim.
    Claimed,
    /// Checked by a reviewer against evidence.
    Verified,
    /// Found wrong by a reviewer.
    Rejected,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CorrectionView {
    pub event_id: String,
    pub at_ms: u64,
    pub target_event_id: String,
    pub category: CorrectionCategory,
    pub component: Option<ComponentName>,
    pub proposed_value: Option<ComponentValue>,
    pub proposed_outcome: Option<OutcomeKind>,
    pub note: Option<SanitizedText>,
    pub provenance: Provenance,
    pub status: ClaimStatus,
    /// Whether it changed anything in this episode. Never in P1: what it targets is kept as it was.
    pub applied: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EpisodeLineage {
    /// The episode's own events, in order.
    pub event_ids: Vec<String>,
    pub consent_receipt_ids: Vec<String>,
    pub rights_policy_ids: Vec<String>,
}

/// One attempt at one goal: goal → state before → help → advice → actions → outcome →
/// corrections.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Episode {
    pub episode_id: String,
    pub schema_version: String,
    pub research_subject_id: String,
    pub session_ids: Vec<String>,
    pub game_id: GameId,
    pub game_variant: Option<String>,
    pub game_build: Option<String>,
    pub goal: Option<GoalView>,
    pub goal_changes: usize,
    pub constraints: Vec<Constraint>,
    pub state_before: Option<StateView>,
    pub help_requests: Vec<Stamped<HelpEvent>>,
    pub advice: Vec<AdviceView>,
    pub actions: Vec<Stamped<PlayerActionEvent>>,
    pub attempts: Vec<AttemptView>,
    pub state_after: Option<StateView>,
    /// The last attempt's.
    pub outcome: AttemptView,
    pub corrections: Vec<CorrectionView>,
    pub feedback: Vec<Stamped<FeedbackEvent>>,
    pub experiments: Vec<Stamped<ExperimentEvent>>,
    /// Capture-quality problems seen during the episode.
    pub capture_issues: usize,
    /// From the goal (or the episode's first event) to the first success, within one session;
    /// `null` when there was no success or the two are in different sessions.
    pub time_to_first_success_ms: Option<u64>,
    /// Whether advice was shown before the first success (or the end): `null` when advice was
    /// given but whether it was shown is not known.
    pub assisted: Option<bool>,
    pub instruction_like_texts: usize,
    pub lineage: EpisodeLineage,
}

/// What the builder saw of the input's quality.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct BuildQuality {
    pub events_in: usize,
    pub events_used: usize,
    pub duplicates_dropped: usize,
    pub conflicting_duplicates: usize,
    pub quarantined: BTreeMap<String, usize>,
    pub sequence_gaps: usize,
    pub clock_anomalies: usize,
    pub future_inputs_rejected: usize,
    pub dangling_refs: usize,
    /// Events that need an episode (help, advice, outcome…) and have none.
    pub orphan_events: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Built {
    pub episodes: Vec<Episode>,
    pub quality: BuildQuality,
}

/// Validate, deduplicate and order `events`.
pub fn prepare(events: &[Event]) -> (Vec<Event>, BuildQuality) {
    let mut quality = BuildQuality {
        events_in: events.len(),
        ..BuildQuality::default()
    };
    let mut by_id: BTreeMap<&str, Vec<&Event>> = BTreeMap::new();
    for event in events {
        if event.validate().is_err() {
            *quality
                .quarantined
                .entry("invalid_event".into())
                .or_insert(0) += 1;
            continue;
        }
        by_id
            .entry(event.envelope.event_id.as_str())
            .or_default()
            .push(event);
    }
    let mut unique: Vec<Event> = Vec::new();
    for copies in by_id.values() {
        let first = copies[0];
        if copies.iter().all(|copy| *copy == first) {
            quality.duplicates_dropped += copies.len() - 1;
            unique.push(first.clone());
        } else {
            quality.conflicting_duplicates += 1;
            *quality
                .quarantined
                .entry("conflicting_duplicate".into())
                .or_insert(0) += copies.len();
        }
    }
    let mut first_seen: BTreeMap<String, DateTime<Utc>> = BTreeMap::new();
    for event in &unique {
        let seen = first_seen
            .entry(event.envelope.session_id.clone())
            .or_insert(event.envelope.ingested_at);
        *seen = (*seen).min(event.envelope.ingested_at);
    }
    let mut sessions: Vec<(DateTime<Utc>, String)> =
        first_seen.into_iter().map(|(s, at)| (at, s)).collect();
    sessions.sort();
    let rank: BTreeMap<String, usize> = sessions
        .into_iter()
        .enumerate()
        .map(|(i, (_, s))| (s, i))
        .collect();
    unique.sort_by(|a, b| {
        let key = |e: &Event| {
            (
                rank[&e.envelope.session_id],
                e.envelope.sequence_no,
                e.envelope.event_id.clone(),
            )
        };
        key(a).cmp(&key(b))
    });
    for pair in unique.windows(2) {
        let (a, b) = (&pair[0].envelope, &pair[1].envelope);
        if a.session_id != b.session_id {
            continue;
        }
        if b.sequence_no > a.sequence_no + 1 {
            quality.sequence_gaps += 1;
        }
        if b.monotonic_timestamp < a.monotonic_timestamp {
            quality.clock_anomalies += 1;
        }
    }
    quality.events_used = unique.len();
    (unique, quality)
}

/// Build the episodes of `events` (in any order, duplicates and all).
pub fn build(events: &[Event]) -> Built {
    let (ordered, mut quality) = prepare(events);
    let index: BTreeMap<&str, usize> = ordered
        .iter()
        .enumerate()
        .map(|(i, e)| (e.envelope.event_id.as_str(), i))
        .collect();
    let mut groups: BTreeMap<(&str, &str), Vec<usize>> = BTreeMap::new();
    for (i, event) in ordered.iter().enumerate() {
        match &event.envelope.episode_id {
            Some(episode) => groups
                .entry((
                    event.envelope.research_subject_id.as_str(),
                    episode.as_str(),
                ))
                .or_default()
                .push(i),
            None => {
                if !matches!(
                    event.payload,
                    Payload::Session(_)
                        | Payload::Observation(_)
                        | Payload::CaptureQuality(_)
                        | Payload::Experiment(_)
                ) {
                    quality.orphan_events += 1;
                }
            }
        }
    }
    let mut built: Vec<(usize, Episode)> = groups
        .values()
        .map(|positions| {
            (
                positions[0],
                assemble(&ordered, &index, positions, &mut quality),
            )
        })
        .collect();
    built.sort_by_key(|(first, _)| *first);
    Built {
        episodes: built.into_iter().map(|(_, e)| e).collect(),
        quality,
    }
}

struct Open {
    started_ms: Option<u64>,
    help: u32,
    advice: u32,
    advice_unknown: u32,
}

impl Open {
    fn at(ms: u64) -> Open {
        Open {
            started_ms: Some(ms),
            help: 0,
            advice: 0,
            advice_unknown: 0,
        }
    }
}

fn state_view(event: &Event) -> Option<StateView> {
    match &event.payload {
        Payload::Observation(o) => Some(StateView {
            event_id: event.envelope.event_id.clone(),
            session_id: event.envelope.session_id.clone(),
            at_ms: event.envelope.monotonic_timestamp,
            coverage: event.envelope.observation_coverage,
            components: o.components.clone(),
        }),
        _ => None,
    }
}

fn same_place(a: &Event, b: &Event) -> bool {
    a.envelope.session_id == b.envelope.session_id
        && a.envelope.research_subject_id == b.envelope.research_subject_id
}

/// How an attempt left open ends: by what the session shows after the episode began.
fn close_by_session(
    ordered: &[Event],
    start: usize,
    last: usize,
    open: Open,
    number: u32,
) -> AttemptView {
    let anchor = &ordered[last];
    let mut view_lost: Option<usize> = None;
    let mut ended: Option<usize> = None;
    let mut last_seen = anchor.envelope.monotonic_timestamp;
    for (i, event) in ordered.iter().enumerate().skip(start) {
        if !same_place(event, anchor) {
            continue;
        }
        last_seen = last_seen.max(event.envelope.monotonic_timestamp);
        match &event.payload {
            Payload::Session(s)
                if s.action == SessionAction::Pause
                    && matches!(
                        s.reason,
                        Some(SessionReason::WindowLost | SessionReason::FocusLost)
                    ) =>
            {
                view_lost.get_or_insert(i);
            }
            Payload::CaptureQuality(c)
                if matches!(
                    c.status,
                    CaptureStatus::FocusLost
                        | CaptureStatus::NotInView
                        | CaptureStatus::LoginScreen
                ) =>
            {
                view_lost.get_or_insert(i);
            }
            Payload::Session(s) if s.action == SessionAction::Resume => view_lost = None,
            Payload::CaptureQuality(c) if c.status == CaptureStatus::Ok => view_lost = None,
            Payload::Session(s) if s.action == SessionAction::End && i > last => {
                ended = Some(i);
                break;
            }
            _ => {}
        }
    }
    let (result, reason, rule, evidence, until) = match (view_lost, ended) {
        (Some(i), _) => (
            OutcomeKind::Unobserved,
            Some(OutcomeReason::WindowLost),
            "view_lost_before_outcome",
            Some(i),
            Some(ordered[i].envelope.monotonic_timestamp),
        ),
        (None, Some(i)) => {
            let reason = match &ordered[i].payload {
                Payload::Session(s) => match s.reason {
                    Some(SessionReason::RecordingEnded) => OutcomeReason::RecordingEnded,
                    Some(SessionReason::SyrupClosed | SessionReason::Crashed) => {
                        OutcomeReason::SyrupClosed
                    }
                    _ => OutcomeReason::Other,
                },
                _ => OutcomeReason::Other,
            };
            (
                OutcomeKind::Censored,
                Some(reason),
                "observation_ended_before_outcome",
                Some(i),
                Some(ordered[i].envelope.monotonic_timestamp),
            )
        }
        (None, None) => (
            OutcomeKind::Unobserved,
            None,
            "no_outcome_in_data",
            None,
            Some(last_seen),
        ),
    };
    AttemptView {
        attempt_no: number,
        started_at_ms: open.started_ms,
        ended_at_ms: None,
        result,
        reason,
        determined_by: Determination::Rule {
            rule: rule.into(),
            evidence_event_id: evidence.map(|i| ordered[i].envelope.event_id.clone()),
        },
        observable_until_ms: until,
        help_requests: open.help,
        advice_shown: open.advice,
        advice_display_unknown: open.advice_unknown,
    }
}

fn assemble(
    ordered: &[Event],
    index: &BTreeMap<&str, usize>,
    positions: &[usize],
    quality: &mut BuildQuality,
) -> Episode {
    let first = &ordered[positions[0]];
    let start = positions[0];
    let last = *positions.last().unwrap_or(&start);
    let events: Vec<(usize, &Event)> = positions.iter().map(|&i| (i, &ordered[i])).collect();

    let mut goal = None;
    let mut goal_changes = 0;
    let mut constraints: BTreeSet<Constraint> = BTreeSet::new();
    let mut help_requests = Vec::new();
    let mut advice = Vec::new();
    let mut actions = Vec::new();
    let mut corrections = Vec::new();
    let mut feedback = Vec::new();
    let mut experiments = Vec::new();
    let mut attempts: Vec<AttemptView> = Vec::new();
    let mut determined_at: Vec<Option<usize>> = Vec::new();
    let mut open = Some(Open::at(first.envelope.monotonic_timestamp));
    let mut instruction_like_texts = 0;

    for &(p, event) in &events {
        let id = event.envelope.event_id.clone();
        let ms = event.envelope.monotonic_timestamp;
        instruction_like_texts += event
            .payload
            .untrusted_texts()
            .iter()
            .filter(|t| t.instruction_like)
            .count();
        match &event.payload {
            Payload::Task(t) => {
                if let Some(g) = &t.goal {
                    constraints.extend(g.constraints.iter().copied());
                }
                if let Some(c) = t.constraint {
                    constraints.insert(c);
                }
                match t.action {
                    TaskAction::GoalSet if goal.is_none() => {
                        if let Some(g) = &t.goal {
                            goal = Some(GoalView {
                                event_id: id,
                                at_ms: ms,
                                goal: g.clone(),
                                provenance: t.provenance.clone(),
                            });
                        }
                    }
                    TaskAction::GoalChanged => goal_changes += 1,
                    TaskAction::AttemptStarted => match &mut open {
                        Some(o) if o.help == 0 && o.advice == 0 => o.started_ms = Some(ms),
                        Some(_) => {}
                        None => open = Some(Open::at(ms)),
                    },
                    TaskAction::GoalAbandoned => {
                        let o = open.take().unwrap_or(Open::at(ms));
                        attempts.push(AttemptView {
                            attempt_no: attempts.len() as u32 + 1,
                            started_at_ms: o.started_ms,
                            ended_at_ms: Some(ms),
                            result: OutcomeKind::Aborted,
                            reason: Some(OutcomeReason::GaveUp),
                            determined_by: Determination::Event {
                                event_id: id,
                                provenance: t.provenance.clone(),
                            },
                            observable_until_ms: Some(ms),
                            help_requests: o.help,
                            advice_shown: o.advice,
                            advice_display_unknown: o.advice_unknown,
                        });
                        determined_at.push(Some(p));
                    }
                    _ => {}
                }
            }
            Payload::Help(h) => {
                open.get_or_insert(Open::at(ms)).help += 1;
                help_requests.push(Stamped {
                    event_id: id,
                    at_ms: ms,
                    data: h.clone(),
                });
            }
            Payload::PlayerAction(a) => {
                open.get_or_insert(Open::at(ms));
                actions.push(Stamped {
                    event_id: id,
                    at_ms: ms,
                    data: a.clone(),
                });
            }
            Payload::Assistant(a) => {
                if let Some(o) = &mut open {
                    if a.displayed_at_ms.is_some() {
                        o.advice += 1;
                    } else {
                        o.advice_unknown += 1;
                    }
                }
                // An input must be an observation of the same session made no later than the
                // decision: when the advice was shown (or, not known, when it was recorded).
                let decision_ms = a.displayed_at_ms.unwrap_or(ms);
                let mut based_on = Vec::new();
                let mut rejected = 0;
                for reference in &a.based_on {
                    match index.get(reference.as_str()) {
                        Some(&i)
                            if same_place(&ordered[i], event)
                                && matches!(ordered[i].payload, Payload::Observation(_)) =>
                        {
                            if i < p && ordered[i].envelope.monotonic_timestamp <= decision_ms {
                                based_on.push(reference.clone());
                            } else {
                                rejected += 1;
                                quality.future_inputs_rejected += 1;
                            }
                        }
                        _ => {
                            rejected += 1;
                            quality.dangling_refs += 1;
                        }
                    }
                }
                advice.push(AdviceView {
                    event_id: id,
                    at_ms: ms,
                    kind: a.kind,
                    in_reply_to: a.in_reply_to.clone(),
                    based_on,
                    rejected_inputs: rejected,
                    sources: a.sources.clone(),
                    text: a.text.clone(),
                    displayed_at_ms: a.displayed_at_ms,
                    display_ms: a.display_ms,
                    constraints_respected: a.constraints_respected,
                    model_version: event.envelope.model_version.clone(),
                    provenance: a.provenance.clone(),
                });
            }
            Payload::Outcome(o) => {
                let current = open.take().unwrap_or(Open {
                    started_ms: None,
                    help: 0,
                    advice: 0,
                    advice_unknown: 0,
                });
                attempts.push(AttemptView {
                    attempt_no: attempts.len() as u32 + 1,
                    started_at_ms: current.started_ms,
                    ended_at_ms: Some(ms),
                    result: o.result,
                    reason: o.reason,
                    determined_by: Determination::Event {
                        event_id: id,
                        provenance: o.provenance.clone(),
                    },
                    observable_until_ms: o.observable_until_ms.or(Some(ms)),
                    help_requests: current.help,
                    advice_shown: current.advice,
                    advice_display_unknown: current.advice_unknown,
                });
                determined_at.push(Some(p));
            }
            Payload::Correction(c) => corrections.push(CorrectionView {
                event_id: id,
                at_ms: ms,
                target_event_id: c.target_event_id.clone(),
                category: c.category,
                component: c.component,
                proposed_value: c.proposed_value.clone(),
                proposed_outcome: c.proposed_outcome,
                note: c.note.clone(),
                provenance: c.provenance.clone(),
                status: match c.provenance.verification_status {
                    VerificationStatus::ReviewerVerified | VerificationStatus::Gold => {
                        ClaimStatus::Verified
                    }
                    VerificationStatus::Rejected => ClaimStatus::Rejected,
                    VerificationStatus::Unverified => ClaimStatus::Claimed,
                },
                applied: false,
            }),
            Payload::Feedback(f) => feedback.push(Stamped {
                event_id: id,
                at_ms: ms,
                data: f.clone(),
            }),
            Payload::Experiment(x) => experiments.push(Stamped {
                event_id: id,
                at_ms: ms,
                data: x.clone(),
            }),
            Payload::Session(_) | Payload::Observation(_) | Payload::CaptureQuality(_) => {}
        }
    }
    if let Some(o) = open {
        attempts.push(close_by_session(
            ordered,
            start,
            last,
            o,
            attempts.len() as u32 + 1,
        ));
        determined_at.push(None);
    }

    // The state before: the last observation of the session before the episode began.
    let state_before = ordered[..start]
        .iter()
        .rev()
        .filter(|e| same_place(e, first))
        .find_map(state_view);
    // The state after: the first observation after what determined the outcome.
    let after = determined_at.last().copied().flatten().unwrap_or(last);
    let state_after = ordered
        .iter()
        .skip(after + 1)
        .filter(|e| same_place(e, &ordered[after]))
        .find_map(state_view);

    // Time to the first success, within one session.
    let (origin_ms, origin_event) = match &goal {
        Some(g) => (
            g.at_ms,
            index.get(g.event_id.as_str()).map(|&i| &ordered[i]),
        ),
        None => (first.envelope.monotonic_timestamp, Some(first)),
    };
    let success_at = attempts
        .iter()
        .zip(&determined_at)
        .find(|(a, _)| a.result == OutcomeKind::Success)
        .and_then(|(_, p)| *p);
    let time_to_first_success_ms = success_at.and_then(|p| {
        let success = &ordered[p];
        origin_event
            .filter(|o| o.envelope.session_id == success.envelope.session_id)
            .and_then(|_| success.envelope.monotonic_timestamp.checked_sub(origin_ms))
    });

    // Assisted: advice shown before the first success (or, with none, during the episode).
    let before: Vec<&AdviceView> = advice
        .iter()
        .filter(|a| {
            index
                .get(a.event_id.as_str())
                .is_some_and(|&i| success_at.map_or(i <= last, |s| i < s))
        })
        .collect();
    let assisted = if before.iter().any(|a| a.displayed_at_ms.is_some()) {
        Some(true)
    } else if before.is_empty() {
        Some(false)
    } else {
        None
    };

    let capture_issues = ordered[start..=last]
        .iter()
        .filter(|e| same_place(e, first))
        .filter(
            |e| matches!(&e.payload, Payload::CaptureQuality(c) if c.status != CaptureStatus::Ok),
        )
        .count();

    let mut session_ids: Vec<String> = Vec::new();
    let mut receipts = BTreeSet::new();
    let mut policies = BTreeSet::new();
    for (_, event) in &events {
        if !session_ids.contains(&event.envelope.session_id) {
            session_ids.push(event.envelope.session_id.clone());
        }
        receipts.insert(event.envelope.consent_receipt_id.clone());
        policies.insert(event.envelope.rights_policy_id.clone());
    }
    let outcome = attempts
        .last()
        .cloned()
        .unwrap_or_else(|| close_by_session(ordered, start, last, Open::at(0), 1));

    Episode {
        episode_id: first.envelope.episode_id.clone().unwrap_or_default(),
        schema_version: first.envelope.schema_version.clone(),
        research_subject_id: first.envelope.research_subject_id.clone(),
        session_ids,
        game_id: first.envelope.game_id,
        game_variant: first.envelope.game_variant.clone(),
        game_build: first.envelope.game_build.clone(),
        goal,
        goal_changes,
        constraints: constraints.into_iter().collect(),
        state_before,
        help_requests,
        advice,
        actions,
        attempts,
        state_after,
        outcome,
        corrections,
        feedback,
        experiments,
        capture_issues,
        time_to_first_success_ms,
        assisted,
        instruction_like_texts,
        lineage: EpisodeLineage {
            event_ids: events
                .iter()
                .map(|(_, e)| e.envelope.event_id.clone())
                .collect(),
            consent_receipt_ids: receipts.into_iter().collect(),
            rights_policy_ids: policies.into_iter().collect(),
        },
    }
}
