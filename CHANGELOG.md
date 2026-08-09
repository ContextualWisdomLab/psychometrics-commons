# Changelog

All notable product and architecture changes are recorded here. Releases use immutable Git tags and provenance; entries move out of **Unreleased** only when the exact integrated protected head satisfies the repository's release gates.

## Unreleased

### Added

- Initial Psychometrics Commons product requirements covering the IPIP-based Big Five consumer vertical slice, reflective modules, longitudinal participation, Research Commons, and Measurement Workbench.
- Technical requirements for hosted runtime ownership, state machines, idempotent response/scoring flows, identity and tenant boundaries, consent and data-rights workflows, research pseudonymization and release, event/outbox integration, accessibility, multilingual instrument versions, deployment profiles, and release evidence.
- Detailed Architecture Decision Record governance and ADR-0001 through ADR-0013 for product ownership, headless clients, Keyverse integration, fast-mlsirm measurement ownership, hosted runtime lifecycle, consent/research separation, research-release boundaries, longitudinal analysis, bounded AI, immutable provenance, deployment profiles, legacy-R exclusion, and multilingual accessibility/invariance.
- Authoritative architecture map linking product modules to their owning CWL bounded contexts and defining failure-degradation behavior and architecture fitness functions.
- Rust hosted-runtime session lifecycle primitives with fail-closed server-authoritative transitions for activation, pause/resume, completion, scoring, release, expiry, cancellation, and invalidation.
- Idempotent response-event ledger semantics with monotonic server sequencing, conflicting replay rejection, and deterministic immutable response snapshots frozen only after session completion.
- Version-pinned scoring-dispatch contracts that consume completed immutable response snapshots without reimplementing psychometric numerics, preserve scored/abstained/failed/excluded outcomes, and reject ambiguous or non-finite scoring evidence.
- Immutable result snapshots that copy score observations and exact AssessmentSpec, instrument, scoring, calibration, norm, narrative, consent, engine-artifact, and supersession provenance without silently recomputing historical results.
- Purpose-specific append-only consent events and immutable consent snapshots that keep service, account persistence, longitudinal observation, research contribution, and communications decisions independently revocable and auditable.
- Research-contribution lifecycle primitives that require an explicit scoped research grant, separate pseudonymous research identity from the operational participant, and preserve idempotent irreversible withdrawal evidence.
- Tenant-scoped participant export/deletion request lifecycle primitives that require request-specific identity verification, preserve durable operation/completion evidence, keep event time monotonic, make exact lifecycle replays idempotent, and represent legal retention exceptions as explicit partial deletion completion rather than a misleading deletion boolean.
- Session-bound item-delivery evidence that pins the immutable instrument release, content digest, locale, presentation context, optional selection provenance, server order, idempotent replay, and duplicate-item protection without duplicating psychometric selection logic.
