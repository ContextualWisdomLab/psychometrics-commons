# ADR-0008: Gyeot and TEPP longitudinal boundary

- Status: Accepted
- Date: 2026-08-09
- Deciders: ContextualWisdomLab maintainers
- Scope: EMA/ESM collection, offline sync, normalized longitudinal ingestion, temporal semantics, event and multiple-membership analysis
- Supersedes: none
- Superseded by: none

## Context

Longitudinal self-understanding requires mobile momentary collection, offline operation, event-time semantics, stable observation identity, and models that distinguish within-person change from between-person differences. Duplicating collection in TEPP or temporal modeling in the product runtime would create inconsistent time semantics and atomistic analyses. Reusing one Psychometrics Commons observation identity for two different source observations would also make immutable provenance ambiguous: a caller could no longer tell which Gyeot observation an accepted Commons record represents.

The decision therefore separates collection, product-owned authorization/ingestion/persistence, and temporal analysis while keeping the identity and transaction rules at the boundary explicit.

This ADR is a mixture of protected-main as-built behavior and target behavior. Protected `main@09534ef52c9307ce0dc559e9d908ebd715c641a1` includes in-memory normalized observation ingestion plus PostgreSQL 18 persistence for immutable observation records and membership shares. Active PR #417 adds the in-memory `observation_record_ref` collision guard and aligns the PostgreSQL adapter's typed collision classification with that same immutable record-identity distinction. Enrollment persistence, participant-facing longitudinal HTTP, live Gyeot adapter execution, and TEPP dispatch remain target work.

## Decision

Gyeot owns the participant-facing EMA/ESM and JITAI collection experience, including offline-first local observations and synchronization. Psychometrics Commons owns product authorization, program enrollment, consent, normalized ingestion, immutable product observation identity, persistence, and reference orchestration. TEPP owns temporal/event/relationship, multilevel, cross-classified, and multiple-membership analytical artifacts.

The hosted runtime does not implement DSEM, continuous-time, event ontology, longitudinal ESEM, or other TEPP-owned numerical kernels. TEPP does not own participant sessions or mobile synchronization. No component reads or writes another component's normal application database.

A distinct source observation must not reuse an already accepted Psychometrics Commons `observation_record_ref`. Exact replay of the same source observation and evidence remains idempotent. A same-source replay with changed evidence remains a source idempotency conflict. A distinct source observation that attempts to reuse the Commons record identity fails closed and must not replace or mutate the first accepted record.

## Ownership and boundaries

| Responsibility | Owner | Interface | Forbidden coupling |
|---|---|---|---|
| Participant EMA/ESM/JITAI capture and offline queue | Gyeot | Versioned collection/sync contract | Gyeot writing Psychometrics Commons tables; TEPP owning client synchronization |
| Enrollment, consent, authorization, normalized ingest, Commons observation identity, immutable product persistence | Psychometrics Commons | Product domain types, PostgreSQL persistence, future versioned API/event adapter | Direct Gyeot/TEPP database access; product-side temporal-model kernels |
| Temporal, event, multilevel, cross-classified, multiple-membership analysis | TEPP | Versioned analysis-job/input-artifact contract | TEPP reading Psychometrics Commons application tables; TEPP mutating product observations |
| Public research catalog/release registration | semantic-data-portal | Immutable release-manifest registration | Direct operational database access |

Dependency direction is `Gyeot/client -> Psychometrics Commons -> TEPP` for the longitudinal product flow. Reusable scientific kernels do not depend on the hosted product runtime.

## Contract details

### Observation identity

A normalized observation carries two independent identity dimensions:

- `observation_record_ref`: the Psychometrics Commons-owned opaque public record identity;
- `(tenant_ref, enrollment_ref, source_system_ref, source_observation_ref)`: the source-observation identity used for exact replay and source-conflict classification.

Protected main persistence already enforces `observation_record_ref` as the primary key and the source tuple as a unique key. Active PR #417 aligns the in-memory aggregate and persistence classification with that durable identity language.

For active PR #417:

- exact same-source/evidence replay returns the already accepted observation;
- same source identity with changed evidence returns `LongitudinalObservationError::IdempotencyConflict` at the in-memory boundary;
- a distinct source observation that reuses an accepted `observation_record_ref` returns `LongitudinalObservationError::ObservationIdentityConflict` before aggregate mutation;
- at the PostgreSQL adapter, a distinct source observation or tenant that collides on an existing durable `observation_record_ref` returns `LongitudinalObservationPersistenceError::ObservationIdentityConflict`, while same-source replay with different evidence remains `LongitudinalObservationPersistenceError::ConflictingReplay`;
- the rejected collision does not replace the accepted observation and does not append or rewrite membership rows.

