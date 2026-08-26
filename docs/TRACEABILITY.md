# Requirements and Architecture Traceability

- Status: Normative traceability index
- Date: 2026-08-27
- Evaluated protected-main implementation baseline: `09534ef52c9307ce0dc559e9d908ebd715c641a1`

This document prevents product requirements, architecture decisions, governance, code, active pull requests, and release evidence from drifting independently. Protected-main truth is evaluated only at the exact baseline above. Active PRs are named separately and never count as shipped behavior.

## 1. Status vocabulary

- **Implemented** — source and tests exist on the evaluated protected-main baseline.
- **Partially implemented** — a reusable domain or persistence contract exists, but transport, integration, lifecycle coverage, or governing evidence remains incomplete.
- **Active PR** — source/evidence exists on a currently open PR but is not protected-main truth.
- **Target** — required by PRD/TRD/ADR but not implemented on the evaluated protected-main baseline.
- **External dependency** — owned in another CWL bounded context and consumed through a versioned contract.

An active PR, architecture document, conversation decision, or scheduler plan is not protected-main implementation.

## 2. Product requirement traceability

| Requirement | Product/technical source | ADR(s) | Evidence on evaluated protected main or active work |
|---|---|---|---|
| Anonymous core assessment | PRD §3.1, §9.1; TRD §5, §10 | ADR-0002, ADR-0003, ADR-0005 | **Partially implemented**: anonymous credential/session/authorization and persist-backed assessment-session create/reload contracts exist. Persist/reload of `assessment_participant` remains Target. |
| Server-authoritative pause/resume lifecycle | PRD §3.1, §9.1; TRD §5 | ADR-0005 | **Implemented** in `src/session.rs`; lifecycle transitions fail closed. |
| Public participant session commands | PRD §3.1, §9.1; TRD §5, §18; `openapi/session-commands.yaml` | ADR-0005, ADR-0014 | **Active PR #414** exposes `POST /v1/sessions/{session_ref}/commands` for participant `activate`, `pause`, `resume`, `complete`, and `cancel`; exact client idempotency, server-issued sequence, strict request framing, server-verified participant authority, and RFC 9457 failures are active-PR evidence only, not protected-main truth. |
| Sequence-aware item delivery | PRD §3.1, §9; TRD §5–7 | ADR-0005, ADR-0010 | **Implemented** domain/persistence evidence in `src/item_delivery.rs` and `src/postgres_item_delivery.rs`; public delivery orchestration remains Target. |
| Idempotent response events | PRD §9.2; TRD §6 | ADR-0005, ADR-0010, ADR-0014 | **Implemented** domain evidence in `src/response.rs`; durable accepted-event persistence and public response transport are not yet protected-main truth. **Active PR #415** is the current response-HTTP reconciliation lane. |
| Immutable response snapshot before scoring | PRD §9.3; TRD §5–8 | ADR-0005, ADR-0010 | **Implemented** domain and PostgreSQL snapshot persistence in `src/response.rs` and `src/postgres_response_snapshot.rs`. |
| Version-pinned scoring | PRD §9.4, §10; TRD §8 | ADR-0004, ADR-0010 | **Implemented** product-side dispatch/persistence and request-bound scoring-engine contract in `src/scoring.rs`, `src/postgres_scoring_request.rs`, and `src/scoring_engine.rs`; live fast-mlsirm execution remains an **External dependency/Target integration**. |
| Bounded async scoring retry/quarantine | PRD §9.4, §10; TRD §8 | ADR-0004, ADR-0010, ADR-0015 | **Implemented** lifecycle and PostgreSQL lease/fencing persistence in `src/scoring_job.rs` and `src/postgres_scoring_job.rs`; live engine worker evidence remains Target. |
| Immutable result provenance and personal export | PRD §3.1, §9.4; TRD §9, §18 | ADR-0003, ADR-0010 | **Implemented** immutable result/export domain and protected-main authorized export transport. Result-serving and persistence evidence remain implementation-gated by their current protected-main modules/tests. |
| Deterministic narrative fallback / Personality Style separation | PRD §3.2, §9.5; AI Governance | ADR-0009, ADR-0010, ADR-0018 | **Partially implemented** narrative/style domain contracts exist; continuous/facet scores remain scientific truth. Personality Style is presentation only and is not MBTI equivalence. |
| Immutable instrument publication | PRD §6, §9; TRD §7 | ADR-0004, ADR-0005, ADR-0010, ADR-0019 | **Implemented** immutable release/version/item/locale identity plus scientific-publication evidence gate and PostgreSQL persistence. Real instrument releases still require rights, locale/translation, calibration/norm and applicable invariance/DIF evidence. |
| Quick/Deep assessment paths | PRD §3.1, §9; TRD §5–7 | ADR-0005, ADR-0010 | **Implemented** release-bound path contract in `src/assessment_path.rs`; complete public delivery/conversion/scoring integration remains Target. |
| Optional Keyverse account linking | PRD §3.1, §9.7; TRD §10 | ADR-0003, ADR-0020 | **Partially implemented** stable participant/first-link domain evidence in `src/participant.rs` and append-only account-link evidence in `src/account_link.rs`; full unlink/relink/recovery persistence/transport remains Target. |
| Purpose-specific consent and research opt-in | PRD §5, §9.6; TRD §12 | ADR-0006 | **Implemented** domain and PostgreSQL consent ledger. Research consent remains separate from service use; HTTP execution remains Target. |
| Participant export/deletion | PRD §3.1, §9, §11; TRD §13 | ADR-0006 | **Implemented** local data-rights lifecycle/persistence and immutable terminal/retained-scope evidence; dependent-system execution remains Target. |
| Research identity separation and public release gate | PRD §5, §11; TRD §14–15 | ADR-0003, ADR-0006, ADR-0007, ADR-0020 | **Partially implemented** research-release domain evidence exists. **Active PR #409** is the current public-release privacy-gate reconciliation and is not protected-main truth. semantic-data-portal remains the **External dependency** for immutable public research catalog/release registration. |
| Durable outbox/inbox semantics | PRD §7, §9; TRD §19–20 | ADR-0014, ADR-0015 | **Partially implemented** in `src/integration.rs`, `src/integration_delivery.rs`, `src/integration_publisher.rs`, `src/postgres_integration.rs`, and `src/postgres_inbox_consumption.rs`; live side-effect worker execution remains Target. |
| Korean/English exact locale versions | PRD §3.1, §9.9; TRD §28 | ADR-0013, ADR-0019 | **Partially implemented** exact locale pinning/presentation exists; real form rights/translation/linking/invariance/DIF and accessible client evidence remain release-specific gates. |
| WCAG 2.2 AA reference client | PRD §9.10; TRD §27 | ADR-0002, ADR-0013 | **Target**; no GA accessibility claim without supported-client evidence. |
| EMA/ESM longitudinal flow | PRD §4; TRD §16 | ADR-0008 | **Implemented** Commons normalized observation domain and PostgreSQL 18 persistence in `src/longitudinal_observation.rs`, `src/postgres_longitudinal_observation.rs`, and `migrations/0028_longitudinal_observation.sql`, including immutable timing/source identity and multiple-membership share evidence. Gyeot collection and TEPP temporal/multilevel/multiple-membership analysis remain **External dependencies**; enrollment and end-to-end orchestration remain Target. |
| Measurement Workbench | PRD §6; Measurement Governance | ADR-0001, ADR-0002, ADR-0004, ADR-0019 | **Target** orchestration that must reuse fast-mlsirm AssessmentSpec/Rubric/Blueprint/scoring/calibration evidence and Inkspan/RankWeave capabilities without copying scientific kernels. |
| Community/Hosted/Enterprise profiles | PRD §7, §13 | ADR-0011, ADR-0017 | **Target** deployment packaging and measured recovery/availability evidence. |

