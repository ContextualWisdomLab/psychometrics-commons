### Added

- Added versioned rater-panel, observation-request, and adjudication aggregate
  roots for governed human, model, and algorithmic rating operations.
- Preserved repeated calls under one exact rater configuration instead of
  representing them as independent raters.
- Preserved provider failures as explicit terminal request states and kept
  adjudication resolutions separate from immutable source invocations.
