# MapleStory Perception System: Complete Architectural Analysis

## STEP 1: Problem Definition

### What are we building?

A **real-time game perception engine** that reconstructs complete, accurate MapleStory game state from pixel data at video framerates (30+ FPS minimum), with high confidence estimates and graceful degradation under occlusion or UI changes.

### What information must we recover?

**Character State:**
- Current HP, maximum HP, percentage
- Current MP, maximum MP, percentage
- Current EXP, maximum EXP, percentage
- Character name (player identity)
- Job/class
- Level
- Position (x, y screen coordinates)
- Facing direction
- Animation state (idle, walking, jumping, attacking, hit, dead)
- Buffs active
- Debuffs active

**World State:**
- Player location on current map
- Map identifier
- Platform/terrain geometry (traversable surfaces)
- Visible environment objects
- Weather/lighting conditions

**Entities:**
- NPCs (identity, position, interaction state)
- Monsters (identity, position, health, predicted trajectory)
- Loot drops (type, position, priority)
- Projectiles/effects (type, position, trajectory)

**UI State:**
- Minimap (visible? position? content?)
- Chat log (visible? text content?)
- Inventory (visible? item state?)
- Quest tracker (visible? quest information?)
- Dialog windows (visible? type? content?)
- Skill cooldowns (visible icons, cooldown state)
- Death prompt (visible?)
- Status effects indicators

**Temporal Information:**
- Frame timestamp
- Frame index
- Frame-to-frame deltas (velocity, state changes)
- Confidence trends (is confidence increasing or decreasing?)

### Measurement of success

For each information class, we need:

1. **Accuracy**: How close to ground truth (where ground truth is known)
2. **Confidence**: How certain is the system about its estimate
3. **Latency**: Processing time per frame
4. **Robustness**: How often does detection fail completely
5. **Stability**: Do values flicker frame-to-frame or remain temporally coherent
6. **Generalization**: Does it work across maps, skins, resolutions, and UI scales

---

## STEP 2: First-Principles System Design

### Design constraint: MapleStory is NOT an arbitrary image

MapleStory is a **deterministic client-side renderer** with:

- Fixed UI layout with known geometry priors
- Fixed sprite atlases for all game objects
- Fixed color palettes for UI elements
- Fixed bitmap fonts for text rendering
- Fixed animation frame sequences
- Deterministic platform layouts per map
- Stable rendering order (background, platforms, NPCs, monsters, player, effects, UI)
- Fixed position offsets for UI panels relative to screen edges and each other

### Design opportunity: Leverage determinism

Unlike arbitrary scene understanding:

- We know the HUD bars are colored rectangles at specific screen locations
- We know the character name is rendered with a specific font at a specific position
- We know the minimap always appears in the top-left corner (when visible)
- We know platforms are the only game-space geometry that matters for pathfinding
- We know monsters and NPCs come from a finite atlas
- We know UI elements have stable borders and distinctive colors

### Proposed architecture

```
[Raw Frame]
     |
     v
[Input Normalization]
  - Capture timestamp, resolution, metadata
  - Apply any preprocessing (histogram equalization if needed)
     |
     v
[Layout Prior Estimation]
  - Identify safe ROIs for HUD, minimap, chat, world-space based on resolution
  - Detect if UI is visible or hidden
  - Detect UI scale (affects all measurements)
     |
     +----> [HUD Subsystem]          [Minimap Subsystem]      [Chat Subsystem]
     |           |                       |                       |
     |           v                       v                       v
     |      [Bar Measurement]      [Panel Detection]       [Text Extraction]
     |           |                       |                       |
     |           v                       v                       v
     |      [Text Recognition]      [Border Validation]    [Font Decoding]
     |           |                       |                       |
     |           +--- [Character Identity] ---+                  |
     |                                        |                  |
     |                                        v                  v
     +----> [World Subsystem]         [UI State]         [Dialog Detection]
     |           |                       |                       |
     |           v                       v                       v
     |      [Sprite Localization]  [Panel Geometry]      [Window Template Match]
     |           |                       |                       |
     |           v                       v                       v
     |      [Player Extraction]     [Visibility State]    [Content Parsing]
     |           |
     |           v
     |      [NPC/Monster Detection]
     |           |
     |           v
     |      [Platform Extraction]
     |
     +----> [Temporal Fusion]
                 |
                 v
         [Confidence Accumulation]
                 |
                 v
         [State Persistence & Prediction]
                 |
                 v
           [GameState Assembly]
                 |
                 v
            [Serialization]
                 |
                 v
            [Output]
```

### Key architectural principles

1. **Separation of concerns**: HUD, UI panels, and world-space perception are separate pipelines that converge at temporal fusion
2. **Geometric prior first**: Every detector starts by localizing the ROI using known layout before attempting recognition
3. **Specialization over generalization**: Each detector uses the specific technique optimal for that object class
4. **Confidence throughout**: Every measurement carries confidence and evidence, not just a binary yes/no
5. **Temporal as first-class**: History and consistency are integrated into every detector, not added as post-processing
6. **Fallback hierarchies**: Detectors have primary and fallback strategies; failure is explicit, not silent

---

## STEP 3: Research the Solution Space

### Approach categories for this problem

#### A. Geometric & Color-Based Detection
- **Strengths**: Deterministic, fast, interpretable, no training required
- **Use for**: Bar measurement, UI panel detection, platform edges, color-based object identification
- **Examples**: Fill-ratio measurement, edge detection, color segmentation, contour analysis

#### B. Template Matching
- **Strengths**: Precise localization, handles known patterns, robust to lighting
- **Use for**: Sprite identification, window borders, known UI elements
- **Examples**: 2D cross-correlation, feature matching, multi-scale template search