## 3. Technical invariant traceability

| Invariant | Enforcement/evidence | Missing evidence before GA |
|---|---|---|
| Server-authoritative session state | `src/session.rs` and lifecycle tests; protected-main persist-backed session create/reload | **Active PR #414** adds participant command HTTP but is not shipped; tenant isolation and complete item/response/scoring transport remain incomplete. |
| Only Active accepts responses | `SessionState::accepts_responses` plus response-domain tests | Protected-main public response transport acceptance; #415 is active work only. |
| Client replay identity cannot rewind lifecycle state | Domain replay contracts in `src/session.rs`; #414 adds public command replay tests | #414 must reach protected main unchanged after all gates/review. |
| Response idempotency conflict fails closed | `src/response.rs` canonical payload-digest/idempotency contract | Durable concurrency and protected-main public transport evidence. |
| Snapshot requires Completed | `src/response.rs` plus PostgreSQL snapshot persistence | Atomic end-to-end completion/snapshot/scoring acceptance. |
| Scientific failure is typed; no invented score | `src/scoring.rs`, `src/scoring_engine.rs`, scoring tests | Live fast-mlsirm failure injection/parity evidence. |
| Historical result does not mutate | `src/result.rs`, result persistence/export tests | Full authorization/tenant HTTP E2E. |
| Narrative cannot mutate numeric score | ADR-0018 + AI Governance + deterministic narrative contracts | End-to-end published mapping/fallback evidence. |
| Published instrument alone starts new sessions | `src/instrument.rs` and persist-backed session creation | **Active PR #414** command transport is not protected-main truth; remaining assessment transport and tenant acceptance stay open. |
| Exact locale; no silent assessment fallback | Instrument release locale pinning and localized result presentation | Real Korean/English form, linking/invariance/DIF and accessible serving evidence. |
| No cross-service application database access | ADR-0001/0015 architecture boundary | Deployment credential/fitness-function evidence. |
| No default tenant for writes | `src/authorization.rs` task/tenant domain gate | Persistence/API cross-tenant negative E2E. |
| Research consent is separate from service consent | `src/consent.rs` | Public API/UI negative test and release-pipeline withdrawal exclusion. |
| Public research fixture excludes operational identity | Research Governance and `src/research_release.rs` | #409 must pass unchanged current-head gates/review before it can become protected-main evidence. |
| Liveness differs from operation readiness | `src/health.rs`, `src/postgres_health.rs` | Live probes, metrics and deployment-profile evidence. |
| GA claims require measured recovery/availability | ADR-0017; Operability; Release Acceptance | Deployed restore/failure-injection/SLO/RPO/RTO evidence; documentation alone is not proof. |

