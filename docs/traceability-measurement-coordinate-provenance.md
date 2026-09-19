# Measurement-coordinate provenance traceability supplement

- **Status:** Active PR supplement; not protected-main truth
- **Owner:** Psychometrics Commons Measurement / Scoring / Result provenance
- **PR:** #449
- **Issue:** #448
- **Consumer gap:** TEPP#602
- **Decision:** ADR 0022 (Proposed)

This file exists because the canonical `docs/TRACEABILITY.md` is a long shared operator record and must not be replaced from a partial fetch. It records the exact #449 mapping that must be folded into canonical TRACEABILITY ordinary-forward before merge. Removing this supplement is permitted only after verified inheritance of every row below.

| Requirement / invariant | Owner source | Exact active-PR evidence | Promotion boundary |
|---|---|---|---|
| One longitudinal coordinate authority includes exact construct identity in addition to scoring-level provenance | `src/measurement_coordinate.rs`; immutable `ResultSnapshot` source state | RED `ae478bdfc5c51dffeb9082e3ea87583a63c189e3`; source/export lineage through current #449 head | Exact-head tests/coverage/security/review, protected-main merge, immutable release |
| Measurement-coordinate authority is participant/source-text free and does not recompute/carry the numeric score | `MeasurementCoordinateProvenance::from_result_snapshot` | `tests/measurement_coordinate_provenance.rs` rejects unknown/unscored constructs and asserts participant/response/narrative state is absent | Privacy/security review and immutable release |
| Canonical bytes have one decode→re-encode identity; aliases, malformed lengths, unsupported versions/schema, invalid references/digests and trailing fields fail closed | `MeasurementCoordinateProvenance::from_canonical_bytes` / `canonical_bytes` | RED `e20dd2fa4462bd1d95eea83ab880c9579d691550`; decoder/edge contracts `354245bdce7aa36a6b35b17948655f7ea9afef1a`, `1f3374e54b72aabc5269c3bf0315700ebff0e7d6` | Exact-head Runtime CI, line/branch coverage and compatibility review |
| Cross-repository coordinate wire is resource-bounded before parse/copy/publication | `MEASUREMENT_COORDINATE_MAX_CANONICAL_BYTES = 8_192`; producer preflight; decoder preflight | QA-PERF-03 RED `68fb77342660c38016825bb58a0be9f98a4a1150`; causal repair `d5996e45cea62cd65043b1b2bd3138d0fafaf959`; public error-surface coverage `600232b9d53c32109a8de16a1d340ec78b40fe4f` | Hosted exact-head CI/security/SAST/SBOM/provenance plus compatibility evidence; no truncation |
| Product scoring API remains unchanged by the new projection | existing protected-main `src/scoring.rs` | over-broad refactor restored ordinary-forward in `770181fb2eee8d2aaac220b44a33f856a371c251`; current base→head compare has no `scoring.rs` delta | Re-check compare immediately before merge |
| Contract is not production authority until immutable release | release policy; issue #448; ADR 0022 Proposed | #449 remains Draft; mutable branch bytes are development evidence only | Protected-main landing, immutable tag/package, SBOM/provenance, reproducibility, rollback/compatibility, qualifying independent review |
| TEPP consumes separate predictor/outcome released coordinate authorities through an ACL | TEPP#602 | No TEPP production repair is authorized while #449 is mutable/unreleased | Immutable Psychometrics Commons release first, then TEPP RED→ACL repair; no source copy/cross-service SQL |
| Provenance identity is not psychometric validity evidence | Measurement Governance; ADR 0019; ADR 0022 Proposed | Contract carries provenance only and explicitly excludes claims about construct validity, invariance, calibration adequacy, linking, fairness or intended-use fitness | Scientific claims require their own evidence and release gates |

## Current exact-head evidence status

At the current owner slice, GitHub Actions jobs have been observed as positive-unassigned before checkout (`queued`, `steps=[]`, `runner_id=0`, empty runner/group identity). `.github#712` owns the organization runner diagnosis. Queued checks are incomplete evidence, not GREEN and not permission to create no-op wake commits or weaken labels.

## Required canonical fold

Before #449 can leave Draft, reconcile this supplement into the current canonical `docs/TRACEABILITY.md` and affected `docs/MEASUREMENT_GOVERNANCE.md`/change history without truncating unrelated protected-main or concurrent-PR material. ADR 0022 must remain Proposed until the owner release path is actually satisfied.
