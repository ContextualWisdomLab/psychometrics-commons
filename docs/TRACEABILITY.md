# Requirements and Architecture Traceability

- Status: Normative traceability index
- Date: 2026-08-09
- Evaluated protected-main implementation baseline: `8b1f410fc16ec4c867d28a1cd26c12fc495b8de5`

This document prevents product requirements, architecture decisions, governance, code, and release evidence from drifting independently. It is intentionally explicit about what is **implemented on the evaluated protected-main baseline** versus **target architecture**.

## 1. Status vocabulary

- **Implemented** — source and tests exist on the evaluated protected-main baseline.
- **Partially implemented** — a reusable domain contract exists, but transport/persistence/integration is not yet complete.
- **Target** — required by PRD/TRD/ADR but not implemented on the evaluated baseline.
- **External dependency** — implemented/owned in another CWL bounded context and consumed through a contract.

A future implementation-status change must be supported by source/test evidence; documentation does not promote a feature to Implemented by itself.

## 2. Product requirement traceability

| Requirement | PRD source | Technical/architecture contract | ADR(s) | Evaluated-main implementation |
|---|---|---|---|---|
| Anonymous core assessment | PRD §3.1, §9.1 | TRD §5, §10; UML anonymous sequence | ADR-0002, ADR-0003, ADR-0005 | Session lifecycle primitives implemented; anonymous credential/HTTP flow is Target |
| Pause/resume | PRD §3.1, §9.1 | TRD §5 | ADR-0005 | **Implemented** in `src/session.rs` with fail-closed transitions |
| Idempotent response events | PRD §9.2 | TRD §6 | ADR-0005, ADR-0010 | **Implemented** in `src/response.rs`; persistence adapter is Target |
| Immutable response snapshot before scoring | PRD §9.3 | TRD §5–8 | ADR-0005, ADR-0010 | **Implemented** domain semantics in `src/response.rs` |
| Version-pinned scoring | PRD §9.4, §10 | TRD §8 | ADR-0004, ADR-0010 | **Implemented** reusable product-side scoring dispatch contract in `src/scoring.rs`; live fast-mlsirm integration is Target |
| Immutable result provenance | PRD §3.1, §9.4 | TRD §9 | ADR-0004, ADR-0010 | **Implemented** in `src/result.rs`; result serving transport is Target |
| Deterministic narrative fallback | PRD §3.2, §9.5 | TRD §17; Architecture narrative view | ADR-0009, ADR-0010 | Target |
| Optional Keyverse linking | PRD §3.1, §9.7 | TRD §10 | ADR-0003 | External identity dependency + Target product adapter |
| Purpose-specific consent | PRD §5, §9.6 | TRD §12 | ADR-0006 | **Implemented** domain contract in `src/consent.rs`; transport/persistence is Target |
| Explicit research contribution + withdrawal | PRD §5 | TRD §12, §14–15 | ADR-0006, ADR-0007 | **Implemented** product-domain lifecycle in `src/consent.rs`; dataset snapshot/release integration is Target |
| Participant export/deletion | PRD §3.1, §9, §11 | TRD §13 | ADR-0006 | **Implemented** domain lifecycle in `src/data_rights.rs`; dependent-system execution is Target |
| Research identity separation | PRD §5, §11 | TRD §14; ERD restricted linkage | ADR-0003, ADR-0006, ADR-0007 | Partially implemented via research-contribution identity separation; restricted linkage persistence is Target |
| Research release manifests | PRD §5 | TRD §15 | ADR-0007, ADR-0010 | Target; semantic-data-portal is External dependency |
| Korean/English exact locale versions | PRD §3.1, §9.9 | TRD §28; UML/ERD instrument version | ADR-0013 | Target |
| WCAG 2.2 AA supported reference client | PRD §9.10 | TRD §27; Quality Attributes | ADR-0002, ADR-0013 | Target; no reference client implementation on evaluated main |
| EMA/ESM longitudinal flow | PRD §4 | TRD §16 | ADR-0008 | External Gyeot/TEPP dependencies + Target product ingestion adapter |
| Measurement Workbench | PRD §6 | C4/component view; Measurement Governance | ADR-0001, ADR-0002, ADR-0004 | Target; Inkspan/RankWeave are External dependencies |
| Headless replaceable clients | PRD §7 | TRD §1, §18; C4 | ADR-0001, ADR-0002 | Architecture established; public transport is Target |
| Community/Hosted/Enterprise profiles | PRD §7, §13 | TRD deployment sections; Deployment/Operations | ADR-0011, ADR-0017 | Target deployment packaging/evidence |

