# MapleStory Perception Redesign: Deterministic Game-State Reconstruction

## Status

- [Designed] This document is the authoritative engineering proposal for replacing the current OCR-centric perception approach with a correctness-first, deterministic perception stack.
- [Measured] The current implementation has already demonstrated that the OCR-first strategy is insufficient on a real MapleStory frame.
- [Planned] The deliverables below are the next engineering steps after the design review.

## 1. Executive Summary

- [Designed] The system should be redesigned as a real-time game perception engine, not as a general-purpose screenshot text reader.
- [Designed] MapleStory should be treated as a structured rendering environment with strong layout priors, stable UI semantics, and predictable temporal behavior.
- [Designed] The perception stack should reconstruct the game state by combining specialized detectors whose recognition strategy matches the properties of each observed object.
- [Designed] Generic OCR should be used only as a last-resort fallback for a very small number of low-risk text regions, not as the primary strategy for core game-state recovery.
- [Designed] The architecture should be built around localization, specialized recognition, temporal fusion, confidence propagation, and replay-based validation.

## 2. Core Design Principles

### 2.1 Deterministic rendering first

- [Designed] Every detector should exploit the fact that MapleStory renders with fixed UI layout, fixed sprite conventions, fixed palette structure, fixed animation timing, and fixed world-space semantics.
- [Designed] The system should reconstruct game state from the rendered scene by aligning its perception modules to those deterministic properties.

### 2.2 Correctness before performance

- [Designed] The first architecture should optimize for the best possible measurement quality, not the fastest possible approximation.
- [Designed] Performance should be addressed after the recognition strategy is correct and validated.

### 2.3 Specialized detectors over generic inference

- [Designed] HP/MP/EXP should be measured with bar geometry and fill-ratio analysis.
- [Designed] Character name/job/level should be recognized with bitmap-font and constrained vocabulary methods.
- [Designed] Minimap should be detected by layout and panel-template matching.
- [Designed] Platforms should be extracted as traversable surfaces, not as arbitrary edge-like regions.

### 2.4 Temporal consistency is a first-class signal

- [Designed] Health, level, job, and UI state should not be treated as independent frame-level guesses.
- [Designed] The system should exploit temporal continuity, state persistence, and confidence accumulation to reject transient single-frame failures.

## 3. Proposed Architecture

### 3.1 High-level pipeline

- [Designed] Input normalization: capture frame, timestamp, resolution, and metadata.
- [Designed] ROI prior generation: identify stable regions of interest for HUD, minimap, chat, player, world-space, and UI panels.
- [Designed] Specialized perception: run the detector stack tailored to each subsystem.
- [Designed] Temporal fusion: integrate the current frame with short history and stateful trackers.
- [Designed] Confidence fusion: combine evidence from geometry, color, templates, and temporal history.
- [Designed] Game-state assembly: emit a structured, serializable game-state object for downstream AI use.

### 3.2 System structure

- [Designed] Frame-level state processor.
- [Designed] Detector registry with typed outputs.
- [Designed] Temporal memory module.
- [Designed] Confidence fusion engine.
- [Designed] Ground-truth evaluator and replay runner.

### 3.3 Detector dependency graph

```text
[Frame Capture]
      |
      v
[ROI Priors]
   /   |    |    \ 
  v    v    v     v
[HUD] [Player] [UI Panels] [World Geometry]
  |      |         |            |
  v      v         v            v
[Character Text] [NPC/Monster] [Dialog/Chat] [Platforms]
      \         |              / 
       \        v             /
        +--> [Temporal Fusion] --> [GameState]
```

- [Designed] HUD and character text depend on ROI priors.
- [Designed] World geometry and UI panels are kept separate so UI borders do not contaminate platform extraction.
- [Designed] Temporal fusion consumes all detector outputs and produces a stable belief state.

## 4. Detector-by-Detector Engineering Study

### 4.1 HP detector

#### Purpose

- [Designed] Recover current HP, maximum HP, percentage, and confidence from the visible HP bar.

#### Possible approaches

- [Designed] Filled-pixel measurement with calibrated bar geometry.
- [Designed] Color segmentation of the filled portion and empty portion.
- [Designed] Border/edge detection around the bar body.
- [Designed] Template matching against known bar shapes.
- [Designed] Hybrid geometry + color + template matching.
- [Designed] OCR as a fallback only.

