# As-Built PostgreSQL Schema Map

- Status: Normative evidence map
- Date: 2026-08-17
- Protected-main baseline: `22dc8ed9534b34ebc027a23db895a2726ca8a411`

This document records which portions of the logical ERD have executable PostgreSQL migrations and adapters. It does **not** promote active-PR DDL or target entities to protected-main truth. `ERD.md` remains the normative logical model; this file is the physical/as-built maturity companion required once migrations exist. Status terms follow `docs/TRACEABILITY.md`: **Implemented** means evidence exists on the named protected-main baseline, **Active PR** means evidence exists only on an open PR, and **Target** means required behavior not yet implemented on that baseline.

## Protected-main physical schema

Protected main contains executable PostgreSQL 18 persistence subsets for integration delivery, scoring-job state, instrument releases, consent, item-delivery evidence, response snapshots, result snapshots, scoring requests, inbox consumption, and data-rights verification/processing-start/propagation. Each listed subset has an owning adapter and real PostgreSQL contract evidence on or before the named protected-main baseline. These are bounded persistence slices, not claims that the complete product lifecycle is deployed or GA-ready.

| Physical object | Logical ownership | Protected-main maturity |
|---|---|---|
| `integration_outbox` | integration | Implemented subset |
| `integration_delivery_attempt` | integration | Implemented subset |
| `integration_inbox` | integration | Implemented subset |
| `integration_consumption` | integration | Implemented subset |
| `scoring_job_state` | scoring | Implemented subset |
| `scoring_request` | scoring | Implemented subset |
| `instrument_release` | instrument publication | Implemented subset |
| `consent_ledger` / `consent_event` | consent | Implemented subset |
| `item_delivery_ledger` / `item_delivery_event` | item delivery | Implemented subset |
| `response_snapshot` / `response_snapshot_entry` | response | Implemented subset |
| `result_snapshot` / `result_snapshot_observation` | result | Implemented subset |
| `data_rights_request_state` | data rights | Implemented subset through processing-start |
| `data_rights_propagation_state` | data rights propagation | Implemented subset |

The protected-main integration identity is source- and tenant-scoped. A physical implementation must continue to preserve the stronger logical tenant/resource, replay, and crash-safety invariants in ADR-0014 and ADR-0015.

## Protected-main inbox-consumption physical schema

`migrations/0012_integration_consumption.sql` and `src/postgres_inbox_consumption.rs` persist one consumption work item for an existing `integration_inbox` receipt. `migrations/0019_inbox_claim_expiry_guard.sql` rejects terminal writes after the database-authoritative claim deadline. The slice stores pending/processing/completed/quarantined evidence, a monotonically increasing fencing token, a time-bounded processing claim, a durable `side_effect_ref`, and optional completion or quarantine evidence. Receipt-only inbox rows remain uncompleted. A processing claim cannot be stolen by another worker. Expire-and-reclaim returns an expired claim to pending without transferring the crashed worker's fence.

## Protected-main scoring-job physical schema

`migrations/0002_scoring_job_state.sql` maps a bounded physical subset of the logical `scoring_job` aggregate into `scoring_job_state`, owned by `src/postgres_scoring_job.rs`. This is an **Implemented subset** on the named protected-main baseline.

The protected-main slice persists:

- immutable `scoring_job_ref` / `scoring_request_ref` identity and bounded `max_attempts`;
- initial queued state and atomic queued-to-leased claim;
- worker and lease references;
- monotonically increasing fencing evidence tied to attempt count;
- lease-expiry fields and database constraints rejecting impossible lifecycle state shapes;
- retry, cancellation, terminal outcomes, and expired-lease recovery without transferring a fence.

Migration reapplication does not trust relation existence or constraint names alone as schema evidence. On initial creation, migration `0002` validates the ordered column/type/nullability contract, expected defaults, the complete contract-relevant PostgreSQL constraint inventory (CHECK, PRIMARY KEY, UNIQUE, FOREIGN KEY, EXCLUDE, and PostgreSQL 18 NOT NULL entries, including validation/enforcement state), and a live invalid-state probe, then records the PostgreSQL-normalized `name:definition` constraint manifest on the owned relation. Reapplication recomputes that inventory and normalized manifest and compares them with the creation-time evidence. Incompatible pre-existing relations, missing manifest evidence, renamed/removed or unexpected constraints, non-validated/non-enforced constraints, and same-name weakened constraint definitions therefore fail closed rather than being accepted as successful migration state.

This protected-main subset does **not** by itself claim live fast-mlsirm execution. That capability requires its own integrated protected-main evidence before it can be promoted here.

## Protected-main instrument-release physical schema

`migrations/0006_instrument_release.sql` maps one locale-specific `instrument_release` publication identity into a PostgreSQL 18 relation owned by `src/postgres_instrument_release.rs`. This is an **Implemented subset** on the named protected-main baseline.

The protected-main slice persists:

- opaque `release_ref` publication identity;
- immutable locale-specific manifest columns, including item-version order, consent-requirement references, optional norm reference, and canonical SHA-256 content digest;
- current `publication_state` with CHECK-constrained Draft/Review/Published/Suspended/Retired values;
- exact-replay classification under `READ COMMITTED`;
- reachable publication-state advance without rewriting immutable manifest columns;
- fail-closed digest/identity rebinding and unreachable lifecycle rewind.

The slice does **not** persist publication-event history, bound scientific evidence records, HTTP publication transport, or session-creation integration. Those remain Target unless separately evidenced on protected main.

## Logical-to-physical mapping rule

A logical entity is classified as physical only when all of the following exist on the named protected-main baseline:

1. a checked-in migration or equivalent durable schema definition;
2. an owning adapter/repository contract;
3. real supported-PostgreSQL tests for relevant constraints and concurrency semantics;
4. traceability that names the exact maturity without treating an active PR as shipped;
5. schema/adapter behavior consistent with accepted ADRs and the logical ERD.

A table may combine multiple logical value objects or lifecycle fields, but physical optimization cannot weaken ownership, cardinality, immutability, tenant binding, idempotency, state-shape, fencing, privacy, or cross-service database boundaries.

## Reconciliation obligations

When a physical migration merges, the same workstream must reconcile this map, `ERD.md`, `TRACEABILITY.md`, ADR-0015, migration/rollback guidance, and any UML sequence whose transactional semantics changed. Only protected-main evidence on the named baseline changes an item from **Active PR** to **Implemented**; an active PR statement never changes shipped maturity by itself.