## 3. Technical invariant traceability

| Invariant | Source | Enforcement/evidence on evaluated main | Missing evidence before GA |
|---|---|---|---|
| Server-authoritative session state | TRD §5 | `src/session.rs` + session contract tests | persistence/API concurrency test |
| Only Active accepts responses | TRD §5–6 | `SessionState::accepts_responses` + response tests | transport-level rejection test |
| Conflicting idempotency replay fails closed | TRD §6 | `src/response.rs` | DB uniqueness/concurrency test |
| Snapshot requires Completed state | TRD §5–6 | `src/response.rs` | transaction atomicity test with persistence |
| Scoring uses durable snapshot identity | TRD §8 | `src/scoring.rs` | live adapter + retry/outbox integration |
| Scientific failure is typed, no invented score | TRD §8; Measurement Governance | scoring contract tests | cross-process failure injection |
| Historical result does not mutate | TRD §9 | `src/result.rs` snapshot semantics | persistence and API supersession tests |
| Research consent separate from service consent | TRD §12; Research Governance | `src/consent.rs` | public API/UI negative test |
| Research withdrawal preserves evidence | TRD §12–15; Research Governance | `src/consent.rs` | release-pipeline exclusion test |
| Export/deletion requires request-specific identity verification | TRD §13 | `src/data_rights.rs` | Keyverse/account/anonymous transport integration |
| Legal retention represented explicitly | TRD §13 | `src/data_rights.rs` partial completion | dependency propagation/restore tests |
| No cross-service DB access | TRD §1–2; ADR-0015 | architecture policy only | deployment credential/fitness-function test |
| No default tenant for writes | TRD §11; Security/Data | target architecture | persistence/API tenant negative tests |
| Transactional outbox/inbox | TRD §19–20; ADR-0015 | target architecture | persistence, crash, duplicate, poison-message tests |
| No operational IDs in public research release | TRD §14–15; Research Governance | architecture policy | release fixture/static/runtime leakage tests |
| AI optional; deterministic core remains | PRD §9.5; TRD §17; AI Governance | architecture policy | narrative fallback end-to-end test |
| AI cannot mutate numeric scientific result | AI Governance; ADR-0009 | architecture policy | product adapter/adversarial mutation tests |
| Exact locale no silent assessment fallback | TRD §28; ADR-0013 | architecture policy | instrument publication/client tests |
| GA claims require measured profile recovery/availability evidence | ADR-0017; Deployment/Operations | architecture policy | deployed SLO/RPO/RTO/restore/incident evidence |
| Architecture mitigation is not risk closure/certification | Compliance Readiness; Risk Register | documentation fitness only | control-specific implementation and independent assessment where claimed |

## 4. Source module map

Current protected-main Rust module surface:

```text
src/lib.rs
├── session.rs       # server-authoritative assessment-session transitions
├── response.rs      # idempotent response ledger + immutable response snapshots
├── scoring.rs       # version-pinned scoring dispatch contract
├── result.rs        # immutable result provenance/supersession
├── consent.rs       # purpose-specific consent + research contribution lifecycle
├── data_rights.rs   # export/deletion lifecycle and retention evidence
└── reference.rs     # internal opaque-reference normalization
```

The architecture expects additional logical modules (`instrument_publication`, `item_delivery`, `tenant_authorization`, `integration_outbox`, `integration_inbox`, adapters/transports/persistence). They remain Target until source and tests land on protected main. Open feature PRs are not promoted to Implemented status in this baseline merely because they are mergeable or under review.

## 5. ADR traceability by concern

