# First report: research episodes — SYNTHETIC

> **SYNTHETIC.** Every row comes from `research::synthetic` (game_id `synthetic`): generated participants, not players. These numbers test the pipeline; they say nothing about any game, player or product.

Export `syn-export-001` revision 1 · purpose `external_research_training` · schema 0.1.0 · data as of 2026-10-10T12:00:00+00:00

## Population

| Participants | Sessions | Episodes | Attempts |
|---|---|---|---|
| 2 | 3 | 5 | 6 |

Episodes by outcome: 2 success, 1 failure, 0 aborted, 1 unobserved, 1 censored.

## Metrics

Definitions: `docs/data-program/METRICS_CATALOG.md` (M1.x); how each is computed here: `src/research/report.rs`. Unweighted (P1 keeps every event, probability 1).

| Metric | Value | Numerator / denominator | Participants | Note |
|---|---|---|---|---|
| M1.1 time to first success — builder cross-check (median) | 14.1 s | 2 episodes with a success (min 13.0 s, max 15.1 s) | 1 | from the goal, within one session; no success is not a time; the catalog's Kaplan–Meier estimate is not computed in P1 |
| M1.2 success, assisted | 33% | 1 / 3 | 2 | attempts with advice shown; outcome seen |
| M1.2 success, unassisted | 100% | 1 / 1 | 1 | attempts with no advice; outcome seen |
| M1.4 help requests per attempt | 83% | 5 / 6 | 2 | attempts with at least one request; all attempts |
| M1.5 unobserved outcomes | 17% | 1 / 6 | 2 | sight lost before the outcome; not a failure |
| M1.5 censored outcomes | 17% | 1 / 6 | 2 | observation ended while the attempt was open; not a failure |

Attempts with a seen outcome whose advice may or may not have been shown (neither arm): 0. Help requests in all: 5. Corrections kept as claims (none applied): 1. Game texts flagged as reading like orders (kept as untrusted text, not obeyed): 1.

## Evidence

**Insufficient evidence.** 2 participant(s) — fewer than 30. The figures describe these episodes only: no confidence intervals, no segments, no comparison is drawn from them.

## What these numbers are not

- Assisted against unassisted compares different attempts, not the effect of help: players ask when stuck.
- Advice shown is not advice heard, and heard is not followed.
- A panel of consenting companion users is not the game's population.
