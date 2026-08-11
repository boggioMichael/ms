# Engineering Organization Structure - Maple Perception Engine

## Overview

The Maple perception engine is being built by a professional engineering organization with **clear separation of concerns**:

- **5 Implementation Teams** (HUD, World Geometry, Entities, Temporal, Validation)
- **1 Architecture Review Board** (MARB)

This mirrors the structure used at Waymo, NVIDIA, OpenAI, and Tesla: teams propose designs, and an independent board challenges them before implementation.

---

## Organization Chart

```
                    Maple Project Director
                            |
                ┌───────────┴───────────┐
                |                       |
         Implementation Teams        MARB
         (5 teams in parallel)    (Challenge Board)
              |
    ┌─────┬──┼──┬──────┬────────┐
    |     |  |  |      |        |
   HUD  World Entity Temporal Validation
   Team Geom Team  Team    Team
        Team
    
Architecture: Top-Down Accountability
└─ Teams design and implement
└─ MARB reviews and challenges
└─ No implementation until MARB approves
└─ MARB can require redesign
└─ MARB can reject any design
```

---

## Current Status

### Design Phase Active (Days 1-4)

**7 parallel design analysis agents** conducting rigorous subsystem research:

| Agent | Team | Subsystem | Status | Deliverable |
|-------|------|-----------|--------|------------|
| bar-measurement-analysis | HUD | HP/MP/EXP bars | Running | Design doc (12 sections) |
| text-recognition-analysis | HUD | Name/Job/Level | Running | Design doc (12 sections) |
| minimap-detection-analysis | HUD | Minimap detection | Running | Design doc (12 sections) |
| player-localization-analysis | Entity | Player sprite | Running | Design doc (12 sections) |
| platform-extraction-analysis | World | Platform geometry | Running | Design doc (12 sections) |
| dialog-detection-analysis | HUD | Dialog detection | Running | Design doc (12 sections) |
| temporal-fusion-analysis | Temporal | State fusion | Running | Design doc (12 sections) |

**Deliverables from each:**
- Problem definition (what information to recover?)
- Approach research (all feasible techniques)
- Objective comparison matrix (accuracy, latency, robustness)
- Challenge phase (what breaks this design?)
- Final recommendation (best approach + justification)
- Implementation plan with success criteria

### MARB Structure Established (Ready for Day 4)

**7 review tasks queued** (to begin after design documents complete):

| Review Task | Team | Status | Deliverable |
|-------------|------|--------|------------|
| HUD Team review | MARB | Pending | 12-section critical assessment |
| World Geometry review | MARB | Pending | 12-section critical assessment |
| Entity Tracking review | MARB | Pending | 12-section critical assessment |
| Temporal Reasoning review | MARB | Pending | 12-section critical assessment |
| Validation review | MARB | Pending | 12-section critical assessment |
| Architecture-wide assessment | MARB | Pending | Systemic issue analysis |
| Approval gate | MARB | Pending | Go/No-Go decision |

---

## Implementation Process

### Phase 0: Design (Days 1-4)

**What happens:**
- 7 agents conduct independent design research
- Each agent produces rigorous technical analysis
- All 5 teams submit design documents

**What does NOT happen:**
- NO code is written
- NO implementations started
- NO architecture finalized

**Blocking gate:**
- Cannot proceed until all designs complete

### Phase 1: Architecture Review (Days 4-13)

**What happens:**
- MARB launches parallel review agents
- Each reviewer produces 12-section critical analysis
- Reviewers challenge every assumption
- Architecture-wide assessment identifies systemic issues
- MARB issues formal Architecture Review Board Report