This ADR does not define a participant-facing HTTP endpoint or event schema for longitudinal ingestion because no such transport is implemented on protected main or #417. A later transport must preserve these identifiers and failure classes without inventing normalization aliases or a different idempotency scope.

### Observation time contract

Each longitudinal observation preserves distinct timestamps where applicable:

- `observed_at`: when the participant reports the state/event;
- `recorded_at`: when the client stored it;
- `received_at`: when the server accepted it;
- `available_at`: when it became analytically available;
- `valid_from` / `valid_to`: interval validity for referenced context.

The current normalized product record persists validity start/end, recorded, received, and ingested clocks plus timezone/UTC-offset evidence. Server receipt time never silently replaces observed/source time. Timezone and offset are preserved, and normalization to UTC retains original civil-time context where the source contract supplies it.

### Context and membership

Observations may reference multiple organizations, projects, relationships, or contexts. Multiple-membership weights are explicit, validated, and versioned; they are not collapsed to a single primary group merely for database convenience. Protected main persists membership shares as immutable child rows linked to the immutable observation record.

### Offline synchronization

Client observations have stable source references and content evidence. Sync is idempotent. Conflicting edits require explicit conflict/supersession handling; there is no blind last-write-wins for scientifically meaningful data.

## Data and persistence impact

Protected main owns two PostgreSQL entities in `migrations/0031_longitudinal_observation.sql`:

- `longitudinal_observation`, keyed by `observation_record_ref` and carrying tenant, enrollment, source, construct/measure, time, timezone, and clock-anomaly evidence;
- `longitudinal_membership_share`, an immutable child collection keyed by observation plus sequence, with unique membership-context references and validated weights.

The source tuple `(tenant_ref, enrollment_ref, source_system_ref, source_observation_ref)` is unique, while `observation_record_ref` is globally unique in the owned table. The persistence adapter operates inside the caller's PostgreSQL transaction. The #417 collision regression explicitly starts a transaction, attempts the conflicting persist, rolls the transaction back on the typed error, then proves the original observation remains the only observation row and its original membership remains the only membership row.

Active PR #417 does not add or change a migration. It aligns the in-memory domain identity invariant and the durable adapter's typed record-identity conflict with the already enforced primary-key/source-identity contract and adds real-PostgreSQL regression evidence.

## Invariants

1. Longitudinal participation requires separate valid consent before collection/ingestion authorization.
2. Offline storage uses platform security capabilities and excludes unnecessary identity data.
3. Accepted observation time/evidence is immutable except through an explicit audited correction or supersession design.
4. Distinct source observations cannot share one accepted Commons `observation_record_ref`.
5. Exact source replay is idempotent and cannot append duplicate observation or membership evidence.
6. Same-source/different-evidence replay fails closed as an idempotency conflict.
7. A rejected record-identity collision preserves the first accepted observation and membership vector unchanged.
8. Tenant/source identity remains part of persistence replay classification; a collision is not allowed to rebind another tenant/source tuple onto the existing record.
9. Multiple-membership and time-varying context are preserved when declared by the study design.
10. TEPP artifacts reference exact immutable input evidence and model/version provenance; Psychometrics Commons does not reinterpret TEPP numerical output as locally computed truth.
11. Within-person and between-person effects are not conflated in published longitudinal interpretation.

Current enforcement evidence includes `tests/longitudinal_observation_record_identity.rs`, `tests/postgres_longitudinal_observation_record_identity.rs`, the broader longitudinal domain/persistence tests, and database constraints/triggers in `migrations/0031_longitudinal_observation.sql`.

## Failure and degraded modes

- A source/network outage leaves collection in the Gyeot-owned bounded offline queue; it does not cause Psychometrics Commons to synthesize observations.
- A duplicate exact source replay returns existing accepted evidence rather than a second logical record.
- A same-source evidence change fails closed as idempotency conflict.
- A distinct source attempting to reuse an existing Commons record identity fails closed with the record-identity conflict class at both the in-memory and durable adapter boundaries; in-memory state is not mutated, and a persistence attempt is rolled back by the caller transaction in the regression contract.
- Existing observation and membership rows remain unchanged after the rejected durable collision.
- Clock anomalies are represented/validated rather than silently reordered.
- TEPP unavailability does not delete or mutate accepted observations. Analysis retry/degraded behavior belongs to the future TEPP dispatch contract and must preserve immutable input references.
- Corrupt durable evidence or unsupported persistence assumptions fail closed through the PostgreSQL adapter rather than returning a partial reconstructed observation.

