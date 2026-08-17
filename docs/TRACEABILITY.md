# Requirements and Architecture Traceability

- Status: Normative traceability index
- Date: 2026-08-18
- Evaluated protected-main implementation baseline: `46142cdbbe5dd5e900a926b70c700adf1878088a`

This document prevents product requirements, architecture decisions, governance, code, and release evidence from drifting independently. It is intentionally explicit about what is **implemented on the evaluated protected-main baseline**, what exists only on an **active PR**, and what remains **target architecture**.

## 1. Status vocabulary

- **Implemented** — source and tests exist on the evaluated protected-main baseline.
- **Partially implemented** — a reusable domain contract exists, but transport, persistence, integration, lifecycle coverage, or a stricter governing evidence rule is incomplete.
- **Active PR** — source/evidence exists on a currently open PR but is not protected-main truth.
- **Target** — required by PRD/TRD/ADR but not implemented on the evaluated baseline.
- **External dependency** — implemented/owned in another CWL bounded context and consumed through a contract.

An active PR, architecture document, conversation decision, or scheduler plan is not protected-main implementation. A future implementation-status change must be supported by source/test/migration/contract evidence on the named protected baseline.

## 2. Product requirement traceability

| Requirement | PRD source | Technical/architecture contract | ADR(s) | Evaluated-main implementation |
|---|---|---|---|---|
| Anonymous core assessment | PRD §3.1, §9.1 | TRD §5, §10; UML anonymous sequence | ADR-0002, ADR-0003, ADR-0005 | Session lifecycle primitives implemented, including creation bound to one published locale-specific release; anonymous credential domain primitive exists, while complete public HTTP journey remains Target |
| Anonymous participant base persistence | PRD §3.1, §9.1, §9.7 | TRD §10–11; ADR-0015 persistence boundary | ADR-0003, ADR-0015, ADR-0020 | **Active PR #250** adds product-owned PostgreSQL base identity persistence with exact `participant_ref`/`tenant_ref` binding and immutable replay semantics; it is not protected-main truth until merged |
| Pause/resume | PRD §3.1, §9.1 | TRD §5 | ADR-0005 | **Implemented** in `src/session.rs` with fail-closed transitions |
| Sequence-aware item delivery evidence | PRD §3.1, §9 | TRD §5–7 | ADR-0005, ADR-0010 | **Implemented** domain primitive in `src/item_delivery.rs` plus protected-main PostgreSQL item-delivery evidence persistence; public delivery orchestration remains Target |
| Idempotent response events | PRD §9.2 | TRD §6 | ADR-0005, ADR-0010 | **Implemented** in `src/response.rs` with canonical SHA-256 payload-digest identity; immutable response-snapshot persistence exists, while complete response-event/public transport lifecycle remains separately evidence-gated |
| Immutable response snapshot before scoring | PRD §9.3 | TRD §5–8 | ADR-0005, ADR-0010 | **Implemented** domain semantics in `src/response.rs` plus protected-main PostgreSQL response-snapshot persistence |
| Version-pinned scoring | PRD §9.4, §10 | TRD §8 | ADR-0004, ADR-0010 | **Implemented** reusable product-side scoring dispatch contract in `src/scoring.rs` with canonical SHA-256 engine-artifact digest provenance plus `migrations/0011_scoring_request.sql` / `src/postgres_scoring_request.rs` request-identity persistence; live fast-mlsirm integration is Target |
| Bounded asynchronous scoring retry/quarantine with stale-worker fencing | PRD §9.4, §10 | TRD §8; ADR-0015 transaction boundary | ADR-0004, ADR-0010, ADR-0015 | **Implemented** product lifecycle plus PostgreSQL enqueue, claim, retry, completion, expiry recovery, and cancellation without transferring a fence; live fast-mlsirm execution remains Target |
| Immutable result provenance | PRD §3.1, §9.4 | TRD §9 | ADR-0004, ADR-0010 | **Implemented** in `src/result.rs` plus protected-main PostgreSQL result-snapshot persistence and result-read authorization; result-serving transport is Target |
| Deterministic narrative fallback | PRD §3.2, §9.5 | TRD §17; Architecture narrative view | ADR-0009, ADR-0010, ADR-0018 | Deterministic narrative primitives exist on protected main; full consumer-facing Personality Style mapping/transport remains separately gated |
| Continuous scores remain source of truth; Personality Style is presentation | PRD §3.2 | Measurement Governance; AI Governance | ADR-0018 | Numeric source remains External fast-mlsirm contract; presentation mapping must remain separately versioned and cannot mutate scientific scores |
| Immutable instrument release/version lifecycle | PRD §6, §9 | TRD §7; UML publication state | ADR-0005, ADR-0010 | **Implemented** in `src/instrument.rs` plus `migrations/0006_instrument_release.sql` and `src/postgres_instrument_release.rs`: immutable release manifest, exact version/digest/locale/item set, fail-closed Draft/Review/Published/Suspended/Retired lifecycle, idempotent publication events, and new-session eligibility |
| Instrument publication requires intended-use scientific/right/locale evidence | PRD §6, §9, §10 | Measurement Governance; publication evidence gate | ADR-0004, ADR-0013, ADR-0019 | **Implemented** policy gate and immutable evidence provenance in `src/instrument.rs`; each real instrument still requires its own rights/locale/scientific evidence artifacts before publication |
| Optional Keyverse account linking | PRD §3.1, §9.7 | TRD §10; UML identity-link lifecycle | ADR-0003, ADR-0020 | **Partially implemented**: issuer-scoped first-link fail-closed domain primitive in `src/participant.rs`; base identity persistence is **Active PR #250**; append-only unlink/relink/recovery history persistence, audit, and transport remain Target |
| Cross-cutting tenant/task authorization | PRD §7, §9 | TRD §11; Security/Data | ADR-0001, ADR-0003 | **Implemented** fail-closed domain gates bind sensitive result, data-rights, and consent reads/actions to tenant/resource/owner context; broader policy-adapter/public-transport integration remains Target |
| Purpose-specific consent | PRD §5, §9.6 | TRD §12 | ADR-0006 | **Implemented** domain contract in `src/consent.rs` plus `migrations/0005_consent_lifecycle.sql` / `src/postgres_consent.rs` purpose-specific ledgers; HTTP transport remains Target |
| Explicit research contribution + withdrawal | PRD §5 | TRD §12, §14–15 | ADR-0006, ADR-0007 | **Implemented** product-domain lifecycle in `src/consent.rs`; dataset snapshot/release integration is Target |
| Participant export/deletion | PRD §3.1, §9, §11 | TRD §13 | ADR-0006 | **Implemented** domain lifecycle in `src/data_rights.rs` plus protected-main data-rights persistence, identity-verification, authorization, and processing-start evidence; dependent-system execution remains Target |
| Research identity separation | PRD §5, §11 | TRD §14; ERD restricted linkage | ADR-0003, ADR-0006, ADR-0007, ADR-0020 | Partially implemented via research-contribution identity separation; restricted linkage persistence is Target |
| Research release manifests | PRD §5 | TRD §15 | ADR-0007, ADR-0010 | Product-side research-release evidence gate exists in `src/research_release.rs`; semantic-data-portal registration and release staging remain External/Target integration |
| Durable outbox/inbox delivery semantics | PRD §7, §9 | TRD §19–20 | ADR-0014, ADR-0015 | **Partially implemented**: domain contracts in `src/integration.rs`; PostgreSQL 18 outbox/inbox identity, delivery-attempt persistence, exclusive outbox lease fencing, and inbox consumption/expiry guard are on protected main; live side-effect execution remains Target |
| Operation-scoped capability health | PRD §7, §13 | `docs/OPERABILITY.md` §3–4; Deployment/Operations | ADR-0011, ADR-0017 | **Implemented** domain health/readiness contract in `src/health.rs` plus `src/postgres_health.rs` PostgreSQL major/write-readiness and caller-declared relation presence; HTTP probes, measured thresholds, and deployment evidence remain Target |
| Korean/English exact locale versions | PRD §3.1, §9.9 | TRD §28; instrument release + locale governance | ADR-0013, ADR-0019 | **Partially implemented**: locale is pinned/validated by `src/instrument.rs`; actual English/Korean form content, rights, translation, invariance and serving are Target |
| WCAG 2.2 AA supported reference client | PRD §9.10 | TRD §27; Quality Attributes | ADR-0002, ADR-0013 | Target; no supported reference client implementation on evaluated main |
| EMA/ESM longitudinal flow | PRD §4 | TRD §16; UML longitudinal sequence; logical ERD extension | ADR-0008 | **Partially implemented**: `src/longitudinal_observation.rs` preserves validity/recorded/received/ingested clocks and explicit membership shares on protected main; Gyeot/TEPP remain External dependencies and enrollment persistence/orchestration transport remains Target |
| Measurement Workbench | PRD §6 | C4/component view; UML publication-evidence sequence; Measurement Governance | ADR-0001, ADR-0002, ADR-0004, ADR-0019 | Target; fast-mlsirm/Inkspan/RankWeave are External dependencies |
| Headless replaceable clients | PRD §7 | TRD §1, §18; C4 | ADR-0001, ADR-0002 | Architecture established; public transport is Target |
| Community/Hosted/Enterprise profiles | PRD §7, §13 | TRD deployment sections; Deployment/Operations | ADR-0011, ADR-0017 | Target deployment packaging/evidence |

