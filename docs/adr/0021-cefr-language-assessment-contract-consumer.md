# ADR-0021: CEFR language-assessment contract consumer boundary

- Status: Proposed
- Date: 2026-08-28
- Deciders: Psychometrics Commons product and measurement owners
- Scope: `psychometrics-commons`, English A1-B2 placement consumer, result evidence binding
- Supersedes: none
- Superseded by: none

## Context

Issue #425 requires the hosted product to consume the shared
`cwl_cefr_language_assessment/v1` profile while keeping CEFR source material,
authored assessment content, response evidence, and psychometric numerics in
their owning boundaries. The upstream repository's PR #5 currently contains a
Draft contract, not a released artifact. The consumer therefore needs a
reviewable exact pin without converting Draft evidence into protected-main or
production conformance.

The Council of Europe CEFR Companion Volume is a framework authority, not a
certification authority for an examination provider. A profile reference,
empirical linking claim, and certification decision must remain distinct.

## Decision

Psychometrics Commons will consume the shared profile through the exact Draft
commit `ec9a2aa312ccd078da7b76c5325c34f1e1eb2482` and three raw schema
SHA-256 digests recorded in `src/cefr_language_assessment.rs`. The product
boundary stores only opaque release/blueprint/scoring/cut-score/result and
validator-evidence references. It does not copy the upstream schemas or CEFR
descriptors and does not calculate probabilities, uncertainty, linking, or
cut scores.

The first profile is English A1-B2 placement with four required domains:
reading reception, listening reception, written production, and spoken
production. It accepts `cefr_aligned` only. Overall reporting is disabled until
an exact immutable blueprint authorizes it and all required domains are measured.

This ADR describes a proposed review consumer. It becomes an implementation
and release decision only after the upstream contract is released and the
product integration adds the required instrument/result persistence and
runtime evidence.

## Ownership and boundaries

| Responsibility | Owner | Interface | Forbidden coupling |
|---|---|---|---|
| CEFR framework and official descriptors | Council of Europe / licensed source | governed source references | copying protected descriptor prose into product payloads |
| Shared blueprint/task/result schemas and validator | learning-interoperability-contracts | released immutable artifact, schema digest, validator evidence | product-owned schema fork or direct database access |
| Instrument publication, sessions, response evidence, result snapshots | psychometrics-commons | product APIs, persistence, versioned references | accepting an unbound result or mutating historical snapshots |
| Psychometric scoring, uncertainty, linking, DIF | fast-mlsirm | exact scoring artifact/contract | Python/LLM numerical reimplementation |
| Placement action/enrollment/completion | Learning Management Platform | versioned API/event contract | product making LMS decisions from a result envelope |
| LLM observations | contextual-orchestrator | bounded task/result contract | direct provider calls or numeric authority |

Dependency direction is product -> shared contract/scoring artifacts. No
external service receives Psychometrics Commons application-database access.

## Contract details

The consumer pins:

```text
source_repository
source_commit
assessment_blueprint_schema_digest
task_specification_schema_digest
result_snapshot_schema_digest
```

The shared profile version is `cwl_cefr_language_assessment/v1`; each result
envelope carries the more specific
`cwl_cefr_language_assessment/result_snapshot/v1` contract version. Each
accepted result must carry the exact versions, immutable blueprint reference,
result-schema digest, opaque executable-validator evidence reference, and
`cefr_aligned` claim status. Measured domains must be unique members of the
four-domain required set; profile-only results may contain an incomplete
measured subset, while overall reporting requires every required domain to be
measured. The product-side gate rejects blank/unsafe references, version or
digest drift, blueprint rebinding, duplicate/unsupported measured domains,
linking/certification claims, and overall reporting. Schema structure remains
the upstream validator's responsibility; the product verifies the validator
evidence identity and binding.

No idempotency key or transport is added by this review-only domain slice. Once
HTTP/event transport exists, its contract must be machine-readable and
versioned according to ADR-0014.

## Data and persistence impact

No migration or database object is added. The value object contains only
product-owned opaque references and immutable external artifact identities.
Raw task content, responses, audio, PII, descriptor prose, and provider output
remain in purpose-bound owners. A later persistence slice must add the profile
reference and result evidence as immutable, tenant-scoped state and reconcile
the logical ERD before migration.

## Invariants

1. The source commit and all three schema digests remain exact and immutable
   for a consumer build.
2. A result cannot cross this boundary without exact profile/result-envelope
   versions, blueprint, result-schema, validator-evidence, and domain bindings.
3. Only `cefr_aligned` is accepted by the initial profile.
4. Overall reporting cannot be enabled by a result payload alone.
5. No CEFR descriptor, raw response/audio, PII, or numeric engine payload is
   copied into this shared contract.

