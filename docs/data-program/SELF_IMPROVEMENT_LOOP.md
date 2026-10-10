# SELF_IMPROVEMENT_LOOP — from a failure a player saw to a fix every player feels

<div dir="rtl" lang="he">

**בקצרה לבעלים:** הלולאה של §13: כישלון → מועמד לדגימה → הסכמה וזכויות → טיהור → תיוג ואימות → regression fixture או ידע מתועד → עדכון detector או מודל → בדיקה על holdout → shadow/canary → rollback.
חלק גדול מהלולאה כבר קיים בקוד, בעבודת יד: ה-sight הנלמד שואל את המודל-המורה רק כשהזיהוי הדטרמיניסטי נכשל, שורה שלא נלמדת שומרת את התמונה שלה, תיקונים נשמרים ב-knowledge.json, וה-probes של היומיים האחרונים הפכו לבדיקות regression בריפו.
מה שחסר: שער הסכמה וזכויות בין כישלון מקומי לשיפור משותף, מקור/גרסה/תוקף לידע גלובלי, כיול אמיתי לביטחון, holdout קבוע, shadow/canary ו-rollback לפי איכות.
מתחילים בספים וכיול, templates, זיהוי UI, retrieval ותזמון/אורך עזרה. בלי לאמן מודל ענק.
מודדים KPI שהשחקן מרגיש (אזעקות שווא, תיקונים לשעה, זמן לתשובה מועילה, עלות לשעה), לא "נאספו עוד נתונים".
העדפה אישית נשארת אישית. תיקון של שחקן אחד לא הופך מיד לידע של כולם.

</div>

> **Scope.** This loop improves Syrup and MapleSyrup themselves, under the separate purpose `improve_syrup`
> ([CONSENT_AND_RIGHTS.md](CONSENT_AND_RIGHTS.md) §1) —
> not the commercial purposes of [B2B_PRODUCT_STRATEGY.md](B2B_PRODUCT_STRATEGY.md). Nothing here uploads
> anything today: there is no upload path (CURRENT_STATE_AUDIT §2.1, §4.1), and none may be added without the
> purpose-based consent of [CONSENT_AND_RIGHTS.md](CONSENT_AND_RIGHTS.md). Players who contribute nothing keep
> every local fix (directive §3). Evidence below cites the audited commit (03303a9) or this branch; "WORKS" means
> passing tests on fixtures, never "verified on the live game" (CURRENT_STATE_AUDIT §2).

## 1. Two loops, kept apart

| | Local loop (every player) | Shared loop (contributors only) |
|---|---|---|
| Where | on the player's PC | the program's servers, behind the export gate |
| Learns | *this* player's screen, HUD font, taught things, corrections, preferences | what generalises: thresholds, templates, UI detection, retrieval, help timing |
| Needs | nothing beyond using the product | the "improve Syrup" purpose granted, with a receipt; a rights basis for the title |
| Exists today | **yes, in large part** (§3) | no |
| Rule | a personal preference stays personal; "remember this" is never permission to publish (directive §3) | one player's correction never becomes global knowledge on its own (directive §11) |

## 2. The loop, stage by stage — what exists, what is missing

