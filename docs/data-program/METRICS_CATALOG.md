# METRICS_CATALOG — what the data program measures, and how

<div dir="rtl" lang="he">

**בקצרה:** הקטלוג מפרט את 12 משפחות המדדים של סעיף 5 בהנחיה: 49 מדדים, ועוד מדד אחד של חזרה ל-Syrup שמוצג יחד עם המגבלה שלו. לכל מדד יש: השאלה העסקית, הנוסחה והמכנה, חלון הזמן, האוכלוסייה הזכאית ויחידת העצמאות הסטטיסטית. יחידת העצמאות היא המשתתף, לא הפריים ולא האירוע. עוד מפורטים מקור התווית, הטיפול בחוסר נתונים (`unobserved` ו-`censored` אינם כישלון, ו-`unknown` אינו אפס), הדיוק הנדרש, הפילוחים המותרים וההסכמה הנדרשת. לכל מדד מצורפת שאילתת DuckDB על היצוא המקומי של P1.
**אף שאילתה עוד לא רצה.** המנהל יריץ אותן על הנתונים הסינתטיים. שמות השדות הותאמו לקוד של w46 (סכמה 0.1.0) כפי שהיה בסביבות 10:35, וכל המיפוי מרוכז בקובץ prelude אחד. 18 שדות שחסרים בסכמה הזאת מופיעים כהצעות. מדד שתלוי באחד מהם מחזיר NULL, לעולם לא 0.
מדדים שדורשים gold, מפת ייחוס מורשית או טבלת מחירים מסומנים P2/P3, ואי אפשר לחשב אותם היום.
גבולות: חזרה ל-Syrup אינה retention של המשחק. זמן משחק ארוך אינו הצלחה. תסכול נרשם רק כדיווח עצמי. אין הסקה של מוצא, הכנסה או פרופיל "לווייתן".

</div>

**Status:** P0 design document (w45, 2026-10-10). No metric here has been computed on any data. Every
query is marked **not yet run**; the manager runs them on the synthetic P1 export
(`events.jsonl` / `episodes.jsonl`, written by the P1 slice described in `DATA_CONTRACTS.md`).
Proposed numbers (precision targets, minimum cell sizes, thresholds) are **proposals to be confirmed** by
the Statistics and Privacy & Security roles, not measured facts.

Related documents: `CURRENT_STATE_AUDIT.md` (what exists today), `DATA_CONTRACTS.md` (the field
names, the source of truth), `DATA_ARCHITECTURE.md` (where each field is produced), `CONSENT_AND_RIGHTS.md`
(purposes, receipts, rights manifests), `THREAT_MODEL.md` (aggregate thresholds and re-identification),
`DATA_QUALITY_AND_BIAS.md` (labels, splits, bias, statistical method), `EVALUATION_PLAN.md`,
`SELF_IMPROVEMENT_LOOP.md`, `B2B_PRODUCT_STRATEGY.md` (which product uses which metric).

## Contents

