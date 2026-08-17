# As-Built PostgreSQL Schema Map

- Status: Normative evidence map
- Date: 2026-08-18
- Protected-main baseline: `46142cdbbe5dd5e900a926b70c700adf1878088a`

This document records which portions of the logical ERD have executable PostgreSQL migrations and adapters. It does **not** promote active-PR DDL or target entities to protected-main truth. `ERD.md` remains the normative logical model; this file is the physical/as-built maturity companion required once migrations exist. Status terms follow `docs/TRACEABILITY.md`: **Implemented** means evidence exists on the named protected-main baseline, **Active PR** means evidence exists only on an open PR, and **Target** means required behavior not yet implemented on that baseline.

## Protected-main physical schema

Protected main contains executable PostgreSQL 18 persistence subsets for integration delivery/consumption, scoring-job/request state, instrument publication, consent, data rights, item delivery, immutable response snapshots, and immutable result snapshots. Each listed subset has checked-in migration/adaptor evidence on or before the named protected-main baseline. These are bounded persistence slices, not claims that the complete product lifecycle is deployed or GA-ready.

| Physical object | Logical ownership | Protected-main maturity |
|---|---|---|
| `integration_outbox` | integration | Implemented subset, including the exclusive delivery-lease extension on protected main |
| `integration_delivery_attempt` | integration | Implemented subset |
| `integration_inbox` | integration | Implemented subset |
| `integration_consumption` | integration | Implemented subset |
| `scoring_job_state` | scoring | Implemented subset |
| `instrument_release` | instrument publication | Implemented subset |

The protected-main integration identity is source- and tenant-scoped. A physical implementation must continue to preserve the stronger logical tenant/resource, replay, and crash-safety invariants in ADR-0014 and ADR-0015.

Other protected-main migrations and their owning adapters remain authoritative even when this compact table does not enumerate every relation. The checked-in migration inventory on the named baseline includes `0001`, `0002`, `0003`, `0004`, `0005`, `0006`, `0007`, `0010`, `0011`, `0012`, `0013`, `0015`, `0018`, and `0019`; this map must not describe an already-merged migration as Active PR work.

## Active PR participant-base physical schema

PR #250 (`automation/participant-base-reconcile-20260818`) adds `migrations/0030_assessment_participant.sql` and `src/postgres_participant.rs` for the stable anonymous-first participant base record. This slice is **Active PR**, not protected-main truth. It stores only the opaque `participant_ref`, exact `tenant_ref`, and server-authoritative creation time; optional Keyverse link history remains a separate append-only identity-link concern.

The adapter requires `READ COMMITTED`, classifies exact replay separately from conflicting tenant/time rebinding, and reloads only through the exact participant-and-tenant pair. The physical table rejects non-canonical public identities and database mutation paths that would silently rewrite or erase stable participant evidence. Real PostgreSQL persistence and recovery tests exercise replay, cross-tenant absence, physical immutability, restart reconstruction, and safe error contracts. This slice does **not** claim participant HTTP transport, account-link history persistence, or Keyverse federation.

## Protected-main outbox delivery-lease physical schema

`migrations/0013_outbox_delivery_lease.sql` and the corresponding `src/postgres_integration.rs` lease paths are present on the named protected-main baseline. Historical PR #60 is closed and is not current Active PR evidence; the implementation reached protected main through later integrated history.

The protected-main lease slice adds:

- nullable `lease_worker_ref` and `lease_ref` opaque ownership references;
- nullable positive `lease_fencing_token` and `lease_expires_at_unix_ms` values that are either all present or all absent for the current lease;
- `delivery_lease_generation BIGINT NOT NULL DEFAULT 0 CHECK (delivery_lease_generation >= 0)` as the persisted monotonic generation;
- an integrity rule requiring any live lease fencing token to equal the current persisted generation;
- exclusive pending-row claims, explicit expired-lease recovery, and fenced attempt recording that clears the current lease after an accepted attempt;
- database-clock authority for lease-expiry classification and recovery, while caller-supplied attempt timestamps remain immutable delivery-attempt evidence;
- fail-closed stale fencing before replay classification whenever a current lease exists, while exact replay after a completed attempt has cleared its lease remains idempotent.

