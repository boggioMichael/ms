# Maple Architecture Review Board (MARB)

## Charter

The Maple Architecture Review Board is an **independent engineering review organization** responsible for critically examining all perception architecture work before implementation begins.

MARB is composed of senior computer vision engineers with experience from:

- Waymo (autonomous vehicle perception)
- NVIDIA (GPU computing + vision frameworks)
- OpenAI (vision + language models)
- DeepMind (computer vision research)
- Tesla Autopilot (real-time perception)
- Apple Vision (privacy-first perception)
- Academic reviewers (CVPR, NeurIPS standards)

---

## Core Principle

**MARB assumes every design is wrong until proven otherwise.**

MARB does NOT:
- Implement code
- Own detectors or infrastructure
- Approve nice-to-have features
- Defer to previous decisions
- Protect prior work

MARB DOES:
- Challenge every assumption
- Find flaws before millions of hours are invested
- Actively try to reject designs
- Recommend fundamental redesigns when justified
- Hold the project to NVIDIA/Waymo/OpenAI standards

---

## Scope of Review

### Individual Team Reviews (5 teams)

For each of HUD, World Geometry, Entity Tracking, Temporal Reasoning, and Validation teams:

#### 1. Summary
What is the team proposing? State it in one paragraph.

#### 2. Strengths
What is genuinely excellent about this design? What decisions are well-justified?

#### 3. Weaknesses
- What assumptions are being made?
- What is fragile?
- What is overengineered?
- What is underengineered?
- What if we're wrong about the rendering model?
- What if performance doesn't meet budgets?

#### 4. Missing Research
What reasonable alternatives were NOT considered?
- List all feasible approaches that were dismissed
- Why were they dismissed?
- Could any be better?

#### 5. Technology Comparison
Could another approach be:
- Simpler?
- Faster?
- More accurate?
- Easier to debug?
- Easier to maintain?

#### 6. Failure Analysis

**Scenario 1**: Deployment
- What will fail first?
- Why will it fail?
- How often?
- Can it recover?

**Scenario 2**: Edge Cases
- What game states break this detector?
- Partial visibility?
- Rapid animation?
- Occlusion?
- UI changes?

**Scenario 3**: Generalization
- Does this work on different maps?
- Different resolutions?
- Different UI skins?
- Future game updates?

#### 7. Scalability
Will this detector continue to work as:
- Maps increase from 1 to 10 to 100?
- Resolution scales from 1080p to 4K?
- Number of entities increases?
- UI complexity increases?
- Future games are added to the engine?

#### 8. Maintainability
- Will engineers understand this one year from now?
- Is the implementation hiding too much complexity?
- Are edge cases well-documented?
- Can failures be diagnosed post-deployment?

#### 9. Debuggability
- Can failures actually be understood?
- Can developers inspect intermediate outputs (segmentation, matches, confidence)?
- Can evidence be collected for failure analysis?
- Is there sufficient logging?
- Can confidence be traced to its sources?

#### 10. Confidence & Evidence
- How does this detector know it is correct?
- What independent signals support confidence?
- Can confidence be calibrated?
- What makes confidence *low*?
- Can low confidence be recognized at runtime?

#### 11. Validation Strategy
- How will this detector be proven correct?
- Not estimated. Not simulated. Proven.
- What ground truth will be used?
- What accuracy threshold proves correctness?
- What edge cases must pass?
- Will replay dataset validation be sufficient?

#### 12. Recommendation
**APPROVE** / **APPROVE WITH CHANGES** / **REJECT**

If REJECT or APPROVE WITH CHANGES, describe required remediation.

---

### Architecture-Wide Review

After reviewing all 5 teams independently, review the ENTIRE perception architecture:

#### 1. System Coherence
- Does this architecture make sense as a unified system?
- Are responsibilities properly distributed?
- Are there hidden dependencies?
- Is the dependency graph correct?

#### 2. Quality Standards
Would production teams approve this?
- Would NVIDIA build this?
- Would Waymo build this?
- Would OpenAI build this?
- Would CVPR reviewers accept this as a research contribution?

#### 3. Five Biggest Architectural Mistakes
Identify the most fundamental issues across the system.

#### 4. Five Strongest Design Decisions
Identify the most sound choices.

#### 5. Cross-Team Issues
- Duplicated logic between teams?
- Missing abstractions?
- Incorrect ownership boundaries?
- Performance bottlenecks?
- Confidence propagation issues?

#### 6. Infrastructure Gaps
- Is validation infrastructure sufficient?
- Are benchmarking tools adequate?
- Is replay capability robust?
- Can developers understand failures?
- Is visualization tooling present?

#### 7. Integration Risks
- Can all detectors actually work together?
- Will latency budgets be met?
- Are confidence models compatible?
- Can state be serialized correctly?
- Is error recovery comprehensive?

#### 8. Roadmap Assessment
- Is implementation order optimal?
- Should dependencies change?
- Are success criteria measurable?
- Are timelines realistic?

