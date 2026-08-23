# ADR-0015: Product persistence and transaction boundaries

- Status: Accepted
- Date: 2026-08-09
- Deciders: ContextualWisdomLab Psychometrics Commons maintainers
- Scope: Psychometrics Commons-owned durable state, local transactions, migration boundaries, outbox/inbox integration
- Supersedes: none
- Superseded by: none
- Current/as-built status: protected main contains PostgreSQL 18.x migrations and adapters for integration delivery/consumption, scoring jobs and requests, assessment sessions, item delivery, consent, instrument releases, response/result snapshots, and data-rights evidence; active PR #318 tightens integration-reference parity, migration atomicity, and PostgreSQL UTF8 readiness but is not protected-main truth until merged
- Target status: upstream PostgreSQL 18.x UTF8 operational persistence with real-database concurrency/crash/recovery evidence and transactional outbox/inbox semantics
- Migration status: protected main has multiple bounded product migrations; each additional or hardened persistence slice must preserve this ADR's ownership, transaction, compatibility, and evidence requirements without synthetic provenance backfills

## Context

The product must preserve session, response, consent, data-rights, result, research-contribution, and integration state durably while remaining independently deployable from Keyverse, fast-mlsirm, TEPP, semantic-data-portal, and other CWL services.

The TRD and ADR-0011 already require service-owned databases and transactional outbox/inbox integration. A focused persistence decision is needed so implementation does not drift toward shared tables, distributed transactions, ad-hoc cross-module writes, or an undefined “PostgreSQL-compatible” behavior set whose transaction/locking semantics have not been verified.

## Decision

1. The initial supported operational database engine is **upstream PostgreSQL major version 18**. Deployments must use a currently supported PostgreSQL 18.x minor release. Forks, proxies, serverless products, and “PostgreSQL-compatible” services are **not implicitly supported**; adding one requires an explicit adapter/capability decision and real conformance/crash tests proving the invariants in this ADR.
2. The product may initially use one physical database, but logical modules own their tables and invariants. Shared physical storage does not permit ad-hoc cross-module mutation.
3. A local domain mutation and its durable outbound event are committed in **one local PostgreSQL transaction** using a transactional outbox.
4. Event receipt is not equivalent to externally visible side-effect completion. Inbox processing uses durable `pending`, `processing`, `completed`, and `quarantined`/terminal-failure evidence. A local side effect and inbox completion are committed atomically when they share the same database. A non-local side effect is represented by durable local recoverable work/outbox state or is marked complete only after the external idempotency-key result and completion evidence are verified.
5. No distributed two-phase commit is used across CWL bounded contexts.
6. No service receives another service's normal application-database credentials. Cross-service state is exchanged through versioned APIs, events, or immutable artifacts.
7. Published/frozen scientific/product artifacts are append-only or superseded rather than updated in place.
8. The first integration-evidence adapter uses the scoped outbox identity `(source_ref, tenant_ref, event_ref)`. `event_ref` is not assumed globally unique across sources or tenants.
9. The first integration-evidence adapter's insert-then-inspect duplicate classifier supports PostgreSQL **READ COMMITTED** isolation only. The adapter verifies the effective transaction isolation and fails closed on `REPEATABLE READ`, `SERIALIZABLE`, or an unknown value. A future implementation may support stronger isolation only if its SQL algorithm and real concurrent replay tests prove identical duplicate-versus-conflict semantics.
10. Operational PostgreSQL databases must report `server_encoding = 'UTF8'` before state-changing readiness is declared. Product-owned Unicode reference constraints may use PostgreSQL 18's built-in `pg_unicode_fast` collation to align whitespace/control classification with the Rust reference boundary; because that collation is available only for UTF8 databases, non-UTF8 databases fail readiness closed rather than silently running a weaker persistence contract.

## Ownership and boundaries