`tests/cefr_language_assessment_contract.rs` enforces pin identity, reference
validation, unique measured-domain subsets, result binding, claim separation,
overall denial, and stable typed errors.

## Failure and degraded modes

Missing or unavailable upstream validator evidence, schema digest drift,
contract/version drift, incomplete domains, unsupported claims, or blueprint
rebinding fail closed. No result is reported as aligned or overall. A result
read path may remain available for already accepted historical product
snapshots, subject to their own authorization, but it must not invent a CEFR
interpretation when validation evidence is unavailable.

Upstream release delay blocks promotion of this consumer from Draft. It does
not block unrelated Big Five, consent, data-rights, or result-read capability.

## Security, privacy, and tenancy

The result and validator references are opaque and tenant-scoped when persisted.
Authorization remains product-owned and is not inferred from upstream schema
validity. The shared profile carries no direct identity, raw response, audio,
descriptor, or provider payload. Encryption, retention, residency, and audit
follow the selected product deployment profile; this ADR adds no new secret or
provider path.

## Deployment and operations impact

The review-only pin adds no runtime network dependency or mandatory provider.
Deployment must record the source commit and schema digests in the build
manifest once the contract is released. Readiness must fail closed for a
required validator/artifact mismatch; unrelated deterministic result reads
remain capability-scoped. No SLO/RPO/RTO claim is created by this ADR.

## Migration and rollback

There is no migration for the Draft consumer slice. After upstream release, a
follow-up change replaces the Draft pin with the immutable released commit and
recomputes/verifies all schema digests. Rollback is a source roll-forward to a
previous accepted pin; already published results retain their original
contract/provenance and are not rewritten.

## Architecture-view impact

- `ARCHITECTURE.md`, `docs/architecture/C4.md`, and
  `docs/architecture/SECURITY_AND_DATA.md`: updated with the external contract
  owner and metadata-only trust boundary.
- `docs/architecture/UML.md` and `docs/architecture/ERD.md`: unchanged; no
  lifecycle or physical/logical entity is implemented by this slice.
- `docs/architecture/DEPLOYMENT_AND_OPERATIONS.md`: unchanged; no runtime
  dependency or recovery behavior is added.
- `docs/TRACEABILITY.md` and `docs/ROADMAP.md`: updated with Active PR/Target
  status and release gates.

## Validation and release evidence

Current evidence is limited to Rust format/check/clippy and seven focused
contract tests. Promotion requires the upstream released artifact, executable
validator conformance fixtures, exact fast-mlsirm scoring evidence, rights and
English locale evidence, real PostgreSQL tenant/immutability/recovery tests,
transport contract tests, and independent review on the exact protected head.
No release, CEFR linking, certification, or production claim is made now.

## Alternatives considered

- Copy the upstream JSON schemas into this repository: rejected because it
  creates drift and duplicates the shared contract authority.
- Treat a valid JSON envelope as a CEFR-linked or certified result: rejected
  because structural validity does not establish empirical linking or
  certification authority.
- Let an LLM assign levels or override missing domains: rejected because AI is
  fallible evidence and cannot change numeric/scientific gates.
- Wait for upstream release before writing any consumer code: rejected for
  review purposes only; an exact Draft pin can expose the dependency contract
  without claiming readiness.

## Consequences

The consumer boundary is small, auditable, and reusable by later persistence
and transport work. The cost is that it cannot produce a publishable instrument
or result until upstream release and scientific/content evidence arrive. The
Draft pin is an explicit accepted review risk, not release evidence.

## Follow-up work

1. Upstream contract owner: release PR #5 after independent review and
   immutable artifact publication.
2. Psychometrics Commons: bind the profile to a persisted immutable instrument
   release and result snapshot after the release pin is available.
3. fast-mlsirm: provide the exact scoring/profile and linking evidence artifacts.
4. Product/content owners: provide rights, English locale, task, and
   standard-setting evidence without copying protected source material.
5. Product API owner: add as-built OpenAPI/event contracts and real PostgreSQL
   recovery/tenant evidence when transport is implemented.

## Reversal conditions

Revisit this decision if the upstream contract changes ownership/versioning,
the CEFR authority changes the relevant framework publication, the product must
support a language/profile outside this four-domain scope, or a stronger
versioned evidence contract supersedes the current validator-boundary design.

## Traceability

- PRD §6.1 and §9.11; TRD §2, §7, §8.1, and §9.
- Issue #425 and upstream `learning-interoperability-contracts` PR #5.
- `src/cefr_language_assessment.rs` and
  `tests/cefr_language_assessment_contract.rs`.
- `docs/doctoring/CEFR_LANGUAGE_ASSESSMENT.md` and Council of Europe (2020).
- ADR-0004, ADR-0010, ADR-0013, ADR-0014, ADR-0019, and ADR-0021.
