# As-Built PostgreSQL Schema Map

- Status: Normative evidence map
- Date: 2026-08-12
- Protected-main baseline: `1733aac738e455214891a51137a3d0bbe092414c`

This document records which portions of the logical ERD have executable PostgreSQL migrations and adapters. It does **not** promote active-PR DDL or target entities to protected-main truth. `ERD.md` remains the normative logical model; this file is the physical/as-built maturity companion required once migrations exist.

## Protected-main physical schema

Protected main contains `migrations/0001_integration_delivery.sql` with the corresponding `src/postgres_integration.rs` adapter and real PostgreSQL contract tests. That slice persists the product-owned integration outbox/inbox identity and delivery-attempt evidence. It does not establish the complete logical integration lifecycle: durable side-effect completion/consumption and crash recovery remain incomplete.

| Physical object | Logical ownership | Protected-main maturity |
|---|---|---|
| `integration_outbox` | integration | Implemented subset |
| `integration_delivery_attempt` | integration | Implemented subset |
| `integration_inbox` | integration | Implemented subset |
| `integration_consumption` | integration | Target only |

The protected-main integration identity is source- and tenant-scoped. A physical implementation must continue to preserve the stronger logical tenant/resource, replay, and crash-safety invariants in ADR-0014 and ADR-0015.

## Active PR #31 physical schema

PR #31 (`feat: persist PostgreSQL scoring-job leases`) is **IMPLEMENTED_ON_ACTIVE_PR**, not protected-main truth. Its current migration `migrations/0002_scoring_job_state.sql` maps a bounded physical subset of the logical `scoring_job` aggregate into `scoring_job_state` and the `src/postgres_scoring_job.rs` adapter.

The active slice persists:

- immutable `scoring_job_ref` / `scoring_request_ref` identity and bounded `max_attempts`;
- initial queued state;
- atomic queued-to-leased claim;
- worker and lease references;
- monotonically increasing fencing evidence tied to attempt count;
- lease expiry evidence;
- database constraints rejecting impossible lifecycle state shapes.

Real PostgreSQL tests on the active branch cover exact replay/conflicting replay, unsupported transaction isolation, fail-closed invalid evidence, shared-fixture serialization, concurrent claim fencing, and database lifecycle-shape constraints. These are review-time facts only until the unchanged head is integrated.

The active slice deliberately does **not** claim durable retry scheduling/reclaim, completion, permanent failure/quarantine transitions, expired-lease recovery, crash/restart recovery, result persistence, or live fast-mlsirm execution. Those remain Target.

## Logical-to-physical mapping rule

A logical entity is classified as physical only when all of the following exist on the named protected-main baseline:

1. a checked-in migration or equivalent durable schema definition;
2. an owning adapter/repository contract;
3. real supported-PostgreSQL tests for relevant constraints and concurrency semantics;
4. traceability that names the exact maturity without treating an active PR as shipped;
5. schema/adapter behavior consistent with accepted ADRs and the logical ERD.

A table may combine multiple logical value objects or lifecycle fields, but physical optimization cannot weaken ownership, cardinality, immutability, tenant binding, idempotency, state-shape, fencing, privacy, or cross-service database boundaries.

## Reconciliation obligations

When a physical migration merges, the same workstream must reconcile this map, `ERD.md`, `TRACEABILITY.md`, ADR-0015, migration/rollback guidance, and any UML sequence whose transactional semantics changed. A later protected-main commit, not this active PR statement, is what changes an item from `IMPLEMENTED_ON_ACTIVE_PR` to `IMPLEMENTED_ON_PROTECTED_MAIN`.