## 3. Technical invariant traceability

| Invariant | Source | Enforcement/evidence on evaluated main | Missing evidence before GA |
|---|---|---|---|
| Server-authoritative session state | TRD §5 | `src/session.rs` + session contract tests, including published-release/locale binding at creation | persistence/API concurrency test |
| Only Active accepts responses | TRD §5–6 | `SessionState::accepts_responses` + response tests | transport-level rejection test |
| Item delivery sequence is positive and evidence-safe | TRD §5–7 | `src/item_delivery.rs` plus protected-main item-delivery persistence/tests | public API integration |
| Conflicting idempotency replay fails closed | TRD §6 | `src/response.rs` and durable snapshot/recovery evidence | durable response-event concurrency/public-route integration |
| Snapshot requires Completed state | TRD §5–6 | `src/response.rs` plus PostgreSQL response-snapshot persistence | full session/response transaction integration |
| Scoring uses durable snapshot identity | TRD §8 | `src/scoring.rs` requires a canonical SHA-256 engine-artifact digest and durable scoring-request identity | live adapter + retry/outbox integration |
| Stale scoring worker cannot complete a newer attempt | TRD §8; ADR-0015 | `src/scoring_job.rs` uses monotonically increasing fencing tokens and rejects stale/expired completion or failure evidence; `src/postgres_scoring_job.rs` persists enqueue, claim, retry, terminal outcomes, expired-lease recovery, and cancellation without transferring a fence | live adapter evidence |
| Scientific failure is typed, no invented score | TRD §8; Measurement Governance | scoring contract tests | cross-process failure injection |
| Historical result does not mutate | TRD §9 | `src/result.rs` snapshot semantics plus PostgreSQL result-snapshot persistence | public API supersession tests |
| Narrative cannot mutate score / deterministic fallback exists | AI Governance; ADR-0018 | deterministic narrative primitives and architecture policy | complete Personality Style mapping + client acceptance |
| Instrument release bytes/version/item order are immutable | TRD §7 | `src/instrument.rs` + publication contract tests; `src/postgres_instrument_release.rs` persists immutable manifest columns | API publication integration |
| Only Published release accepts new sessions | TRD §7 | `PublicationState::accepts_new_sessions` in `src/instrument.rs`; `AssessmentSession` creation copies exact published release/version/locale provenance and fails closed on unpublished eligibility or locale mismatch | session-creation persistence/API integration test |
| Publication event replay is idempotent/conflicting reuse fails closed | TRD §7 | `src/instrument.rs` | durable publication-event history/API integration |
| Published instrument requires exact-version scientific evidence | Measurement Governance; ADR-0019 | `src/instrument.rs` binds approved evidence status, provenance/scope, mandatory evidence references, validity window, and immutable release identity before publication/reactivation | persistence/API publication integration and real instrument-specific evidence artifacts |
| Optional account linking does not rewrite historical participant/result identity | ADR-0003, ADR-0020 | `src/participant.rs` issuer-scoped first-link primitive preserves stable participant ID; **Active PR #250** persists only the anonymous base identity without Keyverse subject data | append-only identity-link persistence + unlink/relink/recovery audit tests |
| Sensitive authorization is tenant- and task-bound | TRD §11; Security/Data | fail-closed authorization contexts/gates for consent, data rights, and results | policy adapter + route/repository integration + cross-tenant E2E tests |
| Research consent separate from service consent | TRD §12; Research Governance | `src/consent.rs` | public API/UI negative test |
| Research withdrawal preserves evidence | TRD §12–15; Research Governance | `src/consent.rs` | release-pipeline exclusion test |
| Export/deletion requires request-specific identity verification | TRD §13 | data-rights domain + PostgreSQL request/verification/processing evidence and authorization guard | Keyverse/account/anonymous public transport integration |
| Legal retention represented explicitly | TRD §13 | `src/data_rights.rs` partial completion | dependency execution/restore tests after local propagation |
| No cross-service DB access | TRD §1–2; ADR-0015 | architecture policy only | deployment credential/fitness-function test |
| Initial physical persistence target is upstream PostgreSQL 18.x | ADR-0015; Deployment/Operations | **Implemented subsets** in checked-in migrations `0001`, `0002`, `0003`, `0004`, `0005`, `0006`, `0007`, `0010`, `0011`, `0012`, `0013`, `0015`, `0018`, `0019`, their owning adapters, and PostgreSQL operational-store readiness | remaining product aggregates, full crash/restart journey acceptance; participant base is **Active PR #250** via `0030` |
| No default tenant for writes | TRD §11; Security/Data | authorization-domain primitive exists; tenant-bound persistence exists for multiple aggregates | public-route tenant-negative tests and remaining aggregates |
| Tenant-bound transactional outbox/inbox | TRD §19–20; ADR-0014/0015 | `src/integration.rs` domain envelope/inbox/retry contracts plus PostgreSQL tenant/source-scoped integration evidence, delivery-attempt persistence, outbox lease fencing, inbox consumption, and claim-expiry guards | live adapter crash/retry/side-effect tests |
| Inbox receipt is not side-effect completion | ADR-0014/0015; UML integration sequence | `src/integration.rs` states/retry semantics; PostgreSQL inbox consumption persists pending/processing/completed and expire-and-reclaim | live adapter side-effect integration |
| Liveness is distinct from operation readiness | Operability §3–4; ADR-0017 | **Implemented** in `src/health.rs` and `src/postgres_health.rs`: liveness is modeled independently from operation-scoped readiness and PostgreSQL write-readiness | live transport probes, metrics, and deployment-profile acceptance |
| Optional capability outage does not fail unrelated work | Operability §3–4; ADR-0011/0017 | **Implemented** in `src/health.rs` and `src/postgres_health.rs`: readiness evaluates only capabilities required by the selected operation and maps PostgreSQL evidence onto that contract | degraded-mode transport/integration tests |
| Unknown/stalled backlog or unknown/incompatible integrity blocks new state-changing work | Operability §3, §6, §8 | **Implemented** domain contract in `src/health.rs`; `src/postgres_health.rs` fails closed on unsupported/read-only PostgreSQL or a missing required relation | persistence/job backlog metrics, stronger schema probes, alerting, and failure-injection evidence |
| Longitudinal clocks preserve event/provenance time rather than receipt-only time | TRD §16; ADR-0008 | `src/longitudinal_observation.rs` preserves validity, recorded, received, and ingested clocks plus source identity | durable enrollment/ingestion persistence and Gyeot transport |
| Multiple-membership longitudinal evidence is explicit | TRD §16; ADR-0008 | `src/longitudinal_observation.rs` stores explicit membership shares rather than one forced primary group | durable persistence and TEPP handoff acceptance |
| No operational IDs in public research release | TRD §14–15; Research Governance | product-side release-evidence gate + architecture policy | release fixture/static/runtime leakage tests |
| AI optional; deterministic core remains | PRD §9.5; TRD §17; AI Governance | deterministic product primitives + architecture policy | narrative end-to-end transport/client test |
| AI cannot mutate numeric scientific result | AI Governance; ADR-0009, ADR-0018 | architecture policy and immutable result domain | product AI adapter/adversarial mutation tests |
| Exact locale no silent assessment fallback | TRD §28; ADR-0013 | instrument locale pinning exists; client serving policy is Target | exact English/Korean published-form/client tests |
| GA claims require measured profile recovery/availability evidence | ADR-0017; Deployment/Operations | architecture policy | deployed SLO/RPO/RTO/restore/incident evidence |
| Architecture mitigation is not risk closure/certification | Compliance Readiness; Risk Register | documentation fitness only | control-specific implementation and scoped independent assessment where claimed |

