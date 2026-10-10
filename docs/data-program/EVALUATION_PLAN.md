# EVALUATION_PLAN — proving utility, or publishing that there is none

<div dir="rtl" lang="he">

**בקצרה לבעלים:** איך מוכיחים שנתוני Syrup משפרים מודל, ולא רק טוענים את זה.
ניסוי בשלושה תנאים עם אותו מודל, אותה חלוקה ואותו compute: (1) בלי תוספת; (2) תוספת מקרית בנפח ובעלות דומים; (3) תוספת Syrup שנבחרה ותויגה. רק אם (3) עדיף על (2), עם רווח סמך שאינו כולל אפס, יש תועלת למכור.
שתי שאלות נבדקות בנפרד: שיפור **אימון** (הנתונים) ושיפור **בזמן ריצה** (Syrup מספק state למודל).
מודדים נכונות מבוססת-מסך, הזיות, כיול ואי-ודאות, התאוששות, ציות לאילוצים, תזמון עזרה, עלות ו-latency. מריצים כמה seeds, learning curves ו-ablations: בלי מטרה, בלי תיקונים, בלי מקור פעולות.
benchmark פרטי עם gold שלא נכנס לאימון, תכנית ריענון ותקציב גישה. מבחן קליפים offline נפרד לגמרי מהצלחת משימה אינטראקטיבית, שדורשת sandbox מורשה.
תוצאה שלילית מתפרסמת. שום דבר כאן עוד לא רץ.

</div>

> **Nothing here has been run.** There is no episode to train on yet (CURRENT_STATE_AUDIT §6), no gold set, no
> evaluation result. This is the protocol P3 runs ([IMPLEMENTATION_PLAN.md](IMPLEMENTATION_PLAN.md)), and the
> evidence the "no improvement" kill criterion of [UNIT_ECONOMICS.md](UNIT_ECONOMICS.md) reads. Metric
> definitions for the product (task completion, help usefulness, vision quality, data health) live in
> [METRICS_CATALOG.md](METRICS_CATALOG.md); splits, gold labelling, annotator agreement and bias in
> [DATA_QUALITY_AND_BIAS.md](DATA_QUALITY_AND_BIAS.md). This file defines what is specific to proving utility.

## 1. Two questions, never mixed

| | Training improvement | Runtime state |
|---|---|---|
| Question | Does *adding Syrup data to training* make a model better? | Does *giving a model Syrup's observed state at inference* make its answers better? |
| What varies | the training data (§2's three conditions) | the inference input: no state / Syrup's state / gold ("oracle") state / Syrup's state with injected errors |
| What is held fixed | model, initialisation, compute, hyperparameter budget, test set | model weights, prompt, decoding, test set |
| What it can sell | Products B and C (data, evaluation) | the Syrup SDK/API and the companion itself |
| Today's analogue | none | MapleSyrup already sends a text snapshot of the game and the frame with every reply (CURRENT_STATE_AUDIT §4.1) — the runtime question about the product as it is |

A result on one question is never quoted as evidence for the other.

## 2. The three-condition replication (training improvement)

Same base model, same initialisation, same train/validation/test split, same evaluation:

1. **Baseline** — the base training set, no addition.
2. **Random added data** — the same base plus data drawn **uniformly at random** from the same consented,
   rights-cleared pool (not failure-driven, not selected), matched to condition 3 twice:
   **2a volume-matched** (same hours / episodes) and **2b cost-matched** (same labelling and collection
   budget, per [UNIT_ECONOMICS.md](UNIT_ECONOMICS.md)). Selected data costs more per hour to label; without 2b
   a win could be bought by spending more.
3. **Syrup-selected and labelled data** — the same base plus episodes chosen by Syrup's sampling (failures,
   corrections, help requests, recoveries, constraints) with their labels.

**The claim that can be sold:** 3 beats 2b on the pre-registered primary outcome, with a 95% confidence
interval (participant-clustered, §5) whose lower bound is above zero. "3 beats 1" alone proves only that more
data helps.

