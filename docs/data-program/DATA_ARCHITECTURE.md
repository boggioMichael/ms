# DATA_ARCHITECTURE — capture, pipeline, storage, deletion and export

<div dir="rtl" lang="he">

**בקצרה:** המסמך מתאר איך נתוני מחקר נלכדים, נדגמים, מטוהרים ומגיעים ללקוח. לכל חלק מסומן אם הוא קיים היום, אם הוא חלק מהמקטע המקומי של P1 (w46) או אם הוא P2 ואילך.
העיקרון: קצב העיבוד המקומי (10 פריימים בשנייה היום) נפרד מקצב השמירה, וקצב השמירה נפרד מקצב ההעלאה. שומרים בשינוי מצב, ובנוסף דוגמים מדגם בסיס אקראי בהסתברות ידועה. מעלים בבאצ'ים. המחקר לא מוסיף אף קריאה לענן, ולא שולח פריים בכל מחזור.
בצד הלקוח הסדר הוא observers → שער הסכמה וזכויות → בונה Episodes → טיהור → דגימה → תור מקומי מוגבל ומוצפן → העלאה. ב-P1 אין רשת בכלל.
בצד השרת (P2) הסדר הוא אימות → בדיקה חוזרת של הסכמה וזכויות → אימות סכמה → קליטה אידמפוטנטית → בקרת איכות → קטלוג ו-lineage → מאגרים מאושרים → שער יצוא לכל לקוח.
ה-MVP: שירות קליטה אחד, Postgres, אחסון אובייקטים מוצפן ו-DuckDB, בלי Kafka, Kubernetes, vector DB או lakehouse. חשבונות והסכמות מופרדים ממאגר המחקר.
מחיקה מגיעה לכל נגזרת דרך אינדקס מחיקה. ספריית Syrup לא תלויה בשום שרת, משחק או מאגר משתמשים.

</div>

**Status:** P0 design (w45, 2026-10-10). Nothing here is deployed. Every figure is either cited from
the repository (with its file) or marked **proposed**. Proposed figures are starting points to replace
by measurement, never claims.

Related documents:
- `CURRENT_STATE_AUDIT.md`: what exists, cited and not repeated here.
- `DATA_CONTRACTS.md` and `src/research/`: the P1 types and code (w46). Field names are theirs.
- `METRICS_CATALOG.md`: what the data is for.
- `CONSENT_AND_RIGHTS.md` and `THREAT_MODEL.md`: the gates and the attacks they answer.
- `DATA_QUALITY_AND_BIAS.md`: sampling bias, splits and labels.
- `UNIT_ECONOMICS.md`: bitrate and cost arithmetic.
- `migrations/0001_research_catalog.sql`: the P2 catalog's DDL.

## 0. Status of every part

Legend:
- **EXISTS**: in the product today, at the audited base (`claude/data-program` from f9b6047; see the
  audit).
- **P1**: in w46's local slice (`src/research/`). It is not wired into the app, has no network, and runs
  on synthetic data.
- **P2+**: designed here and not built.

| Part | Status | Where |
|---|---|---|
| Game-window capture at `--fps` (default 10, clamped 1–30), newest-frame mailbox | EXISTS | `src/bin/maplesyrup/main.rs:99,192,248,561-605` |
| Perception → `WorldState` → `Observation`, gated on the game being in front | EXISTS | `src/perceive.rs`, `src/companion/observation.rs`, `main.rs:644-653` |
| `Detection<T>` with confidence, reliability, failure reason (confidence **uncalibrated**) | EXISTS | syrup `src/detection.rs`; audit §2.1 |
| Per-frame timing (`FrameTimings`, `tracing` spans, `StageRecorder` p50/p95), `vision_bench` | EXISTS | `src/util/stages.rs`, `src/bin/vision_bench.rs`, `bench/README.md` |
| Local play stats (no upload; sharing not offered) | EXISTS | `src/metrics.rs`, `docs/data-and-metrics.md` |
| Full-screen recorder (all windows, all PC sound) | EXISTS, and **not** a research source | `src/app/recorder.rs:363-377`; audit §4.3 |
| Consent ledger and receipts, rights manifests, the gate on every record, flush and exported row | P1 | `src/research/consent.rs` |
| Recorder that exists only when the gate allows it, with a bounded batch that withdrawal cancels | P1 | `src/research/recorder.rs` |
| Sanitizer (names, chat, whispers, links, e-mail, handles, credentials, numbers; `untrusted_text`) | P1 | `src/research/sanitize.rs` |
| Episode builder (dedupe, order, no future inputs, unobserved/censored ≠ failure) | P1 | `src/research/episode.rs` |
| Local store in three layers (spool, episodes, exports) plus lineage; deletion through every layer | P1 | `src/research/store.rs` |
| Local export (JSONL, recipient-scoped pseudonyms, manifest with SHA-256, data card, report) | P1 | `src/research/export.rs`, `tools/research_loader.py` |
| Observers wired into the app; state-on-change and base sampling | P2 | §3, §4 |
| Encrypted, bounded local queue; asynchronous batch upload | P2 | §4 |
| Ingest service, Postgres catalog, encrypted object storage, compaction, deletion job, export gate | P2 | §5–§9 |
| Separate, approved media path (bounded buffer, event clips, base clips) | P2+, and only for a title with written rights | §3.3 |
| RLDS adapter | P3, and only on a buyer's request | §10 |

