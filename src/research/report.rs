//! The first report: four metrics of `METRICS_CATALOG.md`, computed on the exported episodes, each
//! with its numerator, denominator and participant count — and the plain statement that a handful
//! of participants supports no inference. Unweighted (every P1 event is kept with probability 1)
//! and without intervals (too few participants).
//!
//! - **M1.1 `time_to_first_success`** — here only the builder's cross-check: per episode, from the
//!   goal to the first success within one session. Episodes without a success are counted beside
//!   it, never as a time. (The catalog's Kaplan–Meier estimate over a subject's attempts is not
//!   computed in P1.)
//! - **M1.2 `success_by_assistance`** — per attempt: `assisted` = advice shown during the attempt.
//!   Successes over attempts whose outcome was seen (`success`, `failure`, `aborted`), per arm;
//!   attempts whose advice may or may not have been shown are their own bucket, never
//!   "unassisted". Not the effect of help: people ask when stuck.
//! - **M1.4 `help_request_rate_per_attempt`** — attempts with at least one help request, over all
//!   attempts; and requests per attempt.
//! - **M1.5 `unobserved_outcome_rate`** — attempts `unobserved` or `censored`, over all attempts,
//!   each shown apart. Neither is a failure.

use std::collections::BTreeSet;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::research::consent::Purpose;
use crate::research::contracts::OutcomeKind;
use crate::research::episode::{AttemptView, Episode};

/// Below this many participants a figure describes the sample only (no inference, no segments).
pub const MIN_PARTICIPANTS_FOR_INFERENCE: usize = 30;

/// A proportion with what it is made of. `value` is `null` when the denominator is 0.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Rate {
    pub numerator: usize,
    pub denominator: usize,
    /// Distinct participants in the denominator.
    pub participants: usize,
    pub value: Option<f64>,
}