**Fair compute.** Same training steps or FLOPs per condition (or report curves to equal compute); the same
hyperparameter-search budget, tuned on validation only; the same prompts and decoding at test time.

**Publish negative results.** If 3 does not beat 2b, that result goes into the evaluation report and the data
card as prominently as a positive one would, and the "no improvement" kill criterion fires for that claim.

## 3. No future leakage

- A model that is meant to act or advise at time *t* gets only what was observable at or before *t*. Outcomes,
  later frames and later corrections are **labels**, never inputs.
- Actions inferred from video are non-causal labels: OpenAI's VPT notes its inverse dynamics model "can use past
  and future information to guess the action at each step" (fetched). Such labels carry `model_inferred`
  provenance, may be training targets, and are never inputs for a real-time model under evaluation — and never
  presented as recorded input (directive §2 B).
- Splits by participant and by source sequence **before** clips are cut; no visual near-duplicates across
  splits (perceptual hashes); temporal, build/patch and unseen-game holdouts in addition to the random holdout
  (rules in [DATA_QUALITY_AND_BIAS.md](DATA_QUALITY_AND_BIAS.md)).
- The gold test set (§7) never enters any training pool, ours or a buyer's.

## 4. What is measured

Each metric names its denominator. Scores are reported per capability, never as one blended number.

| Metric | Definition (unit: one answer unless stated) | Notes |
|---|---|---|
| **Grounded correctness** | share of answers whose every factual claim about the screen or game state is supported by the gold state at time *t* and that address the request; also claim-level: supported claims ÷ checkable claims | denominator: requests with a gold state; claims extracted by annotators, or by a model with human review of a sample (provenance kept) |
| **Hallucination** | share of answers with ≥ 1 claim *contradicted* by the gold state or sources; reported separately: ≥ 1 *unsupported* claim | "unsupported" is not "false": kept apart |
| **Calibration and uncertainty** | for stated confidences: expected calibration error and Brier score; risk–coverage curve for abstention; "confidently wrong" rate (confidence ≥ an agreed threshold and wrong); clarification asked on items labelled ambiguous | applies to Syrup's own detectors too: today `Detection.confidence` is a heuristic score, not a calibrated probability (CURRENT_STATE_AUDIT §2) |
| **Recovery** | among episodes with a failure: offline, advice that names the failure type and a strategy change consistent with gold; in the sandbox, recovery rate and time to recovery | a pair of similar attempts is not a counterfactual (directive §5.7) |
| **Task success, where measurable** | offline: only outcome *prediction* against the observed outcome; true task success only in the interactive sandbox (§6) | an observed outcome after advice is observational, not causal |
| **Constraint compliance** | share of answers violating a stated constraint ("no spoilers", "hint only", "I want to find it myself") | human-judged against the constraint text |
| **Help timing** | precision and recall of choosing to intervene at moments gold-labelled as "help wanted", from participants' own signals ("this helped", "don't interrupt") | preference is a separate label from in-game outcome (directive §6) |
| **Cost** | USD per 1,000 evaluated items and per correct answer; tokens | from metered usage, not list prices |
| **Latency** | p50 / p95 time to the first useful output | measured on stated hardware or endpoint |

## 5. Statistics

- **Pre-register** before running: the primary outcome (proposed: answer-level grounded correctness on the
  private gold test, condition 3 minus 2b), the minimum difference that matters, the sample size, the analysis,
  the secondary outcomes, and the stopping rule. Everything not pre-registered is reported as exploratory.
- **Unit of independence: the participant.** Items from one participant are correlated; confidence intervals
  come from a cluster bootstrap over participants (or a mixed model with a participant effect). Conditions are
  compared **paired** on the same test items.