#### Evaluation

| Approach | Accuracy | Latency | Determinism | Failure modes | Complexity | Recommendation |
|---|---:|---:|---:|---|---|---|
| Filled-pixel ratio | [Designed] Very high | [Designed] Very low | [Designed] Very high | [Designed] Edge blur, partial occlusion | [Designed] Low | [Designed] Primary |
| Color segmentation | [Designed] High | [Designed] Very low | [Designed] High | [Designed] Palette drift | [Designed] Low | [Designed] Primary |
| Border detection | [Designed] Medium | [Designed] Low | [Designed] High | [Designed] Bar outline ambiguity | [Designed] Medium | [Designed] Support |
| Template matching | [Designed] High | [Designed] Low | [Designed] High | [Designed] Skin/resolution mismatch | [Designed] Medium | [Designed] Support |
| OCR fallback | [Designed] Low | [Designed] Medium | [Designed] Low | [Designed] Misread numbers | [Designed] Low | [Designed] Never primary |

#### Chosen approach

- [Designed] Use a hybrid detector combining fixed ROI localization, bar border detection, color segmentation of filled and empty regions, and a calibrated fill-ratio estimator.
- [Designed] The detector should output current HP, maximum HP, percentage, and a confidence score derived from bar geometry, fill continuity, previous-frame stability, and layout validation.

#### Why this is optimal

- [Designed] The HP bar is a geometric, deterministic feature with strong prior structure.
- [Designed] Its rendered state is directly related to the underlying game value.
- [Designed] A bar detector is more robust than OCR because it measures the actual filled proportion rather than guessing a number from text.

### 4.2 MP detector

#### Purpose

- [Designed] Recover current MP, maximum MP, percentage, and confidence.

#### Approach comparison

| Approach | Accuracy | Latency | Determinism | Failure modes | Complexity | Recommendation |
|---|---:|---:|---:|---|---|---|
| Geometry + fill ratio | [Designed] Very high | [Designed] Very low | [Designed] Very high | [Designed] Color similarity to adjacent UI | [Designed] Low | [Designed] Primary |
| Color segmentation | [Designed] High | [Designed] Very low | [Designed] High | [Designed] Shared UI colors | [Designed] Low | [Designed] Primary |
| Template matching | [Designed] High | [Designed] Low | [Designed] High | [Designed] Template drift | [Designed] Medium | [Designed] Support |
| OCR | [Designed] Low | [Designed] Medium | [Designed] Low | [Designed] Text ambiguity | [Designed] Low | [Designed] Never primary |

#### Chosen approach

- [Designed] Use the same deterministic bar-reconstruction strategy as HP, with independent color thresholds and ROI priors for the MP bar.

### 4.3 EXP detector

#### Purpose

- [Designed] Recover EXP percentage and confidence, including support for animated bars and changing fill states.

#### Approach comparison

| Approach | Accuracy | Latency | Determinism | Failure modes | Complexity | Recommendation |
|---|---:|---:|---:|---|---|---|
| Filled-ratio geometry | [Designed] Very high | [Designed] Very low | [Designed] Very high | [Designed] Animation blur | [Designed] Low | [Designed] Primary |
| Edge + contour analysis | [Designed] High | [Designed] Low | [Designed] High | [Designed] Partial overlap with UI | [Designed] Medium | [Designed] Support |
| OCR | [Designed] Low | [Designed] Medium | [Designed] Low | [Designed] Misread percent values | [Designed] Low | [Designed] Never primary |
| ML classifier | [Designed] Medium | [Designed] Medium | [Designed] Medium | [Designed] Data dependence | [Designed] High | [Designed] Later |

#### Chosen approach

- [Designed] Use filled-pixel measurement with temporal smoothing because EXP values evolve continuously and should not jump from one frame to the next.

### 4.4 Character name detector

#### Purpose

- [Designed] Recover the visible character name with high precision and confidence.

#### Possible approaches

- [Designed] Bitmap-font glyph recognition.
- [Designed] Glyph template matching against a sprite/font dictionary.
- [Designed] Constrained OCR over a known text plate.
- [Designed] CNN-based character classifier.
- [Designed] Hybrid region localization + bitmap-font decoding.

#### Evaluation

