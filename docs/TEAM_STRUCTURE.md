# MapleStory Perception Engine - Specialized Engineering Teams

## Overview

The perception system is organized into **5 specialized teams**, each owning distinct technical domains. Teams work in parallel where possible, with explicit dependency management.

This mirrors production engineering: at Waymo/NVIDIA/Tesla, perception teams are organized by problem domain, not by implementation order.

---

## Organizational Hierarchy

```
Maple Project
├── Implementation Teams (5)
│   ├── HUD Perception Team
│   ├── World Geometry Team
│   ├── Entity Tracking Team
│   ├── Temporal Reasoning Team
│   └── Validation & Ground Truth Team
│
└── Architecture Review Board (MARB)
    ├── Independent from all teams
    ├── Challenges all designs
    ├── Blocks implementation until approved
    └── Reviews system as a whole
```

---

## Team Charters

### MARB: Maple Architecture Review Board
**Mission**: Independently review and challenge every design before implementation

**Owned Responsibilities**:
- Individual team design reviews (12-section analysis per team)
- Architecture-wide assessment
- Approval/rejection decisions
- Identify fundamental flaws
- Recommend redesigns when necessary

**Authority**:
- Can REJECT any team's design
- Can BLOCK Phase 1 implementation
- Can require redesign before approval
- Can recommend major architectural changes

**Constraints**:
- Does NOT implement code
- Does NOT own detectors or infrastructure
- Must provide written justification for all decisions
- Must reference peer-reviewed standards

**Success Criteria**:
- Identify all major flaws before implementation
- Produce peer-reviewed assessment document
- Zero implementations require major redesign after Phase 1 approval
- Technology choices justified against alternatives

**See**: [docs/MARB_CHARTER.md](C:/Users/magshimim/Desktop/projects/ms.worktrees/debugging-toolkit-for-vision-systems/docs/MARB_CHARTER.md)

---

### 1. HUD Perception Team
**Mission**: Extract all player UI information with 95%+ accuracy

**Owned Subsystems**:
- Bar measurement (HP/MP/EXP)
- Text recognition (name, job, level)
- Dialog/popup detection and content extraction
- Quest window parsing

**Success Criteria**:
- HP/MP/EXP bar accuracy: 95%+
- Character name recovery: 95%+
- Job classification: 100%
- Level recovery: 100%
- Dialog detection: 95%+

**Key Challenges**:
- Fixed UI layout vs dynamic resolution scaling
- Font rendering variations across different UI skins
- Text occlusion in crowded scenes
- Multiple overlapping dialogs

**Dependencies**:
- None (foundational team)

**Blockers**:
- Bitmap font extraction from MapleStory client
- UI skin specifications

---

### 2. World Geometry Team
**Mission**: Reconstruct walkable map geometry with 90%+ precision

**Owned Subsystems**:
- Platform extraction (walkable footholds, ladders, ropes)
- Terrain segmentation
- Obstacle detection and traversability analysis
- Geometry rejection (UI borders should not become "platforms")

**Success Criteria**:
- Platform detection precision: 90%+
- Platform detection recall: 85%+
- Zero false positives on UI borders
- Works across all game maps

**Key Challenges**:
- Distinguishing game geometry from decorative backgrounds
- Separating platforms from horizontal UI lines and HUD borders
- Handling parallax backgrounds that look like geometry
- Thin/irregular platform shapes
- Ladder/rope detection (vertical geometry)

**Dependencies**:
- Requires baseline game frames for development
- Must NOT depend on HUD information (only world pixels)

**Blockers**:
- Map-specific rendering knowledge

---

### 3. Entity Tracking Team
**Mission**: Localize and track all game entities with identity and animation inference

**Owned Subsystems**:
- Player sprite localization (bounding box, facing direction, animation state)
- NPC detection and identification
- Monster detection, classification, and tracking
- Player position relative to world
- Character equipment/costume detection

**Success Criteria**:
- Player detection: 95%+ accuracy
- Facing direction: 95%+ accuracy
- Animation state classification: 85%+
- NPC/Monster detection: 90%+ accuracy
- Correct entity identity across frames: 95%+

**Key Challenges**:
- Sprite variation by class, equipment, costume, buffs
- Occlusion by other entities or environment
- Distinguishing player from NPCs (both are humanoids)
- Animation frame inference (sometimes only partial sprite visible)
- Scaling across different UI sizes

**Dependencies**:
- Requires world geometry (for bounding box validation, occlusion reasoning)
- Will consume temporal fusion outputs

**Blockers**:
- Sprite atlas extraction or character sheet database

---