impl Rate {
    /// Over `items` (each with its participant), the share that `hit`.
    fn of<'a, T: 'a>(
        items: impl Iterator<Item = (&'a str, &'a T)>,
        hit: impl Fn(&T) -> bool,
    ) -> Rate {
        let mut numerator = 0;
        let mut denominator = 0;
        let mut participants = BTreeSet::new();
        for (subject, item) in items {
            denominator += 1;
            participants.insert(subject);
            if hit(item) {
                numerator += 1;
            }
        }
        Rate {
            numerator,
            denominator,
            participants: participants.len(),
            value: (denominator > 0).then(|| numerator as f64 / denominator as f64),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Metrics {
    pub participants: usize,
    pub sessions: usize,
    pub episodes: usize,
    pub attempts: usize,
    /// Episodes by their outcome (the last attempt's).
    pub successes: usize,
    pub failures: usize,
    pub aborted: usize,
    pub unobserved_episodes: usize,
    pub censored_episodes: usize,
    /// M1.1, the builder's cross-check.
    pub ttfs_episodes: usize,
    pub ttfs_participants: usize,
    pub ttfs_median_ms: Option<u64>,
    pub ttfs_min_ms: Option<u64>,
    pub ttfs_max_ms: Option<u64>,
    /// M1.2, per attempt with a seen outcome.
    pub success_assisted: Rate,
    pub success_unassisted: Rate,
    /// Attempts with a seen outcome whose advice may or may not have been shown.
    pub success_assistance_unknown: usize,
    /// M1.4.
    pub help_per_attempt: Rate,
    pub help_requests: usize,
    /// M1.5, apart: `unobserved` over attempts, `censored` over attempts.
    pub unobserved_rate: Rate,
    pub censored_rate: Rate,
    pub corrections_claimed: usize,
    pub instruction_like_texts: usize,
    pub insufficient_evidence: bool,
}

fn seen(a: &AttemptView) -> bool {
    matches!(
        a.result,
        OutcomeKind::Success | OutcomeKind::Failure | OutcomeKind::Aborted
    )
}

fn median(values: &mut [u64]) -> Option<u64> {
    values.sort_unstable();
    let n = values.len();
    match n {
        0 => None,
        _ if n % 2 == 1 => Some(values[n / 2]),
        _ => Some((values[n / 2 - 1] + values[n / 2]) / 2),
    }
}

pub fn metrics(episodes: &[Episode]) -> Metrics {
    let participants: BTreeSet<&str> = episodes
        .iter()
        .map(|e| e.research_subject_id.as_str())
        .collect();
    let sessions: BTreeSet<&str> = episodes
        .iter()
        .flat_map(|e| e.session_ids.iter().map(String::as_str))
        .collect();
    let count = |kind: OutcomeKind| episodes.iter().filter(|e| e.outcome.result == kind).count();
    let mut times: Vec<u64> = episodes
        .iter()
        .filter_map(|e| e.time_to_first_success_ms)
        .collect();
    let ttfs_participants = episodes
        .iter()
        .filter(|e| e.time_to_first_success_ms.is_some())
        .map(|e| e.research_subject_id.as_str())
        .collect::<BTreeSet<_>>()
        .len();
    let attempts: Vec<(&str, &AttemptView)> = episodes
        .iter()
        .flat_map(|e| {
            e.attempts
                .iter()
                .map(move |a| (e.research_subject_id.as_str(), a))
        })
        .collect();
    let success = |a: &AttemptView| a.result == OutcomeKind::Success;
    Metrics {
        participants: participants.len(),
        sessions: sessions.len(),
        episodes: episodes.len(),
        attempts: attempts.len(),
        successes: count(OutcomeKind::Success),
        failures: count(OutcomeKind::Failure),
        aborted: count(OutcomeKind::Aborted),
        unobserved_episodes: count(OutcomeKind::Unobserved),
        censored_episodes: count(OutcomeKind::Censored),
        ttfs_episodes: times.len(),
        ttfs_participants,
        ttfs_min_ms: times.iter().min().copied(),
        ttfs_max_ms: times.iter().max().copied(),
        ttfs_median_ms: median(&mut times),
        success_assisted: Rate::of(
            attempts
                .iter()
                .copied()
                .filter(|(_, a)| seen(a) && a.advice_shown > 0),
            success,
        ),
        success_unassisted: Rate::of(
            attempts
                .iter()
                .copied()
                .filter(|(_, a)| seen(a) && a.advice_shown == 0 && a.advice_display_unknown == 0),
            success,
        ),
        success_assistance_unknown: attempts
            .iter()
            .filter(|(_, a)| seen(a) && a.advice_shown == 0 && a.advice_display_unknown > 0)
            .count(),
        help_per_attempt: Rate::of(attempts.iter().copied(), |a| a.help_requests > 0),
        help_requests: episodes.iter().map(|e| e.help_requests.len()).sum(),
        unobserved_rate: Rate::of(attempts.iter().copied(), |a| {
            a.result == OutcomeKind::Unobserved
        }),
        censored_rate: Rate::of(attempts.iter().copied(), |a| {
            a.result == OutcomeKind::Censored
        }),
        corrections_claimed: episodes.iter().map(|e| e.corrections.len()).sum(),
        instruction_like_texts: episodes.iter().map(|e| e.instruction_like_texts).sum(),
        insufficient_evidence: participants.len() < MIN_PARTICIPANTS_FOR_INFERENCE,
    }
}

/// What the report is about.
#[derive(Debug, Clone)]
pub struct ReportContext {
    pub synthetic: bool,
    pub export_id: String,
    pub revision: u32,
    pub purpose: Purpose,
    pub as_of: DateTime<Utc>,
    pub schema_version: String,
}

fn rate_cell(rate: &Rate) -> String {
    match rate.value {
        Some(v) => format!("{:.0}%", v * 100.0),
        None => "n/a (denominator 0)".into(),
    }
}

fn ms_cell(ms: Option<u64>) -> String {
    ms.map_or("n/a".into(), |ms| format!("{:.1} s", ms as f64 / 1000.0))
}

/// The report, as Markdown.
pub fn render(m: &Metrics, ctx: &ReportContext) -> String {
    let mut out = String::new();
    let label = if ctx.synthetic { " — SYNTHETIC" } else { "" };
    out.push_str(&format!("# First report: research episodes{label}\n\n"));
    if ctx.synthetic {
        out.push_str(
            "> **SYNTHETIC.** Every row comes from `research::synthetic` (game_id `synthetic`): \
             generated participants, not players. These numbers test the pipeline; they say \
             nothing about any game, player or product.\n\n",
        );
    }
    out.push_str(&format!(
        "Export `{}` revision {} · purpose `{}` · schema {} · data as of {}\n\n",
        ctx.export_id,
        ctx.revision,
        ctx.purpose.code(),
        ctx.schema_version,
        ctx.as_of.to_rfc3339()
    ));
    out.push_str(
        "## Population\n\n| Participants | Sessions | Episodes | Attempts |\n|---|---|---|---|\n",
    );
    out.push_str(&format!(
        "| {} | {} | {} | {} |\n\n",
        m.participants, m.sessions, m.episodes, m.attempts
    ));
    out.push_str(&format!(
        "Episodes by outcome: {} success, {} failure, {} aborted, {} unobserved, {} censored.\n\n",
        m.successes, m.failures, m.aborted, m.unobserved_episodes, m.censored_episodes
    ));
    out.push_str("## Metrics\n\n");
    out.push_str(
        "Definitions: `docs/data-program/METRICS_CATALOG.md` (M1.x); how each is computed here: \
         `src/research/report.rs`. Unweighted (P1 keeps every event, probability 1).\n\n",
    );
    out.push_str("| Metric | Value | Numerator / denominator | Participants | Note |\n|---|---|---|---|---|\n");
    out.push_str(&format!(
        "| M1.1 time to first success — builder cross-check (median) | {} | {} episodes with a success (min {}, max {}) | {} | from the goal, within one session; no success is not a time; the catalog's Kaplan–Meier estimate is not computed in P1 |\n",
        ms_cell(m.ttfs_median_ms),
        m.ttfs_episodes,
        ms_cell(m.ttfs_min_ms),
        ms_cell(m.ttfs_max_ms),
        m.ttfs_participants
    ));
    for (name, rate, note) in [
        (
            "M1.2 success, assisted",
            &m.success_assisted,
            "attempts with advice shown; outcome seen",
        ),
        (
            "M1.2 success, unassisted",
            &m.success_unassisted,
            "attempts with no advice; outcome seen",
        ),
        (
            "M1.4 help requests per attempt",
            &m.help_per_attempt,
            "attempts with at least one request; all attempts",
        ),
        (
            "M1.5 unobserved outcomes",
            &m.unobserved_rate,
            "sight lost before the outcome; not a failure",
        ),
        (
            "M1.5 censored outcomes",
            &m.censored_rate,
            "observation ended while the attempt was open; not a failure",
        ),
    ] {
        out.push_str(&format!(
            "| {name} | {} | {} / {} | {} | {note} |\n",
            rate_cell(rate),
            rate.numerator,
            rate.denominator,
            rate.participants
        ));
    }
    out.push_str(&format!(
        "\nAttempts with a seen outcome whose advice may or may not have been shown (neither arm): {}. \
         Help requests in all: {}. Corrections kept as claims (none applied): {}. Game texts flagged as \
         reading like orders (kept as untrusted text, not obeyed): {}.\n\n",
        m.success_assistance_unknown, m.help_requests, m.corrections_claimed, m.instruction_like_texts
    ));
    out.push_str("## Evidence\n\n");
    if m.insufficient_evidence {
        out.push_str(&format!(
            "**Insufficient evidence.** {} participant(s) — fewer than {MIN_PARTICIPANTS_FOR_INFERENCE}. \
             The figures describe these episodes only: no confidence intervals, no segments, no comparison \
             is drawn from them.\n\n",
            m.participants
        ));
    } else {
        out.push_str(
            "Participants are enough to describe, not to compare: intervals must account for several \
             attempts per participant (cluster by participant).\n\n",
        );
    }
    out.push_str("## What these numbers are not\n\n");
    out.push_str("- Assisted against unassisted compares different attempts, not the effect of help: players ask when stuck.\n");
    out.push_str("- Advice shown is not advice heard, and heard is not followed.\n");
    out.push_str("- A panel of consenting companion users is not the game's population.\n");
    out
}
