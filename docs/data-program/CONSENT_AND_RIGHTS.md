# CONSENT_AND_RIGHTS — who may use what, for which purpose, on which rights

<div dir="rtl" lang="he">

**בקצרה, למיכאל**

- הרשאה אינה מתג אחד. יש חמישה מסלולים נפרדים: תפעול השירות, שיפור Syrup, אנליטיקה מצרפית חיצונית, מחקר ואימון חיצוניים ותרומת מדיה. כל הרשאה היא צירוף של מטרה × סוגי נתונים × נמענים. כל מסלולי התרומה כבויים כברירת מחדל ופתוחים לבגירים בלבד, ואין מסלול לילדים.
- כל הסכמה נשמרת כקבלה (גרסת הנוסח, מועד, מקור והיקף) ונבדקת מחדש בכל הקלטה, עיבוד, אימון ויצוא. שימוש במוצר, מפתח API או הפעלת הקלטה אינם הסכמה. אין העלאה רטרואקטיבית של memory.json, knowledge.json, log.txt, mic.wav או סרטונים ישנים, ו״תזכור״ אינו היתר לפרסם או למכור.
- ביטול עוצר העלאות ומבטל batches ממתינים. המחיקה מגיעה לאירועים, למדיה, ל-embeddings ולנגזרות, לגיבויים לפי מחזור ולעותקים אצל לקוחות לפי חוזה. היא לא מגיעה למשקלי מודל שכבר אומן, וזה נאמר למשתתף מראש.
- MapleStory ו-MapleStory Worlds מסומנים `requires_title_specific_review`. לא הצלחתי לקרוא את דפי Nexon כי בקשת ההרשאה לגישה לאתר לא נענתה בזמן, ולכן כל מה שכתוב בהם מסומן כאן ״לא אומת״. בלי בסיס זכויות מאושר, הכמות המסחרית מהם היא אפס. הפיילוט המועדף: אולפן שותף שנותן זכויות כתובות, או סביבה בבעלותנו.
- השאלות לעורך הדין (חוק הגנת הפרטיות ותיקון 13, GDPR, COPPA, CCPA, מעמד אפשרי של סוחר מידע ותנאי Nexon) מרוכזות ב-§7. הן מנוסחות כשאלות ולא כתשובות, וזה אינו ייעוץ משפטי.

</div>

> **Not legal advice.** This is an engineering design for P0, written by engineers. Every legal point is a
> question for counsel, not an answer.
>
> **Status (2026-10-10).** Nothing here is deployed and no data is collected for the program. The product
> does not offer sharing (a351a9f; [CURRENT_STATE_AUDIT](CURRENT_STATE_AUDIT.md) §5). The P1 local slice
> (`src/research/`, being written in parallel) implements the gate of §2, §3 and §8.5 on synthetic data
> only. The names used here (`Purpose`, `RecipientClass`, `RightsStatus` and the refusal codes) follow its
> tests as they stand today; [DATA_CONTRACTS](DATA_CONTRACTS.md) is the source of truth for types.
>
> **Sources.** I could not fetch any external source. Every WebFetch (the Nexon pages, the EDPB opinion,
> the Amendment 13 guide) stopped at a permission request to the user that was not answered in time
> (`PROVENANCE_REQUIRED`), and I did not route around it. **Everything this document says about Nexon's
> pages or any law is unverified.** An article number given here is a pointer for counsel, not a quotation.

Related documents: [CURRENT_STATE_AUDIT](CURRENT_STATE_AUDIT.md) (what exists today: §4 data flows, §5 play
stats), [DATA_CONTRACTS](DATA_CONTRACTS.md) (types), [DATA_ARCHITECTURE](DATA_ARCHITECTURE.md) (where the
gate sits on the client and the server), [THREAT_MODEL](THREAT_MODEL.md) (attacks on the gate),
[DATA_QUALITY_AND_BIAS](DATA_QUALITY_AND_BIAS.md) (what opt-in does to the data),
[B2B_PRODUCT_STRATEGY](B2B_PRODUCT_STRATEGY.md) and [UNIT_ECONOMICS](UNIT_ECONOMICS.md) (what rights mean
commercially), [`../data-and-metrics.md`](../data-and-metrics.md) (the play stats).

---

## 0. The rules this document implements

1. **Three flows stay apart**: the inference service, the improvement contribution and commercial
   transfer. (§1)
2. **A permission is purposes × data types × recipients**, not one boolean. (§2)
3. **Contribution is off by default, commercial transfer is off by default, and contribution is for adults
   only.** No track for children. (§2, §6)
4. **No implied consent**: using the product, an API key, a recording, the play stats or "remember" are not
   consent. (§1.1)
5. **A receipt for every decision** (text version, time, source, scope), **re-checked at every step**:
   recording, processing, training, export. (§3)
6. **Nothing retroactive.** Files that existed before a grant never become contributions. (§4.3)
7. **Withdrawal stops and cancels**; deletion reaches every layer it can, and says plainly where it cannot
   (trained weights). (§5)
8. **Rights per game × artifact type.** With no approved basis there is no marketing and no export. A
   player's consent and our MIT licence grant nothing in the game's images, music or other players' data.
   (§8)
9. **Basic use and local fixes for everyone**, contributor or not. (§4.1)

---

## 1. Three flows that never share a switch

| Flow | What it is | Today ([CURRENT_STATE_AUDIT](CURRENT_STATE_AUDIT.md) §4.1) | Purpose | May it feed the research store? |
|---|---|---|---|---|
| **Inference**, the service the player asked for | the conversation, a game frame and the notebook go to a model provider to produce an answer; the answer goes to a voice provider | OpenAI or xAI on every reply (notebook included), ElevenLabs on every spoken line, the phone browser's speech service (Google or Apple), Cloudflare in tunnel mode, the coding agent's vendor in the workshop | `service_operation` | **Never.** The P1 recorder refuses this purpose (`not_a_research_purpose`). |
| **Improvement contribution** | the company uses selected, sanitized episodes and corrections to improve Syrup and MapleSyrup | does not exist (no upload code; `TODO(upload)` in `src/metrics.rs`) | `improve_syrup` | yes, under its own grant |
| **External and commercial transfer** | aggregates, episodes, evaluation items or media that leave the company for a third party | does not exist; the play-stats "sharing" was withdrawn in a351a9f | `aggregate_analytics`, `external_research_training`, `media_donation` | yes, under their own grants **and** the rights manifest (§8) |