These semantics are **Implemented subset** truth on the named protected-main baseline. They do not by themselves prove live downstream side-effect execution, deployment SLOs, or GA recovery evidence.

## Protected-main inbox-consumption physical schema

`migrations/0012_integration_consumption.sql` and `src/postgres_inbox_consumption.rs` persist one consumption work item for an existing `integration_inbox` receipt. This is an **Implemented subset** on protected main after #58. It stores pending/processing/completed/quarantined evidence, a monotonically increasing fencing token, a time-bounded processing claim, a durable `side_effect_ref`, and optional completion or quarantine evidence. Receipt-only inbox rows remain uncompleted. A processing claim cannot be stolen by another worker. Expire-and-reclaim returns an expired claim to pending without transferring the crashed worker's fence. Subsequent claim-deadline columns from later inbox-expiry slices remain documented with those slices.

## Protected-main scoring-job physical schema

`migrations/0002_scoring_job_state.sql` maps a bounded physical subset of the logical `scoring_job` aggregate into `scoring_job_state`, owned by `src/postgres_scoring_job.rs`. This is an **Implemented subset** on the named protected-main baseline.

The protected-main slice persists:

- immutable `scoring_job_ref` / `scoring_request_ref` identity and bounded `max_attempts`;
- initial queued state and atomic queued-to-leased claim;
- worker and lease references;
- monotonically increasing fencing evidence tied to attempt count;
- lease-expiry fields and database constraints rejecting impossible lifecycle state shapes.

Migration reapplication does not trust relation existence or constraint names alone as schema evidence. On initial creation, migration `0002` validates the ordered column/type/nullability contract, expected defaults, the complete contract-relevant PostgreSQL constraint inventory (CHECK, PRIMARY KEY, UNIQUE, FOREIGN KEY, EXCLUDE, and PostgreSQL 18 NOT NULL entries, including validation/enforcement state), and a live invalid-state probe, then records the PostgreSQL-normalized `name:definition` constraint manifest on the owned relation. Reapplication recomputes that inventory and normalized manifest and compares them with the creation-time evidence. Incompatible pre-existing relations, missing manifest evidence, renamed/removed or unexpected constraints, non-validated/non-enforced constraints, and same-name weakened constraint definitions therefore fail closed rather than being accepted as successful migration state.

Protected-main PostgreSQL tests cover exact replay/conflicting replay, enqueue and claim isolation contracts, fail-closed invalid evidence, per-test-suite schema isolation, concurrent claim fencing, exact-shape migration reapplication, incompatible-schema rejection, same-name constraint-definition weakening, unexpected CHECK/UNIQUE/FOREIGN KEY/EXCLUDE/NOT NULL constraint rejection, database lifecycle-shape constraints, database error propagation, and stable non-sensitive error/source contracts.

This protected-main subset does **not** by itself claim live fast-mlsirm execution or deployed profile recovery evidence. Those capabilities require their own integrated protected-main evidence before they can be promoted here.

## Protected-main instrument-release physical schema

`migrations/0006_instrument_release.sql` maps one locale-specific `instrument_release` publication identity into a PostgreSQL 18 relation owned by `src/postgres_instrument_release.rs`. This is an **Implemented subset** on the named protected-main baseline.

The protected-main slice persists:

- opaque `release_ref` publication identity;
- immutable locale-specific manifest columns, including item-version order, consent-requirement references, optional norm reference, and canonical SHA-256 content digest;
- current `publication_state` with CHECK-constrained Draft/Review/Published/Suspended/Retired values;
- exact-replay classification under `READ COMMITTED`;
- reachable publication-state advance without rewriting immutable manifest columns;
- fail-closed digest/identity rebinding and unreachable lifecycle rewind.

The slice does **not** by itself claim complete publication-event history transport, a deployed administration API, or real instrument rights/scientific evidence. Those remain separately evidence-gated.

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
