# Requirements and Architecture Traceability

- Status: Normative traceability index
- Date: 2026-08-26
- Evaluated protected-main implementation baseline: `5f0a5346d60602d4bdfbca526d125f9504d594d3`

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
| Anonymous core assessment | PRD §3.1, §9.1 | TRD §5, §10; UML anonymous sequence | ADR-0002, ADR-0003, ADR-0005 | Session lifecycle primitives and persist-backed `POST /v1/sessions` / `GET /v1/sessions/{session_ref}` are **Implemented**; anonymous credential/public authority flow is still Target |
| Pause/resume | PRD §3.1, §9.1 | TRD §5 | ADR-0005 | **Implemented** in `src/session.rs` with fail-closed transitions; public command HTTP is separate active work |
| Sequence-aware item delivery evidence | PRD §3.1, §9 | TRD §5–7 | ADR-0005, ADR-0010 | **Implemented** domain primitive in `src/item_delivery.rs`; durable/API delivery orchestration remains incomplete |
| Idempotent response events | PRD §9.2 | TRD §6 | ADR-0005, ADR-0010, ADR-0014 | **Implemented** domain semantics in `src/response.rs`; durable accepted-event persistence is Active PR #284; public response-write HTTP and `openapi/responses.yaml` are Active PR #415 |
| Immutable response snapshot before scoring | PRD §9.3 | TRD §5–8 | ADR-0005, ADR-0010 | **Implemented** domain semantics in `src/response.rs` plus protected-main immutable snapshot persistence |
| Version-pinned scoring | PRD §9.4, §10 | TRD §8 | ADR-0004, ADR-0010 | **Implemented** reusable product-side scoring dispatch contract in `src/scoring.rs` with canonical SHA-256 engine-artifact digest provenance, PostgreSQL request identity, and request-bound external adapter `src/scoring_engine.rs`; live fast-mlsirm execution remains Target |
| Bounded asynchronous scoring retry/quarantine with stale-worker fencing | PRD §9.4, §10 | TRD §8; ADR-0015 transaction boundary | ADR-0004, ADR-0010, ADR-0015 | **Implemented** product lifecycle plus PostgreSQL enqueue, claim, retry, completion, expiry recovery, and cancellation without transferring a fence; live fast-mlsirm execution remains Target |
| Immutable result provenance | PRD §3.1, §9.4 | TRD §9 | ADR-0004, ADR-0010 | **Implemented** in `src/result.rs` with protected-main immutable result persistence and authorized read/export boundaries |
| Authorized immutable personal result read | PRD §3.1, §9.4 | TRD §18 `GET /v1/results/{result_ref}` | ADR-0003, ADR-0010 | **Implemented** through merged #257: `src/result_http.rs` checks server-owned participant/result authorization before route identity and returns immutable score/provenance evidence without recomputation |
| Personal JSON and human-readable result export | PRD §3.1, §9.4 | TRD §18 `POST /v1/results/{result_ref}/exports` | ADR-0010 | **Implemented** domain copy through #231, authorization guard through #249, and protected-main HTTP transport through #256 (`src/result_export_http.rs`, `openapi/result-exports.yaml`) |
| Deterministic narrative fallback | PRD §3.2, §9.5 | TRD §17; Architecture narrative view | ADR-0009, ADR-0010, ADR-0018 | **Partially implemented** deterministic product narrative primitives are on protected main, including canonical published rule-reference hardening through merged #287; full participant journey/client integration remains Target |
| Continuous scores remain source of truth; Personality Style is presentation | PRD §3.2 | Measurement Governance; AI Governance | ADR-0018 | Scientific numeric source remains External fast-mlsirm contract; product narrative mapping is separately versioned and cannot mutate scores |
| Immutable instrument release/version lifecycle | PRD §6, §9 | TRD §7; UML publication state | ADR-0005, ADR-0010 | **Implemented** in `src/instrument.rs` plus PostgreSQL release persistence: immutable release manifest, exact version/digest/locale/item set, fail-closed Draft/Review/Published/Suspended/Retired lifecycle, publication replay semantics, and new-session eligibility |
| Quick and Deep assessment paths | PRD §3.1, §9 | TRD §5–7 | ADR-0005, ADR-0010 | **Partially implemented** through merged #261: `src/assessment_path.rs` binds Quick/Deep ordered item subsets to one immutable release and versioned policy; persistence, participant delivery, conversion, and scoring orchestration remain Target |
| Instrument publication requires intended-use scientific/right/locale evidence | PRD §6, §9, §10 | Measurement Governance | ADR-0004, ADR-0013, ADR-0019 | **Implemented** policy gate and immutable evidence provenance in `src/instrument.rs`; each real instrument still requires its own rights/locale/scientific evidence artifacts before publication |
| Optional Keyverse account linking | PRD §3.1, §9.7 | TRD §10; UML identity-link lifecycle | ADR-0003, ADR-0020 | **Partially implemented** issuer-scoped first-link primitive in `src/participant.rs`; append-only link-history persistence, unlink/relink/recovery audit, and transport remain Target |
| Cross-cutting tenant/task authorization | PRD §7, §9 | TRD §11; Security/Data | ADR-0001, ADR-0003 | **Implemented** fail-closed domain gate in `src/authorization.rs`; broader persistence/policy-adapter/public-transport integration remains incomplete |
| Purpose-specific consent | PRD §5, §9.6 | TRD §12 | ADR-0006 | **Implemented** domain contract plus PostgreSQL purpose-specific consent ledger persistence; HTTP transport remains Target |
| Explicit research contribution + withdrawal | PRD §5 | TRD §12, §14–15 | ADR-0006, ADR-0007 | **Implemented** product-domain lifecycle in `src/consent.rs`; restricted linkage/release staging and external registration remain separate work |
| Participant export/deletion | PRD §3.1, §9, §11 | TRD §13 | ADR-0006 | **Implemented** domain lifecycle and PostgreSQL request/propagation evidence; merged #77 persists immutable terminal completion and retained-scope evidence without claiming external dependency execution has completed |
| Research identity separation | PRD §5, §11 | TRD §14; ERD restricted linkage | ADR-0003, ADR-0006, ADR-0007, ADR-0020 | **Partially implemented** through research-contribution identity separation; current Active PR #409 adds a bounded public-release privacy gate but is not protected-main truth |
| Research release manifests | PRD §5 | TRD §15 | ADR-0007, ADR-0010 | Product release-evidence primitives are partial; semantic-data-portal is the External dependency for immutable public catalog/release registration |
| Durable outbox/inbox delivery semantics | PRD §7, §9 | TRD §19–20 | ADR-0014, ADR-0015 | **Partially implemented**: domain contracts, PostgreSQL outbox/inbox identity, delivery-attempt persistence, pending/processing/completed inbox consumption, and exact publisher acknowledgement handoff exist; live external side-effect worker/broker acceptance remains Target |
| Operation-scoped capability health | PRD §7, §13 | `docs/OPERABILITY.md` §3–4 | ADR-0011, ADR-0017 | **Implemented** domain health/readiness and PostgreSQL major/write-readiness/relation probes; HTTP probes, measured thresholds, and deployment evidence remain Target |
| Korean/English exact locale versions | PRD §3.1, §9.9 | TRD §28 | ADR-0013, ADR-0019 | **Partially implemented**: locale is pinned/validated by `src/instrument.rs`; protected main ships exact `ko-KR`/`en-US` participant report labels; real instrument content, rights, translation, linking/invariance evidence, and accessible client serving remain Target |
| WCAG 2.2 AA supported reference client | PRD §9.10 | TRD §27; Quality Attributes | ADR-0002, ADR-0013 | Target; no supported reference client implementation/evidence on evaluated main |
| EMA/ESM longitudinal flow | PRD §4 | TRD §16; UML longitudinal sequence | ADR-0008 | External Gyeot/TEPP dependencies + Commons normalized observation primitives; enrollment persistence/orchestration and live Gyeot/TEPP adapters remain Target |
| Measurement Workbench | PRD §6 | C4/component view; Measurement Governance | ADR-0001, ADR-0002, ADR-0004, ADR-0019 | Target; fast-mlsirm/Inkspan/RankWeave are External dependencies |
| Headless replaceable clients | PRD §7 | TRD §1, §18; C4 | ADR-0001, ADR-0002 | Protected main has several headless HTTP families; supported replaceable reference-client delivery remains Target |
| Community/Hosted/Enterprise profiles | PRD §7, §13 | TRD deployment sections | ADR-0011, ADR-0017 | Target deployment packaging/evidence |

