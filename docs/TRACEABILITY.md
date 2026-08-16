# Requirements and Architecture Traceability

- Status: Normative traceability index
- Date: 2026-08-14
- Evaluated protected-main implementation baseline: `085ef4b4714796a77fd4645eeb46b028f95929fc`

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
| Anonymous core assessment | PRD §3.1, §9.1 | TRD §5, §10; UML anonymous sequence | ADR-0002, ADR-0003, ADR-0005 | Session lifecycle primitives implemented, including creation bound to one published locale-specific release; anonymous credential/HTTP flow is Target |
| Pause/resume | PRD §3.1, §9.1 | TRD §5 | ADR-0005 | **Implemented** in `src/session.rs` with fail-closed transitions |
| Sequence-aware item delivery evidence | PRD §3.1, §9 | TRD §5–7 | ADR-0005, ADR-0010 | **Implemented** domain primitive in `src/item_delivery.rs`; persistence/API delivery orchestration is Target |
| Idempotent response events | PRD §9.2 | TRD §6 | ADR-0005, ADR-0010 | **Implemented** in `src/response.rs` with canonical SHA-256 payload-digest identity; persistence adapter is Target |
| Immutable response snapshot before scoring | PRD §9.3 | TRD §5–8 | ADR-0005, ADR-0010 | **Implemented** domain semantics in `src/response.rs` |
| Version-pinned scoring | PRD §9.4, §10 | TRD §8 | ADR-0004, ADR-0010 | **Implemented** reusable product-side scoring dispatch contract in `src/scoring.rs` with canonical SHA-256 engine-artifact digest provenance plus `migrations/0011_scoring_request.sql` / `src/postgres_scoring_request.rs` request-identity persistence; live fast-mlsirm integration is Target |
| Bounded asynchronous scoring retry/quarantine with stale-worker fencing | PRD §9.4, §10 | TRD §8; ADR-0015 transaction boundary | ADR-0004, ADR-0010, ADR-0015 | **Implemented** product lifecycle plus PostgreSQL enqueue, claim, retry, completion, expiry recovery, and cancellation without transferring a fence; live fast-mlsirm execution remains Target |
| Immutable result provenance | PRD §3.1, §9.4 | TRD §9 | ADR-0004, ADR-0010 | **Implemented** in `src/result.rs`; result-serving transport is Target |
| Deterministic narrative fallback | PRD §3.2, §9.5 | TRD §17; Architecture narrative view | ADR-0009, ADR-0010, ADR-0018 | Target |
| Continuous scores remain source of truth; Personality Style is presentation | PRD §3.2 | Measurement Governance; AI Governance | ADR-0018 | Target product narrative mapping; numeric source remains External fast-mlsirm contract |
| Immutable instrument release/version lifecycle | PRD §6, §9 | TRD §7; UML publication state | ADR-0005, ADR-0010 | **Implemented** in `src/instrument.rs` plus `migrations/0006_instrument_release.sql` and `src/postgres_instrument_release.rs`: immutable release manifest, exact version/digest/locale/item set, fail-closed Draft/Review/Published/Suspended/Retired lifecycle, idempotent publication events, and new-session eligibility |
| Instrument publication requires intended-use scientific/right/locale evidence | PRD §6, §9, §10 | Measurement Governance; publication evidence gate | ADR-0004, ADR-0013, ADR-0019 | **Implemented** policy gate and immutable evidence provenance in `src/instrument.rs`; each real instrument still requires its own rights/locale/scientific evidence artifacts before publication |
| Optional Keyverse account linking | PRD §3.1, §9.7 | TRD §10; UML identity-link lifecycle | ADR-0003, ADR-0020 | **Partially implemented**: issuer-scoped first-link fail-closed domain primitive in `src/participant.rs`; append-only unlink/relink/recovery history, persistence, audit, and transport remain Target |
| Cross-cutting tenant/task authorization | PRD §7, §9 | TRD §11; Security/Data | ADR-0001, ADR-0003 | **Implemented** fail-closed domain gate in `src/authorization.rs` binds consent operations to participant-owned `ConsentLedger` / `ManageOwnConsent`; persistence/policy-adapter/public-transport integration remains Target |
| Purpose-specific consent | PRD §5, §9.6 | TRD §12 | ADR-0006 | **Implemented** domain contract in `src/consent.rs` plus `migrations/0005_consent_lifecycle.sql` / `src/postgres_consent.rs` purpose-specific ledgers; HTTP transport remains Target |
| Explicit research contribution + withdrawal | PRD §5 | TRD §12, §14–15 | ADR-0006, ADR-0007 | **Implemented** product-domain lifecycle in `src/consent.rs`; dataset snapshot/release integration is Target |
| Participant export/deletion | PRD §3.1, §9, §11 | TRD §13 | ADR-0006 | **Implemented** domain lifecycle in `src/data_rights.rs` plus `migrations/0003_data_rights_propagation.sql` and `src/postgres_data_rights.rs`; dependent-system execution remains Target |
| Research identity separation | PRD §5, §11 | TRD §14; ERD restricted linkage | ADR-0003, ADR-0006, ADR-0007, ADR-0020 | Partially implemented via research-contribution identity separation; restricted linkage persistence is Target |
| Research release manifests | PRD §5 | TRD §15 | ADR-0007, ADR-0010 | Target; semantic-data-portal is External dependency |
| Durable outbox/inbox delivery semantics | PRD §7, §9 | TRD §19–20 | ADR-0014, ADR-0015 | **Partially implemented**: domain contracts in `src/integration.rs`; PostgreSQL 18 outbox/inbox identity, delivery-attempt persistence, and inbox consumption distinct from receipt; live side-effect execution remains Target |
| Operation-scoped capability health | PRD §7, §13 | `docs/OPERABILITY.md` §3–4; Deployment/Operations | ADR-0011, ADR-0017 | **Implemented** domain health/readiness contract in `src/health.rs` plus `src/postgres_health.rs` PostgreSQL major/write-readiness and caller-declared relation presence; HTTP probes, measured thresholds, and deployment evidence remain Target |
| Korean/English exact locale versions | PRD §3.1, §9.9 | TRD §28; instrument release + locale governance | ADR-0013, ADR-0019 | **Partially implemented**: locale is pinned/validated by `src/instrument.rs`; actual English/Korean form content, rights, translation, invariance and serving are Target |
| WCAG 2.2 AA supported reference client | PRD §9.10 | TRD §27; Quality Attributes | ADR-0002, ADR-0013 | Target; no reference client implementation on evaluated main |
| EMA/ESM longitudinal flow | PRD §4 | TRD §16; UML longitudinal sequence; logical ERD extension | ADR-0008 | External Gyeot/TEPP dependencies + Target Commons enrollment/normalized-ingestion/orchestration adapter |
| Measurement Workbench | PRD §6 | C4/component view; UML publication-evidence sequence; Measurement Governance | ADR-0001, ADR-0002, ADR-0004, ADR-0019 | Target; fast-mlsirm/Inkspan/RankWeave are External dependencies |
| Headless replaceable clients | PRD §7 | TRD §1, §18; C4 | ADR-0001, ADR-0002 | Architecture established; public transport is Target |
| Community/Hosted/Enterprise profiles | PRD §7, §13 | TRD deployment sections; Deployment/Operations | ADR-0011, ADR-0017 | Target deployment packaging/evidence |

