# ADR 0022 — Released construct-bound measurement-coordinate provenance

- **Status:** Proposed
- **Date:** 2026-09-19
- **Owners:** Measurement / Scoring / Result provenance
- **Related:** #448, PR #449, TEPP#602, ADR 0010, ADR 0014, QA-SCI-01/03, QA-PERF-03, QA-INT-03, QA-MNT-02, QA-OBS-02

## Context

Psychometrics Commons already freezes assessment specification, instrument version, scoring version, calibration, optional norm, requested output schema, scoring-engine artifact digest, and construct-level score observations in an immutable `ResultSnapshot`. Longitudinal consumers such as TEPP need to bind an already-mapped numeric coordinate to that exact Measurement authority without importing participant state, copying Psychometrics Commons product structs, reading its database, or recreating psychometric arithmetic.

A scoring contract can emit multiple constructs. Therefore an instrument/scoring version alone cannot identify one longitudinal coordinate: predictor and outcome may share every scoring-level field and even the same numeric binary64 value while representing different constructs. The coordinate authority must include exact `construct_ref`.

The contract is intended to cross repository and release boundaries. The first #449 decoder/source slice was canonical but unbounded: shared opaque references intentionally do not impose a global length policy, so an attacker-controlled or malformed provenance envelope could force arbitrarily large parse/copy/re-encode work. QA-PERF-03 requires bounded external/model payload behavior.

## Decision

Psychometrics Commons owns a participant/source-text-free `MeasurementCoordinateProvenance` contract. Contract version 1 binds exactly:

- `assessment_spec_ref`;
- `instrument_version_ref`;
- `scoring_version_ref`;
- `calibration_reference`;
- optional `norm_version_ref` with explicit presence semantics;
- requested scoring-output schema version;
- canonical scoring-engine artifact digest;
- exact scored `construct_ref`.

The projection does not include the numeric score. Consumers bind the coordinate value and its coordinate authority separately, preserving the distinction between observed value and provenance. It also excludes participant/session/response/consent/narrative/source text, credentials, provider payloads, and hidden reasoning.

Canonical v1 bytes are domain-separated, field-labelled, length-delimited, ordered, UTF-8 bytes. Decoding is strict: unsupported contract/output-schema versions, non-exact references, malformed engine digests, aliases, trailing fields, malformed lengths, and non-canonical decode→re-encode forms fail closed.

The v1 canonical envelope is bounded at **8,192 bytes**. This is a wire/resource-safety ceiling, not a scientific reference-length rule. Producer projection computes the encoded size before cloning snapshot provenance; consumer decode checks the total byte length before UTF-8 parsing. Provenance is rejected rather than truncated. The limit is part of the v1 compatibility contract and changing it requires explicit compatibility review/versioning evidence.

The 8 KiB choice is deliberately conservative: the contract has a fixed field count and current owner references are compact opaque identifiers, while the repository already treats 8 KiB as a bounded hosted request-envelope scale. A smaller per-reference limit was rejected because it would silently introduce new semantics for existing immutable owner references; an unbounded envelope was rejected under QA-PERF-03. Future transport profiles may impose a lower outer-envelope limit without changing the scientific meaning of the coordinate authority.

## Release and consumer rule

A mutable branch or PR head is development evidence only. Downstream repositories may treat this contract as production authority only after the source lands on protected Psychometrics Commons `main` and an immutable release publishes the landed contract with exact-head CI/security/privacy/coverage/review, package/tag digest, SBOM/provenance, reproducibility, compatibility, and rollback evidence.

TEPP#602 must consume the released contract through an ACL and bind **separate predictor and outcome authorities** into digest-bound longitudinal provenance. Missing, malformed, unsupported, mutable, or unreleased authority fails closed. No consumer may source-copy this module or use cross-service SQL.

## Scientific interpretation boundary

The coordinate authority proves provenance identity only. It does **not** establish construct validity, measurement invariance, calibration adequacy, linking quality, fairness, score interpretation, causal identification, or fitness for a longitudinal intended use. Those claims remain governed by measurement governance and scientific acceptance evidence.

## Alternatives considered

### Reuse only `scoring_version_ref`

Rejected. One scoring result can contain several construct observations; predictor and outcome would collapse under a shared scoring version.

### Put the numeric score into the owner projection

Rejected. TEPP and other consumers already own their admitted coordinate/value evidence. Mixing the value into the owner provenance contract would conflate content with authority and make reuse harder without adding scientific evidence.

### Let each consumer reconstruct canonical bytes or hash arbitrary product structs

Rejected. That duplicates an owner contract across repositories and risks incompatible serialization, aliases, source copying, and mutable schema coupling.

### Keep the canonical wire unbounded

Rejected. Cross-repository decode is an external compatibility boundary and must satisfy QA-PERF-03 before publication.

### Add per-reference truncation or normalization

Rejected. Truncation would corrupt identity; additional normalization would alias exact opaque references. Existing `normalized_reference` exactness remains authoritative.

## Consequences

- `MeasurementCoordinateProvenance` and its decoder/encoder become public compatibility surface and require semantic version discipline.
- `construct_ref` is a first-class part of coordinate authority even when numeric values are equal.
- The 8 KiB total bound is observable API behavior and must have RED/edge tests.
- No database migration is required for this source projection; it is derived from immutable `ResultSnapshot` state.
- No psychometric numerical kernel moves into Psychometrics Commons.
- Release documentation, TRACEABILITY, measurement governance, rustdoc, tests, security/privacy review, SBOM/provenance, and consumer compatibility evidence must remain code-current.

## Evidence and acceptance

Current PR #449 TDD lineage includes:

- construct-bound RED `ae478bdfc5c51dffeb9082e3ea87583a63c189e3`;
- canonical decode/alias RED `e20dd2fa4462bd1d95eea83ab880c9579d691550`;
- resource-safety RED `68fb77342660c38016825bb58a0be9f98a4a1150`;
- bounded-wire repair `d5996e45cea62cd65043b1b2bd3138d0fafaf959`;
- public error-surface edge coverage `600232b9d53c32109a8de16a1d340ec78b40fe4f`.

This ADR remains **Proposed** while #449 is Draft and its exact-head hosted evidence, documentation reconciliation, independent review, protected-main merge, and immutable release are incomplete. It must not be marked Accepted merely because source tests exist.
