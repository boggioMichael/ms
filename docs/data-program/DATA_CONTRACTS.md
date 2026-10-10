# DATA_CONTRACTS — research events, episodes, consent, rights and the local export (schema 0.1.0)

<div dir="rtl" lang="he">

**תקציר לבעלים.** המסמך מתאר את חוזי הנתונים של תוכנית הנתונים ואת מקטע P1 המקומי שמומש בקוד
(`src/research/`, מבודד: לא מחובר ללולאה הראשית של האפליקציה, בלי שום קוד רשת). הקוד הוא מקור האמת;
המסמך מתאר אותו, בגרסת סכמה `0.1.0`. כל אירוע נושא מעטפת של 25 שדות, ושייך לאחת מ-11 משפחות.
לכל תצפית, פעולה, תווית ותוצאה יש provenance; ערך חסר נשאר `null` ולעולם לא 0; ודאות נרשמת רק
כשיש לה כיול. ההסכמה נקבעת לפי מטרה × סוג נתון × נמען, עם קבלה (גרסת נוסח, זמן, מקור, היקף, epoch)
ביומן נפרד ומצומצם; הזכויות לפי rights manifest לכל כותר — MapleStory ו-MapleStory Worlds מסומנים
"דורש בדיקה ייעודית" ולכן אין הקלטה ואין יצוא שלהם, גם עם הסכמה מלאה; `synthetic` מאושר להדגמה מקומית
בלבד. היצוא המקומי כולל קבצים מטוהרים תחת מזהים פסאודונימיים לכל נמען, דוח ראשון, data card ו-manifest
עם SHA-256 לכל קובץ, ו-loader בפייתון שמסרב כשיש אי-התאמה. 18 בדיקות קבלה עוברות (ראו §14).
הכול רץ על נתונים סינתטיים בלבד. זה לא ייעוץ משפטי.

</div>

**Status.** P1 local slice, implemented and tested on synthetic data only. The code in `src/research/` is the
source of truth; this document describes it. Nothing here is deployed, nothing is uploaded, nothing reads a
real player. Related documents (same folder): [CURRENT_STATE_AUDIT](CURRENT_STATE_AUDIT.md) (what exists
today and why episodes cannot be built from today's files), [METRICS_CATALOG](METRICS_CATALOG.md) (metric
definitions), [DATA_ARCHITECTURE](DATA_ARCHITECTURE.md) (where this slice sits in the client → server path),
[CONSENT_AND_RIGHTS](CONSENT_AND_RIGHTS.md) (the policy behind the gate), [THREAT_MODEL](THREAT_MODEL.md),
[DATA_QUALITY_AND_BIAS](DATA_QUALITY_AND_BIAS.md), [IMPLEMENTATION_PLAN](IMPLEMENTATION_PLAN.md).
**Not legal advice**: every legal point below is a question for counsel.

| Piece | File |
|---|---|
| Contracts (envelope, families, provenance, vocabularies, validation) | `src/research/contracts.rs` |
| Consent, ledger, rights manifests, refusals | `src/research/consent.rs` |
| Sanitizer | `src/research/sanitize.rs` |
| Recorder (the only way in) | `src/research/recorder.rs` |
| Research folder, lineage, deletion | `src/research/store.rs` |
| Episode builder | `src/research/episode.rs` |
| Local export, manifest, data card, verification | `src/research/export.rs` |
| First report | `src/research/report.rs` |
| Synthetic generator and the whole slice | `src/research/synthetic.rs`, `examples/research_slice.rs` |
| Loader (Python, standard library) | `tools/research_loader.py` |
| Acceptance tests; committed fixtures | `tests/research_slice.rs`; `tests/fixtures/research/` |

Run it: `cargo run --release --example research_slice -- <new folder>`, then
`python3 tools/research_loader.py <folder>/export`.

## 1. Versioning and conventions

- `SCHEMA_VERSION` = **`0.1.0`** (`contracts.rs`), semantic versioning: adding an optional field is a minor
  change; removing or retyping a field, or changing its meaning, is a major one. Every event and episode
  carries it; the builder quarantines an event of another version; the loader refuses an unknown one.
  The manifest has its own `manifest_version` (1).