## 3. Technical invariant traceability

| Invariant | Source | Enforcement/evidence on evaluated main | Missing evidence before GA |
|---|---|---|---|
| Server-authoritative session state | TRD §5 | `src/session.rs` + session contract tests, including published-release/locale binding at creation | **Active PR** #164 persist, load, in-memory start, and stored-release start created-session identity with exact/conflicting replay; HTTP/API concurrency remains missing |
| Only Active accepts responses | TRD §5–6 | `SessionState::accepts_responses` + response tests | transport-level rejection test |
| Item delivery sequence is positive and evidence-safe | TRD §5–7 | `src/item_delivery.rs` + item-delivery domain tests | durable uniqueness/order/API integration |
| Conflicting idempotency replay fails closed | TRD §6 | `src/response.rs` | DB uniqueness/concurrency test |
| Snapshot requires Completed state | TRD §5–6 | `src/response.rs` | transaction atomicity test with persistence |
| Scoring uses durable snapshot identity | TRD §8 | `src/scoring.rs` requires a canonical SHA-256 engine-artifact digest | live adapter + retry/outbox integration |
| Stale scoring worker cannot complete a newer attempt | TRD §8; ADR-0015 | `src/scoring_job.rs` uses monotonically increasing fencing tokens and rejects stale/expired completion or failure evidence; `src/postgres_scoring_job.rs` persists enqueue, claim, retry, terminal outcomes, expired-lease recovery, and cancellation without transferring a fence | live adapter evidence |
| Scientific failure is typed, no invented score | TRD §8; Measurement Governance | scoring contract tests | cross-process failure injection |
| Historical result does not mutate | TRD §9 | `src/result.rs` snapshot semantics | persistence and API supersession tests |
| Narrative cannot mutate score / deterministic fallback exists | AI Governance; ADR-0018 | architecture policy | mapping implementation + canonical style-assignment key + fallback/no-score-mutation tests |
| Instrument release bytes/version/item order are immutable | TRD §7 | `src/instrument.rs` + publication contract tests; `src/postgres_instrument_release.rs` persists immutable manifest columns | API publication integration |
| Only Published release accepts new sessions | TRD §7 | `PublicationState::accepts_new_sessions` in `src/instrument.rs`; `AssessmentSession` creation copies exact published release/version/locale provenance and fails closed on unpublished eligibility or locale mismatch | **Active PR** #164 start composition (`created_session_for_start`) plus stored-release start (`start_created_assessment_session_from_stored_release`); persist/load without re-checking current eligibility; HTTP session-creation remains missing |
| Publication event replay is idempotent/conflicting reuse fails closed | TRD §7 | `src/instrument.rs` | durable DB uniqueness/concurrency test |
| Published instrument requires exact-version scientific evidence | Measurement Governance; ADR-0019 | `src/instrument.rs` binds approved evidence status, provenance/scope, mandatory evidence references, validity window, and immutable release identity before publication/reactivation | persistence/API publication integration and real instrument-specific evidence artifacts |
| Optional account linking does not rewrite historical participant/result identity | ADR-0003, ADR-0020 | `src/participant.rs` issuer-scoped first-link primitive preserves stable participant ID | append-only identity-link persistence + unlink/relink/recovery audit tests |
| Sensitive authorization is tenant- and task-bound | TRD §11; Security/Data | `src/authorization.rs` fail-closed authorization context/gates bind consent operations to participant ownership | policy adapter + route/repository integration + cross-tenant E2E tests |
| Research consent separate from service consent | TRD §12; Research Governance | `src/consent.rs` | public API/UI negative test |
| Research withdrawal preserves evidence | TRD §12–15; Research Governance | `src/consent.rs` | release-pipeline exclusion test |
| Export/deletion requires request-specific identity verification | TRD §13 | `src/data_rights.rs`; `src/postgres_data_rights.rs` persists the requested identity and local propagation events | Keyverse/account/anonymous transport integration |
| Legal retention represented explicitly | TRD §13 | `src/data_rights.rs` partial completion | dependency execution/restore tests after local propagation |
| No cross-service DB access | TRD §1–2; ADR-0015 | architecture policy only | deployment credential/fitness-function test |
| Initial physical persistence target is upstream PostgreSQL 18.x | ADR-0015; Deployment/Operations | **Implemented subset** in `migrations/0001_integration_delivery.sql`, `migrations/0002_scoring_job_state.sql`, `migrations/0003_data_rights_propagation.sql`, `migrations/0005_consent_lifecycle.sql`, `migrations/0006_instrument_release.sql`, `migrations/0011_scoring_request.sql`, `migrations/0012_integration_consumption.sql`, matching adapters, and PostgreSQL operational-store readiness | remaining product aggregates, crash/restart restore acceptance |
| No default tenant for writes | TRD §11; Security/Data | authorization-domain primitive exists; persistence remains Target | persistence/API tenant negative tests |
| Tenant-bound transactional outbox/inbox | TRD §19–20; ADR-0014/0015 | `src/integration.rs` domain envelope/inbox/retry contracts plus PostgreSQL tenant/source-scoped integration evidence, delivery-attempt persistence, and inbox consumption | durable side-effect processing completion, poison-message/crash recovery, broader aggregate transaction integration |
| Inbox receipt is not side-effect completion | ADR-0014/0015; UML integration sequence | `src/integration.rs` states/retry semantics; PostgreSQL inbox consumption persists pending/processing/completed and expire-and-reclaim | live adapter crash/retry tests |
| Liveness is distinct from operation readiness | Operability §3–4; ADR-0017 | **Implemented** in `src/health.rs` and `src/postgres_health.rs`: liveness is modeled independently from operation-scoped readiness and PostgreSQL write-readiness | live transport probes, metrics, and deployment-profile acceptance |
| Optional capability outage does not fail unrelated work | Operability §3–4; ADR-0011/0017 | **Implemented** in `src/health.rs` and `src/postgres_health.rs`: readiness evaluates only capabilities required by the selected operation and maps PostgreSQL evidence onto that contract | degraded-mode transport/integration tests |
| Unknown/stalled backlog or unknown/incompatible integrity blocks new state-changing work | Operability §3, §6, §8 | **Implemented** domain contract in `src/health.rs`; `src/postgres_health.rs` fails closed on unsupported/read-only PostgreSQL or a missing required relation | persistence/job backlog metrics, stronger schema probes, alerting, and failure-injection evidence |
| No operational IDs in public research release | TRD §14–15; Research Governance | architecture policy | release fixture/static/runtime leakage tests |
| AI optional; deterministic core remains | PRD §9.5; TRD §17; AI Governance | architecture policy | narrative fallback end-to-end test |
| AI cannot mutate numeric scientific result | AI Governance; ADR-0009, ADR-0018 | architecture policy | product adapter/adversarial mutation tests |
| Exact locale no silent assessment fallback | TRD §28; ADR-0013 | instrument locale pinning exists; client serving policy is Target | exact English/Korean published-form/client tests |
| GA claims require measured profile recovery/availability evidence | ADR-0017; Deployment/Operations | architecture policy | deployed SLO/RPO/RTO/restore/incident evidence |
| Architecture mitigation is not risk closure/certification | Compliance Readiness; Risk Register | documentation fitness only | control-specific implementation and scoped independent assessment where claimed |

