# Vision System Architecture

## Overview

The vision subsystem is a professional, production-grade perception pipeline for MapleStory screen analysis. It replaces the earlier "debug toolkit" framing with a rigorous, confidence-aware, temporally-consistent architecture designed to support a scalable AI decision-making layer.

A detailed redesign proposal for the perception strategy is documented in [perception-redesign.md](perception-redesign.md). That document is the authoritative design reference for the shift from generic OCR toward deterministic rendering-based perception.

Every detector in this system returns a [`Detection<T>`] struct that carries:
- **value**: The detected information (or `None` if detection failed)
- **confidence**: A `Confidence` score (`f32` in `[0.0, 1.0]`)
- **timestamp**: When the detection was captured (wall-clock milliseconds)
- **source**: Which detector produced this information (enum: `Hud`, `Motion`, `Dialog`, etc.)
- **reliability**: A qualitative estimate (`Corroborated`, `Heuristic`, `Predicted`, `Unreliable`)
- **failure_reason**: A human-readable explanation if detection failed

This structure ensures that downstream AI consumers (and developers debugging) can:
- Distinguish "very confident the HP is 50%" from "found a red bar, might be HP"
- Track which detector produced each piece of information
- Understand *why* a detector succeeded or failed this frame

## Core Types

### `crate::vision::types`

- **`Confidence(f32)`**: Newtype wrapping confidence in `[0.0, 1.0]`. Methods:
  - `combine(other)`: probabilistic OR for corroborating signals
  - `decay(factor)`: reduce confidence to account for stale observations
  - `is_confident(threshold)`: quick threshold check

- **`Source`**: Enum labeling detector origin: `Hud`, `Motion`, `Dialog`, `Panel`, `Environment`, `Combat`

- **`Reliability`**: Qualitative trust estimate:
  - `Corroborated`: multiple independent signals agree (e.g., color bar + OCR text)
  - `Heuristic`: single geometric/color signal, no independent confirmation
  - `Predicted`: derived from history/prediction, no direct evidence this frame
  - `Unreliable`: detection failed

- **`Detection<T>`**: The universal detector output contract.

- **`Timestamp`**: Wall-clock capture time in milliseconds since Unix epoch (cheaply `Copy`).

## Temporal Reasoning

### `crate::vision::temporal`

Temporal continuity is one of the strongest signals in video. This module provides small, reusable building blocks:

- **`Ema`**: Exponential moving average for scalar signals (e.g., bar percentages, FPS-like series). O(1) time/space per sample.

- **`ConfidenceAccumulator`**: Raises confidence when consecutive observations agree and decays when they don't or observation drops out. Prevents single bad frames from causing values to flicker.

- **`ObjectTracker`**: Minimal centroid-distance multi-object tracker:
  - Assigns stable IDs to detections across frames via nearest-neighbor matching
  - Linear velocity prediction for position on missed frames (occlusion recovery)
  - Configurable grace period (how many frames to survive occlusion before dropping)
  - O(existing_tracks × new_detections) per frame (negligible at expected entity counts)

- **`History<T>`**: Timestamped ring buffer for recent frame-level samples (e.g., motion magnitude), bounded size without unbounded growth.

## Shared Geometry Helpers

### `crate::vision::geometry`

Centralized, deduplicated pixel/rectangle geometry primitives used by every detector:

- **`Rect`**: Generic axis-aligned rectangle. Methods: `center()`, `area()`, `x2()`, `y2()`.

- **Pixel predicates**:
  - `is_color_pixel()`: HSV-based hue/saturation/value matching
  - `is_text_pixel()`: Text-like brightness/desaturation/alpha heuristic
  - `alpha_is_high()`: Alpha opacity threshold

- **Row segmentation**:
  - `segment_row()`: Find runs of min-width-or-longer pixels along a row matching a predicate
  - `group_segments()`: Greedy vertical run-grouping into rectangles

- **Region search**:
  - `find_horizontal_region()`: Largest contiguous rectangle matching a predicate
  - `find_text_block()`: Bounding box of text-like pixels in a region
  - `find_color_bar()`: Solid-color bar (e.g., HP/MP bar)
  - `find_color_regions()`: All color-matching regions
  - `dominant_color_bucket()`: Most-common color (quantized) in region for skin-agnostic panel detection
  - `find_uniform_color_panel()`: Largest solid-color region matching a quantized bucket

## Detectors

All detectors live in `crate::vision::detectors` and implement a consistent interface. Stateless detectors (single-frame) use `fn detect(&self, image: &RgbaImage) -> Detection<T>`. Stateful detectors own their state and use `fn detect(&mut self, image: &RgbaImage) -> Detection<T>`.

### HUD Detector

**Module**: `crate::vision::detectors::hud`

Detects and parses HP/MP/EXP bars and character name/job/level plates via:
1. Geometry-based color-bar localization (raw implementation in `hud_geometry`)
2. OCR text parsing for exact numeric values
3. Confidence scoring: `Corroborated` (geometry + OCR digits), `Heuristic` (geometry-only), or `missing`

