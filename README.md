# Psychometrics Commons

[![Ask DeepWiki](https://deepwiki.com/badge.svg)](https://deepwiki.com/ContextualWisdomLab/psychometrics-commons)

CWL Psychometrics Commons is the headless product repository for public psychometric assessment, reflective self-understanding, longitudinal observation, and consent-governed research data contribution.

```text
Measure -> Understand -> Reflect -> Observe Over Time -> Contribute to Science
```

This repository owns the hosted product runtime and integration composition. It consumes reusable measurement contracts and numerical capabilities from `ContextualWisdomLab/fast-mlsirm`; identity and federation from Keyverse; temporal/event analysis from TEPP; and research release/catalog capabilities from `semantic-data-portal`.

## Start with a published instrument

When the instrument catalog HTTP family is running in this process:

1. Call `GET /v1/instruments` to list startable published locale-specific releases.
2. Copy one `release_ref` and its exact `locale`.
3. Call `POST /v1/sessions` with those values when the session family is available.

Draft, suspended, and retired releases are omitted. The as-built contract is [`openapi/instruments.yaml`](openapi/instruments.yaml).

## Architecture boundary

Psychometrics Commons owns product APIs, instrument publication, participant/session lifecycle, response events, consent and data-rights workflows, scoring dispatch, immutable result snapshots, product persistence, resource authorization, reference-client composition, deployment profiles, research-contribution handoff, observability, and service integration.

It does **not** duplicate psychometric numerical kernels, identity credentials, temporal model kernels, public research catalog internals, or generic LLM orchestration. `g7` is an optional replaceable reference client rather than a platform dependency.

## Personal result export

Protected-main result-export domain evidence is available through `ResultExport::from_snapshot`. Before creating or delivering an export, the server must authorize the authenticated actor against the exact stored result resource, its owning participant, and tenant scope; caller-supplied result, participant, or tenant values are never authority. Only after that authorization succeeds, call `ResultExport::from_snapshot` with an opaque `export_ref`, the exact BCP 47 report locale, and approved limitation text, and deliver the returned JSON or human-readable report to the participant. Before delivery, confirm that every exported construct score, disposition, present standard error, and version provenance match the immutable result snapshot. If authorization or export fails, do not deliver an artifact; repair the authoritative identity/access evidence or the locale, timestamp, or limitation text as appropriate. Do not invent a type score, do not mask the owner `participant_ref`, and do not treat this domain copy as the HTTP `POST /v1/results/{result_ref}/exports` transport; authorized HTTP delivery remains an active slice.

## Documentation

### Product, technical, and governance baseline

- [Product and Technical Gap Baseline](docs/product-technical-gap-baseline.md) — exact protected-main snapshot, current open PR/issue inventory, buyer-visible gaps, and the next executable loop.
- [Product Requirements](docs/PRD.md) — users, consumer MVP, longitudinal and research experiences, acceptance criteria, exclusions, and release policy.
- [Technical Requirements](docs/TRD.md) — APIs/events/data contracts, state machines, identity/tenancy, idempotency, failure modes, security/privacy, accessibility, deployment, and release gates.
- [Psychometric Measurement Governance](docs/MEASUREMENT_GOVERNANCE.md) — factor/model selection, scoreability, recovery, DIF/invariance, multilevel/time/facet structure, automated scoring, and governed rubric/item-bank evidence required for publication.
- [Bounded AI Governance](docs/AI_GOVERNANCE.md) — optional AI use, deterministic fallback, provider/privacy boundaries, LLM-as-a-Judge treatment, and prohibited score/decision mutation.
- [Research Commons Governance](docs/RESEARCH_GOVERNANCE.md) — research consent, identity separation, staging/privacy/scientific review, immutable release bundles, access classes, withdrawal/correction, and reproducibility.
- [Threat Model](docs/THREAT_MODEL.md) — security assets, trust boundaries, principal attack/failure scenarios, required controls, and GA evidence rather than architecture-only mitigation claims.
- [Test Strategy](docs/TEST_STRATEGY.md) — TDD, domain/state/persistence/security/scientific/accessibility/recovery evidence classes, exact-head validation, and non-vacuous coverage requirements.
- [Operability and Recovery](docs/OPERABILITY.md) — deployment-profile health, capability degradation, retries, incident model, backup/restore, migrations, runbooks, and evidence-gated SLO/RPO/RTO.
- [Release Acceptance](docs/RELEASE_ACCEPTANCE.md) — exact-head software release, consumer-instrument publication, and Research Commons release gates plus post-release artifact verification.
- [Quality Attribute Scenarios](docs/QUALITY_ATTRIBUTES.md) — measurable scientific, reliability, availability, security, privacy, accessibility, performance, portability, maintainability, observability, and recovery scenarios.
- [Compliance Readiness](docs/COMPLIANCE_READINESS.md) — SOC 2/CSAP readiness evidence model, control/evidence matrix, separation of duties, and explicit certification non-claim.
- [Risk Register](docs/RISK_REGISTER.md) — material scientific, product, security, privacy, operational, integration, and commercial risks with treatment/evidence state.
- [Glossary](docs/GLOSSARY.md) — canonical product, scientific, identity, research, integration, deployment, and evidence terminology.
- [Architecture](ARCHITECTURE.md) — bounded contexts, dependency direction, runtime modules, data domains, integration consistency, and architecture fitness functions.
- [Architecture Decision Records](docs/adr/README.md) — authoritative material decisions and the required ADR quality contract.

### Architecture viewpoints and evidence

- [Architecture View Index](docs/architecture/README.md) — context/container/component, UML, ERD, security/data, and deployment/operations views.
- [C4-style Context / Container / Component Views](docs/architecture/C4.md) — stakeholders, external systems, target containers, components, and ownership.
- [UML-Aligned Models](docs/architecture/UML.md) — domain class model, lifecycle state machines, and key interaction sequences.
- [Logical ERD](docs/architecture/ERD.md) — product-owned entities, cardinalities, immutable boundaries, linkage restrictions, and persistence invariants.
- [Security, Privacy, and Data Boundaries](docs/architecture/SECURITY_AND_DATA.md) — trust boundaries, classification, threats, identity, research, and AI data policies.
- [Deployment, Operations, and Recovery](docs/architecture/DEPLOYMENT_AND_OPERATIONS.md) — deployment profiles, degraded modes, observability, backup/restore, migration, and GA recovery evidence.
- [Requirements and Architecture Traceability](docs/TRACEABILITY.md) — PRD/TRD/ADR requirements mapped to current protected-main implementation status and future evidence.
- [Product Delivery Roadmap](docs/ROADMAP.md) — dependency-ordered implementation phases and evidence-based exit criteria.
- [Documentation Completeness Assessment](docs/DOCUMENTATION_ASSESSMENT.md) — what is sufficient as an implementation baseline and what still blocks GA operational evidence.

### Repository operation

- [AGENTS.md](AGENTS.md) — repository development and architecture rules for autonomous/agentic work.
- [CLAUDE.md](CLAUDE.md) — concise coding-agent entry point into the same normative contracts.
- [Changelog](CHANGELOG.md) — unreleased and released product/architecture changes.

## Architecture authority and implementation status

An accepted ADR must define concrete ownership, interfaces, invariants, failure behavior, security/privacy/tenancy boundaries, migration and rollback, validation evidence, alternatives, and reversal conditions. Material implementation that contradicts an accepted ADR requires an explicit superseding decision rather than silent architectural drift.

Architecture diagrams may describe **normative target semantics** that are not yet deployed. A diagram, PRD item, ADR, threat mitigation, runbook, or release checklist is not implementation or operational evidence. [`docs/TRACEABILITY.md`](docs/TRACEABILITY.md) identifies what exists on a named protected-main baseline versus what remains a target. When an HTTP API is implemented, its OpenAPI contract becomes a release requirement. When durable event transport is implemented, its AsyncAPI contract becomes a release requirement. When a physical database migration is implemented, its schema and migration evidence become release requirements. Do not fabricate any of these artifacts before the corresponding implementation exists.