## 4. Source module map

Current protected-main Rust module surface on `085ef4b4714796a77fd4645eeb46b028f95929fc`:

```text
src/lib.rs
├── authorization.rs  # fail-closed tenant/task authorization context and gates
├── consent.rs        # purpose-specific consent + research contribution lifecycle
├── data_rights.rs    # export/deletion lifecycle and retention evidence
├── health.rs         # operation-scoped liveness/readiness and capability-state contract
├── instrument.rs     # immutable release manifest + scientific publication-evidence gate
├── integration.rs    # outbox/inbox/retry/quarantine domain contracts
├── item_delivery.rs  # sequence-aware delivery evidence without confidential response data
├── narrative.rs      # deterministic Personality Style identity/key
├── participant.rs    # stable participant identity + issuer-scoped optional Keyverse account link
├── postgres_consent.rs  # PostgreSQL purpose-specific consent ledger persistence
├── postgres_data_rights.rs  # PostgreSQL data-rights request and local propagation persistence
├── postgres_health.rs  # PostgreSQL major/write-readiness and relation-integrity probe
├── postgres_inbox_consumption.rs  # PostgreSQL inbox consumption distinct from receipt
├── postgres_instrument_release.rs  # PostgreSQL locale-specific instrument-release persistence
├── postgres_integration.rs  # PostgreSQL integration evidence/delivery-attempt persistence adapter
├── postgres_scoring_job.rs  # PostgreSQL scoring enqueue/claim/retry/cancel/terminal persistence
├── postgres_scoring_request.rs  # PostgreSQL version-pinned scoring-request identity
├── reference.rs      # internal opaque-reference normalization
├── research_release.rs  # product-side Research Commons release-evidence gate
├── response.rs       # idempotent response ledger + immutable response snapshots
├── result.rs         # immutable result provenance/supersession
├── scoring.rs        # version-pinned scoring dispatch contract
├── scoring_job.rs    # bounded retry/quarantine lifecycle with lease fencing
└── session.rs        # server-authoritative assessment-session transitions bound to a published locale release

migrations/
├── 0001_integration_delivery.sql
├── 0002_scoring_job_state.sql
├── 0003_data_rights_propagation.sql
├── 0005_consent_lifecycle.sql
├── 0006_instrument_release.sql
├── 0011_scoring_request.sql
└── 0012_integration_consumption.sql
```

