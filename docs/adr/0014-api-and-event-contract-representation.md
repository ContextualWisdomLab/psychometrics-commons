# ADR-0014: Machine-readable HTTP and event contract representation

- Status: Accepted
- Date: 2026-08-09
- Deciders: ContextualWisdomLab Psychometrics Commons maintainers
- Scope: Psychometrics Commons public/admin HTTP APIs, product-owned durable domain events, errors, schema/version negotiation
- Supersedes: none
- Superseded by: none
- Current/as-built status: public/admin product HTTP transport and durable external event transport are not yet implemented on protected main; operator GET `/live` and GET `/ready` probes exist only on Active PR #91 with `openapi/health-probes.yaml`; that same PR binds a TCP listener for those listed operations and can answer them from `observe_postgres_operational_snapshot` without exposing driver errors
- Target status: every implemented HTTP/event surface has an exact versioned machine-readable as-built contract and deterministic integrity/idempotency semantics
- Migration status: no deployed HTTP/event transport requires migration yet; the first implementation must introduce the contract in the same or prerequisite PR

## Context

The TRD defines public routes, domain-event families, idempotency, and failure semantics, but prose alone is not sufficient for generated clients, contract testing, integration review, or acquisition due diligence. At the same time, creating a large speculative API specification before transport implementation would falsely imply that unimplemented operations are available.

The product needs one rule that makes machine-readable contracts mandatory **when the corresponding transport exists**, without allowing a future contract to masquerade as an as-built API.

## Decision

1. Implemented Psychometrics Commons HTTP APIs are described by an **OpenAPI 3.2.x** document pinned to the exact supported specification version in repository source. As of this decision, the selected specification is OpenAPI 3.2.0.
2. Implemented product-owned asynchronous channels/events are described by an **AsyncAPI 3.1.x** document pinned to the exact supported specification version. As of this decision, the selected specification is AsyncAPI 3.1.0.
3. HTTP API error responses use **RFC 9457 Problem Details** (`application/problem+json`) for reusable machine-readable error semantics unless a separately documented domain representation is more appropriate.
4. JSON payload/schema definitions use a closed, versioned schema vocabulary compatible with the chosen OpenAPI/AsyncAPI specification. Unknown required semantics fail closed.
5. A machine-readable as-built contract may list only operations/events actually implemented or emitted/consumed by that release. Proposed/future contracts must be clearly identified as non-deployed design artifacts and cannot satisfy release acceptance.
6. Durable event payload integrity uses one canonical payload representation and digest contract; consumers validate it before deduplication completion or any side effect.

## Ownership and boundaries

| Responsibility | Owner | Interface | Forbidden coupling |
|---|---|---|---|
| Psychometrics Commons public/admin HTTP operations | psychometrics-commons | OpenAPI + implementation | clients depending on internal Rust/kernel ABI |
| Psychometrics Commons product-domain events | psychometrics-commons | AsyncAPI + event schemas | consumers inferring schema from application tables |
| fast-mlsirm scoring API/package contract | fast-mlsirm | upstream-owned versioned contract | redefining measurement schema here |
| Keyverse OIDC/OAuth behavior | Keyverse | identity standards/contracts | copying identity-provider internals into product OpenAPI |
| semantic-data-portal API/events | semantic-data-portal | portal-owned contract | defining portal internals in this repository |

## HTTP contract details

Every state-changing operation must define:

- request schema;
- idempotency-key or equivalent resource-specific replay contract;
- authenticated/anonymous authority requirements;
- tenant/resource scope;
- success representation;
- stable error/problem type(s);
- conflict/replay semantics;
- exact path/resource identifier semantics;
- retryability classification where relevant.

Public identifiers are opaque and non-numeric.

Problem Details instances must not expose raw SQL, provider responses, secrets, tokens, restricted linkage values, or raw assessment content. Product-specific problem types use stable URIs under a product-controlled namespace and document the intended HTTP status and remediation semantics.

## Event contract details

Every durable product event includes at minimum:

```text
event_ref
event_type
schema_version
source
tenant_ref when the subject is tenant-scoped
subject_ref
occurred_at
correlation_ref
causation_ref optional
payload_digest
payload
```

### Canonical payload and digest

For JSON events, `payload` is serialized as canonical UTF-8 JSON using the repository-pinned canonicalization profile. The first implementation must use RFC 8785 JSON Canonicalization Scheme or a separately accepted equivalent with byte-for-byte test vectors. Invalid Unicode, duplicate object member names, non-finite numbers, or values not representable under the chosen canonical profile are rejected before publication.

`payload_digest` covers **only the canonical payload bytes**, not the envelope. It uses SHA-256 and is represented as `sha256:` followed by 64 lowercase hexadecimal digits. Envelope fields are independently validated against the event schema and immutable event identity. If a future full-envelope signature/digest is required, it receives a distinct field and an ADR/schema update; `payload_digest` must not silently change meaning.

The producer computes and persists the canonical payload digest before publication. The consumer canonicalizes the received payload under the declared schema/canonicalization version and verifies the digest before recording successful consumption or applying effects. Missing, malformed, or mismatched digests fail closed and the event is quarantined with safe evidence; no partial side effect is allowed.

### Event identity, tenant binding, and deduplication

`event_ref` is unique within the `(source, tenant_ref)` scope for tenant-scoped subjects and within `source` for explicitly non-tenant events. The producer binds `tenant_ref` to the subject resource in the same product-owned transaction that creates the outbox event. A receiver validates that tenant/resource binding before creating a processable inbox record.

Consumer deduplication identity is:

```text
(consumer_name, source, tenant_ref_or_explicit_none, event_ref)
```

An exact replay with the same canonical payload digest and compatible schema returns/reuses the prior consumption outcome. Reuse of the same deduplication identity with a different digest, tenant, subject, or incompatible required semantics is a conflict and is quarantined/fails closed rather than overwriting prior evidence.

Deduplication evidence is retained at least through the maximum supported source replay/broker-retention horizon, disaster-recovery replay horizon, and any unfinished side-effect reconciliation horizon applicable to that event. A deployment may retain it longer for audit/legal policy, but it may not expire deduplication state while the producer or recovery process can legitimately replay the event. The concrete retention policy is version-controlled by deployment profile and tested with replay/restore scenarios rather than an arbitrary universal duration.

### Replay, consumption, and quarantine

Receiving an event creates durable processing evidence such as `pending -> processing -> completed` rather than treating receipt alone as successful side-effect completion. If the external effect cannot be included in the same local transaction, completion is recorded only after an idempotency-key-bound result/evidence exists or a local recoverable work/outbox record guarantees retry after crash. Poison or unverifiable events are quarantined after the configured bounded attempts; quarantine preserves safe event identity/digest/failure evidence and never applies an unknown partial semantic.

Event schemas are versioned independently from transport technology; Kafka, NATS, AMQP, or another broker may be chosen by a deployment without changing domain-event meaning. Ordering guarantees are explicit per stream/resource. A global total order is not assumed.

When durable event transport is implemented, the AsyncAPI/schema artifact must encode or reference these canonicalization, digest, tenant-binding, deduplication, replay, processing-state, and quarantine rules. A prose-only divergence from the machine-readable/implementation contract is a release defect.

## Data and persistence impact

No transport persistence exists yet on protected main. The target logical model requires outbox event identity, tenant/subject binding, schema/canonicalization version, payload digest, delivery attempts, inbox deduplication identity, processing state, side-effect evidence, and quarantine/reconciliation evidence. `docs/architecture/ERD.md` defines the logical target; physical migrations must preserve these semantics when introduced.

## Invariants

1. The same canonical JSON payload always produces the same SHA-256 `payload_digest` under the pinned canonicalization profile.
2. `payload_digest` never changes meaning from payload-only to envelope digest without a versioned contract change.
3. Invalid/unverifiable digest evidence produces no side effect.
4. Tenant-scoped events cannot be consumed under a different tenant/resource binding.
5. The same consumer deduplication identity plus identical evidence produces at most one logical side effect.
6. Conflicting replay evidence never overwrites a prior completed or pending outcome.
7. Inbox receipt is not synonymous with side-effect completion.
8. Deduplication evidence is not expired while legitimate replay/recovery remains possible.
9. Implemented API/event operations are represented by the exact as-built OpenAPI/AsyncAPI contract before release.