## 3. Technical invariant traceability

| Invariant | Source | Enforcement/evidence on evaluated main | Missing evidence before GA |
|---|---|---|---|
| Server-authoritative session state | TRD §5 | `src/session.rs` plus persist-backed session HTTP create/reload bind lifecycle to one published locale-specific release | command/response/item journey integration, tenancy, and end-to-end authority evidence |
| Only Active accepts responses | TRD §5–6 | `SessionState::accepts_responses` + `src/response.rs`; **Active PR #415** adds transport-level rejection and is not protected-main truth | protected-main response-write transport after reviewed merge |
| Item delivery sequence is positive and evidence-safe | TRD §5–7 | `src/item_delivery.rs` + persistence contracts | complete API orchestration and restart journey evidence |
| Conflicting response idempotency replay fails closed | TRD §6 | `src/response.rs`; Active PR #415 maps public `Idempotency-Key` conflict without overwriting accepted evidence | durable HTTP + DB concurrency/restart evidence on one integrated head |
| Snapshot requires Completed state | TRD §5–6 | `src/response.rs` + immutable snapshot persistence | integrated transaction/restart journey from accepted responses to scoring |
| Scoring uses durable snapshot identity | TRD §8 | `src/scoring.rs`, PostgreSQL scoring request, and `src/scoring_engine.rs` request/result provenance binding | live fast-mlsirm adapter + retry/outbox integration |
| Stale scoring worker cannot complete a newer attempt | TRD §8; ADR-0015 | `src/scoring_job.rs` fencing tokens + PostgreSQL claim/retry/terminal persistence | live adapter failure injection |
| Scientific failure is typed, no invented score | TRD §8; Measurement Governance | scoring contracts plus `src/scoring_engine.rs` typed engine/request-mismatch errors | cross-process failure injection and live provider evidence |
| Historical result does not mutate | TRD §9 | `src/result.rs` + immutable PostgreSQL snapshots | integrated production recovery/read acceptance |
| Authorized result read does not expose cross-tenant existence | TRD §11, §18; Security/Data | **Implemented** through merged #257 `src/result_http.rs`, which authorizes supplied server-owned records before requested-route identity comparison | PostgreSQL resource-loading and cross-tenant hosted E2E evidence |
| Result export includes machine-readable provenance and the same scores | ADR-0010; PRD §3.1 | `src/result_export.rs`, authorization guard, and merged #256 HTTP transport copy immutable scores, SEs, dispositions, owner identity, and version provenance | hosted acceptance/client compatibility evidence |
| Narrative cannot mutate score / deterministic fallback exists | AI Governance; ADR-0018 | deterministic narrative modules and canonical published-rule-reference enforcement on protected main | complete participant-facing mapping/fallback and adversarial no-score-mutation E2E |
| Instrument release bytes/version/item order are immutable | TRD §7 | `src/instrument.rs` + `src/postgres_instrument_release.rs` | admin publication API and instrument-specific release evidence |
| Only Published release accepts new sessions | TRD §7 | publication-state guard + persist-backed session create locks stored release evidence | remaining assessment transport journey |
| Publication event replay is idempotent/conflicting reuse fails closed | TRD §7 | `src/instrument.rs` | durable API/concurrency acceptance |
| Published instrument requires exact-version scientific evidence | Measurement Governance; ADR-0019 | `src/instrument.rs` release-bound approved evidence provenance | real instrument-specific rights/locale/scientific artifacts |
| Optional account linking does not rewrite historical participant/result identity | ADR-0003, ADR-0020 | `src/participant.rs` stable participant identity and issuer-scoped first link | append-only link-history persistence + unlink/relink/recovery audit |
| Sensitive authorization is tenant- and task-bound | TRD §11; Security/Data | `src/authorization.rs` fail-closed product authorization primitives | route/store integration + cross-tenant E2E |
| Research consent separate from service consent | TRD §12; Research Governance | `src/consent.rs` | public API/UI negative acceptance |
| Research withdrawal preserves evidence | TRD §12–15; Research Governance | `src/consent.rs` | release-pipeline exclusion and withdrawal E2E |
| Export/deletion requires request-specific identity verification | TRD §13 | data-rights domain and PostgreSQL operation persistence | Keyverse/account/anonymous transport integration |
| Legal retention represented explicitly | TRD §13 | protected-main completion/retained-scope evidence through merged #77 | dependent-system execution/restore evidence |
| No cross-service DB access | TRD §1–2; ADR-0015 | architecture policy + repository-owned adapters only | deployment credential/fitness-function evidence |
| Initial physical persistence target is upstream PostgreSQL 18.x | ADR-0015; Deployment/Operations | **Implemented subset** across repository migrations/adapters and operational readiness checks | remaining aggregates and complete crash/restart acceptance |
| No default tenant for writes | TRD §11; Security/Data | authorization primitives require explicit product context | complete persistence/API tenant-negative suite |
| Tenant-bound transactional outbox/inbox | TRD §19–20; ADR-0014/0015 | `src/integration.rs` plus PostgreSQL tenant/source-scoped outbox/inbox, delivery-attempt, consumption and verified publisher handoff | external side-effect completion, poison-message recovery, aggregate transaction integration |
| Inbox receipt is not side-effect completion | ADR-0014/0015 | pending/processing/completed consumption with reclaim semantics | live adapter crash/retry acceptance |
| Liveness is distinct from operation readiness | Operability; ADR-0017 | `src/health.rs` and `src/postgres_health.rs` | live probes/metrics/deployment-profile acceptance |
| Optional capability outage does not fail unrelated work | Operability; ADR-0011/0017 | readiness evaluates only capabilities required by selected operation | degraded-mode transport/integration tests |
| Unknown/stalled backlog or incompatible integrity blocks state-changing work | Operability | domain readiness + PostgreSQL fail-closed probes | backlog metrics, stronger schema probes, alert/failure injection |
| No operational IDs in public research release | TRD §14–15; Research Governance | architecture policy on protected main; **Active PR #409** adds current-main public-release privacy scanning and inventory fail-closed behavior but is not protected-main truth | reviewed protected-main integration + release fixture acceptance |
| AI optional; deterministic core remains | PRD §9.5; TRD §17 | deterministic product primitives + AI governance | full fallback E2E |
| AI cannot mutate numeric scientific result | AI Governance; ADR-0009, ADR-0018 | score/narrative ownership separation | product adapter/adversarial mutation tests |
| Exact locale no silent assessment fallback | TRD §28; ADR-0013 | exact release locale pinning | English/Korean published-form/client tests + invariance evidence where claims cross locale |
| GA claims require measured profile recovery/availability evidence | ADR-0017 | architecture policy | deployed SLO/RPO/RTO/restore/incident evidence |
| Architecture mitigation is not risk closure/certification | Compliance Readiness; Risk Register | documentation fitness only | control-specific implementation and scoped independent assessment where claimed |