## 4. Source module map

Current protected-main Rust module surface on `46142cdbbe5dd5e900a926b70c700adf1878088a`:

```text
src/lib.rs
├── account_link.rs
├── anonymous_credential.rs
├── anonymous_session.rs
├── authorization.rs
├── consent.rs
├── data_rights.rs
├── data_rights_authorization.rs
├── deterministic_narrative.rs
├── health.rs
├── instrument.rs
├── integration.rs
├── item_delivery.rs
├── longitudinal_observation.rs
├── narrative.rs
├── participant.rs
├── postgres_consent.rs
├── postgres_data_rights.rs
├── postgres_data_rights_processing.rs
├── postgres_health.rs
├── postgres_inbox_consumption.rs
├── postgres_instrument_release.rs
├── postgres_integration.rs
├── postgres_item_delivery.rs
├── postgres_response_snapshot.rs
├── postgres_result_snapshot.rs
├── postgres_scoring_job.rs
├── postgres_scoring_request.rs
├── reference.rs
├── research_release.rs
├── response.rs
├── result.rs
├── result_authorization.rs
├── scoring.rs
├── scoring_job.rs
└── session.rs

migrations/
├── 0001_integration_delivery.sql
├── 0002_scoring_job_state.sql
├── 0003_data_rights_propagation.sql
├── 0004_item_delivery_evidence.sql
├── 0005_consent_lifecycle.sql
├── 0006_instrument_release.sql
├── 0007_result_snapshot.sql
├── 0010_response_snapshot.sql
├── 0011_scoring_request.sql
├── 0012_integration_consumption.sql
├── 0013_outbox_delivery_lease.sql
├── 0015_data_rights_identity_verification.sql
├── 0018_data_rights_processing_start.sql
└── 0019_inbox_claim_expiry_guard.sql
```