## Versioning and compatibility

- An exact specification version is pinned in the contract file.
- API/event major-version changes require an explicit compatibility window and migration plan.
- Additive optional fields may be introduced only when prior semantics remain unchanged.
- New required semantics require a compatible version strategy rather than assuming old clients will ignore them.
- Mutable aliases such as `latest` are not persisted as provenance or compatibility evidence.
- Historical result/release readers remain available for the product's supported retention window.
- Canonicalization/digest semantic changes require an explicit schema/contract version and compatibility or migration path.

## Failure and degraded modes

- Invalid request or unsupported required semantics: fail closed with stable problem type.
- Conflicting HTTP idempotency replay: conflict/problem response; never overwrite prior evidence.
- Optional downstream capability unavailable: surface typed capability-specific degradation while preserving unrelated product functions.
- Event consumer cannot understand schema major/canonicalization semantics: reject/quarantine rather than partially applying unknown semantics.
- Event digest missing/malformed/mismatched: quarantine/fail closed before side effects.
- Transport retry: repeats the same immutable message/request identity; it does not regenerate mutable business content.
- Consumer crash during effect: retry from durable pending/recoverable work evidence; do not mark completed merely because the inbox row exists.

## Security, privacy, and tenancy

- Tenant context for state-changing APIs comes from authenticated authority, not a body default.
- Tenant-scoped event envelopes carry and validate product-derived tenant/resource binding.
- Contract examples/fixtures use synthetic data and contain no real credentials or participant data.
- Client generators must not receive internal service credentials.
- Error/problem examples cannot normalize leaking raw rejected values.
- Event payloads are purpose-minimized; sensitive content is carried only when the receiving bounded context is explicitly authorized to process it.
- Quarantine/audit evidence stores safe identities/digests/failure classes rather than raw sensitive payload by default.

## Deployment and operations impact

Transport/broker choice is deployment-specific, but health and reconciliation must expose backlog, retry, digest-validation failure, quarantine, and deduplication state without leaking payloads. Restore/replay exercises must prove that persisted inbox/outbox identities prevent duplicate logical effects after recovery.

## Migration and rollout

The first implemented HTTP transport must introduce its OpenAPI document in the same PR or an accepted prerequisite PR. The first durable event transport must do the same for AsyncAPI plus the canonicalization/digest implementation and persistence constraints.

Contract changes are validated before deployment. During compatibility windows, old and new versions may be served/consumed concurrently only when the implementation has explicit routing/adapter tests.

Rollback must restore an application version that still understands any messages/resources already committed under the deployed compatible schema. A rollback that would strand newly persisted required semantics is not safe; use roll-forward or a compatibility adapter instead.

## Architecture-view impact

- `docs/architecture/ERD.md` must represent tenant-bound outbox/inbox identity and consumption state.
- `docs/architecture/UML.md` must not model receipt as equivalent to externally visible side-effect completion.
- `docs/architecture/SECURITY_AND_DATA.md` must preserve tenant/purpose boundaries for event payloads and quarantine.
- `docs/architecture/DEPLOYMENT_AND_OPERATIONS.md` must include replay/quarantine/recovery evidence when event transport is implemented.
- `docs/TRACEABILITY.md` remains target until as-built OpenAPI/AsyncAPI and transport tests exist.

## Validation and release evidence

Release gates for an implemented transport include:

