# Red Team Review of ARCHITECTURE_V2.md

## 1. Executive Summary

The architecture is still not ready for implementation. It is more disciplined than the prior design, but it remains a high-level aspiration document with several unresolved architectural hazards. The biggest failures are not in the presence of ideas; they are in the absence of explicit ownership, state semantics, runtime contracts, and a plan for handling disagreement and drift under real deployment conditions.

The architecture does not yet specify enough to survive the first serious stress test. It is still vulnerable to silent failure, hidden coupling, confidence poisoning, temporal overfitting, and nondeterministic replay. It is a better document than the previous version, but it still does not meet the standard of a production-grade perception architecture for dozens of games and millions of gameplay hours.

## 2. Critical Flaws

### 2.1 The architecture still lacks a real state model

The document repeatedly speaks of a “belief state,” but it never defines a rigorous state-space representation. This is a design hole, not a detail.

Why this matters:
- A belief state is not just a bag of current detector outputs. It must define which variables exist, their semantics, allowed transitions, update rules, and invalid states.
- Without this, any temporal fusion module can silently invent states that are not supported by the evidence.

Failure mode:
- A detector reports a temporary false value with high confidence. The temporal layer integrates it. The system then begins to believe that the state has changed. The wrong state then influences downstream logic and debugging.

Cascade risk:
- High.

### 2.2 Confidence is still underspecified and likely to become self-justifying

The confidence architecture is discussed in principle, but it still does not define the actual probability model, calibration procedure, or update equations. This is dangerous because confidence becomes a hidden source of false authority.

Why this matters:
- The architecture suggests fusion of confidence values, but does not define whether confidence is a probability, a heuristic weight, or a reliability score. These are different concepts.
- A detector with poor calibration can poison the entire fusion process.

Failure mode:
- An inaccurate detector with overconfident output dominates the belief state. The system becomes more certain about a wrong answer.

Cascade risk:
- Very high.

### 2.3 The architecture assumes detector independence without proving it

The design proposes to fuse many independent observations, but it does not define how to detect dependence between detectors or how to prevent double-counting the same evidence source.

Why this matters:
- If the same underlying visual artifact causes two detectors to agree, the system may think it has corroboration when it actually has duplicate evidence.
- This is a known issue in sensor fusion and perception systems.

Failure mode:
- Two detectors both fail for the same reason. The fusion layer interprets the agreement as strong evidence.

Cascade risk:
- High.

### 2.4 The replay architecture is described, but not specified as an execution contract

The document says replay should be deterministic and frame-perfect, but it does not define the required invariants, input/output contracts, or the mechanism by which nondeterminism is prevented.

Why this matters:
- A replay system that uses wall-clock timing, nondeterministic threading, or asynchronous buffers can produce different results from the same recorded frame sequence.
- That destroys the value of replay as a scientific device.

Failure mode:
- The system behaves differently when replayed than when recorded. The benchmark cannot be trusted.

Cascade risk:
- High.

### 2.5 The architecture still lacks a proper arbitration model for disagreement

There is no clear rule for what happens when detectors disagree. This is a fundamental omission.

Why this matters:
- A perception system that cannot resolve disagreement cannot be trusted.
- The architecture mentions contradiction handling, but it does not define the arbitration policy.

Failure mode:
- Two modules report contradictory values. The system produces an average, a guess, or an unresolved state. Downstream systems then act on unstable or misleading outputs.

Cascade risk:
- Very high.

### 2.6 The architecture gives too much responsibility to generic “detectors” without defining their runtime budgets

The proposal uses many detectors conceptually, but it does not define per-detector latency budgets, memory budgets, or fallback behavior for late execution.

Why this matters:
- At 20 FPS or 240 FPS, the architecture needs budget discipline. A detector that is too slow or too memory-hungry can collapse the entire system.

Failure mode:
- A newly added detector causes dropped frames or queue buildup. The system becomes stale and the temporal layer over-smooths or over-predicts.

Cascade risk:
- High.

### 2.7 The architecture does not define a robust model for partial observability

The system assumes it can produce a belief state for many variables, but it does not define what happens when a variable is temporarily unobservable.

Why this matters:
- HUD elements can disappear, be occluded, or be outside the frame.
- Entities can vanish behind effects or leave the image.
- The system needs explicit “unknown” semantics, not just “last known good.”

Failure mode:
- Tracked state persists incorrectly through long occlusion or scene changes.

Cascade risk:
- High.

### 2.8 The architecture is still too optimistic about UI and world separation

The document says UI and world should be separated, but it does not define their interaction boundaries in a way that survives real gameplay and UI changes.

Why this matters:
- Real UI overlays can partially obscure the world. Some elements are semi-transparent. Some world objects appear under or over particular panels.
- A panel can temporarily change shape or overlap with the player.

Failure mode:
- The system masks a region incorrectly, causing world geometry or entity tracking to miss visible content.

Cascade risk:
- Medium-high.

### 2.9 The architecture still lacks a proper multi-game abstraction layer

The document says the system should generalize, but it offers no explicit abstraction for game-specific priors, assets, and layout models.

Why this matters:
- A perception architecture that has only a single MapleStory-specific pipeline will not generalize gracefully.
- The architecture needs a formal domain adapter layer.

Failure mode:
- The system becomes a single-game spaghetti pipeline with hidden game assumptions.

Cascade risk:
- High.

### 2.10 The architecture under-specifies failure recovery and self-healing

