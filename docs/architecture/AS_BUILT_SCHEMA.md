# As-Built PostgreSQL Schema Map

- Status: Normative evidence map
- Date: 2026-08-12
- Protected-main baseline: `feb34f2d9e497b0b25cf128b9df222b844ae8b09`

This document records which portions of the logical ERD have executable PostgreSQL migrations and adapters. It does **not** promote active-PR DDL or target entities to protected-main truth. `ERD.md` remains the normative logical model; this file is the physical/as-built maturity companion required once migrations exist. Status terms follow `docs/TRACEABILITY.md`: **Implemented** means evidence exists on the named protected-main baseline, **Active PR** means evidence exists only on an open PR, and **Target** means required behavior not yet implemented on that baseline.

## Protected-main physical schema

Protected main contains `migrations/0001_integration_delivery.sql` with the corresponding `src/postgres_integration.rs` adapter and real PostgreSQL contract tests. That slice persists the product-owned integration outbox/inbox identity and delivery-attempt evidence. It does not establish the complete logical integration lifecycle: durable side-effect completion/consumption and crash recovery remain incomplete.

| Physical object | Logical ownership | Protected-main maturity |
|---|---|---|
| `integration_outbox` | integration | Implemented subset |
| `integration_delivery_attempt` | integration | Implemented subset |
| `integration_inbox` | integration | Implemented subset |
| `integration_consumption` | integration | **Active PR** #58 (not protected-main truth) |

The protected-main integration identity is source- and tenant-scoped. A physical implementation must continue to preserve the stronger logical tenant/resource, replay, and crash-safety invariants in ADR-0014 and ADR-0015.

## Active PR inbox-consumption physical schema

PR #58 (`feat/inbox-consumption-persistence-20260814`) `migrations/0012_integration_consumption.sql` and `src/postgres_inbox_consumption.rs` adapter persist one consumption work item for an existing `integration_inbox` receipt. The slice is **Active PR**, not protected-main truth. It stores pending/processing/completed/quarantined evidence, a monotonically increasing fencing token, a durable `side_effect_ref`, and optional completion or quarantine evidence. Receipt-only inbox rows remain uncompleted. A processing claim cannot be stolen by another worker. Expire-and-reclaim of a crashed processing lease remains Target.

## Active PR #31 physical schema

PR #31 (`feat: persist PostgreSQL scoring-job leases`) is **Active PR**, not protected-main truth. Its current migration `migrations/0002_scoring_job_state.sql` maps a bounded physical subset of the logical `scoring_job` aggregate into `scoring_job_state` and the `src/postgres_scoring_job.rs` adapter.

The active slice persists:

- immutable `scoring_job_ref` / `scoring_request_ref` identity and bounded `max_attempts`;
- initial queued state;
- atomic queued-to-leased claim;
- worker and lease references;
- monotonically increasing fencing evidence tied to attempt count;
- lease expiry evidence;
- database constraints rejecting impossible lifecycle state shapes.

Migration reapplication does not trust relation existence or constraint names alone as schema evidence. On initial creation, migration `0002` validates the ordered column/type/nullability contract, expected defaults, the complete contract-relevant PostgreSQL constraint inventory (CHECK, PRIMARY KEY, UNIQUE, FOREIGN KEY, EXCLUDE, and PostgreSQL 18 NOT NULL entries, including validation/enforcement state), and a live invalid-state probe, then records the PostgreSQL-normalized `name:definition` constraint manifest on the owned relation. Reapplication recomputes that inventory and normalized manifest and compares them with the creation-time evidence. Incompatible pre-existing relations, missing manifest evidence, renamed/removed or unexpected constraints, non-validated/non-enforced constraints, and same-name weakened constraint definitions therefore fail closed rather than being accepted as successful migration state.

Real PostgreSQL tests on the active branch cover exact replay/conflicting replay, enqueue and claim isolation contracts, fail-closed invalid evidence, per-test-suite schema isolation, concurrent claim fencing, exact-shape migration reapplication, incompatible-schema rejection, same-name constraint-definition weakening, unexpected CHECK/UNIQUE/FOREIGN KEY/EXCLUDE/NOT NULL constraint rejection, database lifecycle-shape constraints, first- and second-statement database error propagation, and stable non-sensitive error/source contracts. These are review-time facts only until the unchanged head is integrated.

The active slice deliberately does **not** claim durable retry scheduling/reclaim, completion, permanent failure/quarantine transitions, expired-lease recovery, crash/restart recovery, result persistence, or live fast-mlsirm execution. Those remain Target.

## Active PR instrument-release physical schema

PR #50 (`feat/instrument-release-persistence-20260813`) is **Active PR**, not protected-main truth. Migration `migrations/0006_instrument_release.sql` maps one locale-specific `instrument_release` publication identity into a single PostgreSQL 18 relation owned by `src/postgres_instrument_release.rs`.

The active slice persists:

- opaque `release_ref` publication identity;
- immutable locale-specific manifest columns, including item-version order, consent-requirement references, optional norm reference, and canonical SHA-256 content digest;
- current `publication_state` with CHECK-constrained Draft/Review/Published/Suspended/Retired values;
- exact-replay classification under `READ COMMITTED`;
- reachable publication-state advance without rewriting immutable manifest columns;
- fail-closed digest/identity rebinding and unreachable lifecycle rewind.

The slice does **not** persist publication-event history, bound scientific evidence records, HTTP publication transport, or session-creation integration. Those remain Target.

## Logical-to-physical mapping rule

A logical entity is classified as physical only when all of the following exist on the named protected-main baseline:

1. a checked-in migration or equivalent durable schema definition;
2. an owning adapter/repository contract;
3. real supported-PostgreSQL tests for relevant constraints and concurrency semantics;
4. traceability that names the exact maturity without treating an active PR as shipped;
5. schema/adapter behavior consistent with accepted ADRs and the logical ERD.

A table may combine multiple logical value objects or lifecycle fields, but physical optimization cannot weaken ownership, cardinality, immutability, tenant binding, idempotency, state-shape, fencing, privacy, or cross-service database boundaries.

## Reconciliation obligations

When a physical migration merges, the same workstream must reconcile this map, `ERD.md`, `TRACEABILITY.md`, ADR-0015, migration/rollback guidance, and any UML sequence whose transactional semantics changed. A later protected-main commit, not this active PR statement, changes an item from **Active PR** to **Implemented** after evidence exists on the named protected-main baseline.