Still-Target logical modules/adapters include remaining product aggregate persistence/repositories, public/admin HTTP and event transports, live fast-mlsirm/Keyverse/Gyeot/TEPP/semantic-data-portal adapters, research-release staging, deterministic narrative mapping, longitudinal normalized ingestion, participant identity-link history persistence, runtime health transports/metrics, and Measurement Workbench orchestration. Active PR #164 adds `src/postgres_assessment_session.rs` and `migrations/0014_assessment_session.sql`; those files are not protected-main truth.

### Active implementation work that is not protected-main truth

**Active PR** #164 created-session persist, load, in-memory start, and stored-release start composition is not protected-main truth until an unchanged reviewed/check-clean head is integrated. Created sessions persist participant, published-release, version, digest, locale, state, and creation-time identity under `READ COMMITTED`; exact replay is idempotent and rebinding fails closed. Load restores that created identity without re-checking current publication eligibility. New sessions start through `created_session_for_start` / `start_created_assessment_session`, which call `AssessmentSession::new` and then persist, or through `start_created_assessment_session_from_stored_release`, which loads the stored published release in the same transaction. HTTP session transport and later-state command history remain outside this slice. #138 is the in-memory-start predecessor; #98 is the load-only predecessor; #121 is the uncovered-start predecessor; #109 is the uncovered load predecessor; #106 is the persist-only predecessor.