| Stage | What exists today (evidence) | Missing |
|---|---|---|
| **1. Failure** | Detected in several places: numbers and bars that keep disagreeing make the sight read the HUD again; no bars for a while, or an unlabelable font, call the teacher model, with back-off (`src/sight/mod.rs` header; teacher in `src/sight/teacher.rs`); a reading the detector only guessed at is not an observation (`a_bar_the_detector_only_guessed_at_is_not_an_observation`, `src/companion/observation.rs`); the player's "that's wrong" through `correct_reading` (`src/ai/tools.rs`); a taught object's unexplained near miss (`src/sight/things.rs`). Recent fixes were each a failure class: "a cursor resting on a bar is no reading" (03303a9), "a sliver read from the bar's fill is no reading — no false deaths" (e74aeb7), "a parked cursor is no death" (40ef45b). | A structured failure event (directive §4 `correction`, `capture_quality`, `outcome` families — [DATA_CONTRACTS.md](DATA_CONTRACTS.md)) linked to the observation and advice it concerns: today corrections carry a date only and no link (CURRENT_STATE_AUDIT §6). |
| **2. Sampling candidate** | Local candidates are already kept: "a line that will not learn says the most telling reason and keeps its picture" (`src/sight/numbers.rs`, e2db1f5); things.rs keeps "a near miss nobody can account for … as a candidate for the vision model to judge". | Sampling with known inclusion probability, per-participant caps, and a random baseline so the failure rate is not distorted by sampling only failures (directive §8; [DATA_ARCHITECTURE.md](DATA_ARCHITECTURE.md)). |
| **3. Consent and rights** | Nothing between the local loop and any shared use. Sharing is not offered (a351a9f). | The purpose check against a receipt and the title's rights manifest, at the moment of selection and again at use ([CONSENT_AND_RIGHTS.md](CONSENT_AND_RIGHTS.md)). |
| **4. Sanitization** | Window-only capture of the game while it is in front (CURRENT_STATE_AUDIT §4.3); the stats keep no free text (09274f8). | On-PC removal of chat, names, notifications and credentials before anything is selected; frames not provably clean are never uploaded; in-game text treated as untrusted data, never instructions ([THREAT_MODEL.md](THREAT_MODEL.md)). |
| **5. Label and verify** | The teacher's answers are "strict JSON (structured outputs), checked here before anything is believed" (`src/sight/teacher.rs` header); the bar cross-checks the number. | Three separate sources — automatic label, player correction, reviewed gold — with annotator type and verification status (directive §11; [DATA_QUALITY_AND_BIAS.md](DATA_QUALITY_AND_BIAS.md)). A player's correction is `human_asserted`, not truth. |
| **6. Regression fixture or documented knowledge** | **Fixtures:** the probes of the last two days found rows that were then pinned as a test "in the repo, not only in a probe" (b28cdfb → `hebrew_openers_places_and_decimals_do_not_save_a_recital`, `src/ai/brain.rs`), which fails on the previous filter; HUD fixtures in `tests/hud_accuracy.rs`, `tests/classic_hud.rs`, `tests/golden/maplestory.json`; `examples/evening.rs` replays a scripted evening "to read instead of imagine". **Knowledge:** `knowledge.json` keeps look-ups and corrections; "a correction replaces what it corrects and wins" (`src/ai/knowledge.rs`), sources `Web` or `Player`. | A fixture store for contributed material that is **not this public repository** (access-controlled, with lineage to consent and rights, deletable). Global knowledge with a **source, game version and expiry** (directive §13); today an entry has `from` and a date, no version, no expiry. |
| **7. Detector or model update** | The learned sight re-learns per player (bars, font, taught things); syrup's glyph templates per size (54dad1e). | A shared update path: a change to thresholds, templates or UI detection proposed from many verified fixtures, never from one. |
| **8. Holdout** | CI runs the suite on every commit (CURRENT_STATE_AUDIT §1). | A frozen holdout split by participant and session, never used to tune, scored on every change with the KPIs of §4 ([EVALUATION_PLAN.md](EVALUATION_PLAN.md) for the statistics). |
| **9. Shadow / canary** | None. | Shadow: the new detector runs beside the old on the PC, outputs compared locally, not shown. Canary: a staged release to an opt-in share through the update channel. |
| **10. Rollback** | The updater rolls back "a version that does not come up" and never offers it again (`src/update.rs`, test `a_version_that_does_not_come_up_is_rolled_back_and_never_offered_again`) — but the release channel has never been used (0 releases, 0 tags — CURRENT_STATE_AUDIT §2.1). | Rollback triggered by **quality**, not only by a failed start: a KPI threshold breached in the canary rolls the change back. |

**A question for [CONSENT_AND_RIGHTS.md](CONSENT_AND_RIGHTS.md) and counsel (not a conclusion):** the repository
is public and already holds game screenshots as test fixtures (`resources/maplestory.png`,
`resources/hud-4k-strip.png`, `resources/hud-classic-4k-strip.png`; `resources/README.md` records no source or
rights basis). Derived game imagery is not automatically exempt (directive §10). Whatever the answer for these,
fixtures cut from contributors' frames must never be committed here.

## 3. Where to start — five areas, mapped to today's code

No large model needs training (directive §13). Each area starts from code that exists.

