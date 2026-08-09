# ADR-0017: Operational recovery and GA evidence contract

- Status: Accepted
- Date: 2026-08-09
- Deciders: ContextualWisdomLab Psychometrics Commons maintainers
- Scope: availability evidence, SLO/RPO/RTO governance, backup/restore, disaster recovery, incident runbooks, GA release acceptance
- Supersedes: none
- Superseded by: none
- Current/as-built status: protected main contains domain lifecycle/provenance primitives but no deployed Hosted/Enterprise profile, physical product persistence, backup system, measured SLO/RPO/RTO, or completed restore exercise
- Target status: profile-specific measured operational commitments and repeatable recovery evidence tied to the exact supported release architecture
- Migration status: no production deployment/data migration is performed by this ADR; operational evidence becomes mandatory incrementally as physical persistence and deployable profiles are introduced

## Context

Psychometrics Commons is intended to support the **Community profile**, **Hosted profile**, and **Enterprise profile**. The product architecture already requires capability-scoped degradation, immutable scientific/result artifacts, durable data-rights evidence, transactional integration, and release provenance.

Those design properties are not operational proof. Declaring GA, publishing availability commitments, or claiming enterprise recovery readiness without measured profile-specific evidence would turn architecture intent into an unsupported commercial assertion. Generic SLA numbers chosen before a real deployment topology, workload, storage design, and backup system exist would be arbitrary.

## Decision

1. No deployment profile is called **GA** and no commercial SLO/RPO/RTO commitment is published until that exact profile has version-controlled values, alert thresholds, backup policy, recovery procedure, measured evidence, and operator runbooks on the exact supported release architecture.
2. SLO, RPO, and RTO values are **profile-specific and evidence-derived**. This ADR intentionally does not invent universal numeric targets.
3. Backup copies inherit the security, privacy, tenancy, retention, and data-classification obligations of their primary data.
4. Recovery must preserve immutable instrument/response/result/research provenance, tenant isolation, restricted research linkage, outbox/inbox deduplication, consent/data-rights evidence, and deletion/retention semantics.
5. Destructive schema or storage migrations require a successful backup/restore drill for the affected current schema/application line before release.
6. Capability-scoped dependency outages are tested separately from product-core outages. Optional integration failure must not be reported as total-product unavailability when core capabilities remain safely usable.
7. Incident and recovery evidence is tied to the exact release/build/schema/configuration contract; historical evidence from an older topology is not automatically reusable.

## Ownership and boundaries

| Responsibility | Owner | Interface/evidence | Forbidden coupling |
|---|---|---|---|
| Product-domain recovery invariants | psychometrics-commons | migrations, backup/restore tests, release evidence | restoring rows while violating scientific/privacy semantics |
| Deployment-profile SLO/RPO/RTO | deployment/operator owner for that profile | versioned operational policy + measured evidence | one unmeasured global SLA copied to all profiles |
| Keyverse recovery | Keyverse owner | identity service contract/evidence | product claiming identity restore evidence it does not own |
| fast-mlsirm recovery | fast-mlsirm owner | scoring/artifact contract/evidence | product silently substituting scores during outage |
| External research/AI/temporal services | owning CWL services | versioned capability contracts | treating optional dependency outage as loss of valid local participant state |
| Release acceptance | psychometrics-commons release governance | exact-source/artifact/schema/profile evidence bundle | stale prior-release DR evidence silently transferred |

## Recovery domains

| Durable domain | Recovery requirement |
|---|---|
| Instrument/item publication | exact immutable version bytes/digests and publication state remain interpretable |
| Session/response evidence | accepted events and frozen snapshots preserve ordering, idempotency, and digest binding |
| Scoring/result evidence | exact scoring request/result/provenance and supersession chains survive restore |
| Consent/research contribution | purpose-specific decisions, revocations, and withdrawal evidence remain monotonic and auditable |
| Data-rights requests | completed/partial/rejected/failed evidence is not lost or rolled backward into misleading state |
| Restricted research linkage | remains encrypted/restricted and is not restored into a broader-access context |
| Outbox/inbox | replay after restore cannot silently duplicate externally visible side effects |
| Research release manifests | immutable digests, access class, citation and supersession relations remain verifiable |
| Audit/security evidence | retention and access rules remain consistent with the profile's policy |

## Operational evidence contract

Every profile promoted to GA binds operational evidence to at least:

```text
deployment_profile_ref
source_release_ref
source_commit_or_tag
artifact_digest_set
schema_migration_version
configuration_schema_version
contract_version_set
backup_policy_ref
restore_procedure_ref
slo_policy_ref
rpo_rto_policy_ref
runbook_set_ref
latest_exercise_evidence_refs
alert_policy_ref
reviewed_at
```

References may point to a restricted evidence store when the underlying content is security-sensitive. The product repository still records enough safe metadata to establish scope, version, owner, freshness, and required release gate.

## SLO governance

For each supported GA profile, the repository or an authoritative operations repository must define:

```text
service/capability name
service-level indicator definition
measurement window
target SLO
error-budget policy
alert threshold and burn policy
excluded/maintenance conditions
measurement source
owner and escalation route
```

A single aggregate “availability” number is insufficient when dependencies have different optionality. At minimum, the product must distinguish core assessment/session/result-read capability from authenticated/federated identity, scoring, optional AI narrative, longitudinal analysis, and Research Commons publication.

## RPO/RTO governance

For each durable data domain and deployment profile, define:

```text
backup frequency
backup retention
replication policy if any
recovery point objective
recovery time objective
restore order/dependencies
key/config prerequisites
post-restore verification
owner/escalation
```

RPO/RTO commitments must be demonstrated through drills or measured failover/recovery evidence appropriate to the architecture. A backup job reporting success is not restore evidence.

## Backup and restore invariants

A restore is accepted only when tests prove, as applicable:

1. application and schema version compatibility;
2. immutable artifact digests verify;
3. public opaque resource references remain stable;
4. tenant-scoped access remains isolated;
5. restricted linkage remains restricted;
6. outbox/inbox deduplication prevents duplicate side effects during recovery replay;
7. result/scoring supersession chains remain intact;
8. consent and withdrawal histories remain monotonic;
9. completed or partially completed data-rights evidence is not incorrectly reverted;
10. records that were validly deleted are not silently re-exposed from an old backup without the documented recovery/deletion reconciliation process;
11. research-release manifests still bind to exact immutable artifacts;
12. secrets/keys required for decryption/signature verification are restored or re-associated through approved key-management procedures rather than copied into application configuration.

## Deleted-data recovery reconciliation

Backups create a special privacy risk because an older backup may contain data that was later deleted from primary storage.

Therefore a restore procedure must include a durable deletion/retention reconciliation step before the recovered system becomes generally available. The process re-applies valid completed deletion obligations or establishes an explicit lawful retention exception. It may not silently resurrect deleted participant data because the backup predates the request.

## Failure and degraded-mode exercises

Before GA, enabled capabilities must be exercised against controlled failures including, where applicable:

- operational database unavailability and recovery;
- process crash around transaction/outbox/inbox publication and consumption;
- duplicate/reordered event delivery;
- fast-mlsirm scoring outage and recovery;
- Keyverse/federation outage;
- contextual-orchestrator/provider outage or EgressWeave denial;
- semantic-data-portal registration outage and later reconciliation;
- TEPP analysis outage;
- object/artifact storage outage;
- migration failure;
- backup restore into a clean environment;
- loss or rotation of a non-production test key under the documented key-management process.

Exercises use synthetic/non-production data unless a separately approved production-resilience process exists.

## Incident runbook minimum

GA profiles require maintained runbooks for:

- scoring backlog and scoring dependency outage;
- database failover/restore;
- outbox/inbox poison or stuck messages;
- account-linking conflict;
- data-rights stuck/failed/partial-retention cases;
- research-release digest mismatch or publication failure;
- suspected cross-tenant authorization incident;
- suspected restricted-linkage exposure;
- AI/provider denial and deterministic fallback verification;
- migration failure and rollback/roll-forward;
- release rollback/feature disablement without changing scientific scoring semantics.

Runbooks identify detection signal, triage boundary, safe operator actions, escalation, evidence to preserve, rollback/roll-forward, and closure proof.

## Data and persistence impact

This ADR does not create a database schema by itself. Once product persistence exists, backup/restore scope includes all product-owned relational state plus any approved product-owned encrypted payload/artifact storage required to interpret immutable resources. Deduplication, processing leases, deletion evidence, and restricted linkage are recovery-critical state and may not be treated as disposable caches.

Operational evidence metadata may live in repository/release artifacts or an approved restricted evidence store. Evidence location does not change the requirement to bind it to the exact release/profile.

## Failure and degraded modes

- Missing current restore evidence: GA/release commitment that requires it fails closed; development/research modes may continue under truthful pre-GA labeling.
- Optional dependency outage: only dependent capability degrades when product invariants permit.
- Core persistence corruption/unavailable required store: state-changing core commands fail rather than acknowledging undurable work.
- Restore digest/provenance mismatch: recovered service remains unavailable for affected operations until reconciliation/rollback.
- Deleted-data reconciliation incomplete: recovered participant-serving access remains blocked for affected data scope.
- Stale SLO/RPO/RTO evidence after material topology/schema change: evidence is invalidated for the new release until remeasured.

## Security, privacy, and tenancy

- Backups are encrypted and access-controlled according to their highest contained data classification.
- Backup/restore operators receive least privilege and audited access.
- Restored data is tenant-scoped and subject to the same authorization rules as primary data before serving users.
- Restricted research linkage is not copied to analytics, observability, or lower-trust restore validation environments.
- Restore logs contain resource counts/digests/safe identifiers rather than raw assessment or linkage data.
- Retention policies include backups; “deleted from primary” is not represented as complete deletion if a policy permits recoverable retained backup copies without a lawful basis and reconciliation process.

## Deployment and operations impact