| Approach | Accuracy | Latency | Determinism | Failure modes | Complexity | Recommendation |
|---|---:|---:|---:|---|---|---|
| Bitmap-font recognition | [Designed] Very high | [Designed] Very low | [Designed] Very high | [Designed] Font mismatch | [Designed] Medium | [Designed] Primary |
| Glyph template matching | [Designed] High | [Designed] Low | [Designed] High | [Designed] Missing templates | [Designed] Medium | [Designed] Primary |
| Constrained OCR | [Designed] Medium | [Designed] Medium | [Designed] Medium | [Designed] Similar-looking glyphs | [Designed] Low | [Designed] Fallback |
| CNN classifier | [Designed] Medium | [Designed] Medium | [Designed] Medium | [Designed] Data dependence | [Designed] High | [Designed] Later |

#### Chosen approach

- [Designed] Localize the name plate using a fixed ROI and then decode characters using a bitmap-font dictionary and template matching with a small, constrained vocabulary for the expected name length and format.

#### Why this is optimal

- [Designed] Character names are rendered with a fixed client font and consistent layout.
- [Designed] A bitmap-font approach has far better determinism and precision than unrestricted OCR.

### 4.5 Job detector

#### Purpose

- [Designed] Recover the current job class from the visible UI text field.

#### Possible approaches

- [Designed] Constrained dictionary matching over the job plate.
- [Designed] Glyph template matching against a job-name atlas.
- [Designed] OCR over the job plate.
- [Designed] CNN classifier over the job plate.

#### Chosen approach

- [Designed] Use constrained dictionary recognition with a closed set of job labels and a bitmap-font recognizer over the job plate.
- [Designed] The detector should never treat job recognition as generic OCR over the whole image.

### 4.6 Level detector

#### Purpose

- [Designed] Recover the visible player level from the level plate.

#### Possible approaches

- [Designed] Digit template matching.
- [Designed] Bitmap digit classifier.
- [Designed] OCR over the level plate.
- [Designed] CNN digit recognizer.

#### Chosen approach

- [Designed] Use a digit-only template recognizer over the localized level plate. This is a strongly constrained problem and is far more robust than generic OCR.

### 4.7 Quest window detector

#### Purpose

- [Designed] Detect whether a quest or dialog window is visible and recover its type and boundaries.

#### Possible approaches

- [Designed] Window template matching.
- [Designed] Panel segmentation and border detection.
- [Designed] Layout-based panel classification.
- [Designed] OCR over the window content.
- [Designed] Hybrid template + OCR.

#### Chosen approach

- [Designed] Use window-template matching for localization and layout-based panel classification for type recognition. OCR is used later only for content parsing if needed.

### 4.8 Chat detector

#### Purpose

- [Designed] Recover whether chat is visible and optionally decode text from the localized chat region.

#### Possible approaches

- [Designed] Region-based layout detection.
- [Designed] Font-decoding over a constrained region.
- [Designed] OCR over the chat panel.
- [Designed] Text line segmentation plus constrained font model.

#### Chosen approach

- [Designed] First detect chat presence through layout and panel geometry. Then, if needed, apply a constrained font decoder over the localized chat region rather than generic OCR over the whole screen.

### 4.9 Minimap detector

#### Purpose

- [Designed] Recover whether the minimap is visible, its bounds, and its validity as a UI panel.

#### Possible approaches

- [Designed] Template matching against the known minimap panel shape.
- [Designed] Edge and border detection around the expected corner region.
- [Designed] Color clustering and panel segmentation.
- [Designed] OCR-based UI detection.
- [Designed] Hybrid layout + border + icon verification.

#### Chosen approach

- [Designed] Use a hybrid layout-template detector that searches the upper-left quadrant for the minimap panel, validates its border geometry, and verifies it by checking the expected internal icon or map texture structure.

#### Why this is optimal

- [Designed] The minimap is a fixed UI panel with predictable placement and shape, so it should be detected as a panel, not as an arbitrary region of the scene.

### 4.10 Player detector

#### Purpose

- [Designed] Recover the player bounding box, feet position, center point, facing direction, animation state, and confidence.

#### Possible approaches

- [Designed] Template matching against player sprite atlases.
- [Designed] Segmentation of the player silhouette.
- [Designed] Feature matching with a known sprite library.
- [Designed] Object detection network.
- [Designed] Hybrid sprite atlas + temporal tracking.

#### Chosen approach

