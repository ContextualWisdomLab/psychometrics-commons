# ADR-0010: Versioned provenance and immutable results

- Status: Accepted
- Date: 2026-08-09
- Scope: instrument, item, scoring, calibration, norm, narrative, consent, results, releases

## Context

A result is uninterpretable unless the exact instrument content, scoring algorithm, item parameters, norms, interpretation rules, and consent state are known. Mutable rows and human-readable names are insufficient for scientific reproducibility and regulated audit.

## Decision

Every result and research release is built from immutable versioned artifacts. At minimum a result snapshot records:

```text
instrument_version_ref
item_version_refs or response_snapshot_ref
assessment_spec_ref
scoring_version_ref
calibration_reference
norm_version_ref optional
narrative_version_ref
consent_snapshot_refs
engine_artifact_digest
created_at
supersedes_ref optional
```

Published artifacts are content-addressed by a cryptographic digest in addition to an opaque public reference. Semantic versions communicate compatibility; digests establish identity.

`latest` may be used only as a discovery alias before an operation. It is resolved and pinned before session creation, scoring, export, or release.

## Mutation policy

- Draft definitions may be edited before publication.
- Published definitions are immutable.
- Corrections create a new version or a superseding result.
- Historical snapshots remain readable under their original schemas through compatibility adapters or archived readers.
- Deletion obligations may remove payloads while retaining minimal tombstone/audit evidence where legally permitted; deletion never rewrites a different participant's provenance.

## Schema and contract versioning

Each serialized contract includes a schema version. Major changes require explicit negotiation and migration. Readers reject unknown required semantics rather than discarding fields. Additive optional fields must have documented defaults that do not alter prior meaning.

## Invariants

1. Replaying an immutable scoring bundle either reproduces the result within tolerance or yields a typed reproducibility failure.
2. Digest mismatch is always fatal.
3. Published versions never change their referenced bytes.
4. A narrative version cannot change numeric scores.
5. Norm updates do not retroactively change historical results; rescoring creates a new snapshot linked to the prior result.
6. Result exports include machine-readable provenance.

## Operational controls

A provenance manifest is emitted for each scoring and release operation. Logs refer to manifest and resource IDs, not raw responses. Artifacts are signed or otherwise attestable in release environments. Clock source and build metadata are recorded.

## Validation

- golden replay tests across supported versions;
- migration and backward-reader tests;
- digest tampering tests;
- norm/scoring/narrative independent-version tests;
- supersession-chain integrity tests;
- reproducible build and SBOM/provenance gates for releases.

## Alternatives rejected

- **Mutable `test_id` plus current scoring code:** cannot reproduce historical results.
- **Semantic version without digest:** names can be reused or artifacts changed.
- **Overwrite results after correction:** destroys audit history.
- **Put all versions in one product release number:** prevents independent scientific lifecycle management.

## Reversal conditions

Hash algorithm or artifact format may change through a dual-digest migration and documented compatibility window. Immutability and exact version provenance are not reversible requirements.