## 4. Protected-main source map

The evaluated baseline contains the following representative owned surfaces; this is evidence discovery, not a claim that every product journey is complete:

```text
src/lib.rs
├── account_link.rs
├── anonymous_authorization.rs
├── anonymous_credential.rs
├── anonymous_session.rs
├── api_problem.rs
├── assessment_path.rs
├── authorization.rs
├── consent.rs
├── data_rights.rs
├── deterministic_narrative.rs
├── health.rs
├── instrument.rs
├── integration.rs
├── integration_delivery.rs
├── integration_publisher.rs
├── item_delivery.rs
├── longitudinal_observation.rs
├── participant.rs
├── postgres_assessment_session.rs
├── postgres_consent.rs
├── postgres_data_rights.rs
├── postgres_health.rs
├── postgres_inbox_consumption.rs
├── postgres_instrument_release.rs
├── postgres_integration.rs
├── postgres_item_delivery.rs
├── postgres_longitudinal_observation.rs
├── postgres_response_snapshot.rs
├── postgres_result_snapshot.rs
├── postgres_scoring_job.rs
├── postgres_scoring_request.rs
├── response.rs
├── result.rs
├── result_authorization.rs
├── result_export.rs
├── result_export_authorization.rs
├── scoring.rs
├── scoring_engine.rs
├── scoring_job.rs
├── session.rs
└── session_http.rs
```

Persist/reload of `assessment_participant` remains Target. Remaining Target adapters include complete public/admin assessment transport, live fast-mlsirm/Keyverse/Gyeot/TEPP/semantic-data-portal composition, research staging/release registration, Measurement Workbench orchestration, reference-client delivery, and profile-level operations/recovery evidence.

### Active implementation work that is not protected-main truth

**Active PR** #414 participant session commands are not protected-main truth. The PR adds the bounded public `POST /v1/sessions/{session_ref}/commands` family, strict HTTP/1.1 framing, opaque singleton `Idempotency-Key`, exact replay, server-issued sequence, participant-only command allowlisting, RFC 9457 errors, OpenAPI 3.2.0 contract evidence, wire/persistence vocabulary parity tests, and server-verified exact participant/session authority for authenticated or current anonymous credentials. Scoring/operator commands remain outside this public route.

**Active PR** #415 response-write HTTP is not protected-main truth. It is the current lane for `POST /v1/sessions/{session_ref}/responses`; its hardened socket boundary and response idempotency tests do not count as shipped behavior before integration.

**Active PR** #409 public research-release privacy gating is not protected-main truth. Its exact-head privacy scanner/inventory behavior remains acceptance-gated.

**Active PR** #411 consent durable-reference Unicode parity is not protected-main truth. Its PostgreSQL validator change remains acceptance-gated.

**Active PR** #403 Runtime-CI runner-demand serialization is not product truth and is not acceptance evidence for another PR. It preserves the existing check identities while reducing peak hosted-runner demand; its own exact-head checks remain authoritative.

## 5. ADR traceability by concern

| Concern | Governing ADR(s) |
|---|---|
| Product repository / bounded contexts | ADR-0001 |
| Headless client model | ADR-0002 |
| Keyverse / anonymous participation | ADR-0003 |
| fast-mlsirm scientific source of truth | ADR-0004 |
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