## 1. Where Syrup the library ends

Syrup (the `syrup` crate and `syrup-runtime`) is a vision engine. Per the audit (§2.2), it has no
analytics server, no game and no user database. The data program keeps it that way:

- **Nothing in the program becomes a dependency of Syrup.** No ingest client, no consent store, no
  schema, no game identifier, no network crate. The research code lives in MapleSyrup
  (`src/research/`), which is the application. It consumes Syrup's `Detection<T>` values like any other
  caller.
- **Syrup stays usable without the program.** A studio or a developer using the SDK gets detections
  and nothing else. Their data goes nowhere.
- **The direction is one-way.** An adapter in MapleSyrup turns Syrup's outputs into research
  `observation` events. Syrup knows nothing of episodes, purposes or receipts.
- **Only a clean interface may be added to Syrup.** If the program needs something from Syrup (a
  per-component latency, or a calibrated confidence with its calibration reference), it is added as an
  ordinary, optional, documented field of `Detection<T>`, useful to any caller, and never as a hook into
  the program.

Two facts about Syrup's types matter for the research adapter:
- `Detection::timestamp` is **wall-clock** milliseconds (`SystemTime`), and it falls back to 0 when the
  clock reads before the epoch (syrup `src/detection.rs`, `Timestamp::now`). The adapter never orders
  or measures with it (§3.5).
- `Confidence` is a heuristic score in [0, 1], and `Confidence::NONE` is 0.0 for "no evidence". The
  adapter never exports it as a `confidence`. Schema 0.1.0 accepts a confidence only with a
  `calibration_ref` (`src/research/contracts.rs`), and no calibration exists (audit §2.1).

## 2. The whole path, at a glance

```text
 PLAYER'S PC (MapleSyrup)                                             SERVER (P2)
 ───────────────────────────────────────────────────────────────      ─────────────────────────────────
 capture ─► perception ─► Observation          (EXISTS, 10 fps)
                 │
   observers (state adapter, conversation hooks, explicit feedback)   (P2 wiring; P1 takes drafts)
                 │
   privacy / rights gate: consent × purpose × data type × recipient   (P1)
                 │            and the title's rights manifest; else no recorder
   episode builder (in memory; dedupe, order, no future inputs)       (P1)
                 │
   sanitizer (names, chat, links, credentials; untrusted_text)        (P1)
                 │
   sampling (state on change + base samples with known p; caps)       (P2)
                 │
   bounded, encrypted local queue (withdrawal purges it)              (P1 spool, unencrypted; P2)
                 │
   async batch upload (idempotent batch_id, backoff)  ──HTTPS──►  authentication
                                                                  consent and rights re-check
                                                                  schema validation
                                                                  idempotent ingest (event_id ledger)
                                                                  quality checks / bounded quarantine
                                                                  catalog and lineage (Postgres)
                                                                  curated datasets (Parquet)
                                                                  per-customer export gate
 identity and consent zone  ◄─ separate database, keys, roles ─►  research zone  ─►  customer zones
```

## 3. Capture and sampling (directive §8)

### 3.1 Three rates, kept apart

| Rate | What | Today / proposed |
|---|---|---|
| **Local processing** | how often the product looks at the game | EXISTS: `--fps`, default 10, clamped 1–30 (`main.rs:99,248`). Capture retries every 500 ms while the game is not found (`main.rs:596-600`). The research program **does not change it** |
| **Storage** | how often research events are made | P2: an `observation` event when a tracked component **changes** (with hysteresis), plus **base samples** (§3.2). Conversation, help, advice, feedback and outcome events when they happen. Never one event per frame |
| **Upload** | how often batches leave the PC | P2: one batch per N minutes or M events, whichever comes first. Proposed: 5 minutes or 1,000 events, with random jitter. Never per event, never per frame |

**No cloud call is added by research.** The product's own model calls keep their cadences:
- the coach looks every 45 s, backing off to 180 s (audit §4.1);
- the teacher's HUD check runs every 120 s, and its near-miss checks at most once every 20 s
  (`bench/README.md`).

The research path records what the product already did. It never asks a model for a label at run
time. A frame is never sent anywhere for research at render rate.

### 3.2 State on change, with base samples of known probability

- **On change.** A component (`hp_percent`, `level`, `map`, `menu_open`, …; the closed list in
  `contracts.rs`) is recorded when its value changes beyond a per-component threshold. Proposed: HP and
  MP when they cross a 5-point band, level and map on any change. Each component also has a minimum
  interval between events (proposed: 1 s), so that a flickering reading cannot flood the queue. These
  events carry `sampling_policy = census`: every qualifying change is kept.