Consequences:

- **A frame sent for an answer is not a frame contributed.** Inference data is processed by the provider
  under the provider's terms and, today, under the player's own key. Who is the controller of that flow is a
  question for counsel (§7, Q-G1).
- **Contribution never rides on the inference call.** Research events are built locally from what the
  client has already observed. In P2 they go to our ingest service. They never pass through an inference
  provider, the phone, or the Cloudflare tunnel ([THREAT_MODEL](THREAT_MODEL.md) T16).
- **A labeling model is a recipient.** If a cloud model is later used to label research data, its provider
  receives research data. It must then be a contracted processor, named in the track's recipient register
  ([THREAT_MODEL](THREAT_MODEL.md) T18).

### 1.1 What is not consent

| Act | Is not consent to |
|---|---|
| Installing or using MapleSyrup | any contribution track |
| Adding an OpenAI, xAI or ElevenLabs key | anything beyond service operation with that provider |
| Turning on a recording or marking a moment | uploading the recording or the marks: recordings stay on the PC |
| "Remember…", teaching a fact, correcting a reading | publishing, selling or contributing it: the notebook is personal and is not a shared training set |
| The play stats, which are kept locally | sharing them (sharing is not offered: a351a9f) |
| Accepting the product's terms of use | any contribution track. Track decisions are separate from the terms; whether they may be bundled at all is a question for counsel (§7, Q-G2) |
| Ticking "I'm 18 or older" | anything: it is an age declaration, not a grant |
| Joining the beta or the Discord server | any contribution |

---

## 2. The permission model

### 2.1 The five tracks (purposes)

