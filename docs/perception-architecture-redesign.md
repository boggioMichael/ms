# Perception Architecture Redesign: Deterministic Rendering & Game-State Reconstruction

**Status:** Proposal / design document. Contains `[MEASURED]`, `[DESIGNED]`, `[ESTIMATED]`, and `[PLANNED]` claims per the evidence-labeling convention established in [EVIDENCE.md](/c:/Users/magshimim/Desktop/projects/ms.worktrees/debugging-toolkit-for-vision-systems.worktrees/perception-architecture-redesign-maplestory/EVIDENCE.md). Nothing here should be read as already true in production until it is re-labeled `[MEASURED]` with a linked evidence artifact.

**Scope:** How to evolve the current confidence-aware, OCR-dependent detector pipeline (documented in [vision-architecture.md](/c:/Users/magshimim/Desktop/projects/ms.worktrees/debugging-toolkit-for-vision-systems.worktrees/perception-architecture-redesign-maplestory/docs/vision-architecture.md)) into a two-tier perception system: a **deterministic decoding tier** for anything the client renders predictably (HUD digits, bars, fixed panels), and a **probabilistic inference tier** (the existing detectors) for anything that is inherently ambiguous from pixels alone (motion, dialogs, terrain, unclassified icons). The two tiers feed a single **game-state reconstruction layer** that produces one authoritative, versioned `GameState` per frame.

---

## 1. Executive Summary

The current pipeline treats every on-screen element — including elements the game client renders with byte-for-byte reproducible bitmap fonts and fixed sprites — as an unknown signal to be *inferred* via geometry heuristics and Tesseract OCR. This is why the system is `[MEASURED]` at 136.3ms mean / 86.5ms median latency per frame (`EVIDENCE.md` §5, §6) and fails every real-time budget tested (30/50/60 FPS, all `[DESIGNED BUT BROKEN]`).

The redesign's thesis: **most of what the HUD displays is not ambiguous — it is a known, finite rendering of known assets, and should be decoded, not detected.** MapleStory's UI renders HP/MP/EXP digits and bar fills using a fixed bitmap font and fixed-width bar sprites at a given client resolution/UI scale. Given that assumption:

- Digit and bar-fill recognition become **template/pixel-arithmetic decoding problems** (exact match against a known asset library), not OCR problems.
- Decoding is `[ESTIMATED]` to be 1–2 orders of magnitude faster than Tesseract subprocess OCR and near-deterministic in accuracy for in-library assets, with graceful, explicit fallback to the existing OCR/heuristic path for anything outside the library (unknown UI skin, unexpected resolution, patch-changed assets).
- The existing `Detection<T>` / confidence / temporal machinery is **kept**, not replaced — it becomes the fallback and corroboration layer, and gains a new top reliability tier (`Deterministic`) for exact-match decodes.
- Game-state reconstruction becomes an explicit, testable layer with its own schema, provenance tracking, and determinism guarantees, rather than an implicit side effect of `PerceptionPipeline::detect`.

This document specifies the architecture, compares the detector strategies under consideration, defines a validation strategy that produces ground truth without requiring hand-labeling every frame, and lays out a phased roadmap with risk mitigations.

---

## 2. Problem Statement (why redesign, not patch)

| Finding | Status | Evidence |
|---|---|---|
| Mean per-frame latency 136.3ms, median 86.5ms | `[MEASURED]` | `EVIDENCE.md` §5.1, `docs/demo_evidence/csv/latency_measurements.csv` |
| 0/100 frames meet 30/50/60 FPS budgets | `[MEASURED]` | `EVIDENCE.md` §6 |
| OCR (Tesseract subprocess) is the dominant cost, 300–500ms on frames requiring it vs. 76–86ms geometry-only | `[MEASURED]` | `EVIDENCE.md` §5.2, §8.1 |
| HUD confidence tops out at `Heuristic`/`Corroborated` (0.55 in the sampled frame) even when the bar and digits are perfectly legible | `[MEASURED]` | `EVIDENCE.md` §10.1, `docs/demo_evidence/json/gamestate_frame_0_pretty.json` |
| No ground-truth accuracy numbers exist for HUD digit/bar reading — only latency and "did it run" are measured today | `[VERIFIED]` (absence confirmed by review of `EVIDENCE.md`) | n/a |

Two distinct problems fall out of this:

1. **Latency problem**: OCR is a generic, font-agnostic recognizer being used to solve a font-*specific* problem (the client only ever renders its own bitmap font). This is solvable without ML by exploiting determinism.
2. **Accuracy-measurement problem**: the current architecture has no way to say "HUD reading is X% accurate" because there is no ground truth to compare against — confidence scores are self-reported heuristics, not validated against known-correct values. The redesign must produce that ground truth as a first-class artifact (§6).

Async OCR and frame-skipping were previously `[PLANNED]` mitigations (`EVIDENCE.md` §8.1, §14). They reduce *latency variance* but do not address the accuracy-measurement problem and do not remove OCR's fundamental mismatch with a deterministic renderer. The redesign proposed here supersedes that plan for HUD-class elements while remaining compatible with it for anything that stays OCR-based.

---

## 3. Core Redesign Thesis: Deterministic Rendering

### 3.1 The determinism assumption

For a given `(game_resolution, ui_scale, client_version, chat/UI locale)` tuple, the game client renders:

- **HUD digits** (HP/MP/EXP numbers, level) using a fixed bitmap font — every glyph is pixel-identical every time it is drawn.
- **HP/MP/EXP bar fills** as a solid-color rectangle whose *width* is a deterministic (if not perfectly documented) function of the underlying percentage, drawn inside a fixed-size frame sprite.
- **Fixed UI chrome** (name/level plate borders, minimap frame, buff-icon slots) as static sprites at fixed offsets relative to the window/anchor.

This is fundamentally different from **motion**, **dialog text**, **chat log content**, and **terrain edges**, which vary with gameplay content, camera position, and player-authored/localized text — genuinely unknown at build time.

The architecture should therefore stop applying one detection strategy uniformly and instead **classify every perception target by whether its rendering is closed-form (enumerable) or open-form (must be inferred)**, and route each to the appropriate tier.

### 3.2 Two-tier perception

```
                     ┌──────────────────────────────┐
                     │        Frame (RgbaImage)      │
                     └───────────────┬───────────────┘
                                     │
                 ┌───────────────────┴───────────────────┐
                 │                                        │
      Deterministic Decoding Tier               Probabilistic Inference Tier
      (closed-form UI elements)                 (open-form UI/world elements)
                 │                                        │
    ┌────────────┴────────────┐              ┌────────────┴────────────┐
    │ Glyph template matcher   │              │ Existing detectors:      │
    │ Bar-fill pixel decoder   │              │ Motion, Dialog, Panels,  │
    │ Fixed-panel hash lookup  │              │ Environment, Combat      │
    │ (asset library keyed by  │              │ (Detection<T> + geometry │
    │  resolution/scale/vers.) │              │  + Tesseract OCR + EMA/  │
    └────────────┬────────────┘              │  ObjectTracker as today) │
                 │                            └────────────┬────────────┘
                 │  exact match → Deterministic             │ confidence-scored
                 │  no match → fallback ↓                   │
                 └──────────────┬─────────────────────────┬─┘
                                │                          │
                                ▼                          ▼
                     ┌─────────────────────────────────────────┐
                     │      Game-State Reconstruction Layer      │
                     │  (reconciliation, provenance, versioning) │
                     └───────────────────┬───────────────────────┘
                                         │
                                         ▼
                                    GameState (v2)
```

Key property: **the deterministic tier is not a replacement for the inference tier — it is a fast path with an explicit escape hatch.** If a glyph or bar doesn't exactly match anything in the asset library (new patch, unrecognized UI skin, unexpected resolution, corrupted capture), the pipeline falls back to the existing geometry+OCR path for that field only, on that frame only. This keeps the current heuristic detectors as a permanent, tested safety net rather than deleting them.

### 3.3 New reliability tier

`crate::vision::types::Reliability` gains one new variant, ordered above the existing ones:

- **`Deterministic`**: value decoded via exact template/pixel match against the known asset library; no OCR or fuzzy geometry involved. Distinct from `Corroborated` because it isn't "two independent heuristics agreeing" — it's "the only possible rendering of this exact bit pattern is value X."
- `Corroborated`, `Heuristic`, `Predicted`, `Unreliable` are unchanged and still apply to the inference tier and to deterministic-tier fallback cases.