| Concern | Authoritative artifact | Evidence status |
|---|---|---|
| Product intent | `docs/PRD.md` | Normative product baseline |
| Technical contract | `docs/TRD.md` | Normative technical baseline; implementation remains evidence-gated |
| Measurement/scientific publication | `docs/MEASUREMENT_GOVERNANCE.md` | Governance; numerical kernels remain fast-mlsirm-owned |
| AI/judge/provider authority | `docs/AI_GOVERNANCE.md` | Bounded optional AI; no scientific numeric mutation authority |
| Research contribution/release | `docs/RESEARCH_GOVERNANCE.md` | Purpose-specific contribution/withdrawal and public-release governance |
| Security/privacy | `docs/THREAT_MODEL.md`, `docs/architecture/SECURITY_AND_DATA.md` | Architecture/control requirements; not certification evidence |
| Testing | `docs/TEST_STRATEGY.md` | Exact-head realistic test and scientific-evidence expectations |
| Operations/recovery | `docs/OPERABILITY.md`, `docs/architecture/DEPLOYMENT_AND_OPERATIONS.md` | Evidence-gated; no fabricated SLO/RPO/RTO |
| Release authority | `docs/RELEASE_ACCEPTANCE.md` | Separate software, instrument and research release gates |
| Quality attributes | `docs/QUALITY_ATTRIBUTES.md` | Measurable target scenarios; verified only with evidence |
| Risk/compliance | `docs/RISK_REGISTER.md`, `docs/COMPLIANCE_READINESS.md` | Readiness/risk tracking; no certification claim |
| Canonical terms | `docs/GLOSSARY.md` | Vocabulary baseline |
| Architecture views | `docs/architecture/*` | Target/mixed views; labels distinguish as-built from planned |
| Implementation status | this document | Exact protected-main baseline plus explicitly segregated active work |
| Delivery dependency order | `docs/ROADMAP.md` | Product backlog ordering |

## 7. Durable product boundary reconciliation

The durable product architecture is **Scientific Trait Core + Accessible Narrative + Reflective Capacities + Longitudinal Context + Open Science**, expressed as **Measure → Understand → Reflect → Observe Over Time → Contribute to Science**.

The first consumer family is IPIP Big Five. Continuous/facet scores and uncertainty remain the scientific source of truth. Personality Style is a separately versioned deterministic presentation mapping and cannot be represented as MBTI equivalence. Self-compassion and future-reflection constructs are independently measured instruments, never inferred from Big Five. Anonymous participation is first-class; Keyverse linking is optional and append-only. Research contribution is a separate purpose-specific opt-in. Operational and research identity namespaces are separated. Gyeot owns EMA/ESM collection; TEPP owns temporal/event/multilevel/multiple-membership analytics; this repository owns consented enrollment/normalized ingestion/reference orchestration, not duplicate kernels. AI is bounded and cannot mutate numeric scores, calibration, norms, DIF, uncertainty, or scientific gates. The Measurement Workbench must reuse fast-mlsirm, Inkspan, and RankWeave contracts/capabilities rather than copy their kernels.

## 8. Machine-readable contract gate

Prose API/event families are architecture requirements, not implementation evidence. An implemented HTTP family must have a matching OpenAPI 3.2.x contract and RFC 9457-compatible problem behavior where appropriate. A durable message transport must have an implementation-matched AsyncAPI 3.1.x contract encoding the accepted ADR-0014/0015 integrity, tenant/resource binding, deduplication, replay, lifecycle, and quarantine semantics. Machine-readable contracts must not advertise unimplemented operations as deployed.

## 9. Traceability maintenance gate

A PR that materially changes domain ownership, lifecycle states/transitions, a public/admin API family, event/idempotency semantics, persistence relationships, scientific publication/interpretation rules, AI authority, research access/release, cross-service dependencies, security/privacy boundaries, database/transaction semantics, quality/recovery claims, acceptance criteria, or deployment profiles must update this document or prove the mapping is unchanged.

CI should validate documentation paths/status vocabulary and, where real transports/migrations exist, validate that documented contract/schema references resolve to real artifacts.

## 10. References

Nottingham, M., Wilde, E., & Dalal, S. (2023). *Problem Details for HTTP APIs* (RFC 9457). Internet Engineering Task Force. https://doi.org/10.17487/RFC9457

OpenAPI Initiative. (2025). *OpenAPI Specification, Version 3.2.0*.

AsyncAPI Initiative. (2026). *AsyncAPI Specification, Version 3.1.0*.
