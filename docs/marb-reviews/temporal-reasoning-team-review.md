# MARB Review: Temporal Reasoning Team

## 1. Summary

The temporal reasoning team proposes to fuse detector outputs over time using smoothing, persistence, and tracking. This is necessary and correct in principle, but the design is too abstract and does not yet define the state model, update equations, or failure behavior with enough precision to be trusted.

## 2. Strengths

- The team correctly recognizes that frame-by-frame reasoning is insufficient.
- The proposal is aligned with the requirement to avoid flicker and transient errors.
- The use of stateful memory is appropriate for a game perception system.

## 3. Weaknesses

- The proposal does not define what state variables exist, how they are updated, or what invariants they must satisfy.
- The design does not explain how to distinguish real state changes from detection noise.
- The team has not specified what happens when a detector output becomes temporarily invalid for several frames.
- The plan does not describe how confidence should decay or recover.
- The proposal risks introducing hidden coupling between detector outputs and the tracker.

## 4. Missing Research

The team has not yet sufficiently explored:

- Kalman filters versus simpler bounded-state models.
- Bayesian belief updates.
- Rule-based invariant checking for values such as HP, level, and job.
- Explicit handling of occlusion and reappearance.
- Quantitative analysis of how fusion improves stability and data quality.

## 5. Technology Comparison

The current proposal is too hand-wavy. The team should compare:

- EMA for simple scalars.
- Kalman filtering for smooth continuous variables.
- Rule-based state machines for discrete UI and entity state.
- Hybrid fusion systems for values, entities, and UI windows.

The board does not yet see a convincing case that the selected approach is best.

## 6. Failure Analysis

The major risks are:

- The tracker may incorrectly suppress a real change, such as a sudden health drop or a level-up event.
- It may persist stale state for too long during occlusion.
- It may over-trust incorrect detector outputs and create a false state estimate.
- It may become a hidden source of error that is hard to debug because the state is updated implicitly.

## 7. Scalability

The design needs to scale to:

- multiple output types
- multiple entities
- multiple UI windows
- real-time constraints

The current proposal does not show how the tracker remains efficient as the number of state variables grows.

## 8. Maintainability

The state model must be explicit and inspectable. If the tracker is not explainable, it will be very hard to maintain. The team should define:

- update rules
- state invariants
- confidence semantics
- transition logic

## 9. Debuggability

The tracker needs to expose:

- previous state values
- current evidence
- confidence history
- update reasons
- rejected updates

Without this, the system will be opaque and difficult to root-cause.

## 10. Confidence

Confidence propagation is central to this subsystem. The team must state:

- what evidence increases confidence
- what evidence decreases confidence
- how confidence decays on missing observations
- whether confidence can be calibrated

At present, this is still an open design gap.

## 11. Validation

The team needs to demonstrate that fusion improves performance. This requires:

- replay sequences with known state transitions
- cases of noise, occlusion, and recovery
- objective metrics for stability and accuracy

Without such evidence, the subsystem remains hypothetical.

## 12. Recommendation

Recommendation: APPROVE WITH CHANGES.

The temporal subsystem is necessary, but it must be specified as an explicit state-estimation pipeline with clear invariants and validation before implementation.