The **Community profile** may document operator responsibilities rather than offer a CWL-managed SLA. The **Hosted profile** requires CWL-owned operational evidence for enabled hosted capabilities. The **Enterprise profile** requires an explicit responsibility split between CWL/product artifacts and customer-operated infrastructure. The same domain invariants hold in all profiles even when operational responsibility differs.

`docs/architecture/DEPLOYMENT_AND_OPERATIONS.md` is the target topology/operability view; exact as-built environment topology and measured objectives become versioned evidence only after deployment.

## Migration and rollback

Operational evidence requirements phase in with deployable capability. Introducing the first physical schema requires backup/restore test design before destructive migration is allowed. Material storage/topology/key-management changes invalidate stale recovery evidence until the new configuration passes its exercises.

Rollback is allowed only when the prior application can safely interpret all persisted semantics created by the release. Otherwise use roll-forward/compatibility repair. Recovery evidence itself is append-only/superseded rather than edited to make an older drill appear current.

## Architecture-view impact

- `docs/architecture/DEPLOYMENT_AND_OPERATIONS.md` carries profile topology, failure, health and acceptance views.
- `docs/architecture/ERD.md` identifies durable state whose invariants must survive restore.
- `docs/architecture/SECURITY_AND_DATA.md` governs backup/restricted-linkage/privacy boundaries.
- `docs/TRACEABILITY.md` distinguishes target recovery policy from measured deployment evidence.
- `docs/COMPLIANCE_READINESS.md` must not promote architecture intent into certification/attestation evidence.

## Release acceptance evidence

A GA release candidate must tie together:

```text
exact protected source head / release tag
artifact/container/package digests
SBOM and build provenance
schema/migration version
configuration schema/profile version
supported API/event contract versions
security and accessibility evidence
coverage/test evidence
instrument/scoring/calibration/norm provenance
backup and restore drill evidence
profile SLO/RPO/RTO evidence
operator runbook version
independent review
```

A green feature PR, old disaster-recovery drill, or synthetic merge commit by itself does not establish integrated release readiness.

## Validation and release gates

Required automated/manual evidence evolves with the deployment but includes:

- automated backup success plus periodic real restore drill;
- restored-schema/application compatibility test;
- tenant and restricted-linkage negative authorization tests after restore;
- immutable digest/provenance verification after restore;
- deletion reconciliation test;
- outbox/inbox duplicate-suppression recovery test;
- migration rollback/roll-forward rehearsal for destructive changes;
- capability failure-injection tests;
- alerts/runbook exercise with observed detection and recovery timing;
- release gate that verifies current evidence rather than a stale prior release artifact.

## Alternatives considered

### Hard-code aggressive universal RPO/RTO/SLO values now

Rejected. The topology, workload, backup method, and customer profile are not yet fixed. Unsupported numbers would be marketing assertions rather than engineering contracts.

### Treat managed cloud provider durability as sufficient recovery evidence

Rejected. Provider durability does not prove application-level restore, schema compatibility, deletion reconciliation, deduplication, tenant isolation, or scientific provenance integrity.

### Back up only the operational database

Rejected as a universal policy. A deployment may also own encrypted payload/artifact stores and recovery metadata required to interpret product evidence.

### Make every optional dependency part of one global availability SLO

Rejected. It obscures capability-scoped degradation and can either overstate an outage or hide core-product failure.

## Consequences

Positive:

- GA and SLA claims become evidence-backed;
- recovery preserves scientific and privacy semantics, not merely rows;
- backup privacy/deletion risks are explicit;
- capability-level reliability can be measured honestly.

Costs:

- restore drills, failure injection, and runbook maintenance require ongoing operational investment;
- deployment profiles need separate evidence and may mature to GA at different times.

## Follow-up work

- when physical PostgreSQL persistence lands, create clean-environment backup/restore and deletion-reconciliation fixtures before any destructive migration;
- define capability-level SLI schemas and health metrics before choosing production SLO numbers;
- create versioned runbooks and evidence registry for the first Hosted profile deployment;
- document the customer/CWL responsibility matrix before an Enterprise profile is sold with operational commitments;
- make release automation reject stale/mismatched recovery evidence once those evidence artifacts exist.

## Traceability

- Product/release requirements: `docs/PRD.md`, `docs/TRD.md`.
- Deployment profile decision: ADR-0011.
- Persistence/recovery state: ADR-0015.
- Architecture view: `docs/architecture/DEPLOYMENT_AND_OPERATIONS.md`, `docs/architecture/ERD.md`, `docs/architecture/SECURITY_AND_DATA.md`.
- Assurance boundary: `docs/COMPLIANCE_READINESS.md`, `docs/RISK_REGISTER.md`.
- Status/delivery: `docs/TRACEABILITY.md`, `docs/ROADMAP.md`.

## Reversal conditions

Specific operational targets and mechanisms are expected to evolve. They may be changed through version-controlled operational policy and release evidence. The requirements for measured profile-specific commitments, real restore proof, deletion reconciliation, exact-release evidence, and preservation of domain/security invariants remain unless superseded by a stronger architecture decision.