| Responsibility | Owner | Interface | Forbidden coupling |
|---|---|---|---|
| Product operational relational state | psychometrics-commons | PostgreSQL 18.x persistence adapter + migrations | another CWL service directly querying product tables |
| Module-level persistence invariants | owning product module | repository/adaptor contracts | ad-hoc writes bypassing aggregate invariants |
| Cross-service propagation | owning producer/consumer bounded contexts | versioned API/event/artifact + local outbox/inbox | distributed 2PC or shared application DB |
| Restricted research identity linkage | psychometrics-commons restricted data boundary | separately authorized repository/role | normal assessment/reporting role access |
| Scientific numerical artifacts | fast-mlsirm | immutable upstream references/digests | copying numerical kernel ownership into product DB logic |

## Database capability contract

The first persistence adapter targets upstream PostgreSQL 18.x only and relies on documented PostgreSQL semantics for:

- ACID local transactions and MVCC;
- unique/foreign-key/check constraints needed for idempotency and tenant/resource integrity;
- row-level locking or optimistic version checks selected per aggregate;
- transaction-scoped outbox creation;
- `READ COMMITTED` statement-snapshot refresh for the first integration-evidence adapter's two-statement insert/replay-classification sequence;
- UTF8 server encoding for product-owned Unicode-aware reference validation;
- the PostgreSQL 18 built-in `pg_unicode_fast` collation where a migration explicitly requires Unicode-aware pattern classification;
- JSON/JSONB only where a reviewed schema requires it, never as an excuse to avoid versioned typed contracts;
- transactional DDL characteristics only where migrations explicitly depend on them;
- indexes and partial/expression indexes only when the migration and supported-query contract require them.

The integration-evidence adapter verifies `SHOW transaction_isolation` before a persistence effect after deterministic input validation. State-changing runtime readiness also verifies PostgreSQL major-version compatibility and `SHOW server_encoding = 'UTF8'`; a non-UTF8 database is incompatible with migrations whose reference constraints use `pg_unicode_fast` and must fail closed before product writes.

Every supported minor version is tested through the same real-database migration/concurrency/crash suite. PostgreSQL 19 or any other major version is not added merely because it exists; support requires a compatibility PR that runs the full persistence acceptance suite and updates the supported-version contract. Cloud products or forks may be added later only with an explicit capability matrix covering transaction isolation, locking, constraints, UTF8/Unicode classification, JSON semantics, migrations/DDL, indexes, backup/restore, connection/proxy behavior, and the exact features this product uses.

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
| integration | `integration_outbox`, delivery attempts, `integration_inbox`, `integration_consumption`, quarantine/reconciliation evidence |

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

A scoring worker that does not already know the next job identity claims the oldest due queued or retry-scheduled `scoring_job_state` row under `READ COMMITTED` with `FOR UPDATE SKIP LOCKED` (PostgreSQL Global Development Group, 2026a, 2026b). The claim returns the stored `scoring_request_ref` and fencing lease, or `None` when no job is due. Two concurrent pollers cannot both receive the same row. A retry-scheduled row is skipped until its persisted due time. The worker is still not invoked inside the session-completion transaction.

### Integration outbox enqueue

The first physical integration slice inserts an immutable outbox row under the composite identity `(source_ref, tenant_ref, event_ref)`. `INSERT ... ON CONFLICT DO NOTHING` is followed by an exact immutable-evidence query only when the insert did not create a row. Because a caller may supply its own transaction, this algorithm explicitly requires `READ COMMITTED`, where each statement receives a fresh command snapshot after a conflicting transaction completes. Stronger isolation is rejected rather than converting an invisible concurrent duplicate into a false `ConflictingReplay` result.

Delivery-attempt rows reference the same composite outbox identity. An identical `event_ref` from another tenant or source is therefore an independent event rather than a collision.

### Inbox consumption and external effects

On receipt, a consumer validates source/schema/digest/tenant/resource identity and creates or finds the deduplication record. Processing then follows one of two safe patterns:

1. **Local effect:** the domain change and transition of the inbox record to `completed` happen in the same PostgreSQL transaction.
2. **Non-local effect:** the consumer transaction records `processing` plus a local durable outbox/work item carrying the external operation's stable idempotency key. A worker retries that operation after crashes. The inbox becomes `completed` only after verifiable completion evidence is recorded. If the external service itself exposes a durable idempotency-key result, a retry queries/reuses that result rather than guessing whether the effect happened.

The first physical inbox deduplication slice also uses an insert-then-inspect duplicate classifier and therefore has the same explicit `READ COMMITTED` requirement as outbox enqueue.

An inbox row that merely proves receipt is never marked `completed` before the required effect is locally atomic or durably recoverable. Unknown/mismatched semantics are quarantined without applying the effect.

### Consent and data rights

Consent decisions and data-rights lifecycle events are append-only evidence. External propagation of deletion/export/research changes is asynchronous and reconciled; local state never claims an external effect completed until evidence exists.

## Concurrency and idempotency

Physical constraints must enforce equivalents of:

- unique `(session_ref, client_event_ref)`;
- unique `(session_ref, server_sequence)`;
- unique server/public event references within their documented source/tenant scope;
- one canonical frozen response snapshot per completion/supersession policy;
- unique outbox identity `(source_ref, tenant_ref, event_ref)` for the first integration persistence slice;
- delivery attempts foreign-keyed to that same scoped outbox identity;
- tenant-bound consumer deduplication identity consistent with ADR-0014;
- tenant/resource-scoped request idempotency keys;
- manifest/result content digest consistency;
- monotonic or version-checked aggregate updates where concurrent commands can race.

### Concurrency invariants

1. Two concurrent writes for the same logical idempotency key cannot create two domain effects.
2. Session completion racing a response write produces one serializable domain outcome: a response is either included before the immutable completion snapshot or rejected after collection closes; it cannot appear in an ambiguous half-state.
3. Two workers cannot both own the same processing lease/attempt without fencing or an equivalent compare-and-set guarantee.
4. A stale worker may not mark a job/inbox/outbox effect completed after a newer lease/fencing token supersedes it.
5. Isolation/locking choices must be demonstrated by real PostgreSQL tests; in-memory mutex behavior is not evidence.
6. Deadlock/serialization errors are classified as bounded retryable only when the same immutable command/idempotency identity can be retried safely.
7. A persistence function may not silently assume a caller-owned transaction uses an isolation level compatible with its SQL algorithm; supported isolation is checked or the algorithm must be isolation-independent.

The initial integration-evidence adapter is deliberately narrower than the eventual aggregate persistence layer: `READ COMMITTED` is the only accepted isolation level for its two-statement replay classifier. Other aggregates may select optimistic locking, row locking, or a stronger isolation level when their own adapter semantics and real-database tests prove the required invariants.

## Immutable payloads and large content

Routine rows store references/digests where raw sensitive payloads are unnecessary. Raw response or report payloads may reside in PostgreSQL or an approved encrypted object store.

The adapter must bind payload/reference to digest and preserve authorization, encryption, export/deletion, snapshot replay, and backup/restore semantics. Moving bytes to object storage does not change data ownership.

## Data and persistence impact

Protected main already has bounded physical product persistence across integration, scoring, session, item-delivery, consent, instrument-release, response/result-snapshot, and data-rights modules. Those migrations remain independently owned slices rather than one shared persistence aggregate. Active PR #318 changes only the integration persistence/readiness contract described above and must remain `IMPLEMENTED_ON_ACTIVE_PR` until its exact head lands. Every subsequently persisted entity must map to a named module owner, tenant scope where applicable, immutable/supersession semantics, and database constraints. A schema optimization may differ from the logical ERD layout but cannot weaken the documented cardinality, uniqueness, restricted-linkage, or transaction invariants.