- [Designed] Use a hybrid pipeline: sprite-atlas template matching for coarse localization, contour-based silhouette extraction for precise bounds, and temporal tracking for stable identity and motion state.

### 4.11 NPC detector

#### Purpose

- [Designed] Recover NPC identity, interaction state, bounding box, and confidence.

#### Possible approaches

- [Designed] Sprite atlas matching.
- [Designed] Feature matching with known NPC templates.
- [Designed] Object detection network.
- [Designed] Hybrid atlas + temporal tracker.

#### Chosen approach

- [Designed] Use sprite-atlas matching for identification and temporal tracking for stability. This is more robust than a general object detector because the NPC set is finite and the appearance is constrained by the game client.

### 4.12 Monster detector

#### Purpose

- [Designed] Recover monster identity, bounding box, tracking, predicted future position, and confidence.

#### Possible approaches

- [Designed] Sprite atlas matching.
- [Designed] Motion-based blob tracking.
- [Designed] Object detection network.
- [Designed] Hybrid atlas + tracker + temporal prediction.

#### Chosen approach

- [Designed] Use a hybrid architecture: atlas matching for identity and contour-based tracking for bounding boxes, with temporal prediction used to maintain continuity across frames.

### 4.13 Platform detector

#### Purpose

- [Designed] Recover traversable surfaces and reject UI borders, decorative lines, and non-walkable geometry.

#### Possible approaches

- [Designed] Edge detection plus geometric grouping.
- [Designed] World-space region segregation plus contour extraction.
- [Designed] Template matching for known platform shapes.
- [Designed] ML segmentation network.
- [Designed] Hybrid world-space filtering + edge extraction + temporal validation.

#### Chosen approach

- [Designed] Use a hybrid world-space geometry pipeline: remove UI regions first, detect candidate surfaces in the remaining world-space area, group connected contours into platform hypotheses, reject non-walkable shapes, and apply temporal consistency to stabilize the platform graph.

#### Why this is optimal

- [Designed] The old platform detector was fundamentally flawed because it treated arbitrary horizontal edges as walkable geometry.
- [Designed] The new design explicitly separates game world geometry from UI geometry and extracts actual traversable surfaces.

## 5. Temporal Perception Architecture

### 5.1 Why temporal reasoning is required

- [Designed] Health values do not jump randomly.
- [Designed] Character level, job, and name do not change on a frame-to-frame basis.
- [Designed] UI windows remain stable until the scene changes.
- [Designed] Moving objects follow continuous motion.

### 5.2 Temporal fusion model

- [Designed] Each detector should produce a belief state with a confidence and evidence summary.
- [Designed] The temporal module should fuse current beliefs with historical state using smoothing, persistence, and prediction.
- [Designed] Confidence should increase when repeated evidence agrees and should decay when observations are missing or contradictory.

### 5.3 Components

- [Designed] Short-horizon state tracker for HUD values.
- [Designed] Object tracker for player/NPC/monster entities.
- [Designed] UI state tracker for windows and panels.
- [Designed] Platform persistence tracker for world geometry.

## 6. Confidence Propagation Model

- [Designed] Each detector should output value, confidence, evidence, and failure reason.
- [Designed] Confidence should be a combination of measurement confidence and temporal support.
- [Designed] A detector should explain why it believes something, not just emit a scalar.

### 6.1 Example: HP confidence

- [Designed] Evidence sources: bar geometry, fill ratio, edge continuity, layout validation, and previous-frame agreement.
- [Designed] The fused confidence should increase when all signals agree and decrease when only weak evidence exists.

### 6.2 Example: character name confidence

- [Designed] Evidence sources: ROI localization, glyph recognition, temporal stability, and vocabulary consistency.

## 7. Ground Truth and Validation Strategy

### 7.1 Ground-truth schema

- [Planned] Every frame should carry expected values for HP, MP, EXP, name, job, level, player position, map id, minimap visibility, dialogs, NPCs, monsters, platforms, and UI state.

### 7.2 Replay dataset requirements

- [Planned] The benchmark dataset should be gameplay recordings, not single screenshots.
- [Planned] It should contain idle, walking, jumping, combat, loot, NPC interaction, inventory, quest dialogs, boss fights, multiple maps, multiple UI scales, multiple resolutions, multiple classes, and varying weather and lighting.

### 7.3 Evaluation method