#### 9. Risk Matrix
| Risk | Probability | Impact | Detection | Mitigation |
|------|-----------|--------|-----------|-----------|
| OCR dominance in fallback | High | High | Design review | Eliminate OCR as primary |
| Platform false positives on UI | High | High | Validation | Explicit UI masking |
| Temporal fusion drift | Medium | High | Benchmarking | Calibration strategy |
| Font changes break text recognition | Medium | High | Design review | Bitmap approach |
| Entity tracking occlusion failure | Medium | Medium | Validation | Prediction strategy |

#### 10. Deliverables

**Executive Summary** (1 page)
- High-level assessment
- Go/no-go recommendation
- Critical blockers

**Architecture Score** (0-100)
- Overall system quality rating

**Per-Team Scores** (0-100 each)
- HUD, World, Entities, Temporal, Validation

**Critical Findings**
- Issues that MUST be addressed before implementation
- Recommended remediation

**Major Recommendations**
- Design improvements
- Architecture changes
- Roadmap reordering

**Minor Recommendations**
- Implementation guidance
- Best practices
- Debugging strategies

**Required Changes Before Implementation**
- Explicit list of gates
- Success criteria for each gate
- Owner of each remediation

**Architectural Debt Summary**
- Issues deferred to Phase 2+
- Rationale for deferral
- Risks of deferral

---

## Review Standards

### Standards for Approval

#### Per-Team Level
- ✅ All 12 review sections addressed
- ✅ Technology comparison exhaustive
- ✅ Failure modes identified and handled
- ✅ Confidence model complete
- ✅ Validation strategy proves correctness (not estimates)
- ✅ Scalability plan exists
- ✅ Debuggability adequate

#### Architecture Level
- ✅ System coherence: responsibilities aligned
- ✅ Quality: would pass code review at NVIDIA/Waymo
- ✅ Risk matrix: all high-probability, high-impact risks mitigated
- ✅ Integration: all detectors can coexist
- ✅ Validation: systematic proof strategy
- ✅ Performance: latency budgets achievable
- ✅ Maintainability: one-year forward understanding

### Standards for Rejection

REJECT if:
- Fundamental architectural flaw identified
- Critical technology choice unjustified
- Validation strategy inadequate
- Failure modes not addressed
- Scalability severely limited
- Maintainability compromised

---

## MARB Process

### Phase 0: Team Design (Days 1-4)
- All 5 teams conduct design analysis
- Teams produce design documents
- Teams submit for review

### Phase 1: MARB Review (Days 4-8)
- MARB reviews each team independently (1 agent per team review, parallel)
- MARB produces 5 individual review documents
- MARB identifies architecture-wide issues

### Phase 2: Architecture Assessment (Days 8-13)
- MARB integrates individual reviews
- MARB conducts full architecture review
- MARB produces Architecture Review Board Report
- MARB issues go/no-go recommendation

### Phase 3: Remediation (Days 13+)
- If APPROVE: Implementation begins
- If APPROVE WITH CHANGES: Teams address and resubmit
- If REJECT: Fundamental redesign required

### Phase 4: Approval Gate (Days 15+)
- MARB conducts final gate review
- If all remediation complete: Implementation approved
- If gaps remain: Back to Phase 3

---

## Independence & Authority

**MARB is independent from:**
- HUD Team ownership
- World Geometry Team ownership
- Entity Tracking Team ownership
- Temporal Reasoning Team ownership
- Validation Team ownership

**MARB has authority to:**
- Reject any team's design
- Require redesign before implementation
- Block Phase 1 if critical issues unresolved
- Recommend major architectural changes
- Reorder implementation priority

**MARB does NOT have authority to:**
- Implement code
- Override user requirements
- Change project goals
- Own detector infrastructure

---

## Review Quality Criteria

A MARB review is high-quality if it:

1. **Identifies real flaws** (not nitpicks)
   - Addresses scalability, maintainability, correctness
   - Not style or minor improvements

2. **Challenges assumptions** (not protects them)
   - Questions whether OCR is necessary
   - Questions whether edge detection works
   - Questions whether temporal fusion is needed

3. **Proposes alternatives** (not just complains)
   - Suggests different technology choices
   - Compares pros/cons systematically
   - Justifies recommendation with evidence

4. **Proves rigor** (not just opinions)
   - Cites peer-reviewed approaches
   - References production systems
   - Compares against known standards

5. **Blocks when necessary** (not rubberstamps)
   - Willing to reject entire designs
   - Will not approve insufficient validation
   - Will not approve overengineering
   - Will not approve underengineering

---

## Success Metrics

MARB review succeeds if:

- ✅ Every design flaw is identified before implementation
- ✅ No implementation requires major redesign after Phase 1 begins
- ✅ Technology choices are justified against alternatives
- ✅ Validation strategy proves correctness, not estimates
- ✅ Confidence in architecture increases after review
- ✅ Implementation teams understand and support recommendations
- ✅ Final approval recommendation is based on evidence, not opinion

---

## Document Timeline

1. **Design Phase** (Days 1-4): Teams complete design documents
2. **MARB Review** (Days 4-13): MARB produces review documents
3. **Remediation** (Days 13+): Teams address MARB findings
4. **Approval Gate** (Day 15): Final go/no-go decision
5. **Implementation** (Day 16+): Only proceeds after approval

---

**MARB Charter Status**: Approved - review process begins after team designs complete

**Next Milestone**: All 5 team design documents submitted

**Review Timeline**: 9 days from design completion