Still-Target logical modules/adapters include remaining aggregate persistence/repositories, public/admin HTTP and event transports, live fast-mlsirm/Keyverse/Gyeot/TEPP/semantic-data-portal adapters, research-release staging, complete Personality Style presentation mapping, longitudinal enrollment persistence/orchestration, participant identity-link history persistence, runtime health transports/metrics, reference-client product experience, and Measurement Workbench orchestration.

### Active implementation work that is not protected-main truth

**Active PR #250** anonymous participant-base persistence (`migrations/0030_assessment_participant.sql`, `src/postgres_participant.rs`) is not protected-main truth until an unchanged reviewed/check-clean head is integrated. It persists only opaque product `participant_ref`, exact `tenant_ref`, and server-authoritative creation time, classifies exact replay under `READ COMMITTED`, rejects tenant/time rebinding, and reloads only through the exact participant-and-tenant pair. Optional Keyverse link history remains a separate append-only concern.

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
| API/event representation and event integrity | ADR-0014 |
| PostgreSQL persistence/transaction boundaries | ADR-0015 |
| Architecture views/traceability | ADR-0016 |
| Operational recovery/GA evidence | ADR-0017 |
| Continuous score / narrative separation | ADR-0018 |
| Scientific publication evidence gate | ADR-0019 |
| Append-only participant identity-link history | ADR-0020 |