| Area | Today | First loop iteration | KPI a player would feel |
|---|---|---|---|
| **Thresholds and calibration** | `Detection{value, confidence, source, failure_reason}` (syrup `src/detection.rs`); trusted readings win over the bar, untrusted ones fall back (`observation.rs` tests); confidences are heuristic scores, uncalibrated (CURRENT_STATE_AUDIT §2). | Measure reliability of the existing scores on held-out fixtures; set each gating threshold from that curve, not by hand; keep `unknown` an outcome, never a zero. | false alarms per hour (low-HP warnings and deaths that did not happen); share of HUD readings `unknown` |
| **Templates** | The game's font learned from the teacher's labels and read every frame (`src/sight/numbers.rs`); the Classic World HUD (4288187); one font at two sizes keeps a template per size (syrup 54dad1e). | A line that will not learn already keeps its picture and reason: with consent, those become fixtures for a shared template set per HUD style. | time until the HUD numbers are read after first launch; number-backed bars (the CI self-test reported "0 of 3 backed by a number" on its screenshot — CURRENT_STATE_AUDIT §2.1) |
| **UI detection** | HUD found from the pixels alone; the teacher only when that fails (`src/sight/mod.rs`); taught objects with tracking, near misses as new poses (`a_near_miss_on_a_confident_track_becomes_a_new_pose`, `src/sight/things.rs`). | Teacher calls as the failure signal: each one is a frame the deterministic path could not read. Fewer calls over a session = the sight improving. | teacher-model calls per hour (cost and latency); wrong readings corrected per hour |
| **Retrieval** | `knowledge.json`: a looked-up answer is reused next time; a correction wins (`src/ai/knowledge.rs` tests); matched by shared words. | Version and expiry on every global entry; a correction becomes shared knowledge only after verification against several independent sources or reviewed gold (poisoning, directive §11). | repeated-question rate; answers given without a model (`instant_answers` in the local stats) |
| **Help timing and length** | The coach looks every 45 s, backing off to 180 s (`LOOK_EVERY`, `LOOK_AT_MOST`, `src/coach/mod.rs`); "it holds its tongue after four" (54d73c4); "say it twice, ask, then stop" (6d4840a); status recitals filtered (b28cdfb). | Players' explicit signals ("this helped", "don't interrupt", mute) as labels for when and how long to speak — self-report, never inferred frustration (directive §5). | interruptions per hour and mute rate; time to the first useful answer (`reply_ms_median` exists locally); reply length vs the player's stated preference |

## 4. KPIs — felt by the player, not "more data collected"

| KPI | Direction | Measured where |
|---|---|---|
| false alarms per hour of play | down | locally, from corrections and outcomes; shared only as aggregates with consent |
| wrong readings corrected per hour ("that's wrong") | down | correction events |
| HUD readings `unknown` | down, without turning `unknown` into a guess | detector outputs |
| time to the first useful answer, p50 / p95 | down | `reply_ms_median` exists in the local stats (`docs/data-and-metrics.md`) |
| answers without a model call | up | `instant_answers` exists in the local stats |
| model cost per hour of play | down | launch plan 2.5.1 measures it |
| interruptions per hour; mute rate | down | help events |
| repeated questions | down | help events |

Every KPI names its denominator and eligible population in [METRICS_CATALOG.md](METRICS_CATALOG.md). A change
ships when the holdout shows it improves at least one KPI and harms none beyond an agreed tolerance; the canary
confirms it; otherwise it rolls back.

## 5. Rules that keep the loop honest

- **No immediate global change from one correction.** Weight by independent confirmations; flag sudden
  floods from one participant; adjudicate disagreements (directive §11).
- **Originals are kept.** The original perception output, the model's prediction and the later correction are
  separate records with lineage; nothing is overwritten so that a mistake disappears (directive §7).
- **Personal stays personal.** A player's style, words and preferences improve that player's companion only.
- **Global knowledge carries a source, a game version and an expiry**, and is re-checked after a patch.
- **No backfill.** `log.txt`, `mic.wav`, recordings, `memory.json` and `knowledge.json` from before consent
  never enter the shared loop (CURRENT_STATE_AUDIT §6).

Roles (logical): Computer Vision owns thresholds, templates and UI detection; Model Evaluation owns the holdout,
shadow and canary decisions; Data Engineering owns the fixture store and lineage; Privacy & Security owns
stages 3–4; Product owns the KPIs. Tasks and order: [IMPLEMENTATION_PLAN.md](IMPLEMENTATION_PLAN.md).