**Active PR** #196 additionally locks `instrument_release` with `SELECT … FOR UPDATE` during persist classification so a Duplicate published result cannot lose the row to a concurrent Suspend or Retire before the caller transaction ends. That lock is not protected-main truth. Prefer #180 for locked stored-release *start*; this slice is the persist-side publication lock. #164 remains the unlocked stored-release start predecessor.

**Active PR** #76 data-rights processing-start persistence is not protected-main truth until an unchanged reviewed/check-clean head is integrated. Identity-verified requests persist an immutable operation identity and processing-start time under `FOR UPDATE` so later lifecycle composition cannot race the classified row. Dependent-system execution remains outside this slice.

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
| Technical contract | `docs/TRD.md` | Protected-main normative technical baseline; transport/persistence evidence remains implementation-gated |
| Measurement/scientific publication | `docs/MEASUREMENT_GOVERNANCE.md` | Protected-main governance; numerical implementation remains fast-mlsirm-owned |
| Continuous score/narrative interpretation | ADR-0018 + `docs/AI_GOVERNANCE.md` | Target product mapping/fallback; numeric result domain exists but narrative mapping does not |
| Instrument scientific publication gate | ADR-0019 + `docs/MEASUREMENT_GOVERNANCE.md` | **Implemented policy gate:** `src/instrument.rs` requires exact release-bound approved evidence provenance; instrument-specific rights/locale/scientific artifacts remain release evidence inputs rather than shipped core content |
| AI/judge/provider authority | `docs/AI_GOVERNANCE.md` | Protected-main governance; target adapters remain unimplemented |
| Research contribution/release | `docs/RESEARCH_GOVERNANCE.md` | Protected-main governance; partial domain lifecycle exists in `src/consent.rs` |
| Nonfunctional measurable scenarios | `docs/QUALITY_ATTRIBUTES.md` | Protected-main evidence contract; scenarios become verified only as implementations exist |
| Assurance readiness | `docs/COMPLIANCE_READINESS.md` | Architecture-defined only; no SOC 2/CSAP external attestation/certification claimed |
| Material risk | `docs/RISK_REGISTER.md` | Architecture/evidence-state register; individual risks remain open until evidence/accepted risk |
| Canonical terms | `docs/GLOSSARY.md` | Protected-main terminology baseline |
| Architecture views | `docs/architecture/*` | Normative target/mixed views; not as-built proof |
| Implementation status | this document | Named evaluated-main baseline plus explicitly segregated Active PR work |
| Delivery dependency order | `docs/ROADMAP.md` | Protected-main delivery baseline |

