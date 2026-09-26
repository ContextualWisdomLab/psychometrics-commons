# Psychometrics Commons Architecture

This document is the architectural map for the hosted product. Detailed normative decisions live in [`docs/adr/`](docs/adr/README.md); product and technical acceptance criteria live in [`docs/PRD.md`](docs/PRD.md) and [`docs/TRD.md`](docs/TRD.md). When these documents disagree, an accepted or superseding ADR governs architectural ownership and the PRD/TRD govern intended product behavior unless a later decision explicitly changes them.

## Architecture description and implementation evidence

The architecture is intentionally described through multiple stakeholder viewpoints rather than one overloaded diagram:

- [`docs/architecture/C4.md`](docs/architecture/C4.md) — stakeholder concerns, system context, target containers, and runtime components;
- [`docs/architecture/UML.md`](docs/architecture/UML.md) — domain class model, lifecycle state machines, and interaction sequences;
- [`docs/architecture/ERD.md`](docs/architecture/ERD.md) — product-owned logical data model, cardinalities, immutable aggregates, restricted linkage, and persistence invariants;
- [`docs/architecture/SECURITY_AND_DATA.md`](docs/architecture/SECURITY_AND_DATA.md) — trust boundaries, data classification, identity/privacy/AI controls, and threats;
- [`docs/architecture/DEPLOYMENT_AND_OPERATIONS.md`](docs/architecture/DEPLOYMENT_AND_OPERATIONS.md) — deployment profiles, capability degradation, observability, backup/restore, migration, and GA evidence;
- [`docs/TRACEABILITY.md`](docs/TRACEABILITY.md) — intended requirements and architecture mapped to a named protected-main implementation baseline;
- [`docs/ROADMAP.md`](docs/ROADMAP.md) — dependency-ordered product delivery and evidence-based exit criteria;
- [`docs/DOCUMENTATION_ASSESSMENT.md`](docs/DOCUMENTATION_ASSESSMENT.md) — architecture-document completeness assessment and remaining GA evidence gaps.

Architecture views may describe **normative target semantics** that have not yet been implemented. Diagrams and accepted ADRs are not by themselves as-built evidence. `docs/TRACEABILITY.md` is the current status index; implemented HTTP/event transports and physical persistence must eventually carry machine-readable OpenAPI/AsyncAPI/schema/migration evidence rather than being inferred from target diagrams.

## Product boundary

Psychometrics Commons is the **hosted product and integration composition layer**, not a replacement for the reusable CWL scientific and infrastructure services it consumes.

```text
Standalone Web / g7 / Gyeot / LifeOS / Institutional Embed
                         |
                         v
                Psychometrics Commons
        +----------------+----------------+
        |                |                |
        v                v                v
     Keyverse        fast-mlsirm     semantic-data-portal
     identity        measurement        research release
        |                |
        |          +-----+-------------------------+
        |          |                               |
        v          v                               v
      Gyeot --->  TEPP                  contextual-orchestrator
      EMA/ESM     temporal                    bounded AI
                                               |
                                               v
                                         pg-llm-batch

Additional optional capabilities:
Inkspan authoring | RankWeave search fusion | Clearfolio reports | EgressWeave egress controls
```

The arrow direction is dependency direction. `fast-mlsirm` never imports or depends on Psychometrics Commons.

## Bounded-context ownership

| Capability | Owner | Psychometrics Commons responsibility |
|---|---|---|
| Authentication, federation, passkeys, account linking | Keyverse | Validate identity claims; make product resource-authorization decisions |
| AssessmentSpec, RubricSpecification, scoring contracts, psychometric numerics | fast-mlsirm | Pin versions, dispatch work, persist immutable result provenance |
| Participant/session lifecycle, response events, consent, data rights | psychometrics-commons | Full source of truth |
| EMA/ESM participant collection | Gyeot | Enrollment/consent and normalized ingestion |
| Temporal, event, relationship, multilevel and multiple-membership analysis | TEPP | Submit immutable input snapshots and consume versioned artifacts |
| Research release catalog, lineage, license and discovery | semantic-data-portal | Build and approve immutable release manifests |
| Real-time model orchestration | contextual-orchestrator | Supply bounded, purpose-specific tasks and validate returned schemas |
| Bulk model work | pg-llm-batch | Submit auditable batch jobs where appropriate |
| External-call SSRF/DNS/resource enforcement | EgressWeave | Route approved outbound operations through the policy boundary |
| Assessment/rubric authoring primitives | Inkspan | Own published instrument state and approval workflow |
| Search fusion | RankWeave | Consume search results through an explicit integration contract |
| Document/report rendering | Clearfolio | Supply immutable result/report payloads |
| Reference CMS client | g7 | None; it is replaceable and not a source of truth |

No service receives direct read/write access to another service's normal application database.

## Hosted runtime modules

The initial product may deploy as one service, but code and persistence ownership must preserve these logical modules:

```text
instrument_publication
assessment_session
item_delivery
response_event
consent_record
scoring_dispatch
result_snapshot
data_rights
research_contribution
tenant_authorization
integration_outbox
integration_inbox
```