## 6. Governance and evidence artifact traceability

| Concern | Authoritative artifact | Evidence status on evaluated baseline |
|---|---|---|
| Product intent | `docs/PRD.md` | Protected-main normative product baseline |
| Technical contract | `docs/TRD.md` | Protected-main normative technical baseline; incomplete transports/deployment remain implementation-gated |
| Measurement/scientific publication | `docs/MEASUREMENT_GOVERNANCE.md` | Protected-main governance; numerical implementation remains fast-mlsirm-owned |
| Continuous score/narrative interpretation | ADR-0018 + `docs/AI_GOVERNANCE.md` | Numeric result/deterministic primitives exist; complete presentation mapping remains implementation-gated |
| Instrument scientific publication gate | ADR-0019 + `docs/MEASUREMENT_GOVERNANCE.md` | **Implemented policy gate:** `src/instrument.rs` requires exact release-bound approved evidence provenance; instrument-specific rights/locale/scientific artifacts remain release evidence inputs rather than shipped core content |
| AI/judge/provider authority | `docs/AI_GOVERNANCE.md` | Protected-main governance; target adapters remain unimplemented |
| Research contribution/release | `docs/RESEARCH_GOVERNANCE.md` | Protected-main governance; product-side consent/release-evidence primitives exist, while staging/catalog integration remains Target |
| Nonfunctional measurable scenarios | `docs/QUALITY_ATTRIBUTES.md` | Protected-main evidence contract; scenarios become verified only as implementations exist |
| Assurance readiness | `docs/COMPLIANCE_READINESS.md` | Architecture-defined only; no SOC 2/CSAP external attestation/certification claimed |
| Material risk | `docs/RISK_REGISTER.md` | Architecture/evidence-state register; individual risks remain open until evidence/accepted risk |
| Canonical terms | `docs/GLOSSARY.md` | Protected-main terminology baseline |
| Architecture views | `docs/architecture/*` | Normative target/mixed views; as-built claims require the named implementation evidence map |
| Implementation status | this document | Named evaluated-main baseline plus explicitly segregated Active PR work |
| Delivery dependency order | `docs/ROADMAP.md` | Protected-main delivery baseline |