1. [How to read this catalog](#1-how-to-read-this-catalog)
2. [Rules shared by every metric](#2-rules-shared-by-every-metric)
3. [What these metrics cannot say](#3-what-these-metrics-cannot-say)
4. [Field names, reconciled with schema 0.1.0](#4-field-names-reconciled-with-schema-010)
5. [The query prelude](#5-the-query-prelude)
6. [F1 Task completion and onboarding](#f1-task-completion-and-onboarding)
7. [F2 Navigation and UI friction](#f2-navigation-and-ui-friction)
8. [F3 Learning over time](#f3-learning-over-time)
9. [F4 Help usefulness](#f4-help-usefulness)
10. [F5 Interruption and user control](#f5-interruption-and-user-control)
11. [F6 Errors and corrections](#f6-errors-and-corrections)
12. [F7 Recovery](#f7-recovery)
13. [F8 Localization](#f8-localization)
14. [F9 Vision quality](#f9-vision-quality)
15. [F10 Model quality and cost](#f10-model-quality-and-cost)
16. [F11 In-game economy friction](#f11-in-game-economy-friction)
17. [F12 Data health](#f12-data-health)
18. [The ten queries of §14, mapped](#18-the-ten-queries-of-14-mapped)
19. [What I could not verify](#19-what-i-could-not-verify)

---

## 1. How to read this catalog

Each metric has an id (`M<family>.<n>`), a snake_case name used in reports, and one table with the
eleven attributes the directive (§5) requires, followed by its DuckDB query.

**Computable when** says what data the metric needs:

| Tag | Meaning |
|---|---|
| **P1** | computable on the P1 local export (`events.jsonl`, `episodes.jsonl`) if the export carries the fields of §4 |
| **P2** | needs the P2 catalog (`migrations/0001_research_catalog.sql`), the consent ledger, a licensed reference table, a price table or the media path. The query is written against those, and on P1 it is expected to fail or return "not computable" |
| **P3** | needs a reviewed gold set (`DATA_QUALITY_AND_BIAS.md`), so it is not computable before P3 |

What is **not** in this catalog: Syrup's existing local play stats (`src/metrics.rs`,
`docs/data-and-metrics.md`). Those stay on the PC, and sharing them is not offered (a351a9f). Nothing here
reads them, and no metric may be backfilled from `log.txt`, `memory.json`, `knowledge.json`, `mic.wav` or
old recordings (directive §3; CURRENT_STATE_AUDIT §6).

## 2. Rules shared by every metric

These apply to every metric unless its table says otherwise. A metric table repeats the values that
matter for it, so it can be read on its own.

### 2.1 Population — what a figure describes

A figure describes **the consented research panel**: adults, recruited for a study, on a title whose
rights policy permits the purpose. It does **not** describe all players of the game. Companion users
choose to use a companion, and panel members opt in. Both are selection effects
(`DATA_QUALITY_AND_BIAS.md`). Every report states the panel's size and how it was recruited. A Studio
Friction Radar report (product A) says "in this panel of N participants", never "players".

### 2.2 Unit of statistical independence

The unit is the **research subject** (`research_subject_id`). Attempts, episodes, events and frames
from one person are correlated. Standard errors are therefore cluster-robust by subject (the ratio
estimator of §5's `ratio_se` macro) or come from a bootstrap that resamples subjects. A hierarchical
model is the other option, fitted in the report tool. A sample size is the **number of subjects**,
reported beside the number of episodes, sessions and observed hours. It is never a count of frames
or events (§11).

### 2.3 Missing data

- `unknown` and missing values are **NULL**, never 0. A query never uses `coalesce(x, 0)` on a measured
  value. It uses it only on counts, where "no matching row" really does mean zero.
- **`unobserved` and `censored` are not failures.** A lost window, Syrup closing or a recording that
  ended proves nothing about the player (§4). Every rate over outcomes reports three things:
  (a) the rate among **observed** outcomes (`success`, `failure`, `aborted`), with that denominator;
  (b) the **unobserved share** (`unobserved` + `censored`) over all attempts;
  (c) the **bounds** that make no assumption about the unobserved: `success / all` and
  `(success + unobserved) / all`.
- **Time-to-event** metrics use the Kaplan–Meier estimator. Censoring happens at the last moment the
  system could see the attempt (`observable_until_ms`). This assumes censoring is non-informative.
  That is checked by comparing censoring rates across segments, and stated as an assumption.
- `aborted` means the player abandoned the attempt, and the system could see that happen (an explicit
  "stop", or a goal change the player made). `aborted` is an observed outcome. It is not a failure.

### 2.4 Sampling

Rates are computed only on **rate-eligible** records: `sampling_policy` is not `event_triggered`
and `sampling_probability` is known (`rate_eligible` in §5). They are weighted by
w = `1 / sampling_probability` (a Hájek ratio). Records sampled because something happened
(`event_triggered`, such as clips around errors) are **excluded from every rate denominator**, because a
sample of errors makes errors look more common (§8). Those records serve as evidence and examples.
Per-participant caps (`DATA_ARCHITECTURE.md` §3.4) keep one participant from dominating. M12.5 measures
the concentration that remains.

### 2.5 Precision and evidence

- Each metric has a **required precision**, written as a target half-width of the 95% confidence
  interval at the level the decision is made. These targets are **proposals**, set by the decision
  each one serves. Ranking tasks by friction for a studio report tolerates ±10 percentage points. A
  regression gate in `SELF_IMPROVEMENT_LOOP.md` needs ±5 pp or a pre-registered minimal important
  difference.
- **Reporting rule (proposed):** a cell is shown only when it has **at least 20 distinct subjects** and
  its half-width is no larger than the target. Otherwise it shows `insufficient evidence` with its
  subject count (the `evidence()` macro of §5). External outputs must also pass the aggregate threshold
  and small-cell suppression of `THREAT_MODEL.md` / `CONSENT_AND_RIGHTS.md`. A threshold alone is not
  anonymity: the play stats' documented floor was 50 distinct installs (`docs/data-and-metrics.md`,
  "Aggregation before any sale").
- **Sample size for a rate, with clustering:** the number of subjects needed is
  `m ≈ 1.96² · p(1−p) · deff / (h² · k̄)`, where `k̄` is the mean number of attempts per subject and
  `deff = 1 + (k̄ − 1) · ICC`. A worked example with **hypothetical inputs**, not measurements:
  p = 0.5, h = 0.10, k̄ = 3 and ICC = 0.3 give deff = 1.6 and m ≈ 52 subjects per cell. At h = 0.05 the
  same inputs need about 205 subjects per cell. The ICC must be measured in a pilot. Until then, every
  segment count is limited by this arithmetic.
- No winner is declared after scanning many metrics. Experiments pre-register one primary outcome, the
  minimal important difference, the sample size and the analysis (`DATA_QUALITY_AND_BIAS.md`,
  `EVALUATION_PLAN.md`).

### 2.6 Segmentation

**Allowed** (when the field is known; `unknown` is its own segment, never dropped silently):
`game_id`, `game_variant`, `world_id`, `game_build`, `platform`, `client_version`, `detector_version`,
`model_version`, `task_key`, constraint set, assisted versus unassisted, experiment arm, the game's UI
language **as set or detected**, the conversation language **as chosen**, in-game level band (an
observed game state), week or month of collection.

**Forbidden in every output:** inferred origin, nationality, ethnicity, religion, gender, age (beyond the
adult gate), health or mental state, income, ability or willingness to pay, "whale" or spender tiers,
and any trait inferred from the face, voice, accent or click rhythm. Per-subject rows are never shown
in anything external. A breakdown that turns a cell into one person (a rare class on a rare map in one
week) is suppressed or merged.

### 2.7 Consent and rights

Every metric names the purposes it serves. The purpose names below follow the directive §10 and
`CONSENT_AND_RIGHTS.md`:

| Purpose id (assumed spelling) | Meaning |
|---|---|
| `service_operation` | running the service the player uses |
| `improve_syrup` | improving Syrup itself (internal) |
| `aggregate_analytics` | external aggregate insights, such as studio reports |
| `external_research_training` | episodes licensed for research, training or evaluation |
| `media_donation` | donated media |

A query runs only over rows whose `collection_purpose` covers the metric's purpose. In P1 the export
gate already filtered the rows. In P2 a report also re-checks the consent epoch and the rights policy at
query time (`DATA_ARCHITECTURE.md` §5). **Rights come before metrics.** MapleStory and MapleStory Worlds
are `requires_title_specific_review`, so no external report on them exists until that review approves
one. Until then, external metric outputs come from synthetic data, a partner studio's title with
written rights, or an environment the program owns (directive §10).

### 2.8 Reporting

Every figure carries:
- `data_as_of`: the newest `ingested_at`;
- the denominator's definition, in words;
- `n_subjects`, `n_episodes`, `n_sessions` and observed hours;
- the segment.

Output from the synthetic export is marked **SYNTHETIC** on every table and chart (§14). Causal wording
("help caused success") is reserved for randomized experiments analysed by intention-to-treat.
Everything else is association (§11).

### 2.9 Time

`monotonic_timestamp` orders events inside a session and measures durations inside a session. It is
never compared across sessions. Sessions of one subject are ordered by their coarse start time
(`started_at`, §4). Durations that span sessions are sums of within-session durations (active time),
never wall-clock differences.

## 3. What these metrics cannot say

These limits apply to every report built on this catalog. Each one is repeated where it bites.

1. **Returning to Syrup is not game retention.** The program can measure whether a participant comes
   back to Syrup (L1 below). It cannot claim churn or retention **of the game** without separate,
   authoritative game data from the publisher (directive §5). A player who stops using Syrup may still
   be playing.
2. **Long play is not success.** Minutes played, session length and observed hours are denominators and
   costs. They are never outcomes to maximise. No metric rewards more play, and no experiment optimises
   for it (directive §5, §11: no optimisation for addiction or compulsive spending).
3. **Frustration is a self-report.** It is recorded only when the player says so (a `feedback` event of
   kind `frustration` with `source_type = human_asserted`). It is never inferred from the face, voice,
   speech rate, click rhythm or deaths. Self-reports have their own non-response bias, so the response
   rate is always reported beside them.
4. **Shown is not heard, and heard is not followed.** Advice displayed or spoken is "shown". "Heard" is
   known only from an explicit acknowledgement where the interface has one. "Followed" is known only
   with an action whose `source_type` says how it was known. Each stage has its own denominator (F4).
5. **Association is not causation.** Assisted and unassisted attempts differ because people ask for
   help when they are stuck. Two similar attempts are not a counterfactual (§11, F7).
6. **Satisfaction is not correctness, and success does not prove the wording was preferred** (§6).
   Preference ratings, outcomes in the game and factual correctness are separate labels.
7. **A panel is not the population** (2.1).

### L1 `syrup_return_rate` — the one "return" metric allowed, with its limit

| | |
|---|---|
| Business question | Do panel participants come back to Syrup in the following week? This is a product-health signal for Syrup itself. **It says nothing about the game.** |
| Formula and denominator | subjects with ≥1 session in week w+1 ÷ subjects with ≥1 session in week w. A ratio estimator; each subject contributes 0 or 1 |
| Time window | ISO week pairs, over the study period. The last week is excluded because its follow-up is not complete (right-censored) |
| Eligible population | panel subjects with a session in week w, whose participation in the study had not ended by week w+1 |
| Unit of independence | research subject |
| Label source | `session` events (`direct_observation` of the client starting) |
| Missing data | a subject who withdrew or whose study ended during w+1 is excluded from that week's denominator, not counted as "not returned" |
| Required precision | ±10 pp (proposed); a descriptive metric only |
| Allowed segmentations | client_version, game_id, recruitment cohort |
| Consent | `improve_syrup` |
| Computable when | P1 (needs `started_at` on session events) |

```sql
-- L1 syrup_return_rate — NOT YET RUN
WITH weeks AS (
    SELECT DISTINCT research_subject_id, date_trunc('week', started_at) AS wk
    FROM session_order WHERE started_at IS NOT NULL
),
last_week AS (SELECT max(wk) AS wk FROM weeks)
SELECT w.wk AS week,
       count(*)                                  AS n_subjects,
       count(n.research_subject_id)              AS n_returned,
       count(n.research_subject_id) / count(*)   AS return_rate
FROM weeks w
LEFT JOIN weeks n
       ON n.research_subject_id = w.research_subject_id AND n.wk = w.wk + INTERVAL 7 DAY
WHERE w.wk < (SELECT wk FROM last_week)
GROUP BY w.wk ORDER BY w.wk;
-- Limit: says nothing about game retention or churn.
-- Not modelled here: subjects whose study ended or who withdrew in w+1 (needs the panel roster, P2).
```

## 4. Field names: reconciled with schema 0.1.0

The metric queries read **canonical views** (`ev`, `ep`, `attempts`, `v_help`, `v_assistant`, …),
never the files directly. The prelude (§5) is the only place that maps the export's physical fields to
those views.

This mapping was **reconciled on 2026-10-10 (~10:35) against w46's code as it stood**:
`src/research/contracts.rs` (`SCHEMA_VERSION = "0.1.0"`), `episode.rs` and `export.rs`. Two caveats:
that code was still in progress, and `DATA_CONTRACTS.md` had not been written yet. If either changes,
re-check §4.1 and edit the prelude only.

### 4.1 Where each canonical column comes from

| Canonical view and column | Schema 0.1.0 source |
|---|---|
| `ev` envelope columns | the envelope, exactly the directive's §7 names, flattened into each line of `events.jsonl` |
| `ev.t_ms` | `monotonic_timestamp`: milliseconds since the session's start, comparable within a session only |
| `ev.observation_coverage` | `full`, `partial`, `not_visible` or `unknown` |
| `ev.rate_eligible`, `ev.w` | `sampling_policy` ≠ `event_triggered` and `sampling_probability` known; w = 1 / `sampling_probability` |
| family fields | `payload` is serde's externally tagged enum: `payload.<family>.<field>`, for example `payload.session.action` |
| `v_session.phase`, `reason` | `payload.session.action` (`start`, `end`, `pause`, `resume`) and `.reason` |
| `session_order.started_at` | `min(ingested_at)` of the session. w46's builder also orders sessions by first ingestion |
| `session_order.conversation_language` | the envelope `locale` (Syrup's interface language; the closest field) |
| `v_task.phase`, `goal_kind`, `goal_source` | `payload.task.action` (`goal_set`, `goal_changed`, `constraint_added`, `attempt_started`, `goal_abandoned`), `.goal.kind`, `.goal.origin` (`explicit` or `inferred`) |
| `v_help.help_id`, `intent`, `channel` | `event_id`; `payload.help.kind` (`question`, `hint_request`, `stuck`, `clarification`); `.channel` |
| `v_assistant.advice_id`, `phase` | `event_id`; `shown` when `payload.assistant.displayed_at_ms` is not null, else `produced` |
| `v_assistant.in_reply_to`, `kind`, `shown_at_ms`, `spoken_ms`, `length_chars`, `constraints_respected` | `.in_reply_to` (the help event's id); `.kind` (`answer`, `hint`, `warning`, `clarifying_question`, `refusal`); `.displayed_at_ms`; `.display_ms`; the length of `.text.text`; `.constraints_respected` |
| `v_feedback.kind`, `target_advice_id` | `payload.feedback.kind` (`helped` becomes `helpful`; `not_helpful`, `dont_interrupt`, `wrong`, `goal_reached`); `.about_event_id` |
| `v_action.action_kind`, `linked_advice_id`, `measurement` | `payload.player_action.kind`, `.follows_advice`, `.measurement` (`self_report`, `inferred_from_state`, `publisher_sdk`, `licensed_replay`) |
| `v_correction.verification_status` | `provenance.verification_status`: `unverified` → `claimed`; `reviewer_verified` and `gold` → `verified`; `rejected` → `rejected` |
| `v_correction.target_kind` | the event type of `target_event_id` (`assistant` becomes `advice`) |
| `v_outcome.status`, `reason`, `determined_by` | `payload.outcome.result`, `.reason`, `.provenance.source_type` |
| `v_observation` (one row per component) | `payload.observation.components[]`: `name` → `component`; `status` (`observed`, `unknown`, `not_visible`); `value` (`number`, `category`, `flag` or `text.untrusted_text`) → `value`, `value_text`, `value_number`; `provenance` (`confidence` only with a `calibration_ref`; `producer_version` → `process_version`) |
| `v_capture` | `payload.capture_quality.status` (`ok`, `degraded`, `focus_lost`, `not_in_view`, `login_screen`, `detection_failed`, `frames_dropped`, `clock_drift`), `.dropped_frames`, `.clock_offset_ms` |
| `ep` | `episodes.jsonl` (the builder's `Episode`): `goal.goal.{kind, origin, target}`, `constraints`, `attempts[]`, `outcome`, `assisted`, `time_to_first_success_ms`, `session_ids`, `lineage.{consent_receipt_ids, rights_policy_ids}`. Sampling, purpose and client fields are taken from the episode's events |
| `ep.task_key` | `goal.kind` + `:` + the lower-cased `goal.target` text. **Not a closed vocabulary yet** (§4.2) |
| `attempts` | `ep.attempts[]` (the builder's `AttemptView`): `attempt_no`, `result`, `reason`, `determined_by` (an event's `source_type`, or `rule`), `observable_until_ms`, `started_at_ms`, `ended_at_ms`, `help_requests`, `advice_shown`. The builder already decides `unobserved` and `censored` |
| `observed_time` | from the session's own `start` / `pause` / `resume` / `end` events: in-view time is the time after a `start` or `resume` until the next session event |

Attempts and episodes come from the **builder**. Its rules are authoritative: no outcome event becomes
`unobserved` or `censored`, never `failure`. An event belongs to an attempt when its `episode_id`
matches and its `t_ms` lies between the attempt's start and end. An episode that spans two sessions
(`session_ids` has more than one entry) has attempts whose times cannot be compared, so the
time-window joins treat it as unknown.

### 4.2 What schema 0.1.0 does not record yet: capability flags

A metric that needs one of these returns **NULL** ("not measurable") through the prelude's
`capabilities` table, never 0. Each row is a proposal for `DATA_CONTRACTS.md`, as a minor schema change.
When the field lands, set the row's `available` flag to TRUE.

| Capability flag | Proposed addition | Metrics that need it |
|---|---|---|
| `heard_ack` | `FeedbackKind::HeardAck`, only where the interface offers an acknowledgement | M4.1 |
| `satisfaction` | `FeedbackKind::Satisfaction` with a 1–5 value, for the optional end-of-episode check-in | M4.1, M4.3 |
| `mute` | `FeedbackKind::Mute` (one line, or everything) | M5.2 (`dont_interrupt` exists) |
| `preference` | `FeedbackKind::Preference` (`shorter`, `longer`, `hint_only`, `no_spoilers`) | M5.4 |
| `constraint_flag` | `FeedbackKind::ConstraintViolation`: the player says "that was a spoiler" | M5.6 (the assistant's `constraints_respected` exists, with its own provenance) |
| `strategy_change` | `FeedbackKind::StrategyChange` (a self-report) | M7.2 |
| `missing_resource` | `OutcomeReason::MissingResource` with the resource kind | M11.1 |
| `resource_sufficiency` | a resource component with the amount held and the amount required | M11.2 |
| `advice_cancel` | the player cutting a line off (`interrupted_by_player` on the advice) | M5.3 |
| `template_id` | the template of templated lines (warnings, coach lines) | M5.5 |
| `assertiveness` | `hedged` or `assertive`, as delivered | M10.2 |
| `token_counts` | tokens in and out, audio ms, provider | M10.4 |
| `base_samples` | a closed `sampling_policy` vocabulary (`census`, `base_random`, `event_triggered`) and periodic base observations with a known probability (`DATA_ARCHITECTURE.md` §3.2) | M8.2, M9.3, M9.4, M9.7 |
| `game_ui_language` | the game's UI language, as set or detected | M8.1, M8.2 |
| `assistant_available` | on the session start | M1.4, M3.1, M8.1 (unknown is treated as "maybe available") |
| `comprehension_intent` | `HelpKind::Comprehension` ("I don't understand this text") | M8.1 |
| `component_latency` | per-component processing time on base samples | M9.7 |
| `task_vocabulary` | a closed, per-game task key from a licensed vocabulary, instead of kind + untrusted target text | F1–F3, for comparisons across participants |

Two more checks belong to the export, not to a flag:
- **Shared pseudonym key.** Every reference field must be pseudonymized with the same recipient key as
  `event_id`, or the joins in F4, F6 and F10 break. In 0.1.0 this holds: `export.rs` `id_kind` gives
  `in_reply_to`, `based_on`, `follows_advice`, `target_event_id`, `about_event_id` and
  `evidence_event_id` the same kind as `event_id`. A new reference field must be added there too.
- **`ingested_at` precision.** The session order uses `ingested_at`, which 0.1.0 writes to the
  millisecond on every event. Whether an export should coarsen it, for example to the hour, is a
  linkage question for `THREAT_MODEL.md`.

## 5. The query prelude

Run this once per DuckDB session, from the folder that holds `export/`. Every metric query below reads
only these views and macros. **Not yet run:** DuckDB is not installed in the environment this was
written in, so not even the syntax has been checked.

```sql
-- prelude.sql — METRICS_CATALOG §5 — NOT YET RUN
-- Maps the P1 export (schema 0.1.0, src/research/contracts.rs) to the catalog's canonical columns.

-- What 0.1.0 records (§4.2). A metric that needs a FALSE capability returns NULL, never 0.
CREATE OR REPLACE TABLE capabilities AS
SELECT * FROM (VALUES
    ('heard_ack', FALSE), ('satisfaction', FALSE), ('mute', FALSE), ('preference', FALSE),
    ('constraint_flag', FALSE), ('strategy_change', FALSE), ('missing_resource', FALSE),
    ('resource_sufficiency', FALSE), ('advice_cancel', FALSE), ('template_id', FALSE),
    ('assertiveness', FALSE), ('token_counts', FALSE), ('base_samples', FALSE),
    ('game_ui_language', FALSE), ('assistant_available', FALSE), ('comprehension_intent', FALSE),
    ('component_latency', FALSE), ('task_vocabulary', FALSE)
) AS t(name, available);
CREATE OR REPLACE MACRO has(cap) AS (SELECT available FROM capabilities WHERE name = cap);

-- Cluster-robust standard error of a ratio estimator. Input: one row per subject, y = (weighted)
-- numerator, n = (weighted) denominator. Var(R) = m/(m-1) · Σ(y_i − R·n_i)² / (Σn)², expanded into
-- plain aggregates: Σy² − 2R·Σyn + R²·Σn².
CREATE OR REPLACE MACRO ratio_se(y, n) AS
    sqrt(count(*) / (count(*) - 1.0)
         * (sum(y * y) - 2 * (sum(y) / sum(n)) * sum(y * n) + power(sum(y) / sum(n), 2) * sum(n * n)))
    / sum(n);

-- The reporting rule of §2.5 (20 subjects is a proposal).
CREATE OR REPLACE MACRO evidence(n_subjects, half_width, target) AS
    CASE WHEN n_subjects < 20 OR half_width IS NULL OR half_width > target
         THEN 'insufficient evidence' ELSE 'ok' END;

-- Events as exported, before deduplication (M12.4 measures duplicates on this).
CREATE OR REPLACE VIEW ev_raw AS
SELECT
    j->>'event_id'                              AS event_id,
    j->>'schema_version'                        AS schema_version,
    j->>'session_id'                            AS session_id,
    j->>'episode_id'                            AS episode_id,
    j->>'research_subject_id'                   AS research_subject_id,
    j->>'event_type'                            AS event_type,
    j->>'game_id'                               AS game_id,
    j->>'game_variant'                          AS game_variant,
    j->>'world_id'                              AS world_id,
    j->>'game_build'                            AS game_build,
    j->>'platform'                              AS platform,
    j->>'locale'                                AS locale,
    j->>'client_version'                        AS client_version,
    j->>'detector_version'                      AS detector_version,
    j->>'model_version'                         AS model_version,
    CAST(j->>'sequence_no' AS BIGINT)           AS sequence_no,
    CAST(j->>'monotonic_timestamp' AS BIGINT)   AS t_ms,
    CAST(j->>'ingested_at' AS TIMESTAMPTZ)      AS ingested_at,
    j->>'observation_coverage'                  AS observation_coverage,
    j->>'consent_receipt_id'                    AS consent_receipt_id,
    CAST(j->>'consent_epoch' AS INTEGER)        AS consent_epoch,
    j->>'collection_purpose'                    AS collection_purpose,
    j->>'rights_policy_id'                      AS rights_policy_id,
    j->>'sampling_policy'                       AS sampling_policy,
    CAST(j->>'sampling_probability' AS DOUBLE)  AS sampling_probability,
    j->'payload'                                AS payload
FROM read_ndjson_objects('export/events.jsonl') AS t(j);

-- One row per event_id (a retried or replayed event counts once), with its rate weight.
CREATE OR REPLACE VIEW ev AS
SELECT *,
       sampling_policy IS DISTINCT FROM 'event_triggered'
         AND sampling_probability IS NOT NULL          AS rate_eligible,
       w                       AS w
FROM ev_raw
QUALIFY row_number() OVER (PARTITION BY event_id ORDER BY ingested_at NULLS LAST, sequence_no) = 1;

-- The event families (payload.<family>.<field>).
CREATE OR REPLACE VIEW v_session AS
SELECT ev.*, payload->'session'->>'action' AS phase, payload->'session'->>'reason' AS reason
FROM ev WHERE event_type = 'session';

CREATE OR REPLACE VIEW v_task AS
SELECT ev.*, payload->'task'->>'action' AS phase,
       payload->'task'->'goal'->>'kind'   AS goal_kind,
       payload->'task'->'goal'->>'origin' AS goal_source,
       payload->'task'->>'constraint'     AS constraint_added
FROM ev WHERE event_type = 'task';

CREATE OR REPLACE VIEW v_help AS
SELECT ev.*, event_id AS help_id, 'requested' AS phase,
       payload->'help'->>'kind'                      AS intent,
       payload->'help'->>'channel'                   AS channel,
       payload->'help'->'provenance'->>'source_type' AS source_type
FROM ev WHERE event_type = 'help';

CREATE OR REPLACE VIEW v_assistant AS
SELECT ev.*, event_id AS advice_id,
       CASE WHEN payload->'assistant'->>'displayed_at_ms' IS NOT NULL
            THEN 'shown' ELSE 'produced' END                                AS phase,
       payload->'assistant'->>'in_reply_to'                                 AS in_reply_to,
       payload->'assistant'->>'kind'                                        AS kind,
       CAST(payload->'assistant'->>'displayed_at_ms' AS BIGINT)             AS shown_at_ms,
       CAST(payload->'assistant'->>'display_ms' AS BIGINT)                  AS spoken_ms,
       length(payload->'assistant'->'text'->>'text')                        AS length_chars,
       CAST(payload->'assistant'->>'constraints_respected' AS BOOLEAN)      AS constraints_respected,
       payload->'assistant'->'provenance'->>'source_type'                   AS source_type,
       payload->'assistant'->'sources'                                      AS sources,
       payload->'assistant'->'based_on'                                     AS based_on,
       CAST(NULL AS VARCHAR) AS template_id, CAST(NULL AS VARCHAR) AS assertiveness,   -- §4.2
       CAST(NULL AS BIGINT)  AS tokens_in,   CAST(NULL AS BIGINT)  AS tokens_out,
       CAST(NULL AS BIGINT)  AS audio_ms,    CAST(NULL AS VARCHAR) AS provider
FROM ev WHERE event_type = 'assistant';

CREATE OR REPLACE VIEW v_feedback AS
SELECT ev.*,
       CASE payload->'feedback'->>'kind' WHEN 'helped' THEN 'helpful'
            ELSE payload->'feedback'->>'kind' END            AS kind,
       payload->'feedback'->>'about_event_id'                AS target_advice_id,
       CAST(NULL AS VARCHAR)                                 AS value,          -- §4.2
       payload->'feedback'->'provenance'->>'source_type'     AS source_type
FROM ev WHERE event_type = 'feedback';

CREATE OR REPLACE VIEW v_action AS
SELECT ev.*, payload->'player_action'->>'kind'           AS action_kind,
       payload->'player_action'->>'follows_advice'        AS linked_advice_id,
       payload->'player_action'->>'measurement'           AS measurement,
       payload->'player_action'->'provenance'->>'source_type'         AS source_type,
       payload->'player_action'->'provenance'->>'verification_status' AS verification_status
FROM ev WHERE event_type = 'player_action';

CREATE OR REPLACE VIEW v_correction AS
SELECT c.*, c.event_id AS correction_id,
       c.payload->'correction'->>'target_event_id'                     AS target_event_id,
       CASE t.event_type WHEN 'assistant' THEN 'advice'
            ELSE coalesce(t.event_type, 'unknown') END                 AS target_kind,
       c.payload->'correction'->>'category'                            AS category,
       CASE c.payload->'correction'->'provenance'->>'verification_status'
            WHEN 'unverified' THEN 'claimed' WHEN 'reviewer_verified' THEN 'verified'
            WHEN 'gold' THEN 'verified' WHEN 'rejected' THEN 'rejected' END AS verification_status,
       c.payload->'correction'->'provenance'->>'source_type'           AS source_type,
       c.payload->'correction'->'provenance'->>'annotator_type'        AS annotator_type
FROM ev c LEFT JOIN ev t ON t.event_id = c.payload->'correction'->>'target_event_id'
WHERE c.event_type = 'correction';

CREATE OR REPLACE VIEW v_outcome AS
SELECT ev.*, payload->'outcome'->>'result' AS status, payload->'outcome'->>'reason' AS reason,
       CAST(payload->'outcome'->>'observable_until_ms' AS BIGINT) AS observable_until_ms,
       payload->'outcome'->'provenance'->>'source_type'           AS determined_by
FROM ev WHERE event_type = 'outcome';

-- One row per observed component.
CREATE OR REPLACE VIEW v_observation AS
WITH o AS (
    SELECT ev.*, unnest(CAST(payload->'observation'->'components' AS JSON[])) AS c
    FROM ev WHERE event_type = 'observation'
)
SELECT o.* EXCLUDE (c),
       c->>'name'                                          AS component,
       c->>'status'                                        AS status,       -- observed | unknown | not_visible
       c->>'status' = 'unknown'                            AS is_unknown,
       CASE WHEN json_type(c->'value') = 'NULL' THEN NULL ELSE c->'value' END AS value,
       coalesce(c->'value'->>'number', c->'value'->>'category', c->'value'->>'flag',
                c->'value'->'text'->>'untrusted_text')     AS value_text,
       CAST(c->'value'->>'number' AS DOUBLE)               AS value_number,
       c->'provenance'->>'source_type'                     AS source_type,
       CAST(c->'provenance'->'confidence'->>'value' AS DOUBLE) AS confidence,  -- only with a calibration
       CAST(NULL AS DOUBLE)                                AS latency_ms,      -- §4.2
       c->'provenance'->>'evidence_ref'                    AS evidence_ref,
       c->'provenance'->>'producer_version'                AS process_version
FROM o;

CREATE OR REPLACE VIEW v_capture AS
SELECT ev.*, payload->'capture_quality'->>'status'                         AS capture_status,
       CAST(payload->'capture_quality'->>'dropped_frames' AS BIGINT)       AS frames_dropped,
       CAST(payload->'capture_quality'->>'clock_offset_ms' AS BIGINT)      AS clock_offset_ms
FROM ev WHERE event_type = 'capture_quality';

CREATE OR REPLACE VIEW v_experiment AS
SELECT ev.*, payload->'experiment'->>'experiment_id' AS experiment_id, payload->'experiment'->>'arm' AS arm
FROM ev WHERE event_type = 'experiment';

-- Episodes, as the builder made them; sampling and client fields from the episode's events.
CREATE OR REPLACE VIEW ep AS
WITH e AS (
    SELECT
        j->>'episode_id'                                         AS episode_id,
        j->>'schema_version'                                     AS schema_version,
        j->>'research_subject_id'                                AS research_subject_id,
        CAST(j->'session_ids' AS VARCHAR[])                      AS session_ids,
        j->>'game_id' AS game_id, j->>'game_variant' AS game_variant, j->>'game_build' AS game_build,
        j->'goal'->'goal'->>'kind'                               AS goal_kind,
        j->'goal'->'goal'->>'origin'                             AS goal_source,
        j->'goal'->'goal'->'target'->>'untrusted_text'           AS goal_target,
        CAST(j->'constraints' AS VARCHAR[])                      AS constraints,
        CAST(j->'attempts' AS JSON[])                            AS attempts_json,
        j->'outcome'->>'result'                                  AS outcome,
        coalesce(j->'outcome'->'determined_by'->'event'->'provenance'->>'source_type', 'rule')
                                                                 AS outcome_determined_by,
        CAST(j->'outcome'->>'observable_until_ms' AS BIGINT)     AS observable_until_ms,
        CAST(j->'attempts'->0->>'started_at_ms' AS BIGINT)       AS start_ms,
        CAST(j->'outcome'->>'ended_at_ms' AS BIGINT)             AS end_ms,
        CAST(j->>'assisted' AS BOOLEAN)                          AS assisted,
        CAST(j->>'time_to_first_success_ms' AS BIGINT)           AS builder_ttfs_ms,
        CAST(j->'lineage'->'rights_policy_ids' AS VARCHAR[])     AS rights_policy_ids,
        CAST(j->'lineage'->'consent_receipt_ids' AS VARCHAR[])   AS consent_receipt_ids
    FROM read_ndjson_objects('export/episodes.jsonl') AS t(j)
),
s AS (
    SELECT episode_id, bool_and(rate_eligible) AS rate_eligible, max(w) AS w,
           any_value(collection_purpose) AS collection_purpose, any_value(locale) AS locale,
           any_value(platform) AS platform, any_value(world_id) AS world_id,
           any_value(client_version) AS client_version, any_value(detector_version) AS detector_version,
           any_value(model_version) AS model_version
    FROM ev WHERE episode_id IS NOT NULL GROUP BY episode_id
)
SELECT e.*,
       e.session_ids[1]                                          AS session_id,
       e.goal_kind || coalesce(':' || lower(e.goal_target), '') AS task_key,     -- §4.2 task_vocabulary
       e.end_ms - e.start_ms                                     AS duration_ms,
       e.rights_policy_ids[1]                                    AS rights_policy_id,
       s.* EXCLUDE (episode_id)
FROM e LEFT JOIN s USING (episode_id);

-- Attempts, as the builder determined them (it already turns a missing outcome into
-- unobserved/censored, never failure).
CREATE OR REPLACE VIEW attempts AS
WITH x AS (SELECT ep.*, unnest(ep.attempts_json) AS a FROM ep)
SELECT x.* EXCLUDE (a, attempts_json, outcome, start_ms, end_ms, duration_ms, observable_until_ms),
       CAST(a->>'attempt_no' AS INTEGER)                                         AS attempt_no,
       a->>'result'                                                              AS status,
       a->>'reason'                                                              AS reason,
       CASE WHEN a->>'result' = 'failure' THEN a->>'reason' END                  AS failure_type,
       CASE WHEN a->>'result' IN ('unobserved', 'censored') THEN a->>'reason' END AS unobserved_reason,
       CAST(NULL AS VARCHAR)                                                     AS missing_resource,
       coalesce(a->'determined_by'->'event'->'provenance'->>'source_type', 'rule') AS determined_by,
       CAST(a->>'observable_until_ms' AS BIGINT)                                 AS observable_until_ms,
       CAST(a->>'started_at_ms' AS BIGINT)                                       AS start_ms,
       CAST(a->>'ended_at_ms' AS BIGINT)                                         AS end_ms,
       CAST(a->>'ended_at_ms' AS BIGINT) - CAST(a->>'started_at_ms' AS BIGINT)   AS duration_ms,
       CAST(a->>'help_requests' AS INTEGER)                                      AS help_requests,
       CAST(a->>'advice_shown' AS INTEGER)                                       AS advice_shown,
       len(x.session_ids) > 1                                                    AS spans_sessions
FROM x;

CREATE OR REPLACE VIEW attempt_exposure AS
SELECT episode_id, attempt_no, help_requests, advice_shown FROM attempts;

-- A subject's sessions in order (by first ingestion, as the builder orders them).
CREATE OR REPLACE VIEW session_order AS
SELECT research_subject_id, session_id,
       min(ingested_at)                AS started_at,
       CAST(NULL AS BOOLEAN)           AS assistant_available,      -- §4.2
       any_value(locale)               AS conversation_language,
       CAST(NULL AS VARCHAR)           AS game_ui_language,         -- §4.2
       row_number() OVER (PARTITION BY research_subject_id
                          ORDER BY min(ingested_at), session_id) AS session_no
FROM ev GROUP BY research_subject_id, session_id;

-- In-view time per session: from each start/resume to the next session event (pause, end, …).
-- A session with no end (a crash) loses its last interval: observed_ms is a lower bound.
CREATE OR REPLACE VIEW observed_time AS
WITH s AS (
    SELECT research_subject_id, session_id, phase, t_ms,
           lead(t_ms) OVER (PARTITION BY session_id ORDER BY sequence_no) AS next_ms
    FROM v_session
)
SELECT research_subject_id, session_id,
       sum(next_ms - t_ms) FILTER (WHERE phase IN ('start', 'resume') AND next_ms IS NOT NULL) AS observed_ms
FROM s GROUP BY ALL;
```

The P2/P3 inputs (A19 below) have their own prelude. A query that needs one of them says so. On the P1
export it is expected to fail with "file not found" or return nothing, which is the honest result: the
input does not exist yet.

**A19, the P2/P3 inputs.** None of these is in the P1 export:
- `labels.jsonl`: automatic, user-correction and reviewed-gold labels in one file, told apart by
  `label_source` and never merged, with `split` set per subject.
- `eval/matches.jsonl` and `eval/tracks.jsonl`: written by the evaluation tool of `EVALUATION_PLAN.md`.
- `reference/ref_paths.csv`: a licensed, version-matched map graph.
- `reference/prices.csv`: a dated price table with its source. A hypothesis input, never a measured cost.
- `reference/costs.csv`: the finance input.
- The P2 catalog in Postgres (`migrations/0001_research_catalog.sql`), read from DuckDB with
  `ATTACH … (TYPE postgres, READ_ONLY)`.

```sql
-- prelude_p2p3.sql — NOT YET RUN. P2/P3 inputs only.
CREATE OR REPLACE VIEW labels AS
SELECT j->>'label_id' AS label_id, j->>'target_event_id' AS target_event_id,
       j->>'label_source' AS label_source,      -- automatic | user_correction | reviewed_gold
       j->'value' AS value, j->>'value' AS value_text,
       j->>'annotator_type' AS annotator_type, j->>'verification_status' AS verification_status,
       j->>'process_version' AS process_version,
       j->>'split' AS split                     -- train | validation | test, assigned per subject
FROM read_ndjson_objects('export/labels.jsonl') AS t(j);

CREATE OR REPLACE VIEW matches AS               -- one row per (frame, class) from the evaluation tool
SELECT j->>'research_subject_id' AS research_subject_id, j->>'component' AS component,
       j->>'detector_version' AS detector_version, CAST(j->>'tp' AS INTEGER) AS tp,
       CAST(j->>'fp' AS INTEGER) AS fp, CAST(j->>'fn' AS INTEGER) AS fn
FROM read_ndjson_objects('eval/matches.jsonl') AS t(j);

CREATE OR REPLACE VIEW track_eval AS            -- one row per gold track from the evaluation tool
SELECT j->>'research_subject_id' AS research_subject_id, j->>'component' AS component,
       CAST(j->>'gold_seconds' AS DOUBLE) AS gold_seconds, CAST(j->>'id_switches' AS INTEGER) AS id_switches,
       CAST(j->>'mean_center_error' AS DOUBLE) AS mean_center_error   -- in normalised frame units
FROM read_ndjson_objects('eval/tracks.jsonl') AS t(j);

-- The P2 catalog, read-only (connection string from the environment, never written into a query file):
-- INSTALL postgres; LOAD postgres;
-- ATTACH '' AS cat (TYPE postgres, READ_ONLY);   -- uses PGHOST/PGDATABASE/… of a read-only role
```

---

## F1 Task completion and onboarding

Serves product A (where do new players get stuck), the self-improvement loop (does help shorten a task),
and the four metrics the P1 report computes (M1.1, M1.2, M1.4 and M1.5; see `DATA_CONTRACTS.md`). An
**attempt** is one of the builder's attempts inside an episode (§4.1). A "beginner" at a task is a subject whose **first** attempt at that
`task_key` falls in the window. Experience before the panel is unknown unless the Goal Check-in asked
("first time?" is `human_asserted`).

### M1.1 `time_to_first_success`

| | |
|---|---|
| Business question | How much **active** play does a participant spend on a task before their first success? Which onboarding tasks are slow? |
| Formula and denominator | Per (subject, `task_key`): T = the sum of attempt durations, in session order, up to and including the first `success`. A subject with no success is censored at the total observed attempt time. Estimand: the Kaplan–Meier median and 75th percentile of T. Denominator: the subjects who attempted the task |
| Time window | Per `game_build` (a patch changes the task), over the stated study period |
| Eligible population | Subjects whose first attempt at the task is in the window. `task_key` is known. Sampling is rate-eligible |
| Unit of independence | Subject: one T per subject and task |
| Label source | The outcome `status` and how it was determined. The primary analysis excludes `model_inferred` successes. A sensitivity run includes them, and the two are never mixed silently |
| Missing data | No success while observed → censored, not failed. Durations or session order unknown → the subject is excluded and **counted** (`n_excluded_time_unknown`). A median the curve never reaches is reported as "not reached" |
| Required precision | 95% bootstrap CI (subjects resampled) no wider than ±25% of the median (proposed). At least 20 subjects per task |
| Allowed segmentations | `task_key`, `game_build`, `game_variant`, `client_version`, level band at start, game UI language, ever-assisted (descriptive only) |
| Consent | `improve_syrup`. For studio reports, `aggregate_analytics` plus a rights policy that permits it |
| Computable when | P1. Sessions are ordered by first ingestion (§4.1). The builder's own `time_to_first_success_ms` (within one session) is a cross-check |

```sql
-- M1.1 time_to_first_success — NOT YET RUN
WITH a AS (
    SELECT a.research_subject_id, a.task_key, a.status, a.determined_by, a.duration_ms,
           s.session_no, a.end_ms
    FROM attempts a
    JOIN session_order s ON s.session_id = a.session_id
    WHERE a.task_key IS NOT NULL AND a.rate_eligible
),
ordered AS (
    SELECT *, sum(duration_ms) OVER (PARTITION BY research_subject_id, task_key
                                     ORDER BY session_no, end_ms) AS cum_ms
    FROM a
),
per_subject AS (
    SELECT research_subject_id, task_key,
           min(cum_ms) FILTER (WHERE status = 'success'
                                 AND determined_by IS DISTINCT FROM 'model_inferred') AS t_success_ms,
           max(cum_ms)                                                                AS t_seen_ms,
           bool_or(duration_ms IS NULL OR session_no IS NULL)                         AS time_unknown
    FROM ordered GROUP BY ALL
),
km_in AS (
    SELECT task_key, coalesce(t_success_ms, t_seen_ms) / 60000.0 AS t_min,
           CAST(t_success_ms IS NOT NULL AS INTEGER) AS success
    FROM per_subject WHERE NOT time_unknown
),
km AS (SELECT task_key, t_min, sum(success) AS d, count(*) AS leaving FROM km_in GROUP BY ALL),
surv AS (   -- S(t) = P(no success yet after t active minutes)
    SELECT *, product(1 - d / at_risk) OVER (PARTITION BY task_key ORDER BY t_min) AS s_t
    FROM (SELECT *, sum(leaving) OVER (PARTITION BY task_key ORDER BY t_min DESC) AS at_risk FROM km)
)
SELECT task_key,
       sum(leaving)                              AS n_subjects,
       sum(d)                                    AS n_succeeded,
       sum(leaving) - sum(d)                     AS n_censored,
       min(t_min) FILTER (WHERE s_t <= 0.5)      AS median_active_minutes,   -- NULL = not reached
       min(t_min) FILTER (WHERE s_t <= 0.25)     AS p75_active_minutes,
       (SELECT count(*) FROM per_subject p
         WHERE p.task_key = surv.task_key AND p.time_unknown) AS n_excluded_time_unknown
FROM surv GROUP BY task_key ORDER BY task_key;
```

### M1.2 `success_by_assistance`

| | |
|---|---|
| Business question | How often do attempts succeed with the assistant's advice shown, and how often without? Where is help needed, and is it associated with success? |
| Formula and denominator | `assisted` = at least one advice `shown` during the attempt. Per arm: weighted successes ÷ weighted attempts with an **observed** outcome. Also per arm: the unobserved share over all attempts, and the bounds of §2.3 |
| Time window | Per `game_build`, over the study period |
| Eligible population | Attempts with rate-eligible sampling. The assistant was available (sessions where it was not are reported separately) |
| Unit of independence | Subject (the cluster-robust ratio) |
| Label source | Outcome as in M1.1. `assisted` comes from Syrup's own `assistant` events (what it showed is directly known) |
| Missing data | `unobserved` and `censored` stay out of the observed rate and appear in the unobserved share and the bounds |
| Required precision | ±10 pp per (`task_key` × arm) for friction reports; ±5 pp overall (proposed) |
| Allowed segmentations | `task_key`, `game_build`, `model_version`, constraint set, help intent, experiment arm |
| Consent | `improve_syrup`; `aggregate_analytics` with rights |
| Computable when | P1 |

**Association only:** people get help when they are stuck. A causal claim needs a randomized comparison
between reasonable help variants (`EVALUATION_PLAN.md`, §11), never withholding help someone asked for.

```sql
-- M1.2 success_by_assistance — NOT YET RUN
WITH a AS (
    SELECT a.research_subject_id, a.task_key, a.status, a.determined_by,
           x.advice_shown > 0 AS assisted, a.w AS w
    FROM attempts a
    JOIN attempt_exposure x ON x.episode_id = a.episode_id AND x.attempt_no = a.attempt_no
    WHERE a.rate_eligible
),
per_subject AS (
    SELECT task_key, assisted, research_subject_id,
           coalesce(sum(w) FILTER (WHERE status = 'success'
                                     AND determined_by IS DISTINCT FROM 'model_inferred'), 0) AS y,
           coalesce(sum(w) FILTER (WHERE status IN ('success', 'failure', 'aborted')), 0)    AS n_obs,
           coalesce(sum(w) FILTER (WHERE status IN ('unobserved', 'censored')), 0)           AS n_unobs,
           sum(w)                                                                            AS n_all
    FROM a GROUP BY ALL
)
SELECT task_key, assisted,
       count(*)                                  AS n_subjects,
       sum(n_all)                                AS attempts_weighted,
       sum(y) / nullif(sum(n_obs), 0)            AS success_rate_observed,
       ratio_se(y, n_obs)                        AS se_cluster,
       sum(n_unobs) / sum(n_all)                 AS unobserved_share,
       sum(y) / sum(n_all)                       AS bound_low,
       (sum(y) + sum(n_unobs)) / sum(n_all)      AS bound_high,
       evidence(count(*), 1.96 * ratio_se(y, n_obs), 0.10) AS evidence
FROM per_subject GROUP BY ALL ORDER BY task_key, assisted;
```

### M1.3 `attempts_to_first_success`

| | |
|---|---|
| Business question | How many tries does a task take? Which tasks are rarely done on the first try? |
| Formula and denominator | Per (subject, task): k = the index of the first successful attempt, in session order. Outputs: the first-try success share (k = 1) over subjects who attempted; the median k **among subjects who succeeded** (a conditional statistic, labelled as such); the count still without success (censored). A Kaplan–Meier median over k, computed as in M1.1, is the unconditional figure |
| Time window | Per `game_build` |
| Eligible population | As M1.1 |
| Unit of independence | Subject |
| Label source | As M1.1 |
| Missing data | An attempt with an unobserved outcome still counts as a try. It cannot be the success. Subjects with unknown session order are excluded and counted |
| Required precision | First-try share ±10 pp (proposed) |
| Allowed segmentations | As M1.1 |
| Consent | As M1.1 |
| Computable when | P1 |

```sql
-- M1.3 attempts_to_first_success — NOT YET RUN
WITH numbered AS (
    SELECT a.research_subject_id, a.task_key, a.status, a.determined_by, s.session_no,
           row_number() OVER (PARTITION BY a.research_subject_id, a.task_key
                              ORDER BY s.session_no, a.end_ms) AS k
    FROM attempts a JOIN session_order s ON s.session_id = a.session_id
    WHERE a.task_key IS NOT NULL AND a.rate_eligible
),
per_subject AS (
    SELECT task_key, research_subject_id,
           min(k) FILTER (WHERE status = 'success'
                            AND determined_by IS DISTINCT FROM 'model_inferred') AS k_success,
           bool_or(session_no IS NULL)                                          AS order_unknown
    FROM numbered GROUP BY ALL
)
SELECT task_key,
       count(*) FILTER (WHERE NOT order_unknown)                                    AS n_subjects,
       count(*) FILTER (WHERE NOT order_unknown AND k_success = 1)
         / nullif(count(*) FILTER (WHERE NOT order_unknown), 0)                     AS first_try_success_share,
       quantile_cont(k_success, 0.5) FILTER (WHERE NOT order_unknown)               AS median_k_among_succeeded,
       count(*) FILTER (WHERE NOT order_unknown AND k_success IS NULL)              AS n_no_success_yet,
       count(*) FILTER (WHERE order_unknown)                                        AS n_excluded_order_unknown
FROM per_subject GROUP BY task_key ORDER BY task_key;
```

### M1.4 `help_request_rate_per_attempt`

| | |
|---|---|
| Business question | On which tasks do participants ask for help, and how often? |
| Formula and denominator | Weighted attempts with ≥1 `help` `requested` ÷ weighted attempts (**all** attempts, because a request is observed whatever the outcome). Also reported: requests per attempt |
| Time window | Per `game_build` |
| Eligible population | Attempts in sessions where the assistant was available (`assistant_available`; otherwise asking is impossible). rate-eligible sampling |
| Unit of independence | Subject |
| Label source | `help` events: the request itself is directly observed. Its `intent` has its own `source_type` |
| Missing data | Help asked outside any attempt (no `episode_id`) is counted separately, not assigned to a task |
| Required precision | ±10 pp per task (proposed) |
| Allowed segmentations | `task_key`, `game_build`, help intent, channel (voice, text, button), constraint set, game UI language |
| Consent | `improve_syrup`; `aggregate_analytics` with rights |
| Computable when | P1 |

```sql
-- M1.4 help_request_rate_per_attempt — NOT YET RUN
WITH a AS (
    SELECT a.research_subject_id, a.task_key, x.help_requests, a.w AS w
    FROM attempts a
    JOIN attempt_exposure x ON x.episode_id = a.episode_id AND x.attempt_no = a.attempt_no
    JOIN session_order s ON s.session_id = a.session_id
    WHERE a.rate_eligible AND s.assistant_available IS DISTINCT FROM FALSE
),
per_subject AS (
    SELECT task_key, research_subject_id,
           sum(w * CAST(help_requests > 0 AS INTEGER)) AS y,
           sum(w)                                      AS n,
           sum(w * help_requests)                      AS requests
    FROM a GROUP BY ALL
)
SELECT task_key, count(*) AS n_subjects, sum(n) AS attempts_weighted,
       sum(y) / sum(n)              AS attempts_with_help_share,
       ratio_se(y, n)               AS se_cluster,
       sum(requests) / sum(n)       AS requests_per_attempt,
       evidence(count(*), 1.96 * ratio_se(y, n), 0.10) AS evidence
FROM per_subject GROUP BY ALL ORDER BY task_key;

-- Help asked outside any attempt (reported, never assigned to a task):
SELECT count(*) AS requests_outside_attempts, count(DISTINCT research_subject_id) AS n_subjects
FROM v_help WHERE phase = 'requested' AND episode_id IS NULL;
```

### M1.5 `unobserved_outcome_rate`

| | |
|---|---|
| Business question | For what share of attempts did the system **not** see how they ended, and why? Shown beside every completion metric, it says how far those metrics can be trusted |
| Formula and denominator | Weighted attempts with `status` in (`unobserved`, `censored`) ÷ weighted attempts. A second table breaks it down by `unobserved_reason` |
| Time window | As the completion metric it accompanies |
| Eligible population | All attempts with rate-eligible sampling |
| Unit of independence | Subject |
| Label source | The episode builder's outcome determination (`DATA_CONTRACTS.md`): focus loss, Syrup closing, recording end, detector failure |
| Missing data | This metric **is** the missing-data measure. A reason that was not recorded is its own category (`not_recorded`) |
| Required precision | ±5 pp (proposed). A segment above 20% unobserved (proposed) has its completion metrics flagged "outcome coverage low" |
| Allowed segmentations | `task_key`, `game_build`, `client_version`, platform, reason |
| Consent | `improve_syrup` |
| Computable when | P1 |

```sql
-- M1.5 unobserved_outcome_rate — NOT YET RUN
WITH per_subject AS (
    SELECT task_key, research_subject_id,
           coalesce(sum(w)
                    FILTER (WHERE status IN ('unobserved', 'censored')), 0) AS y,
           sum(w)                                  AS n
    FROM attempts WHERE rate_eligible
    GROUP BY ALL
)
SELECT task_key, count(*) AS n_subjects, sum(n) AS attempts_weighted,
       sum(y) / sum(n) AS unobserved_share, ratio_se(y, n) AS se_cluster,
       sum(y) / sum(n) > 0.20 AS outcome_coverage_low
FROM per_subject GROUP BY ALL ORDER BY unobserved_share DESC;

SELECT status, coalesce(unobserved_reason, 'not_recorded') AS reason,
       count(*) AS attempts, count(DISTINCT research_subject_id) AS n_subjects
FROM attempts WHERE status IN ('unobserved', 'censored')
GROUP BY ALL ORDER BY attempts DESC;
```

---

## F2 Navigation and UI friction

Serves product A. **Map identity** comes only from an id in a licensed, version-matched reference list
for the game build. The map name the screen shows is game text: untrusted, possibly not the game's own.
Today the play stats keep map names on the PC only (`docs/data-and-metrics.md`). Without the reference
list, the `map` component is `unknown` and F2 cannot be computed. **There is no invented "optimal route"**: an
excess-path figure exists only against a licensed reference (M2.4). The UI panel detectors run only for
the preview window today (`bench/README.md`, phase 2). Putting them on the research path is P2 work,
and its overhead must be measured (`DATA_ARCHITECTURE.md` §11).

### M2.1 `map_revisit_rate`

| | |
|---|---|
| Business question | Do participants go back and forth between the same areas while trying a task (a sign of being lost)? |
| Formula and denominator | Per attempt: revisits = changes into a map already seen earlier in the attempt. Outputs: the weighted share of attempts with ≥1 revisit, and revisits per attempt. Denominator: attempts with ≥2 known map observations |
| Time window | Per `game_build` (maps change with patches) |
| Eligible population | Attempts with a known `task_key`, rate-eligible sampling, and map ids from the reference list |
| Unit of independence | Subject |
| Label source | `observation` of the `map` component (a `category` value; `direct_observation` by the detector, validated against the reference list) |
| Missing data | Unknown-map observations are skipped. A gap where the game was out of view is not a revisit, and only consecutive known observations count. Attempts with fewer than 2 known observations are excluded and counted |
| Required precision | ±10 pp per task (proposed) |
| Allowed segmentations | `task_key`, `game_build`, game UI language, assisted (descriptive) |
| Consent | `improve_syrup`; `aggregate_analytics` with rights |
| Computable when | P2 (needs the reference list for `map`); P1 only on synthetic map ids |

```sql
-- M2.1 map_revisit_rate — NOT YET RUN
WITH m AS (
    SELECT a.research_subject_id, a.task_key, a.episode_id, a.attempt_no, a.w,
           o.sequence_no, o.value_text AS map_id
    FROM attempts a
    JOIN v_observation o
      ON o.episode_id = a.episode_id AND o.t_ms BETWEEN a.start_ms AND a.end_ms AND NOT a.spans_sessions
    WHERE o.component = 'map' AND o.value_text IS NOT NULL
      AND a.task_key IS NOT NULL AND a.rate_eligible
),
changes AS (
    SELECT *, lag(map_id) OVER (PARTITION BY episode_id, attempt_no ORDER BY sequence_no) AS prev_map
    FROM m
),
rv AS (   -- entering a map already seen earlier in the same attempt
    SELECT c.episode_id, c.attempt_no, count(*) AS n_revisits
    FROM changes c
    WHERE c.prev_map IS NOT NULL AND c.map_id <> c.prev_map
      AND EXISTS (SELECT 1 FROM m e WHERE e.episode_id = c.episode_id AND e.attempt_no = c.attempt_no
                                      AND e.sequence_no < c.sequence_no AND e.map_id = c.map_id)
    GROUP BY ALL
),
per_attempt AS (
    SELECT research_subject_id, task_key, episode_id, attempt_no,
           any_value(w) AS w, count(*) AS known_obs
    FROM m GROUP BY ALL
),
per_subject AS (
    SELECT p.task_key, p.research_subject_id,
           sum(p.w * CAST(coalesce(r.n_revisits, 0) > 0 AS INTEGER)) AS y,
           sum(p.w)                                                  AS n,
           sum(p.w * coalesce(r.n_revisits, 0))                      AS revisits
    FROM per_attempt p LEFT JOIN rv r USING (episode_id, attempt_no)
    WHERE p.known_obs >= 2
    GROUP BY ALL
)
SELECT task_key, count(*) AS n_subjects, sum(n) AS attempts_weighted,
       sum(y) / sum(n) AS attempts_with_revisit_share, ratio_se(y, n) AS se_cluster,
       sum(revisits) / sum(n) AS revisits_per_attempt
FROM per_subject GROUP BY ALL ORDER BY attempts_with_revisit_share DESC;
```

### M2.2 `menu_reopen_rate`

| | |
|---|---|
| Business question | Which UI panels do participants open, close and open again shortly after (a sign they did not find what they looked for)? |
| Formula and denominator | The `menu_open` component's value names the open menu (`not_visible` = none). An open of menu X is X appearing. A reopen is an open of X within 60 s (a pre-registered parameter; 30 s and 120 s as sensitivity) after X disappeared, in the same episode. Metric: weighted reopens ÷ weighted opens, per menu |
| Time window | Per `game_build` and `detector_version` |
| Eligible population | Episodes with `menu_open` readings, rate-eligible sampling |
| Unit of independence | Subject |
| Label source | `observation` of `menu_open` (a `category` value), `direct_observation` |
| Missing data | `unknown` readings are dropped. The transition across them is taken as adjacent, so a reopen hidden in a gap is missed (a lower bound). The first open in an episode can never be a reopen and counts in the denominator only |
| Required precision | ±10 pp per menu (proposed) |
| Allowed segmentations | menu, `task_key`, `game_build`, game UI language |
| Consent | `improve_syrup`; `aggregate_analytics` with rights |
| Computable when | P2 (menu detectors on the research path; the meaning of `menu_open`'s value to be fixed in `DATA_CONTRACTS.md`) |

```sql
-- M2.2 menu_reopen_rate — NOT YET RUN
WITH s AS (
    SELECT research_subject_id, episode_id, sequence_no, t_ms, w,
           CASE WHEN status = 'observed' THEN value_text END AS open_menu    -- NULL: none open
    FROM v_observation
    WHERE component = 'menu_open' AND status IN ('observed', 'not_visible')
      AND episode_id IS NOT NULL AND rate_eligible
),
t AS (
    SELECT *, lag(open_menu) OVER (PARTITION BY episode_id ORDER BY sequence_no) AS prev_menu FROM s
),
closes AS (
    SELECT episode_id, prev_menu AS menu, t_ms AS closed_ms
    FROM t WHERE prev_menu IS NOT NULL AND prev_menu IS DISTINCT FROM open_menu
),
opens AS (
    SELECT t.research_subject_id, t.open_menu AS menu, t.w,
           EXISTS (SELECT 1 FROM closes c
                   WHERE c.episode_id = t.episode_id AND c.menu = t.open_menu
                     AND c.closed_ms <= t.t_ms AND t.t_ms - c.closed_ms <= 60000) AS reopened
    FROM t WHERE t.open_menu IS NOT NULL AND t.open_menu IS DISTINCT FROM t.prev_menu
),
per_subject AS (
    SELECT menu, research_subject_id,
           coalesce(sum(w) FILTER (WHERE reopened), 0) AS y, sum(w) AS n
    FROM opens GROUP BY ALL
)
SELECT menu, count(*) AS n_subjects, sum(n) AS opens_weighted,
       sum(y) / nullif(sum(n), 0) AS reopen_within_60s_share, ratio_se(y, n) AS se_cluster
FROM per_subject GROUP BY menu ORDER BY reopen_within_60s_share DESC;
```

### M2.3 `time_to_locate`

| | |
|---|---|
| Business question | How long do participants search for a portal, an NPC or a button they set out to find? |
| Formula and denominator | For goals of kind `find_npc` or `find_place`: the first attempt per subject. T = its duration until success, censored at the attempt's end when it was not found. Estimand: the Kaplan–Meier median. Denominator: subjects |
| Time window | Per `game_build` |
| Eligible population | Subjects with a `find_npc` or `find_place` attempt, rate-eligible sampling |
| Unit of independence | Subject (first attempt only) |
| Label source | Outcome `success` = the target was reached (`direct_observation`, or `human_asserted` "found it"). `model_inferred` is excluded from the primary analysis |
| Missing data | Not found while observed = censored. A failure or an abort is a competing risk: it is censored here, and a cumulative-incidence analysis is the sensitivity analysis |
| Required precision | ±25% of the median (proposed); at least 20 subjects per task |
| Allowed segmentations | `task_key`, `game_build`, help intent `locate` asked or not (descriptive) |
| Consent | `improve_syrup`; `aggregate_analytics` with rights |
| Computable when | P1 |

```sql
-- M2.3 time_to_locate — NOT YET RUN
WITH firsts AS (
    SELECT a.*, row_number() OVER (PARTITION BY a.research_subject_id, a.task_key
                                   ORDER BY s.session_no, a.end_ms) AS k
    FROM attempts a JOIN session_order s ON s.session_id = a.session_id
    WHERE a.goal_kind IN ('find_npc', 'find_place') AND a.rate_eligible
),
km_in AS (
    SELECT task_key, duration_ms / 1000.0 AS t_s,
           CAST(status = 'success' AND determined_by IS DISTINCT FROM 'model_inferred' AS INTEGER) AS found
    FROM firsts WHERE k = 1 AND duration_ms IS NOT NULL
),
km AS (SELECT task_key, t_s, sum(found) AS d, count(*) AS leaving FROM km_in GROUP BY ALL),
surv AS (
    SELECT *, product(1 - d / at_risk) OVER (PARTITION BY task_key ORDER BY t_s) AS s_t
    FROM (SELECT *, sum(leaving) OVER (PARTITION BY task_key ORDER BY t_s DESC) AS at_risk FROM km)
)
SELECT task_key, sum(leaving) AS n_subjects, sum(d) AS n_found,
       min(t_s) FILTER (WHERE s_t <= 0.5) AS median_seconds_to_locate      -- NULL = not reached
FROM surv GROUP BY task_key ORDER BY task_key;
```

### M2.4 `excess_path_ratio` — only against a licensed reference

| | |
|---|---|
| Business question | In successful attempts, how much longer is the route participants take than the shortest route in the game's own map graph? |
| Formula and denominator | Observed map changes from the first to the last observed map of a successful attempt ÷ `shortest_hops` between those maps in the **licensed reference graph for the same `game_build`**. One successful attempt per subject. Estimand: the median ratio |
| Time window | Per `game_build`, and only while the reference is valid |
| Eligible population | Successful attempts whose start and end maps both match a reference row |
| Unit of independence | Subject |
| Label source | `map` observations, plus the reference graph (`publisher_ground_truth` when the studio supplies it) |
| Missing data | **No licensed reference → no number.** The query reports how many attempts had no reference and never substitutes an estimate. The start map may be unobserved (the attempt began out of view), so the ratio is a lower bound on the true route |
| Required precision | ±0.25 on the median ratio (proposed) |
| Allowed segmentations | `task_key`, `game_build` |
| Consent | `improve_syrup`; `aggregate_analytics` with rights for the game **and** for the reference graph |
| Computable when | P2 (`reference/ref_paths.csv`, A19). There is no such file today |

```sql
-- M2.4 excess_path_ratio — NOT YET RUN; P2 (licensed, version-matched reference graph)
WITH m AS (
    SELECT a.research_subject_id, a.task_key, a.game_id, a.game_build, a.episode_id, a.attempt_no,
           o.sequence_no, o.value_text AS map_id,
           lag(o.value_text) OVER (PARTITION BY a.episode_id, a.attempt_no ORDER BY o.sequence_no) AS prev_map
    FROM attempts a
    JOIN v_observation o
      ON o.episode_id = a.episode_id AND o.t_ms BETWEEN a.start_ms AND a.end_ms AND NOT a.spans_sessions
    WHERE a.status = 'success' AND o.component = 'map' AND o.value_text IS NOT NULL
),
path AS (   -- one successful attempt per subject and task (a deterministic pick)
    SELECT research_subject_id, task_key, game_id, game_build, episode_id, attempt_no,
           arg_min(map_id, sequence_no) AS from_map, arg_max(map_id, sequence_no) AS to_map,
           count(*) FILTER (WHERE prev_map IS NOT NULL AND map_id <> prev_map) AS observed_hops
    FROM m GROUP BY ALL
    QUALIFY row_number() OVER (PARTITION BY research_subject_id, task_key
                               ORDER BY episode_id, attempt_no) = 1
),
ref AS (
    SELECT * FROM read_csv('reference/ref_paths.csv', header = true)
    WHERE CAST(valid_until AS DATE) >= current_date
)
SELECT p.task_key, p.game_build,
       count(*)                                        AS n_subjects,
       count(r.shortest_hops)                          AS n_with_reference,
       count(*) - count(r.shortest_hops)               AS n_without_reference,   -- never imputed
       median(p.observed_hops / nullif(r.shortest_hops, 0)) AS median_excess_ratio
FROM path p
LEFT JOIN ref r
  ON r.game_id = p.game_id AND r.game_build = p.game_build
 AND r.from_map = p.from_map AND r.to_map = p.to_map
GROUP BY ALL ORDER BY p.task_key;
```

---

## F3 Learning over time

Whether a skill helped once is later done without help. The directive asks to separate a change in the
**player** from changes in difficulty, equipment, version or the assistant's availability. The
version (`game_build`) and assistant availability are observed and used as strata. Difficulty tier
and equipment are **not** observed today, and that is stated as a limit in every result. None of these
metrics is causal.

### M3.1 `later_unassisted_success`

| | |
|---|---|
| Business question | After a participant succeeds at a task **with** help, do they later succeed at the same task **without** it? |
| Formula and denominator | For each (subject, task) with an assisted success in session s₀: take the first attempt at that task in a later session. y = it succeeded with no advice shown. n = such later attempts with an observed outcome. Reported per stratum: same build or not, and assistant available or not. The primary stratum is "same build, assistant available", where doing it alone was a choice |
| Time window | The later attempt within 28 days of s₀ (proposed; needs `started_at`) |
| Eligible population | Subjects with ≥1 assisted success and ≥1 later attempt at the same task |
| Unit of independence | Subject (a subject may contribute several tasks, so clustered by subject) |
| Label source | Outcome (`model_inferred` excluded); `assistant` shown events |
| Missing data | A later attempt with an unobserved outcome goes to the unobserved share. Subjects with **no** later attempt are not in the denominator and are counted (a selection effect, reported) |
| Required precision | ±10 pp in the primary stratum (proposed) |
| Allowed segmentations | task family, the strata above, constraint `find_myself` |
| Consent | `improve_syrup`; `external_research_training` when exported as episodes |
| Computable when | P1 (needs `started_at` and several sessions per subject) |

```sql
-- M3.1 later_unassisted_success — NOT YET RUN
WITH a AS (
    SELECT a.*, s.session_no, s.started_at, s.assistant_available      -- a.advice_shown: the builder's count
    FROM attempts a
    JOIN session_order s ON s.session_id = a.session_id
    WHERE a.task_key IS NOT NULL AND s.session_no IS NOT NULL
      AND a.rate_eligible
),
first_assisted AS (
    SELECT research_subject_id, task_key, min(session_no) AS s0,
           arg_min(game_build, session_no) AS build0, min(started_at) AS t0
    FROM a
    WHERE status = 'success' AND determined_by IS DISTINCT FROM 'model_inferred' AND advice_shown > 0
    GROUP BY ALL
),
later AS (
    SELECT a.*, f.build0,
           row_number() OVER (PARTITION BY a.research_subject_id, a.task_key
                              ORDER BY a.session_no, a.end_ms) AS k
    FROM a JOIN first_assisted f
      ON f.research_subject_id = a.research_subject_id AND f.task_key = a.task_key
     AND a.session_no > f.s0 AND a.started_at <= f.t0 + INTERVAL 28 DAY
)
SELECT (game_build IS NOT DISTINCT FROM build0)                              AS same_build,
       assistant_available,
       count(DISTINCT research_subject_id)                                   AS n_subjects,
       count(*)                                                              AS n_subject_task_pairs,
       count(*) FILTER (WHERE status = 'success' AND advice_shown = 0
                          AND determined_by IS DISTINCT FROM 'model_inferred')
         / nullif(count(*) FILTER (WHERE status IN ('success', 'failure', 'aborted')), 0)
                                                                             AS unassisted_success_share,
       count(*) FILTER (WHERE status IN ('unobserved', 'censored')) / count(*) AS unobserved_share
FROM later WHERE k = 1
GROUP BY ALL ORDER BY same_build DESC, assistant_available DESC;
-- CI: bootstrap over subjects in the report tool. Difficulty and equipment are not controlled.
```

### M3.2 `help_by_attempt_index`

| | |
|---|---|
| Business question | Over repeated attempts at the same task, does a participant need less help and less time? (A learning curve.) |
| Formula and denominator | For attempt index k = 1…4, on a **balanced panel**: only subjects with ≥4 attempts at the task, so every index has the same people (no survivorship). Per k: mean help requests per attempt, the share of attempts with help, and the median duration of successful attempts. The within-subject slope comes from a mixed model in the report tool (a random intercept per subject; `game_build` and assistant availability as covariates) |
| Time window | Study period; curves that span a build change are flagged (`n_builds > 1`) |
| Eligible population | Subjects with ≥4 attempts at a task |
| Unit of independence | Subject (repeated measures) |
| Label source | `help` events, attempt durations, outcomes |
| Missing data | Attempts with unknown order are excluded. The balanced-panel rule is the survivorship control. The unbalanced curve is shown only as a sensitivity analysis |
| Required precision | A slope's CI must exclude 0 at the pre-registered minimal important difference before a "learning" claim (`DATA_QUALITY_AND_BIAS.md`) |
| Allowed segmentations | `task_key`, `game_build`, assistant availability |
| Consent | `improve_syrup` |
| Computable when | P1 (needs several attempts per subject) |

```sql
-- M3.2 help_by_attempt_index — NOT YET RUN
WITH a AS (
    SELECT a.research_subject_id, a.task_key, a.game_build, s.assistant_available, a.status,
           x.help_requests, a.duration_ms,
           row_number() OVER (PARTITION BY a.research_subject_id, a.task_key
                              ORDER BY s.session_no, a.end_ms) AS k
    FROM attempts a
    JOIN session_order s ON s.session_id = a.session_id
    JOIN attempt_exposure x ON x.episode_id = a.episode_id AND x.attempt_no = a.attempt_no
    WHERE a.task_key IS NOT NULL AND s.session_no IS NOT NULL
      AND a.rate_eligible
),
panel AS (SELECT research_subject_id, task_key FROM a GROUP BY ALL HAVING max(k) >= 4)
SELECT a.task_key, a.k,
       count(DISTINCT a.research_subject_id)                              AS n_subjects,
       avg(a.help_requests)                                               AS help_requests_per_attempt,
       avg(CAST(a.help_requests > 0 AS INTEGER))                          AS attempts_with_help_share,
       median(a.duration_ms) FILTER (WHERE a.status = 'success') / 60000.0 AS median_minutes_successful,
       count(DISTINCT a.game_build)                                       AS n_builds,
       bool_or(a.assistant_available IS DISTINCT FROM TRUE)               AS assistant_sometimes_unavailable
FROM a JOIN panel USING (research_subject_id, task_key)
WHERE a.k <= 4
GROUP BY ALL ORDER BY a.task_key, a.k;
```

---

## F4 Help usefulness

**Shown is not heard, and heard is not followed** (§3.4). Each stage of help has its own denominator.
Where there is no evidence for a stage, its value is "unknown", never "no". "Followed" is reported **by
the source of the evidence**: what the player said (`human_asserted`), what a studio SDK reported
(`publisher_ground_truth`), and what a model inferred from a change of state (`model_inferred`). These
three are never added together. Syrup observes no input (CURRENT_STATE_AUDIT §3), so no "followed" is
ever `direct_observation`.

### M4.1 `help_funnel`

| | |
|---|---|
| Business question | When a participant asks for help, how far does it get: answered, shown, acknowledged, followed, completed, satisfying? Where does help get lost? |
| Formula and denominator | For each `help` `requested`: answered = an advice in reply reached `shown`; heard = an explicit `heard_ack` on that advice; followed = a linked `player_action`, by source; completed = the attempt holding the request ended in `success` (observed); satisfied = a satisfaction self-report. Each stage after "answered" has the answered requests as its denominator. Completion uses observed outcomes and reports the unobserved share beside it |
| Time window | Per `model_version` and `client_version` |
| Eligible population | Help requests with rate-eligible sampling |
| Unit of independence | Subject (the per-stage CI uses the ratio pattern of M1.2 in the report tool) |
| Label source | Syrup's own events for asked and shown; the player's acknowledgements, actions and ratings (`human_asserted`); a studio SDK (`publisher_ground_truth`); state-change inference (`model_inferred`) |
| Missing data | No acknowledgement is **unknown**, not "not heard": the heard share is a lower bound and is labelled so. No satisfaction answer is non-response, reported as a response rate |
| Required precision | ±10 pp per stage and intent (proposed) |
| Allowed segmentations | intent, channel, `model_version`, advice kind, constraint set, `task_key` |
| Consent | `improve_syrup`; `external_research_training` for exported episodes |
| Computable when | P1 for asked, answered, followed and completed; heard and satisfied need the `heard_ack` and `satisfaction` capabilities (§4.2), and until they exist those columns are NULL |

```sql
-- M4.1 help_funnel — NOT YET RUN
WITH req AS (
    SELECT research_subject_id, episode_id, help_id, t_ms, intent, w
    FROM v_help WHERE phase = 'requested' AND rate_eligible
),
shown AS (   -- the first answer shown to each request
    SELECT in_reply_to AS help_id, min_by(advice_id, sequence_no) AS advice_id
    FROM v_assistant WHERE phase = 'shown' AND in_reply_to IS NOT NULL GROUP BY ALL
),
fb AS (
    SELECT target_advice_id AS advice_id,
           bool_or(kind = 'heard_ack')                                      AS heard,          -- §4.2
           max(CAST(value AS INTEGER)) FILTER (WHERE kind = 'satisfaction') AS satisfaction    -- §4.2
    FROM v_feedback WHERE target_advice_id IS NOT NULL AND source_type = 'human_asserted' GROUP BY ALL
),
act AS (
    SELECT linked_advice_id AS advice_id,
           bool_or(source_type = 'human_asserted')         AS followed_said,
           bool_or(source_type = 'publisher_ground_truth') AS followed_publisher,
           bool_or(source_type = 'model_inferred')         AS followed_inferred
    FROM v_action WHERE linked_advice_id IS NOT NULL GROUP BY ALL
),
att AS (
    SELECT r.help_id, a.status, a.determined_by
    FROM req r JOIN attempts a
      ON a.episode_id = r.episode_id AND r.t_ms BETWEEN a.start_ms AND a.end_ms AND NOT a.spans_sessions
),
f AS (
    SELECT r.research_subject_id, r.intent, r.w,
           s.advice_id IS NOT NULL                  AS answered,
           coalesce(b.heard, FALSE)                 AS heard_acked,
           coalesce(c.followed_said, FALSE)         AS followed_said,
           coalesce(c.followed_publisher, FALSE)    AS followed_publisher,
           coalesce(c.followed_inferred, FALSE)     AS followed_inferred,
           t.status, t.determined_by, b.satisfaction
    FROM req r
    LEFT JOIN shown s USING (help_id)
    LEFT JOIN fb b    ON b.advice_id = s.advice_id
    LEFT JOIN act c   ON c.advice_id = s.advice_id
    LEFT JOIN att t   USING (help_id)
)
SELECT intent,
       count(DISTINCT research_subject_id)                                            AS n_subjects,
       sum(w)                                                                         AS requests_weighted,
       sum(w) FILTER (WHERE answered) / sum(w)                                        AS answered_share,
       CASE WHEN has('heard_ack') THEN coalesce(sum(w) FILTER (WHERE heard_acked), 0)
            / nullif(sum(w) FILTER (WHERE answered), 0) END                          AS heard_ack_share_lower_bound,
       coalesce(sum(w) FILTER (WHERE followed_said), 0) / nullif(sum(w) FILTER (WHERE answered), 0)      AS followed_said_share,
       coalesce(sum(w) FILTER (WHERE followed_publisher), 0) / nullif(sum(w) FILTER (WHERE answered), 0) AS followed_publisher_share,
       coalesce(sum(w) FILTER (WHERE followed_inferred), 0) / nullif(sum(w) FILTER (WHERE answered), 0)  AS followed_inferred_share,
       sum(w) FILTER (WHERE answered AND status = 'success'
                        AND determined_by IS DISTINCT FROM 'model_inferred')
         / nullif(sum(w) FILTER (WHERE answered AND status IN ('success', 'failure', 'aborted')), 0)
                                                                                      AS completed_share_observed,
       sum(w) FILTER (WHERE answered AND status IN ('unobserved', 'censored'))
         / nullif(sum(w) FILTER (WHERE answered), 0)                                  AS completion_unobserved_share,
       CASE WHEN has('satisfaction') THEN coalesce(sum(w) FILTER (WHERE satisfaction IS NOT NULL), 0)
            / nullif(sum(w) FILTER (WHERE answered), 0) END                           AS satisfaction_response_rate,
       avg(satisfaction)                                                              AS mean_satisfaction_of_responders
FROM f GROUP BY intent ORDER BY intent;
```

### M4.2 `advice_response`

| | |
|---|---|
| Business question | Of **all** advice shown, including unrequested advice, how much do participants rate as helpful, and how much do they say they followed? |
| Formula and denominator | Per advice kind and solicited/proactive: response rate = rated ÷ shown; helpful share = rated helpful ÷ rated; followed share by evidence source = followed ÷ shown |
| Time window | Per `model_version` |
| Eligible population | `assistant` `shown`, rate-eligible |
| Unit of independence | Subject |
| Label source | The player's ratings (`human_asserted`); follow-through by source as in M4.1 |
| Missing data | Unrated advice is non-response, not "unhelpful". The helpful share is reported with its response rate, never alone |
| Required precision | ±10 pp on the helpful share per kind (proposed) |
| Allowed segmentations | advice kind, proactive, `model_version`, constraint set, length band |
| Consent | `improve_syrup` |
| Computable when | P1 |

```sql
-- M4.2 advice_response — NOT YET RUN
WITH shown AS (
    SELECT research_subject_id, advice_id, kind, in_reply_to IS NULL AS proactive,
           w AS w
    FROM v_assistant WHERE phase = 'shown' AND rate_eligible
),
fb AS (
    SELECT target_advice_id AS advice_id,
           bool_or(kind IN ('helpful', 'not_helpful')) AS rated, bool_or(kind = 'helpful') AS helpful
    FROM v_feedback WHERE source_type = 'human_asserted' GROUP BY ALL
),
act AS (
    SELECT linked_advice_id AS advice_id,
           bool_or(source_type = 'human_asserted') AS followed_said,
           bool_or(source_type = 'model_inferred') AS followed_inferred
    FROM v_action WHERE linked_advice_id IS NOT NULL GROUP BY ALL
),
per_subject AS (
    SELECT s.kind, s.proactive, s.research_subject_id,
           sum(s.w)                                                  AS n_shown,
           coalesce(sum(s.w) FILTER (WHERE b.rated), 0)              AS n_rated,
           coalesce(sum(s.w) FILTER (WHERE b.helpful), 0)            AS n_helpful,
           coalesce(sum(s.w) FILTER (WHERE c.followed_said), 0)      AS n_followed_said,
           coalesce(sum(s.w) FILTER (WHERE c.followed_inferred), 0)  AS n_followed_inferred
    FROM shown s LEFT JOIN fb b USING (advice_id) LEFT JOIN act c USING (advice_id)
    GROUP BY ALL
)
SELECT kind, proactive, count(*) AS n_subjects, sum(n_shown) AS shown_weighted,
       sum(n_rated) / sum(n_shown)                AS response_rate,
       sum(n_helpful) / nullif(sum(n_rated), 0)   AS helpful_share_of_rated,
       ratio_se(n_helpful, n_rated)               AS se_cluster,
       sum(n_followed_said) / sum(n_shown)        AS followed_said_share,
       sum(n_followed_inferred) / sum(n_shown)    AS followed_inferred_share
FROM per_subject GROUP BY ALL ORDER BY kind, proactive;
```

### M4.3 `satisfaction_self_report`

| | |
|---|---|
| Business question | How satisfied are participants with an episode, by how it ended? (Satisfaction is **not** correctness, and success does not prove the wording was preferred, §6.) |
| Formula and denominator | Episode-level satisfaction (1–5, from the optional check-in at the end of an episode): the response rate = episodes answered ÷ episodes; the mean among responders. Per outcome |
| Time window | Per `model_version` |
| Eligible population | Episodes with rate-eligible sampling. Answering is optional, and the check-in respects "don't interrupt" |
| Unit of independence | Subject |
| Label source | `feedback` `satisfaction`, `human_asserted`, with no `target_advice_id` |
| Missing data | Non-response is reported, never imputed. Low response (below 30%, proposed) is flagged, because responders differ from non-responders |
| Required precision | ±0.3 on the 1–5 mean per outcome (proposed) |
| Allowed segmentations | outcome, `model_version`, constraint set, experiment arm |
| Consent | `improve_syrup` |
| Computable when | P1 once the `satisfaction` capability exists (§4.2) |

```sql
-- M4.3 satisfaction_self_report — NOT YET RUN
WITH sat AS (
    SELECT episode_id, max(CAST(value AS INTEGER)) AS satisfaction
    FROM v_feedback
    WHERE kind = 'satisfaction' AND source_type = 'human_asserted' AND target_advice_id IS NULL
    GROUP BY ALL
),
per_subject AS (
    SELECT e.outcome, e.research_subject_id,
           count(*)                           AS n_episodes,
           count(s.satisfaction)              AS n_responded,
           coalesce(sum(s.satisfaction), 0)   AS sum_satisfaction
    FROM ep e LEFT JOIN sat s USING (episode_id)
    WHERE e.rate_eligible
    GROUP BY ALL
)
SELECT outcome, count(*) AS n_subjects, sum(n_episodes) AS n_episodes,
       CASE WHEN has('satisfaction') THEN sum(n_responded) / sum(n_episodes) END AS response_rate,
       sum(sum_satisfaction) / nullif(sum(n_responded), 0)    AS mean_satisfaction_1_to_5,
       ratio_se(sum_satisfaction, n_responded)                AS se_cluster,
       CASE WHEN has('satisfaction') THEN sum(n_responded) / sum(n_episodes) < 0.30 END AS low_response_flag
FROM per_subject GROUP BY outcome ORDER BY outcome;
-- Schema 0.1.0 has no satisfaction event (§4.2): every column but the counts is NULL until it does.
```

---

## F5 Interruption and user control

What the assistant costs the player in attention, and how much control the player exercises. Each
of these metrics is a **cost or a control signal**. None is an engagement target. Per-hour rates use
the **observed** game time as the denominator: in-view time from the session's own `start`, `pause`,
`resume` and `end` events (`observed_time`, §4.1). Because the numerator and the denominator must come
from the same sampling, per-hour rates use rate-eligible events only.

### M5.1 `proactive_per_observed_hour`

| | |
|---|---|
| Business question | How often does the assistant speak up without being asked (warnings, coaching, hints), per hour of observed play? |
| Formula and denominator | Proactive advice `shown` (`in_reply_to` NULL) ÷ observed hours. A per-subject ratio estimator, plus the distribution of per-subject rates (p50, p90) |
| Time window | Per `client_version` and `model_version` |
| Eligible population | Subjects with observed time > 0; rate-eligible events |
| Unit of independence | Subject |
| Label source | Syrup's own `assistant` events; in-view time from the session events |
| Missing data | Time with the game out of view is not in the denominator (nothing could be observed then). An interval with unknown focus time is excluded and counted |
| Required precision | ±15% of the rate (proposed) |
| Allowed segmentations | advice kind (warning, coach, hint), attitude setting, `client_version` |
| Consent | `improve_syrup` |
| Computable when | P1 |

```sql
-- M5.1 proactive_per_observed_hour — NOT YET RUN
WITH pro AS (
    SELECT research_subject_id, kind, count(*) AS n_proactive
    FROM v_assistant
    WHERE phase = 'shown' AND in_reply_to IS NULL AND rate_eligible
    GROUP BY ALL
),
hours AS (
    SELECT research_subject_id, sum(observed_ms) / 3600000.0 AS observed_h
    FROM observed_time GROUP BY ALL HAVING sum(observed_ms) > 0
),
kinds AS (SELECT DISTINCT kind FROM pro),
grid AS (   -- every subject × kind, so a subject with none of a kind counts as 0 lines
    SELECT h.research_subject_id, k.kind, h.observed_h, coalesce(p.n_proactive, 0) AS n
    FROM hours h CROSS JOIN kinds k
    LEFT JOIN pro p ON p.research_subject_id = h.research_subject_id AND p.kind = k.kind
)
SELECT kind, count(*) AS n_subjects, sum(observed_h) AS observed_hours,
       sum(n) / sum(observed_h)                     AS proactive_per_hour,
       ratio_se(n, observed_h)                      AS se_cluster,
       quantile_cont(n / observed_h, [0.5, 0.9])    AS per_subject_p50_p90
FROM grid GROUP BY kind ORDER BY kind;
```

### M5.2 `control_requests_per_100_proactive`

| | |
|---|---|
| Business question | How often do participants tell the assistant to stop interrupting or mute it, relative to how often it interrupts? |
| Formula and denominator | (`dont_interrupt` + `mute` feedback aimed at a proactive line) × 100 ÷ proactive lines shown, per kind. Global mutes (not aimed at a line) are counted per observed hour in a second output |
| Time window | Per `client_version` |
| Eligible population | Subjects with ≥1 proactive line; rate-eligible events |
| Unit of independence | Subject |
| Label source | The player's control actions (`human_asserted`: a button press or a voice command) |
| Missing data | A control that cannot be tied to a line counts only in the global output |
| Required precision | ±2 per 100 lines (proposed) |
| Allowed segmentations | advice kind, attitude, length band |
| Consent | `improve_syrup` |
| Computable when | P1 |

```sql
-- M5.2 control_requests_per_100_proactive — NOT YET RUN
WITH pro AS (
    SELECT research_subject_id, kind, count(*) AS n
    FROM v_assistant WHERE phase = 'shown' AND in_reply_to IS NULL AND rate_eligible
    GROUP BY ALL
),
ctl AS (
    SELECT f.research_subject_id, a.kind, count(*) AS n_control
    FROM v_feedback f
    JOIN v_assistant a ON a.advice_id = f.target_advice_id AND a.phase = 'shown' AND a.in_reply_to IS NULL
    WHERE f.kind IN ('dont_interrupt', 'mute')
    GROUP BY ALL
)
SELECT p.kind, count(*) AS n_subjects, sum(p.n) AS proactive_shown,
       100.0 * sum(coalesce(c.n_control, 0)) / sum(p.n)       AS control_per_100_lines,
       100.0 * ratio_se(coalesce(c.n_control, 0), p.n)        AS se_cluster_per_100
FROM pro p LEFT JOIN ctl c USING (research_subject_id, kind)
GROUP BY p.kind ORDER BY p.kind;

-- Global mutes per observed hour:
SELECT count(*) / (SELECT sum(observed_ms) / 3600000.0 FROM observed_time) AS global_mutes_per_hour,
       count(DISTINCT research_subject_id) AS n_subjects
FROM v_feedback WHERE kind IN ('mute', 'dont_interrupt') AND target_advice_id IS NULL;
```

### M5.3 `advice_cancel_rate`

| | |
|---|---|
| Business question | How often do participants cut a line off before it ends? |
| Formula and denominator | Advice the **player** cancelled ÷ advice shown, per kind (a line the system replaced is not counted) |
| Time window | Per `model_version` |
| Eligible population | `assistant` shown, rate-eligible |
| Unit of independence | Subject |
| Label source | `assistant` `cancelled` (Syrup records the player's stop) |
| Missing data | Advice with no recorded end state (Syrup closed while it played) is excluded and counted |
| Required precision | ±5 pp (proposed) |
| Allowed segmentations | kind, length band, proactive or not, channel |
| Consent | `improve_syrup` |
| Computable when | P1 once the `advice_cancel` capability exists (§4.2). Until then the share is NULL (no line has a known end), never 0 |

```sql
-- M5.3 advice_cancel_rate — NOT YET RUN
WITH adv AS (
    SELECT research_subject_id, advice_id, any_value(kind) AS kind,
           bool_or(phase = 'shown') AS shown, bool_or(phase = 'cancelled') AS cancelled,
           bool_or(phase IN ('cancelled', 'completed')) AS ended_known
    FROM v_assistant WHERE rate_eligible
    GROUP BY ALL
),
per_subject AS (
    SELECT kind, research_subject_id,
           count(*) FILTER (WHERE shown AND ended_known)  AS n,
           count(*) FILTER (WHERE shown AND cancelled)    AS y,
           count(*) FILTER (WHERE shown AND NOT ended_known) AS n_end_unknown
    FROM adv GROUP BY ALL
)
SELECT kind, count(*) AS n_subjects, sum(n) AS shown_with_known_end,
       sum(y) / nullif(sum(n), 0) AS cancelled_share, ratio_se(y, n) AS se_cluster,
       sum(n_end_unknown) AS excluded_end_unknown
FROM per_subject GROUP BY kind ORDER BY kind;
```

### M5.4 `response_length`

| | |
|---|---|
| Business question | How long are the assistant's lines, and do they get shorter after a participant asks for shorter answers? |
| Formula and denominator | The distribution (p50, p90) of `length_chars` and `spoken_ms` per kind, split into before and after the first explicit "shorter" preference in the session, and sessions with no preference stated |
| Time window | Per `model_version` |
| Eligible population | Advice shown |
| Unit of independence | Subject. The pooled quantiles are descriptive. Per-subject medians are the robustness check |
| Label source | Syrup's own `assistant` events; the preference is `human_asserted` |
| Missing data | A NULL length is unknown and is not a zero-length line |
| Required precision | Descriptive; at least 20 subjects per row |
| Allowed segmentations | kind, `model_version`, attitude, channel |
| Consent | `improve_syrup` |
| Computable when | P1 |

```sql
-- M5.4 response_length — NOT YET RUN
WITH shown AS (
    SELECT research_subject_id, session_id, sequence_no, kind, length_chars, spoken_ms
    FROM v_assistant WHERE phase = 'shown'
),
pref AS (
    SELECT research_subject_id, session_id, min(sequence_no) AS pref_seq
    FROM v_feedback WHERE kind = 'preference' AND value = 'shorter' AND source_type = 'human_asserted'
    GROUP BY ALL
)
SELECT s.kind,
       CASE WHEN p.pref_seq IS NULL THEN 'no_preference_stated'
            WHEN s.sequence_no < p.pref_seq THEN 'before_shorter'
            ELSE 'after_shorter' END                    AS preference_state,
       count(DISTINCT s.research_subject_id)            AS n_subjects,
       count(*)                                         AS n_advice,
       quantile_cont(s.length_chars, [0.5, 0.9])        AS chars_p50_p90,
       quantile_cont(s.spoken_ms / 1000.0, [0.5, 0.9])  AS spoken_seconds_p50_p90
FROM shown s LEFT JOIN pref p USING (research_subject_id, session_id)
GROUP BY ALL ORDER BY s.kind, preference_state;
```

### M5.5 `repetition_rate`

| | |
|---|---|
| Business question | How often does the assistant repeat itself (the same line within ten minutes)? |
| Formula and denominator | Lines shown whose `template_id` was already shown in the same session within the last 10 minutes ÷ lines shown with a `template_id` |
| Time window | Per `model_version` and `client_version` |
| Eligible population | Lines with a `template_id` (templated warnings and coach lines) |
| Unit of independence | Subject |
| Label source | Syrup's own events |
| Missing data | Free-form model lines have no `template_id` and are excluded and counted. A near-duplicate measure for them would need the text, which the export does not carry by design |
| Required precision | ±5 pp (proposed) |
| Allowed segmentations | kind, attitude, `client_version` |
| Consent | `improve_syrup` |
| Computable when | P1 once the `template_id` capability exists (§4.2). Until then it returns no rows, and every line is counted as free-form |

```sql
-- M5.5 repetition_rate — NOT YET RUN
WITH shown AS (
    SELECT research_subject_id, session_id, sequence_no, t_ms, kind, template_id
    FROM v_assistant WHERE phase = 'shown'
),
flagged AS (
    SELECT *, lag(t_ms) OVER (PARTITION BY session_id, template_id ORDER BY sequence_no) AS prev_same_ms
    FROM shown WHERE template_id IS NOT NULL
),
per_subject AS (
    SELECT kind, research_subject_id, count(*) AS n,
           count(*) FILTER (WHERE t_ms - prev_same_ms <= 600000) AS y
    FROM flagged GROUP BY ALL
)
SELECT kind, count(*) AS n_subjects, sum(n) AS templated_lines,
       sum(y) / sum(n) AS repeated_within_10min_share, ratio_se(y, n) AS se_cluster,
       (SELECT count(*) FROM shown WHERE template_id IS NULL) AS excluded_free_form_lines
FROM per_subject GROUP BY kind ORDER BY kind;
```

### M5.6 `constraint_violation_rate`

| | |
|---|---|
| Business question | When a participant set a constraint ("no spoilers", "hint only", "I want to find it myself"), how often did the advice break it? (Constraint compliance, §6.) |
| Formula and denominator | Advice shown in episodes carrying constraint c that was judged to break it ÷ advice shown in those episodes. There are three judges, **kept apart**: (1) `constraints_respected = false` on the advice itself, reported by its `source_type` (in 0.1.0 a model or rule judges its own output: `model_inferred`); (2) the player's flag "that was a spoiler" (`constraint_flag`, §4.2); (3) a reviewer's label on a random sample (P3) |
| Time window | Per `model_version` |
| Eligible population | Episodes with ≥1 constraint |
| Unit of independence | Subject |
| Label source | As above. A system judging its own compliance is a claim about itself and never stands in for (2) or (3) |
| Missing data | `constraints_respected` NULL is unknown and stays out of the numerator **and** the denominator, counted. An unflagged line is not "compliant": the player rate is a **lower bound** |
| Required precision | ±5 pp (proposed); the reviewed rate is the gate |
| Allowed segmentations | constraint kind, `model_version`, advice kind |
| Consent | `improve_syrup`; `external_research_training` (the constraint corpus of §6) |
| Computable when | P1 for (1); (2) once `constraint_flag` exists; P3 for (3) |

```sql
-- M5.6 constraint_violation_rate — NOT YET RUN
WITH c AS (
    SELECT episode_id, research_subject_id, unnest(constraints) AS constraint_kind
    FROM ep WHERE len(constraints) > 0
),
shown AS (
    SELECT episode_id, advice_id, constraints_respected, source_type
    FROM v_assistant WHERE phase = 'shown'
),
flags AS (
    SELECT DISTINCT target_advice_id AS advice_id FROM v_feedback
    WHERE kind = 'constraint_violation' AND source_type = 'human_asserted'
),
per_subject AS (
    SELECT c.constraint_kind, s.source_type AS self_judged_by, c.research_subject_id,
           count(*)                                                    AS n_advice,
           count(s.constraints_respected)                              AS n_self_judged,
           count(*) FILTER (WHERE s.constraints_respected = FALSE)     AS n_self_broken,
           count(f.advice_id)                                          AS n_player_flagged
    FROM c JOIN shown s USING (episode_id) LEFT JOIN flags f USING (advice_id)
    GROUP BY ALL
)
SELECT constraint_kind, self_judged_by, count(*) AS n_subjects, sum(n_advice) AS advice_under_constraint,
       sum(n_advice) - sum(n_self_judged)                     AS self_judgement_unknown,
       sum(n_self_broken) / nullif(sum(n_self_judged), 0)     AS self_judged_violation_share,
       ratio_se(n_self_broken, n_self_judged)                 AS se_cluster,
       CASE WHEN has('constraint_flag')
            THEN sum(n_player_flagged) / sum(n_advice) END    AS player_flagged_share_lower_bound
FROM per_subject GROUP BY ALL ORDER BY constraint_kind, self_judged_by;
```

---

## F6 Errors and corrections

A correction is a **claim** until it is verified (§7, §11). The original perception, the model's
prediction and the later correction are separate records with lineage, and nothing is overwritten
(`DATA_CONTRACTS.md`). A correction counts toward an "error" only once it is `verified`. Claims are
reported as claims. Errors nobody corrected exist, so correction counts are a lower bound on errors and
are never an error rate. The error rate needs gold (F9, F10).

### M6.1 `corrections_by_category`

| | |
|---|---|
| Business question | What do participants correct most: wrong object, wrong fact, ambiguous direction, outdated source or missing information? Which errors should the self-improvement loop fix first? |
| Formula and denominator | Per target kind and category: claims and verified corrections. Advice corrections per 100 advice shown; observation corrections per 1,000 component readings. Both use rate-eligible denominators |
| Time window | Per `model_version` and `detector_version` |
| Eligible population | All corrections; the denominators come from rate-eligible events |
| Unit of independence | Subject (the subject count is reported for each category) |
| Label source | `correction` events: the player (`human_asserted`, `claimed`), and verification by a reviewer or a check against an authoritative source (`human_reviewed`, `verified` or `rejected`) |
| Missing data | Corrections still pending verification stay `claimed`, never `verified` by default |
| Required precision | Descriptive ranking. A category enters the improvement backlog with ≥5 distinct subjects (proposed): one player cannot set the backlog |
| Allowed segmentations | target kind, category, `model_version`, `detector_version`, `game_build`, component |
| Consent | `improve_syrup` |
| Computable when | P1 |

```sql
-- M6.1 corrections_by_category — NOT YET RUN
WITH adv AS (SELECT count(*) AS n FROM v_assistant WHERE phase = 'shown' AND rate_eligible),
obs AS (SELECT count(*) AS n FROM v_observation WHERE rate_eligible),
cor AS (
    SELECT target_kind, category, research_subject_id,
           count(*)                                                AS n_claims,
           count(*) FILTER (WHERE verification_status = 'verified') AS n_verified
    FROM v_correction GROUP BY ALL
)
SELECT target_kind, category,
       count(DISTINCT research_subject_id)  AS n_subjects,
       sum(n_claims)                        AS claims,
       sum(n_verified)                      AS verified,
       CASE target_kind
            WHEN 'advice'      THEN 100.0  * sum(n_verified) / (SELECT n FROM adv)
            WHEN 'observation' THEN 1000.0 * sum(n_verified) / (SELECT n FROM obs)
       END                                  AS verified_per_100_advice_or_1000_observations,
       count(DISTINCT research_subject_id) >= 5 AS backlog_eligible
FROM cor GROUP BY ALL ORDER BY verified DESC, claims DESC;
```

### M6.2 `correction_verification`

| | |
|---|---|
| Business question | How many corrections are verified, rejected or still claims? Is any one participant flooding corrections (a poisoning signal)? |
| Formula and denominator | Shares of `claimed`, `verified` and `rejected` within each target kind; top-subject share = the largest single subject's corrections ÷ all corrections of that kind |
| Time window | Rolling 28 days (proposed) |
| Eligible population | All corrections |
| Unit of independence | Subject (the concentration measure is about subjects) |
| Label source | `correction.verification_status` and `annotator_type` |
| Missing data | A missing status is reported as `not_recorded`, never as verified |
| Required precision | Descriptive. Flag a kind when one subject supplies over 20% of its corrections (proposed; `DATA_QUALITY_AND_BIAS.md` sets the policy) |
| Allowed segmentations | target kind, category, annotator type |
| Consent | `improve_syrup` |
| Computable when | P1 |

```sql
-- M6.2 correction_verification — NOT YET RUN
WITH c AS (
    SELECT research_subject_id, target_kind, coalesce(verification_status, 'not_recorded') AS status
    FROM v_correction
),
by_subject AS (SELECT target_kind, research_subject_id, count(*) AS n FROM c GROUP BY ALL),
conc AS (SELECT target_kind, max(n) / sum(n) AS top_subject_share FROM by_subject GROUP BY ALL)
SELECT c.target_kind, c.status,
       count(*)                                                       AS corrections,
       count(DISTINCT c.research_subject_id)                          AS n_subjects,
       count(*) / sum(count(*)) OVER (PARTITION BY c.target_kind)     AS share_within_kind,
       any_value(k.top_subject_share)                                 AS top_subject_share,
       any_value(k.top_subject_share) > 0.20                          AS flooding_flag
FROM c JOIN conc k USING (target_kind)
GROUP BY ALL ORDER BY c.target_kind, corrections DESC;
-- `claimed` corrections never change a label, a fixture or a detector until verified.
```

---

## F7 Recovery

What happens after a failure inside an episode: the type of failure, a change of strategy, the time to
recover, and the result. **Two similar attempts are not a counterfactual** (§7). A success after a
change of strategy does not show that the change caused it.

### M7.1 `recovery_rate`

| | |
|---|---|
| Business question | After a failure, how often does the participant go on to succeed in the same episode? Which failure types are hardest to recover from? |
| Formula and denominator | For each attempt with `status = failure`: recovered = a later attempt of the same episode ended in `success` (not `model_inferred`). Recovered share = recovered ÷ failures whose continuation is known. Not recovered is known when the episode ended in an observed failure or abort. The continuation-unknown share is reported beside it |
| Time window | Per `game_build` |
| Eligible population | Failures with rate-eligible sampling |
| Unit of independence | Subject |
| Label source | Outcomes and `failure_type` (as determined, with its `source_type`) |
| Missing data | A continuation that ended unobserved or censored is unknown, never "not recovered" |
| Required precision | ±10 pp per failure type (proposed) |
| Allowed segmentations | failure type, `task_key`, assisted between the attempts or not (descriptive) |
| Consent | `improve_syrup`; `external_research_training` (failure → correction → success sequences, product B) |
| Computable when | P1 |

```sql
-- M7.1 recovery_rate — NOT YET RUN
WITH f AS (
    SELECT research_subject_id, episode_id, attempt_no, coalesce(failure_type, 'unknown') AS failure_type,
           w AS w
    FROM attempts WHERE status = 'failure' AND rate_eligible
),
after AS (
    SELECT f.research_subject_id, f.episode_id, f.attempt_no, f.failure_type, f.w,
           bool_or(a.status = 'success' AND a.determined_by IS DISTINCT FROM 'model_inferred') AS recovered,
           bool_or(a.status IN ('unobserved', 'censored'))                                     AS later_unobserved
    FROM f LEFT JOIN attempts a ON a.episode_id = f.episode_id AND a.attempt_no > f.attempt_no
    GROUP BY ALL
),
per_subject AS (
    SELECT failure_type, research_subject_id,
           coalesce(sum(w) FILTER (WHERE recovered), 0)                                            AS y,
           coalesce(sum(w) FILTER (WHERE recovered OR NOT coalesce(later_unobserved, FALSE)), 0)   AS n_known,
           sum(w)                                                                                  AS n_all
    FROM after GROUP BY ALL
)
SELECT failure_type, count(*) AS n_subjects, sum(n_all) AS failures_weighted,
       sum(y) / nullif(sum(n_known), 0)   AS recovered_share_of_known,
       ratio_se(y, n_known)               AS se_cluster,
       1 - sum(n_known) / sum(n_all)      AS continuation_unknown_share
FROM per_subject GROUP BY failure_type ORDER BY failures_weighted DESC;
```

### M7.2 `strategy_change_after_failure`

| | |
|---|---|
| Business question | After a failure, do participants change strategy, and how do the next attempts go when they do? |
| Formula and denominator | For each failure followed by a next attempt: said_changed = the player reported a change of strategy (`strategy_change`, `human_asserted`); advice_between = advice was shown during the next attempt. Per combination: the next attempt's success share (observed) and unobserved share |
| Time window | Per `game_build` |
| Eligible population | Failures with a next attempt in the same episode |
| Unit of independence | Subject |
| Label source | Self-report for the strategy change. A strategy inferred from action patterns (`model_inferred`) would be a separate, labelled column. It is not computed in P1 |
| Missing data | No report of a strategy change is "unknown", not "unchanged" |
| Required precision | Descriptive, ±10 pp per cell (proposed) |
| Allowed segmentations | failure type, `task_key` |
| Consent | `improve_syrup`; `external_research_training` |
| Computable when | P1 for `advice_between`; the self-report needs the `strategy_change` capability (§4.2), and until it exists `said_changed` is NULL |

```sql
-- M7.2 strategy_change_after_failure — NOT YET RUN
WITH nxt AS (
    SELECT f.research_subject_id, f.episode_id, n.status AS next_status,
           n.start_ms, n.end_ms, n.spans_sessions
    FROM attempts f
    JOIN attempts n ON n.episode_id = f.episode_id AND n.attempt_no = f.attempt_no + 1
    WHERE f.status = 'failure'
),
flags AS (
    SELECT x.research_subject_id, x.episode_id, x.start_ms, x.next_status,
           CASE WHEN has('strategy_change') THEN
                coalesce(bool_or(fb.kind = 'strategy_change' AND fb.source_type = 'human_asserted'), FALSE)
           END                                                                    AS said_changed,
           coalesce(bool_or(ad.phase = 'shown'), FALSE)                           AS advice_between
    FROM nxt x
    LEFT JOIN v_feedback fb
      ON fb.episode_id = x.episode_id AND fb.t_ms BETWEEN x.start_ms AND x.end_ms AND NOT x.spans_sessions
    LEFT JOIN v_assistant ad
      ON ad.episode_id = x.episode_id AND ad.t_ms BETWEEN x.start_ms AND x.end_ms AND NOT x.spans_sessions
    GROUP BY ALL
)
SELECT said_changed, advice_between,
       count(DISTINCT research_subject_id) AS n_subjects, count(*) AS next_attempts,
       count(*) FILTER (WHERE next_status = 'success')
         / nullif(count(*) FILTER (WHERE next_status IN ('success', 'failure', 'aborted')), 0) AS next_success_share_observed,
       count(*) FILTER (WHERE next_status IN ('unobserved', 'censored')) / count(*)           AS next_unobserved_share
FROM flags GROUP BY ALL ORDER BY said_changed DESC, advice_between DESC;
-- Association only: not a counterfactual comparison.
```

### M7.3 `time_to_recovery`

| | |
|---|---|
| Business question | How long does it take to succeed after a failure? |
| Formula and denominator | From the failure's outcome to the next success in the same episode. One failure per subject and failure type (the first, a deterministic pick). Kaplan–Meier median, censored at the last moment the episode was observable |
| Time window | Per `game_build` |
| Eligible population | Failures with rate-eligible sampling |
| Unit of independence | Subject |
| Label source | Outcomes and their `observable_until_ms` |
| Missing data | Censored at the last observable moment. An end that is not observable is never a failure to recover |
| Required precision | ±25% of the median (proposed) |
| Allowed segmentations | failure type, `task_key` |
| Consent | `improve_syrup`; `external_research_training` |
| Computable when | P1 |

```sql
-- M7.3 time_to_recovery — NOT YET RUN
WITH f AS (
    SELECT research_subject_id, episode_id, attempt_no, end_ms AS failed_ms,
           coalesce(failure_type, 'unknown') AS failure_type
    FROM attempts WHERE status = 'failure' AND rate_eligible
    QUALIFY row_number() OVER (PARTITION BY research_subject_id, coalesce(failure_type, 'unknown')
                               ORDER BY episode_id, attempt_no) = 1
),
nxt AS (
    SELECT f.research_subject_id, f.failure_type, f.failed_ms,
           min(a.end_ms) FILTER (WHERE a.status = 'success'
                                   AND a.determined_by IS DISTINCT FROM 'model_inferred') AS recovered_ms,
           max(coalesce(a.observable_until_ms, a.end_ms))                                AS seen_until_ms
    FROM f LEFT JOIN attempts a ON a.episode_id = f.episode_id AND a.attempt_no > f.attempt_no
    GROUP BY ALL
),
km_in AS (
    SELECT failure_type, (coalesce(recovered_ms, seen_until_ms, failed_ms) - failed_ms) / 1000.0 AS t_s,
           CAST(recovered_ms IS NOT NULL AS INTEGER) AS recovered
    FROM nxt
),
km AS (SELECT failure_type, t_s, sum(recovered) AS d, count(*) AS leaving FROM km_in GROUP BY ALL),
surv AS (
    SELECT *, product(1 - d / at_risk) OVER (PARTITION BY failure_type ORDER BY t_s) AS s_t
    FROM (SELECT *, sum(leaving) OVER (PARTITION BY failure_type ORDER BY t_s DESC) AS at_risk FROM km)
)
SELECT failure_type, sum(leaving) AS n_subjects, sum(d) AS n_recovered,
       min(t_s) FILTER (WHERE s_t <= 0.5) AS median_seconds_to_recovery     -- NULL = not reached
FROM surv GROUP BY failure_type ORDER BY failure_type;
```

---

## F8 Localization

Languages are **settings**, not traits. The game's UI language is what the game shows, as set or
detected (else `unknown`). The conversation language is what the participant chose. Neither is ever
used to infer origin, nationality, ethnicity or any other trait. Nothing is inferred from an accent or a
voice (§2.6, directive §5.8).

### M8.1 `comprehension_help_by_language`

| | |
|---|---|
| Business question | Do participants misunderstand in-game text or instructions more often under some UI language and conversation language combinations? Where does localization need work? |
| Formula and denominator | Weighted attempts with ≥1 help request of intent `comprehension` ÷ weighted attempts, per (game UI language × conversation language). A second output contrasts matched and mismatched languages |
| Time window | Per `game_build` |
| Eligible population | Attempts with rate-eligible sampling, in sessions with the assistant available |
| Unit of independence | Subject. A subject who switches languages contributes to several cells, still clustered by subject |
| Label source | The help intent: chosen by the player ("I don't understand the text", `human_asserted`) or classified (`model_inferred`). Analysed separately |
| Missing data | An unknown UI language is its own cell (`unknown`), never dropped |
| Required precision | ±10 pp per language pair (proposed). Small pairs show `insufficient evidence` |
| Allowed segmentations | the two languages, `task_key`, `game_build`, intent source |
| Consent | `improve_syrup`; `aggregate_analytics` with rights (a studio's localization report) |
| Computable when | P1 |

```sql
-- M8.1 comprehension_help_by_language — NOT YET RUN
WITH comp AS (
    SELECT a.episode_id, a.attempt_no, count(h.event_id) AS n_comprehension
    FROM attempts a JOIN v_help h
      ON h.episode_id = a.episode_id AND h.t_ms BETWEEN a.start_ms AND a.end_ms AND NOT a.spans_sessions
    WHERE h.phase = 'requested' AND h.intent = 'comprehension'
    GROUP BY ALL
),
per_subject AS (
    SELECT coalesce(s.game_ui_language, 'unknown')       AS game_ui_language,
           coalesce(s.conversation_language, 'unknown')  AS conversation_language,
           a.research_subject_id,
           sum(CAST(coalesce(c.n_comprehension, 0) > 0 AS INTEGER) * a.w) AS y,
           sum(a.w)                                                AS n
    FROM attempts a
    JOIN session_order s ON s.session_id = a.session_id
    LEFT JOIN comp c ON c.episode_id = a.episode_id AND c.attempt_no = a.attempt_no
    WHERE a.rate_eligible AND s.assistant_available IS DISTINCT FROM FALSE
    GROUP BY ALL
)
SELECT game_ui_language, conversation_language, count(*) AS n_subjects, sum(n) AS attempts_weighted,
       CASE WHEN has('comprehension_intent') THEN sum(y) / sum(n) END AS comprehension_help_share,
       CASE WHEN has('comprehension_intent') THEN ratio_se(y, n) END AS se_cluster,
       CASE WHEN has('comprehension_intent') THEN evidence(count(*), 1.96 * ratio_se(y, n), 0.10)
            ELSE 'not measurable: no comprehension intent in this schema' END AS evidence
FROM per_subject GROUP BY ALL ORDER BY n_subjects DESC;

-- Matched versus mismatched languages (descriptive; the difference's CI comes from a subject bootstrap):
-- rerun the per_subject CTE, grouped by (game_ui_language = conversation_language) instead of the pair.
```

### M8.2 `ui_text_unknown_by_language`

| | |
|---|---|
| Business question | Does Syrup fail to read on-screen text more often in some UI languages (fonts, scripts, right-to-left)? |
| Formula and denominator | The share **of time** text components were unknown: weighted `base_random` readings of `dialog_text` or `screen_text` with `status = unknown` ÷ weighted base readings that were `observed` or `unknown` (`not_visible` = no text on screen, excluded), per UI language and component |
| Time window | Per `detector_version` |
| Eligible population | Base samples while the game was in view (no samples exist otherwise) |
| Unit of independence | Subject |
| Label source | The component's own `status` (`unknown`: looked for and not read) |
| Missing data | This **is** the missing-data measure. Accuracy when read needs gold (M9.1) |
| Required precision | ±5 pp per language (proposed) |
| Allowed segmentations | UI language, component, `detector_version`, `game_build`, platform |
| Consent | `improve_syrup` |
| Computable when | P1 (if base samples of `text.*` exist) |

```sql
-- M8.2 ui_text_unknown_by_language — NOT YET RUN
WITH per_subject AS (
    SELECT coalesce(s.game_ui_language, 'unknown') AS game_ui_language, o.component, o.detector_version,
           o.research_subject_id,
           coalesce(sum(o.w) FILTER (WHERE o.is_unknown), 0) AS y,
           sum(o.w)                                            AS n
    FROM v_observation o JOIN session_order s ON s.session_id = o.session_id
    WHERE o.component IN ('dialog_text', 'screen_text') AND o.status <> 'not_visible'
      AND o.sampling_policy = 'base_random'
    GROUP BY ALL
)
SELECT game_ui_language, component, detector_version, count(*) AS n_subjects, sum(n) AS samples_weighted,
       sum(y) / sum(n) AS unknown_share_of_time, ratio_se(y, n) AS se_cluster
FROM per_subject GROUP BY ALL ORDER BY unknown_share_of_time DESC;
```

---

## F9 Vision quality

Accuracy needs **reviewed gold**, kept apart from automatic labels and from user corrections (§11). An
OCR reading or a player's confirmation is not absolute truth (§7). Gold items come from a random sample
with known probability, never only from the frames where something went wrong (§8). The split into
train, validation and test is made by subject and source sequence before any clip is cut
(`DATA_QUALITY_AND_BIAS.md`). Accuracy is reported on the **test** split only. "Unknown" is never
scored as right or wrong: coverage (how often a value was read) and accuracy when read are separate
numbers. Today every confidence in the code is an uncalibrated heuristic score (CURRENT_STATE_AUDIT
§2.1). So `confidence` is absent from the export until a calibration exists, and M9.6 reports "not
calibrated".

### M9.1 `ocr_accuracy_vs_gold`

| | |
|---|---|
| Business question | When Syrup reads a number or text on screen (level, HP and MP values, quest text), how often is it exactly right? |
| Formula and denominator | Per component: correct ÷ gold items **read** (a value present). Correct means an exact match, and for percents within 1 point. The character error rate (Levenshtein edits ÷ gold characters) is computed for text components when read. Coverage = read ÷ all gold items |
| Time window | Per `detector_version` and `game_build` |
| Eligible population | Observations with a `reviewed_gold` label in the test split |
| Unit of independence | Subject |
| Label source | `labels.label_source = reviewed_gold` (double-annotated and adjudicated, `DATA_QUALITY_AND_BIAS.md`) |
| Missing data | Unread items lower the coverage. They never count as errors or as matches |
| Required precision | ±2 pp on exact match per component (proposed: the HUD numbers drive warnings) |
| Allowed segmentations | component, `detector_version`, `game_build`, UI language, resolution class, platform |
| Consent | `improve_syrup` (internal evaluation); `external_research_training` for a released benchmark |
| Computable when | P3 |

```sql
-- M9.1 ocr_accuracy_vs_gold — NOT YET RUN; P3 (prelude_p2p3.sql)
WITH pairs AS (
    SELECT o.research_subject_id, o.component, o.detector_version, o.value_text AS read, g.value_text AS gold,
           CASE WHEN o.component LIKE '%_percent'                  -- percents: within 1 point
                THEN abs(o.value_number - CAST(g.value_text AS DOUBLE)) <= 1.0
                ELSE o.value_text = g.value_text END AS correct
    FROM v_observation o JOIN labels g ON g.target_event_id = o.event_id
    WHERE g.label_source = 'reviewed_gold' AND g.split = 'test' AND o.status <> 'not_visible'
      AND o.component IN ('level', 'hp_percent', 'mp_percent', 'exp_percent', 'boss_hp_percent',
                          'item_count', 'dialog_text', 'screen_text')
),
per_subject AS (
    SELECT component, detector_version, research_subject_id,
           count(*) FILTER (WHERE read IS NOT NULL AND correct)               AS y_exact,
           count(*) FILTER (WHERE read IS NOT NULL)                            AS n_read,
           count(*)                                                            AS n_all,
           sum(levenshtein(read, gold)) FILTER (WHERE read IS NOT NULL
                                                  AND component LIKE '%_text')  AS edits,
           sum(length(gold)) FILTER (WHERE read IS NOT NULL AND component LIKE '%_text') AS gold_chars
    FROM pairs GROUP BY ALL
)
SELECT component, detector_version, count(*) AS n_subjects, sum(n_all) AS gold_items,
       sum(n_read) / sum(n_all)                AS coverage,
       sum(y_exact) / nullif(sum(n_read), 0)   AS exact_match_when_read,
       ratio_se(y_exact, n_read)               AS se_cluster,
       sum(edits) / nullif(sum(gold_chars), 0) AS cer_when_read
FROM per_subject GROUP BY ALL ORDER BY component;
```

### M9.2 `object_precision_recall`

| | |
|---|---|
| Business question | For each kind of object Syrup detects (taught objects, portals, NPCs), how much of what it reports is there (precision), and how much of what is there does it find (recall)? |
| Formula and denominator | Per class: precision = TP ÷ (TP + FP); recall = TP ÷ (TP + FN). A match is one-to-one with IoU ≥ 0.5, greedy by score, computed by the evaluation tool (`EVALUATION_PLAN.md`); the query only aggregates its output |
| Time window | Per `detector_version` |
| Eligible population | Gold frames in the test split, sampled at random with known probability |
| Unit of independence | Subject |
| Label source | `reviewed_gold` boxes |
| Missing data | Frames the detector did not process (dropped, rejected by the privacy filter) are not gold items. They are counted in M12.8 |
| Required precision | ±5 pp per class (proposed) |
| Allowed segmentations | class, `detector_version`, `game_build`, resolution class |
| Consent | `improve_syrup` |
| Computable when | P3 |

```sql
-- M9.2 object_precision_recall — NOT YET RUN; P3 (eval/matches.jsonl)
WITH per_subject AS (
    SELECT component, detector_version, research_subject_id,
           sum(tp) AS tp, sum(fp) AS fp, sum(fn) AS fn
    FROM matches GROUP BY ALL
)
SELECT component, detector_version, count(*) AS n_subjects,
       sum(tp) / nullif(sum(tp) + sum(fp), 0)   AS precision,
       ratio_se(tp, tp + fp)                    AS se_precision_cluster,
       sum(tp) / nullif(sum(tp) + sum(fn), 0)   AS recall,
       ratio_se(tp, tp + fn)                    AS se_recall_cluster
FROM per_subject GROUP BY ALL ORDER BY component;
```

### M9.3 `unknown_rate`

| | |
|---|---|
| Business question | For what share of the time is each component (HP, MP, level, map, panels, objects) unknown, and why? This is the detector-gap backlog |
| Formula and denominator | Weighted `base_random` readings with `status = unknown` ÷ weighted base readings that were `observed` or `unknown`, per component (`not_visible` is reported apart: nothing to read). A share of in-view time (§4.2 `base_samples`). A second table breaks the readings down by status |
| Time window | Per `detector_version` and `game_build` |
| Eligible population | Base samples while the game was in view |
| Unit of independence | Subject |
| Label source | The component's own `status` |
| Missing data | This **is** the missing-data measure. Time out of view is coverage (M12.8), not "unknown" |
| Required precision | ±3 pp per component (proposed: a release gate in `SELF_IMPROVEMENT_LOOP.md`) |
| Allowed segmentations | component, reason, `detector_version`, `game_build`, `game_variant` (the regular MapleStory HUD versus the Classic World HUD), resolution class, platform |
| Consent | `improve_syrup` |
| Computable when | P1 |

```sql
-- M9.3 unknown_rate — NOT YET RUN
WITH per_subject AS (
    SELECT component, detector_version, game_variant, research_subject_id,
           coalesce(sum(w) FILTER (WHERE is_unknown), 0) AS y,
           sum(w)                                        AS n
    FROM v_observation WHERE sampling_policy = 'base_random' AND status <> 'not_visible'
    GROUP BY ALL
)
SELECT component, detector_version, game_variant, count(*) AS n_subjects, sum(n) AS samples_weighted,
       sum(y) / sum(n) AS unknown_share_of_time, ratio_se(y, n) AS se_cluster
FROM per_subject GROUP BY ALL ORDER BY unknown_share_of_time DESC;

SELECT component, status, count(*) AS samples, count(DISTINCT research_subject_id) AS n_subjects
FROM v_observation WHERE sampling_policy = 'base_random'
GROUP BY ALL ORDER BY component, samples DESC;
```

### M9.4 `detector_drift`

| | |
|---|---|
| Business question | Did a game patch (a new `game_build`) or a detector change (a new `detector_version`) change how often components are unknown, or (at P3) their accuracy? |
| Formula and denominator | The difference in M9.3's unknown share between consecutive builds at a fixed detector version (a patch effect), and between consecutive detector versions at a fixed build (a detector effect), with an approximate CI |
| Time window | Consecutive builds or versions |
| Eligible population | As M9.3 |
| Unit of independence | Subject. The two cells share subjects, so the independent-cells CI below is approximate, and a paired subject bootstrap is the reported CI |
| Label source | As M9.3; at P3, as M9.1 |
| Missing data | As M9.3 |
| Required precision | Flag a change larger than the pre-registered minimal important difference (proposed: 5 pp) whose CI excludes 0 |
| Allowed segmentations | component, `game_variant` |
| Consent | `improve_syrup` |
| Computable when | P1 (unknown share); P3 (accuracy) |

```sql
-- M9.4 detector_drift — NOT YET RUN (a patch effect: builds compared at a fixed detector version)
WITH per_subject AS (
    SELECT component, game_build, detector_version, research_subject_id,
           coalesce(sum(w) FILTER (WHERE is_unknown), 0) AS y,
           sum(w)                                        AS n
    FROM v_observation WHERE sampling_policy = 'base_random' AND status <> 'not_visible' GROUP BY ALL
),
cells AS (
    SELECT component, game_build, detector_version, count(*) AS n_subjects,
           sum(y) / sum(n) AS unknown_share, ratio_se(y, n) AS se
    FROM per_subject GROUP BY ALL
)
SELECT component, detector_version, game_build, n_subjects, unknown_share,
       unknown_share - lag(unknown_share) OVER w            AS delta_vs_previous_build,
       1.96 * sqrt(se * se + power(lag(se) OVER w, 2))      AS approx_half_width
FROM cells
WINDOW w AS (PARTITION BY component, detector_version ORDER BY game_build)
ORDER BY component, detector_version, game_build;
-- Builds must be ordered by their release order from the rights/reference tables. The lexical
-- ORDER BY game_build is a placeholder. For a detector effect, swap game_build and detector_version.
```

### M9.5 `tracking_error`

| | |
|---|---|
| Business question | When Syrup follows an object across frames, how often does it confuse identities, and how far off is its position? |
| Formula and denominator | ID switches per 100 gold-track seconds; the mean centre error (in normalised frame units) weighted by track seconds; both from the evaluation tool's per-track output |
| Time window | Per `detector_version` |
| Eligible population | Gold tracks in the test split |
| Unit of independence | Subject |
| Label source | `reviewed_gold` tracks |
| Missing data | Gold seconds the detector did not process are excluded and counted by the tool |
| Required precision | ±1 switch per 100 s (proposed) |
| Allowed segmentations | class, `detector_version`, frame rate class |
| Consent | `improve_syrup` |
| Computable when | P3 |

```sql
-- M9.5 tracking_error — NOT YET RUN; P3 (eval/tracks.jsonl)
WITH per_subject AS (
    SELECT component, research_subject_id, sum(id_switches) AS switches, sum(gold_seconds) AS seconds,
           sum(mean_center_error * gold_seconds) AS err_weighted
    FROM track_eval GROUP BY ALL
)
SELECT component, count(*) AS n_subjects, sum(seconds) AS gold_seconds,
       100.0 * sum(switches) / sum(seconds)      AS id_switches_per_100s,
       100.0 * ratio_se(switches, seconds)       AS se_cluster_per_100s,
       sum(err_weighted) / sum(seconds)          AS mean_center_error
FROM per_subject GROUP BY component ORDER BY component;
```

### M9.6 `calibration`

| | |
|---|---|
| Business question | When a component states a confidence, does it mean what it says? (Do readings at 0.8 turn out right 80% of the time?) |
| Formula and denominator | The expected calibration error over 10 equal-width bins: Σ (n_b / N) · \|accuracy_b − mean confidence_b\|. Also the Brier score. Computed only for components that emit a **calibrated** `confidence` |
| Time window | Per `detector_version` |
| Eligible population | Gold items with a `confidence` present |
| Unit of independence | Subject (a bootstrap over subjects for the CI) |
| Label source | `reviewed_gold` |
| Missing data | No `confidence` means "not calibrated" and gives **no number**. The raw `detector_score` may be used to *fit* a calibration, never reported as if it were one |
| Required precision | ECE CI half-width ≤ 0.02 (proposed) |
| Allowed segmentations | component, `detector_version` |
| Consent | `improve_syrup` |
| Computable when | P3, and only after a calibration exists (none does today) |

```sql
-- M9.6 calibration — NOT YET RUN; P3
WITH p AS (
    SELECT o.component, o.research_subject_id, o.confidence,
           CAST(o.value_text IS NOT DISTINCT FROM g.value_text AS INTEGER) AS correct
    FROM v_observation o JOIN labels g ON g.target_event_id = o.event_id
    WHERE g.label_source = 'reviewed_gold' AND g.split = 'test' AND o.confidence IS NOT NULL
),
bins AS (
    SELECT component, least(floor(confidence * 10), 9) AS bin,
           count(*) AS n, avg(confidence) AS mean_conf, avg(correct) AS accuracy
    FROM p GROUP BY ALL
),
ece AS (SELECT component, sum(n * abs(accuracy - mean_conf)) / sum(n) AS ece_10_bins FROM bins GROUP BY ALL)
SELECT p.component, count(DISTINCT p.research_subject_id) AS n_subjects, count(*) AS n_items,
       any_value(e.ece_10_bins) AS ece_10_bins,
       avg(power(p.confidence - p.correct, 2)) AS brier
FROM p JOIN ece e USING (component) GROUP BY p.component;
-- An empty result is the expected answer today: no component emits a calibrated confidence.
```

### M9.7 `component_latency`

| | |
|---|---|
| Business question | How long does each perception component take per frame on participants' machines (p50, p95, p99)? |
| Formula and denominator | Quantiles of `latency_ms` per component, over base samples. A per-subject p95 distribution is the robustness check, because machines differ |
| Time window | Per `detector_version` and `client_version` |
| Eligible population | Base-sampled observations that carry a latency |
| Unit of independence | Subject (machines). The pooled quantiles are descriptive |
| Label source | The client's own timing (the same spans as `vision_bench`; `DATA_ARCHITECTURE.md` §11) |
| Missing data | No latency = not measured, excluded and counted |
| Required precision | Descriptive; at least 20 subjects per platform |
| Allowed segmentations | component, platform, resolution class, `detector_version` |
| Consent | `improve_syrup` |
| Computable when | P1 |

```sql
-- M9.7 component_latency — NOT YET RUN
SELECT component, detector_version, platform,
       count(DISTINCT research_subject_id)                    AS n_subjects,
       count(*)                                               AS samples,
       quantile_cont(latency_ms, [0.5, 0.95, 0.99])           AS latency_ms_p50_p95_p99,
       count(*) FILTER (WHERE latency_ms IS NULL)             AS samples_without_latency
FROM v_observation
WHERE sampling_policy = 'base_random'
GROUP BY ALL ORDER BY component, platform;
```

---

## F10 Model quality and cost

Correctness is judged on a **random sample** of shown advice by reviewers with a rubric. It is never
judged only on advice somebody corrected, because that sample is selected on error. Inter-rater
agreement and the adjudication procedure are in `DATA_QUALITY_AND_BIAS.md`. Cost is computed from
token and audio counts times a **dated price table with its source** (a hypothesis input, A19). It is
never a guessed figure. Unknown cost is counted, not set to zero.

### M10.1 `grounded_correctness`

| | |
|---|---|
| Business question | How much of the advice is correct **and** grounded in what was on screen and in the cited sources? |
| Formula and denominator | Reviewed advice judged `grounded_correct` ÷ reviewed advice that could be assessed, per `model_version`. Reviewers see the observation and the sources the advice had, and nothing from later frames |
| Time window | Per `model_version` |
| Eligible population | A random sample of shown advice (known probability), in the test split |
| Unit of independence | Subject |
| Label source | `reviewed_gold` verdicts (`grounded_correct`, `ungrounded`, `incorrect`, `not_assessable`) |
| Missing data | `not_assessable` is excluded and counted, never scored |
| Required precision | ±5 pp per model version (proposed: a regression gate) |
| Allowed segmentations | `model_version`, advice kind, intent, constraint set |
| Consent | `improve_syrup`; `external_research_training` for a released eval (product C) |
| Computable when | P3 |

```sql
-- M10.1 grounded_correctness — NOT YET RUN; P3
WITH r AS (
    SELECT a.research_subject_id, a.model_version, g.value->>'verdict' AS verdict
    FROM v_assistant a JOIN labels g ON g.target_event_id = a.event_id
    WHERE a.phase = 'shown' AND g.label_source = 'reviewed_gold' AND g.split = 'test'
),
per_subject AS (
    SELECT model_version, research_subject_id,
           count(*) FILTER (WHERE verdict = 'grounded_correct') AS y,
           count(*) FILTER (WHERE verdict <> 'not_assessable')  AS n,
           count(*) FILTER (WHERE verdict = 'not_assessable')   AS n_not_assessable
    FROM r GROUP BY ALL
)
SELECT model_version, count(*) AS n_subjects, sum(n) AS assessed,
       sum(y) / nullif(sum(n), 0) AS grounded_correct_share, ratio_se(y, n) AS se_cluster,
       sum(n_not_assessable) AS not_assessable
FROM per_subject GROUP BY model_version ORDER BY model_version;
```

### M10.2 `confident_wrong_rate`

| | |
|---|---|
| Business question | How often is the assistant wrong while sounding sure? (The most damaging error for a player.) |
| Formula and denominator | Among reviewed advice delivered `assertive`: judged `incorrect` ÷ assessed. The same for `hedged`, for comparison. `assertiveness` is how the line was **delivered** (a property of the text), not a probability |
| Time window | Per `model_version` |
| Eligible population | As M10.1 |
| Unit of independence | Subject |
| Label source | `reviewed_gold` verdicts; `assertiveness` from the assistant event |
| Missing data | Unknown assertiveness is its own group |
| Required precision | ±3 pp on the assertive group (proposed) |
| Allowed segmentations | `model_version`, kind, intent |
| Consent | `improve_syrup`; `external_research_training` |
| Computable when | P3 |

```sql
-- M10.2 confident_wrong_rate — NOT YET RUN; P3
WITH r AS (
    SELECT a.research_subject_id, a.model_version, coalesce(a.assertiveness, 'unknown') AS assertiveness,
           g.value->>'verdict' AS verdict
    FROM v_assistant a JOIN labels g ON g.target_event_id = a.event_id
    WHERE a.phase = 'shown' AND g.label_source = 'reviewed_gold' AND g.split = 'test'
),
per_subject AS (
    SELECT model_version, assertiveness, research_subject_id,
           count(*) FILTER (WHERE verdict = 'incorrect')       AS y,
           count(*) FILTER (WHERE verdict <> 'not_assessable') AS n
    FROM r GROUP BY ALL
)
SELECT model_version, assertiveness, count(*) AS n_subjects, sum(n) AS assessed,
       sum(y) / nullif(sum(n), 0) AS incorrect_share, ratio_se(y, n) AS se_cluster
FROM per_subject GROUP BY ALL ORDER BY model_version, assertiveness;
```

### M10.3 `clarification_success`

| | |
|---|---|
| Business question | When the assistant asks a clarifying question, does it lead to an answer the participant accepts? |
| Formula and denominator | Clarifying questions shown → the next answer or hint shown in the same session. Success = that answer exists and drew no correction. Denominator: clarifying questions shown |
| Time window | Per `model_version` |
| Eligible population | `assistant` shown with `kind = clarifying_question` |
| Unit of independence | Subject |
| Label source | Syrup's own events; corrections by the player |
| Missing data | "Uncorrected" is not "correct": this is a **proxy**, labelled as such. The reviewed version uses M10.1's labels at P3 |
| Required precision | ±10 pp (proposed) |
| Allowed segmentations | `model_version`, intent of the original request |
| Consent | `improve_syrup` |
| Computable when | P1 |

```sql
-- M10.3 clarification_success — NOT YET RUN
WITH q AS (
    SELECT research_subject_id, session_id, sequence_no, advice_id
    FROM v_assistant WHERE phase = 'shown' AND kind = 'clarifying_question'
),
next_answer AS (
    SELECT q.advice_id AS question_id, min_by(a.advice_id, a.sequence_no) AS answer_id
    FROM q JOIN v_assistant a
      ON a.session_id = q.session_id AND a.sequence_no > q.sequence_no
     AND a.phase = 'shown' AND a.kind IN ('answer', 'hint')
    GROUP BY ALL
),
corrected AS (
    SELECT DISTINCT a.advice_id FROM v_correction c JOIN v_assistant a ON a.event_id = c.target_event_id
),
per_subject AS (
    SELECT q.research_subject_id,
           count(*)                                          AS n_questions,
           count(n.answer_id)                                AS n_answered,
           count(n.answer_id) FILTER (WHERE c.advice_id IS NULL) AS n_answered_uncorrected
    FROM q
    LEFT JOIN next_answer n ON n.question_id = q.advice_id
    LEFT JOIN corrected c   ON c.advice_id = n.answer_id
    GROUP BY ALL
)
SELECT count(*) AS n_subjects, sum(n_questions) AS questions,
       sum(n_answered) / sum(n_questions)              AS answered_share,
       sum(n_answered_uncorrected) / sum(n_questions)  AS clarification_success_proxy,
       ratio_se(n_answered_uncorrected, n_questions)   AS se_cluster
FROM per_subject;
```

### M10.4 `cost_per_completed_task`

| | |
|---|---|
| Business question | What does inference cost per task a participant completes, including the cost of tasks that were not completed? |
| Formula and denominator | Σ over all episodes of (tokens in × price in + tokens out × price out + audio minutes × audio price), with prices from a dated price table, ÷ episodes ending in `success`. Reported with the price table's date and source |
| Time window | Per `model_version`, at one price-table date |
| Eligible population | All episodes (rate-eligible) |
| Unit of independence | Subject (the CI is a subject bootstrap in the report tool) |
| Label source | Token and audio counts in `assistant` events (`produced`); the price table is an external, dated input |
| Missing data | A call with unknown counts or no price row has **unknown** cost. It is counted (`calls_cost_unknown`), and the total is then a lower bound, labelled so |
| Required precision | ±15% (proposed) |
| Allowed segmentations | `model_version`, provider, `task_key`, mode (text, voice, live call) |
| Consent | `service_operation` (cost of the service) and `improve_syrup` |
| Computable when | P2 (`reference/prices.csv`). P1 has counts but no prices |

```sql
-- M10.4 cost_per_completed_task — NOT YET RUN; P2 (reference/prices.csv: dated, sourced)
WITH price AS (SELECT * FROM read_csv('reference/prices.csv', header = true)),
costed AS (
    SELECT a.episode_id,
           a.tokens_in / 1000.0 * p.usd_per_1k_in
             + a.tokens_out / 1000.0 * p.usd_per_1k_out
             + a.audio_ms / 60000.0 * p.usd_per_audio_min AS usd     -- NULL when anything is unknown
    FROM v_assistant a
    LEFT JOIN price p ON p.provider = a.provider AND p.model_version = a.model_version
    WHERE a.rate_eligible
),
per_episode AS (
    SELECT e.research_subject_id, e.episode_id, e.outcome,
           sum(c.usd) AS usd, count(*) FILTER (WHERE c.episode_id IS NOT NULL AND c.usd IS NULL) AS calls_cost_unknown
    FROM ep e LEFT JOIN costed c ON c.episode_id = e.episode_id
    WHERE e.rate_eligible
    GROUP BY ALL
)
SELECT count(DISTINCT research_subject_id) AS n_subjects, count(*) AS episodes,
       count(*) FILTER (WHERE outcome = 'success') AS successful_episodes,
       sum(usd) / nullif(count(*) FILTER (WHERE outcome = 'success'), 0) AS usd_per_successful_episode,
       sum(calls_cost_unknown) AS calls_cost_unknown,           -- > 0: the figure is a lower bound
       (SELECT max(as_of) FROM price) AS price_table_as_of
FROM per_episode;
```

### M10.5 `latency_to_first_useful_answer`

| | |
|---|---|
| Business question | How long after asking does a participant get a first answer, and a first **useful** one? |
| Formula and denominator | From a help request to (a) the first answer shown; (b) the first answer that drew no correction and that the player rated helpful or said they followed (or a studio SDK reported followed). Quantiles p50 and p95. The share of requests where usefulness is known is reported beside (b) |
| Time window | Per `model_version` and `client_version` |
| Eligible population | Help requests (rate-eligible) |
| Unit of independence | Subject (the pooled quantiles are descriptive; per-subject medians as robustness) |
| Label source | Syrup's own events; the player's ratings and follow reports |
| Missing data | Usefulness unknown → excluded from (b) and counted. (b) is conditional on known usefulness and is labelled so |
| Required precision | ±0.5 s on the p50 of (a) (proposed) |
| Allowed segmentations | `model_version`, provider, channel, intent, mode |
| Consent | `improve_syrup` |
| Computable when | P1 |

```sql
-- M10.5 latency_to_first_useful_answer — NOT YET RUN
WITH req AS (
    SELECT research_subject_id, help_id, t_ms AS asked_ms FROM v_help
    WHERE phase = 'requested' AND rate_eligible
),
ans AS (SELECT advice_id, in_reply_to AS help_id, shown_at_ms AS shown_ms
        FROM v_assistant WHERE phase = 'shown' AND in_reply_to IS NOT NULL),
useful AS (
    SELECT target_advice_id AS advice_id FROM v_feedback
    WHERE kind = 'helpful' AND source_type = 'human_asserted'
    UNION
    SELECT linked_advice_id FROM v_action
    WHERE source_type IN ('human_asserted', 'publisher_ground_truth') AND linked_advice_id IS NOT NULL
),
corrected AS (SELECT DISTINCT a.advice_id FROM v_correction c JOIN v_assistant a ON a.event_id = c.target_event_id),
first_any AS (SELECT help_id, min(shown_ms) AS first_ms FROM ans GROUP BY ALL),
first_useful AS (
    SELECT a.help_id, min(a.shown_ms) AS useful_ms
    FROM ans a JOIN useful u USING (advice_id) LEFT JOIN corrected c USING (advice_id)
    WHERE c.advice_id IS NULL GROUP BY ALL
)
SELECT count(DISTINCT r.research_subject_id) AS n_subjects, count(*) AS requests,
       quantile_cont((f.first_ms - r.asked_ms) / 1000.0, [0.5, 0.95])  AS first_answer_s_p50_p95,
       count(u.useful_ms) / count(*)                                    AS usefulness_known_share,
       quantile_cont((u.useful_ms - r.asked_ms) / 1000.0, [0.5, 0.95]) AS first_useful_s_p50_p95_when_known
FROM req r LEFT JOIN first_any f USING (help_id) LEFT JOIN first_useful u USING (help_id);
```

---

## F11 In-game economy friction

Only **in-game** resources the system actually saw on screen (mesos, potions, a required item), and
only as friction in a task. **Prohibited:** inferring personal income, ability or willingness to pay,
spender or "whale" profiles; linking to real-money purchases; predicting spending; and any optimisation
that exploits a weakness or pushes spending (directive §5.11, §11). These metrics feed aggregate
studio friction reports and Syrup's advice. They never feed targeting.

### M11.1 `resource_shortfall_failure_rate`

| | |
|---|---|
| Business question | Which tasks fail because the participant lacked an in-game resource the task needed? |
| Formula and denominator | Weighted attempts that failed with `failure_type = missing_resource`, where the missing resource was **observed** (`missing_resource` not NULL) ÷ weighted attempts with an observed outcome. Per task and resource kind |
| Time window | Per `game_build` (economies change with patches) |
| Eligible population | Attempts with rate-eligible sampling |
| Unit of independence | Subject |
| Label source | The outcome's `failure_type` with its `source_type`; the resource observation (`direct_observation`) |
| Missing data | A shortfall the screen did not show is not counted: the figure is a lower bound, labelled so. Unobserved outcomes as in §2.3 |
| Required precision | ±5 pp per task (proposed) |
| Allowed segmentations | `task_key`, resource kind (`currency`, `consumable`, `item`), `game_build`, level band. **Never** by any spending or payment attribute |
| Consent | `improve_syrup`; `aggregate_analytics` with rights |
| Computable when | P1 once the `missing_resource` capability exists (§4.2). Until then the share is NULL, never 0 |

```sql
-- M11.1 resource_shortfall_failure_rate — NOT YET RUN
WITH per_subject AS (
    SELECT task_key, research_subject_id,
           coalesce(sum(w)
                    FILTER (WHERE status = 'failure' AND failure_type = 'missing_resource'
                              AND missing_resource IS NOT NULL), 0)                       AS y,
           coalesce(sum(w)
                    FILTER (WHERE status IN ('success', 'failure', 'aborted')), 0)       AS n
    FROM attempts WHERE rate_eligible
    GROUP BY ALL
)
SELECT task_key, count(*) AS n_subjects, sum(n) AS observed_attempts_weighted,
       CASE WHEN has('missing_resource') THEN sum(y) / nullif(sum(n), 0) END AS shortfall_failure_share_lower_bound,
       CASE WHEN has('missing_resource') THEN ratio_se(y, n) END             AS se_cluster
FROM per_subject GROUP BY task_key ORDER BY task_key;

SELECT missing_resource, count(*) AS failures, count(DISTINCT research_subject_id) AS n_subjects
FROM attempts WHERE failure_type = 'missing_resource' AND missing_resource IS NOT NULL
GROUP BY ALL ORDER BY failures DESC;
```

### M11.2 `time_to_acquire_resource`

| | |
|---|---|
| Business question | Once a participant is observed short of a needed in-game resource, how much active play does it take until they are observed to have enough? |
| Formula and denominator | From the first observation `resource.<kind>` with `sufficient = false` to the first later observation in the same session with `sufficient = true`. One shortfall per subject and kind. Kaplan–Meier median, censored at the last observation of that resource |
| Time window | Per `game_build`; within one session (no cross-session durations, §2.9) |
| Eligible population | Subjects with an observed shortfall |
| Unit of independence | Subject |
| Label source | `resource.*` observations (`direct_observation`) |
| Missing data | Not observed sufficient = censored. Resources never shown on screen are out of scope, not zero |
| Required precision | ±25% of the median (proposed) |
| Allowed segmentations | resource kind, `task_key`, `game_build` |
| Consent | `improve_syrup`; `aggregate_analytics` with rights |
| Computable when | P1 once the `resource_sufficiency` capability exists (§4.2). Until then the query returns no rows |

```sql
-- M11.2 time_to_acquire_resource — NOT YET RUN
WITH r AS (
    SELECT research_subject_id, session_id, component, sequence_no, t_ms,
           CAST(value->>'sufficient' AS BOOLEAN) AS sufficient
    FROM v_observation WHERE component LIKE 'resource.%' AND value IS NOT NULL
),
short AS (
    SELECT * FROM r WHERE sufficient = FALSE
    QUALIFY row_number() OVER (PARTITION BY research_subject_id, component
                               ORDER BY session_id, sequence_no) = 1
),
got AS (
    SELECT s.research_subject_id, s.component, s.t_ms AS short_ms,
           min(r.t_ms) FILTER (WHERE r.sufficient) AS got_ms,
           max(r.t_ms)                              AS last_seen_ms
    FROM short s LEFT JOIN r
      ON r.session_id = s.session_id AND r.component = s.component AND r.sequence_no > s.sequence_no
    GROUP BY ALL
),
km_in AS (
    SELECT component, (coalesce(got_ms, last_seen_ms, short_ms) - short_ms) / 60000.0 AS t_min,
           CAST(got_ms IS NOT NULL AS INTEGER) AS acquired
    FROM got
),
km AS (SELECT component, t_min, sum(acquired) AS d, count(*) AS leaving FROM km_in GROUP BY ALL),
surv AS (
    SELECT *, product(1 - d / at_risk) OVER (PARTITION BY component ORDER BY t_min) AS s_t
    FROM (SELECT *, sum(leaving) OVER (PARTITION BY component ORDER BY t_min DESC) AS at_risk FROM km)
)
SELECT component, sum(leaving) AS n_subjects, sum(d) AS n_acquired,
       min(t_min) FILTER (WHERE s_t <= 0.5) AS median_active_minutes_to_acquire     -- NULL = not reached
FROM surv GROUP BY component ORDER BY component;
```

---

## F12 Data health

The program's own quality, rights and cost, measured in units a customer receives (§14, §15). The
consent ledger is separate from the research store (`DATA_ARCHITECTURE.md` §6). The research export
holds only consented rows, so **consent rates cannot come from it**. They come from aggregate tallies
in the consent ledger, which hold no subject ids. The P2 queries read the catalog read-only through
DuckDB's Postgres attachment (A19).

### M12.1 `consent_rate`

| | |
|---|---|
| Business question | When the consent offer for a purpose is shown, how often is it granted? How many subjects hold an active grant per purpose? |
| Formula and denominator | Per purpose and consent-text version: granted ÷ offers shown, from aggregate tallies. Plus subjects with an active (not withdrawn) grant per purpose. Offers are counted per showing, not per person, and the rate is labelled "per offer" |
| Time window | Per consent-text version (a new text is a new denominator), by week |
| Eligible population | Adults who reached the offer (age assurance comes first, `CONSENT_AND_RIGHTS.md`) |
| Unit of independence | None at the person level, because the tallies hold no ids by design. Descriptive only |
| Label source | The consent ledger (receipts and tallies) |
| Missing data | An offer with no decision (closed) counts as shown, not declined, and is reported separately |
| Required precision | Descriptive |
| Allowed segmentations | purpose, text version, language of the text, client version |
| Consent | The ledger is kept under `service_operation` (proof of consent), with its own minimal policy. It is not a back door for telemetry (§4) |
| Computable when | P2 (P1: the export manifest's consent summary, which has no offers) |

```sql
-- M12.1 consent_rate — NOT YET RUN; P2
SELECT o.purpose, t.version, t.language,
       sum(o.shown) AS offers_shown, sum(o.granted) AS granted, sum(o.declined) AS declined,
       sum(o.shown) - sum(o.granted) - sum(o.declined) AS closed_without_decision,
       sum(o.granted) / nullif(sum(o.shown), 0) AS grant_rate_per_offer
FROM cat.consent.offer_tallies o
JOIN cat.consent.consent_texts t
  ON t.text_id = o.text_id AND t.version = o.text_version AND t.language = o.text_language
GROUP BY ALL ORDER BY o.purpose, t.version;

SELECT purpose, count(DISTINCT research_subject_id) AS subjects_with_active_grant
FROM cat.consent.current_grants GROUP BY purpose ORDER BY purpose;
```

### M12.2 `rights_eligible_hours`

| | |
|---|---|
| Business question | How many hours (and episodes) may be used **today** for each use (`capture`, `store`, `annotate`, `train_internal`, `train_external`, `evaluate`, `transfer`), per purpose and recipient class? |
| Formula and denominator | Σ episode hours (first attempt's start to the outcome) of episodes whose rights policy is `approved`, valid now, not revoked, and allows the use ÷ Σ episode hours. Per game and use. In-view hours come from `observed_time` per session, and the P2 catalog keeps them per episode |
| Time window | As of now (rights expire and can be revoked) |
| Eligible population | All episodes in the catalog |
| Unit of independence | Not a sample estimate: an inventory |
| Label source | `rights.rights_policies` and `rights.rights_grants` (`migrations/0001_research_catalog.sql`, mirroring `consent.rs` `RightsManifest`; `CONSENT_AND_RIGHTS.md`) |
| Missing data | No grant → `no_grant` and not eligible. A policy under review (`requires_title_specific_review`) or `denied` is not eligible |
| Required precision | Exact counts |
| Allowed segmentations | game, variant, build, use, territory |
| Consent | Consent is a separate gate; this metric is about rights only. Eligible to **sell** = rights ∧ consent ∧ QA (M12.6) |
| Computable when | P2. On the synthetic P1 export every row carries the synthetic policy (local demonstration only), so eligible hours for any external use are **0 by construction**, and that is the correct answer |

```sql
-- M12.2 rights_eligible_hours — NOT YET RUN; P2
WITH pol AS (   -- one row per granted (purpose, recipient class, use); no row = not granted
    SELECT p.rights_policy_id, g.purpose, g.recipient_class, g.use,
           p.status = 'approved' AND (p.revoked_at IS NULL OR p.revoked_at > now())
             AND now() >= p.valid_from AND now() < p.valid_until AS eligible
    FROM cat.rights.rights_policies p
    JOIN cat.rights.rights_grants g ON g.rights_policy_id = p.rights_policy_id
)
SELECT e.game_id, coalesce(p.use, 'no_grant') AS use, p.purpose, p.recipient_class,
       count(DISTINCT e.research_subject_id)                                AS n_subjects,
       count(*)                                                             AS episodes,
       sum(e.duration_ms) / 3600000.0                                       AS episode_hours,
       coalesce(sum(e.duration_ms) FILTER (WHERE p.eligible), 0) / 3600000.0 AS eligible_episode_hours,
       count(*) FILTER (WHERE p.eligible)                                   AS eligible_episodes
FROM ep e LEFT JOIN pol p ON p.rights_policy_id = e.rights_policy_id
GROUP BY ALL ORDER BY e.game_id, use;
```

### M12.3 `qa_pass_rate`

| | |
|---|---|
| Business question | What share of what arrives passes validation and quality checks, and why is the rest held or rejected? |
| Formula and denominator | Events accepted ÷ events received, per week, from the ingest ledger. Episodes by QA status. Open quarantine items by reason, with their age and expiry (the quarantine is bounded) |
| Time window | Weekly |
| Eligible population | Everything received |
| Unit of independence | Not a sample estimate: an operational count |
| Label source | The ingest service's validation and quality checks (`DATA_ARCHITECTURE.md` §5) |
| Missing data | A batch without counts is a defect of the ingest service and is reported as such |
| Required precision | Exact counts |
| Allowed segmentations | reason, schema version, client version |
| Consent | `service_operation` |
| Computable when | P2 (P1: the exporter's own rejection counts in its report) |

```sql
-- M12.3 qa_pass_rate — NOT YET RUN; P2
SELECT date_trunc('week', received_at) AS week, count(*) AS batches,
       sum(events_received) AS received, sum(events_accepted) AS accepted,
       sum(events_duplicate) AS duplicates, sum(events_quarantined) AS quarantined,
       sum(events_rejected) AS rejected,
       sum(events_accepted) / nullif(sum(events_received), 0) AS accepted_share
FROM cat.research.ingest_batches GROUP BY ALL ORDER BY week;

SELECT qa_status, count(*) AS episodes FROM cat.research.episodes GROUP BY qa_status;

SELECT reason, count(*) AS items, min(quarantined_at) AS oldest, min(expires_at) AS next_expiry
FROM cat.research.quarantine WHERE resolved_at IS NULL GROUP BY reason ORDER BY items DESC;
```

### M12.4 `duplicate_rate`

| | |
|---|---|
| Business question | How many events arrive more than once (retries, replays, a second copy of the app)? |
| Formula and denominator | 1 − distinct `event_id` ÷ events. On the P1 export this is expected to be 0, because the episode builder already deduplicates. The meaningful figure is at ingest (`ingest_batches.events_duplicate`, M12.3). Content duplicates (the same subject, task and outcome within a minute under different ids) are a second check |
| Time window | Per export, or weekly at ingest |
| Eligible population | All events |
| Unit of independence | An operational count |
| Label source | `event_id` |
| Missing data | An event with no `event_id` violates the contract: counted, never accepted |
| Required precision | Exact counts |
| Allowed segmentations | client version, schema version |
| Consent | `service_operation` |
| Computable when | P1 (the file); P2 (ingest) |

```sql
-- M12.4 duplicate_rate — NOT YET RUN
SELECT count(*) AS events_in_file, count(DISTINCT event_id) AS distinct_events,
       1 - count(DISTINCT event_id) / count(*) AS duplicate_share,
       count(*) FILTER (WHERE event_id IS NULL) AS events_without_id          -- must be 0
FROM ev_raw;

-- Content duplicates: two episodes of one subject and task ending the same way within a minute.
SELECT count(*) AS suspected_content_duplicates
FROM ep a JOIN ep b
  ON a.research_subject_id = b.research_subject_id AND a.session_id = b.session_id
 AND a.task_key = b.task_key AND a.outcome = b.outcome
 AND a.episode_id < b.episode_id AND abs(a.end_ms - b.end_ms) <= 60000;
```

### M12.5 `coverage_and_concentration`

| | |
|---|---|
| Business question | How many distinct people, sessions, tasks, builds and languages does the data cover, and does a handful of participants dominate it? |
| Formula and denominator | Counts of unique subjects, sessions, episodes, observed hours, task keys, builds and locales. Concentration: the episode share of the top 5% of subjects, and the Herfindahl index of episodes by subject. Coverage matrix: (task × build) cells with ≥20 subjects |
| Time window | Per export or release |
| Eligible population | The export or dataset |
| Unit of independence | Subject |
| Label source | The export itself |
| Missing data | Unknown task or build is its own row in the matrix |
| Required precision | Exact counts |
| Allowed segmentations | game, build, task, locale |
| Consent | `improve_syrup`; the counts go on every data card (§14) |
| Computable when | P1 |

```sql
-- M12.5 coverage_and_concentration — NOT YET RUN
WITH per_subject AS (SELECT research_subject_id, count(*) AS episodes FROM ep GROUP BY ALL),
ranked AS (SELECT *, row_number() OVER (ORDER BY episodes DESC) AS r, count(*) OVER () AS m FROM per_subject)
SELECT (SELECT count(DISTINCT research_subject_id) FROM ep)  AS n_subjects,
       (SELECT count(DISTINCT session_id) FROM ep)           AS n_sessions,
       (SELECT count(*) FROM ep)                             AS n_episodes,
       (SELECT sum(observed_ms) / 3600000.0 FROM observed_time) AS observed_hours,
       (SELECT count(DISTINCT task_key) FROM ep)             AS n_task_keys,
       (SELECT count(DISTINCT game_build) FROM ep)           AS n_game_builds,
       (SELECT count(DISTINCT locale) FROM ep)               AS n_locales,
       sum(episodes) FILTER (WHERE r <= greatest(1, ceil(m * 0.05))) / sum(episodes) AS top_5pct_subjects_share,
       sum(episodes * episodes) / (sum(episodes) * sum(episodes))                    AS hhi_by_subject
FROM ranked;

SELECT coalesce(task_key, 'unknown') AS task_key, coalesce(game_build, 'unknown') AS game_build,
       count(DISTINCT research_subject_id) AS n_subjects, count(*) AS n_episodes,
       count(DISTINCT research_subject_id) >= 20 AS reportable
FROM ep GROUP BY ALL ORDER BY task_key, game_build;
```

### M12.6 `cost_per_approved_unit`

| | |
|---|---|
| Business question | What does one **approved** hour, episode or label cost, all costs included? (The unit economics are per unit the customer receives, not per GB; `UNIT_ECONOMICS.md`.) |
| Formula and denominator | Σ costs for the period (inference, storage and egress, labelling, recruitment and incentives, support, legal, security, sales; from a finance input with sources) ÷ approved units in the period. Approved means QA passed, not deleted, and, per purpose, rights ∧ consent eligible |
| Time window | Monthly |
| Eligible population | The catalog's episodes |
| Unit of independence | An inventory and ledger figure |
| Label source | The finance input (`reference/costs.csv`), the catalog |
| Missing data | A cost category with no entry is listed as missing. It is not taken as zero |
| Required precision | Exact arithmetic on stated inputs. The inputs carry their own uncertainty |
| Allowed segmentations | purpose, game, cost category |
| Consent | Not personal data |
| Computable when | P2 |

```sql
-- M12.6 cost_per_approved_unit — NOT YET RUN; P2 (reference/costs.csv: period, category, usd, source)
WITH costs AS (SELECT period, category, usd FROM read_csv('reference/costs.csv', header = true)),
approved AS (
    SELECT strftime(collection_date, '%Y-%m') AS period, count(*) AS approved_episodes,
           sum(observed_ms) / 3600000.0 AS approved_hours
    FROM cat.research.episodes
    WHERE qa_status = 'accepted'                        -- deleted episodes are gone (§9)
    GROUP BY ALL
)
SELECT a.period, a.approved_episodes, a.approved_hours,
       sum(c.usd)                                      AS total_usd,
       count(DISTINCT c.category)                      AS cost_categories_present,   -- compare with the 8 expected
       sum(c.usd) / nullif(a.approved_hours, 0)        AS usd_per_approved_hour,
       sum(c.usd) / nullif(a.approved_episodes, 0)     AS usd_per_approved_episode
FROM approved a LEFT JOIN costs c ON c.period = a.period
GROUP BY a.period, a.approved_episodes, a.approved_hours ORDER BY a.period;
```

### M12.7 `deletion_reach`

| | |
|---|---|
| Business question | Are deletion requests completed, how fast, and what did each one reach? For one participant: which datasets, exports, customers and training runs hold their rows? (The tenth query of §14.) |
| Formula and denominator | Requests by status; the median time to completion; the oldest open request. For one subject: every artifact in the deletion index (`delivery.subject_presence`), with its exports and the training runs that consumed it |
| Time window | Continuous; reported weekly |
| Eligible population | All deletion requests |
| Unit of independence | An operational count |
| Label source | The deletion ledger and the lineage tables |
| Missing data | An artifact without lineage is a defect: a dataset whose membership is unknown cannot be shipped (the export gate refuses it) |
| Required precision | Exact |
| Allowed segmentations | status, artifact kind |
| Consent | `service_operation` (the minimal proof of handling a request, `CONSENT_AND_RIGHTS.md`) |
| Computable when | P2 (P1: the slice's deletion test lists the affected exports, `DATA_CONTRACTS.md`) |

```sql
-- M12.7 deletion_reach — NOT YET RUN; P2
SELECT status, count(*) AS requests,
       median(epoch(completed_at) - epoch(requested_at)) / 3600.0                  AS median_hours_to_complete,
       max(epoch(now()) - epoch(requested_at)) FILTER (WHERE completed_at IS NULL) / 3600.0 AS oldest_open_hours
FROM cat.deletion.deletion_requests GROUP BY status;

-- One subject (a pseudonymous id, never a name):  SET VARIABLE subject = '<research_subject_id>';
SELECT a.artifact_kind, a.artifact_id, a.dataset_id, x.export_id, x.customer_id, x.delivered_at,
       t.training_ref, t.consumed_at
FROM cat.delivery.subject_presence p
JOIN cat.delivery.artifacts a                   ON a.artifact_id = p.artifact_id
LEFT JOIN cat.delivery.exports x                ON x.dataset_id = a.dataset_id
LEFT JOIN cat.delivery.training_consumption t   ON t.dataset_id = a.dataset_id
WHERE p.research_subject_id = getvariable('subject')
ORDER BY a.artifact_kind, a.artifact_id;
```

### M12.8 `sampling_integrity`

| | |
|---|---|
| Business question | Did capture and sampling behave as declared: dropped frames, frames rejected by the privacy filter, degraded intervals, participants at their cap, clock offsets, and realised versus declared base-sample probability? |
| Formula and denominator | Schema 0.1.0 records capture problems as `capture_quality` events with a `status` (`ok`, `degraded`, `focus_lost`, `not_in_view`, `login_screen`, `detection_failed`, `frames_dropped`, `clock_drift`), `dropped_frames` and `clock_offset_ms`. Outputs: events per status per observed hour; dropped frames per observed hour; p95 of \|clock offset\|; the declared sampling policies and probabilities. The ratios in `DATA_ARCHITECTURE.md` §3.4 (dropped ÷ processed, privacy-rejected ÷ processed, realised ÷ declared base rate, subjects at cap) need counters 0.1.0 does not have (frames processed, frames rejected by the privacy filter, base slots, caps): proposed for `DATA_CONTRACTS.md` |
| Time window | Per `client_version` and platform |
| Eligible population | All capture-quality events and sessions |
| Unit of independence | Subject |
| Label source | The client's own capture status (`DATA_ARCHITECTURE.md` §3) |
| Missing data | A NULL `dropped_frames` or `clock_offset_ms` is unknown, not 0, and is counted |
| Required precision | Realised versus declared probability within ±10% relative once measurable (proposed). A larger gap means rates weighted by the declared probability are biased, and they are flagged |
| Allowed segmentations | platform, `client_version`, status |
| Consent | `improve_syrup` |
| Computable when | P1 (status, drops, offsets); P2 (the ratios, which need the counters above) |

```sql
-- M12.8 sampling_integrity — NOT YET RUN
WITH hours AS (SELECT sum(observed_ms) / 3600000.0 AS h FROM observed_time)
SELECT c.platform, c.client_version, c.capture_status,
       count(DISTINCT c.research_subject_id)                               AS n_subjects,
       count(*)                                                            AS events,
       count(*) / any_value(hours.h)                                       AS events_per_observed_hour,
       sum(c.frames_dropped) / any_value(hours.h)                          AS dropped_frames_per_observed_hour,
       count(*) FILTER (WHERE c.frames_dropped IS NULL)                    AS dropped_frames_unknown,
       quantile_cont(abs(c.clock_offset_ms), 0.95)                         AS abs_clock_offset_ms_p95,
       count(*) FILTER (WHERE c.clock_offset_ms IS NULL)                   AS clock_offset_unknown
FROM v_capture c, hours
GROUP BY c.platform, c.client_version, c.capture_status
ORDER BY c.platform, c.client_version, events DESC;

SELECT sampling_policy, sampling_probability, count(*) AS events,
       count(DISTINCT research_subject_id) AS n_subjects
FROM ev GROUP BY ALL ORDER BY sampling_policy, sampling_probability;
```

---

## 18. The ten queries of §14, mapped

The directive (§14) asks for ten real queries with checks, and a dashboard showing data age, the
denominator and participant counts (§2.8). Each check below is what the query must show on the
**synthetic** P1 export, given what its fixtures contain (`DATA_CONTRACTS.md`; the generator is w46's).
The manager runs them, and nothing here has been run.

| # | Question (§14) | Metrics | Check on the synthetic export |
|---|---|---|---|
| 1 | Where do beginners get stuck? | M1.1, M1.4, M1.5 by `task_key` | The fixture's unobserved ending is **censored** (`n_censored` ≥ 1 in M1.1, present in M1.5) and never a failure |
| 2 | Success before and after help | M1.2, M4.1 | `bound_low` ≤ `success_rate_observed` ≤ `bound_high` in every row of M1.2; the funnel's shares never exceed 1 |
| 3 | Frequent corrections | M6.1, M6.2 | The poisoned correction stays `claimed`: it is in `claims` and absent from `verified` |
| 4 | Language and UI coverage | M8.1, M8.2, M12.5 | `unknown` languages form their own row; no column holds an inferred trait |
| 5 | Change after a patch | M9.4; M1.x by `game_build` | With one synthetic build, `delta_vs_previous_build` is NULL ("no previous build"), not 0 |
| 6 | Detector gaps | M9.3 | `unknown` stays NULL: `SELECT count(*) FROM v_observation WHERE status <> 'observed' AND value IS NOT NULL` returns 0 (no stand-in 0 for an unread or hidden component) |
| 7 | Recovery cases | M7.1, M7.3 | The failure → correction → success fixture gives `recovered_share_of_known` > 0 |
| 8 | Training-eligible hours | M12.2 | `eligible_hours` = 0 for every external use (the synthetic policy is local-only), and no MapleStory or MapleStory Worlds row is eligible |
| 9 | Cost per approved episode | M12.6, M10.4 | On P1 it is not computable (no finance input, no price table): the query fails with "file not found". It does not print a made-up figure |
| 10 | Everything affected by deleting one participant | M12.7 | After the slice deletes a synthetic subject: 0 rows for that subject in `ev` and `ep`, and the affected-exports list equals the one the slice reports |

Two further checks hold across all ten:
- **Dedup:** the fixture's duplicate event counts once (`ev` has one row per `event_id`).
- **Ordering:** the out-of-order event yields the same attempts as the in-order version (attempts are
  built from `sequence_no`, not from file order).

## 19. What I could not verify

1. **No query has been run or even parsed.** DuckDB is not installed in the environment this was written
   in, and pip had no network. Every query is marked "not yet run". Constructs worth confirming first
   when the manager runs the prelude:
   - `read_ndjson_objects(...) AS t(j)` with the `->` and `->>` JSON operators;
   - aggregates inside the `ratio_se` macro;
   - `product()` as a window aggregate;
   - `CAST(json AS VARCHAR[])`;
   - qualified column references after `JOIN … USING`;
   - `getvariable()` (DuckDB ≥ 1.1);
   - the Postgres attachment reading the catalog's domain-typed columns (`vocab.*`) as text.
2. **The field mapping was reconciled against w46's code as it stood at about 10:35** (§4.1, schema
   0.1.0), before `DATA_CONTRACTS.md` existed. If the contract changes, edit the prelude only. The 18
   capabilities 0.1.0 lacks (§4.2) are proposals, and the metrics that need them return NULL until they exist.
3. **The precision targets, the 20-subject minimum, the 60 s reopen window, the 10-minute repetition window,
   the 20% unobserved flag and the 30% response flag are proposals**, not derived from data. The ICC that
   drives sample sizes is unknown until a pilot measures it.
4. **Sources were not read.** The fetch permission was not granted in this session, so the directive's
   sources (Unity Analytics events, RLDS, the Data Cards Playbook, and others) are unverified here.
   Nothing in this catalog depends on what they say.
5. **The catalog tables F12 reads** exist as DDL (`migrations/0001_research_catalog.sql`). That DDL was
   applied once to an empty, throwaway PostgreSQL 16 cluster with a synthetic smoke script (see its
   header). None of the DuckDB queries against it has been run.
6. **P2 and P3 inputs do not exist yet:** labels and gold, the evaluation tool's outputs, the licensed
   reference graph, the price table, the finance input and the catalog database. The queries that need
   them are written against the designed shapes (A19, `migrations/0001_research_catalog.sql`).