- **Base samples.** The timeline is divided into slots (proposed: 10 s). In each slot, with probability
  p (proposed: p = 1 in P2's small panel, and lower once volume is measured), one full observation is
  taken **at a uniformly random instant inside the slot**. A random instant rather than a fixed phase
  avoids aliasing with periodic game events (respawn timers, buff cycles). These events carry
  `sampling_policy = base_random` and `sampling_probability = p`.
- **Why both.** Change events describe what happened. Base samples describe **time**: "HP unknown
  12% of the time" can only be estimated from a random sample of moments. A sample of changes, or of
  errors, gives a different and biased figure (directive §8; `METRICS_CATALOG.md` §2.4).
- **Event-triggered** records (`sampling_policy = event_triggered`, such as context around an error)
  are evidence. They are never part of a rate's denominator.
- **Status.** Schema 0.1.0 has the `sampling_policy` and `sampling_probability` fields, but no closed
  vocabulary and no base sampler yet. Both are listed in `METRICS_CATALOG.md` §4.2.

### 3.3 The media path: separate, approved, bounded (P2+)

The program's default carries **no pixels**. Events hold game components, never images. A media path
exists only as a separate track:
- its own consent purpose: `media_donation`, off by default;
- its own rights condition: the title's manifest licenses capture, storage and transfer of footage.
  For MapleStory and MapleStory Worlds that is `requires_title_specific_review`, so **no media path
  for them** until a review approves one;
- the owner's approval.

Its design:
- **Source:** the game window's client area only, through the same capture as perception. **Never the
  existing recorder**, which records the whole primary display with the cursor and all PC sound (audit
  §4.3). No audio in the MVP (`CONSENT_AND_RIGHTS.md`, retention proposals).
- **Bounded ring buffer, in memory only.** It holds the last T seconds at the product's resolution and
  frame rate, bounded in bytes (proposed: 30 s, capped at 64 MB). When full, the oldest frame goes and
  the drop is counted. Nothing is written to disk until a clip is selected and has passed §3.6.
- **Event clips** around significant moments (a failure, a help request, a correction, a success):
  pre-roll from the buffer and post-roll captured after the event (proposed: 10 s each). A short clip
  is not a substitute for a long sequence. Planning tasks need minutes of context, so a product that
  needs them requests longer windows explicitly, at lower frame rates.
- **Base clips** at random slots, with a known probability, so that clip-based error rates are not
  inflated by event triggering.
- **Review before upload.** Selected clips wait in the local queue for a review window (proposed:
  24 hours) in which the participant can see and delete them (directive §3). Nothing uploads during the
  window.

### 3.4 Inclusion probabilities, drops, rejections and caps

Every capture interval produces a `capture_quality` event. In 0.1.0 it carries `status`,
`dropped_frames` and `clock_offset_ms` (`contracts.rs`). P2 adds the counters the integrity metric needs
(`METRICS_CATALOG.md` M12.8):
- frames processed;
- frames rejected by the privacy filter;
- base slots eligible and taken (realised versus declared probability);
- in-focus milliseconds;
- whether a **per-participant cap** was reached. Proposed caps: at most 4 recorded hours a day and 20
  clips a day per participant, so that one heavy player cannot dominate a dataset
  (`DATA_QUALITY_AND_BIAS.md`).

When a cap is reached, research recording stops for the day and the product carries on.

### 3.5 Clocks, jitter, offsets, coordinates; degrade or reject

- **Order and duration** use the monotonic clock only. Inside a session, `sequence_no` (one counter in
  the recorder) gives the order, and `monotonic_timestamp` (milliseconds since the session's start)
  gives durations (`contracts.rs`). Wall-clock time appears only as `ingested_at` and is never used to
  order events. Syrup's `Detection::timestamp` is wall-clock and is not used (§1).
- **Cross-session order** uses the first `ingested_at` of each session, as w46's builder does. Durations
  never span sessions (`METRICS_CATALOG.md` §2.9).
- **Audio, video and event synchronisation** (media path only): every frame carries the capture
  thread's monotonic time at grab, and every event carries its own. The offset between them is measured
  per session against a known UI event. The existing recorder's CI test shows the method on its own
  path ("the tone +39 ms from the flash", audit §2.1); the research path needs its own measurement.
- **Jitter:** the per-interval p95 deviation of `FrameTimings::frame_interval` from 1/fps, which exists
  today in the frame record (`main.rs:684-697`).
- **Coordinates:** frame pixel → window client area → normalised [0, 1] relative to the client
  rectangle. The client size, the DPI scale and the capture path (GPU or GDI) are recorded. A resize or
  a DPI change starts a new geometry epoch, and no clip spans two epochs.
- **Thresholds.** If |offset| exceeds T₁, jitter p95 exceeds T₂, or geometry changes mid-clip, the record
  is **degraded** (marked, and excluded from timing-sensitive products) or **rejected** (dropped and
  counted). T₁ and T₂ are set **per product by measurement**: precise vision-action training needs far
  tighter sync than HUD understanding (§3.7). No universal value is proposed.

### 3.6 When capture stops, and what never leaves

**Stop conditions.** Research recording pauses (a `session` event with `action = pause` and its reason)
and resumes only when the condition clears:
- **Focus loss or a window switch.** The product already clears its frame buffer when the game is not in
  front (`main.rs:644-653`).
- **The game is not found or not capturable:** `GameView::NotFound` or `Unavailable`
  (`observation.rs:32-39`).
- **Detection failure:** the HUD has not been seen for N seconds (proposed: 5 s). Outcomes during that
  time become `unobserved`, never `failure`.
- **Login, character-select and account screens,** and any screen showing account or payment details
  (`CaptureStatus::LoginScreen` exists in 0.1.0).
- **An unreliable game identity.** The audit found that "the game" is any window whose title contains
  "maplestory" (`src/capture.rs:57-67`, `pick_game_window`). Research needs more than the title. It also
  needs the process image name, the window class and a HUD fingerprint consistent with the declared
  `game_id` and `game_variant`. Otherwise `game_id = unknown`, which has no rights manifest, so the gate
  refuses and nothing is recorded.

**Local filtering, before anything is written:**
- **Personal fields never enter an event.** Today's `Observation` includes `name` (the character name
  read from the HUD, `observation.rs:55`). The observer adapter drops it, and schema 0.1.0 has no
  component for it. The play stats' closed class list (`CLASSES` in `src/metrics.rs`) is reusable for
  `job_class`.
- **Chat, whispers, party and guild text are dropped whole.** The product's `chat_log` detector
  (`snapshot.rs:30`) feeds nothing in research.
- **Text kept from the game is `untrusted_text`** (`contracts.rs`), redacted by the sanitizer: names,
  links, e-mail addresses, handles, credentials and numbers. It is **data, never an instruction**. The
  sanitizer flags text that reads like an order (`instruction_like`), and no code path changes a
  consent, a permission, a rights decision or an export because of what game text says (§17: prompt
  injection through in-game text).
- **The player's own words** stay on the PC unless a purpose that covers them was consented. When they
  are kept, it is as `SanitizedText` with redactions. Until then, help events carry their kind and
  channel only.
- **Credentials and keys** (`openai-key.txt`, `xai-key.txt`, tokens) are never read by the research
  path. Logs never hold them (`THREAT_MODEL.md`).

**Frames (media path only):** a frame is uploaded only when it is **known** to be clean. Clean is a
positive verdict:
- the chat region and name tags are masked by layout;
- a text detector finds no unmasked text outside the HUD's allow-listed regions;
- there are no notification overlays.

Anything uncertain is rejected and counted, never uploaded "probably clean". This is default-deny.

### 3.7 Resolution and frame rate, chosen per product by measured utility

There is no single setting. Each product states what it needs, and the setting is chosen by an ablation
on that product's task (`EVALUATION_PLAN.md`):
- **HUD and UI understanding** needs legible pixels where the text is: native-resolution crops of the
  HUD and the dialog regions, at a low rate.
- **Offline video understanding** may tolerate a downscaled frame.
- **Vision-action training** needs a high, steady rate and tight sync, and it also needs an action
  source that is verified (`publisher_sdk` or `licensed_replay`; `contracts.rs` `Measurement`). It is
  never actions inferred from video.

The storage cost of these choices is computed in `UNIT_ECONOMICS.md` (the §15 bitrate comparison) and
is not repeated here.

## 4. The client pipeline

Each stage below has a single job and **bounded resources**. When a stage fails, research stops and
the product carries on.

**The critical path stays free.** Research runs on its own thread behind a bounded channel. When the
channel is full, research events are **dropped and counted** (a `capture_quality` event). The vision
thread and the main loop never wait on it.

| # | Stage | Job | Bounded by | On failure | Status |
|---|---|---|---|---|---|
| 1 | **Observers** | Turn what the product already has into event drafts: the observation adapter (`Observation` components, with the `name` field dropped), conversation hooks (a help request, an advice shown), explicit feedback ("that helped", "that's wrong", "don't interrupt", "goal reached"), and outcomes the companion already detects (deaths, level-ups). They read no input devices and no new screen regions | the bounded channel | drop and count | P2 (P1 accepts drafts from its caller) |
| 2 | **Privacy and rights gate** | Is there an adult's consent in force for this purpose, data type and recipient class? Does the title's rights manifest, in force, license capture and storage for this purpose? If not, **there is no recorder** (`Option` is `None`): no work, no folder, no file. Asked again on every record and every flush | – | refuse (no event) | P1 (`consent.rs`, `recorder.rs`) |
| 3 | **Episode builder** | Group events into episodes (goal → state before → help → advice → actions → outcome → corrections). Dedupe by `event_id`, order by `sequence_no`, reject later inputs to earlier advice, and never turn a lost window into a failure | memory per open episode (proposed cap: 10,000 events, then the episode is closed `censored`) | close the episode `censored` | P1 (`episode.rs`; locally it runs on the spool, not in memory) |
| 4 | **Sanitizer** | Drop chat and whispers; redact names, links, e-mail addresses, handles, credentials and numbers; keep game text as `untrusted_text`. Idempotent | per-text length cap | withhold the text, keep its kind | P1 (`sanitize.rs`) |
| 5 | **Sampling** | Apply §3.2 (change thresholds, base slots with p), the caps of §3.4, and stamp `sampling_policy` and `sampling_probability` | per-participant caps | stop research for the day | P2 |
| 6 | **Local queue** | The bounded, protected spool of batches waiting to upload | proposed: 50 MB and 7 days, oldest dropped and counted | stop research (not the product) until space returns | P1 (spool, unencrypted, local only); P2 encrypted at rest with a per-user OS key (Windows DPAPI is the candidate to evaluate), not readable by other OS users |
| 7 | **Upload** | Asynchronous batches over HTTPS, each with an idempotent `batch_id`; exponential backoff with jitter; obeys the server's `Retry-After` | one batch in flight | keep the batch; retry later | P2 (P1 has **no network code at all**, and a test proves it) |

**Withdrawal** purges stage 6 at once and ends stage 2's recorder, so a pending batch never leaves (P1
test: "withdrawal with a batch pending cancels it"). Upstream deletion follows (§9).

