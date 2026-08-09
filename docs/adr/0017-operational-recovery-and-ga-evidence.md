# ADR-0017: Operational recovery and GA evidence contract

- Status: Accepted
- Date: 2026-08-09
- Scope: availability evidence, SLO/RPO/RTO governance, backup/restore, disaster recovery, incident runbooks, GA release acceptance
- Supersedes: none

## Context

Psychometrics Commons is intended to support Community/Research, CWL Hosted, and Enterprise/Self-hosted deployment profiles. The product architecture already requires capability-scoped degradation, immutable scientific/result artifacts, durable data-rights evidence, transactional integration, and release provenance.

Those design properties are not operational proof. Declaring GA, publishing availability commitments, or claiming enterprise recovery readiness without measured profile-specific evidence would turn architecture intent into an unsupported commercial assertion. Generic SLA numbers chosen before a real deployment topology, workload, storage design, and backup system exist would be arbitrary.

## Decision

1. No deployment profile is called **GA** and no commercial SLO/RPO/RTO commitment is published until that exact profile has version-controlled values, alert thresholds, backup policy, recovery procedure, measured evidence, and operator runbooks on the exact supported release architecture.
2. SLO, RPO, and RTO values are **profile-specific and evidence-derived**. This ADR intentionally does not invent universal numeric targets.
3. Backup copies inherit the security, privacy, tenancy, retention, and data-classification obligations of their primary data.
4. Recovery must preserve immutable instrument/response/result/research provenance, tenant isolation, restricted research linkage, outbox/inbox deduplication, consent/data-rights evidence, and deletion/retention semantics.
5. Destructive schema or storage migrations require a successful backup/restore drill for the affected current schema/application line before release.
6. Capability-scoped dependency outages are tested separately from product-core outages. Optional integration failure must not be reported as total-product unavailability when core capabilities remain safely usable.
7. Incident and recovery evidence is tied to the exact release/build/schema/configuration contract; historical evidence from an older topology is not automatically reusable.

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
- process crash around transaction/outbox publication;
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

## Security, privacy, and tenancy

- Backups are encrypted and access-controlled according to their highest contained data classification.
- Backup/restore operators receive least privilege and audited access.
- Restored data is tenant-scoped and subject to the same authorization rules as primary data before serving users.
- Restricted research linkage is not copied to analytics, observability, or lower-trust restore validation environments.
- Restore logs contain resource counts/digests/safe identifiers rather than raw assessment or linkage data.
- Retention policies include backups; “deleted from primary” is not represented as complete deletion if a policy permits recoverable retained backup copies without a lawful basis and reconciliation process.

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

## Reversal conditions

Specific operational targets and mechanisms are expected to evolve. They may be changed through version-controlled operational policy and release evidence. The requirements for measured profile-specific commitments, real restore proof, deletion reconciliation, exact-release evidence, and preservation of domain/security invariants remain unless superseded by a stronger architecture decision.