| Concern | Governing ADR(s) |
|---|---|
| Product repository / bounded contexts | ADR-0001 |
| Headless client model | ADR-0002 |
| Keyverse / anonymous participation | ADR-0003 |
| fast-mlsirm source of truth | ADR-0004 |
| Runtime/session lifecycle | ADR-0005 |
| Consent, research, data rights | ADR-0006 |
| semantic-data-portal research release | ADR-0007 |
| Gyeot/TEPP longitudinal boundary | ADR-0008 |
| Bounded AI / egress | ADR-0009 |
| Versioned provenance / immutable results | ADR-0010 |
| Deployment profiles / integration | ADR-0011 |
| Legacy R exclusion | ADR-0012 |
| Multilingual/accessibility/invariance | ADR-0013 |
| API/event representation | ADR-0014 |
| Persistence/transaction boundaries | ADR-0015 |
| Architecture views/traceability | ADR-0016 |
| Operational recovery/GA evidence | ADR-0017 |

## 6. Governance and evidence artifact traceability

| Concern | Authoritative artifact | Evidence status on evaluated baseline |
|---|---|---|
| Product intent | `docs/PRD.md` | Existing protected-main document |
| Technical contract | `docs/TRD.md` | Existing protected-main document |
| Measurement/scientific publication | `docs/MEASUREMENT_GOVERNANCE.md` | Target governance added by documentation-baseline PR; numerical implementation remains fast-mlsirm-owned |
| AI/judge/provider authority | `docs/AI_GOVERNANCE.md` | Target product governance added by documentation-baseline PR |
| Research contribution/release | `docs/RESEARCH_GOVERNANCE.md` | Target governance added by documentation-baseline PR; partial domain lifecycle exists in `src/consent.rs` |
| Nonfunctional measurable scenarios | `docs/QUALITY_ATTRIBUTES.md` | Target/evidence contract; scenarios become verified only as implementations exist |
| Assurance readiness | `docs/COMPLIANCE_READINESS.md` | Architecture-defined only; no SOC 2/CSAP external attestation claimed |
| Material risk | `docs/RISK_REGISTER.md` | Architecture/evidence-state register; individual risks remain open until evidence/accepted risk |
| Canonical terms | `docs/GLOSSARY.md` | Target terminology baseline |
| Architecture views | `docs/architecture/*` | Normative target views; not as-built proof |
| Implementation status | this document | Named evaluated-main baseline only |
| Delivery dependency order | `docs/ROADMAP.md` | Target delivery baseline |

## 7. Machine-readable contract gate

The prose API/event families in TRD are architecture requirements, not evidence of an implemented transport.

When the first HTTP API is implemented, the same PR or a prerequisite PR must add and validate an OpenAPI 3.2.x document whose operations and problem responses match the actual implementation. HTTP errors use RFC 9457 problem details unless a documented domain representation is more appropriate.

When durable message transport is implemented, the same PR or a prerequisite PR must add and validate an AsyncAPI 3.1.x document for actually produced/consumed event channels and message schemas.

A machine-readable contract may not list unimplemented operations as if they were available. Target/future contracts, if needed, must be clearly marked non-deployed and cannot satisfy release acceptance.

## 8. Traceability maintenance gate

A PR that materially changes any of the following must update this document or prove no traceability change is needed:

- domain module ownership;
- lifecycle states/transitions;
- public/admin API family;
- event family;
- persistent logical entity or relationship;
- scientific publication or score interpretation rule;
- AI/judge/provider authority;
- research contribution/release/access rule;
- cross-service dependency;
- security/privacy trust boundary;
- quality-attribute/recovery claim;
- material risk/evidence state;
- consumer/research acceptance criterion;
- deployment profile/recovery contract.

CI should eventually validate all linked documentation paths and, when machine-readable contracts/migrations exist, validate that documented references map to real contract/schema artifacts.

## 9. References

Nottingham, M., Wilde, E., & Dalal, S. (2023). *Problem Details for HTTP APIs* (RFC 9457). Internet Engineering Task Force. https://doi.org/10.17487/RFC9457

OpenAPI Initiative. (2025). *OpenAPI Specification, Version 3.2.0*.

AsyncAPI Initiative. (2026). *AsyncAPI Specification, Version 3.1.0*.