The document mentions confidence decay and contradiction handling, but it does not define a recovery policy or recovery state machine.

Why this matters:
- Systems fail. The architecture needs clear policies for when to forget, when to reinitialize, when to re-locate, and when to ask for human input or fallback.

Failure mode:
- The system drifts into a wrong but stable state and no mechanism exists to re-anchor it.

Cascade risk:
- High.

## 3. High-Risk Assumptions

### 3.1 Assumption: the “deterministic first” principle is sufficient

This is fragile because the architecture still assumes that deterministic priors cover enough of the problem to make the others manageable. In practice, graphics effects, dynamic UI, and animation make the rendering less stable than the document assumes.

### 3.2 Assumption: hand-crafted priors will remain valid across versions

This is not robust. Game updates can alter UI, fonts, colors, and effects without changing the overall architecture.

### 3.3 Assumption: confidence can be calibrated later

This is an architectural mistake. If the confidence framework is not specified from the beginning, it will be impossible to calibrate properly later.

### 3.4 Assumption: replay can be deterministic without a strict execution contract

This is likely false. Replay correctness requires explicit ordering, bounded concurrency, hardware-independent assumptions, and deterministic logging.

### 3.5 Assumption: a central belief state can safely absorb all detector outputs

This is overly ambitious. The belief state will become a hidden bottleneck and an implicit source of coupling unless it is heavily constrained.

## 4. Medium-Risk Assumptions

### 4.1 The architecture assumes that OCR can be removed from the core path without a replacement strategy for text-heavy states

That is a risk because some states are inherently text-driven. The architecture needs a formal text-uncertainty strategy if it is replacing OCR.

### 4.2 The architecture assumes that entities can be tracked by appearance and motion alone

This will fail under severe occlusion, texture similarity, and identical entities.

### 4.3 The architecture assumes UI masks can be produced robustly

This is likely false for dynamic or partially transparent overlays.

### 4.4 The architecture assumes per-detector budgets can be managed later

This is too late. The architecture needs explicit runtime budgets from the beginning.

## 5. Low-Risk Improvements

These are not sufficient to fix the architecture, but they would be useful:

- add explicit module ownership boundaries
- add state transition diagrams for major variables
- add detector contract examples
- add replay tracing for intermediate outputs
- add a deterministic ordering requirement for all detector invocations
- add explicit interface definitions for disagreement handling

## 6. Missing Subsystems

The architecture still lacks several subsystems that are necessary for real deployment:

- a formal state machine for game-state variables
- a contradiction resolution subsystem
- a domain-adapter layer for game-specific priors
- a calibration and drift-monitoring subsystem
- an out-of-distribution detector for unseen UI or map states
- a recover-and-reinitialize subsystem
- a resource budget manager
- a detector dependency graph with actual scheduling constraints
- a cross-detector provenance and evidence ledger
- a multi-hypothesis tracker for ambiguous entities
- a scene-change detector that can reset stale beliefs
- an uncertainty-aware fallback policy layer

## 7. Missing Interfaces

The following interfaces are not defined and should be considered mandatory:

- DetectorInputContext
- DetectorObservation
- BeliefUpdate
- ContradictionReport
- ArbitrationDecision
- CalibrationModel
- ReplaySnapshot
- RuntimeBudget
- DetectorExecutionPlan
- StateTransitionEvent
- RecoveryAction
- DebugTrace
- BenchmarkResult

## 8. Missing Data

The architecture does not define the data required to operate safely:

- per-detector calibration datasets
- label schema for ambiguous states
- negative examples for false-positive rejection
- per-frame runtime telemetry
- memory profile data
- latency budgets by detector
- expected failure distribution statistics
- cross-detector disagreement statistics
- human-reviewed cases of ambiguous UI states

## 9. Missing Testing

The testing strategy is still far too weak for production readiness.

Missing tests include:

- adversarial UI-change tests
- occlusion stress tests
- disagreement tests
- calibration tests
- replay determinism tests
- latency budget tests
- memory pressure tests
- race-condition tests
- state-transition tests
- long-duration drift tests
- cross-game transfer tests
- localization and scaling stress tests
- false-positive rejection tests
- restart and recovery tests

## 10. Suggested Redesigns

### 10.1 Replace the generic belief-state concept with an explicit state graph

The architecture needs a state graph or state machine for each major game-state variable. Do not let the belief state be an unstructured bag of values.

### 10.2 Introduce a first-class arbitration layer

A dedicated arbitration subsystem should decide how to handle disagreement between modules. This should be a formal, auditable layer, not an informal fusion heuristic.

### 10.3 Replace the current confidence concept with a calibrated evidence model

Confidence must be tied to calibration data and explicit evidence types. It should not be a scalar produced ad hoc by each detector.

### 10.4 Introduce explicit runtime budgets and scheduling

Every detector needs a declared cost model and a fallback policy if it misses its budget.

### 10.5 Make replay execution a hard contract

Replay must be deterministic and tested as a first-class requirement. The architecture should specify ordering, timing, and serialization invariants.

### 10.6 Add a world-state consistency checker

The system needs a layer that checks whether the current game-state estimate is internally consistent. It should catch impossible states before they influence downstream systems.

## 11. Architecture Score (0–100)

Architecture score: 41/100

## 12. Implementation Readiness Score

Implementation readiness: 24/100

## 13. Decision

Decision: REJECT
