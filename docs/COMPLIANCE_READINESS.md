# Compliance Readiness and Assurance Evidence

- Status: Normative readiness/evidence baseline
- Date: 2026-08-09
- Scope: SOC 2 and CSAP readiness considerations plus generally required security/privacy/operational evidence
- Explicit non-claim: this repository is **not** asserting SOC 2 attestation, CSAP certification, or equivalence to either program

The purpose of this document is to ensure the architecture can produce auditable controls and evidence without hard-coding an unverified certification claim into product behavior. Actual certification scope/control mappings must be reviewed against the then-current program requirements, deployment model, organization policies, and independent auditor/assessor guidance.

## 1. Readiness model

Each control/evidence area can be in one of four states:

- **architecture_defined** — design/ownership/expected evidence is documented;
- **implemented** — control exists in code/infrastructure/process;
- **verified** — current exact-release/deployment evidence proves operation;
- **externally_assessed** — an independent auditor/assessor has evaluated the applicable scope and produced assessment evidence.

`externally_assessed` is one required evidence state for any external attestation/certification claim, but it is **not sufficient by itself**. Any claim also requires the actual deployment/product/legal-entity scope to match the assessment, a current control mapping for that scope, and the assessor's explicit scope, conclusions, exceptions, and validity period to support the claim. Repository documentation or this readiness architecture never constitutes SOC 2 attestation or CSAP certification. Repository documentation alone is `architecture_defined` evidence.

## 2. Control/evidence matrix

| Assurance area | Architecture requirement | Evidence required before hosted GA | Current architectural source |
|---|---|---|---|
| Identity and authentication | Keyverse federation; anonymous path separately bounded; token issuer/audience/signature/replay validation | identity integration tests, configuration, federation runbook, credential rotation evidence | ADR-0003, `SECURITY_AND_DATA.md` |
| Authorization and tenant isolation | product-owned resource/tenant authorization; no default tenant; opaque ID is not authority | cross-tenant negative tests, permission matrix, privileged-access evidence | TRD, ERD, security architecture |
| Least privilege | separate service roles, no cross-service application DB credentials, restricted research linkage role | deployed IAM/database grants, credential inventory, negative access tests | ADR-0001, ADR-0015 |
| Change management | protected branches, independent review when required, exact-head CI/security/coverage gates, ADR supersession | GitHub rules/review evidence, release provenance, approved migrations | AGENTS, ADR process, release docs |
| Secure SDLC | TDD, SAST/dependency/secret scanning, SBOM/provenance, 100% owned-code coverage target | exact-release pipeline evidence and unresolved-finding inventory | TRD, AGENTS, quality attributes |
| Vulnerability management | dependency scanning, static analysis, prioritized remediation and release blocking | scan results, remediation SLA/policy, exception approvals, release evidence | TRD/security requirements |
| Supply-chain integrity | immutable pins where practical, SBOM, reproducible/signed provenance | artifact attestation, SBOM, dependency lock evidence, verification procedure | deployment/operations, release gates |
| Encryption | encrypted transport/storage, secret-manager integration, classified backup encryption | deployed TLS/storage/KMS policy evidence, key rotation and restore test | security/data, ADR-0017 |
| Secret management | secrets not committed/client-exposed/logged; deployment secret manager | secret inventory references, scanning, rotation/incident evidence | security/data, AI governance |
| Audit logging | resource/correlation/digest-based safe audit trails, privileged access logging | log schema, retention/access controls, sample audit reconstruction | TRD observability, security/data |
| Data minimization/purpose limitation | purpose-specific consent and data projection; no blanket masking as primary architecture | field/data-flow inventory, approved projections, consent/policy tests | ADR-0006, research governance |
| Data retention/deletion | deployment-specific retention, durable data-rights lifecycle, backup deletion reconciliation | retention schedule, deletion tests, restore reconciliation | data rights, ADR-0017 |
| Research privacy | separate pseudonym namespace, restricted linkage, release privacy review | release leakage tests, access roles, privacy review evidence | ADR-0006/0007, research governance |
| Data residency | profile/tenant-specific storage/provider constraints | deployed region/resource inventory and enforcement tests | ADR-0011, deployment/operations |
| Availability | capability-scoped health/degradation; no invented SLA | measured profile SLO and alert evidence | deployment/operations, ADR-0017 |
| Business continuity / DR | backup/restore, RPO/RTO evidence, runbooks, deletion reconciliation | real restore/failure drill on current release | ADR-0017 |
| Incident response | runbooks for auth, tenant, linkage, DB, queue, migration, provider and release incidents | exercise evidence, escalation/contact/closure artifacts | deployment/operations |
| Monitoring | logs/metrics/traces for API, jobs, outbox/inbox, data rights, scientific failures | dashboards/alerts tied to SLO and incident response | TRD/quality attributes |
| Privacy incident boundary | raw sensitive responses/linkage excluded from routine logs and public releases | redaction/leak tests, incident runbook | security/data, research governance |
| AI/provider governance | task purpose/data/provider/residency/retention policy; EgressWeave control; deterministic fallback | provider contracts/config, egress tests, task provenance, adversarial tests | AI governance |
| Accessibility | reference client WCAG 2.2 AA target | automated/manual/assistive-technology evidence | ADR-0013, quality attributes |
| Scientific integrity | exact version/digest provenance, recovery/scoreability/DIF/invariance, immutable result | instrument/scoring validation bundle on exact release | measurement governance |
| Third-party/service dependency | explicit bounded contexts and failure-scoped capability model | dependency inventory, contract/version evidence, outage tests | C4, ADR-0011 |
| Operator access | least privilege, separation of identity admin/research steward/product publisher roles | role mapping, access-review evidence, audit samples | security/data, C4 |
| Release governance | integrated protected-head release only after full gates | release checklist, signed provenance, rollback/restore evidence | deployment/operations, ADR-0017 |

