# Operability, Recovery, and Incident Contract — Psychometrics Commons

- Status: Normative operations baseline; measured service levels remain evidence-gated
- Date: 2026-08-10
- Scope: Community/Research, CWL Hosted, and Enterprise/Self-hosted deployment profiles
- Maturity rule: this document defines operational behavior and evidence required before GA. It does not claim a currently deployed topology, measured SLO, RPO/RTO, certification, or restore result unless `docs/TRACEABILITY.md` or a dated release-evidence artifact links that evidence.

## 1. Operational objective

Psychometrics Commons must remain understandable and recoverable under partial dependency outages without compromising measurement integrity, consent, research separation, or participant rights. Reliability is therefore defined as **preserving correct durable state and truthful capability status**, not merely returning HTTP 200 responses.

## 2. Deployment profiles

### Community / Research

Required minimum composition:

- Psychometrics Commons runtime;
- supported PostgreSQL operational persistence once physical persistence is integrated;
- fast-mlsirm-compatible scoring path;
- standalone client or API consumer.

Keyverse, TEPP, Gyeot, semantic-data-portal, contextual-orchestrator, pg-llm-batch, g7, Inkspan, RankWeave, LifeOS and Clearfolio integrations may be absent without changing the scientific meaning of core assessment results.

### CWL Hosted

Composes CWL bounded contexts as explicit independently observable capabilities. Dependency unavailability is surfaced as a capability state rather than hidden fallback. Secrets and provider authority are centrally governed but do not leak into browser clients or unrelated services.

### Enterprise / Self-hosted

Adds deployment-specific federation, residency, retention, encryption, network policy, backup/restore, observability, provider policy and operator evidence. These controls may be stricter than the hosted profile but cannot redefine scientific scoring semantics or silently alter historical result provenance.

## 3. Health model

A single undifferentiated `healthy=true` is insufficient.

The implementation must distinguish at least:

- **liveness** — process can make progress / is not irrecoverably wedged;
- **readiness** — mandatory dependencies for the selected profile are available enough to accept new work safely;
- **capability health** — optional/independent capabilities such as authenticated linking, scoring, AI narrative, research registration, or temporal analysis;
- **backlog health** — durable work is within measured operating bounds and not silently stalled;
- **data integrity health** — migrations/schema/digests/reconciliation do not indicate incompatible or corrupt state.

Readiness must not fail solely because an optional capability is unavailable if the selected operation can safely proceed without it. Conversely, a process can be live while not ready to accept new state-changing requests.

Operator HTTP probes, when implemented, are GET `/live` and GET `/ready`. `/live` answers process liveness only and must not perform store I/O; a hung or failed PostgreSQL connection must not restart a still-live process. `/ready` answers operation-scoped readiness and may name required capabilities as repeated `capability` query parameters. When the PostgreSQL adapter answers a bare GET `/ready` (no `capability=`), it requires `postgres_operational_store` so a read-only or unsupported store cannot advertise readiness to a load balancer. These probes do not publish measured SLO values. A bound TCP listener, when present, serves those same operations in a blocking accept loop until accept fails, or one request per accepted connection. Interrupted, aborted, or reset accepts retry, including `ConnectionReset` before `accept` returns, matching TCP reset processing in RFC 9293. A dropped probe connection does not stop later probes. It applies a bounded read/write timeout, rejects oversized requests without echoing them, and is not a measured availability claim. PostgreSQL observation happens after accept and only for GET `/ready`. Probe failure is unknown/unready and must not expose driver errors. Unsupported methods and paths use explicit `urn:psychometrics-commons:problem:` types rather than `about:blank`. Operators start the probe process with `run_health_process` after setting `HEALTH_LISTEN_ADDR` or platform `PORT`. Optional `DATABASE_URL` is observed only for GET `/ready`. Optional `HEALTH_BACKLOG_HEALTH` must be `within_bounds`, `stalled`, or `unknown`; missing backlog stays unknown and not ready. Optional `HEALTH_REQUIRED_RELATIONS` declares the exact comma-separated `schema.relation` identities whose presence is verified for readiness; without declared relations, data-integrity evidence stays unknown and readiness fails closed even with a reachable store. Point liveness at GET `/live` and readiness at GET `/ready`. Do not treat a single `accept_one_*` call as a running probe server.

## 4. Capability degradation matrix

