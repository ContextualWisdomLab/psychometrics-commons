# ADR-0004: fast-mlsirm as measurement and scoring source of truth

- Status: Accepted
- Date: 2026-08-09
- Scope: assessment contracts, scoring, calibration, uncertainty, model diagnostics

## Context

Psychometrics Commons must produce reproducible scores while remaining able to evolve instrument, calibration, norm, and narrative versions. Duplicating psychometric formulas in the product runtime or delegating final numeric scores to an LLM would produce irreconcilable results and invalidate scientific evidence.

## Decision

`fast-mlsirm` owns the canonical reusable measurement contracts and all production psychometric numerical kernels. Psychometrics Commons orchestrates scoring but does not reimplement likelihoods, gradients, Hessians, optimization, latent scoring, linking, DIF, uncertainty, or model-selection arithmetic.

The minimum scoring contract is:

```text
AssessmentSpec + RubricSpecification
+ response_snapshot_ref
+ scoring_version_ref
+ calibration_reference
+ norm_version_ref optional
-> ScoringRequest
-> ScoreObservation[] / ScoringResult
```

Every scoring request is content-addressed or has a deterministic idempotency key. Every result identifies the exact core/package version, backend, model specification, item parameters, numerical tolerances, and warnings.

## Response and result states

Provider or rater observations use explicit states: `scored`, `abstained`, `failed`, `excluded`. Missing, abstained, failed, and zero are never conflated.

A scoring job ends in one of:

- `completed`: valid result snapshot produced;
- `rejected`: contract or scientific precondition failed;
- `failed_retryable`: infrastructure failure with no result;
- `failed_terminal`: deterministic numerical or data failure requiring intervention.

## Invariants

1. The same immutable response snapshot and version bundle reproduces the same score within documented numerical tolerance.
2. Python may validate and marshal but numerical kernels remain Rust-first.
3. GPU paths require CPU parity evidence and cannot silently change model semantics.
4. An LLM may generate narrative text but cannot alter numeric scores, norms, uncertainty, or pass/fail gates.
5. Correlation-only evidence is insufficient for a new estimator; true-parameter bias, RMSE, interval coverage, convergence, and backend parity are required.
6. Multilevel, cross-classified, multiple-membership, testlet, and longitudinal structures must not be flattened when the declared AssessmentSpec requires them.

## Compatibility

Psychometrics Commons declares the contract versions it supports. Unsupported major versions fail before scoring. Minor versions are accepted only when schemas and semantic feature flags are compatible. Model artifacts are immutable and referenced by digest, not mutable names such as `latest`.

## Failure behavior

Invalid models, unknown relations in model comparison, non-identification, insufficient linking anchors, non-finite estimates, and scoreability failures are fail-closed. The runtime returns a typed scientific error and never substitutes a simpler score without a separately approved policy.

## Security

Raw responses are passed by bounded payload or secure reference according to deployment policy. Logs contain hashes and identifiers, not response content. Model artifacts and scoring binaries are verified before use. External model calls, where a domain adapter requires them, are not part of the deterministic core result.

## Validation and release evidence

- Rust unit/property tests and Python delegation tests;
- true-parameter recovery under realistic data-generating conditions;
- CPU/GPU parity where GPU is enabled;
- regression tests for published instrument fixtures;
- DIF/invariance and linking acceptance tests;
- reproducible package and artifact provenance.

## Alternatives rejected

- **Scoring code in the product service:** duplicates scientific logic.
- **LLM-produced personality scores:** non-deterministic and not calibrated.
- **Raw-score-only MVP as permanent architecture:** cannot support versioned norms, uncertainty, or adaptive testing.

## Reversal conditions

A different engine may replace fast-mlsirm only through the same contracts and after independent recovery, parity, and migration evidence. Historical results continue to reference their original engine artifact.