| Track (`Purpose`) | What it allows | Default | Recipients it can ever name | Data types it can ever cover | Adults only | Rights manifest |
|---|---|---|---|---|---|---|
| `service_operation` | running what the player asked for: inference, voice, the local stats, and a crash report the player chooses to send (launch plan [2.3](../launch-plan/tasks/2.3-telemetry-and-crashes.md)) | on while a feature is used, with notice naming every provider; turning it off turns that feature off | this device; the named inference, voice and crash-report providers | what each feature needs, disclosed per provider | no (the product's own age terms apply) | the product's own terms-of-service question (launch plan task 1.4.3) |
| `improve_syrup` | the company uses selected, sanitized structured episodes, corrections and feedback to improve detectors, thresholds, templates and knowledge ([SELF_IMPROVEMENT_LOOP](SELF_IMPROVEMENT_LOOP.md)) | **off** | the company; its contracted processors | structured events, corrections, feedback, self-reports; **no media** | yes (MVP) | yes: capture, storage, annotation, internal training |
| `aggregate_analytics` | aggregated statistics, thresholded and suppressed ([THREAT_MODEL](THREAT_MODEL.md) §6.6), shared outside the company, e.g. a studio friction report (product A) | **off** | the company; external recipients of **aggregates only** | structured events, released only as aggregates | yes | yes: capture, storage, transfer of aggregates |
| `external_research_training` | episode-level, pseudonymous, sanitized records given or licensed to external researchers and AI companies for training and evaluation (products B, C, D) | **off** | non-commercial researchers and licensed (paying) buyers, **each chosen separately** | structured events and episodes, including the player's own help and correction text after sanitization; **no media** | yes, with stronger assurance (§6) | yes, for every use: external training, transfer, territory, term |
| `media_donation` | sanitized frames and clips of the game window, through a separate media path ([DATA_ARCHITECTURE](DATA_ARCHITECTURE.md)) | **off**; not built in P1 (`media_path_not_built`) | as for research, each chosen separately | media frames and clips; never raw audio in the MVP | yes, with stronger assurance | yes, and the strictest: media carries the publisher's images and music |

- **"Commercial" is a recipient choice inside the tracks**, not a track of its own. A participant can say
  yes to non-commercial research and no to paying buyers. The consent text names the recipient classes and
  says whether anyone pays for the data and whether the participant is paid.
- The five tracks are the directive's (§10). Their identifiers follow the P1 code's `Purpose`.

### 2.2 Data types

The identifiers are the P1 code's `DataType` (one per event family, plus media and voice).

| Data type | Examples | Tracks that can cover it | Never |
|---|---|---|---|
| `session_metadata` | app version, platform, UI language without region, screen size class | all | install path, PC name, Windows user name |
| `gameplay_state` | structured readings with per-component certainty: HUD values, map id, UI state, `observation_coverage` | improve, aggregate, research | — |
| `goals` | a goal picked from a list or confirmed by the player; constraints such as "no spoilers" or "hint only" | improve; aggregate (as categories); research | an inferred goal presented as stated: it is marked `inferred` |
| `help_requests` | the help request's category and sanitized text | improve, research | transcripts as such; voice |
| `assistant_advice` | the advice, its sources, model version and display time | improve, research | the model's whole input (frame and notebook) |
| `player_actions` | what the player says they did, or a change of state from which an action is inferred (marked `inferred`) | improve, research | input captured from the keyboard or mouse: there is none (no keylogger: audit §3) |
| `corrections` | "that's wrong" with a category and a correction | improve; aggregate (as categories); research | — |
| `feedback` | "that helped", "don't interrupt"; satisfaction; reasons the player chose to give; frustration only as self-report | improve; aggregate (as categories); research | anything inferred from face, voice or click rhythm (directive §5) |
| `outcomes` | success, failure, aborted, unobserved, censored, with how each was determined | improve, aggregate, research | an unobserved ending recorded as a failure |
| `experiment_assignments` | the arm and its allocation probability ([DATA_QUALITY_AND_BIAS](DATA_QUALITY_AND_BIAS.md) §8.2) | improve, research | — |
| `capture_quality` | coverage, dropped frames, rejections | all contribution tracks | — |
| `media` | sanitized frames and clips of the game window | `media_donation` only (no path in P1) | other windows, the desktop, login screens, chat, names |
| `voice` | — | **none in the MVP** (§10; no path in P1) | — |

Derived artifacts (labels, embeddings, features, aggregates) are not a data type of their own: each follows
the grants of the data it was computed from, through lineage. Two kinds of data are **outside every track**
and have no data type: the notebook (`memory.json`, `about-me.txt`, `knowledge.json`), which serves
`service_operation` only (today it goes to the inference provider with every reply: audit §4.1), and the
legacy local files (`log.txt`, `mic.wav`, recordings, `markers.csv`, `mark-*.png`, `learned/` crops,
workshop logs), which never become contributions (§4.3).

### 2.3 Recipients

| Recipient class | Who | Tracks |
|---|---|---|
| `this_device` | the player's PC | all |
| `syrup_team` | the operator of Syrup and MapleSyrup, through logical roles (Product B2B, Computer Vision, Data Engineering, Model Evaluation, Statistics, Privacy & Security) | improve and the external tracks |
| `processor` | contracted vendors acting for the company: hosting, storage, labeling, payouts | improve and the external tracks; each named in the recipient register (§4.2) |
| `inference_provider` | OpenAI, xAI, ElevenLabs, the phone browser's speech service, Cloudflare in tunnel mode | `service_operation` only |
| `external_researcher` | a non-commercial research recipient | aggregate, research, media |
| `licensed_buyer` | a paying licensee | aggregate, research, media; consented separately from researchers |

The P1 code's `RecipientClass` has `this_device`, `syrup_team`, `external_researcher` and
`licensed_buyer`. `processor` is a proposal for P2, when vendors first receive data. `inference_provider`
names the service path only and need not exist in the research code, which never sends anything there.
Both are [DATA_CONTRACTS](DATA_CONTRACTS.md) decisions.

### 2.4 What a grant is, and how it is asked

- A grant is a set of (purpose, data type, recipient class) triples, plus the term and the retention period
  stated to the participant (§10).
- **Default deny.** What is not granted is refused. A track never implies another: research does not imply
  aggregate analytics, and media does not imply research.
- **Narrower wins.** A later, narrower grant replaces a broader one from that moment on.
- **Asked one track at a time.** The participant sees five cards, one per track. Each has a fixed,
  plain-language bundle of data types and recipients, a "Yes" and a "No thanks" of equal weight, and nothing
  pre-ticked. An "advanced" view shows the full matrix. Five decisions with fixed bundles, not 5 × 11 × 6
  checkboxes: granularity that nobody can read is not informed consent.

Example: what three grants permit.

| Participant's grant | Improve Syrup | Aggregate report to a studio | Episodes to a university lab | Episodes to a paying AI company | Frames to anyone |
|---|---|---|---|---|---|
| none (most players) | no | no | no | no | no |
| `improve_syrup` only | yes (structured, no media) | no | no | no | no |
| `improve_syrup` + `external_research_training` (researchers only) | yes | no | yes, if the rights manifest allows it | **no** | no |

---

## 3. Consent receipts and the ledger

### 3.1 The receipt (proposal; the types are in [DATA_CONTRACTS](DATA_CONTRACTS.md), the P2 table in `migrations/0001_research_catalog.sql`)

| Field | Meaning |
|---|---|
| `receipt_id` | random id |
| `research_subject_id` | pseudonymous and random, made at the first grant. It is never the play-stats id, a telemetry or crash-report id, an account id or a payout id, and the research store holds no key that links them |
| `action` | `grant`, `narrow`, `withdraw`, `renew` |
| `scope` | the (purpose, data type, recipient class) triples |
| `text_id`, `text_version`, `text_sha256`, `language` | exactly what was shown |
| `shown_at`, `decided_at` | UTC from the device clock, checked against the monotonic clock; in P2 also the server's receipt time |
| `source` | the surface (phone page or PC), app version, commit, page build |
| `method` | the explicit act (a toggle plus a confirmation), never a default |
| `age_assurance` | the method and its result, with the time. The P1 code records `self_declared_adult` or `not_assured`; the stronger levels of §6.3 would add values (for example a third-party check). Never the document, never the birth date (§6) |
| `consent_epoch` | increases with every change for this subject. Each event carries the epoch it was collected under |
| `term` | until withdrawn, with a re-confirmation date (proposal: 12 months) |
| `retention` | the retention period stated to the participant for each track (§10) |
| `prev_receipt_id` | the chain |

### 3.2 The ledger

- **Append-only and separate.** In P1 it is a local file (`ConsentLedger`) outside the research store. In P2
  a server copy exists for contributing subjects only, in the accounts-and-consent store, which is kept
  apart from the research store ([DATA_ARCHITECTURE](DATA_ARCHITECTURE.md)).
- **Withdrawal appends; it never erases the fact of consent.** The play stats deleted `share.json` on
  withdrawal and so lost both the grant and the withdrawal (audit §5). The ledger keeps both.
- **Minimal, with its own retention (§10), and not a telemetry channel.** It holds decisions only, no usage
  data. When contribution is off, nothing about the player leaves the PC through it (directive §4).

### 3.3 Re-checked at every step

| Step | What is checked | Refusal (P1 code where it exists) |
|---|---|---|
| Open a recorder | a valid grant for this purpose, and rights that allow capture for this game | `no_consent`, `purpose_not_consented`, `not_a_research_purpose`, `rights_not_approved`, `rights_expired` |
| Write an event | still granted at write time; the epoch is current | `consent_withdrawn` |
| Flush a batch locally (P1), or upload it (P2) | the grant is valid now; the batch's events are inside the scope | `consent_withdrawn`, and the batch is cancelled |
| Ingest (P2) | the server ledger and the event's epoch; the event was made before any withdrawal | rejected and quarantined |
| Curate or label | the scope covers annotation; the rights allow annotation | excluded |
| A training or evaluation run | the scope covers the use; the snapshot is recorded for lineage (§5.2) | excluded |
| Export | purpose × recipient × data types × rights × territory × term | `nothing_eligible`, with the reasons per record (`purpose_not_consented`, `use_not_licensed`, `rights_not_approved`, `rights_expired`) |

### 3.4 When the text changes

- A change that widens the scope is a new version and needs a new decision. Data collected under the old
  text stays under the old scope.
- A typo or translation fix is a new version with the same scope. The hash differs and the ledger keeps both.
- Each language is its own text with its own hash. The Hebrew and English texts are reviewed to say the
  same thing.

---

## 4. What the player sees and controls

### 4.1 Basic use for everyone

- **No feature is withheld or degraded for saying no.** Local fixes work for everyone: the player's own
  corrections (`knowledge.json`), the learned sight, the notebook.
- **No "pay with data".** Incentives in the research panel (product D) pay for a study's time, never for a
  broader grant (§4.4).

### 4.2 See, stop, delete, understand

- **What was selected.** Before a batch leaves the PC (P2), the player can open "what will be sent": the
  episodes, their fields and, for media, a thumbnail of every frame, with "exclude this" on each item. In P1
  the local export is that view.
- **Stop.** One switch stops every contribution at once, and each track has its own switch.
- **Delete.** One item, one session, or everything (§5).
- **Who gets what, and why.** A recipient register lists, per track: the recipient classes, the named
  processors and (once there are any) the named buyers, the purpose, the retention and the territory. It is
  updated *before* a new recipient receives anything. A new recipient class is a scope change (§3.4).
- **History.** The player's own receipts, in their language.

### 4.3 Nothing retroactive

- These files never become contributions, even after a grant: `memory.json`, `about-me.txt`,
  `knowledge.json`, `log.txt`, `mic.wav`, recordings (`recording HH-MM-SS.mp4`), `markers.csv`,
  `mark-NNN.png`, the `learned/` crops, the workshop's job folders, and the play stats' `sessions.jsonl`.
  - They were made under the promise that they stay on the PC (`package/README.txt`, "Privacy").
  - They hold transcripts, voice, full-screen video with every PC sound, and personal facts (audit §4.2).
  - None of them carries the provenance an episode needs (audit §6).
- A grant covers only events the research recorder creates **after** the receipt's `decided_at`, in the new
  structured log.
- **"Remember" stays personal.** What the player asks MapleSyrup to remember stays on the PC. It also goes
  to the inference provider as context, which the Privacy text has said since f9b6047. It is never
  published, sold or pooled into a shared training set.

### 4.4 Fair incentives in the research panel

- Participants know they are in a study, may skip any task and may stop at any time. Proposal: stopping
  never forfeits payment for time already given.
- Pay per completed study block, with a daily and weekly cap. Never pay per extra hour of play, per failure
  or per correction ([DATA_QUALITY_AND_BIAS](DATA_QUALITY_AND_BIAS.md) §3.6). Those reward excessive play
  and unreliable labels.
- Payment never depends on granting more than the study needs.
- Payouts go through a payout processor, which holds the identity. The research store never does.

---

## 5. Withdrawal and deletion

### 5.1 Withdrawal

When the player withdraws one track, or all of them:

1. That track's recorder closes at once. No new event is written.
2. Pending batches, queued but not sent, are cancelled and deleted locally (P1 test
   `withdrawal_with_a_batch_pending_cancels_it`).
3. In P2 the server is told. It marks the subject withdrawn for that track, and ingest refuses any later
   event for it, including a late upload from an offline client.
4. In the MVP, withdrawal also deletes that track's data (§5.2). "Stop but keep what I gave" is not
   offered, because it would need a basis for keeping data after consent ends (§7, Q-G5).

### 5.2 What deletion reaches

| Layer | How | When (proposal) | Proof kept |
|---|---|---|---|
| Local queue and pending batches | deleted | at once | count |
| Local research store (P1) | the subject's events, episodes and export rows removed; affected exports listed (P1 test `deleting_a_research_subject_removes_their_events_episodes_and_export_rows_and_lists_the_exports`) | at once | counts; affected exports |
| Ingested events (P2) | deletion index by `research_subject_id`; affected Parquet partitions rewritten; Postgres rows deleted ([DATA_ARCHITECTURE](DATA_ARCHITECTURE.md)) | ≤ 30 days | counts per partition; rewrite job id |
| Media objects | the objects deleted, with their crops, thumbnails and derived frames | ≤ 30 days | object count |
| Labels and annotations of those items | deleted with the items | with the items | count |
| Embeddings, features, caches | deleted by lineage, using the dependency graph from observation to customer product (directive §7) | with the items | count |
| Curated datasets and the gold benchmark | rebuilt without the subject as a new version; the old version retired | next rebuild, ≤ 30 days | versions retired |
| Published aggregates | not retracted (above thresholds, not about one person); the subject is left out of future aggregates | — | — |
| Backups | not edited in place. They expire on a documented cycle (proposal: ≤ 35 days), and a restore replays the deletion log before the data is used again | ≤ one backup cycle | the cycle; the replay log |
| Customer exports already delivered | per contract: the buyer deletes on notice within a stated time and certifies it; later releases leave the subject out | proposal: ≤ 30 days from notice | notice sent; certificate received |
| **Trained model weights** | **Not removed.** We record which training runs consumed which dataset snapshots, leave the subject's data out of every future run, and, where the contract requires it, retrain or retire the affected models at their next scheduled release. We do not promise "unlearning" | — | the runs that used the subject's data |
| The consent ledger | not deleted: its receipts are the proof (§5.3) | — | — |

**The caveat is part of the consent text** of every track that allows training: *"If your data was used to
train a model before you delete it, deleting it does not remove what that model already learned. We stop
using your data for any future training and keep a record of which models used it."* Before any licence for
external training, the legal and operational answer to deletion requests reaching a buyer's trained models
must be agreed (directive §10; §7, Q-EU4 and Q-G7).

### 5.3 The minimal record that proves it

- **A deletion record**: `deletion_request_id`, `research_subject_id`, requested at, scope (tracks), per layer
  the completion time and counts, the exports notified and the certificates received, the backup expiry
  date, and the training runs that had used the data. No content.
- **A deny-list entry** for the subject id, so that a late upload is refused.
- Both are pseudonymous personal data. How long to keep them is a question for counsel (§7, Q-G6).

---

## 6. Age: adults only, with proportionate assurance

### 6.1 The product choice

- **Every contribution track is for adults (18+) in the MVP.** The research track, the media track and the
  paid panel require stronger assurance (§6.3).
- **This is a product choice, not a claim that 18+ settles every duty.** Laws differ between countries on the
  age of consent for data, and on minors' contracts and payments (§7).
- **There is no track for children.** A minor can use the product on the product's own terms and
  contributes nothing.

### 6.2 Why a checkbox alone is not enough

- **MapleStory has young players.** [`../data-and-metrics.md`](../data-and-metrics.md) ("Age") says so.
  A self-declared checkbox is easily false, and a service that knows it has young users may be held to that
  knowledge. Whether COPPA's "actual knowledge" or mixed-audience rules reach us is a question (§7, Q-US1;
  unverified).