| Dependency/capability failure | Required product behavior |
|---|---|
| Keyverse unavailable | anonymous flow and already-established valid short-lived product sessions continue where safe; new authenticated/linking operations fail explicitly |
| fast-mlsirm scoring unavailable | completed response snapshot remains durable; scoring job waits/retries; no fabricated score |
| contextual-orchestrator / model provider unavailable | deterministic narrative fallback; numeric result retrieval remains available |
| EgressWeave policy denial | optional outbound capability fails closed; direct SDK/network bypass prohibited |
| semantic-data-portal unavailable | participant result unaffected; approved release registration remains durable/retryable |
| TEPP unavailable | longitudinal observations/input snapshots remain durable; temporal analysis waits |
| Gyeot client offline | observations remain locally recoverable and synchronize idempotently when permitted |
| PostgreSQL unavailable | state-changing commands requiring persistence fail without partial domain/outbox state; read behavior depends on profile/cache authority and must not invent freshness |
| migration/schema incompatible | readiness fails closed for the affected deployment; no compatibility-by-label claim |
| outbox transport unavailable | local valid action remains committed with durable unpublished outbox evidence; backlog alarms and replay/reconciliation apply |
| inbox downstream effect unavailable | inbox remains pending/processing with stable idempotency and bounded retry; receipt is not completion |

## 5. Observability contract

Operational telemetry is purpose-limited. Routine logs must not contain raw assessment responses, unrestricted free text, credentials, Keyverse subject identifiers when a product reference suffices, restricted research linkage values, or raw provider secrets.

Minimum structured telemetry, when the corresponding capability exists:

- correlation reference and tenant/resource reference appropriate to authorization;
- operation/state transition accepted or rejected;
- instrument/version and response/result snapshot references where operationally necessary;
- scoring job state and typed failure class;
- outbox age, publish attempts and quarantine class;
- inbox state, processing age, completion-evidence presence and quarantine class;
- dependency/capability state;
- data-rights request age/state;
- research snapshot/release registration reconciliation state;
- migration/schema version and readiness status;
- release/build/provenance identifier.

Metrics and alerts must be scoped so a single large tenant cannot hide another tenant's starvation or failure.

## 6. Durable work and retry policy

Retries are not proof of reliability unless they preserve idempotency and bounded resource use.

Every durable job/event path specifies:

- retryable versus terminal failure taxonomy;
- maximum attempts or bounded elapsed retry policy appropriate to the operation;
- backoff/jitter policy;
- stable idempotency identity for any side effect;
- cancellation semantics;
- quarantine/dead-letter behavior;
- operator-visible cause and evidence;
- replay/reconciliation procedure after recovery.

Unknown integrity, authorization, schema, tenant-binding, digest, or scientific failures are not blindly retryable. Provider/network availability errors may be retryable without weakening provider/privacy/egress policy.

## 7. Backup and restore

Backup policy is deployment-profile specific and becomes release/GA evidence only after execution.

For persistence owned by Psychometrics Commons, a GA profile must establish and periodically re-prove:

1. backup creation from the supported database version and encryption context;
2. restoration into an isolated recovery environment;
3. migration/schema compatibility and expected indexes/constraints;
4. exact digest/provenance consistency for immutable snapshots/artifacts;
5. tenant authorization and restricted-linkage protections after restore;
6. outbox/inbox/job recovery without duplicate domain effects;
7. data-rights and research withdrawal/eligibility state preservation;
8. key/secret dependency recovery without embedding secrets in backup artifacts improperly;
9. measured restore duration and recoverable data point for that exact profile.

The architecture deliberately does **not** publish universal RPO/RTO numbers before these profile-specific measurements exist.

## 8. Migration operations

Schema/application deployment must preserve a backward-compatible window or explicitly use a reviewed maintenance/roll-forward strategy.

Before a destructive or irreversible migration:

- inventory affected immutable/scientific and operational entities;
- verify backup/restore evidence;
- test migration against realistic prior-version data;
- prove tenant/restricted-linkage constraints after migration;
- document rollback versus roll-forward-only choice and trigger;
- prevent application versions outside the compatibility window from writing incompatible state;
- verify no migration rewrites published scientific payloads merely to mimic a new schema.

A failed migration is a readiness failure. Operators must not bypass it by manually relabeling schema version metadata.

## 9. Incident model

Operational incidents are classified by first failing boundary and impact, not by the loudest downstream symptom.

For each incident capture:

- detection time and evidence source;
- exact deployed application/migration/artifact versions;
- affected tenant/capability scope;
- first failing boundary;
- immediate cause, technical root cause, systemic/control cause where material;
- privacy/security/scientific/data-integrity impact assessment;
- containment and recovery action;
- replay/reconciliation required;
- participant/research/operator communication obligation where applicable;
- regression/fitness function added or strengthened;
- evidence that protected operation is restored.

A dependency outage and a source-code scientific defect are separate evidence classes. Infrastructure failure must not be misreported as a fabricated source-code finding.

## 10. Operational runbooks required before GA

The deployed profile must have executable or operator-tested runbooks for at least:

- database unavailable / pool exhaustion;
- migration failure and compatibility refusal;
- restore from backup;
- outbox publication backlog;
- inbox processing backlog or poison message;
- scoring dependency outage and job reconciliation;
- Keyverse/JWKS/federation outage;
- account-link conflict/adjudication;
- optional AI provider/orchestrator outage and deterministic fallback verification;
- EgressWeave denial/provider policy mismatch;
- research release registration failure/reconciliation;
- restricted linkage access anomaly;
- data export/deletion backlog/failure;
- TEPP analysis backlog and longitudinal data reconciliation;
- secret/key rotation;
- release rollback/roll-forward and artifact provenance verification.

A runbook link that has never been exercised is documentation, not recovery evidence.

## 11. SLI/SLO evidence model

Potential indicators include:

- assessment session create/resume/complete success;
- accepted response-event latency/error classes;
- scoring completion by terminal class and age;
- result-read availability;
- outbox oldest-unpublished age and retry rate;
- inbox oldest-pending/processing age and quarantine rate;
- data-rights completion age;
- research release registration age;
- capability-specific dependency health;
- migration/restore success and duration;
- tenant-isolation/security-control test status;
- accessibility regression status for supported reference clients.

SLO thresholds are selected only after workload, topology, dependency profile and recovery measurements exist. A target number in architecture prose cannot be sold as a measured service commitment.

## 12. Privacy-preserving operations

Operators should diagnose with references, digests, state, timing and typed failures before accessing content. Privileged content access is purpose-bound, least-privilege, time/scoped where practical, and auditable. There is no general-purpose "unmask all PII" operational mode; equally, there is no blanket masking that makes legitimate incident/adjudication workflows impossible.

Restricted linkage access is isolated from normal support and analytics. Diagnostic exports must not become an ungoverned alternative research dataset.

## 13. Release-to-operations handoff

A release is not operationally accepted until the target profile proves, as applicable:

- migrations bootstrap/upgrade correctly;
- readiness/liveness/capability health reflect actual failure states;
- backup/restore and rollback/roll-forward procedures work;
- required dashboards/alerts identify backlog and integrity failures;
- secrets/identity/provider policies are installed without embedding credentials in artifacts;
- degraded-mode tests match architecture promises;
- exact release SBOM/provenance/artifact digests are preserved;
- operator docs refer to current resource/state names;
- no unresolved critical or high security/privacy/scientific/data-integrity risk remains
  unless `docs/RISK_REGISTER.md` records evidence-backed closure or an explicitly
  accepted risk by authorized governance.

## 14. Acquisition-readiness evidence

Buyer due diligence should be able to distinguish:

- architecture-defined control;
- implemented control on protected main;
- verified control in an integration environment;
- measured operational evidence in a named deployment profile;
- independently assessed/certified claim, if any.

Never collapse these maturity levels. SOC 2/CSAP readiness work may map evidence and controls, but certification/attestation is not claimed without the external process.

## 15. References

Eddy, W. (Ed.). (2022). *Transmission Control Protocol (TCP)* (RFC 9293). Internet Engineering Task Force. https://doi.org/10.17487/RFC9293

Fielding, R., Nottingham, M., & Reschke, J. (Eds.). (2022). *HTTP Semantics* (RFC 9110). Internet Engineering Task Force. https://doi.org/10.17487/RFC9110

International Organization for Standardization & International Electrotechnical Commission. (2023). *ISO/IEC 25010:2023 Systems and software engineering—Systems and software Quality Requirements and Evaluation (SQuaRE)—Product quality model*.

Kubernetes Authors. (2024). *Configure liveness, readiness and startup probes*. Kubernetes Documentation. https://kubernetes.io/docs/tasks/configure-pod-container/configure-liveness-readiness-startup-probes/

National Institute of Standards and Technology. (2022). *Secure Software Development Framework (SSDF) Version 1.1: Recommendations for mitigating the risk of software vulnerabilities* (NIST SP 800-218). https://doi.org/10.6028/NIST.SP.800-218