**Public API**:
- `HudDetector`: stateless detector
- `HudReading`: output struct with `markers` (geometry) and `Detection<HudMetric>` for each stat plus `Detection<String>` for text fields

The underlying `hud_geometry` module is intentionally kept separate because it was tuned iteratively against real captured frames and is tested independently. The confidence layer wraps it rather than rewriting it.

### Motion Detector

**Module**: `crate::vision::detectors::motion`

Detects moving entities via frame differencing:
1. Compute luminance diff mask between current and previous frame
2. Extract connected blobs from the diff mask
3. Feed blobs into `ObjectTracker` for stable IDs across frames
4. Report `MovingEntity` structs with position, velocity, age, and is-predicted flag

**Stateful**: Owns previous frame and `ObjectTracker` instance.

**Configuration**: `MotionConfig` with tunable diff threshold, min blob size, track match distance, grace frames.

**Known limitation**: Detects *that* something moved, not *what* it is. Sprite classification would require a trained classifier.

### Dialog Detector

**Module**: `crate::vision::detectors::dialog`

Detects dialog/popup/notification panels (death, revive, rune activation, quest dialogs):
1. Find largest uniform-color region in the central search band (skin-agnostic via `dominant_color_bucket`)
2. OCR the panel text
3. Classify the text via `crate::knowledge::dialogs::classify()` keyword matching
4. Return `DialogReading` with bounds and classified `DialogKind`

**Configuration**: `DialogConfig` with quantization step and min area fraction.

### Panel Detectors

**Module**: `crate::vision::detectors::panels`

Three fixed-position UI panel detectors:

- **`MinimapDetector`**: Locates minimap as largest solid-color panel in top-left quadrant
- **`ChatLogDetector`**: Locates chat log as dense text block in lower-left quadrant
- **`IconRowDetector`**: Detects small saturated icon blobs (buff/cooldown icons) in top-right

All use proportional search regions rather than fixed pixel offsets, so they adapt to any resolution.

**Known limitation**: Icon row identifies *that* icons are present and how many, not *which* buffs/skills they represent. Buff identification requires either a visual icon classifier (not available) or server state.

### Environment Detector

**Module**: `crate::vision::detectors::environment`

Detects platform/foothold edges via vertical luminance gradients:
1. Scan frame for pixels with strong vertical brightness discontinuity
2. Group horizontally contiguous runs
3. Report as `PlatformEdge` candidates

**Known limitation**: Reports candidate edges, not a verified walkable-foothold graph. A real platform understanding would require matching detected edges against the map's collision data.

### Combat Intensity Detector

**Module**: `crate::vision::detectors::combat`

Meta-detector combining motion and history to classify combat intensity:
- Maintains moving average of per-frame diff magnitude
- Combines with current `ObjectTracker` entity count
- Classifies into: `Idle`, `Light`, `Moderate`, `Heavy`

**Stateful**: Owns moving average history.

Used by downstream AI for heuristics like "don't start a new action while combat intensity is heavy".

## Knowledge Base

### `crate::knowledge`

Structured, non-verbatim, original-authored knowledge about MapleStory gameplay. All facts are general public knowledge or normalized summaries, not verbatim-scraped content.

**Modules**:

- **`dialogs`**: `DialogKind` enum and keyword-based classification for death, revive, level-up, rune-activation, generic, and none dialogs.

- **`mechanics`**: `RUNE_MECHANICS`, `PORTAL_MECHANICS`, `FARMING_HEURISTICS` with gameplay rules and rationale.

- **`monsters`**: `MonsterBehavior` table for common early-game monsters (Snail, Blue Snail, Slime, Stump, Wild Boar, Ribbon Pig) with aggression, movement patterns, and threat assessment.

Design philosophy: zero-parse (const Rust data structures), zero-allocation, and explicitly documented provenance so future developers understand "this is general knowledge, not licensed content".

## Pipeline Orchestrator

### `crate::vision::snapshot`

- **`WorldState`**: Aggregated snapshot struct collecting output from all detectors into one convenient bag.

- **`PerceptionPipeline`**: Owns all detector instances and produces one `WorldState` per frame. Downstream AI queries this struct without needing to know which detector produced each piece of information.

```rust
let mut pipeline = PerceptionPipeline::new();
let state = pipeline.detect(&image);
// Access: state.hud, state.motion, state.dialog, etc.
// Each field is a Detection<T> carrying confidence/reliability/failure reason
```

## Design Principles

### 1. Confidence-Aware, Never Binary

Every detector returns a confidence score and reliability estimate. "Found / not found" is insufficient; we distinguish:
- "Very confident the HP is 50% (corroborated by geometry + OCR)"
- "Found a red bar that might be HP (heuristic-only, 55% confidence)"
- "No HP bar found; OCR text confirms unusual UI state (predicted, using history)"

### 2. Temporal Continuity