- **A false "yes" costs a lot and lasts.** Once a child's data is in a corpus licensed for training, it cannot
  be taken back out of trained weights (§5.2).
- **Hence proportionality.** The stronger the use (external, commercial, media), the stronger the assurance.

### 6.3 A ladder of options (not decisions)

| Level | Method | What it shows | Costs and risks | Proposed for |
|---|---|---|---|---|
| 1 | a neutral age question: the birth year, with no hint of the cut-off and no instant retry | a declaration | easily bypassed; only the yes/no result is kept | `improve_syrup` (internal, structured, no media), if counsel agrees |
| 2 | level 1 plus friction: a cool-down after an under-age answer; acting on explicit statements made to support | a declaration with fewer impulsive lies | still a declaration | `aggregate_analytics` |
| 3 | third-party assurance that returns only "18+: yes or no": an ID-document check by a vendor (the document is not kept by us); an existing verification by a panel-recruitment vendor; or the identity check a payout processor performs anyway | evidence of adulthood | cost per check; the vendor processes identity documents as our processor; people without ID are excluded. Facial age estimation processes biometric data and is a separate question (§7, Q-G9) | `external_research_training`, `media_donation`, the paid panel |
| — | a parent's consent for a minor | — | not offered: there is no track for children | — |

- **What is kept**: the method, the result and the time, in the receipt's `age_assurance`. Never the
  document, a photo or a birth date.
