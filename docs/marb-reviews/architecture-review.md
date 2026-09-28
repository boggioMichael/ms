# MARB Architecture-Wide Review

## Executive Summary

The architecture is directionally promising but not yet ready for implementation. The strongest element is the explicit decision to move away from generic OCR and toward specialized detectors. The weakest elements are the lack of a validated evidence base, the absence of replay infrastructure, the under-specified confidence model, and the failure to separate UI, world, and temporal reasoning with enough rigor.

## Architecture Score

Score: 42/100

## Per-Team Scores

- HUD Team: 58/100
- World Geometry Team: 54/100
- Entity Tracking Team: 49/100
- Temporal Reasoning Team: 57/100
- Validation Team: 46/100

## Risk Matrix

| Risk | Probability | Impact | MARB Assessment |
|---|---:|---:|---|
| Over-reliance on OCR-like fallback | High | High | Must be removed from primary strategy |
| False platform detection from UI or background | High | High | Must be fully addressed before implementation |
| Entity identity swaps under occlusion | High | High | Must be redesigned with stronger state priors |
| Confidence is not calibrated | High | High | Current proposal is not trustworthy |
| Validation dataset is absent | High | High | Current validation strategy is insufficient |

## Critical Findings

1. The architecture is not yet evidence-based. The project has not yet demonstrated the core detector hypotheses on real replay data.
2. The confidence model is under-defined and cannot be trusted.
3. The system still lacks a robust plan for distinguishing UI from world-space geometry.
4. The architecture has not yet defined a concrete replay and ground-truth pipeline.
5. The design assumes too much about font extraction, sprite matching, and UI consistency without proving the assumptions.

## Major Recommendations

1. Reorder the roadmap: establish replay infrastructure and annotated datasets before detector implementation.
2. Make deterministic geometry and layout priors the first-class design substrate.
3. Define explicit confidence propagation and calibration from the beginning.
4. Rework the architecture to separate UI perception, world geometry, entity tracking, and temporal fusion more explicitly.
5. Replace vague detector claims with validated baselines and measured failure cases.

## Minor Recommendations

- Add explicit intermediate outputs for every detector.
- Impose a standard for detector contracts: value, confidence, evidence, failure_reason.
- Include negative examples in all detector validation.
- Add a debugging dashboard that visualizes detector outputs and confidence over time.

## Required Changes before Implementation

- Build a replay capture and annotation pipeline.
- Produce at least one annotated replay segment with ground truth for HUD values, entity positions, and platforms.
- Rework the architecture to remove OCR as a primary strategy.
- Define and calibrate confidence for each detector.
- Add explicit fail-safe mechanisms for occlusion, missing observations, and ambiguous UI states.

## Roadmap Reordering

The current roadmap should be reordered as follows:

1. Validation infrastructure and dataset creation.
2. Deterministic HUD bar and layout baseline.
3. UI panel and text-region localization baseline.
4. Platform geometry and world-space separation baseline.
5. Entity tracking and temporal fusion.

## Architectural Debt

The system currently carries significant architectural debt in the following areas:

- validation debt
- research debt
- technical debt around detector contracts and confidence propagation
- integration debt caused by weak separation between subsystems

## Technical Debt

- Detector outputs are under-specified.
- Confidence handling is not yet formalized.
- The system lacks a clear abstraction boundary for game-specific vs game-agnostic reasoning.

## Research Debt

- The architecture has not yet demonstrated which recognition strategy is best for each subsystem on real data.
- The team has not yet compared multiple approaches rigorously on the same dataset.

## Validation Debt

- No replay dataset.
- No ground-truth annotation standard.
- No explicit metrics for calibration or temporal stability.

## Final Recommendation

Recommendation: REJECT.

The project should not proceed to implementation in the current form. It should first complete the validation and evidence foundation, then re-present the subsystem designs with measured evidence and a stronger confidence model.