## Migration policy

- Database object names contain at least two descriptive words and use `snake_case` by default.
- Public opaque references remain stable across migrations.
- Schema changes support at least one backward-compatible application deployment window unless a separately approved maintenance migration proves otherwise.
- Destructive migrations require verified backup/restore evidence and explicit roll-forward/rollback instructions.
- New immutable identity/digest fields are deterministically backfilled or the migration fails; synthetic placeholder provenance is forbidden.
- Published scientific payloads are not rewritten merely to simplify a schema transition.
- PostgreSQL major-version upgrades are operational migrations and require full persistence/concurrency/restore acceptance on the target major before support is declared.
- If a migration changes an `IMMUTABLE` validation function or Unicode/collation-dependent CHECK predicate, it must recreate or explicitly revalidate the dependent constraints so historical rows are checked against the new semantics; changing the helper alone is not accepted upgrade evidence.

Each product migration must prove its owned identities, foreign keys, tenant/source scoping, digests, bounded states, replay behavior, and reapplication semantics through real PostgreSQL tests on the exact reviewed head. A migration that tightens an existing CHECK must fail closed on incompatible historical data or provide an explicit reviewed cleanup/roll-forward procedure; it may not silently retain a weaker same-named constraint.

## Failure and degraded modes

- Transaction failure: no partial domain transition and no orphan outbox event.
- Unsupported transaction isolation for an adapter path: fail before the persistence effect with a typed safe error; do not silently downgrade the caller's transaction.
- Unsupported server encoding for a persistence path: fail state-changing readiness before product writes; do not execute Unicode-dependent migrations under a non-UTF8 database.
- Outbox publication failure: local committed resource remains valid; dispatch retries are bounded and observable.
- Duplicate message: inbox deduplication reuses existing processing/completion evidence and prevents duplicate logical effect.
- Consumer crash after receipt but before local effect: pending/processing evidence remains retryable; receipt alone does not suppress the effect.
- External effect uncertainty after network failure: retry/query through the same external idempotency identity; do not mark completed without evidence.
- Poison/invalid event: bounded attempts then quarantine with typed cause and reconciliation path.
- Cross-service outage: does not roll back already-valid local participant action.
- Digest or tenant/resource conflict during reconciliation: fail closed and require operator/scientific adjudication; last-write-wins is forbidden.
- Database major/minor/encoding incompatibility: readiness/upgrade fails closed rather than silently running an unvalidated persistence contract.

## Security, privacy, and tenancy

- Tenant context is required for tenant-scoped state and is derived from authorized context.
- Database roles enforce least privilege; normal runtime paths do not receive unrestricted access to `research_identity_linkage`.
- Cross-service credentials cannot be used to query another service's application tables.
- Routine logs and outbox metadata contain resource references/digests rather than raw sensitive assessment content.
- Backup copies retain the same classification and access obligations as primary data.
- Tenant/resource binding is validated before event consumption and included in physical uniqueness/authorization constraints where appropriate.
- Outbox `event_ref` is scoped by both `source_ref` and `tenant_ref`; one source/tenant cannot suppress another source/tenant solely by reusing the same opaque event reference.

## Deployment and operations impact

The Community, Hosted, and Enterprise profiles may package PostgreSQL differently, but the initial product persistence contract remains upstream PostgreSQL 18.x with UTF8 server encoding unless a later adapter decision expands support. `pg_unicode_fast` is PostgreSQL 18's built-in Unicode-oriented collation used by migrations that need stable Unicode pattern classification; those migrations therefore depend on the same UTF8 prerequisite checked by runtime readiness. Operators must expose database compatibility, encoding, migration status, pool health, transaction/lock timeout failures, outbox/inbox age, processing leases, quarantine, and restore readiness without exposing sensitive payloads. Connection pool settings and retry budgets must be bounded to prevent overload amplification.