- **No inference of age.** We never infer age from voice, face, writing or behaviour (directive §5: no
  inference of sensitive traits). We act on explicit information: the participant or a parent tells us, or
  an assurance check fails. Then the subject's contribution stops, their data is deleted (§5.2), and the case
  goes to review ([THREAT_MODEL](THREAT_MODEL.md) §9).

---

## 7. Questions that need a professional opinion

**These are questions, not answers, and none of this is legal advice.** Every "pointer" names where counsel
might start; I could not fetch any of the sources (unverified). For each question: why it matters to the
design, and the conservative default we keep until it is answered.

Facts counsel will need: where participants are (Israel, the EU, the US including California, elsewhere);
which legal entity will be the controller and the licensor; the five tracks (§2.1); the data types (§2.2);
the inference providers and where they process data (audit §4.1); the intended buyers' countries.

### General

| # | Question | Why it matters | Until answered |
|---|---|---|---|
| Q-G1 | Inference with the player's own API key: who is controller and who is processor for that flow, and what must the notice say? | `service_operation` disclosures; the Privacy text (f9b6047) | disclose every provider; never route research through inference |
| Q-G2 | What makes a track decision valid consent in each regime (freely given, specific, informed, unambiguous, as easy to withdraw as to give)? May it ever be bundled with the terms of use? Pointer: GDPR Art. 4(11) and 7 | the consent UI (§2.4) | never bundled; equal "Yes" and "No thanks" |
| Q-G3 | Which lawful basis per track: consent for every contribution track (our assumption)? Contract or legitimate interest for service operation (crash reports, local stats)? Do local-only stats need anything? | the default state of each track | consent for every contribution track |
| Q-G4 | Are pseudonymous episodes personal data (our assumption: yes)? When may an aggregate be called anonymous? What does "deidentified" require under the CCPA? | wording to participants and buyers; threshold design ([THREAT_MODEL](THREAT_MODEL.md) §6.6) | call them pseudonymous; never "anonymous" |
| Q-G5 | After withdrawal, may anything be kept, and on what basis? | "stop" vs "stop and delete" (§5.1) | delete on withdrawal |
| Q-G6 | How long to keep the ledger, deletion records and the deny-list? | §3.2, §5.3, §10 | keep while the data they cover exists, plus a proof period to be set |
| Q-G7 | Which entity is controller and licensor? What must processor agreements and buyer licences contain: no re-identification, no onward transfer, deletion on notice including what happens to trained models, security, audit, prohibited uses? | every export | no export |
| Q-G8 | The paid panel: is the participant agreement a consumer contract? Tax and reporting for incentives? Do academic buyers expect an ethics review? | product D | no panel before review |
| Q-G9 | Voice, and facial age estimation: are they biometric or special-category data where we operate? | audio is out of the MVP; age assurance options (§6.3) | no audio upload; no facial estimation |

### Israel

| # | Question | Until answered |
|---|---|---|
| Q-IL1 | Under the Privacy Protection Law, 5741-1981, as amended by Amendment 13: what must the notice at collection say for each track? | notice per track, in Hebrew and English |
| Q-IL2 | Must a database whose purposes include transferring information to others (licensing episodes) be registered or notified after Amendment 13? (Already asked in [`../data-and-metrics.md`](../data-and-metrics.md), item 4.) | no transfer |
| Q-IL3 | Is a privacy protection officer required for this activity? | the Privacy & Security role is assigned anyway |
| Q-IL4 | Which security level of the Privacy Protection (Data Security) Regulations, 2017 (pointer, unverified) applies, and what incident-reporting duty and timeline follow? ([THREAT_MODEL](THREAT_MODEL.md) §8) | treat as the highest plausible level in design |
| Q-IL5 | Transfers abroad (inference providers and buyers outside Israel): which conditions apply? | no transfer beyond the inference providers already disclosed |
| Q-IL6 | Minors: capacity to consent and to receive payment | minors excluded (§6) |
| Q-IL7 | What enforcement exposure does Amendment 13 create? Pointer: the Privacy Protection Authority's professional guide to Amendment 13 (directive source 17; not fetched, unverified) | conservative defaults throughout |

### European Union (if participants are there)

| # | Question | Until answered |
|---|---|---|
| Q-EU1 | Does GDPR apply (offering a service to people in the EU, or monitoring their behaviour; pointer: Art. 3(2))? Is an EU representative needed (pointer: Art. 27, already asked in [`../data-and-metrics.md`](../data-and-metrics.md))? | assume it applies to EU participants |
| Q-EU2 | Is a DPIA required (pointer: Art. 35) for systematic observation of gameplay with AI? | do a DPIA-style review before P2 regardless |
| Q-EU3 | Transfers: Israel's adequacy status as it stands now, and transfers to US providers | verify before P2 |
| Q-EU4 | Erasure and trained models. The EDPB opinion on AI models (directive source 14) addresses when a model can be considered anonymous and what unlawful processing means for a model; not fetched, unverified | the §5.2 caveat; lineage of training runs |
| Q-EU5 | Is our adults-only assurance proportionate (pointer: Art. 8 on children's consent, which an adults-only design should not reach)? | §6.3 ladder |

### United States

| # | Question | Until answered |
|---|---|---|
| Q-US1 | COPPA: is the service "directed to children", or do we have "actual knowledge" of children, given MapleStory's young players? What do the 2025 amendments change for us? (Directive source 15 is the FTC's January 2025 press release, whose address calls it changes "limiting companies' ability to monetize kids' data"; its content was not fetched, unverified) | adults only; no child data in any track |
| Q-US2 | CCPA/CPRA: do we meet its thresholds? Is licensing episodes a "sale" or "sharing"? What must the notice at collection say? What do "deidentified" and "sensitive personal information" cover? (Directive source 16, not fetched) | treat licensing as a sale needing explicit consent |
| Q-US3 | Data-broker status: does selling licensed data collected from our own consenting participants, with whom we have a direct relationship, make us a data broker under the California Delete Act or other state registries? (Also Q-IL2 for Israel) | no sale |
| Q-US4 | Other state privacy laws, as participants arrive | review before recruiting in a new state |