No poison-message queue is implemented for longitudinal transport on this baseline; a future async adapter must define bounded retry/quarantine behavior before it is represented as shipped.

## Security, privacy, and tenancy

Psychometrics Commons remains the product authorization and tenant-context owner. Gyeot and TEPP do not receive direct credentials for the Psychometrics Commons application database. Operational participant identity, research identity, and public release identity remain separate namespaces.

`tenant_ref` is persisted on each longitudinal observation and participates in the durable source-identity uniqueness contract and load/persist authorization boundary. `observation_record_ref` remains an opaque, non-numeric Commons record identity and cannot be reused to rebind a different source observation. The record-identity collision error does not justify exposing source payloads, participant identity, membership details, or another tenant's existence on a future public transport.

This #417 change does not add a new trust boundary, encryption mechanism, retention period, residency claim, or audit policy. Those remain governed by the existing security/data, consent, and deployment documents and require separate evidence when implemented.

## Deployment and operations impact

The #417 correction introduces no new runtime service, listener, dependency, readiness probe, migration, backup format, or deployment profile. Existing PostgreSQL longitudinal persistence remains the operational durability boundary on protected main.

Operators should treat a durable `ObservationIdentityConflict` on `observation_record_ref` as an immutable record-identity rebinding attempt to investigate, not as permission to overwrite the accepted row. A same-source replay with changed immutable evidence remains `ConflictingReplay`; a cross-tenant reuse of the same record identity is an identity conflict. Normal mutation of longitudinal observation or membership evidence remains database-blocked by the existing immutability controls.

No SLO, RPO, or RTO value is introduced by this ADR. Repository recovery/backup acceptance remains governed by the deployment/operations architecture and existing product persistence evidence.

## Migration and rollback

There is no database migration in #417. The active change is additive domain/persistence classification plus tests. Rolling back #417 before merge means dropping that branch; protected-main persistence still retains its existing primary-key/source-identity constraints and the broader historical `ConflictingReplay` classification.

After #417 is integrated, reverting the record-identity guards/classification alone would intentionally reintroduce a divergence between in-memory and durable identity semantics and therefore should occur only if a superseding decision changes the product identity contract. Accepted immutable observation rows are not rewritten as part of rollback or roll-forward.

A future migration that changes longitudinal identity cardinality must fail closed on incompatible historical evidence rather than silently renaming or merging immutable records.

## Architecture-view impact

- `ARCHITECTURE.md`: unchanged; the bounded-context split and longitudinal ownership already describe the same decision.
- `docs/architecture/C4.md`: unchanged; no container/component or dependency direction changes.
- `docs/architecture/UML.md`: no new lifecycle or public operation is introduced by #417; current longitudinal domain semantics remain compatible.
- `docs/architecture/ERD.md`: protected-main logical/physical identity cardinality already has one immutable observation identity; #417 changes in-memory enforcement and typed persistence classification, not the schema.
- `docs/architecture/SECURITY_AND_DATA.md`: unchanged for #417 because no trust/data-flow/privacy class changes; existing tenant/privacy rules continue to apply.
- `docs/architecture/DEPLOYMENT_AND_OPERATIONS.md`: unchanged because there is no new runtime/deployment/recovery mechanism.
- `docs/TRACEABILITY.md`: active-PR status must remain clearly distinguished from protected-main truth until #417 merges.
- `docs/ROADMAP.md`: no backlog ordering change is required for this narrow invariant repair.

## Validation and release evidence

The identity correction is not accepted merely because documentation describes it. Required evidence includes:

- domain regression proving a distinct source cannot reuse an accepted Commons record identity and that the first record remains in the aggregate;
- exact source replay and same-source/different-evidence idempotency regressions;
- real PostgreSQL regression proving a durable record-identity collision returns `LongitudinalObservationPersistenceError::ObservationIdentityConflict`, the failed transaction is rolled back, and the original observation plus membership rows remain unchanged;
- tenant-scoped persistence/reload tests from the existing longitudinal persistence suite;
- the exact owned-production statement, 100% branch coverage, and all other metrics exposed by tooling, using realistic tests without meaningless exclusions;
- rustfmt, Clippy, rustdoc, Runtime CI, Security Scan, SAST, SPDX SBOM evidence, supply-chain provenance, and every then-live mandatory organization workflow on the unchanged exact head;
- zero valid unresolved review findings and qualifying independent non-author approval under live policy.

