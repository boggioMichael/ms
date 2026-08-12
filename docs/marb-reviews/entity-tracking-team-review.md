# MARB Review: Entity Tracking Team

## 1. Summary

The entity tracking team proposes to localize the player and other entities using sprite atlas matching, contour-based refinement, and temporal tracking. This is directionally better than a generic object detector, but the current proposal is still under-specified and likely to fail under occlusion, costume variation, and ambiguous humanoid appearances.

## 2. Strengths

- The team correctly recognizes that MapleStory entities are not arbitrary objects; they are constrained by a game client and its sprite conventions.
- The plan uses temporal tracking, which is a necessary component for stable identity.
- The proposal is more realistic than relying on a generic CNN detector for all entity classes.

## 3. Weaknesses

- The proposal does not yet define how the team will distinguish the player from NPCs and monsters that look similar.
- The design assumes that sprite atlas matching will work reliably across equipment, animation, and scale changes. That is not yet demonstrated.
- The team does not yet define how to recover an entity when it is partially or fully occluded for several frames.
- The proposal does not explain how the team will infer animation state in a calibrated, robust way.
- The system is likely to suffer from identity swaps under crowded scenes.

## 4. Missing Research

The following gaps matter:

- A comparison of atlas matching, feature matching, silhouette matching, and learned classifiers for the same entity classes.
- A study of how much entity identity can be inferred from pose and motion instead of appearance alone.
- A plan for tracking under occlusion and temporary disappearance.
- A method to reject false positives from decorative sprites or particle effects.

## 5. Technology Comparison

The proposal should not assume atlas matching is the best method for all entities. The team should compare:

- sprite-template matching
- contour and silhouette matching
- motion-based tracking
- learned appearance classifiers
- hybrid appearance-plus-geometry pipelines

At present, the proposal does not make a convincing case that the chosen method dominates the alternatives.

## 6. Failure Analysis

Likely failure modes include:

- Entity identity swaps in crowded scenes.
- Player detection failures when the sprite is partially obscured or in a non-standard pose.
- Animation classification errors during combat or special effects.
- Tracking drift when the entity leaves the frame and returns.

These problems are not edge cases; they are core expectations for a real-time game perception system.

## 7. Scalability

The team’s approach will not scale well unless entity classes are represented with explicit priors. The system must handle:

- different player classes
- different equipment skins
- multiple entity types
- cluttered scenes
- multiple entities in close proximity

Without explicit class and appearance priors, the pipeline will become brittle.

## 8. Maintainability

The design is likely to become difficult to maintain unless it explicitly separates:

- entity localization
- entity identity classification
- temporal tracking
- animation inference

At present, these responsibilities are still loosely defined.

## 9. Debuggability

The system needs to expose:

- candidate entity boxes
- matching scores per template
- track identity history
- occlusion and uncertainty state
- animation classification evidence

Without these, entity tracking failures will be opaque.

## 10. Confidence

The proposal must define confidence as a combination of multiple sources, such as:

- appearance similarity
- geometric consistency
- temporal persistence
- motion plausibility
- class likelihood

The current design lacks a concrete calibration strategy.

## 11. Validation

The validation approach is not yet sufficient. A small number of hand-annotated frames will not be enough. The team needs:

- replay-based entity annotations
- multiple view conditions
- occlusion cases
- crowded scenes
- identity-consistency checks across frames

## 12. Recommendation

Recommendation: REJECT.

The team should not proceed to implementation until it defines a stronger entity identity model, a clear occlusion strategy, and a validation framework that can actually prove the system works under realistic conditions.