### Rights in the game and in the content

| # | Question | Until answered |
|---|---|---|
| Q-R1 | Nexon's terms for MapleStory (which entity and which terms in each region) and for MapleStory Worlds: do they allow (a) the companion's capture at all (launch plan task 1.4.3), (b) storing frames, (c) annotating them, (d) training internal models, (e) licensing derived structured data without pixels, (f) licensing media? Does each need a written licence? | `requires_title_specific_review`: no export, no media (§8.4) |
| Q-R2 | Is structured data derived from frames (HUD numbers, map ids, UI states) a reproduction or derivative of protected material, or a factual observation? Do the terms restrict it regardless of copyright, as a matter of contract? | derived data is not assumed exempt |
| Q-R3 | Other players visible in frames (names, chat): what remains our duty even after sanitization? | no media upload in the MVP |
| Q-R4 | MapleStory Worlds' user-created worlds: whose rights (Nexon, the world's creator, other players)? | refused |
| Q-R5 | The participant's own words (help requests, corrections): must the consent text include a licence from the participant to us and onward to buyers? | include a plain-language licence in the research track's text, subject to review |
| Q-R6 | Database rights in our curated datasets, and the publisher's database rights in game data | — |
| Q-R7 | Can contributing put a player's game account at risk under the publisher's terms? | a stop condition ([THREAT_MODEL](THREAT_MODEL.md) §9) |

---

## 8. The rights manifest

### 8.1 Why consent is not enough

- **A player's consent covers the player's own personal data, nothing more.** It grants nothing in the
  game's images, music, text, characters, maps or interface, which belong to the rights holder. It grants
  nothing in other players' data on screen either: their names, chat, guilds and characters.
- **Our MIT licence covers our code.** It grants nothing in the game.
- **Derived data is not automatically exempt.** A label on a screenshot, a crop, a template learned from the
  HUD (the `learned/` folder) and an embedding of a frame are all derived from the game's pixels. Structured
  readings such as HP numbers or a map name are closer to facts, but the publisher's terms may restrict
  extracting or commercially using them as a matter of contract regardless (§7, Q-R2).

### 8.2 Fields

**In the P1 code** (`RightsManifest`, one per title and deliverable; [DATA_CONTRACTS](DATA_CONTRACTS.md)
governs):

| Field | Meaning |
|---|---|
| `rights_policy_id` | the id every event carries in its envelope |
| `game_id` | `maplestory`, `maplestory_worlds`, `synthetic`, `other` or `unknown`. MapleStory and MapleStory Worlds are told apart; anything not reliably identified is `unknown` |
| `deliverable` | `event_metadata` (events and episodes: metadata and sanitized text, no pictures, no sound) or `media` (screens, clips, audio) |
| `rights_holder` | as named in the governing terms (to be verified per region) |
| `status` | `approved`, `requires_title_specific_review` or `denied` |
| `basis` | the evidence of permission, or of an approved analysis; empty when there is none |
| `applies_to` | the versions and worlds covered |
| `grants` | per purpose: the recipient classes and the uses, from `capture`, `store`, `annotate`, `train_internal`, `train_external`, `evaluate`, `transfer` |
| `territory` | where the uses are allowed (recorded; P1 sends nothing off the machine, so it is not enforced there) |
| `valid_from`, `valid_until`, `revoked_at` | the term, and a revocation. An expired or revoked manifest refuses everything |
| `reviewed_by_role` | the logical role that reviewed it |
| `notes` | the reasoning, in words |

**Proposed additions for P2**, before any real title is approved:

| Field | Meaning |
|---|---|
| `game_variant`, `world_id`, `build_range`, `service_region` | a finer scope than `applies_to`. Terms and publishers can differ by region |
| `governing_terms` | the document reviewed: its address, version or effective date, and the SHA-256 of the copy reviewed |
| `basis_kind` | `written_licence`, `publisher_sdk_terms`, `approved_legal_analysis` or `owned_environment` |
| `evidence_ref` | the signed licence or the approved analysis: id, date, author or signatory, hash |
| `next_review_at` | the date of the next review |
| uses `market` and `evaluate_external` | showing data as available in sales material; evaluation by an outside party, apart from `evaluate` for our own |
| `revocation_terms` | how notice arrives, the notice period, and what we must do by when: stop capture, stop exports, purge, notify buyers |
| `restrictions` | attribution, "no monetised footage", "no bots" and the like; the prohibited uses passed on to buyers |
| finer deliverables | the artifact types of §8.3, where `event_metadata` and `media` are too coarse |

### 8.3 Artifact types

The P1 code distinguishes two deliverables. `event_metadata` covers structured events, the participant's
sanitized text, the assistant's advice, episodes and labels. `media` covers frames, clips and audio. The finer
types below are a proposal for P2: rights and risks differ between them.

| Artifact type | The publisher's material in it? | The participant's personal data? | Other players' data? |
|---|---|---|---|
| `structured_events` (game state, tasks, outcomes) | derived facts (§7, Q-R2) | yes, pseudonymous | should not be (sanitized) |
| `participant_text` (help requests, corrections, self-reports) | may quote game text | yes | possibly (a name mentioned); sanitized |
| `assistant_text` (advice) | may quote game knowledge | linked to the participant | — |
| `aggregate_report` | low | no (aggregated, thresholded) | no |
| `episode_dataset` | as its parts | yes, pseudonymous | as its parts |
| `labels_annotations` | derived | linked | — |
| `embeddings_features` | derived from pixels when visual | linked | possibly |
| `media_frame`, `media_clip` | **yes**: images, interface, text, and music in clips | yes: the play, possibly the cursor | possibly: names, chat |
| `audio` | game music and sound; voice | yes | — (not in the MVP) |
| `eval_suite` (items with gold) | depends on the items (frames: yes) | linked | as its items |
| `trained_model` | whether it embodies the material is a question (§7, Q-R2, Q-EU4) | possibly | possibly |