Queued, pending, skipped, cancelled, absent, stale, predecessor-head, synthetic, model-only, or administrator-bypass evidence is not passing.

The broader scientific release path additionally requires the TEPP-owned temporal/multilevel/multiple-membership validation appropriate to the intended analysis. Product-side identity integrity is necessary but is not evidence of longitudinal model validity.

## Alternatives considered

- **Psychometrics Commons implements all temporal models:** rejected because it duplicates TEPP and expands product runtime beyond orchestration. Reconsider only if TEPP ceases to be an independently reusable analysis boundary and an accepted superseding ADR demonstrates lower coupling.
- **TEPP collects mobile observations directly:** rejected because it couples modeling to participant/client lifecycle. Reconsider only if collection ownership moves with an explicit migration and independent-client use disappears.
- **One timestamp and one group per observation:** rejected because it cannot represent the intended longitudinal/multiple-membership designs.
- **Treat `observation_record_ref` as replaceable display metadata:** rejected because protected-main persistence already uses it as immutable record identity; replacement would make provenance/restart behavior ambiguous.
- **Allow record-identity collision and let PostgreSQL overwrite/update:** rejected because longitudinal evidence is immutable and database mutation is explicitly guarded.
- **Use the colliding record reference plus tenant as a composite identity:** rejected for this baseline because the persisted table already defines `observation_record_ref` as the product record primary key. Changing cardinality requires a separate data-model decision and migration.

## Consequences

Positive consequences:

- in-memory and durable observation identity semantics converge, including the typed record-identity conflict class;
- exact replay remains idempotent while genuine identity rebinding is distinguishable from same-source evidence conflict;
- a failed collision cannot silently replace observation or membership evidence;
- ownership between Gyeot, Psychometrics Commons, and TEPP remains explicit.

Costs and burdens:

- callers must mint a new Commons record reference for a distinct source observation instead of relying on normalization or overwrite behavior;
- operators must investigate typed identity conflicts rather than automatically repairing them by mutation;
- future transports must preserve the same identity/error contract.

Accepted risk:

- participant-facing longitudinal HTTP and live Gyeot/TEPP adapters are not yet implemented, so transport-level retry/quarantine/privacy behavior remains target work rather than shipped evidence.

## Follow-up work

- Psychometrics Commons: preserve this identity invariant when durable enrollment and participant-facing longitudinal ingestion transport are implemented.
- Psychometrics Commons: define machine-readable API/event contracts only when the corresponding transport becomes real; include idempotency and typed conflict mapping.
- Psychometrics Commons: keep recovery/backup tests synchronized with any future longitudinal schema or adapter expansion.
- Gyeot integration: preserve stable source observation references and immutable source evidence through the versioned sync contract.
- TEPP integration: consume immutable input snapshots/references without direct access to product tables and return versioned analysis artifacts.

## Reversal conditions

Revisit this decision only if one of the following becomes true:

- Gyeot/TEPP ownership changes through an accepted superseding architecture decision;
- the product adopts a different immutable observation-identity cardinality and a reviewed migration proves historical/replay safety;
- evidence shows the current record/source identity split cannot support required offline replay or scientifically necessary correction semantics without ambiguity.

A reversal must preserve historical provenance and may not silently rewrite accepted observations.

## Traceability

- Product intent: `docs/PRD.md` longitudinal product and product-boundary sections.
- Technical contract: `docs/TRD.md` system-of-record, public identifier, consent, tenant, and integration rules.
- Architecture map: `ARCHITECTURE.md` longitudinal boundary and bounded-context ownership.
- Protected-main persistence: `src/longitudinal_observation.rs`, `src/postgres_longitudinal_observation.rs`, `migrations/0031_longitudinal_observation.sql`.
- Active PR #417 domain identity regression: `tests/longitudinal_observation_record_identity.rs`.
- Active PR #417 durable collision/rollback/preservation regression: `tests/postgres_longitudinal_observation_record_identity.rs`.
- Canonical maturity mapping: `docs/TRACEABILITY.md`; active-PR behavior must not be promoted to protected-main implementation before merge.