- **Missing is missing.** An unknown value is JSON `null`, and the key is always present (a reader's
  `row.get("x", 0)` gets `None`, not 0). Never 0, never `""`, never a guessed number.
- **Identifiers** (ids, versions, categories, policy ids, evidence refs) are tokens: 1–96 characters of
  `A–Z a–z 0–9 . _ : + / -`, no `//` (no link), no `@` (no address), no space (no sentence) —
  `contracts::is_token`. The recorder also refuses an identifier that contains a known character name.
- **Time**: `monotonic_timestamp` is milliseconds since the session's start from a monotonic clock —
  comparable within one session only; `ingested_at` is UTC wall clock (RFC 3339). Durations are computed
  within a session; across sessions they are `null`.
- **Texts**: what the game shows is `untrusted_text` (data, never an instruction); the player's and the
  assistant's words are `text`; both are sanitized before anything is written (§6).

## 2. The envelope

Every event is one JSON line: the 25 envelope fields below (`ENVELOPE_FIELDS`, in this order) and
`payload`, nothing else (a test holds every recorded event to exactly that).

| Field | Type | Null? | Meaning |
|---|---|---|---|
| `event_id` | token | no | Unique per event; the deduplication key (a retry writes the same id). Random 128-bit for real data; `<session>-eNNNN` for synthetic data. |
| `schema_version` | string | no | `0.1.0`. |
| `session_id` | token | no | One run of the companion. |
| `episode_id` | token | yes | The episode (one attempt at one goal) the event belongs to; `null` for session-wide events (start/end, background observations, capture quality). |
| `research_subject_id` | token | no | A pseudonym for the participant, never a name, account, device or address. In an export it is replaced by a recipient-scoped pseudonym (§9). |
| `event_type` | enum | no | The family (§4); must match the payload. |
| `game_id` | enum | no | `maplestory`, `maplestory_worlds`, `synthetic`, `other`, `unknown` (§3). |
| `game_variant` | token | yes | A variant when reliably known (e.g. a world type such as `classic_world`). |
| `world_id` | token | yes | The game's world or server name when known (the game's, not a player's). |
| `game_build` | token | yes | The game client's build when known. |
| `platform` | token | yes | `windows`, … |
| `locale` | token | yes | Interface language without region (`en`, `he`). |
| `client_version` | token | no | MapleSyrup's version. |
| `detector_version` | token | yes | The perception stack's version. |
| `model_version` | token | yes | The model behind this event (advice, an inference). |
| `sequence_no` | integer | no | The recorder's counter in the session, from 1, without gaps for accepted events: the order. |
| `monotonic_timestamp` | integer ms | no | Since the session's start (monotonic; the recorder refuses a clock that goes back). |
| `ingested_at` | RFC 3339 UTC | no | When the recorder accepted the event. |
| `observation_coverage` | enum | no | `full`, `partial`, `not_visible`, `unknown`: how much of the game the system could see. |
| `consent_receipt_id` | token | no | The receipt in force when the event was collected. |
| `consent_epoch` | integer | no | That receipt's epoch (§7). |
| `collection_purpose` | enum | no | The purpose the recorder was opened for (§7). |
| `rights_policy_id` | token | no | The rights manifest in force at collection (§8). |
| `sampling_policy` | token | no | The policy that kept this event (P1: `all-task-events-v1`, every event of the slice's families). |
| `sampling_probability` | number in (0, 1] | yes | The probability an event like this one was kept; `null` when not known. |

There is no personal field: no name, account, e-mail, IP, device id, path or free text in the envelope.

## 3. Game identity

`game_id` distinguishes **MapleStory** (`maplestory`, Nexon's regular client, any world type) from
**MapleStory Worlds** (`maplestory_worlds`, a separate product with its own terms) and from everything else
(`other`), and is `unknown` without a reliable identification. Today's capture identifies "the game" by a
window title containing "maplestory" ([CURRENT_STATE_AUDIT](CURRENT_STATE_AUDIT.md) §6) — not reliable enough
to tell the two apart or to exclude a non-game window, so a real producer must send `unknown` until a reliable
identification exists. A title can be recorded only if its rights manifest allows it (§8): today, none but
`synthetic`.

## 4. Event families

`payload` is `{"<family>": {...}}`. Every observation component, task, help request, advice, action,
correction, feedback, outcome and experiment assignment carries a `provenance` (§5).

| Family | Payload fields | Notes |
|---|---|---|
| `session` | `action` (`start`, `end`, `pause`, `resume`), `reason` (`user_stopped`, `syrup_closed`, `window_lost`, `focus_lost`, `recording_ended`, `crashed`, `unknown`) | A pause for `window_lost`/`focus_lost` means the system lost sight of the game. |
| `observation` | `components[]`: `name`, `status` (`observed`, `unknown`, `not_visible`), `value` (`{"number"}`, `{"category"}`, `{"flag"}`, `{"text": UntrustedText}` or `null`), `provenance` | `name` is a closed list: `hp_percent`, `mp_percent`, `exp_percent`, `level`, `job_class`, `map`, `screen`, `quest_state`, `dialog_text`, `screen_text`, `npc_visible`, `portal_visible`, `boss_hp_percent`, `menu_open`, `item_count`. There is no component for a character name, chat, party or guild. `observed` ⇔ a value; `unknown`/`not_visible` ⇔ `null`. Per-component certainty is its provenance's `confidence` (calibrated) or `null`. |
| `task` | `action` (`goal_set`, `goal_changed`, `constraint_added`, `attempt_started`, `goal_abandoned`), `goal` {`kind`, `origin` (`explicit`/`inferred`), `target` (UntrustedText), `constraints[]`}, `constraint`, `provenance` | An explicit goal is `human_asserted`, an inferred one `model_inferred` (checked). Constraints: `no_spoilers`, `hint_only`, `find_myself`, `no_purchase`. |
| `help` | `kind` (`question`, `hint_request`, `stuck`, `clarification`), `channel` (`voice`, `text`, `button`), `text` (sanitized or `null`), `provenance` | |
| `assistant` | `kind` (`answer`, `hint`, `warning`, `clarifying_question`, `refusal`), `in_reply_to` (help `event_id`), `based_on[]` (observation `event_id`s), `sources[]` {`kind`: `knowledge_base`, `web_lookup`, `model_knowledge`, `screen_reading`, `player_notes`; `reference`; `version`}, `text`, `displayed_at_ms`, `display_ms`, `constraints_respected`, `provenance` | `displayed_at_ms` `null` = not known to have been shown. Shown is not heard; heard is not followed. |
| `player_action` | `kind`, `follows_advice` (advice `event_id`), `measurement` (`self_report`, `inferred_from_state`, `publisher_sdk`, `licensed_replay`), `limits[]` (`timing_approximate`, `not_directly_observed`, `partial_view`, `self_reported`), `provenance` | MapleSyrup captures no input. `self_report` ⇒ `human_asserted`; `inferred_from_state` ⇒ `model_inferred`; only a publisher's SDK or licensed replay is `publisher_ground_truth` (checked). An inferred movement is never presented as recorded input. |
| `correction` | `target_event_id`, `category` (`wrong_object`, `wrong_fact`, `ambiguous_direction`, `stale_source`, `missing_info`, `wrong_outcome`, `other`), `component`, `proposed_value`, `proposed_outcome`, `note` (sanitized), `provenance` | A claim about another event; it never overwrites it. |
| `feedback` | `kind` (`helped`, `not_helpful`, `dont_interrupt`, `wrong`, `goal_reached`), `about_event_id`, `provenance` | Self-report. Satisfaction is not correctness. |
| `outcome` | `result` (`success`, `failure`, `aborted`, `unobserved`, `censored`), `reason` (`goal_reached`, `died`, `timed_out`, `gave_up`, `window_lost`, `syrup_closed`, `recording_ended`, `other`), `observable_until_ms`, `provenance` | |
| `experiment` | `experiment_id`, `arm`, `assignment_probability`, `provenance` | Recorded only; P1 runs no experiment. |
| `capture_quality` | `status` (`ok`, `degraded`, `focus_lost`, `not_in_view`, `login_screen`, `detection_failed`, `frames_dropped`, `clock_drift`), `dropped_frames`, `clock_offset_ms` | `focus_lost`/`not_in_view`/`login_screen` mean sight lost; `ok` regained. |

**Consent and security events are not research events.** They go to the separate ledger (§7), never to the
spool.

## 5. Provenance

Every observation component, action, label and outcome carries:

| Field | Values |
|---|---|
| `source_type` | `publisher_ground_truth`, `direct_observation`, `human_asserted`, `human_reviewed`, `model_inferred`, `synthetic`, `unknown` |
| `confidence` | `{"value": 0..1, "calibration_ref": token}` **only** when the number was calibrated against a named calibration; else `null`. An uncalibrated score cannot be represented. |
| `evidence_ref` | token (a frame reference, a document, an event) or `null` |
| `annotator_type` | `publisher`, `detector`, `player`, `human_reviewer`, `model`, `rule`, `generator`, `unknown` |
| `verification_status` | `unverified`, `reviewer_verified`, `gold`, `rejected` |
| `producer_version` | token: the producing process and its version (`name/version`) |

`Provenance::check` (applied by the recorder and again by the builder) refuses incoherent provenance:
`human_reviewed` needs a `human_reviewer` and evidence; `publisher_ground_truth` needs the `publisher` and
evidence; `human_asserted` is the `player`'s; `model_inferred` comes from a `model` or a `rule`;
`synthetic` from a `generator`; **`reviewer_verified` and `gold` exist only on `human_reviewed` or
`publisher_ground_truth` labels**. So a model's inference can never call itself verified, and a player cannot
dress a claim as a review. A review, when P2 has reviewers, is a *new* label with its own provenance and a
reference to the label it checks — the original stays as it was.

OCR readings and a player's confirmation are not truth: they stay `direct_observation` / `human_asserted`,
`unverified`.

## 6. Texts and the sanitizer

`sanitize.rs`, applied by the recorder before anything is kept, and again by the export:

- **Dropped whole** (withheld: the text becomes `null`, the kind `chat` is recorded): anything read from the
  chat window or a whisper (`origin` `chat_window`/`whisper`), and any line shaped like chat — `[Name] …`,
  `<Name> …`, `Name : …`, `From Name: …`, `To Name: …`, `>> …`.
- **Replaced inside a kept text**: the player's own character names (`{name}`; the recorder is given them and
  never writes them), links (`{url}`), e-mail addresses (`{email}`), handles — `@someone`, `name#1234` —
  (`{handle}`), anything that looks like a credential — `password: …`, `my password is …`, `token=…`, known
  key prefixes, long mixed strings — (`{credential}`), phone-like numbers (`{number}`). Over 400 characters is
  cut (`truncated`).
- Each text records the **kinds** redacted (`redactions`), never what was removed.
- **Game text is untrusted**: `UntrustedText` {`untrusted_text`, `origin`, `redactions`, `instruction_like`}.
  Text that reads like an order to the system ("ignore previous instructions", "grant consent", "export
  everything", "mark as gold", …) is flagged `instruction_like` and kept as data; nothing in the module changes
  a permission, consent, label or export because of what a text says (tested).

Limits, stated: other players' names are caught only through the chat's shape — a name typed into the
player's own words is caught only when it is one of the player's own; false positives fall on the side of
privacy (a quest line shaped like chat is withheld). The sanitizer is idempotent.

## 7. Consent

**Purposes** (`Purpose`, each its own consent, all off unless given): `service_operation` (running what the
player asked for — inference; **never** a source for the research store: the recorder refuses it),
`improve_syrup`, `aggregate_analytics`, `external_research_training`, `media_donation` (no path exists in P1:
refused). **Data types** (`DataType`): one per family — `session_metadata`, `gameplay_state`, `goals`,
`help_requests`, `assistant_advice`, `player_actions`, `corrections`, `feedback`, `outcomes`,
`experiment_assignments`, `capture_quality` — plus `media` and `voice` (no path in P1). **Recipient
classes**: `this_device`, `syrup_team`, `external_researcher`, `licensed_buyer`; a recipient also has its own
id, to which its pseudonyms are scoped.

A **`ConsentReceipt`**: `receipt_id`, `research_subject_id`, `text_id`, `text_version`, `text_language`,
`text_sha256` (of the exact text shown), `given_at`, `source` (surface and version), `scope` (a list of
{`purpose`, `data_types[]`, `recipients[]`}), `epoch` (set by the ledger: the subject's last epoch + 1),
`age_assurance` (`self_declared_adult` or `not_assured`; research is adults-only and a non-adult receipt is
refused — whether self-declaration is proportionate is for counsel). The slice ships only a **draft** text
(`RESEARCH_CONSENT_DRAFT_EN`, version `0.1-draft`), not reviewed by counsel and shown to nobody.

**The ledger** (`ConsentLedger`, one JSON line per entry, at a path of its own — apart from the research
folder): `granted` (the receipt), `withdrawn` {`receipt_id`, `research_subject_id`, `at`, `epoch`},
`subject_deleted` {`research_subject_id`, `at`, `events_removed`, `episodes_removed`, `exports_affected[]`},
`export_written` {`export_id`, `revision`, `at`, `purpose`, `recipient_class`, `recipient_id`, `rows`}. It is
written only when a person gives or withdraws consent, or data is deleted or exported: with consent off, it
does not exist (tested). A withdrawal is a receipt beside the grant, not a deletion of it. Open question for
counsel: whether the deletion record should keep the pseudonymous subject id or a hash of it.

## 8. Rights manifests

A **`RightsManifest`** per title and deliverable (`event_metadata` or `media`): `rights_policy_id`,
`game_id`, `deliverable`, `rights_holder`, `status` (`approved`, `requires_title_specific_review`,
`denied`), `basis` (the evidence of permission or approved analysis, or `null`), `applies_to` (versions,
worlds), `grants[]` {`purpose`, `recipients[]`, `uses[]` of `capture`, `store`, `annotate`,
`train_internal`, `train_external`, `evaluate`, `transfer`}, `territory` (recorded; not enforced in P1, which
has no recipient off this machine), `valid_from`, `valid_until` (every policy expires), `revoked_at`,
`reviewed_by_role`, `notes`.

Built in (`RightsRegistry::builtin`):

| Policy | Title | Status | Allows |
|---|---|---|---|
| `rights-maplestory-review-required-v1` | `maplestory` | `requires_title_specific_review` | nothing |
| `rights-maplestory-worlds-review-required-v1` | `maplestory_worlds` | `requires_title_specific_review` | nothing |
| `rights-synthetic-local-demo-v1` | `synthetic` | `approved` (generated data, no game content, no player) | `improve_syrup`, `aggregate_analytics`, `external_research_training` → `this_device` only; capture, store, annotate, evaluate, transfer; 2026-10-01 to 2027-10-01 |

The MapleStory entries follow the owner's directive §10: Nexon publishes explicit restrictions on commercial
use of gameplay videos and on selling licences to them, so both titles are marked for title-specific review,
not approved; a player's consent or the code's MIT licence grants no rights in the game's images, music,
third-party content or other players' data, and derived data is not automatically exempt. The Nexon pages the
directive lists (IP Guide for Content Creators; VOD and Streaming policy; MapleStory Worlds terms of
13 August 2025) were **not read for this document — unverified here**; [CONSENT_AND_RIGHTS](CONSENT_AND_RIGHTS.md)
is where they are read and quoted. Unknown titles have no manifest: refused (default deny).

## 9. The gate, and where it is applied

| Check | Recorder open | Every `record` | Every `flush` | Every exported row |
|---|---|---|---|---|
| Research purpose (not `service_operation`; `media_donation` has no path) | ✓ | | | |
| Consent in force (not withdrawn), adult | ✓ | ✓ | ✓ | ✓ |
| Consent covers the purpose | ✓ | ✓ | ✓ | ✓ |
| … the data type | | ✓ | | ✓ |
| … the recipient class — now **and** in the receipt the row was collected under (no widening backwards) | | | | ✓ |
| Rights: manifest exists, `approved`, not revoked, `valid_from ≤ t < valid_until` | ✓ | ✓ | ✓ | ✓ |
| Rights license the use: capture + store for the purpose / transfer to the recipient class | ✓ | ✓ | ✓ | ✓ |

Each check uses the time of the check. A refusal at open returns `None` and writes nothing. A refusal while
recording that ends the recorder (consent withdrawn or narrowed, rights expired, revoked or changed) **cancels
the pending batch** — nothing of it is written — and refuses everything after. A row refused at export is left
out and counted by reason (`manifest.excluded`, counts only); when no row passes, there is no export and no
folder (`nothing_eligible`, with the reasons). Refusal codes: `no_consent`, `consent_withdrawn`,
`purpose_not_consented`, `data_type_not_consented`, `recipient_not_consented`, `not_adult`, `receipt_mismatch`,
`not_a_research_purpose`, `media_path_not_built`, `no_rights_manifest`, `rights_not_approved`,
`rights_revoked`, `rights_not_yet_valid`, `rights_expired`, `use_not_licensed`, `invalid_event`, `io`,
`nothing_eligible`, `export_exists`, `real_data_needs_random_keys`.

## 10. The episode

An episode is one attempt at one goal, not a time window (directive §4). `episode::build` turns events (any
order, with copies) into episodes:

1. **Validate** each event (`Event::validate`: schema, family, tokens, probabilities, provenance, no chat
   kept); an invalid one is quarantined (`quality.quarantined.invalid_event`).
2. **Deduplicate by `event_id`**: exact copies count once (`duplicates_dropped`); two different events under
   one id are both kept out (`conflicting_duplicates`) — neither can be believed.
3. **Order** by `sequence_no` within each session; sessions by when first ingested. Gaps and a clock that goes
   back are counted (`sequence_gaps`, `clock_anomalies`). The same events in any order build byte-identical
   episodes (tested).
4. **Assemble** each (`research_subject_id`, `episode_id`):

| Field | How |
|---|---|
| `goal` | The first `goal_set` task, with its provenance (explicit = `human_asserted`, inferred = `model_inferred`). `goal_changes` counted. |
| `constraints` | The goal's plus `constraint_added`. |
| `state_before` | The last observation of the session **before** the episode began — never a later one. |
| `help_requests`, `actions`, `feedback`, `experiments` | The episode's events, stamped (`event_id`, `at_ms`). |
| `advice` | Each advice with its sources, model version, display time — and `based_on` kept only for observations of the same session made **no later than the decision** (`displayed_at_ms`, else the advice's own time); a later one is dropped (`rejected_inputs`, `quality.future_inputs_rejected`). |
| `attempts` | Closed by an `outcome` event (or `goal_abandoned` → `aborted`). A new attempt opens on help, an action or `attempt_started` after a closed one. Each has its result, reason, `determined_by` (the event and its provenance, or the builder's rule and its evidence event), `observable_until_ms`, and its counts of help requests, advice shown, and advice whose showing is not known (`advice_display_unknown`). |
| attempt left open | **Not a failure.** `unobserved` (rule `view_lost_before_outcome`, `observable_until_ms` = when sight was lost) if the session lost sight of the game (pause for window/focus lost, capture `focus_lost`/`not_in_view`/`login_screen`) and did not regain it before the end; `censored` (rule `observation_ended_before_outcome`) if the session ended (Syrup closed, recording ended, stopped) while it still saw; `unobserved` (rule `no_outcome_in_data`) if the data simply stops. |
| `outcome` | The last attempt's. |
| `state_after` | The first observation after what determined the outcome. |
| `corrections` | Every correction of the episode, **as a claim**: `status` `claimed` (unverified), `verified` (only a reviewer's or the publisher's label) or `rejected`; `applied` is always `false` in P1 — what it targets is kept as it was. |
| `time_to_first_success_ms` | From the goal (else the first event) to the first success, within one session; `null` otherwise. |
| `assisted` | `true` if advice was shown (`displayed_at_ms` known) before the first success (or the end); `false` if no advice came before; `null` if advice came but whether it was shown is not known. |
| `capture_issues`, `instruction_like_texts` | Counts. |
| `lineage` | The episode's `event_ids` in order, its `consent_receipt_ids`, its `rights_policy_ids`. |

## 11. The research folder and the export

On this machine (`store.rs`), created by the first batch a recorder writes — never by reading, never with
consent off:

```text
<root>/spool/events.jsonl            sanitized events as recorded (layer 1: sanitized data received)
<root>/episodes/episodes.jsonl       built episodes (layer 2)
<root>/lineage/exports.jsonl         each export: folder, purpose, recipient, revision, and subject → pseudonym
<root>/lineage/recipient-keys.json   each recipient's random pseudonym key (never exported)
<ledger path>                        the consent and security ledger, apart (§7)
```

The **local export** (`export::export_local`, into a new or empty folder; written beside it and moved into
place whole):

| File | Content |
|---|---|
| `events.jsonl` | The eligible events, sanitized again, identifiers pseudonymized. |
| `episodes.jsonl` | Episodes built from the eligible events only, pseudonymized. |
| `REPORT.md` | The first report (§12), marked SYNTHETIC when every row is. |
| `DATA_CARD.md` | What it is, contents with checksums, counts, consent and rights summary, intended and prohibited uses, limits and biases, retention and deletion, how to load. |
| `manifest.json` | `manifest_version`, `schema_version`, `export_id`, `revision`, `created_at`, `synthetic`, `purpose`, `recipient_class`, `recipient_id`, `files[]` {`path`, `sha256`, `bytes`, `rows`}, `counts` {`participants`, `sessions`, `episodes`, `events`, `observed_hours`, `sessions_without_known_duration`}, `consent_and_rights` {`consent_texts`, `age_assurance`, `rights[]`, `checked_at`}, `excluded` (rows left out, by reason), `quality` (the builder's counts), `deletions_applied`, `loader`. No participant, no path. |

**Pseudonyms.** Every identifier (`research_subject_id`, `session_id`, `episode_id`, `event_id`,
`consent_receipt_id` and every reference to them) becomes `<kind>-<20 hex>` = HMAC-SHA-256 under the
recipient's own key. Two recipients' copies share no join key; only the producer's lineage links back. Real
data gets a random key per recipient; a reproducible key (derived from the recipient id) is refused unless every
row is synthetic (`real_data_needs_random_keys`). Pseudonymous is not anonymous (GDPR terms: still personal
data). Exact timestamps are kept in P1 — coarsening them for buyers is an open item.

**Verification.** `export::verify` and `tools/research_loader.py` check every listed file's SHA-256, size and
row count, the manifest and schema versions, and refuse on any mismatch (the loader exits 2 with the reason).
The manifest itself is not signed in P1 (checksums only).

## 12. The first report

`report.rs` computes, on the exported episodes, four metrics of [METRICS_CATALOG](METRICS_CATALOG.md), each
with numerator, denominator and distinct participants, unweighted (P1 keeps every event, probability 1):

- **M1.1 `time_to_first_success`** — only the builder's cross-check the catalog names: per episode, from the
  goal to the first success within one session (median, min, max); an episode without a success is not a
  time. The catalog's Kaplan–Meier estimate over a subject's attempts is not computed in P1.
- **M1.2 `success_by_assistance`** — per attempt: `assisted` = advice shown during the attempt; successes over
  attempts whose outcome was seen (`success`, `failure`, `aborted`), per arm. An attempt whose advice may or
  may not have been shown is in neither arm (counted apart), never "unassisted".
- **M1.4 `help_request_rate_per_attempt`** — attempts with ≥ 1 request over all attempts; and requests in all.
- **M1.5 `unobserved_outcome_rate`** — attempts `unobserved`, and attempts `censored`, each over all attempts.

Below 30 participants the report says **insufficient evidence** and draws no comparison; it states that
assisted vs unassisted is not the effect of help. Where the catalog and this code differ, the catalog governs
and the code follows in its next revision.

## 13. Deletion and lineage

`ResearchStore::delete_subject` removes the subject's events from the spool, their episodes, and — through
the lineage — their rows from every export (matched by that export's pseudonym), then makes each export whole
again: report, data card, counts and checksums recomputed, `revision` + 1, `deletions_applied` + 1. It returns
the affected exports (id, folder, rows removed, revision, or `missing` when the folder is gone) and appends a
`subject_deleted` entry to the ledger. It is idempotent. It cannot reach copies already delivered (the
agreement must) nor undo a model already trained on the rows. Withdrawal is not deletion: withdrawal stops
collection and cancels the pending batch; the participant chooses deletion separately.

## 14. Acceptance (each a test in `tests/research_slice.rs`)

| Row (directive §17, w46 brief) | Test |
|---|---|
| Consent off → no recorder, nothing written, no research folder | `consent_off_makes_no_recorder_writes_nothing_and_creates_no_folder` |
| … and the module has no network code (no sockets, no subprocess, no HTTP, nothing else of the crate) | `the_research_module_has_no_network_code_and_reaches_nothing_outside_itself` |
| Withdrawal with a batch pending cancels it | `withdrawal_with_a_batch_pending_cancels_it` |
| An expired rights policy refuses (recording, an open recorder, export) | `an_expired_rights_policy_refuses_recording_and_export` |
| An export for a purpose or recipient not consented or not licensed refuses | `an_export_for_a_purpose_or_recipient_not_consented_or_not_licensed_refuses` |
| MapleStory (and Worlds) data refused even with full consent | `maplestory_data_is_refused_even_with_full_consent` |
| A name, chat line or credential never reaches a written file | `a_name_a_chat_line_or_a_credential_never_reaches_a_written_file` |
| Duplicate and out-of-order events build the same episode | `duplicate_and_out_of_order_events_build_the_same_episodes` |
| `unknown` never becomes 0 | `unknown_never_becomes_zero` |
| `model_inferred` never becomes `human_reviewed`/gold | `model_inferred_never_becomes_human_reviewed_or_gold` |
| A poisoned correction is kept as a claim, not applied | `a_poisoned_correction_is_kept_as_a_claim_not_applied` |
| Deleting a subject removes events, episodes, export rows; lists the exports | `deleting_a_research_subject_removes_their_events_episodes_and_export_rows_and_lists_the_exports` |
| The manifest's checksums match; the loader verifies them | `the_manifest_checksums_match_and_the_loader_verifies_them` |
| In-game text that gives orders is untrusted data and changes nothing | `in_game_text_that_gives_orders_is_kept_as_untrusted_text_and_changes_nothing` |
| A window loss or Syrup closing is not a failure | `a_window_loss_or_syrup_closing_is_not_a_failure` |
| No later observation is an input to earlier advice | `no_later_observation_is_an_input_to_earlier_advice` |
| Every event carries the whole envelope; this document lists it | `every_event_carries_the_whole_envelope_and_the_contract_document_lists_it` |
| The committed fixtures are what the generator makes (and are small) | `the_committed_fixtures_are_what_the_generator_makes` |

"Zero leaks in these tests" is an acceptance condition, not a proof that production cannot leak.

**Fixtures** (`tests/fixtures/research/`, ~130 KB, all synthetic): `spool-events.jsonl` (the spool after a
retry's faults — one duplicate line, one line out of order) and `sample-export/` (a complete export with its
manifest, data card and report). `RESEARCH_FIXTURES=write cargo test --release --offline --test research_slice`
rewrites them; the test fails if they drift from the generator.

## 15. Not in P1, and open questions

Not built: any upload or server (P2: authentication, re-check, schema validation, idempotent ingest,
quarantine, catalog); a media path (`media_donation`, clips, voice); sampling beyond "every event of the
families"; real producers (nothing in the app calls this module); reliable title/build/world detection; a
reviewer workflow (no `human_reviewed` label exists); applying verified corrections in a resolved view;
territory enforcement; small-cell suppression and other protections for aggregate analytics shared outside;
timestamp coarsening for buyers; a signed manifest; RLDS/Parquet adapters; retention timers; train/test splits.

For the owner and counsel (not legal advice): the consent texts and whether a self-declared 18+ is
proportionate; whether the deletion record keeps the pseudonymous subject id; the retention of the ledger and
of the lineage (which holds the join keys); whether exact timestamps in a buyer's export make it identifiable
in combination with a publisher's logs; and, before any real title, a reviewed rights basis per title and
deliverable.