### 8.4 MapleStory and MapleStory Worlds

The manifests the P1 code ships (`RightsRegistry::builtin()`):

| `rights_policy_id` | `game_id` | Status | What is allowed |
|---|---|---|---|
| `rights-maplestory-review-required-v1` | `maplestory`: every world type, including Classic World until shown otherwise | `requires_title_specific_review`; no grants | **Nothing in the research path.** MapleStory data is refused even with full consent (test `maplestory_data_is_refused_even_with_full_consent`). Service operation on the player's own PC is the product itself, and depends on the terms question in launch plan task 1.4.3 |
| `rights-maplestory-worlds-review-required-v1` | `maplestory_worlds` | `requires_title_specific_review`; no grants | **Nothing.** World creators add a layer of rights (§7, Q-R4) |
| `rights-synthetic-local-demo-v1` | `synthetic` | `approved`, with grants that name only `this_device` | demonstrating the slice locally; generated data, no game content, no player; never presented as game data or as a basis for a real title |
| none | `other`, `unknown` | no manifest, so refused | — |

All three manifests cover the `event_metadata` deliverable. None grants anything for `media`, so media is
refused everywhere (in the P1 export test, a media export is refused with `use_not_licensed`).

**Game identity is a precondition.** Today MapleSyrup decides what "the game" is by its window title alone:
any title containing "maplestory" (audit §4.3, §6). That cannot tell MapleStory from MapleStory Worlds, a
private server, or a Notepad file named "maplestory notes.txt". The rights gate therefore needs `game_id`,
`game_variant` and `world_id` from a reliable detection; anything else is `unknown` and refused
([THREAT_MODEL](THREAT_MODEL.md) T5).

#### What Nexon's pages say (read 2026-10-10)

Fetched on 2026-10-10 with the session's fetch tool, which returns the passages a small model extracted on
request (verbatim by instruction). **Check each against the live page before relying on it**; counsel reads
the full terms. Section numbers are the pages' own.

