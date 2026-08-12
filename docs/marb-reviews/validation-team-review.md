# MARB Review: Validation & Ground Truth Team

## 1. Summary

The validation team is responsible for creating the evidence base for the whole perception stack. This is essential. However, the current plan is underdeveloped and does not yet define a validation system that can actually prove correctness rather than merely produce the appearance of rigor.

## 2. Strengths

- The team correctly identifies that a replay dataset is necessary.
- The plan recognizes the need for ground truth and automated reporting.
- The notion of frame-level evaluation is sound.

## 3. Weaknesses

- The current validation plan is still too abstract. It does not specify the actual data pipeline, annotation workflow, or tooling.
- The repository currently contains a single validation frame. That is not enough. The team has not yet proven it can produce a replay dataset that covers the required game states.
- The plan does not define an annotation standard for ambiguous cases such as entity identity, animation state, or platform walkability.
- There is no evidence yet of robust automation for metrics, comparisons, or regression testing.

## 4. Missing Research

The board expects the team to address:

- how ground truth will be captured and reviewed
- whether the dataset will be deterministic and reproducible
- how ambiguous states will be labeled
- how the validation system will integrate into CI or local workflow
- what error bars and confidence intervals will be reported

## 5. Technology Comparison

The team should compare:

- manual frame annotation
- semi-automatic annotation assisted by templates and priors
- replay-based logging with deterministic simulation
- automated disagreement review between annotators

The current design does not yet show which approach is both scalable and trustworthy.

## 6. Failure Analysis

The validation infrastructure will fail if:

- the dataset is not representative of real gameplay
- annotations are inconsistent or noisy
- replay data is not deterministic
- metrics are not tied to clear success criteria

Without a trustworthy validator, the project will not know whether it is improving or merely changing its failure modes.

## 7. Scalability

The validation system needs to scale to:

- hundreds of frames and replays
- multiple resolutions and UI scales
- multiple maps
- multiple entity classes
- future game updates

The current plan does not yet show a scalable annotation and evaluation workflow.

## 8. Maintainability

Validation infrastructure must be easy to extend. A brittle one-off script will not survive. The team must define:

- data schema
- annotation format
- reporting format
- regression system

## 9. Debuggability

The infrastructure should produce:

- frame-by-frame reports
- detector error breakdowns
- confusion matrices
- failure examples with context

Without these, the team cannot learn from mistakes.

## 10. Confidence

The validation team should define what level of agreement between annotators is required before a dataset is accepted. It should also define how confidence calibration will be evaluated.

## 11. Validation

The validation plan must be proven, not assumed. The team should demonstrate:

- at least one complete annotated replay segment
- ground truth for core fields
- automated metrics generation
- regression support for detector changes

## 12. Recommendation

Recommendation: REJECT.

The validation subsystem is currently too immature to support implementation. It should be rebuilt around a concrete data pipeline and annotation methodology before the broader system proceeds.