Single-frame detections are noisy. The system uses:
- **Smoothing** (EMA) for scalar signals
- **Tracking** (ObjectTracker) for entities
- **Accumulation** (ConfidenceAccumulator) for repeated observations
- **History** (History<T>) for meta-detectors

So "HP flickered between 25% and 50% for 2 frames" is recognized as an OCR hiccup, not a real HP swing.

### 3. Minimal Allocations

- Most APIs work with borrowed `&RgbaImage` references
- Result buffers (Vec<Rect>, Vec<MovingEntity>) are small and data-dependent
- No unnecessary image copies
- Geometry helpers are reused across detectors (no duplication)

### 4. Failure Transparency

Failed detections don't return bare `None`. They return `Detection::missing(source, "reason")` so:
- Developers can trace why a detector missed this frame
- Downstream AI can distinguish "plate visible but OCR couldn't read text" from "plate not found"
- Temporal reasoning can account for transient glitches

### 5. Extensible Architecture

Adding a new detector:
1. Create `src/vision/detectors/my_detector.rs`
2. Implement a detector struct with `detect()` method
3. Define an output type wrapping the detected information
4. Return `Detection<MyOutput>` with appropriate confidence/reliability
5. Add to `PerceptionPipeline` in `snapshot.rs`

The shared geometry helpers (`crate::vision::geometry`) and temporal primitives (`crate::vision::temporal`) are available for reuse.

## Performance Characteristics

- **Frame processing**: O(width × height) for most detectors (pixel iteration)
- **Motion tracking**: O(existing_tracks × new_blobs)
- **OCR**: Bottleneck; Tesseract is subprocess-based and ~100-300ms per region
- **Memory**: One frame stored (motion detector), no unbounded buffers

For typical 1366×767 MapleStory capture at 50 FPS:
- Motion detector frame diff + tracking: ~5-10ms
- Dialog detection (without OCR): ~2-3ms
- Panel detection: ~1-2ms
- HUD detection (with OCR): ~150-250ms (mostly Tesseract)

The pipeline is designed to be called once per captured frame.

## Known Limitations & Future Work

1. **Sprite Classification**: Motion detector identifies moving blobs, not sprite types. A trained CNN would enable "player vs monster vs item drop" classification.

2. **Icon Classification**: Icon row detector counts icons but cannot identify which buff/skill each represents. Either visual icon matching or server state integration would be needed.

3. **Map/Collision Understanding**: Environment detector finds platform edges, not a verified walkable collision graph. Integration with map data would enable path finding.

4. **OCR Reliability**: Tesseract OCR is sometimes unreliable on small text or unusual fonts. Confidence scoring accounts for this, but perfect OCR is not guaranteed.

5. **Lighting/Skin Variations**: While detectors use skin-agnostic heuristics (color bucket quantization, text pixel predicates), extreme lighting or custom UI skins might cause misdetections.

6. **Temporal Coherence Across Mode Changes**: If the player alt-tabs or the game window minimizes and restores, the motion detector's frame baseline is lost. This is handled (detector reports "frame size changed"), but velocity prediction is reinitialized.

## Testing

- **Unit tests**: Every detector module has unit tests validating core logic.
- **Integration test**: `tests/hp_bar_integration.rs` exercises HUD detection on a real captured screenshot (`resources/maplestory.png`).
- **All tests pass**: `cargo test` runs 36 unit tests + 1 integration test.

## Extending Documentation

To add a new detector:
1. Document its behavior, confidence scoring logic, and known limitations in its module doc comment.
2. Add unit tests covering at least:
   - A successful detection
   - A negative case (no target present)
   - An edge case (tiny/huge input, empty frame, etc.)
3. Update this architecture doc with the detector's role and limitations.

To integrate new knowledge:
1. Add to `crate::knowledge` as structured, const Rust data (no runtime parsing).
2. Document provenance (e.g., "common gameplay knowledge" vs "empirically observed heuristic").
3. Ensure it's accessible without parsing/allocation.

## Key Files

- `src/vision/types.rs`: Core `Detection<T>`, `Confidence`, `Source`, `Reliability` contract
- `src/vision/temporal.rs`: Temporal primitives (EMA, tracker, accumulator)
- `src/vision/geometry.rs`: Shared geometry helpers
- `src/vision/hud_geometry.rs`: Low-level HUD geometry + OCR logic (battle-tested)
- `src/vision/detectors/*.rs`: Individual detector implementations
- `src/vision/snapshot.rs`: `PerceptionPipeline` orchestrator
- `src/knowledge/*.rs`: Structured MapleStory knowledge
- `src/hud.rs`: Convenience re-export for backward compatibility
- `src/util/`: Timing, pixel, image operation helpers
- `tests/hp_bar_integration.rs`: Real-world validation test

## References

- Temporal reasoning design: inspired by classic multi-object tracking (centroid nearest-neighbor, Kalman prediction, occlusion grace)
- Confidence modeling: influenced by Bayesian probability and multi-signal corroboration
- UI detection: color-quantization for skin-agnostic panel localization; OCR confidence scoring based on independent signal availability