Splitting a module into a separate service later must not change its domain semantics or bypass existing versioned contracts.

## Core lifecycle

### Instrument publication

```text
draft -> review -> published -> suspended -> retired
```

Published content is immutable. Any semantic content change creates a new instrument version. Suspension prevents new sessions without erasing historical result provenance. Retirement is permanent for that version. ADR-0019 additionally requires exact-version scientific/content/rights evidence before review can transition to publication.

Instrument catalog discovery is an advisory read of product-owned persisted publication evidence. It does not add a publication lifecycle state and does not itself authorize or create a session. Because discovery is non-locking and publication can change after a page is read, session start locks and revalidates the exact persisted release immediately before minting a session. This preserves the existing TRD/ADR mappings: catalog persistence does not change bounded-context ownership, public HTTP operation families, logical ERD cardinalities, scientific publication gates, or cross-service database boundaries.

### Assessment session

```text
created -> active <-> paused -> completed -> scoring -> scored -> released
```

Explicit terminal alternatives:

```text
expired | cancelled | invalidated
```

Clients issue commands. The server owns state transitions. Duplicate equivalent commands are idempotent; undocumented transitions fail closed. Only active sessions accept ordinary response events.

Completion freezes an immutable response snapshot before any asynchronous scoring dispatch. A scoring or downstream outage therefore cannot cause loss or mutation of the participant's completed evidence.

## Product data domains

Data is separated by purpose rather than relying on blanket masking.

```text
Identity domain (Keyverse)
        |
        v
Operational participant domain
        |
        +--> Result/provenance domain
        |
        +-- explicit research opt-in --> Restricted linkage boundary
                                      |
                                      v
                               Research participant domain
                                      |
                                      v
                               Research staging snapshot
                                      |
                              privacy/scientific review
                                      |
                                      v
                               Immutable research release
```

Public research releases contain neither Keyverse subject references nor operational participant references. A restricted linkage store is the only allowed bridge between operational and research pseudonyms.

## Identity and anonymous participation

Anonymous participation is first-class. A participant can complete the core assessment without creating a Keyverse account. Optional account linking requires proof of control of the anonymous session and authenticated Keyverse subject; it adds a mapping but never rewrites historical response or result identifiers.

Keyverse identity roles do not automatically confer Psychometrics Commons research-steward or instrument-publisher authority. Resource authorization remains a product-domain decision.

## Measurement and scoring

Psychometrics Commons does not duplicate psychometric formulas. A scoring operation pins at minimum:

```text
response_snapshot_ref
assessment_spec_ref
instrument_version_ref
scoring_version_ref
calibration_reference
norm_version_ref?
requested_output_schema_version
```

The returned result pins engine/package provenance and typed scientific outcomes. Non-identification, insufficient anchors, unsupported contract versions, non-finite estimates, or scoreability failures fail closed. No LLM result can overwrite numeric score, calibration, norm, uncertainty, DIF, or other scientific output from the measurement engine.

## Narrative layer

The measured Big Five/facet profile is continuous. Personality Style is a separately versioned presentation mapping.

```text
fast-mlsirm ScoreProfile
        |
        v
canonical style-assignment key
        |
        v
approved interpretation rules
        |
        +--> deterministic localized narrative
        |
        +--> optional contextual-orchestrator prose rendering
```

AI text is replaceable and optional. If AI is unavailable or rejected, deterministic approved output still permits result retrieval. Narrative versions may change without mutating historical numeric results. ADR-0018 defines the exact behavior-affecting references/digests that make a style assignment deterministic and replayable.

## Research-release boundary

A research release is created only from an immutable approved dataset snapshot.

```text
research contribution
 -> pseudonymized staging
 -> de-identification/privacy-risk review
 -> dataset snapshot
 -> scientific/release approval
 -> release manifest + checksums
 -> semantic-data-portal registration
```

The portal owns catalog/discovery/lineage but never queries operational assessment tables. Corrections create a new release with an explicit supersession relation rather than replacing bytes under an existing release identifier.

## Longitudinal boundary

Gyeot collects observations; TEPP estimates temporal/event/relationship models. Psychometrics Commons preserves consent, enrollment, normalized ingestion, and immutable analytical input references.

Time is not collapsed to one timestamp. Preserve `observed_at`, `recorded_at`, `received_at`, `available_at`, validity intervals, original timezone/civil-time context, and multiple-membership context where applicable. This is required to distinguish within-person change, between-person variation, event ordering, delayed availability, and contextual membership.

## Integration consistency

Cross-service state changes use versioned APIs/events and transactional outbox/inbox patterns.

```text
local domain transaction
   = resource mutation + tenant-bound outbox event
                     |
                     v
              at-least-once transport
                     |
                     v
        validate schema/digest/tenant binding
                     |
                     v
              consumer inbox: pending
                     |
             processing + durable work
                     |
          idempotent local/external effect
                     |
                     v
          completion evidence + completed
```