## 7. Whole-conversation reconciliation gate

The durable product architecture is **Scientific Trait Core + Accessible Narrative + Reflective Capacities + Longitudinal Context + Open Science**, expressed to users as **Measure → Understand → Reflect → Observe Over Time → Contribute to Science**.

The first consumer family is IPIP Big Five. Continuous/facet scores and uncertainty remain the scientific source of truth; Personality Style is a separately versioned deterministic presentation mapping and cannot be represented as MBTI equivalence. Self-compassion and future reflective constructs are independently measured instruments, never inferred from Big Five. Anonymous participation is first-class; Keyverse account linking is optional and append-only. Research contribution is a separate purpose-specific opt-in, with operational and research identity namespaces separated. Gyeot owns EMA/ESM collection, TEPP owns temporal/event/multilevel/multiple-membership analytics, and this product owns consented normalized ingestion/orchestration rather than duplicating either kernel. AI is bounded and cannot mutate numeric scores, calibration, norms, DIF, uncertainty, or scientific publication gates. The Measurement Workbench reuses fast-mlsirm scientific contracts and Inkspan/RankWeave capabilities rather than copying their kernels.

Whenever a durable conversation decision changes one of those boundaries, the appropriate PRD/TRD/ADR/architecture/governance artifact must be reconciled before an implementation can be treated as architecture-compliant.

## 8. Machine-readable contract gate

The prose API/event families in TRD are architecture requirements, not evidence of an implemented transport.

When the first HTTP API is implemented, the same PR or a prerequisite PR must add and validate an OpenAPI 3.2.x document whose operations and problem responses match the actual implementation. HTTP errors use RFC 9457 problem details unless a documented domain representation is more appropriate.

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

CI should validate linked documentation paths and status/name consistency now and, when machine-readable contracts/migrations exist, validate that documented references map to real contract/schema artifacts.

## 10. References

Nottingham, M., Wilde, E., & Dalal, S. (2023). *Problem Details for HTTP APIs* (RFC 9457). Internet Engineering Task Force. https://doi.org/10.17487/RFC9457

OpenAPI Initiative. (2025). *OpenAPI Specification, Version 3.2.0*.

AsyncAPI Initiative. (2026). *AsyncAPI Specification, Version 3.1.0*.
