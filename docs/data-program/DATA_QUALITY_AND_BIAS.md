# DATA_QUALITY_AND_BIAS — labels, splits, bias and honest statistics

<div dir="rtl" lang="he">

**בקצרה, למיכאל**

- שלושה מקורות תיוג נשמרים בנפרד ולא דורסים זה את זה: תיוג אוטומטי, תיקוני משתמש ו-gold שנבדק בידי מתייגים. תחזית של מודל לא הופכת ל-gold, ותיקון של שחקן אחד לא משנה שום דבר לכולם.
- מודדים את הדיוק של כל מתייג (פריטי gold נסתרים) ואת ההסכמה בין מתייגים, ויש הליך הכרעה מתועד. מטפלים ברעש, בכפילויות, במשתתפים שמציפים, בשינויי ממשק אחרי patch, בהרעלה ובתמריצים שמייצרים תיוג גרוע: לא משלמים לפי תיקון, לפי שעת משחק או לפי כישלון.
- החלוקה ל-train/validation/test נעשית לפי משתתף ולפי רצף מקור, לפני החיתוך ל-clips, בלי חפיפה חזותית, ועם מבחנים נפרדים לזמן, לגרסה ולמשחק שלא נראו. סופרים משתתפים, משימות, מפגשים ושעות, לא פריימים.
- ההטיות נאמרות בגלוי: מי שבוחר בעוזר קולי, מי שמסכים לתרום, כיסוי חלקי, ועזרה שמתבקשת דווקא כשקשה. התוצאות מתארות פאנל משתתפים, לא את כל שחקני המשחק.
- בדוחות: רווחי סמך שמביאים בחשבון את התלות בין נתונים של אותו שחקן, ״insufficient evidence״ כשאין מספיק משתתפים, ותוצאה ראשית עם שינוי מינימלי חשוב שנקבעים מראש. A/B רק בין שתי דרכי עזרה סבירות, בהסכמה ועם ניתוח intention-to-treat, ובלי למנוע עזרה שהשחקן ביקש.

</div>

> **Status (2026-10-10).** P0 design. **Nothing here has been measured**: there is no program data yet. Every
> threshold (qualification scores, agreement cut-offs, caps, sample shares) is a **proposal** to be tuned in
> a pilot, not a finding. P1 test names come from `tests/research_slice.rs` as w46 is writing it; I have not
> run them.
>
> This document owns label quality, splits, bias and the statistical protocol. The per-metric rules for
> population, missing data, sampling weights, precision and the "insufficient evidence" display are in
> [METRICS_CATALOG](METRICS_CATALOG.md) §2 and are not repeated here. The evaluation design (the
> three-condition replication, the private benchmark) is in [EVALUATION_PLAN](EVALUATION_PLAN.md). The
> components that run these checks are in [DATA_ARCHITECTURE](DATA_ARCHITECTURE.md). The field names are in
> [DATA_CONTRACTS](DATA_CONTRACTS.md).

---

## 1. Three label sources, kept apart

