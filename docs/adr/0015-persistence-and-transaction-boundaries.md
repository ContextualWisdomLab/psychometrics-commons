# ADR-0015: Product persistence and transaction boundaries

- Status: Accepted
- Date: 2026-08-09
- Last reconciled: 2026-08-15
- Deciders: ContextualWisdomLab Psychometrics Commons maintainers
- Scope: Psychometrics Commons-owned durable state, local transactions, migration boundaries, outbox/inbox integration
- Supersedes: none
- Superseded by: none
- Evaluated protected-main baseline: `cc5850a0d1eacbbf16d03075534fce460a8286e6`
- Current/as-built status: **PARTIAL** — protected main contains bounded PostgreSQL 18 persistence for integration delivery/consumption, scoring jobs and requests, data-rights request/propagation plus identity verification, purpose-specific consent, immutable instrument release evidence, and immutable completed response snapshots. Other logical aggregates and transaction compositions remain incomplete.
- Target status: upstream PostgreSQL 18.x operational persistence with real-database concurrency/crash/recovery evidence and transactional outbox/inbox semantics across every persisted product aggregate.

## Context

Psychometrics Commons must preserve product-owned session, response, consent, data-rights, scoring-dispatch, result, research-contribution, and integration state durably while remaining independently deployable from Keyverse, fast-mlsirm, Gyeot, TEPP, semantic-data-portal, and other CWL services.

The TRD and ADR-0011 require service-owned data and transactional outbox/inbox integration. Persistence therefore cannot drift toward shared application databases, distributed transactions, ad-hoc cross-module writes, or an undefined “PostgreSQL-compatible” behavior class whose transaction and locking semantics are untested.

## Decision

1. The initial supported operational relational engine is **upstream PostgreSQL major version 18**. A deployment uses a currently supported 18.x minor release. Forks, proxies, serverless products, or wire-compatible services are not implicitly supported; each requires an explicit compatibility decision and real conformance/recovery evidence.
2. One physical database may host multiple product modules, but logical module ownership remains authoritative. Shared storage does not authorize bypassing aggregate invariants.
3. A local domain mutation and its durable outbound event are committed in **one local PostgreSQL transaction** when both belong to this service.
4. Event receipt is not side-effect completion. Inbox processing preserves durable pending/processing/completed/quarantined or equivalent terminal evidence. A non-local effect is complete only after verifiable external idempotency/completion evidence is recorded.
5. No distributed two-phase commit is used across CWL bounded contexts.
6. No service receives another service's normal application-database credentials. Cross-service state moves through versioned APIs, events, or immutable artifacts.
7. Published/frozen scientific and product artifacts are append-only or superseded rather than rewritten in place.
8. Public product references are opaque and non-numeric; durable replay classification is scoped by the documented tenant/resource identity rather than guessed global uniqueness.
9. Persistence adapters must either prove their transaction-isolation algorithm or fail closed on unsupported isolation. Existing insert-then-inspect adapters explicitly require **READ COMMITTED** where statement-snapshot refresh is part of the replay classifier.
10. A persistence replay is classified from immutable command/resource evidence. Mutable later lifecycle state must not silently convert an otherwise exact immutable creation replay into a conflict unless the operation's contract explicitly defines that lifecycle state as part of replay identity.

## Protected-main as-built persistence

On protected main `cc5850a0d1eacbbf16d03075534fce460a8286e6`, the physical migration set is exactly:

```text
migrations/0001_integration_delivery.sql
migrations/0002_scoring_job_state.sql
migrations/0003_data_rights_propagation.sql
migrations/0005_consent_lifecycle.sql
migrations/0006_instrument_release.sql
migrations/0010_response_snapshot.sql
migrations/0011_scoring_request.sql
migrations/0012_integration_consumption.sql
migrations/0015_data_rights_identity_verification.sql
```

The matching protected-main adapter surface includes:

```text
src/postgres_integration.rs
src/postgres_scoring_job.rs
src/postgres_data_rights.rs
src/postgres_consent.rs
src/postgres_instrument_release.rs
src/postgres_response_snapshot.rs
src/postgres_scoring_request.rs
src/postgres_health.rs
```

This list is an as-built inventory, not a claim that the full logical ERD or hosted product lifecycle is already persisted. Item-delivery ledgers, response-event ledgers, created-session persistence, result persistence, identity-link history, research-contribution/release evidence, later data-rights processing/completion, recovery fixtures, and broader atomic transaction compositions may exist on active PRs but are not protected-main truth until integrated.

## Ownership and boundaries