| Source (directive's list) | What it says |
|---|---|
| 10. Nexon Game IP Guide for Content Creators | §2: "UGC refers to derivative works generated by creators based on NEXON Game IPs, including merchandise, game footage (including gameplay streams and fan films, etc.)". §4: "Commercial use of UGC is prohibited." §4.2: "You may not sell, display, or otherwise provide access to UGC in return for monetary compensation." §5 permits only advertising revenue or donations on content platforms (§5.1) and merchandise at events Nexon hosts (§5.2). The page says nothing about licensing, datasets, AI or machine learning. |
| 11. Statement on Permissible Uses of VOD and Streaming | "You may not engage in commercial use of our content, which is a violation of our Terms of Service." "It is not okay to put those videos or streams behind a paywall, to require subscriptions for access, or to sell licenses to your videos or streams." "It is not okay to make any other commercial use of our content." Nexon "will not respond to any request to determine if a particular use is non-commercial". Nothing on data, research or AI. |
| 12. MapleStory Worlds Terms of Service and EULA (effective 13 Aug 2025) | §II.1: the licence is for "personal, non-commercial, entertainment use only" (creators: under the Creator Terms). §VII (Code of Conduct) prohibits "use of a data mining utility program to intercept, collect, read or mine information generated by the Services", "any robot, spider, site search/retrieval application or other manual or automatic device" to retrieve, index or data-mine Services content, "macros, auto-looting or robot play", and "hacks, cracks, bots, or third-party software that may modify" the Services; §IV.3: the Services may monitor the device for prohibited third-party programs. §III: users grant Toben a royalty-free, perpetual, irrevocable licence to User Content "for any purpose". Nothing on machine learning or AI training. |

**What follows, for counsel to confirm (not legal advice):**

- Selling or licensing anything derived from MapleStory gameplay footage is, on the face of the IP guide and
  the VOD statement, a commercial use Nexon prohibits without its permission — so both titles stay
  `requires_title_specific_review` with no grants, and the commercial quantity from them is zero until
  Nexon grants a written licence. The preferred pilot remains a partner studio's own title.
- **A question for the product itself, not only the data program:** MapleStory Worlds' §VII prohibits "a
  data mining utility program to intercept, collect, read or mine information generated by the Services".
  Whether a companion that reads the screen (MapleSyrup) is such a program, under MapleStory Worlds' terms
  and under the MapleStory terms that govern Classic World and the other worlds, is for counsel before the
  launch (launch plan task 1.4.3).

**What to quote when the pages are read** (the clause, its heading and the page's date):

- **IP guide**: (1) which games, regions and kinds of content it covers; (2) whether videos and streams may
  be monetised, and on what conditions; (3) whether selling, licensing or sublicensing gameplay footage or
  other content to third parties is prohibited; (4) whether it covers screenshots, music and art; (5)
  whether it says anything about datasets, AI, machine learning, or any use other than videos and streams;
  (6) whether the permission is revocable and Nexon may demand removal.
- **VOD statement**: the same six points, and (7) its date, and whether the IP guide supersedes it.
- **MapleStory Worlds terms**: (1) the licence to users and its limits (personal, non-commercial); (2)
  prohibited conduct: third-party programs, data mining, scraping, automation, capturing or extracting data;
  (3) user-created content: who owns a world and its assets, and what licence users give Nexon and each
  other; (4) commercial use of the service or its content; (5) any clause on AI, machine learning or
  training; (6) age requirements; (7) governing law and disputes; (8) the effective date, and whether a
  later version exists.
- **Not in the directive's list but needed**: the current MapleStory terms of service and EULA for each
  region where participants play (which Nexon entity publishes each is unverified), and any third-party
  software policy (launch plan task 1.4.3 already asks about overlays).

**The preferred path to a commercial pilot** (directive §10) is a partner studio that grants written rights
for a defined game, build, set of uses, territory and term, or an environment we own with the right
licences for its assets. A synthetic or owned environment also serves Syrup SDK and evaluation demos without
any player data. [B2B_PRODUCT_STRATEGY](B2B_PRODUCT_STRATEGY.md) picks the first track; per
[UNIT_ECONOMICS](UNIT_ECONOMICS.md), without approved rights the commercial quantity is zero.

### 8.5 How the gate uses the manifest

- **Every step checks consent and rights together**: capture, storage, annotation, a training run, an
  evaluation and an export are each checked for (game, artifact type, use, territory, date). Default deny;
  an unknown game is refused.
- **Expiry refuses.** A policy past `valid_until` refuses recording and export (P1 test
  `an_expired_rights_policy_refuses_recording_and_export`, code `rights_expired`).
- **Revocation by the rights holder**: stop capture for that title, quarantine its data, stop exports,
  notify buyers per contract, purge on the stated timeline, and record it in the deletion log.
- **Two roles change a manifest.** Privacy & Security and Product B2B review every change, and every change
  is logged. `approved` requires a `basis` (in P2, an `evidence_ref` to the signed document). A forged
  "approved" would mean an unlicensed export
  ([THREAT_MODEL](THREAT_MODEL.md) T11).
- **No marketing without rights.** An artifact whose manifest does not allow `transfer` to a
  `licensed_buyer` never
  appears in a catalogue, sample or pitch as available. Research-only material is never counted as sellable
  (directive §14).

---

## 9. How this maps onto what exists

| Today | State | What replaces it |
|---|---|---|
| Play stats: one boolean `Consent.on`, a date and the install id. The 18+ answer was asked and not kept, and withdrawal deleted the only record ([CURRENT_STATE_AUDIT](CURRENT_STATE_AUDIT.md) §5) | **withdrawn**: `metrics::SHARING_OFFERED` is `false`, the PC refuses an "on" (`403 not_available`), and a choice made with an earlier build is withdrawn at start (a351a9f) | The local stats stay local under service operation (notice; §7, Q-G3). Any future external use of them goes through `aggregate_analytics`: a receipt, adults only, the rights check and thresholds. The stats' install id is never reused as a `research_subject_id` |
| The sharing card's text: share "with MapleSyrup's partners, who may buy them" (audit §5) | not shown | a card per track (§2.4) and the recipient register (§4.2) |
| Launch plan [2.3](../launch-plan/tasks/2.3-telemetry-and-crashes.md): crash reports and minimal telemetry, opt-in, through an existing service such as Sentry, under an install id the plan calls "anonymous" | planned for week 2 | This is `service_operation` (reliability), not a contribution: it never feeds the research store. The service is a named processor. The id is pseudonymous, not anonymous (the play-stats document says the same of its own id), and it must never equal or link to a `research_subject_id`. The crash report leaves out conversation text (2.3 already says so), keys, and paths that contain the Windows user name |
| The notebook, sent with every reply to OpenAI or xAI (audit §4.1) | disclosed since f9b6047 | `service_operation` only; never a contribution type (§2.2) |
| Recordings of the whole screen, all PC sound and the phone microphone | local | never a contribution. Research media is a separate path (`media_donation`, [DATA_ARCHITECTURE](DATA_ARCHITECTURE.md)) |
| Corrections in `knowledge.json` | local | the player's own. Corrections made later under `improve_syrup` are new structured events, never the file |
| The P1 slice (`src/research/`) | being written | `ConsentLedger`, `RightsRegistry` and the refusal codes, on synthetic data, with no network |

---

## 10. Retention proposals (for discussion, not legal duties)

| Data | Proposal | Why |
|---|---|---|
| Raw audio | **never in the cloud in the MVP** | voice is the highest-risk data, and P1–P2 do not need it. On the PC it remains the player's own file |
| Approved media clips, for QA | **≤ 30 days**, then deleted unless kept under an explicit `media_donation` grant with a written justification | QA needs only a short window |
| Detailed events after ingest | **≤ 90 days**, then deleted or kept only as curated episodes or aggregates | metrics and quality checks |
| A longer-lived curated episode corpus | **only with a written justification and an explicit grant whose text states the period**, reviewed every year | directive §10 |
| Quarantined records | ≤ 14 days | quarantine must not become a back door for keeping data |
| Consent ledger, deletion records, deny-list | while the data they cover exists, plus a proof period | §7, Q-G6 |
| Security and audit logs | 12 months, with no content | incident investigation ([THREAT_MODEL](THREAT_MODEL.md) §6.5) |
| Backups | a cycle of ≤ 35 days | the reach of deletion (§5.2) |
| Customer exports | per contract; deleted at the end of the term, with a certificate | §5.2 |
| Local personal files (notebook, recordings, logs, stats) | the player decides; outside the program | directive §10 |

---

## 11. Decisions for the owner

| # | Decision | Needs counsel? |
|---|---|---|
| D1 | Adopt the five-track model, contribution off by default, adults only, no track for children | the product rule is the owner's; whether it suffices is for counsel (§7) |
| D2 | Choose the age-assurance level for each track (§6.3) | yes, together with a cost check |
| D3 | Commission the review of §7 before P2. Q-R1 (Nexon's terms) and Q-IL2/Q-US3 (database registration, data-broker status) come first: they decide whether any MapleStory commercial track exists at all | yes |
| D4 | The first commercial path: a partner studio with written rights, or an environment we own ([B2B_PRODUCT_STRATEGY](B2B_PRODUCT_STRATEGY.md)) | yes, for the licence |
| D5 | Take the retention proposals (§10) to counsel | yes |
| D6 | Which legal entity is the controller and the licensor (§7, Q-G7) | yes |
| D7 | The legal sources other than Nexon's three pages still need reading (§8.4 has Nexon's, read 2026-10-10) | no |

## Verification status

- **Unverified**: everything about Nexon's IP guide, its VOD statement, the MapleStory Worlds terms, GDPR, the
  EDPB opinion, COPPA and the FTC's amendments, the CCPA, and the Amendment 13 guide. None of them could be
  fetched (permission request unanswered). Article numbers are pointers for counsel.
- **From the repository**: the existing flows, play stats and refusal behaviour are cited from
  [CURRENT_STATE_AUDIT](CURRENT_STATE_AUDIT.md) and [`../data-and-metrics.md`](../data-and-metrics.md). The
  P1 test names cited are from `tests/research_slice.rs` as it stood when I wrote this; **I did not run
  them**, and the module they test was still being written.
- **Proposals**: every number here (12-month re-confirmation, 30/35/90/14-day periods, 12-month logs) is a
  proposal for discussion, not a measured need or a legal duty.