## 3. Evidence hierarchy

An assurance statement should identify its evidence level:

```text
architecture intent
< implementation evidence
< exact-release automated/manual verification
< deployed operational evidence over time
< independent external assessment
```

Do not promote a lower level into a higher-level claim. Examples:

- “encrypted storage is required” is architecture intent, not evidence that a production database is encrypted;
- a successful unit test is not an operational restore drill;
- an SBOM file is not by itself supply-chain attestation;
- “designed for SOC 2/CSAP readiness” is not SOC 2/CSAP certification;
- an external assessment report outside the claimed deployment scope or validity period does not support a current product-wide claim.

## 4. Evidence registry requirement

Before hosted GA, maintain a versioned evidence index that maps each applicable control to:

```text
control/evidence area
scope/profile/tenant where relevant
owner
implementation reference
verification procedure
latest evidence reference/time
release/deployment version
exceptions/accepted risk
review/expiry date
```

Evidence containing security-sensitive configuration, incident data, or participant information may live in a restricted evidence store; the repository stores safe references and required contract metadata.

## 5. Separation of duties

Architecture should permit separate authority for:

- source author;
- independent reviewer/approver where policy requires;
- release publisher;
- production operator;
- Keyverse identity administrator;
- Psychometrics Commons instrument publisher;
- research data steward/release approver;
- restricted linkage administrator;
- security incident responder.

A small deployment may assign multiple roles to one person, but the policy and audit trail must still distinguish which authority was exercised. A Keyverse identity administrator is not implicitly a research-release approver.

## 6. Data-location and subcontractor/provider registry

Hosted/enterprise profiles require a current inventory of external data processors/providers and locations for enabled features, including:

- identity/federation dependencies;
- databases/object stores/backups;
- observability/logging;
- AI/model providers;
- email/notification providers if added;
- security/scanning services where data leaves the boundary;
- research catalog/artifact distribution.

The inventory identifies data classes, purpose, region/residency, retention, encryption, contractual basis, and disable/fallback behavior. Provider inventory is configuration/evidence, not hard-coded into the domain model.

## 7. Auditability without blanket masking

PII masking is not treated as the only privacy control because it can make legitimate assessment/research/data-rights workflows unusable or misleading.

The assurance model instead requires:

- identity/domain separation;
- data classification;
- purpose-limited projections;
- field/record authorization;
- encrypted storage/transport;
- restricted linkage;
- privileged-access audit;
- retention and data-rights processing;
- provider/data-location policy;
- minimized routine logging.

Masked/tokenized views may still be used where they help a specific role without breaking the workflow.

## 8. Certification readiness gate

Before initiating or claiming readiness for a specific external assessment, create a dated scope-specific mapping against the **current authoritative program/control requirements** and actual deployed architecture.

The mapping identifies:

- included legal entity/product/services/environments;
- customer/deployment responsibility split;
- applicable and non-applicable controls with rationale;
- existing evidence and gaps;
- remediation owner/date;
- auditor/assessor-required evidence format;
- any inherited controls from cloud/provider services.

A static 2026 architecture document must not be assumed to remain a correct future certification checklist.

## 9. Readiness blockers before hosted GA

At minimum the following are unresolved until implementation/evidence exists:

- physical tenant-aware persistence and IAM/grant evidence;
- deployed Keyverse/resource authorization integration;
- current encrypted backup and real restore drill;
- measured SLO/RPO/RTO;
- incident runbook exercises;
- deployed observability/alerting;
- release SBOM/provenance verification;
- public/admin API and event contract validation;
- research-release privacy/scientific end-to-end evidence;
- supported client accessibility acceptance;
- instrument/scoring scientific release evidence;
- exact provider/data-location/retention inventory for enabled hosted features.

These are roadmap/evidence items, not reasons to overstate the current maturity.