| Responsibility | Owner | Durable interface | Forbidden coupling |
|---|---|---|---|
| Product operational relational state | psychometrics-commons | PostgreSQL 18.x adapters + repository migrations | another CWL service directly querying product tables |
| Module-level persistence invariants | owning product module | typed repository/adapter contracts | ad-hoc SQL bypassing aggregate invariants |
| Cross-service propagation | producer/consumer bounded contexts | versioned API/event/artifact + local outbox/inbox | distributed 2PC or shared application DB |
| Restricted research identity linkage | psychometrics-commons restricted boundary | separately authorized product repository/role | normal assessment/reporting role access |
| Scientific numerical artifacts | fast-mlsirm | immutable upstream references/digests | copying psychometric numerical kernels into product persistence logic |
| Public research catalog/release registration | semantic-data-portal | immutable handoff/registration contract | Commons directly owning the public catalog |

## Database capability contract

Adapters may rely only on PostgreSQL behavior covered by repository acceptance evidence, including where used:

- ACID local transactions and MVCC;
- unique, foreign-key, and check constraints for idempotency and tenant/resource integrity;
- row locking or explicit compare-and-set/fencing selected by the owning aggregate;
- transaction-scoped outbox creation;
- `READ COMMITTED` statement-snapshot refresh for insert-then-inspect replay classifiers that require it;
- bounded, typed JSON/JSONB only where a versioned schema requires it;
- migration/DDL behavior actually exercised by the migration chain; and
- indexes required by a reviewed query/invariant contract.

An adapter that depends on effective transaction isolation checks `SHOW transaction_isolation` before its persistence effect. Stronger isolation is not silently downgraded. Support for PostgreSQL 19, a fork, proxy, or hosted compatibility layer requires an explicit compatibility PR and the complete persistence/concurrency/recovery suite.

## Logical ownership

The logical ERD remains authoritative for conceptual ownership even when one entity is physically split or co-located:

| Module | Representative logical entities |
|---|---|
| instrument publication | instrument definition/version/item/release evidence |
| assessment session | participant/session/item-delivery lifecycle |
| response | response event, immutable response snapshot/entries |
| scoring dispatch | scoring request/job/attempt/evidence |
| result | immutable result snapshot + presentation provenance |
| consent | consent form/snapshot/change evidence |
| research contribution | contribution, withdrawal, restricted staging references |
| restricted identity | research participant/linkage with separately restricted access |
| data rights | request, verification, operation/completion/retention evidence |
| integration | outbox, delivery attempt/lease, inbox/consumption/quarantine evidence |

A physical optimization may differ from the conceptual table layout only when it preserves ownership, cardinality, immutability, tenant/resource binding, and transaction invariants documented in `docs/architecture/ERD.md` and `docs/architecture/AS_BUILT_SCHEMA.md`.

## Transaction boundaries

### Response recording and completion

Response acceptance must serialize server sequencing and idempotency evidence for one session. Completion freezes one exact accepted response prefix. The durable completed response snapshot already exists on protected main; a future hosted transaction that also transitions session state and dispatches scoring must commit the required local business evidence atomically rather than leave an accepted snapshot without recoverable scoring work.

### Scoring dispatch and completion

Scoring requests/jobs are product-owned durable evidence, while fast-mlsirm owns scientific numerics. Dispatch, retry, fencing, result acceptance, and downstream outbox publication must preserve exact request/version/provenance identity. A stale worker cannot complete a newer attempt. Open transaction-composition work is not treated as protected-main implementation until merged.

### Integration outbox and inbox

The outbox identity is scoped by its documented source/tenant/event identity. Exact replay reuses identical immutable evidence; conflicting replay fails closed. Inbox receipt is distinct from effect completion. Local effects and inbox completion share one transaction; non-local effects require durable recoverable work and an external idempotency/completion contract.

### Consent and data rights

Consent decisions and data-rights lifecycle evidence are append-only or monotonic. Purpose-specific research consent remains separate from service use. Data-rights propagation is asynchronous and reconciled; local state never claims that another bounded context completed deletion/export merely because a local event was enqueued or received.

### Research handoff

Restricted operational/research linkage remains inside the separately authorized Commons research boundary. Public release/catalog registration belongs to semantic-data-portal. No public artifact may contain operational participant identifiers merely because the data shares one physical PostgreSQL deployment.

## Concurrency and idempotency invariants