`Confidence` scoring for `Deterministic` reads are `[DESIGNED]` to be pinned near 1.0 (e.g. 0.99, never a hard 1.0, to leave room for capture artifacts like partial alpha blending or scaling filters) rather than computed from heuristic signal-agreement.

---

## 4. Detector Strategy Comparison

Candidates evaluated for HUD-class (digit/bar) recognition, since that is where OCR cost is concentrated (`EVIDENCE.md` §5.2):

| Strategy | Mechanism | Latency (est.) | Accuracy (est.) | Robustness to UI changes | Maintenance cost | Verdict |
|---|---|---|---|---|---|---|
| **Tesseract OCR** (current) | Generic subprocess OCR on cropped text region | `[MEASURED]` 300–500ms when invoked | Unknown — never validated against ground truth; anecdotally unreliable on small/stylized fonts (`vision-architecture.md` §"Known Limitations") | High (font-agnostic) but that generality is wasted here | Low (no asset maintenance) but produces unverifiable output | Keep only as fallback |
| **Template-matched bitmap font decoder** (proposed) | Slice fixed-width glyph cells from the digit region, hash or pixel-diff each cell against a pre-extracted glyph atlas | `[ESTIMATED]` 0.1–2ms per field (pure in-process pixel compare, no subprocess) | `[ESTIMATED]` ~100% on frames matching a known client version/resolution/scale; falls back cleanly otherwise | Low — breaks silently if the client changes font assets or scaling; **must** detect "no confident match" and fall back rather than guess | Medium — requires building/versioning the glyph atlas per client version, but it's a one-time asset extraction plus regression tests | **Primary path for HUD digits/level/plate numerics** |
| **Bar-fill pixel-arithmetic decoder** (proposed) | Count/measure filled-vs-empty pixel run length inside the known bar sprite bounds; convert directly to percentage via known bar geometry | `[ESTIMATED]` <0.5ms per bar | `[ESTIMATED]` ±1 pixel-of-error accuracy (bounded by bar sprite pixel width, typically sub-1% at common resolutions) | Medium — robust to color/skin changes if bounds are anchor-relative, sensitive to bar-sprite resizing across resolutions | Low — this is arithmetic, not an asset library | **Primary path for HP/MP/EXP bar percentage**; already partially implemented as `find_color_bar` in `crate::vision::geometry` — this proposal formalizes it as a first-class decoder with accuracy validation rather than a supporting heuristic |
| **Fixed-panel hash/template lookup** (proposed) | Perceptual hash or exact-crop compare of static chrome (minimap frame, buff slot borders) against known sprite hashes | `[ESTIMATED]` <0.5ms per panel | `[ESTIMATED]` near-100% for presence/position; does not identify *dynamic content* inside the panel (e.g., which buff icon) | Medium — same asset-versioning caveat as glyph atlas | Low | Presence/position only; icon *identity* still needs a classifier (unchanged limitation, see §7 roadmap Phase 3) |
| **Trained CNN classifier** (considered, deferred) | Small classifier trained on cropped HUD regions | `[ESTIMATED]` 1–10ms per field (in-process inference) | `[ESTIMATED]` high but probabilistic; requires labeled training data and periodic retraining as the client patches | High — generalizes across minor rendering variance | High — training pipeline, dataset curation, retraining on patches, GPU/CPU inference dependency | Deferred: only worth it for genuinely ambiguous targets (icon identity, sprite classification — see `vision-architecture.md` "Known Limitations" #1/#2), not for HUD digits where determinism already gives near-perfect accuracy for free |
| **Current geometry-only heuristic (no OCR)** | Existing `hud_geometry` bar localization without digit reading | `[MEASURED]` 76–86ms (dominated by other detectors' per-frame cost, not this one specifically) | Bar position only; no numeric value | N/A | N/A (already shipped) | Superseded by bar pixel-arithmetic decoder for numeric percentage, kept for bar *localization* which the decoder still depends on |

**Net recommendation:** template/pixel decoders become the default path for HUD numerics and bar percentages; Tesseract remains wired in as the tier-2 fallback for the "unrecognized asset" case, and as the primary path for genuinely free-text regions (chat log, dialog text) where no fixed glyph atlas is possible. A CNN classifier is scoped only for icon/sprite identity work, which was already flagged as a gap in the existing architecture and is orthogonal to this redesign.

---

## 5. Game-State Reconstruction Layer

### 5.1 Why a distinct layer

Today, `PerceptionPipeline::detect` (in `crate::vision::snapshot`) directly produces `WorldState`, and `src/game_state.rs` builds `GameState` from it in a single pass per frame with no explicit reconciliation step, no provenance beyond the per-field `Source` enum, and no versioning contract. Once two tiers (deterministic + inference) can both produce a candidate value for the same field, reconciliation must be explicit and testable on its own, not implicit in a large `detect()` call.

### 5.2 Proposed schema additions

```rust
pub struct ReconstructedField<T> {
    pub value: Option<T>,
    pub confidence: Confidence,
    pub reliability: Reliability,       // now includes Deterministic
    pub source_tier: Tier,              // Deterministic | Inference | Fallback
    pub decoder: &'static str,          // e.g. "glyph_template_v3", "tesseract_ocr"
    pub asset_version: Option<AssetVersion>, // which glyph/sprite atlas produced this, if any
}

pub enum Tier { Deterministic, Inference, Fallback }

pub struct GameStateV2 {
    pub schema_version: u16,
    pub frame_seq: u64,
    pub timestamp: Timestamp,
    pub hud: ReconstructedField<HudMetrics>,
    pub motion: Vec<MovingEntity>,       // unchanged, inference-tier only
    pub dialog: ReconstructedField<DialogReading>,
    pub panels: PanelsState,
    // ... existing WorldState fields, each wrapped where a deterministic path exists
}
```

- **`schema_version`** makes `GameState` changes explicit and diffable — required for replay-based validation (§6.3) to detect when a decoder change silently altered output shape.
- **`decoder` + `asset_version`** give per-field provenance sufficient to answer "why did this value change between two builds" — critical once there are two independent code paths that can produce the HP value.
- **Reconciliation rule** when both tiers produce a value for the same field (should be rare — deterministic tier should only run where inference tier is disabled to save time, not run both live): deterministic wins unless it reports "no match," in which case inference tier's value is used and `source_tier = Fallback`.

### 5.3 Determinism guarantee for reconstruction

The reconstruction layer itself must be a pure function of `(WorldState_or_decoder_outputs, previous GameState)` — no hidden global state, no wall-clock reads beyond the timestamp field — so that **replaying the same sequence of frames through the same code always produces the same `GameState` sequence**. This is what makes golden-frame regression testing (§6.2) meaningful: a diff against a recorded expected output is only a valid signal if the pipeline is reproducible.

---

## 6. Validation Strategy

The current test suite (`[VERIFIED]` 38 unit tests + 1 integration test, `EVIDENCE.md` §9) validates that detectors *run without panicking* and that individual geometry/temporal primitives behave correctly in isolation. It does not validate **decoding accuracy against ground truth** because no ground truth exists. This is the gap the redesign must close before any accuracy numbers in §8 can move from `[ESTIMATED]` to `[MEASURED]`.

### 6.1 Ground truth without hand-labeling: synthetic rendering harness

Because the redesign's own premise is that HUD rendering is deterministic and asset-driven, the *same* asset library used for decoding can be used in reverse to **generate labeled test frames**: composite known digit glyphs and known bar-fill percentages onto a captured background at known positions, producing `(frame, expected_value)` pairs at arbitrary scale. This sidesteps the most expensive part of building a validation set (manual labeling of thousands of real screenshots) and lets accuracy be measured against an exactly-known answer rather than a human's best guess.

- `[PLANNED]` Build a `tests/synthetic_hud_corpus` generator that renders N random (value, position, resolution) combinations from the extracted glyph atlas.
- `[PLANNED]` Sweep edge cases deliberately: leading zeros, maximum digit counts, percentage boundary values (0%, 100%, values that round awkwardly), partially-occluded bars (icon overlapping bar edge), non-default UI scale.

### 6.2 Golden-frame regression corpus (real captures)

Complementary to synthetic data — synthetic frames validate the *decoder's logic*, but real captures validate the *end-to-end pipeline* against real compression artifacts, anti-aliasing, and compositing the client actually produces.

- Extend `resources/` (currently just `last.png`) into a small versioned corpus of real captures, each paired with a hand-verified `expected.json` (one-time manual labeling effort, amortized because the corpus is small and stable).
- `tests/hp_bar_integration.rs` becomes the template for a family of `tests/*_integration.rs` per decoder, each asserting exact equality against `expected.json`, not just "detection succeeded."
- CI regression gate: any PR touching a decoder must show 0 regressions against the golden corpus; corpus is small enough (`[ESTIMATED]` <50 frames) to run on every commit.

### 6.3 Shadow-mode comparison (deterministic vs. current heuristic)

Before removing OCR from the hot path for any field, run both tiers on the same frame in a non-production shadow mode and log divergences:

- `[PLANNED]` Add a `--shadow-compare` mode to `demo_video`/`measure_evidence` binaries that runs both the new deterministic decoder and the existing OCR/geometry path per frame, records agreement rate and latency for both, and flags disagreements for manual review.
- This directly produces the `[MEASURED]` accuracy numbers this document currently only estimates (§8), using the existing heuristic detector's OCR output as one (imperfect) point of comparison and the golden corpus as ground truth.

### 6.4 Statistical accuracy methodology

For each decoded field type (digit string, bar percentage, panel presence):

- **Per-field exact-match rate** against golden corpus (primary accuracy metric).
- **Per-digit error rate** for numeric fields (secondary metric — distinguishes "off by one digit" from "completely wrong").
- **Fallback-trigger rate**: how often the deterministic tier reports "no confident match" and defers to inference tier — this should be near-zero in normal operation and near-100% deliberately when testing with an out-of-library resolution/scale, confirming the escape hatch works.
- **Latency percentiles** (P50/P90/P99), reusing the existing `measure_evidence` methodology (`EVIDENCE.md` §5.1) so before/after comparisons are apples-to-apples.

All four numbers must be published together — an accuracy number without a fallback-trigger rate is misleading (a decoder that silently guesses wrong looks the same as one that correctly abstains, unless fallback-trigger rate is tracked).

---

## 7. Roadmap

| Phase | Goal | Key deliverables | Exit criteria |
|---|---|---|---|
| **0. Foundation** | Stand up ground-truth tooling before touching decoders | Synthetic corpus generator (§6.1); golden real-frame corpus + `expected.json` schema (§6.2); `GameStateV2` schema + `Tier`/`Deterministic` types (§5.2) added alongside existing types (non-breaking) | Corpus + schema merged; existing tests still green; no decoder behavior changed yet |
| **1. Bar-fill decoder** | Replace bar-percentage *inference* with pixel-arithmetic decode | `bar_fill_decoder` module; wired behind a feature flag; shadow-mode comparison run (§6.3) against current `hud_geometry` bar reading | `[MEASURED]` exact-match rate + latency published from shadow run; flag flipped on only if exact-match rate materially exceeds current heuristic and fallback path is exercised/tested |
| **2. Glyph template decoder** | Replace Tesseract for HUD digit fields | Glyph atlas extraction tool + versioning; `glyph_template_decoder` module; fallback-to-OCR wiring; golden-corpus regression tests (§6.2) | `[MEASURED]` exact-match rate on golden corpus; `[MEASURED]` latency improvement vs. Tesseract path; fallback path proven to trigger correctly on an out-of-library test resolution |
| **3. Reconstruction layer formalization** | Make provenance/versioning explicit end-to-end | `GameStateV2` fully replaces ad hoc `GameState` construction; `schema_version` bumps tracked in changelog; replay-determinism test (feed recorded frame sequence twice, assert identical output) | Replay test passes; `game_state.rs` no longer directly stitches `WorldState` — goes through reconciliation layer |
| **4. Fixed-panel decoders** | Presence/position for minimap/chat/icon-row via hash lookup | Panel sprite hash library; latency comparison vs. existing proportional-search heuristics | `[MEASURED]` latency reduction on panel detectors (currently 1–2ms combined, `[MEASURED]`, `EVIDENCE.md` — target is correctness/robustness gain more than latency here, since panels are already fast) |
| **5. Real-time reassessment** | Re-run the exact `EVIDENCE.md` methodology end-to-end with decoders live | Updated `measure_evidence` run; updated `EVIDENCE.md` claims | Publish new `[MEASURED]` FPS numbers; only claim real-time capability if the same 100-frame methodology shows it, matching this repo's existing evidentiary standard |
| **6 (stretch). Icon/sprite identity** | Close the pre-existing "known limitation" gap (icon identity, sprite classification) | Evaluate template-hash-per-known-icon vs. small CNN classifier (§4) | Out of scope for this redesign's core deliverable; tracked as a separate follow-on, not blocking phases 0–5 |

Each phase ships behind a flag and is validated against the golden/synthetic corpora before being promoted to default — this repo's own evidence history (`EVIDENCE.md` §13, the "real-time" claim that turned out `[DESIGNED BUT BROKEN]`) is the reason no phase here is allowed to update a claim in prose without a linked measurement.

---

## 8. Risks & Mitigations

| Risk | Impact | Likelihood | Mitigation |
|---|---|---|---|
| **Client patches change fonts/sprites**, silently breaking exact-match decoding | Deterministic decoder starts returning "no match" for everything (safe) or, worse, subtly wrong matches (unsafe) if match threshold is too loose | Medium — game clients patch periodically | Use exact/near-exact pixel match with a strict threshold, never a loose fuzzy threshold, for the deterministic tier; a strict threshold fails *closed* (falls back to OCR) rather than *open* (silently wrong). Track `asset_version` per decode so a patch-day spike in fallback-trigger rate is immediately visible in the metric from §6.4. |
| **Resolution/UI-scale explosion**: every supported resolution × UI scale combination needs its own glyph atlas / bar geometry constants | Maintenance burden grows combinatorially | Medium-High | Scope the asset library to the resolutions actually used (start with the one already captured in `resources/last.png`, 1366×767, `[MEASURED]`, `EVIDENCE.md` §1.1); extract atlases on demand rather than pre-generating every combination; keep OCR fallback permanently for unsupported combinations rather than trying to cover all of them deterministically. |
| **False confidence from "Deterministic" label** — a bug in the decoder or atlas produces a wrong-but-confident answer more damaging than a heuristic "guess" because downstream consumers trust it more | High if it happens (silently wrong game-state feeding automation) | Low if match threshold is strict, but consequence is severe | Golden-corpus regression tests (§6.2) run on every commit specifically to catch decoder regressions before merge; shadow-mode comparison (§6.3) required before any flag flip; never skip the "no match → fallback" path even under performance pressure. |
| **Two-tier reconciliation bugs**: inconsistent field provenance, or fallback logic that doesn't trigger when it should | Produces `GameState` fields with wrong `source_tier`, undermining the provenance goal in §5 | Medium | Reconciliation layer gets its own unit tests independent of decoders (feed it synthetic `Tier::Deterministic`/`Tier::Fallback` inputs directly, assert output selects correctly); replay-determinism test (Phase 3) also catches nondeterministic reconciliation bugs. |
| **Scope creep into ML/CNN work** diluting the deterministic-rendering thesis | Redesign timeline balloons, core latency win delayed | Medium | Icon/sprite classification (genuinely ambiguous, needs ML) is explicitly deferred to Phase 6/stretch (§7) and called out as orthogonal in §4, not blocking the deterministic decoders which are the primary latency/accuracy win. |
| **Existing detectors regress** while reconstruction layer is refactored around them | Breaks currently-passing 38 unit tests / 1 integration test | Low-Medium | Phase 0 explicitly adds `GameStateV2` *alongside* existing types rather than replacing in place; existing `hp_bar_integration.rs` and unit tests must stay green through every phase; only Phase 3 touches the stitching logic, after decoders are already validated in isolation. |
| **Validation corpus itself has labeling errors** (manual `expected.json` mistakes) | Accuracy numbers measured against wrong ground truth | Low | Keep the real-capture golden corpus small (~dozens of frames) specifically so each entry can be double-checked by hand; rely primarily on the synthetic corpus (§6.1) for volume, since its ground truth is generated, not hand-labeled, and therefore can't contain labeling errors. |

---

## 9. Expected Accuracy & Performance (targets, not yet measured)

All figures below are `[ESTIMATED]`/`[DESIGNED]` targets this redesign is accountable to, framed the same way `EVIDENCE.md` frames its measured numbers, so that Phase 5 (§7) can directly overwrite this table with `[MEASURED]` values from the same methodology.

| Field / Path | Current (`[MEASURED]`) | Target after redesign (`[ESTIMATED]`) | Measurement method once implemented |
|---|---|---|---|
| HUD digit exact-match rate | Not measured (no ground truth exists today) | ≥99.5% on in-library resolution/scale; explicit, logged fallback (not silent error) on out-of-library input | Golden + synthetic corpus exact-match rate (§6.4) |
| Bar-fill percentage error | Not measured | ≤1% absolute error (bounded by bar sprite pixel width) | Synthetic corpus with known ground-truth percentage (§6.1) |
| HUD field latency (decode only, excludes capture) | `[MEASURED]` 300–500ms when OCR triggers, 76–86ms geometry-only total pipeline (`EVIDENCE.md` §5) | 0.5–3ms per field (template/pixel decode, in-process, no subprocess) | Same `measure_evidence` harness, decoder-only timer added |
| End-to-end pipeline latency (all detectors, HUD via new decoder) | `[MEASURED]` mean 136.3ms / median 86.5ms | `[ESTIMATED]` mean <30ms once OCR is off the default hot path for HUD fields (remaining cost dominated by other detectors' existing `[MEASURED]` 5-10ms each) | Re-run of `EVIDENCE.md` §5.1 methodology, Phase 5 |
| Achievable FPS | `[MEASURED]` 11.6 FPS median, 4.0 FPS P90, 0% at 30/50/60 FPS budgets | `[ESTIMATED]` 30+ FPS median plausible; 60 FPS not promised until measured | Re-run of `EVIDENCE.md` §6 methodology, Phase 5 |
| Fallback-trigger rate (deterministic → inference) | N/A (tier doesn't exist yet) | Target near-0% on supported resolution/scale in steady state; must reach ~100% deliberately when tested against an unsupported resolution (proves the safety net works) | Dedicated fallback-trigger test in Phase 2 exit criteria |
| Existing inference-tier detectors (motion, dialog, panels, environment, combat) | `[MEASURED]` unchanged (5-10ms motion, 2-3ms dialog, 1-2ms panels — `EVIDENCE.md`/`vision-architecture.md`) | Unchanged — this redesign does not modify these detectors | No new measurement needed; regression-tested via existing suite |

**Explicit non-goal:** this document does not claim any of the target numbers above are true yet. The single biggest process risk this redesign guards against — repeating the "real-time capable at 60 FPS" mistake documented in `EVIDENCE.md` §13 — is treated as the primary reason every number in this table must be re-measured with the same published methodology before it appears in any user-facing documentation as fact.

---

## 10. Summary

The redesign's core move is narrow and low-risk: **stop asking a generic OCR engine to solve a problem the client already solved deterministically**, by building template/pixel decoders for HUD digits and bar fills, keeping the existing confidence-aware heuristic detectors as a permanent, explicitly-triggered fallback rather than deleting them. This is paired with a formalized game-state reconstruction layer (explicit provenance, versioning, replay-determinism) and a validation strategy that generates its own ground truth (synthetic rendering) rather than requiring large-scale manual labeling, closing the current gap where HUD accuracy is asserted but never measured.

Every phase in the roadmap (§7) ships behind a flag, is validated against golden/synthetic corpora before promotion, and is required to re-run this repository's existing evidence methodology (`measure_evidence`, `EVIDENCE.md`) rather than restate old claims — so that the performance and accuracy numbers in §9 move from `[ESTIMATED]` to `[MEASURED]` the same way every other claim in this codebase is expected to.

## Related documents

- [vision-architecture.md](/c:/Users/magshimim/Desktop/projects/ms.worktrees/debugging-toolkit-for-vision-systems.worktrees/perception-architecture-redesign-maplestory/docs/vision-architecture.md) — current confidence-aware detector architecture this proposal extends
- [development.md](/c:/Users/magshimim/Desktop/projects/ms.worktrees/debugging-toolkit-for-vision-systems.worktrees/perception-architecture-redesign-maplestory/docs/development.md) — quick-start/integration guide for the current pipeline
- [EVIDENCE.md](/c:/Users/magshimim/Desktop/projects/ms.worktrees/debugging-toolkit-for-vision-systems.worktrees/perception-architecture-redesign-maplestory/EVIDENCE.md) — evidence-labeling convention and current measured baseline this proposal is accountable to
- [VISION_EXPANSION_SUMMARY.md](/c:/Users/magshimim/Desktop/projects/ms.worktrees/debugging-toolkit-for-vision-systems.worktrees/perception-architecture-redesign-maplestory/VISION_EXPANSION_SUMMARY.md) — history of the current architecture's design rationale