**What each reviewer evaluates:**
1. Summary (what is being proposed?)
2. Strengths (what is good?)
3. Weaknesses (what's fragile, wrong assumptions?)
4. Missing research (what alternatives weren't considered?)
5. Technology comparison (could another approach be better?)
6. Failure analysis (what will break? when? why?)
7. Scalability (works on 1 map? 100 maps? 4K resolution?)
8. Maintainability (will engineers understand this in 1 year?)
9. Debuggability (can failures be understood?)
10. Confidence (how does detector know it's correct?)
11. Validation (how to PROVE correctness, not estimate?)
12. Recommendation (APPROVE / APPROVE WITH CHANGES / REJECT?)

**What does NOT happen:**
- No code written
- No implementation begins
- No approval gate passes

**MARB authority:**
- Can REJECT entire designs
- Can require fundamental redesign
- Can block all implementation until satisfied
- Can recommend major architectural changes

### Phase 2: Approval Gate (Day 15)

**What happens:**
- MARB issues final go/no-go decision
- If APPROVE: Implementation begins (Day 16)
- If APPROVE WITH CHANGES: Teams remediate, resubmit (Day 16+)
- If REJECT: Back to design phase (Day 20+)

**Critical principle:**
- **NO IMPLEMENTATION UNTIL MARB APPROVES**

### Phase 3: Implementation (Days 16-35)

Only begins after MARB approval gate passes.

5 teams implement in parallel:
- HUD detector implementation
- World geometry detector implementation
- Entity tracking implementation
- Temporal fusion implementation
- Validation framework

Each team validates against:
- Unit tests (correctness)
- Integration tests (interaction)
- Test frame (resources/maplestory.png with known ground truth)

### Phase 4: Validation (Days 35-50)

All detectors validated against:
- Replay dataset (200+ diverse frames)
- Ground truth annotations
- Accuracy metrics
- Regression tests

### Phase 5: Production (Day 50+)

System deployed with:
- 95%+ accuracy on all detectors
- <10ms latency per frame
- Full confidence models
- Comprehensive validation tests
- Complete documentation

---

## MARB Principles

MARB operates under these uncompromising principles:

### 1. Assume Wrong
- Assumes every design is wrong until proven otherwise
- Does NOT trust previous decisions
- Does NOT protect prior work
- Challenges every assumption from first principles

### 2. Challenge Actively
- Actively tries to REJECT designs (not approve them)
- Looks for flaws, not strengths
- Proposes alternatives when design is weak
- Refuses to rubber-stamp

### 3. Reference Standards
- Compares against Waymo perception standards
- Compares against NVIDIA architecture standards
- Compares against OpenAI/DeepMind research standards
- References CVPR/NeurIPS peer review standards

### 4. Block When Necessary
- Will REJECT fundamental flaws
- Will BLOCK implementation indefinitely if needed
- Will require redesign before approval
- Does NOT defer to implementation team preferences

### 5. Provide Evidence
- All recommendations justified with evidence
- All alternatives compared systematically
- All risks documented with mitigation
- All assumptions challenged and addressed

---

## Key Responsibilities

### Implementation Teams
- **Own**: Subsystem design and implementation
- **Deliver**: Working code + tests + documentation
- **Report to**: MARB for approval
- **Cannot override**: MARB rejection

### MARB
- **Owns**: Architectural review and approval
- **Delivers**: Design reviews + go/no-go decision
- **Authority**: Can reject any design
- **Cannot do**: Implement code or override user requirements

### Users/Project Owner
- **Owns**: Overall vision and success criteria
- **Sets**: Quality standards and timelines
- **Authorizes**: MARB charter and decisions
- **Cannot**: Override MARB approval gate (quality mechanism)

---

## Success Criteria for MARB

A MARB review succeeds if:

1. **Every flaw identified before implementation**
   - No major design issues discovered during Phase 1 coding
   - No redesigns required mid-Phase 1

2. **Technology choices justified against alternatives**
   - Every detector approach compared against 3+ alternatives
   - Recommendation grounded in evidence, not opinion

3. **Validation strategy proves correctness**
   - Not estimates or simulations
   - Ground truth framework comprehensive
   - Accuracy metrics measurable and verifiable

4. **Confidence in architecture increases**
   - Teams understand why designs are sound
   - Developers can explain architectural choices
   - Future maintainers understand trade-offs

5. **Implementation proceeds confidently**
   - No architectural surprises during Phase 1
   - No fundamental redesigns needed
   - Teams execute plans with minimal changes

---

## Documents

- **[docs/TEAM_STRUCTURE.md](C:/Users/magshimim/Desktop/projects/ms.worktrees/debugging-toolkit-for-vision-systems/docs/TEAM_STRUCTURE.md)**
  - Full team charters
  - Roadmap with timelines
  - Success metrics
  - Risk analysis

- **[docs/MARB_CHARTER.md](C:/Users/magshimim/Desktop/projects/ms.worktrees/debugging-toolkit-for-vision-systems/docs/MARB_CHARTER.md)**
  - MARB mission and authority
  - 12-section review template
  - Review standards and gates
  - Independence guarantees

- **[docs/ARCHITECTURE_ANALYSIS.md](C:/Users/magshimim/Desktop/projects/ms.worktrees/debugging-toolkit-for-vision-systems/docs/ARCHITECTURE_ANALYSIS.md)**
  - Systems-level architecture
  - First-principles design principles
  - Approach research categories
  - Quality gates for all implementations

---

## Next Steps

### Waiting For (Days 1-4)
- All 7 design analysis agents to complete
- Teams to finalize and submit designs

### MARB Activation (Day 4)
- Launch 5 parallel review agents (one per implementation team)
- Launch architecture-wide review agent
- Produce critical analysis reports

### MARB Decision (Day 15)
- Issue Architecture Review Board Report
- Go/No-Go decision on implementation
- List of required remediations (if any)

### Implementation Authorization (Day 15+)
- Only if MARB approves
- Teams proceed to Phase 1 coding
- Follow approved architecture precisely

---

## The Philosophy Behind MARB

This organization structure embodies a key principle:

**Flaws discovered during design are exponentially cheaper to fix than flaws discovered during implementation.**

- Design flaw fix: Edit document (hours)
- Implementation flaw fix: Rewrite code (days)
- Deployment flaw fix: Post-hoc patches (weeks) + reliability damage (ongoing)

MARB's purpose: **Prevent expensive engineering mistakes by reviewing rigorously before any code is written.**

This is how the best technology companies build perception systems: design → review → challenge → redesign if needed → code (only after approval).

---

**Status**: Organization structure established, design phase active, MARB ready to review (Day 4)

**Next milestone**: Design documents complete, MARB reviews begin

**Critical gate**: No implementation begins until MARB approves