**No implied consent.** Using the product, having an API key, or pressing "record" grants nothing
(directive §3). The gate reads the consent ledger only. The existing local files (`memory.json`,
`knowledge.json`, `log.txt`, `mic.wav`, recordings) are **never** read by the research path and never
backfilled (audit §6).

## 5. The server pipeline (P2)

| # | Stage | Job | Notes |
|---|---|---|---|
| 1 | **Authentication** | A device-bound, short-lived token, issued at enrolment in the panel and rotated | No API key in a URL or a log. Per-token rate limits |
| 2 | **Consent and rights re-check** | For each batch: the `consent_receipt_id` and `consent_epoch` are current for the `collection_purpose`, and the `rights_policy_id` is approved, in force, covers the game, build, territory and use, and is not revoked. Consent is asked through `consent.receipt_allows()`, the ingest role's only access to the consent zone (the DDL) | **Re-checked here even though the client checked.** A withdrawn subject's late batch is refused, and the client purges it |
| 3 | **Schema validation** | `schema_version` is known. Every envelope field is present (null when unknown). Payloads validate against the contract (`contracts.rs` validation) | An unknown version goes to quarantine, never best-effort parsing |
| 4 | **Idempotent ingest** | `batch_id` already seen → return the stored result. Each `event_id` goes through the idempotency ledger (`research.event_ids`). A duplicate is counted, not re-inserted | One transaction per batch |
| 5 | **Quality checks and bounded quarantine** | Range and type checks; per-session monotonic `sequence_no` (gaps counted); clock anomalies; flooding (per-participant caps); poisoning heuristics on corrections (`DATA_QUALITY_AND_BIAS.md`) | Quarantine is bounded in size and time (proposed: 14 days). Then it is resolved or deleted. It is never a back door to keep data |
| 6 | **Catalog and lineage** | Rows in Postgres (`migrations/0001_research_catalog.sql`): sessions, episodes, events (partitioned), media objects; lineage edges from event to episode to dataset to export to training run | No graph database (directive §7). The edges table is enough |
| 7 | **Curated datasets** | Verified episodes (layer 2), split train / validation / test **by participant and source sequence before any clip** (`DATA_QUALITY_AND_BIAS.md`), written as Parquet | Each dataset records its members in the deletion index (§9) |
| 8 | **Per-customer export gate** | At export time: purpose, recipient, rights, consent and QA are all re-checked; aggregate thresholds for analytics products; per-customer pseudonyms; manifest and data card (§10) | Refuses a dataset whose membership is unknown |