1. Two concurrent commands for one logical idempotency identity cannot create two logical effects.
2. Exact immutable replay is idempotent; any changed immutable tenant/resource/digest/version/evidence binding fails closed.
3. Session completion racing response acceptance yields one unambiguous serial order.
4. Two workers cannot both own the same processing attempt without fencing or an equivalent compare-and-set guarantee.
5. A stale worker cannot mark work complete after a newer fence supersedes it.
6. Locking/isolation claims require real PostgreSQL tests; in-memory synchronization is not evidence.
7. Deadlock/serialization retries are bounded and safe only when the immutable command identity can be retried exactly.
8. An adapter never assumes a caller-owned transaction uses compatible isolation without checking or using an isolation-independent algorithm.
9. Mutable lifecycle timestamps/states are not retroactively part of an immutable creation command identity unless the operation contract explicitly says so.

## Migration policy

- Database objects use descriptive two-or-more-word `snake_case` names by default.
- Public opaque references remain stable across migrations.
- New immutable identity/digest/provenance fields are deterministically derived or the migration fails; synthetic placeholder provenance is forbidden.
- Published scientific payloads are not rewritten to simplify a migration.
- Destructive migrations require verified backup/restore evidence plus an explicit recovery path.
- PostgreSQL major upgrades are operational migrations and require the complete acceptance suite before support is declared.
- Physical migrations and `AS_BUILT_SCHEMA.md` are reconciled in the same workstream.

## Failure and degraded modes

- Transaction failure leaves no partial local transition/orphan required outbox effect.
- Unsupported isolation fails before the persistence effect with a typed safe error.
- Outbox publication failure does not invalidate an already-committed local resource; durable delivery remains retryable and observable.
- Duplicate message receipt reuses existing processing/completion evidence and cannot duplicate the logical effect.
- Crash after receipt but before effect leaves durable recoverable work.
- External effect uncertainty is retried/queried through the same external idempotency identity; completion is never guessed.
- Invalid/poison evidence is bounded and quarantined or rejected with a typed cause.
- Cross-service outage cannot force a valid independent participant action to use another service's database.
- Digest/tenant/resource conflicts fail closed; last-write-wins is forbidden.
- Database compatibility failure blocks readiness rather than silently running an unvalidated contract.

## Security, privacy, and tenancy

- Tenant context is required for tenant-scoped durable state and comes from authorized product context.
- Runtime roles apply least privilege; restricted research linkage is not visible to ordinary assessment/reporting roles.
- Cross-service credentials cannot query another service's application tables.
- Routine logs/outbox metadata prefer opaque references and digests over raw sensitive assessment payloads.
- Backup copies retain the same classification/access obligations as primary data.
- Public research release never inherits operational identity simply because the restricted linkage exists in Commons.

## Validation and release evidence

A release claiming production persistence must provide, on one exact protected head as applicable:

- clean migration chain on supported PostgreSQL 18.x;
- real-database exact/conflicting replay and concurrency tests;
- cross-tenant negative tests;
- transaction/outbox/inbox crash and recovery tests;
- worker lease/fencing tests;
- immutable snapshot/result/release constraint tests;
- backup/restore evidence preserving deduplication, tenant, provenance, and restricted-data boundaries;
- schema-to-logical-ERD and AS_BUILT_SCHEMA reconciliation;
- unsupported database/isolation rejection tests; and
- deployment-profile migration/rollback or roll-forward evidence.

Architecture documents, active PRs, or an isolated passing unit test do not constitute GA evidence by themselves. `docs/TRACEABILITY.md` records the exact protected-main maturity baseline.

## Consequences

### Positive

- Product ownership remains explicit without shared-database coupling.
- Crash-safe local transactions and transactional outbox/inbox provide defensible recovery semantics.
- Immutable provenance and exact replay semantics support audit, privacy, acquisition diligence, and scientific reproducibility.
- Compatibility claims become evidence-based rather than protocol-label based.

### Costs

- PostgreSQL 18 is initially a deliberately narrow support target.
- Real concurrency/recovery testing is mandatory and more expensive than in-memory tests.
- Additional transaction-composition work is required before the hosted lifecycle can be called complete.
- Restricted research linkage requires separate authorization/operational controls even if physically co-located.

## References

- `docs/TRD.md`
- `ARCHITECTURE.md`
- `docs/architecture/ERD.md`
- `docs/architecture/AS_BUILT_SCHEMA.md`
- `docs/architecture/UML.md`
- `docs/architecture/DEPLOYMENT_AND_OPERATIONS.md`
- `docs/TRACEABILITY.md`
- ADR-0001, ADR-0003, ADR-0004, ADR-0005, ADR-0006, ADR-0010, ADR-0011, ADR-0014, ADR-0017