The integration-evidence adapter additionally requires its caller/session configuration to use `READ COMMITTED`. A profile that forces a different isolation policy is incompatible with this adapter until an isolation-independent implementation or separately proven stronger-isolation algorithm lands. PostgreSQL major upgrades must rerun the migration/constraint acceptance suite, including Unicode reference parity and owned CHECK revalidation, before the new major can be declared supported.

## Validation and release evidence

When physical persistence exists, required evidence includes:

- clean install and migration on upstream PostgreSQL 18.x current supported minor;
- runtime proof that the supported database reports `server_encoding = 'UTF8'`, plus a negative test proving non-UTF8 readiness fails closed where Unicode-dependent persistence is required;
- migration upgrade/rollback or tested roll-forward strategy;
- real-database concurrency/idempotency tests;
- exact replay and conflicting replay tests scoped by `(source_ref, tenant_ref, event_ref)`;
- proof that identical `event_ref` values from different tenants and sources persist independently;
- contract tests proving the current integration adapter accepts `READ COMMITTED` and fails closed on `REPEATABLE READ` and `SERIALIZABLE` before the persistence effect;
- real-database parity tests for any Rust/SQL shared identity predicate, including Unicode whitespace, controls, numeric-like references, and accepted mixed opaque identifiers;
- migration reapplication tests proving changed validation helpers recreate/revalidate dependent CHECK constraints and reject incompatible historical rows;
- crash tests around transaction/outbox/inbox/worker boundaries;
- inbox pending/processing/completed/quarantine replay tests;
- external side-effect idempotency/recovery tests using a deterministic contract test service;
- cross-tenant database/API negative tests;
- immutable snapshot/result/release constraint tests;
- lock/deadlock/serialization-retry tests under bounded timeouts for adapters that actually support those retry classes;
- restore tests preserving deduplication, tenant, provenance, and restricted-linkage boundaries;
- schema-to-logical-ERD fitness validation;
- unsupported database/fork/major-version/encoding rejection tests until separately supported.

## Architecture-view impact

- `docs/architecture/ERD.md` must reflect tenant-bound outbox/inbox and processing evidence, including source/tenant-scoped event identity.
- `docs/architecture/UML.md` sequences must not imply that receipt alone completes non-local effects.
- `docs/architecture/DEPLOYMENT_AND_OPERATIONS.md` must name PostgreSQL 18.x and UTF8 as the initial validated store contract rather than an undefined compatible class.
- `docs/TRACEABILITY.md` must classify active-PR persistence separately from protected-main implementation until migrations and real-database tests land on protected main.

## Alternatives considered

### Undefined “PostgreSQL-compatible” operational store

Rejected for the initial release. Compatibility claims differ in transaction isolation, DDL/migration behavior, locking, extensions, JSON, indexing, failover, proxy, encoding, and Unicode-classification semantics. Support is earned by conformance evidence, not a wire-protocol label.

### Support every currently maintained PostgreSQL major immediately

Rejected as unnecessary pre-GA compatibility burden. The initial baseline targets PostgreSQL 18.x; additional major versions require explicit evidence and can be added when buyer/deployment needs justify them.

### Shared organization-wide database

Rejected. It bypasses bounded contexts, expands credential blast radius, and prevents independent deployment.

### Distributed two-phase commit across CWL services

Rejected. It couples independent availability and deployment domains and is unnecessary when durable outbox/inbox plus reconciliation preserve product semantics.

### Globally unique `event_ref` without source/tenant scope

Rejected. The domain contract does not guarantee global uniqueness across bounded-context sources or tenants. A single-column primary key could reject a valid event in one tenant because another source or tenant used the same opaque event reference.

### Allow stronger isolation without changing the replay classifier

Rejected. `REPEATABLE READ` and `SERIALIZABLE` retain transaction snapshots that can make the post-conflict inspection unable to observe the row that caused `ON CONFLICT DO NOTHING`. The adapter fails closed instead of reporting a deterministic duplicate/conflict result it cannot prove.