- OpenAPI/AsyncAPI parser/schema validation;
- implementation-to-contract route/message coverage;
- request/response/event fixture validation;
- RFC 9457 problem-type contract tests;
- canonical payload and SHA-256 test vectors;
- invalid/malformed/duplicate-key/non-finite payload rejection tests;
- tenant/resource envelope mismatch tests;
- idempotency/deduplication/conflicting replay tests;
- crash/retry tests proving pending side effects remain recoverable;
- quarantine tests for invalid digest, unsupported schema, and poison events;
- restore/replay tests covering the configured deduplication-retention horizon;
- unsupported-version fail-closed tests;
- client/consumer compatibility tests for the supported window;
- security tests that verify examples/errors/quarantine evidence do not disclose prohibited data.

Until the transport exists, these are explicit target acceptance requirements rather than fabricated passing evidence.

## Alternatives considered

### Prose-only API/event documentation

Rejected. It is not sufficient for generated clients, automated compatibility checks, or exact release evidence.

### Generate a complete future API now

Rejected. It would create a false as-built contract for operations that do not exist.

### Hash arbitrary serializer output

Rejected. Different serializers/key orders/number representations can produce different bytes for semantically equivalent JSON and undermine replay/integrity evidence.

### Treat receipt/inbox insert as side-effect completion

Rejected. A crash after receipt but before a non-transactional side effect could permanently suppress the missing effect on replay.

### GraphQL/gRPC as the mandatory product transport

Not selected at this stage. A future ADR may add one if a concrete product need exists. The domain contracts remain transport-neutral.

### Custom error envelope

Rejected as the default. RFC 9457 already defines interoperable HTTP problem semantics and avoids inventing another generic error format.

## Consequences

Positive:

- API/event behavior becomes machine-checkable and client-generator friendly;
- contract/version drift becomes release-detectable;
- payload integrity and replay behavior are deterministic across implementations;
- tenant/resource binding and crash-safe consumption are explicit;
- error semantics are standardized;
- target architecture remains distinguishable from deployed capability.

Costs:

- transport changes must update schemas and compatibility tests;
- canonicalization, deduplication, quarantine, and recovery state require implementation/operational complexity;
- generated client/server tooling must be pinned and maintained.

## Follow-up work

- when the first HTTP transport lands, add the exact OpenAPI document and route/problem contract tests;
- when the first durable event transport lands, add AsyncAPI plus canonicalization/digest test vectors and tenant-bound outbox/inbox migrations;
- add consumer crash/replay/quarantine integration tests against the selected persistence/broker adapters;
- link deployment-specific deduplication-retention policy to backup/restore and broker retention evidence.

## Traceability

- Product requirements: `docs/PRD.md` functional, security/privacy, and release acceptance.
- Technical requirements: `docs/TRD.md` API, event, transactional integration, version compatibility, security, and validation sections.
- Architecture: `ARCHITECTURE.md`, `docs/architecture/ERD.md`, `docs/architecture/UML.md`, `docs/architecture/DEPLOYMENT_AND_OPERATIONS.md`.
- Decisions: ADR-0015 for persistence/transaction boundaries.
- Delivery evidence: `docs/TRACEABILITY.md`, `docs/ROADMAP.md`, `tests/documentation_architecture_contract.rs` until as-built transport tests replace documentation-only fitness evidence.

## Reversal conditions

A future major transport change may supersede this ADR if OpenAPI/AsyncAPI no longer describe the primary deployed interfaces. The principles that deployed interfaces require machine-readable, versioned, as-built contracts and deterministic integrity/idempotency semantics remain.

## References

Bray, T. (Ed.). (2017). *The JavaScript Object Notation (JSON) Data Interchange Format* (RFC 8259). Internet Engineering Task Force. https://doi.org/10.17487/RFC8259

Rundgren, A., Jordan, B., & Erdtman, S. (2020). *JSON Canonicalization Scheme (JCS)* (RFC 8785). Internet Engineering Task Force. https://doi.org/10.17487/RFC8785

AsyncAPI Initiative. (2026). *AsyncAPI Specification, Version 3.1.0*.

Nottingham, M., Wilde, E., & Dalal, S. (2023). *Problem Details for HTTP APIs* (RFC 9457). Internet Engineering Task Force. https://doi.org/10.17487/RFC9457

OpenAPI Initiative. (2025). *OpenAPI Specification, Version 3.2.0*.
