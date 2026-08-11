# MARB Review: World Geometry Team

## 1. Summary

The world geometry team proposes to recover traversable platforms and world-space structure by separating UI from world-space regions, then applying geometry-based extraction and temporal validation. This is a better direction than the earlier edge-only approach, but the proposal still lacks a concrete method for rejecting false positives and remains too optimistic about platform semantics.

## 2. Strengths

- The team explicitly recognizes that UI borders and decorative geometry should not be treated as platforms.
- The plan separates world-space perception from HUD/UI perception, which is essential.
- The emphasis on traversable surfaces rather than arbitrary edges is correct.
- The proposal is more grounded in actual geometry than the earlier simplistic detector designs.

## 3. Weaknesses

- The proposal still does not define a formal criterion for what counts as a traversable platform.
- The architecture risks confusing decorative horizontal lines, shadows, and environmental features with true surfaces.
- The design has not yet proven that world-space segmentation can be robust across maps, tile sets, and lighting conditions.
- The plan does not specify how to recover negative evidence: how do we know a candidate is not a platform?
- The review board is not convinced that a geometry-only approach is sufficient for the full platform problem.

## 4. Missing Research

The following approaches were not sufficiently explored:

- Explicit use of map- and tile-specific priors.
- Multi-scale platform validation using both local geometry and global context.
- Comparison between contour-based grouping, Hough-based line extraction, and semantic segmentation.
- Robust methods for rejecting UI and background artifacts.
- A formal representation of platform connectivity and walkability rather than simple object detection.

## 5. Technology Comparison

A geometry-first approach is reasonable, but it should not be assumed to be best by default. The team should compare:

- Geometry-based contour grouping.
- Template-based platform detection for known map tiles.
- Segmentation-based methods for world-space surfaces.
- Hybrid methods that combine geometry with temporal persistence.

The current proposal does not yet show why the chosen approach should beat the others in precision and recall.

## 6. Failure Analysis

The highest-risk failures are:

- False positives from UI borders, decorative floor lines, or background geometry.
- False negatives on thin, slanted, or partially occluded platforms.
- Confusion between walkable surfaces and non-walkable features with similar geometry.
- Unstable performance across different maps and visual conditions.

This is not a minor issue. Platform extraction will poison downstream pathfinding and entity reasoning if it is wrong.

## 7. Scalability

The proposed approach is unlikely to generalize without a stronger map-aware layer. The team must handle:

- Multiple maps with different art styles.
- Different tile sets and seasonal visual effects.
- Different zoom levels and resolutions.
- Future content updates that change environment rendering.

Without explicit generalization logic, the system is likely to become a brittle map-specific detector.

## 8. Maintainability

The maintainability risk is high because the proposed system does not yet define a clear separation between:

- world-space geometry extraction
- platform graph construction
- UI masking and rejection
- map-specific priors

These concerns should be separated. Otherwise maintainers will inherit a tangled, hard-to-debug pipeline.

## 9. Debuggability

The team must provide:

- Candidate platform masks
- Rejected candidates with reasons
- UI masking outputs
- Geometry confidence per platform hypothesis

Without these intermediate artifacts, failures will be impossible to diagnose.

## 10. Confidence

Confidence is not yet trustworthy. The proposed team needs a principled way to model:

- geometric support
- temporal persistence
- walkability consistency
- rejection evidence

A platform should not be reported as present because it looks plausible. It should be reported because it passes multiple independent checks.

## 11. Validation

The current validation story is insufficient. The team does not yet have a replay dataset or a sufficient number of annotated platform examples. The system can only be trusted once it has been evaluated on:

- multiple maps
- multiple resolutions
- multiple lighting conditions
- negative examples that are not platforms

## 12. Recommendation

Recommendation: APPROVE WITH CHANGES.

The overall direction is sound, but the team must add a formal definition of traversability, a stronger false-positive rejection strategy, and an evidence-based validation plan before implementation.
