# ADR-0014: Machine-readable HTTP and event contract representation

- Status: Accepted
- Date: 2026-08-09
- Scope: Psychometrics Commons public/admin HTTP APIs, product-owned durable domain events, errors, schema/version negotiation
- Supersedes: none

## Context

The TRD defines public routes, domain-event families, idempotency, and failure semantics, but prose alone is not sufficient for generated clients, contract testing, integration review, or acquisition due diligence. At the same time, creating a large speculative API specification before transport implementation would falsely imply that unimplemented operations are available.

The product needs one rule that makes machine-readable contracts mandatory **when the corresponding transport exists**, without allowing a future contract to masquerade as an as-built API.

## Decision

1. Implemented Psychometrics Commons HTTP APIs are described by an **OpenAPI 3.2.x** document pinned to the exact supported specification version in repository source. As of this decision, the current published specification is OpenAPI 3.2.0.
2. Implemented product-owned asynchronous channels/events are described by an **AsyncAPI 3.1.x** document pinned to the exact supported specification version. As of this decision, the current specification line is AsyncAPI 3.1.0.
3. HTTP API error responses use **RFC 9457 Problem Details** (`application/problem+json`) for reusable machine-readable error semantics unless a separately documented domain representation is more appropriate.
4. JSON payload/schema definitions use a closed, versioned schema vocabulary compatible with the chosen OpenAPI/AsyncAPI specification. Unknown required semantics fail closed.
5. A machine-readable as-built contract may list only operations/events actually implemented or emitted/consumed by that release. Proposed/future contracts must be clearly identified as non-deployed design artifacts and cannot satisfy release acceptance.

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
subject_ref
occurred_at
correlation_ref
causation_ref optional
payload_digest
payload
```

Consumers deduplicate before side effects. Event schemas are versioned independently from transport technology; Kafka, NATS, AMQP, or another broker may be chosen by a deployment without changing domain-event meaning.

Ordering guarantees are explicit per stream/resource. A global total order is not assumed.

## Versioning and compatibility

- An exact specification version is pinned in the contract file.
- API/event major-version changes require an explicit compatibility window and migration plan.
- Additive optional fields may be introduced only when prior semantics remain unchanged.
- New required semantics require a compatible version strategy rather than assuming old clients will ignore them.
- Mutable aliases such as `latest` are not persisted as provenance or compatibility evidence.
- Historical result/release readers remain available for the product's supported retention window.

## Failure and degraded modes

- Invalid request or unsupported required semantics: fail closed with stable problem type.
- Conflicting idempotency replay: HTTP conflict/problem response; never overwrite prior evidence.
- Optional downstream capability unavailable: surface typed capability-specific degradation while preserving unrelated product functions.
- Event consumer cannot understand schema major version: reject/quarantine rather than partially applying unknown semantics.
- Transport retry: repeats the same immutable message/request identity; it does not regenerate mutable business content.

## Security, privacy, and tenancy

- Tenant context for state-changing APIs comes from authenticated authority, not a body default.
- Contract examples/fixtures use synthetic data and contain no real credentials or participant data.
- Client generators must not receive internal service credentials.
- Error/problem examples cannot normalize leaking raw rejected values.
- Event payloads are purpose-minimized; sensitive content is carried only when the receiving bounded context is explicitly authorized to process it.

## Migration and rollout

The first implemented HTTP transport must introduce its OpenAPI document in the same PR or an accepted prerequisite PR. The first durable event transport must do the same for AsyncAPI.

Contract changes are validated before deployment. During compatibility windows, old and new versions may be served/consumed concurrently only when the implementation has explicit routing/adapter tests.

Rollback must restore an application version that still understands any messages/resources already committed under the deployed compatible schema. A rollback that would strand newly persisted required semantics is not safe; use roll-forward or a compatibility adapter instead.

## Validation and release evidence

Release gates for an implemented transport include:

- OpenAPI/AsyncAPI parser/schema validation;
- implementation-to-contract route/message coverage;
- request/response/event fixture validation;
- RFC 9457 problem-type contract tests;
- idempotency/conflict tests;
- unsupported-version fail-closed tests;
- client/consumer compatibility tests for the supported window;
- security tests that verify examples/errors do not disclose prohibited data.

## Alternatives considered

### Prose-only API/event documentation

Rejected. It is not sufficient for generated clients, automated compatibility checks, or exact release evidence.

### Generate a complete future API now

Rejected. It would create a false as-built contract for operations that do not exist.

### GraphQL/gRPC as the mandatory product transport

Not selected at this stage. A future ADR may add one if a concrete product need exists. The domain contracts remain transport-neutral.

### Custom error envelope

Rejected as the default. RFC 9457 already defines interoperable HTTP problem semantics and avoids inventing another generic error format.

## Consequences

Positive:

- API/event behavior becomes machine-checkable and client-generator friendly.
- Contract/version drift becomes release-detectable.
- Error semantics are standardized.
- Target architecture remains distinguishable from deployed capability.

Cost:

- transport changes must update schemas and compatibility tests;
- generated client/server tooling must be pinned and maintained.

## Reversal conditions

A future major transport change may supersede this ADR if OpenAPI/AsyncAPI no longer describe the primary deployed interfaces. The principle that deployed interfaces require machine-readable, versioned, as-built contracts remains.

## References

AsyncAPI Initiative. (2026). *AsyncAPI Specification, Version 3.1.0*.

Nottingham, M., Wilde, E., & Dalal, S. (2023). *Problem Details for HTTP APIs* (RFC 9457). Internet Engineering Task Force. https://doi.org/10.17487/RFC9457

OpenAPI Initiative. (2025). *OpenAPI Specification, Version 3.2.0*.
