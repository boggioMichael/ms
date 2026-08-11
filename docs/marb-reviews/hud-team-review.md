# MARB Review: HUD Perception Team

## 1. Summary

The HUD team proposes to recover HP/MP/EXP bars, player identity text, and dialog state using a combination of geometric bar measurement, bitmap-font recognition, layout priors, and limited OCR fallback. This is the right high-level direction, but the current proposal still lacks a concrete evidence base and the assumptions around font extraction, UI scaling, and text robustness are under-specified.

## 2. Strengths

- The team correctly rejects generic OCR as the primary strategy for bars and UI text.
- The proposal is aligned with the deterministic rendering assumption of MapleStory.
- The bar measurement concept is grounded in a physically meaningful signal: fill ratio.
- The architecture separates HUD reasoning from world-space perception, which is a good design principle.

## 3. Weaknesses

- The proposal still assumes that font extraction and template generation will be straightforward. That is not yet demonstrated.
- The document does not prove that the chosen font recognition approach will work across different resolutions, client skins, or non-Latin text.
- The plan does not describe how to distinguish the correct bar from adjacent UI decorations or partially occluded elements.
- The confidence model is described, but not calibrated. A confidence score without calibration is not an engineering artifact; it is guesswork.
- The proposal still leaves the text problem under-specified: name recognition, job classification, and level extraction are not yet reduced to a robust, testable pipeline.

## 4. Missing Research

The review board considers the following alternatives insufficiently explored:

- A purely geometric layout-based HUD parser that ignores text recognition entirely for some fields.
- A dictionary-driven parser that uses the game client’s known UI fields and known value domains before any OCR-like recognition.
- A more explicit comparison of template matching versus bitmap-font decoding versus constrained OCR across real UI samples.
- A failure analysis for small fonts, anti-aliased edges, and mixed-language UI text.
- A study of how much of the HUD can be recovered from raw screen-space geometry without any text recognition at all.

## 5. Technology Comparison

The current proposal would benefit from a more explicit competition between approaches:

- Bar measurement: geometric fill ratio is best; OCR should never be primary.
- Text recognition: bitmap-font matching is plausible; constrained OCR is only a fallback.
- Dialog detection: template matching and layout priors are preferable to whole-image OCR.

The main weakness is not that the chosen methods are bad; it is that the proposal does not yet prove they are better than the alternatives under actual deployment conditions.

## 6. Failure Analysis

The HUD pipeline will fail first in the following scenarios:

- Font mismatch or UI skin changes.
- Small or compressed text regions.
- Partial occlusion by effects, dialogue overlays, or combat animations.
- Resolution scaling that breaks the assumed ROI geometry.
- False positives from decorative borders or bars that are visually similar.

These failures are not edge cases; they are the norm in real gameplay. The design must show how the system detects uncertainty and degrades gracefully rather than silently producing wrong values.

## 7. Scalability

The current HUD design is unlikely to scale well unless it is tied to a robust font/UI adaptation layer. The system must support:

- 1080p, 1440p, and 4K screens.
- Multiple client themes and localization settings.
- Future game updates that shift UI spacing or bar colors.
- Different window scaling and DPI settings.

Without explicit adaptation logic, the system will be brittle.

## 8. Maintainability

The HUD subsystem is maintainable only if it is organized around a small set of clearly defined abstractions:

- ROI priors
- Recognizers
- Confidence models
- Validation hooks

As written, the design still mixes recognition logic, layout priors, and UI assumptions too tightly.

## 9. Debuggability

The proposal needs explicit debugging hooks for:

- Intermediate bar masks
- ROI localization outputs
- Glyph matching scores
- Template matching scores
- Confidence provenance per field

Without these, failures cannot be diagnosed quickly.

## 10. Confidence

The current design says the detector should output confidence, but it does not define how confidence is estimated or calibrated. Confidence should be derived from multiple independent evidences:

- ROI alignment
- Fill continuity
- Temporal persistence
- Character-level glyph agreement
- Known-value constraints

Without such a model, confidence is not trustworthy.

## 11. Validation

The HUD team does not yet have a sufficient validation story. The current repository has a single frame and no replay dataset. That is not enough. The team must prove correctness with:

- Ground-truth annotations for multiple frames
- Replays containing UI changes and occlusions
- Measurement of accuracy, precision, and calibration error

## 12. Recommendation

Recommendation: APPROVE WITH CHANGES.

The design direction is good, but the team must produce a stronger evidence strategy before implementation. The most important changes are:

- Replace vague confidence claims with an explicit confidence model.
- Produce a small but real validation dataset before coding.
- Demonstrate font/UI adaptation strategy.
- Add explicit failure handling for occlusion and UI changes.