#### C. Bitmap Font Recognition
- **Strengths**: Perfect accuracy for known fonts, deterministic, fast
- **Use for**: Character name, job class, level numbers
- **Examples**: Glyph dictionary, bitmap matching, constrained vocabulary decoding

#### D. OCR (Optical Character Recognition)
- **Strengths**: Flexible, works with unknown text
- **Use for**: Dialog content, chat text, quest information (fallback position)
- **Examples**: Tesseract, EasyOCR, commercial OCR APIs
- **Weakness for this domain**: High false positive rate on small/styled text

#### E. Segmentation & Connected Components
- **Strengths**: Finds all instances, no templates required
- **Use for**: Entity detection, platform segmentation, object clustering
- **Examples**: Blob detection, watershed, flood fill, contour extraction

#### F. Tracking & Motion Analysis
- **Strengths**: Maintains identity across frames, predicts motion
- **Use for**: Player tracking, monster tracking, entity association
- **Examples**: Kalman filter, Hungarian algorithm, optical flow, centroid tracking

#### G. Classical Computer Vision Pipelines
- **Strengths**: Interpretable, debuggable, real-time capable
- **Use for**: All geometric and structural problems
- **Examples**: Hough transform, morphological operations, statistical analysis

#### H. Machine Learning (CNN/Transformer)
- **Strengths**: Learns complex patterns, handles variation
- **Use for**: Identity classification where deterministic methods fail (monsters under occlusion)
- **Weakness for this domain**: Requires training data, slower, less interpretable, harder to debug
- **When to use**: Only after deterministic methods plateau

#### I. Hybrid Systems
- **Strengths**: Combines strengths of multiple approaches
- **Use for**: Everything (start with geometry, fall back to learning)

---

## STEP 4: Subsystem Dependency Graph

```
[Input Frame]
     |
     v
[Resolution & Scale Detection]
     |
     +-- determines ROI priors for all subsystems
     |
     v
[HUD Subsystem]                [World Subsystem]            [UI Panel Subsystem]
  |                              |                            |
  +-- Bar Measurement            +-- Player Sprite            +-- Minimap
  |   (HP/MP/EXP %)              |   Localization             +-- Chat
  |                              |                            +-- Dialogs
  +-- Text Recognition           +-- World Space              +-- Inventory
  |   (Name/Job/Level)           |   Segmentation             +-- Quest Tracker
  |                              |
  +-- Buffs/Debuffs              +-- NPC Detection
  |                              |
  +-- Status Indicators          +-- Monster Detection
                                 |
                                 +-- Platform Extraction
                                 |
                                 +-- Loot Detection

       [All Subsystems]
              |
              v
       [Temporal Fusion]
              |
              v
       [Confidence Accumulation]
              |
              v
       [State Persistence]
              |
              v
       [GameState Assembly]
              |
              v
          [Output]
```

### Why this dependency order?

1. **Resolution detection** affects all ROI calculations
2. **HUD subsystem** is independent; requires only resolution
3. **World subsystem** requires resolution; independent from HUD
4. **UI panels** require resolution; mostly independent
5. **Temporal fusion** consumes all and produces stable output

---

## STEP 5: Quality Gates

Before implementing any subsystem, it must pass:

1. **Problem clarity**: Can we state the exact information to recover?
2. **Approach research**: Have we identified and compared all realistic techniques?
3. **Objective evaluation**: Can we measure accuracy/latency/robustness?
4. **Failure analysis**: Do we understand and can we handle failure modes?
5. **Temporal design**: Have we planned temporal consistency?
6. **Confidence model**: Can we explain why we believe something?
7. **Validation data**: Do we have ground truth to validate against?

---

## STEP 6: Implementation Prerequisites

**Before ANY implementation:**

1. Capture a **validation dataset** (multiple frames from various scenarios)
2. Establish **ground truth** (what SHOULD each detector output?)
3. Define **accuracy metrics** (how do we measure success?)
4. Design **test cases** (what scenarios must we handle?)
5. Plan **performance benchmarks** (latency budget per subsystem)

**During implementation:**

1. Write unit tests immediately
2. Run against validation dataset after each component
3. Track accuracy metrics continuously
4. Document all assumptions and failure modes

---

## STEP 7: Confidence Model

Every detector output should include:

```rust
struct DetectorOutput<T> {
    value: Option<T>,
    confidence: f32,  // [0.0, 1.0]
    evidence: Vec<Evidence>,  // Why we believe this
    failure_reason: Option<String>,  // Why we don't believe it
}

enum Evidence {
    GeometricMatch { score: f32 },
    ColorMatch { hue_match: bool, saturation: f32, brightness: f32 },
    TemplateMatch { correlation: f32, best_match_scale: f32 },
    OcrConfirmation { raw_text: String, confidence: f32 },
    TemporalPersistence { frames_consistent: u32, consistency_score: f32 },
    ConsensusVote { agreeing_signals: usize, total_signals: usize },
}
```

Confidence should be a **function of evidence**, not a constant:

```
confidence = f(geometric_evidence, color_evidence, template_evidence, temporal_evidence, consensus)
```

---

## Next Steps

This architecture document establishes the foundation. The next analysis covers **each subsystem in order**:

1. ✅ Systems Architecture (this document)
2. Bar Measurement (HP/MP/EXP)
3. Text Recognition (Name/Job/Level)
4. Minimap Detection
5. Dialog Detection
6. Player Localization
7. NPC/Monster Detection
8. Platform Extraction
9. Temporal Fusion
10. Integration & Validation

Each subsystem will receive:
- Complete problem definition
- All approach research
- Objective comparison
- Challenge & refinement
- Final recommendation with justification
- Detailed implementation phases
- Validation criteria