ADR-0014 defines canonical payload digests, tenant-bound event identity, deduplication scope, replay and quarantine. ADR-0015 defines local transaction, inbox processing, crash recovery, and PostgreSQL persistence semantics. Receipt alone is never considered successful completion of a required non-local side effect. Poison/unverifiable messages are quarantined after bounded handling; a downstream outage cannot roll back a valid local participant action already committed by the owning domain.

## Versioning and provenance

Published scientific/product artifacts are immutable and content-addressed in addition to having opaque public references. Mutable discovery aliases such as `latest` must be resolved before an operation and never stored as result provenance.

A result must be replayable from its immutable version bundle within documented numerical tolerance. Norm, scoring, or narrative changes create new/superseding result snapshots; historical results do not silently drift.

## Multilingual and accessibility architecture

A translated assessment is a distinct instrument version. Item text is never silently substituted from another locale. Cross-locale comparisons and shared norms are enabled only after appropriate linking, anchor-stability, DIF/invariance, and score-recovery evidence.

Supported reference clients target WCAG 2.2 AA. Accessibility behavior that may affect the response process is part of instrument/version evidence rather than an untracked cosmetic client choice.

## Deployment profiles

### Community profile

Requires only the Psychometrics Commons runtime, upstream PostgreSQL 18.x operational persistence, a fast-mlsirm-compatible scoring path, and a standalone client. AI, TEPP, semantic-data-portal, g7, and other optional integrations can be absent.

### Hosted profile

CWL-operated composition of CWL bounded contexts as individually observable capabilities. Optional capability failure is scoped to that capability. Upstream PostgreSQL 18.x is the initial supported relational store.

### Enterprise profile

Customer/self-hosted or contracted deployment adding deployment-specific federation, data residency, retention, encryption, networking, provider, and audit controls without changing core domain contracts or historical result portability. Responsibility for database/network/backup/observability is explicit per contract.

Canonical profile terminology is defined in `docs/GLOSSARY.md` and ADR-0011. Operational recovery, profile-specific SLO/RPO/RTO evidence, backup/restore, and GA release requirements are detailed in [`docs/architecture/DEPLOYMENT_AND_OPERATIONS.md`](docs/architecture/DEPLOYMENT_AND_OPERATIONS.md) and ADR-0017. No universal SLA value is assumed without measured deployment evidence.

## Security and privacy principles

- least privilege and explicit service audiences;
- server-side tenant/resource authorization;
- no cross-service application-database access;
- no browser-held long-lived service credentials;
- purpose-bound projections for AI/export/research;
- restricted identity linkage instead of ambient identifiers;
- exact-authority egress controls for external providers;
- immutable release/scoring provenance and auditable privileged operations;
- SBOM, secret scanning, SAST, dependency and reproducibility evidence at release gates.

The detailed trust/data model is maintained in [`docs/architecture/SECURITY_AND_DATA.md`](docs/architecture/SECURITY_AND_DATA.md).

## Failure-degradation principle

Failure is capability-scoped whenever scientifically and securely possible.

| Dependency failure | Product behavior |
|---|---|
| Keyverse | anonymous flow and already-established valid short-lived sessions remain usable where safe |
| fast-mlsirm scoring path | completed response snapshot remains durable; no invented fallback score |
| contextual-orchestrator | deterministic narrative fallback; numeric result remains available |
| semantic-data-portal | personal result unaffected; release registration remains queued |
| TEPP | longitudinal observation persists; analysis waits |
| external provider denied by EgressWeave/equivalent reviewed boundary | optional AI capability fails closed with no bypass |
| unsupported/unverified database engine | deployment readiness/installation fails closed; no compatibility-by-label claim |

## Architecture fitness functions

The architecture is enforced by tests and release controls, not diagrams alone. As implementation grows, CI must prove at least:

- no reverse dependency from fast-mlsirm to this repository;
- exact-head validation rather than synthetic-merge-only evidence for repository-owned gates;
- exhaustive/fail-closed lifecycle transition behavior;
- idempotent command, response, outbox, and inbox handling;
- tenant-bound integration event identity and cross-tenant denial tests;
- crash-recoverable pending/processing/completed inbox semantics;
- immutable provenance and supersession behavior;
- no operational identity in public release fixtures;
- deterministic fallback when optional AI is unavailable;
- locale no-silent-fallback behavior;
- accessibility acceptance for supported clients;
- PostgreSQL 18.x migration/concurrency/recovery compatibility while it is the supported persistence target;
- exact owned-production coverage targets defined by repository policy;
- architecture-view and traceability links remain valid;
- as-built OpenAPI/AsyncAPI/schema contracts exist when the corresponding transports/persistence are implemented.

## Decision governance

New material architecture decisions use [`docs/adr/0000-template.md`](docs/adr/0000-template.md). A code change that contradicts an accepted ADR must carry or depend on a superseding ADR. ADRs must include concrete ownership, contracts, invariants, failure behavior, security/privacy/tenancy effects, migration/rollback, measurable validation evidence, alternatives, and reversal conditions.

ADR-0016 governs architecture-description viewpoints and target/as-built traceability. A stale diagram is a documentation defect; a diagram must never be used to silently override accepted product or technical contracts.