| Source | `source_type` ([DATA_CONTRACTS](DATA_CONTRACTS.md)) | Who makes it | Example | May be used for | Never |
|---|---|---|---|---|---|
| **Automatic** | `direct_observation` (a detector reading pixels) or `model_inferred` (a model's judgement) | a detector or a model, with its version | "HP 1,291 / 1,351", read by the font reader; "the player is fighting a boss", inferred by a model | training signal with a known noise rate; candidates for review; features | gold; ground truth for evaluation |
| **User correction** | `human_asserted` | the participant | "that's wrong: that bar is MP, not HP" | a claim in the review queue; the participant's own local knowledge; promotion by the rule of §3.5 | gold without review; a global change on its own |
| **Reviewed gold** | `human_reviewed`, after adjudication (§2.5) | trained annotators and an adjudicator | the HUD in this frame shows HP 1,291 / 1,351, verified against the frame | ground truth for evaluation; calibration; checking annotators | training, when it belongs to the held-out benchmark ([EVALUATION_PLAN](EVALUATION_PLAN.md)) |

Two more `source_type` values sit outside the three: `publisher_ground_truth` (from a licensed SDK or replay;
only with a partner studio, and the strongest source) and `synthetic` (never mixed with real data in any
metric). Anything else is `unknown`.

Rules:

1. **Every label keeps its source.** A later label never overwrites an earlier one. The original perception,
   the model's prediction and the later correction are kept side by side with lineage (directive §7).
2. **No automatic promotion.** `model_inferred` never becomes `human_reviewed` or gold (P1 test
   `model_inferred_never_becomes_human_reviewed_or_gold`). `human_asserted` becomes `human_reviewed` only
   through review.
3. **"Gold" is a status**: a label on an item in a *versioned* gold set, under a named guideline version.
4. **Even gold has an error rate**, and it is measured (§2). An OCR reading or a player's confirmation is not
   absolute truth (directive §7).
5. **Agreement between an automatic label and a user's label is not verification.** Both can be fooled by
   the same misleading screen.

---

## 2. Annotators: accuracy, agreement, adjudication

### 2.1 Guidelines

One versioned annotation guide per task, with examples, counter-examples and explicit `ambiguous` and
`unanswerable` options (the frame does not show it). Every label records its `guideline_version`.

### 2.2 Qualification

Before labeling for real, an annotator passes a qualification set drawn from gold. Proposals: at least 90%
on the task's main label; for text transcription, a character error rate of at most 1%.

### 2.3 Accuracy over time

- Hidden gold items are mixed into every annotator's queue (proposal: 5–10%).
- Accuracy is tracked per annotator and per task over a rolling window.
- Below the threshold: retraining or removal, and their labels since their last good window are reviewed
  again.

### 2.4 Agreement

A share of production items is labeled twice, independently (proposal: 10–20%), and every gold-set
candidate is.

| Kind of label | Agreement measure |
|---|---|
| categorical (correction category, outcome) | Cohen's κ for two annotators; Krippendorff's α for more, or when labels are missing (preferred: it handles both) |
| ordinal (a satisfaction scale) | weighted κ, or Krippendorff's α for ordinal data |
| text (an OCR transcription) | character error rate between annotators; exact-match rate |
| boxes and regions (objects, interface elements) | IoU; share matched at IoU ≥ 0.5 |
| time boundaries (start and end of an attempt, time of an outcome) | share within a tolerance (proposal: ±1 s) |

- **Raw agreement is reported next to κ or α.** With rare classes, κ is low even when annotators almost
  always agree (the prevalence effect).
- **Cut-offs (proposal, following a common convention for α):** at least 0.80 for use as gold; 0.67 to 0.80
  is "tentative" and is not used to evaluate anything; below 0.67 the guideline is reworked.

### 2.5 Adjudication

1. When two labels disagree, or a label disagrees with a hidden gold item, the item goes to an adjudicator: a
   senior annotator who did not label it, and who does not see who said what.
2. The adjudicator chooses a label, `ambiguous` or `unanswerable`, with a reason code.
3. **All original labels are kept** with the decision. Nothing is overwritten.
4. Systematic disagreement (a category below the cut-off) leads to a revised guideline under a new version.
   The affected items are labeled again; the old labels stay, under their old version.
5. Every gold item has two independent labels and an adjudication.

### 2.6 Working conditions

Annotators are paid per hour or per batch, with quality gates; never per item at speed. They get realistic
time budgets and see sanitized items only ([THREAT_MODEL](THREAT_MODEL.md) T13).

---

## 3. Noise, duplicates, floods, interface changes, poisoning, incentives

### 3.1 Noise

- The label noise of each source is estimated against gold (a confusion matrix per source) and appears in
  every release's quality report.
- Training can weight sources by their measured noise, or use noise-aware methods; the Model Evaluation role
  decides.
- Items where a model and gold disagree go to review first: they are the most informative.

### 3.2 Duplicates

- **Exact**: a hash of the normalised event payload, and a content hash for every frame.
- **Near**: a perceptual hash on frames; overlapping sequences from retries and re-uploads; the same episode
  exported twice.
- **Deduplicate before splitting** (§4), and report the duplicate rate (the data-health family in
  [METRICS_CATALOG](METRICS_CATALOG.md)). P1: duplicate events, by `event_id`, build the same episodes (test
  `duplicate_and_out_of_order_events_build_the_same_episodes`).

### 3.3 Participants who flood

- Caps per participant and period during collection (DATA_ARCHITECTURE, per-participant caps) and again in
  every dataset. Proposal: no participant above 2% of a training set's episodes, or above a fixed number of
  episodes per task.
- Metrics are computed per participant first ([METRICS_CATALOG](METRICS_CATALOG.md) §2.2).
- Report the concentration that remains: the share held by the 10 largest contributors.

### 3.4 Interface changes and game updates

- Every record carries `game_build`, `detector_version` and `model_version`. A new build starts a new stratum.
- Drift monitors per build: the `unknown` rate, the distribution of detector scores, and the OCR error on a
  labeled gold sample ([METRICS_CATALOG](METRICS_CATALOG.md) F9).
- After a patch, a canary sample from the new build is labeled before automatic labels on it are trusted.
- Never pool data across an interface change without stratifying by build.

### 3.5 Poisoning: malicious or careless corrections

- **A correction is a claim**, stored with its evidence reference, and never applied globally on its own
  (directive §11; P1 test `a_poisoned_correction_is_kept_as_a_claim_not_applied`).
- **Promotion to shared knowledge or labels** (proposal) needs all of:
  1. agreement from at least *m* independent participants (proposal: *m* ≥ 3: distinct subjects and distinct
     sessions, not minutes apart);
  2. a reviewer's confirmation against the evidence (the frame, the game version), or publisher ground truth;
  3. a rollout through shadow and canary, with rollback ([SELF_IMPROVEMENT_LOOP](SELF_IMPROVEMENT_LOOP.md)).
- **Reliability comes from agreement with gold**, never from popularity or volume.
- **Detection**: bursts of similar corrections, corrections that contradict gold, accounts created together,
  identical wording.
- A rate limit per participant on corrections entering the review queue.
- A correction that contains instructions is `untrusted_text` ([THREAT_MODEL](THREAT_MODEL.md) T2).
- **Independence is judged without collecting more personal data**: no network fingerprinting for this
  purpose. Use subject ids, session timing, and in the paid panel the payout-verified identity.

### 3.6 Incentives that produce unreliable labels

| Incentive | What it produces | Instead |
|---|---|---|
| pay per correction | spam corrections | pay per study block; corrections unpaid |
| pay per hour of play | idle hours; excessive play | pay per study block, with caps |
| pay per failure or rare event | staged failures and events | recruit for the conditions (product D), never pay for outcomes |
| telling participants which events are "valuable" | play performed for the dataset | say what the study is about, as consent requires, but not which events count more |
| leaderboards for contributors | volume over truth | none |
| annotators paid per item | speed over accuracy | pay per time, with quality gates |

Participants must know they are in a study and what it is about (directive §2, product D). Telling them the
study's topic is required. Telling them which events earn more is not, and would bias the data.

---

## 4. Splits: by participant and source sequence, before any clip

### 4.1 Order of operations

1. Deduplicate (§3.2).
2. Assign every participant to train, validation or test by a salted hash of `research_subject_id`. The
   assignment is deterministic for a dataset release, and the salt is recorded with it.
3. **Only then** cut episodes into clips or windows.

Never split at the level of clips or frames: neighbouring frames of one session would land on both sides.

### 4.2 Source sequences stay whole

Every clip from one session or recording goes with its participant. A sequence that cannot be tied to one
participant goes, whole, to one split.

### 4.3 No visual overlap

A perceptual-hash check across splits. Overlapping windows from one sequence never cross a split. The same
map background in two splits is inherent to a game and acceptable; the same frames are not.

### 4.4 Held-out tests

Each reported separately from the in-distribution test set:

- **time**: episodes after a cut-off date;
- **version**: a game build absent from training;
- **game**: a title absent from training;
- **interface**: a UI language, a screen-size class, a HUD variant (modern or Classic World) absent from
  training.

### 4.5 The gold benchmark

The private benchmark never enters training ([EVALUATION_PLAN](EVALUATION_PLAN.md): refreshed, with an access
budget).

### 4.6 Automated leakage checks for every release

- no subject in two splits;
- no near-duplicate frame across splits;
- no later observation used as input to an earlier decision (P1 test
  `no_later_observation_is_an_input_to_earlier_advice`);
- no label derived from an outcome used as an input at decision time;
- no test or benchmark item in any training manifest (comparison of hash lists).

---

## 5. What every dataset and report counts

| Count | Why |
|---|---|
| unique participants | the unit of independence ([METRICS_CATALOG](METRICS_CATALOG.md) §2.2) |
| tasks (distinct task keys) | what the data covers |
| episodes and sessions | volume, at the level that has meaning |
| observed hours; eligible and approved hours per purpose | what can be used, and for what ([CONSENT_AND_RIGHTS](CONSENT_AND_RIGHTS.md) §2) |
| the share held by the 10 largest contributors | concentration (§3.3) |
| coverage by game build, HUD variant, UI language, screen class, task, outcome (including `unobserved` and `censored`) | where the data is thin |
| rejections and quarantines, by reason | what was lost, and why |
| frames | **only as a secondary count, never as the headline** |

Counts are given per split and per segment. Output from synthetic data is marked SYNTHETIC
([METRICS_CATALOG](METRICS_CATALOG.md) §2.8).

---

## 6. Bias and coverage

Nothing below has been measured. These are the biases to expect, to state as limitations and to measure
where it is possible without collecting more.

### 6.1 Who uses a companion

Players who install a voice AI companion for MapleStory need a Windows PC, in practice a phone for the voice
link, today their own OpenAI key and its cost (launch plan [2.5](../launch-plan/tasks/2.5-monetization.md): about one or two
dollars per hour of play), an interface in English or Hebrew (the phone page's languages), and the wish to
talk to an assistant. They are likely more engaged than the average player. The opening of Classic World
(October 2026) brings a wave of returning players: a cohort effect in anything collected around it.

### 6.2 Who opts in

Contributors differ from non-contributors: in privacy attitudes, engagement and, by design, age (adults only).
We do not collect non-contributors' behaviour, so the direction of this bias is unknown and cannot be
corrected from our data. The opt-in rate itself is reported only if it can be computed without new
collection; otherwise it is reported as unknown.

### 6.3 Partial coverage

- The companion sees the game only while its window is in front. Outcomes can be `unobserved` or `censored`
  ([METRICS_CATALOG](METRICS_CATALOG.md) §2.3).
- Event-triggered samples are not a picture of normal play; rates use the random base sample only
  ([METRICS_CATALOG](METRICS_CATALOG.md) §2.4).
- The detectors cover part of the interface. Today the companion reads the HUD; the other detectors feed
  only the preview ([CURRENT_STATE_AUDIT](CURRENT_STATE_AUDIT.md) §2.1).
- Per-participant caps change what the data represents, on purpose.

### 6.4 Assisted versus unassisted

Help is asked for when a player is struggling (confounding by need), so assisted attempts are harder ones.
Players who never ask differ from those who do. A naive comparison of assisted and unassisted success can
show help "hurting". Descriptive comparisons are stratified by task and by previous attempts. Causal claims
come only from randomized comparisons (§8).

### 6.5 Survivorship

Those who keep using Syrup are those it works for. A trend over time partly reflects who stayed.

### 6.6 Platform

Windows only; certain screen sizes; the modern HUD and the Classic World HUD.

### 6.7 Synthetic data

Never mixed with real data in a metric; always labeled.

### 6.8 What every report states

The panel's size, how it was recruited, the period, the consent tracks it came from, the adults-only rule,
the coverage table (§5), the biases above and what was not measured. Results describe the panel, not the
game's players ([METRICS_CATALOG](METRICS_CATALOG.md) §2.1).

### 6.9 No reweighting to "all players"

Post-stratification needs the population's margins, which we do not have (the publisher might). Without
them, figures are reported unweighted, with the panel described.

---

## 7. Statistics in reports

This section complements [METRICS_CATALOG](METRICS_CATALOG.md) §2.2–§2.5 (the participant as the unit, the
display rule of at least 20 subjects and a target half-width, the sample-size formula with the design effect).

### 7.1 Choosing the method for dependence within a participant

| Situation | Method |
|---|---|
| a rate or a mean, many participants | cluster-robust (sandwich) standard errors by participant; the ratio estimator ([METRICS_CATALOG](METRICS_CATALOG.md) `ratio_se`) |
| few participants (rule of thumb: fewer than about 40) | cluster-robust errors are too small with few clusters: use a bootstrap that resamples participants (or a wild cluster bootstrap) with *t* on G − 1 degrees of freedom, or report "insufficient evidence" |
| repeated measures over time per participant (learning curves, [METRICS_CATALOG](METRICS_CATALOG.md) F3) | a hierarchical (mixed-effects) model with a random intercept, and a random slope where needed, per participant |
| time to an event, with censoring | Kaplan–Meier with confidence intervals from a participant bootstrap; Cox models with robust variance |
| comparing builds or versions | stratify by task; participant-clustered errors, or a mixed model with task effects |

The ICC behind the design effect is measured in the pilot. Until then, sample sizes rest on stated
assumptions, as in [METRICS_CATALOG](METRICS_CATALOG.md) §2.5.

### 7.2 Insufficient evidence

- Descriptive cells follow [METRICS_CATALOG](METRICS_CATALOG.md) §2.5.
- For comparisons and experiments: if the planned sample gives less than 80% power to detect the minimal
  important difference, the result is "insufficient evidence".
- An underpowered test never yields "no effect". Report the confidence interval instead.
- **Every segment needs its own check.** An experiment powered for the whole panel is usually underpowered
  for its segments. Subgroup effects are either pre-registered with their own sample size, or labelled
  exploratory.
- Privacy protections cost power: suppression and differential-privacy noise
  ([THREAT_MODEL](THREAT_MODEL.md) §6.6) shrink the effective sample, and the plan counts them.

### 7.3 Pre-registration, before any experiment or claimed change

Written, versioned in the repository and timestamped **before the data are seen**:

1. the question;
2. **one primary outcome**, by its metric id in [METRICS_CATALOG](METRICS_CATALOG.md), with its denominator;
3. **the minimal important difference**, with the decision it would change;
4. the population and eligibility;
5. the unit of randomization and of analysis;
6. the sample size and how it was computed, including the ICC assumed;
7. the analysis: intention-to-treat, clustered errors;
8. missing and unobserved outcomes: the bounds of [METRICS_CATALOG](METRICS_CATALOG.md) §2.3;
9. secondary outcomes, labelled exploratory;
10. guardrails (§8.2);
11. stopping rules: a fixed horizon, or group-sequential boundaries with alpha spending. No peeking;
12. the correction for multiple comparisons: Holm for a small family of confirmatory outcomes;
    Benjamini–Hochberg for an exploratory screen.

### 7.4 Many metrics, one claim

No winner is declared after a search through hundreds of metrics (directive §11). A finding from an
exploratory screen is a hypothesis for a new, pre-registered test.

---

## 8. Association, causation and experiments

### 8.1 What observational data cannot say

- The link between help and success is confounded by need, skill, task difficulty, game version and time of
  day.
- Regression to the mean: help is asked for at a low point, and things often improve anyway.
- Selection into asking for help (§6.4).
- **A pair of similar attempts is not a counterfactual** (directive §5). Player actions and videos alone
  cannot tell what would have happened in another world (directive §11).
- Causal wording is kept for randomized comparisons analysed by intention-to-treat
  ([METRICS_CATALOG](METRICS_CATALOG.md) §2.8). Everything else is association.

### 8.2 A/B tests: the rules

- **Only between reasonable ways to help**: a hint or a full answer (when the player has not asked for "hint
  only"); a shorter or a longer reply; an earlier or a later warning within safe bounds; two phrasings.
- **Never "help versus no help" when help was asked for.** Never weaken a safety warning below the product's
  standard.
- **Consent**: experiments run only with participants who granted `improve_syrup`, whose text says that Syrup
  sometimes compares two reasonable ways of helping; anything beyond that needs a specific consent. Whether
  that notice is enough is a question for counsel ([CONSENT_AND_RIGHTS](CONSENT_AND_RIGHTS.md) §7).
- **Documented assignment**: the participant is the unit of randomization (a crossover design only if
  pre-registered). The allocation probability is recorded in `experiment` events
  ([DATA_CONTRACTS](DATA_CONTRACTS.md)), and the assignment code and its seed are versioned.
- **Intention-to-treat**: analysed by the assigned arm, whether or not the help was shown, heard or followed
  (the help funnel, [METRICS_CATALOG](METRICS_CATALOG.md) F4, tells those apart). Per-protocol analysis is
  secondary only.
- **Guardrails**: interruptions, mutes and cancellations, explicit dislikes. A guardrail that worsens past a
  pre-set margin stops the test.
- **Never optimise for addiction, play time or spending** (directive §11). Long play is not success
  (directive §5).
- **Game-design experiments only with the studio** (directive §11).

### 8.3 What observational analysis can still do

Descriptive figures with their denominators; comparisons stratified by task; "natural experiments", such as
a patch that changes an interface, read as before and after against a comparison group and with caution. All
of it is labelled association.

---

## 9. Quality gates in the pipeline

The components are in [DATA_ARCHITECTURE](DATA_ARCHITECTURE.md); the checks are:

- **At ingest**: schema validation; value ranges; the provenance fields present; `unknown` kept as missing,
  never 0; clock checks; sequence gaps. Failures go to quarantine, which is bounded
  ([CONSENT_AND_RIGHTS](CONSENT_AND_RIGHTS.md) §10).
- **In curation**: deduplication (§3.2); caps (§3.3); a sanitization check on a sample
  ([THREAT_MODEL](THREAT_MODEL.md) §9.1); label agreement against the cut-offs (§2.4); drift per build (§3.4).
- **At release**, the quality report (directive §14) states: the counts (§5); coverage and bias (§6); the
  label sources and their agreement (§1, §2); the noise estimates (§3.1); the duplicate rate; rejections by
  reason; `unknown` rates; the leakage checks passed (§4.6); the known limitations.

---

## 10. The acceptance tests that belong here (directive §17)

| Row | P1 test (w46; not run by me) | Later |
|---|---|---|
| `unknown` never becomes 0 | `unknown_never_becomes_zero` | schema validation at ingest |
| model inference never becomes gold | `model_inferred_never_becomes_human_reviewed_or_gold` | the labeling tool |
| a poisoned correction | `a_poisoned_correction_is_kept_as_a_claim_not_applied` | the promotion rule (§3.5) |
| duplicates and out-of-order events | `duplicate_and_out_of_order_events_build_the_same_episodes` | idempotent ingest |
| train/test leakage | `no_later_observation_is_an_input_to_earlier_advice` (no future input) | the splitter's tests in P3: no subject in two splits, no near-duplicate across splits, no benchmark item in training (§4.6) |
| a lost window or Syrup closing is not a failure | `a_window_loss_or_syrup_closing_is_not_a_failure` | — |
