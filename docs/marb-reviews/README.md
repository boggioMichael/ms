# MARB Review Package

This directory contains the independent architecture reviews produced by the Maple Architecture Review Board (MARB).

## Review Documents

- [HUD Team Review](./hud-team-review.md)
- [World Geometry Team Review](./world-geometry-team-review.md)
- [Entity Tracking Team Review](./entity-tracking-team-review.md)
- [Temporal Reasoning Team Review](./temporal-reasoning-team-review.md)
- [Validation Team Review](./validation-team-review.md)
- [Architecture-Wide Review](./architecture-review.md)

## Executive Summary

The current perception architecture is not yet ready for implementation. The redesign is directionally correct, but the proposed system still lacks the evidence base, validation infrastructure, and failure-mode discipline required for a production-grade game-state perception engine.

## Overall Assessment

- Architecture Score: 42/100
- Recommendation: REJECT
- Required before implementation: redesign the evidence strategy, validation dataset, and confidence model; remove OCR as a primary strategy; add explicit UI/world separation and replay infrastructure.