## 6. Three logical layers, and the zones that keep identity apart

| Layer | Holds | Locally (P1) | Server (P2) |
|---|---|---|---|
| **L1: accepted, sanitized data** | events that passed the gate and validation. "Raw" here never means an unsanitized private screen | `spool/events.jsonl` | `research.events` (Postgres, partitioned) and nightly Parquet (§8) |
| **L2: verified episodes** | built, QA-passed episodes with labels kept apart (automatic, user correction, reviewed gold) | `episodes/episodes.jsonl` | `research.episodes`, `research.labels`, Parquet datasets |
| **L3: customer products** | per customer and per purpose: datasets, eval suites, aggregate reports | the local export folder | `delivery.*` and per-customer object-storage prefixes |

**Zones (directive §9, §10):**
- **Identity and consent zone.** Accounts, contact and incentive-payment details (product D's fair
  compensation), the **only** table linking an account to a `research_subject_id`, the consent
  receipts and texts, and offer tallies. A **separate database** with its own credentials and roles.
  Proposed: a separate managed instance, or at least a separate database and role that the research
  service cannot read.
- **Research zone.** Keyed only by `research_subject_id`, a pseudonym. It holds no name, e-mail,
  account or payment field. It can ask the consent zone one question through a narrow interface: "is
  receipt R at epoch E valid for purpose P?" It cannot list who consented.