### Mark inbox completed at receipt before non-local side effect

Rejected. A crash after receipt but before the side effect could make retries suppress an effect that never happened.

### Synchronous call to downstream service inside every product transaction

Rejected. A transient dependency outage must not erase or partially commit a participant's valid local action.

### Event sourcing for every aggregate

Not selected as a universal storage pattern. Append-only evidence is required where it serves audit/scientific semantics, but full event sourcing would add complexity without a demonstrated product need.

## Consequences

Positive:

- precise database compatibility instead of an unverifiable “compatible” claim;
- tenant/source-scoped outbox identity aligned with the event-domain contract;
- fail-closed, explicit isolation and UTF8 readiness behavior;
- clear ownership and recovery boundaries;
- crash-safe cross-service propagation;
- independent service deployability;
- durable, reproducible product/scientific evidence.

Costs:

- PostgreSQL 18.x with UTF8 server encoding becomes an explicit initial operational dependency;
- the first integration adapter is intentionally limited to `READ COMMITTED` until redesigned/proven otherwise;
- Unicode-aware SQL/Rust parity and major-upgrade revalidation add compatibility-test burden;
- explicit worker/reconciliation machinery;
- duplicate/idempotency/processing state;
- migration, concurrency, crash, and restore testing burden;
- additional database services/forks/major versions require separate conformance work.

## Follow-up work

- land the active integration-reference/UTF8 hardening only after exact-head review and evidence gates pass;
- implement remaining typed repository adapters and restart/read paths from `docs/architecture/ERD.md` and the protected-main roadmap;
- add real PostgreSQL concurrency/crash tests for response completion, outbox, inbox, and worker leases;
- implement durable inbox processing states and delivery-attempt APIs consistent with ADR-0014;
- evaluate whether a single-statement replay classifier can safely remove the integration adapter's `READ COMMITTED` restriction without weakening duplicate/conflict semantics;
- add profile-specific database install/upgrade/restore runbooks;
- evaluate additional managed PostgreSQL services only when a concrete deployment need exists and record a capability/conformance matrix before claiming support.

## Traceability

- Product requirements: `docs/PRD.md` product persistence, security/privacy, and release acceptance.
- Technical requirements: `docs/TRD.md` transaction, database, integration, naming, observability, failure and release sections.
- Architecture: `ARCHITECTURE.md`, `docs/architecture/ERD.md`, `docs/architecture/DEPLOYMENT_AND_OPERATIONS.md`.
- Event integrity/consumption: ADR-0014.
- Recovery: ADR-0017.
- Delivery state: `docs/TRACEABILITY.md`, `docs/ROADMAP.md`.

## Reversal conditions

The physical database technology or decomposition may change if scale, residency, or operational evidence requires it. Any replacement must preserve logical ownership, immutable artifacts, local transaction/outbox semantics, crash-recoverable inbox/side-effect processing, tenant isolation, and no-direct-cross-service-database rules, with real conformance evidence before support is claimed. The `READ COMMITTED` restriction may be removed only when a replacement replay algorithm is proven isolation-correct by real concurrent PostgreSQL tests and the documentation/traceability contract is updated in the same change. The UTF8 requirement may be removed only if every product migration and runtime path that depends on Unicode-aware PostgreSQL classification is replaced by an equally strict, independently verified mechanism and exact cross-layer parity remains proven.

## References

PostgreSQL Global Development Group. (2026). *PostgreSQL 18 documentation*.

PostgreSQL Global Development Group. (2026). *PostgreSQL versioning policy*.

PostgreSQL Global Development Group. (2026a). *SELECT*. https://www.postgresql.org/docs/18/sql-select.html

PostgreSQL Global Development Group. (2026b). *Transaction isolation*. https://www.postgresql.org/docs/18/transaction-iso.html