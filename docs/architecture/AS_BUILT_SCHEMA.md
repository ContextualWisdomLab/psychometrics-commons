# As-Built PostgreSQL Schema Map

- Status: Normative evidence map
- Date: 2026-08-20
- Protected-main baseline: `5544149ca5dc55d2bfc3402cc59c03c44830de5f`

This document records which portions of the logical ERD have executable PostgreSQL migrations and adapters. It does **not** promote active-PR DDL or target entities to protected-main truth. `ERD.md` remains the normative logical model; this file is the physical/as-built maturity companion required once migrations exist. Status terms follow `docs/TRACEABILITY.md`: **Implemented** means evidence exists on the named protected-main baseline, **Active PR** means evidence exists only on an open PR, and **Target** means required behavior not yet implemented on that baseline.

## Protected-main physical schema

Protected main contains executable PostgreSQL 18 persistence subsets for integration delivery, scoring-job state, and instrument releases. Each listed subset has an owning adapter and real PostgreSQL contract evidence on or before the named protected-main baseline. These are bounded persistence slices, not claims that the complete product lifecycle is deployed or GA-ready.

| Physical object | Logical ownership | Protected-main maturity |
|---|---|---|
| `integration_outbox` | integration | Implemented subset; exclusive delivery-lease extension is **Active PR** #60 |
| `integration_delivery_attempt` | integration | Implemented subset |
| `integration_inbox` | integration | Implemented subset |
| `scoring_job_state` | scoring | Implemented subset |
| `instrument_release` | instrument publication | Implemented subset |
| `integration_consumption` | integration | Implemented subset |
| `assessment_session` | session | **Active PR** #218 (not protected-main truth) |

The protected-main integration identity is source- and tenant-scoped. A physical implementation must continue to preserve the stronger logical tenant/resource, replay, and crash-safety invariants in ADR-0014 and ADR-0015.

## Active PR assessment-session physical schema

PR #218 (`migrations/0014_assessment_session.sql`, `migrations/0016_assessment_session_command.sql`, and `src/postgres_assessment_session.rs`) persist and load one assessment-session identity bound to a published locale-specific release, plus append-only command history. New sessions start only through `created_session_for_start` / `start_created_assessment_session` / `start_created_assessment_session_from_stored_release`. Durable start locks `instrument_release` with `SELECT … FOR UPDATE` so a stale in-memory Published object cannot insert after persist Suspend or Retire. First insert through `persist_assessment_session` takes the same lock, so a reconstituted Created aggregate cannot insert after that later persist. When that lock finds a missing or unpublished release, persist still classifies an exact stored Created row as duplicate so a concurrent retry after the first insert commits cannot turn a later Suspend or Retire into a false unpublished failure. Exact replay of an already stored start or Created row still returns the original session after a later persist Suspend or Retire. The slice is **Active PR**, not protected-main truth. It stores participant, release, version, digest, locale, current state, and creation time. Exact replay is idempotent. Rebinding any stored field or command evidence, or persisting a shorter command history than already stored, fails closed so a stale Activate-only worker cannot rewind Pause/Resume. Command persist locks the `assessment_session` header row with `SELECT … FOR UPDATE` before inserting or counting commands. Load restores created identity without asking whether the release still accepts new sessions, then replays commands so Activate/Pause/Resume survive restart. Isolation is the global opaque `session_ref` primary key; this slice does not add `tenant_ref` because the domain `AssessmentSession` aggregate does not carry tenant. Persist-backed `POST /v1/sessions` / `GET /v1/sessions/{session_ref}` (`src/session_http.rs`, `openapi/sessions.yaml`) sit on this start path. Command HTTP remains outside this slice. #205 is the unlocked-peek first-insert-seal predecessor; #209 is the weaker NotFound-allows-insert competitor; #198 is the exact start-replay predecessor; #180 is the stored-publication lock predecessor; #188 is the in-memory replay predecessor that still lacks the store lock; #153 is the in-memory-start predecessor; #164 is the unlocked stored-load predecessor; #146 is the header-lock predecessor; #129 is the sequential stale-prefix predecessor; #125 is the command-history predecessor that still rewinds on a stale shorter persist; #109 is the persist-and-load predecessor.

## Active PR outbox delivery-lease physical schema

PR #60 (`feat/outbox-delivery-lease-20260814`) extends the protected-main `integration_outbox` relation through `migrations/0013_outbox_delivery_lease.sql` and `src/postgres_integration.rs`. The extension is **Active PR**, not protected-main truth.

The active slice adds:

- nullable `lease_worker_ref` and `lease_ref` opaque ownership references;
- nullable positive `lease_fencing_token` and `lease_expires_at_unix_ms` values that are either all present or all absent for the current lease;
- `delivery_lease_generation BIGINT NOT NULL DEFAULT 0 CHECK (delivery_lease_generation >= 0)` as the persisted monotonic generation;
- `integration_outbox_fencing_generation_check`, requiring any live `lease_fencing_token` to equal the current persisted generation;
- exclusive pending-row claims, explicit expired-lease recovery, and fenced attempt recording that clears the current lease after an accepted attempt;
- database-clock authority for both worker-side lease-expiry classification and exclusive-lease recovery, while caller-supplied attempt timestamps remain immutable delivery-attempt evidence and a future caller observation cannot steal a still-live lease;
- fail-closed stale fencing before replay classification whenever a current lease exists, while exact replay after a completed attempt has cleared its lease remains idempotent.

Real PostgreSQL evidence on the active PR is carried by `tests/postgres_outbox_delivery_lease.rs`, `tests/postgres_outbox_delivery_lease_fencing_integrity.rs`, `tests/postgres_outbox_delivery_lease_authority.rs`, `tests/postgres_outbox_delivery_lease_concurrency.rs`, `tests/postgres_outbox_delivery_lease_coverage_edges.rs`, and `tests/postgres_outbox_delivery_lease_migration_isolation.rs`. These tests cover exclusive claim/recovery, monotonic fencing, invalid physical state rejection, database-authoritative expiry, rejection of a future caller timestamp against a still-live lease, stale-fence replay precedence, blocking-proven concurrent claims, schema isolation, and persistence failure paths. The slice must remain **Active PR** until the exact reviewed/check-clean head is merged and protected main is refetched.

## Active PR participant-base physical schema

PR #250 (`migrations/0030_assessment_participant.sql`, `src/postgres_participant.rs`) adds a durable anonymous-first participant base record. This slice is **Active PR**, not protected-main truth. It stores only opaque `participant_ref`, exact `tenant_ref`, and server-authoritative creation time; optional Keyverse link history remains a separate append-only identity-link concern.

The adapter requires `READ COMMITTED`, waits for a concurrent uncommitted unique-key winner, then classifies exact replay separately from conflicting tenant/time rebinding. Reload uses the exact participant-and-tenant pair and never returns raw identity-provider subject data. The physical table rejects the same numeric-like public identities as Rust `char::is_numeric` and blocks update, delete, and truncate paths that would silently rewrite or erase stable participant evidence.

Real PostgreSQL persistence, recovery, numeric-parity, and concurrency tests exercise these contracts. This slice does **not** claim participant HTTP transport, account-link history persistence, or Keyverse federation. It must remain **Active PR** until the exact reviewed/check-clean head is integrated and the protected-main baseline is refetched.

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

This protected-main subset does **not** by itself claim durable retry scheduling/reclaim, completion, permanent failure/quarantine transitions, expired-lease recovery, crash/restart recovery, result persistence, or live fast-mlsirm execution. Those capabilities require their own integrated protected-main evidence before they can be promoted here.

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