## 7. Whole-conversation reconciliation gate

The durable product architecture is **Scientific Trait Core + Accessible Narrative + Reflective Capacities + Longitudinal Context + Open Science**, expressed to users as **Measure → Understand → Reflect → Observe Over Time → Contribute to Science**.

The first consumer family is IPIP Big Five. Continuous/facet scores and uncertainty remain the scientific source of truth; Personality Style is a separately versioned deterministic presentation mapping and cannot be represented as MBTI equivalence. Self-compassion and future reflective constructs are independently measured instruments, never inferred from Big Five. Anonymous participation is first-class; Keyverse account linking is optional and append-only. Research contribution is a separate purpose-specific opt-in, with operational and research identity namespaces separated. Gyeot owns EMA/ESM collection, TEPP owns temporal/event/multilevel/multiple-membership analytics, and this product owns consented normalized ingestion/orchestration rather than duplicating either kernel. AI is bounded and cannot mutate numeric scores, calibration, norms, DIF, uncertainty, or scientific publication gates. The Measurement Workbench reuses fast-mlsirm scientific contracts and Inkspan/RankWeave capabilities rather than copying their kernels.

Whenever a durable conversation decision changes one of those boundaries, the appropriate PRD/TRD/ADR/architecture/governance artifact must be reconciled before an implementation can be treated as architecture-compliant.

## 8. Machine-readable contract gate

The prose API/event families in TRD are architecture requirements, not evidence of an implemented transport.

When an HTTP API family is implemented, the same PR or a prerequisite PR must add or reconcile an OpenAPI 3.2.x document whose operations and problem responses match the actual implementation. HTTP errors use RFC 9457 problem details unless a documented domain representation is more appropriate.

When durable message transport is implemented, the same PR or a prerequisite PR must add and validate an AsyncAPI 3.1.x document for actually produced/consumed event channels and message schemas. It must encode/reference ADR-0014 canonical UTF-8 payload hashing, SHA-256 payload digest semantics, tenant/resource binding, deduplication identity, pending/processing/completed consumption, replay retention, and quarantine behavior.

A machine-readable contract may not list unimplemented operations as if they were available. Target/future contracts, if needed, must be clearly marked non-deployed and cannot satisfy release acceptance.

## 9. Traceability maintenance gate

A PR that materially changes any of the following must update this document or prove no traceability change is needed:

- domain module ownership;
- lifecycle states/transitions;
- public/admin API family;
- event family/integrity/idempotency semantics;
- persistent logical entity or relationship;
- scientific publication or score interpretation rule;
- AI/judge/provider authority;
- research contribution/release/access rule;
- cross-service dependency;
- security/privacy trust boundary;
- database support/transaction semantics;
- quality-attribute/recovery claim;
- material risk/evidence state;
- consumer/research acceptance criterion;
- deployment profile/recovery contract.