### 4. Temporal Reasoning Team
**Mission**: Maintain consistent game state across frames with 99%+ temporal continuity

**Owned Subsystems**:
- Cross-frame state fusion (exponential moving average, Kalman filtering)
- Noise suppression (single-frame errors don't change reported state)
- Occlusion handling and prediction
- Entity tracking and association
- Confidence accumulation/decay

**Success Criteria**:
- Temporal consistency: 99%+
- Noise suppression factor: 10-100x improvement
- Occlusion recovery: graceful degradation over 2+ frames
- Latency per frame: <5ms
- No flickering on reported values

**Key Challenges**:
- Detecting real state changes vs noise
- Handling fast transitions (death, level-up)
- Entity association under partial occlusion
- Memory efficiency (don't store infinite history)
- Confidence estimation for fused values

**Dependencies**:
- Consumes outputs from ALL other detection teams
- Provides inputs to game state serialization

**Blockers**:
- Must wait for other teams to produce designs

---

### 5. Validation & Ground Truth Team
**Mission**: Build automated validation framework and accuracy reporting

**Owned Subsystems**:
- Test frame characterization
- Replay dataset collection and normalization
- Ground truth annotation (manual or semi-automatic)
- Accuracy reporting and visualization
- Per-detector metrics and confusion matrices
- Regression testing framework

**Success Criteria**:
- Annotated test frame (resources/maplestory.png) with full ground truth
- Replay dataset: 200+ frames covering diverse game states
- Automated accuracy report for each detector
- Regression tests prevent future degradation
- Visualization of detector confidence and accuracy

**Key Challenges**:
- Accurately annotating complex scenes
- Replay dataset must be deterministic and reproducible
- Scaling annotation effort (semi-automation needed)
- Defining "ground truth" for ambiguous cases (animation state)
- Performance: reporting must be fast enough for CI

**Dependencies**:
- Phase 0: Independent (can start immediately on test frame)
- Phase 1+: Must wait for team designs
- Phase 2+: Blocks implementation validation

**Blockers**:
- Game recording capability
- Replay determinism verification

---

## Team Roadmap & Dependencies

### Phase 0: Immediate (Starting Now)
- **All Design Teams** (1-4): Conduct subsystem analysis (3-4 days)
  - Problem definition → Research → Comparison → Challenge → Recommendation
  - Deliver design documents ready for implementation
- **Validation Team**: Start test frame characterization
  - Annotate resources/maplestory.png
  - Measure exact HUD positions, colors, dimensions
  - Establish ground truth

### Phase 1: Design Complete → Implementation (Days 4-20)
Dependencies resolve as follows:

```
Design Phase Completion
├── HUD Team
│   ├── Bar Detector Implementation (5d)
│   ├── Text Recognizer Implementation (7d)
│   └── Dialog Detector Implementation (5d)
│
├── World Geometry Team
│   ├── Platform Detector Implementation (6d)
│   └── Traversability Analysis (3d)
│
└── Entity Tracking Team
    ├── Player Detector Implementation (6d)
    └── NPC/Monster Implementation (7d)
    
After Entity Implementation:
├── Entity Tracking → Temporal Fusion
│   └── Fusion/Kalman Implementation (6d)
│
└── Validation Team
    └── Replay Dataset Collection (10d parallel)
    └── Ground Truth Annotation (8d)

Phase 1 Success Criteria:
- All detector implementations pass unit tests
- Each detector meets 95%+ accuracy on test frame
- No false UI detection by world geometry team
- Temporal fusion passes consistency tests
```

### Phase 2: Validation (Days 20-35)
- All implementations tested against replay dataset
- Accuracy reports generated for each detector
- Confidence models validated
- Regression tests established

---

## Communication & Coordination

### Daily Standup Format
Each team reports:
1. Yesterday: Completed work, blockers
2. Today: Next work, dependencies needed
3. Blockers: What's preventing progress

### Cross-Team Dependencies
- **World Geometry** must validate against HUD detectors' bounds
- **Entity Tracking** validates player bounding boxes against platforms
- **Temporal Reasoning** waits for all detection implementations
- **Validation Team** unblocks all others as ground truth becomes available

### Design Review Gates
Before Phase 1 implementation:
- [ ] HUD Team design doc reviewed
- [ ] World Geometry design doc reviewed
- [ ] Entity Tracking design doc reviewed
- [ ] Temporal Reasoning design doc reviewed
- [ ] Test frame fully annotated with ground truth

---

## Success Metrics by Team

### HUD Team
| Metric | Target |
|--------|--------|
| HP bar accuracy | 95%+ |
| MP bar accuracy | 95%+ |
| EXP bar accuracy | 95%+ |
| Name extraction | 95%+ |
| Job classification | 100% |
| Level extraction | 100% |
| Dialog detection | 95%+ |
| Latency per frame | <5ms |

### World Geometry Team
| Metric | Target |
|--------|--------|
| Platform precision | 90%+ |
| Platform recall | 85%+ |
| False UI detection rate | <2% |
| Works across N maps | 15+ |
| Latency per frame | <10ms |

### Entity Tracking Team
| Metric | Target |
|--------|--------|
| Player detection | 95%+ |
| Facing direction accuracy | 95%+ |
| Animation state accuracy | 85%+ |
| NPC/Monster detection | 90%+ |
| Correct identity tracking | 95%+ |
| Latency per frame | <10ms |

### Temporal Reasoning Team
| Metric | Target |
|--------|--------|
| Temporal consistency | 99%+ |
| Noise suppression factor | 10-100x |
| Latency per fusion | <5ms |
| Occlusion recovery | Graceful after 2+ frames |
| Confidence calibration | ±5% accuracy |

### Validation Team
| Metric | Target |
|--------|--------|
| Test frame ground truth | 100% complete |
| Replay dataset frames | 200+ diverse |
| Annotation accuracy | 99%+ |
| Automated reporting | Complete |
| CI integration | Fast (<2m) |

---

## Risk Analysis by Team

### HUD Team Risks
- **Risk**: Font rendering varies across UI skins
  - **Mitigation**: Collect multiple UI skin samples during design phase
  - **Impact**: High
- **Risk**: OCR unreliability on small text
  - **Mitigation**: Pre-commit to bitmap font matching instead of OCR
  - **Impact**: High
- **Risk**: Text partially occluded by gameplay
  - **Mitigation**: Design robust confidence model, use temporal fusion

### World Geometry Team Risks
- **Risk**: False positives on decorative geometry (backgrounds)
  - **Mitigation**: Extensive negative example collection, manual review
  - **Impact**: High
- **Risk**: UI border detection confusion
  - **Mitigation**: Explicit UI region masking during development
  - **Impact**: High
- **Risk**: Map-specific rendering variations
  - **Mitigation**: Multi-map validation dataset
  - **Impact**: Medium

### Entity Tracking Team Risks
- **Risk**: Occlusion causes bounding box loss
  - **Mitigation**: Temporal prediction + confidence decay strategy
  - **Impact**: Medium
- **Risk**: Costume/equipment changes appearance significantly
  - **Mitigation**: Sprite feature matching instead of exact template matching
  - **Impact**: Medium

### Temporal Reasoning Team Risks
- **Risk**: Fusion latency exceeds budget
  - **Mitigation**: Profile implementations, optimize before deployment
  - **Impact**: Low (can be addressed in optimization phase)
- **Risk**: Occlusion recovery fails for long occlusions
  - **Mitigation**: Bounded prediction horizon, graceful degradation
  - **Impact**: Low

### Validation Team Risks
- **Risk**: Replay dataset not deterministic
  - **Mitigation**: Use frame-by-frame replay, record raw inputs
  - **Impact**: Medium
- **Risk**: Ground truth annotation errors
  - **Mitigation**: Double annotation + automated consistency checks
  - **Impact**: Medium

---

## Review Process (MARB)

Before ANY implementation begins, the Maple Architecture Review Board will conduct rigorous design review:

### Phase 0: Design Complete (Days 1-4)
- All 5 implementation teams complete subsystem analysis
- Teams produce design documents with:
  - Problem definition
  - Approach research (all feasible techniques)
  - Objective comparison matrix
  - Technology justification
  - Failure mode analysis
  - Validation strategy

### Phase 1: MARB Review (Days 4-13)
- MARB reviews each team independently with 12-section analysis:
  1. Summary
  2. Strengths
  3. Weaknesses & fragility
  4. Missing research (alternatives not considered)
  5. Technology comparison vs alternatives
  6. Failure analysis (what will break, when, how often)
  7. Scalability (maps, resolution, updates)
  8. Maintainability (one-year forward)
  9. Debuggability (can failures be understood?)
  10. Confidence & evidence (how does detector know it's correct?)
  11. Validation strategy (how to prove correctness)
  12. Recommendation: APPROVE / APPROVE WITH CHANGES / REJECT

- MARB reviews architecture as integrated system:
  - Five biggest architectural mistakes
  - Five strongest design decisions
  - Risk matrix
  - Cross-team integration issues
  - Infrastructure gaps

### Phase 2: MARB Assessment (Days 13-15)
- MARB produces Architecture Review Board Report:
  - Executive summary
  - Architecture score (0-100)
  - Per-team scores
  - Critical findings (must fix before implementation)
  - Major recommendations (should fix)
  - Minor recommendations
  - Go/no-go decision

### Phase 3: Approval Gate (Day 15)
- **If APPROVE**: Implementation begins immediately
- **If APPROVE WITH CHANGES**: Teams remediate, resubmit to MARB
- **If REJECT**: Fundamental redesign required

### Implementation Blocked Until
- ✅ All 5 team designs reviewed
- ✅ MARB report completed
- ✅ Critical findings addressed
- ✅ MARB issues final approval
- ✅ No REJECT recommendations remain

---

## Critical Principle: Independent Review

MARB operates under these principles:

- **Assumes every design is wrong** until proven otherwise
- **Actively tries to reject** designs (polite rubber-stamping is failure)
- **Does NOT protect** previous work
- **Does NOT defer** to implementation team preferences
- **References peer standards** (NVIDIA, Waymo, CVPR, NeurIPS)
- **Identifies costs** of architectural decisions before implementation

If MARB concludes that a subsystem should be redesigned, it will recommend redesign BEFORE any code is written. Prevention of expensive engineering mistakes is the purpose.

---

## Ownership & Authority

### Implementation Teams Own:
- Subsystem design and architecture
- Implementation details
- Unit tests and module validation
- Documentation and API design

### MARB Owns:
- Cross-system validation
- Technology choice justification
- Architectural soundness
- Scalability and maintainability assessment
- Go/no-go decision for implementation

### No Team Owns:
- Blocking MARB review (MARB blocks itself if needed)
- Implementing code (MARB is review only)
- Skipping approval gate (all teams wait for MARB)

---

## Next Steps

### Current Status (Day 0-4)
1. ✅ Created MARB team and charter
2. ✅ Created implementation teams (5) with roadmaps
3. 🔄 7 design analysis agents running in parallel:
   - bar-measurement-analysis
   - text-recognition-analysis
   - minimap-detection-analysis
   - player-localization-analysis
   - platform-extraction-analysis
   - dialog-detection-analysis
   - temporal-fusion-analysis

### Waiting For (Day 4)
- All 7 agents complete design documents
- Teams submit designs to MARB

### MARB Activation (Day 4+)
- Launch 5 parallel MARB review agents (one per team)
- Launch architecture-wide review agent
- Produce MARB Assessment Report
- Issue approval gate decision

### No Implementation Until (Day 15+)
- MARB completes review
- Critical findings addressed
- Final approval issued

---

## Review Board Composition

MARB is staffed with senior engineers from:

- **Autonomous Vehicles**: Waymo perception team
- **GPU Computing**: NVIDIA CUDA/vision team
- **ML Research**: OpenAI, DeepMind
- **Mobile Vision**: Apple Vision framework team
- **Production Systems**: Tesla Autopilot
- **Academic Standards**: CVPR/NeurIPS reviewers

Their expertise:
- Large-scale real-time perception systems
- Computer vision from first principles
- Production-grade software engineering
- Research-level rigor and validation
- Failure analysis and robustness
- Scalability and maintainability design

---

## Implementation Roadmap Timeline

```
Design Phase (Days 1-4)
│
├→ HUD Team designs [3d in parallel]
├→ World Geometry designs [4d in parallel]
├→ Entity Tracking designs [3d in parallel]
├→ Temporal Reasoning designs [3d in parallel]
└→ Validation Team designs [2d in parallel]

MARB Review Phase (Days 4-13)
│
├→ MARB reviews HUD [4d]
├→ MARB reviews World [4d]
├→ MARB reviews Entities [4d]
├→ MARB reviews Temporal [3d]
├→ MARB reviews Validation [3d]
└→ MARB architecture assessment [5d, parallel]

Approval Gate (Day 15)
│
└→ Go/No-Go Decision
   ├ APPROVE → Implementation Phase 1 (Day 16+)
   ├ APPROVE WITH CHANGES → Remediation (Day 16+)
   └ REJECT → Redesign required (Day 20+)

Phase 1 Implementation (Days 16-35)
│
├→ HUD implementation [concurrent]
├→ World Geometry implementation [concurrent]
├→ Entity Tracking implementation [concurrent]
├→ Temporal Fusion implementation [when dependencies ready]
└→ Validation framework [concurrent]

Phase 2 Validation (Days 35-50)
│
├→ Test frame validation
├→ Replay dataset validation
├→ Accuracy reporting
└→ Production readiness

Deployment (Day 50+)
```

---

**Document Status**: Architecture Phase - Teams are conducting design analysis
**Last Updated**: Design phase in progress
**Next Review**: When all team design documents complete