- **Customer zones.** One per customer. Identifiers are `HMAC-SHA-256(customer_key, research_subject_id)`
  (w46's export already does this per recipient). Customer keys are kept apart, so two customers'
  copies share **no key to join them**.
- **The P2 migration** keeps the consent tables in their own schema, with **no foreign key** from
  research tables into them, so that the schema can move to a separate database without a rewrite
  (`migrations/0001_research_catalog.sql`, header).

The minimal consent and security ledger is its own record with its own policy. It is not a back door for
telemetry when product collection is off (directive §4).

## 7. The MVP stack, and what is deliberately absent

**Default (directive §9):**
- **One ingest service**: a single process that runs stages 1–6 of §5.
- **Postgres** for metadata, permissions, the catalog and, at P2's volumes, the hot event store (§8).
- **Encrypted object storage** for Parquet datasets, exports and (only on the media path) media. Each
  object is encrypted at rest with server-side encryption; media additionally use per-object data keys
  wrapped by a managed key service.
- **DuckDB** for analysis and dataset builds. It reads Parquet directly, and reads Postgres through its
  read-only Postgres attachment (`METRICS_CATALOG.md` §5).

**Fit with what exists.** The team's rule is "as free as possible without compromising quality"
(`docs/launch-plan/README.md`). The program has one domain (`datta-syrup.ai`, which serves the update
manifest; audit §4.1). The ingest endpoint would be a **separate host name, service and credential
set** from the update channel, so that compromising one does not reach the other (`THREAT_MODEL.md`).
Its hosting provider is an owner decision. It is not decided here: no paid service is set up and no
deployment is made in P0 or P1.

**Not in the MVP, with the measurement that would justify each:**

| Component | Add only when |
|---|---|
| Kafka or another log broker | Measured sustained ingest that one service with Postgres cannot absorb at the agreed p99 latency, or several independent consumers that need replay |
| Kubernetes | Several services with independent scaling needs, where measured operations time exceeds what a single VM or a managed container service costs |
| A vector database | A product that needs similarity search over embeddings, at a measured scale beyond what DuckDB or Postgres can do. Embeddings are another deletion surface (§9) |
| A lakehouse table format (Iceberg, Delta and the like) | Concurrent writers, time travel or schema evolution at a scale that the partitioned Parquet plus the catalog demonstrably cannot handle |

## 8. Storage layout and the failure modes it handles

**Partitioning** is by game, build and collection date, **never one file per user** (directive §9):
- **Hot store.** `research.events` is partitioned by `LIST (game_id)` and then by `RANGE
  (collection_date)` (`migrations/0001_research_catalog.sql`). It is the system of record for ingest,
  because it gives idempotency, the re-check, quarantine and deletion in one transactional place.
  Partitions are **dropped** when their retention ends (proposed: detailed events kept at most 90 days;
  `CONSENT_AND_RIGHTS.md`). Dropping a whole partition is cheap retention.
- **Analytical store.** A nightly job writes each accepted day as Parquet:
  `l1/events/game_id=<g>/game_build=<b>/dt=<YYYY-MM-DD>/part-<n>.parquet`. This is the single compaction
  point: one file per partition-day while volumes are small, and files split at a target size chosen by
  measurement. The job records each file in the catalog with its checksum, row count and the subjects
  it contains (§9).
- **Datasets and exports** (L2, L3): `l2/datasets/<dataset_id>/…` and
  `l3/<customer>/<export_id>/…`, each with a manifest.
- **Migration path.** When measured volume makes Postgres the bottleneck, events land directly in
  Parquet under the same partition keys, and Postgres keeps only the catalog. The queries do not change,
  because they read views (`METRICS_CATALOG.md` §5).

**Failure modes:**

| Case | Handling |
|---|---|
| **Small files** | The client batches (§3.1). The server writes rows, not files. Parquet is written once per partition-day by the nightly job |
| **Duplicates** | `batch_id` idempotency (`research.ingest_batches`), and the `event_id` ledger (`research.event_ids`, an unpartitioned primary key) with `ON CONFLICT DO NOTHING`. The episode builder dedupes again, and two different events under one id are both kept out (w46's builder) |
| **Out of order** | Accepted as they arrive. Order is always `(session_id, sequence_no)` at read time. A session that receives late events is marked dirty, and its episodes are rebuilt |
| **Retry** | Same `batch_id`, same `event_id`s, so the server's answer is the same |
| **Clock drift** | The client's wall clock is never trusted for order. The server compares the client's send time with its own receive time per batch, and flags a skew beyond a threshold (proposed: 1 day) in the batch record. `collection_date` comes from the session's first event, and is bounded by the server's receive date ± 2 days, or the batch is quarantined |
| **Offline** | The queue holds batches up to its cap and age (§4, stage 6). Older batches are dropped and counted. A withdrawal made offline purges the queue locally at once. On reconnect, the server's re-check (§5, stage 2) refuses anything collected under a stale epoch |
| **Backpressure** | The server answers `429` or `503` with `Retry-After`, and the client backs off. A full queue stops research recording until space returns, and the gap is recorded as a `capture_quality` event. **The product is never slowed** (§4) |

## 9. Deletion and the deletion index

"Append-only" describes how files are written. It does not mean data is kept forever (directive §9).

- **On the PC (P1, built).** A withdrawal stops the recorder and cancels the pending batch.
  `ResearchStore::delete_subject` removes the subject's rows from the spool, from the episodes and from
  every export, and remakes the exports' reports, data cards, counts and checksums. Its lineage lists
  the exports affected (`src/research/store.rs`).
- **On the server (P2).** A deletion request (`deletion.deletion_requests`) runs as tasks
  (`deletion.deletion_tasks`), one per target, each closed with a minimal proof (time, target id, new
  checksum, and no content):
  1. Mark the subject `deleting`. The export gate now refuses any dataset that contains them.
  2. Delete the subject's rows from the hot store (events, episodes, labels, media-object rows).
  3. Delete media objects, **including every stored version** in versioned buckets.
  4. **Rewrite** every Parquet file the deletion index lists for the subject: copy without their rows,
     record the new checksum, swap, delete the old object. The index is `delivery.subject_presence
     (research_subject_id, artifact_id, rows)`, maintained by every job that writes an artifact.
  5. Rebuild or rewrite derived artifacts (datasets, features, and embeddings if any ever exist).
  6. **Customer exports:** tell each affected customer under the contract's terms. The lineage gives the
     export ids. The customer deletes, and confirms where the contract requires it.
  7. **Backups** expire on a documented cycle (proposed: 35 days). A restore replays the deletion ledger
     before the data is used again.
  8. **Trained models:** record which training runs consumed which snapshots
     (`delivery.training_consumption`). **No promise is made that deleting a file removes its influence
     from weights already trained** (directive §10). The contract says what happens, and that is a
     legal and operational decision to make before any external training license
     (`CONSENT_AND_RIGHTS.md`).
- **What is kept:** the pseudonymous consent receipts and the deletion proof. This is the minimum
  needed to show the request was handled. The identity zone's link from account to subject is deleted.
  Whether this minimum is enough is a question for the lawyer.

## 10. Exports

**Per release** (directive §14). w46's P1 export already writes a local version of the first four
items:

| Item | Content |
|---|---|
| Metadata | Parquet (P2) or JSONL (P1): events, episodes, and labels kept apart by source |
| Media | Separately, and only what is licensed, consented and verified clean (§3.6). Each file is referenced from the metadata by id and checksum |
| `manifest.json` | Every file with its SHA-256, bytes and rows; `schema_version`; the dataset and build-process versions; counts of unique participants, sessions, episodes and observed hours; a consent and rights summary that exposes no participant; `export_id`; the pseudonym scheme. P2 adds a detached signature with a key used **only** for exports, never the app-update key |
| `DATA_CARD.md` | The data dictionary, quality and coverage/bias reports, consent and rights summary, intended and prohibited uses, limits, retention and deletion terms, and the loader with example queries. The Data Cards Playbook (directive source 8) was **not fetched** in this session, so its recommended structure is unverified here. The content list above is the directive's own (§14) |
| Loader | `tools/research_loader.py` (P1, stdlib only): verifies the checksums and refuses on a mismatch. Example queries: `METRICS_CATALOG.md` |

**Delivery (P2):** the customer's own prefix; short-lived signed URLs (proposed: at most 24 hours); every
access logged (`audit.access_log`); nothing public.

**RLDS adapter (P3, only where the semantics fit, and only on a buyer's request).** The RLDS repository
(directive source 7) was **not fetched** in this session, so the field mapping below is the author's
understanding and must be checked against the repository before anything is built:
- An episode becomes an RLDS episode. A step comes from a base-sampled observation.
- The step's **action** comes only from an action with a verified source (`publisher_sdk` or
  `licensed_replay`). Self-reports are language annotations, not actions. **No action inferred from
  video is presented as recorded input** (directive §2B).
- **Unknown reward stays missing.** No convenience metric (success, satisfaction, time) is turned into a
  reward (directive §9).
- A terminal step comes only from an observed `success` or `failure`. An `unobserved` or `censored`
  episode is truncated, not terminated.
- **Before building it,** check the buyer's preferred format (`BUYER_VALIDATION.md`). RLDS is one
  candidate, not a default.

## 11. Overhead: what to measure, against what baseline

The directive (§17) requires overhead measured against a baseline, with the critical path kept
non-blocking. Acceptance thresholds come from the measurement and the players' needs, not from this
document.

| Quantity | How | Exists today |
|---|---|---|
| Frame time p50, p95, p99 | the `frame` span and its children (`vision`, `observation`, `sight`) through `StageRecorder`, offline in `vision_bench` and live in the dashboard | p50 and p95 exist (`src/util/stages.rs:101,119-120`). **p99 is to be added** |
| Research cost per frame | new spans (`research.adapter`, `research.gate`, `research.sanitize`, `research.queue`) on the same recorder | P2 |
| CPU, GPU, RAM | process counters during a 2-hour session, with research off versus on | launch-plan task 1.2.3 measures CPU, GPU and memory after two hours for the product. Reuse it |
| Disk | bytes written per observed hour by the queue | P2 |
| Network | bytes sent per observed hour | P2. P1 sends nothing by construction |
| Cost per play hour | server, storage and egress per observed hour, from the P2 cost ledger | P2 (`METRICS_CATALOG.md` M12.6; `UNIT_ECONOMICS.md`) |

**Baseline.** The same build and session, with research **off**. In P1 the module is not wired into the
app, so its overhead on the product is zero by construction, and the measurement starts in P2 when it
is wired.

The documented reference numbers are the team's, not this program's: `bench/README.md` phase 6 (CPU)
gives frame mean / p50 / p95 of **5.2 / 4.9 / 9.0 ms** at 1366×768 on a 2-core Linux machine. These
were measured by the team and **not re-run here**. Windows capture numbers are still blank in that
file ("—"). The research path is judged by the **difference** it makes on the same machine, at the same
fps, on the same recording.

## 12. Acceptance tests (directive §17), and where each is handled

| Test | Component | Phase |
|---|---|---|
| Consent off: no recorder, no file, no folder, no network | gate, recorder (§4) | P1 test |
| Withdrawal with a batch queued | gate, queue (§4) | P1 test |
| Expired rights license | gate; server re-check (§5) | P1 test; P2 |
| Export for a purpose or recipient not consented or not licensed (MapleStory refused even with consent) | export gate (§5, §10) | P1 test |
| Alt-tab, login screens | stop conditions (§3.6) | P2 (needs wiring) |
| Chat, a player name, a notification | sanitizer, observer adapter, frame verdict (§3.6) | P1 test (text); P2 (frames) |
| Full queue | queue cap, backpressure (§4, §8) | P2 |
| Network down | offline handling (§8) | P2 |
| Duplicate and out-of-order events | builder; idempotent ingest (§8) | P1 test; P2 |
| Clock drift | §3.5, §8 | P2 |
| `unknown` never becomes 0 | contracts; prelude capabilities (`METRICS_CATALOG.md` §4.2) | P1 test |
| A model inference never becomes gold | provenance check (`contracts.rs`) | P1 test |
| A poisoned correction | kept as a claim, never applied (builder); quarantine heuristics (§5) | P1 test; P2 |
| Customer separation | per-customer pseudonyms (§6) | P1 (recipient keys); P2 |
| Train/test leakage | splits by participant and sequence before clips (§5, stage 7) | P3 |
| Deletion reaching every derivative | deletion index and tasks (§9) | P1 test (local layers); P2 |
| Overhead against the baseline | §11 | P2 |

**Zero leaks in these tests is an acceptance condition, not a proof** that no leak can happen in
production (directive §17). Human checks and the stop policy are in `THREAT_MODEL.md`.

## 13. What I could not verify

1. **Almost nothing here has been run.** No ingest service, no queue, no compaction job and no deletion
   job exists. The P1 behaviour cited is w46's code as read at about 10:35 on 2026-10-10. Its tests are
   w46's to report. The one exception is the catalog DDL. It was applied once to an empty, throwaway
   PostgreSQL 16 cluster, with a rolled-back smoke script on synthetic rows: partition routing,
   append-only triggers, the gold, payload-tag, token and quarantine constraints, consent refused after a
   withdrawal, no MapleStory grants, and deletion leaving no rows all behaved as designed. The cluster
   was then deleted. That shows the DDL is accepted. It says nothing about performance or operations.
2. **The proposed figures are starting points, not measurements:** slot length, base probability,
   thresholds, caps, buffer sizes, queue limits, retention periods, quarantine time and URL lifetime.
3. **Sources not read.** The directive's sources (RLDS, Apache Parquet, the Data Cards Playbook, Unity
   Analytics' event documentation and the others) could not be fetched in this session, because the
   fetch permission was not granted. Anything said about RLDS's structure (§10) is marked unverified.
4. **Windows behaviour** is unverified: the capture path, DPAPI as the queue's key store, focus and
   window events, and the overhead on the owner's PC (`bench/README.md` has no Windows capture numbers).
5. **Game identity beyond the title** (§3.6: process name, window class, HUD fingerprint) is a design.
   No code distinguishes MapleStory from MapleStory Worlds today (audit §6).
6. **Legal points** (retention periods, the sufficiency of the deletion proof, model weights after
   deletion, cross-border hosting) are questions for counsel (`CONSENT_AND_RIGHTS.md`). They are not
   answered here.