## 4. Source module map

Current protected-main Rust module surface is evaluated at `5f0a5346d60602d4bdfbca526d125f9504d594d3`. Representative product modules include:

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
├── data_rights_authorization.rs
├── deterministic_narrative.rs
├── health.rs
├── instrument.rs
├── integration.rs
├── integration_delivery.rs
├── integration_publisher.rs
├── item_delivery.rs
├── localized_result_report.rs
├── longitudinal_observation.rs
├── narrative.rs
├── participant.rs
├── postgres_assessment_session.rs
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
├── result_export.rs
├── result_export_authorization.rs
├── result_export_http.rs
├── result_http.rs
├── scoring.rs
├── scoring_engine.rs
├── scoring_job.rs
├── session.rs
└── session_http.rs
```

The evaluated protected main also contains repository-owned PostgreSQL migrations for integration, scoring, consent, instrument release, result/response snapshots, assessment sessions/commands, data-rights processing/completion, and related integrity/recovery evidence. This list is an orientation map, not a substitute for exact source/migration inspection.

Still-Target modules/adapters include remaining aggregate persistence/repositories, remaining public/admin transports, live fast-mlsirm/Keyverse/Gyeot/TEPP/semantic-data-portal adapters, research release staging/registration, longitudinal enrollment persistence, append-only participant identity-link history persistence, runtime health transports/metrics, supported reference clients, and Measurement Workbench orchestration. Persist/reload of `assessment_participant` remains Target.

### Active implementation work that is not protected-main truth

**Active PR** #415 response-event write HTTP is not protected-main truth. It adds `POST /v1/sessions/{session_ref}/responses`, exact `Idempotency-Key` replay semantics, authoritative `AssessmentSession` binding, `openapi/responses.yaml`, one hardened socket-framing owner in `response_http_boundary.rs`, and direct-handler UTF-8 framing failure protection. Its exact head still requires current CI/security/supply-chain evidence and qualifying independent review before merge.

**Active PR** #409 public research-release privacy reconciliation is not protected-main truth. It provides the current-main landing vehicle for the fail-closed public-fixture identity/credential leakage scanner and required restricted-identity inventory boundary. It must not be confused with superseded historical #301.

**Active PR** #284 durable accepted response-event persistence is not protected-main truth. It keeps an accepted mid-session response prefix durable across restart with replay classification, contiguous sequence recovery, immutable provenance, and fail-closed migration/reference contracts. It is independent of #415's in-process public transport and must be integrated only under current dependency/review evidence.

Other open PRs remain governed by their exact current heads, live bases, reviews, dependencies and checks. This document intentionally does not copy transient check state or promote an open branch to protected-main maturity.

Protected-main evidence relevant to these lanes includes merged #232 session create/reload HTTP, #256 result export HTTP, #257 authorized result read HTTP, #261 Quick/Deep assessment-path domain evidence, #287 deterministic narrative reference hardening, and #413 test repair for authoritative session-bound response-ledger signatures.

Merged #225 anonymous-session resource authorization compares the verified actor to supplied participant/session records and does not prove those records were store-loaded. Persist/reload of `assessment_participant` remains Target. Append-only identity-link history persistence and public anonymous authority transport remain later slices.

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
| Technical contract | `docs/TRD.md` | Protected-main normative technical baseline; transport/persistence evidence is implementation-gated |
| Measurement/scientific publication | `docs/MEASUREMENT_GOVERNANCE.md` | Protected-main governance; numerical implementation remains fast-mlsirm-owned |
| Continuous score/narrative interpretation | ADR-0018 + `docs/AI_GOVERNANCE.md` | Product separation is protected-main policy; complete participant-facing mapping remains incomplete |
| Instrument scientific publication gate | ADR-0019 + `docs/MEASUREMENT_GOVERNANCE.md` | **Implemented policy gate** in `src/instrument.rs`; real instrument rights/locale/scientific artifacts remain release evidence inputs |
| AI/judge/provider authority | `docs/AI_GOVERNANCE.md` | Protected-main governance; optional provider adapters remain evidence-gated |
| Research contribution/release | `docs/RESEARCH_GOVERNANCE.md` | Protected-main governance + partial domain lifecycle; #409 is Active PR privacy evidence only |
| Nonfunctional measurable scenarios | `docs/QUALITY_ATTRIBUTES.md` | Protected-main evidence contract; scenarios become verified only as implementations/deployments exist |
| Assurance readiness | `docs/COMPLIANCE_READINESS.md` | Architecture-defined only; no SOC 2/CSAP external attestation/certification claimed |
| Material risk | `docs/RISK_REGISTER.md` | Architecture/evidence-state register; individual risks remain open until evidence/accepted risk |
| Canonical terms | `docs/GLOSSARY.md` | Protected-main terminology baseline |
| Architecture views | `docs/architecture/*` | Normative target/mixed views; not as-built proof by themselves |
| Implementation status | this document | Named evaluated-main baseline plus segregated Active PR work |
| Delivery dependency order | `docs/ROADMAP.md` | Protected-main delivery baseline; live PR ancestry/review state remains authoritative at execution time |

## 7. Whole-conversation reconciliation gate

The durable product architecture is **Scientific Trait Core + Accessible Narrative + Reflective Capacities + Longitudinal Context + Open Science**, expressed to users as **Measure → Understand → Reflect → Observe Over Time → Contribute to Science**.

The first consumer family is IPIP Big Five. Continuous/facet scores and uncertainty remain the scientific source of truth; Personality Style is a separately versioned deterministic presentation mapping and cannot be represented as MBTI equivalence. Self-compassion and future reflective constructs are independently measured instruments, never inferred from Big Five. Anonymous participation is first-class; Keyverse account linking is optional and append-only. Research contribution is a separate purpose-specific opt-in, with operational and research identity namespaces separated. Gyeot owns EMA/ESM collection, TEPP owns temporal/event/multilevel/multiple-membership analytics, and this product owns consented normalized ingestion/orchestration rather than duplicating either kernel. AI is bounded and cannot mutate numeric scores, calibration, norms, DIF, uncertainty, or scientific publication gates. The Measurement Workbench reuses fast-mlsirm scientific contracts and Inkspan/RankWeave capabilities rather than copying their kernels.

Whenever a durable decision changes one of those boundaries, the appropriate PRD/TRD/ADR/architecture/governance artifact must be reconciled before an implementation can be treated as architecture-compliant.

## 8. Machine-readable contract gate

The prose API/event families in TRD are architecture requirements, not evidence of an implemented transport.

Protected-main HTTP families must have exact implementation-to-contract evidence for the machine-readable artifacts they ship. New HTTP families must add or update an OpenAPI 3.2.x document in the same PR or an accepted prerequisite; active PR #415 follows that rule with `openapi/responses.yaml`. HTTP errors use RFC 9457 problem details unless a documented domain representation is more appropriate.

When durable external message transport is implemented, the same PR or a prerequisite PR must add and validate an AsyncAPI 3.1.x document for actually produced/consumed channels and message schemas. It must encode/reference ADR-0014 canonical UTF-8 payload hashing, SHA-256 payload digest semantics, tenant/resource binding, deduplication identity, pending/processing/completed consumption, replay retention, and quarantine behavior.

A machine-readable contract may not list unimplemented operations as if they were available. Target/future contracts must be clearly marked non-deployed and cannot satisfy release acceptance.

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

CI should validate linked documentation paths, maturity vocabulary, live entity/state/API names, and status consistency. When machine-readable contracts or migrations exist, documentation references must map to real contract/schema artifacts rather than target-only prose.

## 10. References

Nottingham, M., Wilde, E., & Dalal, S. (2023). *Problem Details for HTTP APIs* (RFC 9457). Internet Engineering Task Force. https://doi.org/10.17487/RFC9457

OpenAPI Initiative. (2025). *OpenAPI Specification, Version 3.2.0*.

AsyncAPI Initiative. (2026). *AsyncAPI Specification, Version 3.1.0*.