- **Sample size** for a paired comparison: n_items ≈ (z₁₋α/₂ + z₁₋β)² · σ_d² / Δ² · DEFF, with
  DEFF = 1 + (m − 1) · ICC for m items per participant. σ_d and the ICC are unknown until a pilot measures them;
  no number is assumed here. A cell without the power to detect Δ is reported as **insufficient evidence**.
- **Seeds:** at least 3 training seeds per condition when training is stochastic (5 preferred); report the mean
  and the spread, and treat seed variance as part of the uncertainty. For sampled model outputs, several samples
  per item or deterministic decoding, stated.
- **Multiple comparisons:** many metrics × slices invite a false win. One primary outcome decides; secondary
  outcomes use Holm correction or are labelled exploratory. No victory declared after searching hundreds of
  metrics (directive §11).

## 6. Offline clips and the interactive sandbox — separate products, separate claims

| | Offline clip benchmark | Interactive sandbox |
|---|---|---|
| Input | frames, Syrup state and the request, up to time *t* | a running game build in which an agent acts, or a human follows the assistant |
| Measures | perception, grounding, advice quality, calibration, constraint compliance, outcome prediction | task success, recovery, time to success |
| Where | our servers, on licensed clips | a **licensed** sandbox only: a partner's test build with a publisher-provided interface, or an environment we own |
| Never | read as permission or proof to act in game accounts | connected to MapleSyrup's observational product, or to a live game account |

MapleSyrup itself never acts in a game (CURRENT_STATE_AUDIT §3). Any interactive evaluation lives in separate
code, separate binaries and separate accounts, under the sandbox's own licence.

## 7. The private gold benchmark

- **Construction.** Items from participants held out of every training pool (participant-level holdout), from
  titles whose rights manifest covers evaluation use and display to the buyer; double-labelled with
  adjudication ([DATA_QUALITY_AND_BIAS.md](DATA_QUALITY_AND_BIAS.md)). Each item: observable inputs up to *t*,
  the request and any constraint, the gold state, gold answer criteria, metadata (game, build, locale,
  difficulty), and its provenance.
- **Kept out of training.** A registry of exact and perceptual hashes checked by every training-data export;
  a canary marker in item files; buyers contractually barred from training on items or outputs; no per-item
  feedback beyond a small fixed public sample.
- **Refresh plan.** New items from each new game build or patch; items retired once exposed beyond the access
  budget; a frozen anchor subset kept so scores stay comparable across refreshes; every report names the
  benchmark version it used. Refresh cadence: set with the first buyer (no number assumed).
- **Access budget.** Per buyer per period: a fixed number of evaluation runs; aggregate scores only; rate
  limits; every run in an audit log; repeated near-identical submissions treated as probing.

## 8. Learning curves and ablations

- **Learning curves:** conditions 2 and 3 at 10%, 25%, 50% and 100% of the added data. A claim needs 3 to
  dominate 2 across the curve, not at one point; no extrapolation beyond the largest point measured.
- **Ablations** of condition 3 — which part carries the value:
  - **no goal** — the stated goal and constraint fields removed;
  - **no corrections** — correction events and correction-derived labels removed;
  - **no action source** — verified actions removed (or replaced by `model_inferred` ones), to price the cost
    of not having a verified action source;
  - **no help requests** — help and assistant events removed;
  - for the runtime question: **no state**, **noisy state**, **oracle state** (§1).
- An ablation that costs nothing shows that component is not what a buyer pays for — that goes into the data
  card too.

## 9. The evaluation report a buyer receives

Benchmark and dataset versions; conditions and their matching (volume and cost); compute per condition;
seeds; the pre-registration and every deviation from it; primary outcome with its clustered interval; secondary
and exploratory results, labelled; learning curves; ablations; slices with counts of participants, tasks,
sessions and hours (never only frames), "insufficient evidence" marked; cost and latency; known limitations,
including opt-in and companion-user selection bias ([DATA_QUALITY_AND_BIAS.md](DATA_QUALITY_AND_BIAS.md)); and
the negative results.