- [Planned] Run every detector over the replay dataset and compare outputs against frame-level ground truth.
- [Planned] Produce per-detector accuracy, precision, recall, and confusion metrics.
- [Planned] Publish the results as machine-readable JSON and human-readable reports.

## 8. Accuracy Targets

| Detector | Designed target |
|---|---:|
| HP | [Designed] 95%+ |
| MP | [Designed] 95%+ |
| EXP | [Designed] 95%+ |
| Name | [Designed] 95%+ |
| Job | [Designed] 95%+ |
| Level | [Designed] 95%+ |
| Minimap | [Designed] 95%+ |
| Quest dialogs | [Designed] 90%+ |
| Player pose | [Designed] 90%+ |
| NPC identity | [Designed] 85%+ |
| Monster identity | [Designed] 85%+ |
| Platform geometry | [Designed] 90%+ |

## 9. Failure Analysis

### 9.1 Common failure modes

- [Designed] UI skin changes create template mismatch.
- [Designed] Resolution changes break fixed ROI assumptions.
- [Designed] Lighting or palette shifts reduce color-based confidence.
- [Designed] Temporal occlusion makes object trackers lose continuity.
- [Designed] Platform extraction can still confuse decorative geometry if the world/UI segmentation is imperfect.

### 9.2 Failure handling policy

- [Designed] Detectors should explicitly report uncertainty instead of silently making a wrong prediction.
- [Designed] Temporal fusion should preserve prior state when current evidence is weak.
- [Designed] Confidence should decay gracefully during occlusion or partial visibility.

## 10. Detector Technology Allocation

### 10.1 Classical CV

- [Designed] HP/MP/EXP bar measurement.
- [Designed] Minimap localization.
- [Designed] Quest window detection.
- [Designed] Global UI panel detection.
- [Designed] Platform geometry extraction.
- [Designed] Player silhouette and motion analysis.

### 10.2 Bitmap/font matching

- [Designed] Character name recognition.
- [Designed] Job recognition.
- [Designed] Level recognition.
- [Designed] Chat text recognition where the font is known and constrained.

### 10.3 Machine learning

- [Designed] ML should be used only where the problem is genuinely statistical and the deterministic baseline is insufficient.
- [Designed] Candidate ML areas: NPC/monster identity under strong appearance variation, low-level segmentation of visually complex objects, and confidence calibration.

### 10.4 What should never use OCR as the primary strategy

- [Designed] HP, MP, EXP bars.
- [Designed] Character name, job, and level.
- [Designed] Minimap detection.
- [Designed] Dialog window detection.
- [Designed] Platform extraction.
- [Designed] Player/NPC/monster identity.

## 11. Generalization Strategy for Future Games

- [Designed] The architecture should be organized around a generic perception core with game-specific adapters.
- [Designed] The detector registry should be pluggable so new games can swap in new templates, fonts, and layout priors without rewriting the whole pipeline.
- [Designed] The temporal fusion and confidence engine should remain game-agnostic.
- [Designed] The game-specific knowledge layer should be isolated from the general perception engine.

## 12. Implementation Roadmap Ordered by ROI

### Phase 1: Establish the correctness baseline

- [Planned] Rebuild the HUD bar detector around geometric fill-ratio measurement.
- [Planned] Rebuild name/job/level recognition around bitmap-font and constrained vocabulary methods.
- [Planned] Rebuild minimap detection around layout and panel-template matching.

### Phase 2: Add temporal stability

- [Planned] Add deterministic state persistence and confidence accumulation.
- [Planned] Add tracker modules for player/NPC/monster entities.

### Phase 3: Add environment understanding

- [Planned] Rebuild platform extraction around world-space geometry filtering and traversable-surface recognition.

### Phase 4: Add validation infrastructure

- [Planned] Build replay capture, frame-level ground truth, and automated accuracy reporting.

### Phase 5: Add learned calibration

- [Planned] Use machine learning only for confidence calibration and edge-case classification after the deterministic stack is mature.

## 13. Final Recommendation

- [Designed] The correct architecture is not an OCR project. It is a deterministic, specialized, confidence-aware game perception stack.
- [Designed] The highest-value path is to rebuild the perception system around geometric bar measurement, bitmap-font recognition, layout-based UI panel detection, sprite-based object identification, world-space geometry extraction, and temporal fusion.
- [Designed] This approach is the most credible path to a production-grade MapleStory game-state reconstruction engine.
