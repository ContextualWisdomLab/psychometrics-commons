# ADR-0015: Product persistence and transaction boundaries

- Status: Accepted
- Date: 2026-08-09
- Scope: Psychometrics Commons-owned durable state, local transactions, migration boundaries, outbox/inbox integration
- Supersedes: none

## Context

The product must preserve session, response, consent, data-rights, result, research-contribution, and integration state durably while remaining independently deployable from Keyverse, fast-mlsirm, TEPP, semantic-data-portal, and other CWL services.

The TRD and ADR-0011 already require service-owned databases and transactional outbox/inbox integration. A focused persistence decision is needed so implementation does not drift toward shared tables, distributed transactions, or ad-hoc cross-module writes.

## Decision

1. Psychometrics Commons initially uses a **PostgreSQL-compatible operational store** for product-owned durable relational state.
2. The product may initially use one physical database, but logical modules own their tables and invariants. Shared physical storage does not permit ad-hoc cross-module mutation.
3. A local domain mutation and its durable outbound event are committed in **one local database transaction** using a transactional outbox.
4. Consumers record inbox/deduplication evidence **before** applying externally visible side effects.
5. No distributed two-phase commit is used across CWL bounded contexts.
6. No service receives another service's normal application-database credentials. Cross-service state is exchanged through versioned APIs, events, or immutable artifacts.
7. Published/frozen scientific/product artifacts are append-only or superseded rather than updated in place.

## Logical schema ownership

| Module | Logical entities |
|---|---|
| `instrument_publication` | `instrument_definition`, `instrument_version`, `instrument_item`, item/version references |
| `assessment_session` | `assessment_participant`, `assessment_session` |
| `response_event` | `response_event`, `response_snapshot`, `response_snapshot_entry` |
| `scoring_dispatch` | `scoring_job`, scoring attempt/evidence records |
| `result_snapshot` | `result_snapshot`, narrative/result-access metadata |
| `consent_record` | `consent_form`, `consent_snapshot` |
| `research_contribution` | `research_contribution`, research staging references |
| restricted identity boundary | `research_participant`, `research_identity_linkage` with separately restricted access |
| `data_rights` | `data_rights_request` and durable operation/evidence references |
| integration | `integration_outbox`, delivery attempts, `integration_inbox`, consumption evidence |

A physical migration may split an entity across tables or co-locate value objects, but it must preserve the logical ownership and invariants documented in `docs/architecture/ERD.md`.

## Transaction boundaries

### Response recording

One transaction validates the current session state, reserves server sequence, applies the idempotency/uniqueness contract, and stores the accepted response event. Two concurrent requests cannot both create the same logical `client_event_ref`.

### Session completion

One transaction:

1. validates that completion is legal;
2. freezes the exact accepted response prefix into an immutable response snapshot;
3. transitions the session to Completed;
4. writes the scoring-request outbox record.

The scoring worker is not called inside this transaction.

### Scoring completion

A scoring result is persisted with exact request/version/provenance evidence and result-snapshot creation in a local transaction. Any downstream narrative/report/release effect is represented by local durable work/outbox evidence rather than a distributed transaction.

### Consent and data rights

Consent decisions and data-rights lifecycle events are append-only evidence. External propagation of deletion/export/research changes is asynchronous and reconciled; local state never claims an external effect completed until evidence exists.

## Concurrency and idempotency

Physical constraints must enforce equivalents of:

- unique `(session_ref, client_event_ref)`;
- unique `(session_ref, server_sequence)`;
- unique server/public event references;
- one canonical frozen response snapshot per completion/supersession policy;
- unique outbox event reference;
- unique `(consumer_name, source_event_ref)` inbox identity;
- tenant/resource-scoped idempotency keys;
- manifest/result content digest consistency.

Optimistic or pessimistic locking may be selected per aggregate, but the chosen adapter must have deterministic concurrency tests. In-memory semantics alone are insufficient release evidence after persistence is introduced.

## Immutable payloads and large content

Routine rows store references/digests where raw sensitive payloads are unnecessary. Raw response or report payloads may reside in the operational database or an approved encrypted object store.

The adapter must bind payload/reference to digest and preserve authorization, encryption, export/deletion, snapshot replay, and backup/restore semantics. Moving bytes to object storage does not change data ownership.

## Migration policy

- Database object names contain at least two descriptive words and use `snake_case` by default.
- Public opaque references remain stable across migrations.
- Schema changes support at least one backward-compatible application deployment window unless a separately approved maintenance migration proves otherwise.
- Destructive migrations require verified backup/restore evidence and explicit roll-forward/rollback instructions.
- New immutable identity/digest fields are deterministically backfilled or the migration fails; synthetic placeholder provenance is forbidden.
- Published scientific payloads are not rewritten merely to simplify a schema transition.

## Failure and degraded modes

- Transaction failure: no partial domain transition and no orphan outbox event.
- Outbox publication failure: local committed resource remains valid; dispatch retries are bounded and observable.
- Duplicate message: inbox deduplication prevents duplicate side effect.
- Poison message: bounded attempts then quarantine with typed cause and reconciliation path.
- Cross-service outage: does not roll back already-valid local participant action.
- Digest conflict during reconciliation: fail closed and require operator/scientific adjudication; last-write-wins is forbidden.

## Security, privacy, and tenancy

- Tenant context is required for tenant-scoped state and is derived from authorized context.
- Database roles enforce least privilege; normal runtime paths do not receive unrestricted access to `research_identity_linkage`.
- Cross-service credentials cannot be used to query another service's application tables.
- Routine logs and outbox metadata contain resource references/digests rather than raw sensitive assessment content.
- Backup copies retain the same classification and access obligations as primary data.

## Validation and release evidence

When physical persistence exists, required evidence includes:

- migration up/down or tested roll-forward strategy;
- real-database concurrency/idempotency tests;
- crash tests around transaction/outbox boundaries;
- inbox duplicate/poison tests;
- cross-tenant database/API negative tests;
- immutable snapshot/result/release constraint tests;
- restore tests preserving deduplication, tenant, provenance, and restricted-linkage boundaries;
- schema-to-logical-ERD fitness validation.

## Alternatives considered

### Shared organization-wide database

Rejected. It bypasses bounded contexts, expands credential blast radius, and prevents independent deployment.

### Distributed two-phase commit across CWL services

Rejected. It couples independent availability and deployment domains and is unnecessary when durable outbox/inbox plus reconciliation preserve product semantics.

### Synchronous call to downstream service inside every product transaction

Rejected. A transient dependency outage must not erase or partially commit a participant's valid local action.

### Event sourcing for every aggregate

Not selected as a universal storage pattern. Append-only evidence is required where it serves audit/scientific semantics, but full event sourcing would add complexity without a demonstrated product need.

## Consequences

Positive:

- clear ownership and recovery boundaries;
- crash-safe cross-service propagation;
- independent service deployability;
- durable, reproducible product/scientific evidence.

Costs:

- explicit worker/reconciliation machinery;
- duplicate/idempotency state;
- migration and restore testing burden.

## Reversal conditions

The physical database technology or decomposition may change if scale, residency, or operational evidence requires it. Any replacement must preserve logical ownership, immutable artifacts, local transaction/outbox semantics, tenant isolation, and no-direct-cross-service-database rules.
