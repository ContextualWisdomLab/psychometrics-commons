# Requirements and Architecture Traceability

- Status: Normative traceability index
- Date: 2026-08-21
- Evaluated protected-main implementation baseline: `4499d9c0889c082487ddbd7fd8d0d5d18257995d`

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
| Version-pinned scoring | PRD §9.4, §10 | TRD §8 | ADR-0004, ADR-0010 | **Implemented** reusable product-side scoring dispatch contract in `src/scoring.rs` with canonical SHA-256 engine-artifact digest provenance, `migrations/0011_scoring_request.sql` / `src/postgres_scoring_request.rs` request-identity persistence, and protected-main request-bound external adapter `src/scoring_engine.rs`; live fast-mlsirm execution remains Target |
| Bounded asynchronous scoring retry/quarantine with stale-worker fencing | PRD §9.4, §10 | TRD §8; ADR-0015 transaction boundary | ADR-0004, ADR-0010, ADR-0015 | **Implemented** product lifecycle plus PostgreSQL enqueue, claim, retry, completion, expiry recovery, and cancellation without transferring a fence; live fast-mlsirm execution remains Target |
| Immutable result provenance | PRD §3.1, §9.4 | TRD §9 | ADR-0004, ADR-0010 | **Implemented** in `src/result.rs`; result-serving transport is Target |
| Personal JSON and human-readable result export | PRD §3.1, §9.4 | TRD §18 `POST /v1/results/{result_ref}/exports`; ADR-0010 export provenance | ADR-0010 | **Implemented** domain copy through merged #231, delivery guard through merged #249 (`src/result_export_authorization.rs`), and authorized HTTP transport through merged #256 (`src/result_export_http.rs`, `openapi/result-exports.yaml`) |
| Deterministic narrative fallback | PRD §3.2, §9.5 | TRD §17; Architecture narrative view | ADR-0009, ADR-0010, ADR-0018 | Target |
| Continuous scores remain source of truth; Personality Style is presentation | PRD §3.2 | Measurement Governance; AI Governance | ADR-0018 | Target product narrative mapping; numeric source remains External fast-mlsirm contract |
| Immutable instrument release/version lifecycle | PRD §6, §9 | TRD §7; UML publication state | ADR-0005, ADR-0010 | **Implemented** in `src/instrument.rs` plus `migrations/0006_instrument_release.sql` and `src/postgres_instrument_release.rs`: immutable release manifest, exact version/digest/locale/item set, fail-closed Draft/Review/Published/Suspended/Retired lifecycle, idempotent publication events, and new-session eligibility |
| Instrument publication requires intended-use scientific/right/locale evidence | PRD §6, §9, §10 | Measurement Governance; publication evidence gate | ADR-0004, ADR-0013, ADR-0019 | **Implemented** policy gate and immutable evidence provenance in `src/instrument.rs`; each real instrument still requires its own rights/locale/scientific evidence artifacts before publication |
| Optional Keyverse account linking | PRD §3.1, §9.7 | TRD §10; UML identity-link lifecycle | ADR-0003, ADR-0020 | **Partially implemented**: issuer-scoped first-link fail-closed domain primitive in `src/participant.rs`; append-only unlink/relink/recovery history, persistence, audit, and transport remain Target |
| Cross-cutting tenant/task authorization | PRD §7, §9 | TRD §11; Security/Data | ADR-0001, ADR-0003 | **Implemented** fail-closed domain gate in `src/authorization.rs` binds consent operations to participant-owned `ConsentLedger` / `ManageOwnConsent`. Active PR composes that gate through `persist_authorized_consent_ledger` and `persist_authorized_anonymous_consent_ledger` before a durable insert; low-level `persist_consent_ledger` remains an adapter-test entry. Outbox tail stays on #142. Public-transport integration remains Target |
| Purpose-specific consent | PRD §5, §9.6 | TRD §12 | ADR-0006 | **Implemented** domain contract in `src/consent.rs` plus `migrations/0005_consent_lifecycle.sql` / `src/postgres_consent.rs` purpose-specific ledgers; HTTP transport remains Target. Prefer consent/outbox landing head #142 over #134/#123/#120/#112/#70 |
| Explicit research contribution + withdrawal | PRD §5 | TRD §12, §14–15 | ADR-0006, ADR-0007 | **Implemented** product-domain lifecycle in `src/consent.rs`; dataset snapshot/release integration is Target |
| Participant export/deletion | PRD §3.1, §9, §11 | TRD §13 | ADR-0006 | **Implemented** domain lifecycle in `src/data_rights.rs` plus `migrations/0003_data_rights_propagation.sql` and `src/postgres_data_rights.rs`; merged #77 adds protected-main immutable terminal completion identity/time evidence and tenant-bound retained-scope child evidence for partially completed deletion requests; dependent-system execution remains Target |
| Research identity separation | PRD §5, §11 | TRD §14; ERD restricted linkage | ADR-0003, ADR-0006, ADR-0007, ADR-0020 | Partially implemented via research-contribution identity separation; restricted linkage persistence is Target |
| Research release manifests | PRD §5 | TRD §15 | ADR-0007, ADR-0010 | Target; semantic-data-portal is External dependency |
| Durable outbox/inbox delivery semantics | PRD §7, §9 | TRD §19–20 | ADR-0014, ADR-0015 | **Partially implemented**: domain contracts in `src/integration.rs`; PostgreSQL 18 outbox/inbox identity, delivery-attempt persistence, and inbox consumption distinct from receipt; merged #264 adds a protected-main exact-event publisher acknowledgement to fenced persistence handoff; live side-effect worker execution remains Target |
| Operation-scoped capability health | PRD §7, §13 | `docs/OPERABILITY.md` §3–4; Deployment/Operations | ADR-0011, ADR-0017 | **Implemented** domain health/readiness contract in `src/health.rs` plus `src/postgres_health.rs` PostgreSQL major/write-readiness and caller-declared relation presence; HTTP probes, measured thresholds, and deployment evidence remain Target |
| Korean/English exact locale versions | PRD §3.1, §9.9 | TRD §28; instrument release + locale governance | ADR-0013, ADR-0019 | **Partially implemented**: locale is pinned/validated by `src/instrument.rs`; merged #259 ships protected-main exact `ko-KR`/`en-US` participant report labels that copy immutable scores and provenance; real form content, rights, translation, invariance, HTTP delivery, and accessible reference-client serving remain Target |
| WCAG 2.2 AA supported reference client | PRD §9.10 | TRD §27; Quality Attributes | ADR-0002, ADR-0013 | Target; no reference client implementation on evaluated main |
| EMA/ESM longitudinal flow | PRD §4 | TRD §16; UML longitudinal sequence; logical ERD extension | ADR-0008 | External Gyeot/TEPP dependencies + Target Commons enrollment/orchestration adapter; `src/longitudinal_observation.rs` records validity, recorded, received, and ingested clocks with explicit membership shares. Enrollment state, PostgreSQL persistence, HTTP, Gyeot collection, and TEPP kernels remain Target |
| Measurement Workbench | PRD §6 | C4/component view; UML publication-evidence sequence; Measurement Governance | ADR-0001, ADR-0002, ADR-0004, ADR-0019 | Target; fast-mlsirm/Inkspan/RankWeave are External dependencies |
| Headless replaceable clients | PRD §7 | TRD §1, §18; C4 | ADR-0001, ADR-0002 | Architecture established; public transport is Target |
| Community/Hosted/Enterprise profiles | PRD §7, §13 | TRD deployment sections; Deployment/Operations | ADR-0011, ADR-0017 | Target deployment packaging/evidence |

## 3. Technical invariant traceability

| Invariant | Source | Enforcement/evidence on evaluated main | Missing evidence before GA |
|---|---|---|---|
| Server-authoritative session state | TRD §5 | `src/session.rs` + session contract tests, including published-release/locale binding at creation and protected-main persist-backed `POST /v1/sessions` / `GET /v1/sessions/{session_ref}` | Command HTTP, tenant isolation, and complete response/item flow remain missing |
| Only Active accepts responses | TRD §5–6 | `SessionState::accepts_responses` + response tests | transport-level rejection test |
| Item delivery sequence is positive and evidence-safe | TRD §5–7 | `src/item_delivery.rs` + item-delivery domain tests | durable uniqueness/order/API integration |
| Conflicting idempotency replay fails closed | TRD §6 | `src/response.rs` | DB uniqueness/concurrency test |
| Snapshot requires Completed state | TRD §5–6 | `src/response.rs` | transaction atomicity test with persistence |
| Scoring uses durable snapshot identity | TRD §8 | `src/scoring.rs` requires a canonical SHA-256 engine-artifact digest and `src/scoring_engine.rs` rejects a result that does not match the complete dispatched request | live fast-mlsirm adapter + retry/outbox integration |
| Stale scoring worker cannot complete a newer attempt | TRD §8; ADR-0015 | `src/scoring_job.rs` uses monotonically increasing fencing tokens and rejects stale/expired completion or failure evidence; `src/postgres_scoring_job.rs` persists enqueue, named claim, claim-next poll, retry, terminal outcomes, expired-lease recovery, and cancellation without transferring a fence | live adapter evidence |
| Scientific failure is typed, no invented score | TRD §8; Measurement Governance | scoring contract tests plus `src/scoring_engine.rs` typed engine/request-mismatch errors | cross-process failure injection and live provider evidence |
| Historical result does not mutate | TRD §9 | `src/result.rs` snapshot semantics | persistence and API supersession tests |
| Result export includes machine-readable provenance and the same scores | ADR-0010; PRD §3.1 | Protected-main `src/result_export.rs` copies snapshot scores, standard errors, dispositions, owner identity, and version provenance into JSON and a human-readable report | authorized HTTP transport shipped through merged #256 |
| Personal export delivery is authorized from stored participant/result records | ADR-0010; ADR-0003; TRD §11 | Protected-main `src/result_export_authorization.rs` (merged #249) reuses stored-record `ReadOwnResult` and fail-closes cross-tenant before export-binding details | HTTP export contract tests on merged `tests/result_export_http.rs` |
| Narrative cannot mutate score / deterministic fallback exists | AI Governance; ADR-0018 | architecture policy | mapping implementation + canonical style-assignment key + fallback/no-score-mutation tests |
| Instrument release bytes/version/item order are immutable | TRD §7 | `src/instrument.rs` + publication contract tests; `src/postgres_instrument_release.rs` persists immutable manifest columns | API publication integration |
| Only Published release accepts new sessions | TRD §7 | `PublicationState::accepts_new_sessions` in `src/instrument.rs`; protected-main `start_created_assessment_session_from_stored_release` locks publication evidence and persists HTTP create/reload; load still restores created identity without re-checking current eligibility | Command HTTP and the rest of the assessment transport remain missing |
| Publication event replay is idempotent/conflicting reuse fails closed | TRD §7 | `src/instrument.rs` | durable DB uniqueness/concurrency test |
| Published instrument requires exact-version scientific evidence | Measurement Governance; ADR-0019 | `src/instrument.rs` binds approved evidence status, provenance/scope, mandatory evidence references, validity window, and immutable release identity before publication/reactivation | persistence/API publication integration and real instrument-specific evidence artifacts |
| Optional account linking does not rewrite historical participant/result identity | ADR-0003, ADR-0020 | `src/participant.rs` issuer-scoped first-link primitive preserves stable participant ID | append-only identity-link persistence + unlink/relink/recovery audit tests |
| Sensitive authorization is tenant- and task-bound | TRD §11; Security/Data | `src/authorization.rs` fail-closed authorization context/gates bind consent operations to participant ownership; `persist_authorized_consent_ledger` requires `ManageOwnConsent` before inserting consent evidence; `persist_authorized_anonymous_consent_ledger` requires a current anonymous session bound to the same participant | route/repository integration + HTTP cross-tenant E2E tests |
| Research consent separate from service consent | TRD §12; Research Governance | `src/consent.rs` | public API/UI negative test |
| Research withdrawal preserves evidence | TRD §12–15; Research Governance | `src/consent.rs` | release-pipeline exclusion test |
| Export/deletion requires request-specific identity verification | TRD §13 | `src/data_rights.rs`; `src/postgres_data_rights.rs` persists the requested identity and local propagation events | Keyverse/account/anonymous transport integration |
| Legal retention represented explicitly | TRD §13 | `src/data_rights.rs` partial completion; protected-main merged #77 persists terminal `completion_evidence_ref` / `completed_at_unix_ms` and immutable retained-scope evidence | dependency execution/restore tests after local propagation |
| No cross-service DB access | TRD §1–2; ADR-0015 | architecture policy only | deployment credential/fitness-function test |
| Initial physical persistence target is upstream PostgreSQL 18.x | ADR-0015; Deployment/Operations | **Implemented subset** in `migrations/0001_integration_delivery.sql`, `migrations/0002_scoring_job_state.sql`, `migrations/0003_data_rights_propagation.sql`, `migrations/0005_consent_lifecycle.sql`, `migrations/0006_instrument_release.sql`, `migrations/0011_scoring_request.sql`, `migrations/0012_integration_consumption.sql`, matching adapters, and PostgreSQL operational-store readiness | remaining product aggregates, crash/restart restore acceptance |
| No default tenant for writes | TRD §11; Security/Data | authorization-domain primitive exists; persistence remains Target | persistence/API tenant negative tests |
| Tenant-bound transactional outbox/inbox | TRD §19–20; ADR-0014/0015 | `src/integration.rs` domain envelope/inbox/retry contracts plus PostgreSQL tenant/source-scoped integration evidence, delivery-attempt persistence, and inbox consumption; protected-main merged #264 binds the verified publisher receipt to the exact source/tenant/event fence before persistence | durable side-effect processing completion, poison-message/crash recovery, broader aggregate transaction integration |
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

Current protected-main Rust module surface on `4499d9c0889c082487ddbd7fd8d0d5d18257995d`:

```text
src/lib.rs
├── account_link.rs  # dual-proof anonymous-to-account linking evidence
├── anonymous_authorization.rs  # supplied-record anonymous session command authorization
├── anonymous_credential.rs  # hashed short-lived anonymous credential evidence
├── anonymous_session.rs  # tenant/participant/session-bound anonymous authority
├── authorization.rs  # fail-closed tenant/task authorization context and gates
├── consent.rs        # purpose-specific consent + research contribution lifecycle
├── data_rights.rs    # export/deletion lifecycle and retention evidence
├── data_rights_authorization.rs  # stored participant-owned data-rights resource authorization
├── deterministic_narrative.rs  # deterministic AI-independent approved style narrative fallback
├── health.rs         # operation-scoped liveness/readiness and capability-state contract
├── instrument.rs     # immutable release manifest + scientific publication-evidence gate
├── localized_result_report.rs  # exact-locale report presentation over immutable exports (merged #259)
├── integration.rs    # outbox/inbox/retry/quarantine domain contracts
├── integration_publisher.rs  # product-owned immutable integration-event publishing boundary (merged)
├── integration_delivery.rs  # verified publisher-to-fenced-persistence handoff (merged #264)
├── item_delivery.rs  # sequence-aware delivery evidence without confidential response data
├── longitudinal_observation.rs  # longitudinal clocks, identity, and membership-share evidence
├── narrative.rs      # deterministic Personality Style identity/key
├── participant.rs    # stable participant identity + issuer-scoped optional Keyverse account link
├── postgres_consent.rs  # PostgreSQL purpose-specific consent ledger persistence
├── postgres_consent_authorization.rs  # Active PR: ManageOwnConsent or current anonymous session, then persist
├── postgres_data_rights.rs  # PostgreSQL data-rights request and local propagation persistence
├── postgres_data_rights_processing.rs  # PostgreSQL identity-verified data-rights operation persistence
├── postgres_health.rs  # PostgreSQL major/write-readiness and relation-integrity probe
├── postgres_inbox_consumption.rs  # PostgreSQL inbox consumption distinct from receipt
├── postgres_instrument_release.rs  # PostgreSQL locale-specific instrument-release persistence
├── postgres_integration.rs  # PostgreSQL integration evidence/delivery-attempt persistence adapter
├── postgres_item_delivery.rs  # PostgreSQL tenant/session-bound item-delivery evidence persistence
├── postgres_scoring_job.rs  # PostgreSQL scoring enqueue/named claim/claim-next/retry/cancel/terminal persistence
├── postgres_scoring_request.rs  # PostgreSQL version-pinned scoring-request identity
├── postgres_response_snapshot.rs # PostgreSQL immutable response-snapshot persistence
├── postgres_result_snapshot.rs   # PostgreSQL immutable result-snapshot persistence
├── postgres_assessment_session.rs # PostgreSQL session/reload/command persistence
├── result_authorization.rs       # personal result resource authorization
├── result_export.rs              # immutable personal result export domain copy
├── session_http.rs               # persist-backed session HTTP transport
├── reference.rs      # internal opaque-reference normalization
├── research_release.rs  # product-side Research Commons release-evidence gate
├── response.rs       # idempotent response ledger + immutable response snapshots
├── result.rs         # immutable result provenance/supersession
├── result_export_authorization.rs  # post-#231 export-delivery guard (merged #249)
├── scoring_engine.rs # request-bound external scoring-engine adapter boundary
├── scoring.rs        # version-pinned scoring dispatch contract
├── scoring_engine.rs # request-bound external scoring-engine adapter boundary
├── scoring_job.rs    # bounded retry/quarantine lifecycle with lease fencing
└── session.rs        # server-authoritative assessment-session transitions bound to a published locale release

migrations/
├── 0001_integration_delivery.sql through 0007_result_snapshot.sql
├── 0010_response_snapshot.sql through 0016_assessment_session_command.sql
├── 0018_data_rights_processing_start.sql
└── 0019_inbox_claim_expiry_guard.sql
```

Still-Target logical modules/adapters include remaining product aggregate persistence/repositories, remaining public/admin HTTP and event transports, live fast-mlsirm/Keyverse/Gyeot/TEPP/semantic-data-portal adapters, research-release staging, deterministic narrative mapping, longitudinal enrollment persistence, participant identity-link history persistence, runtime health transports/metrics, and Measurement Workbench orchestration.

### Active implementation work that is not protected-main truth

**Active PR** #284 durable accepted response-event persistence is not protected-main truth until an unchanged reviewed/check-clean head is integrated. `migrations/0020_response_event.sql` and `src/postgres_response_event.rs` keep the accepted mid-session ledger prefix durable across process restart with exact replay classification, contiguous sequence recovery, immutable provenance, and fail-closed migration/reference contracts. Public response HTTP transport and completed snapshot reload remain separate slices.

**Active PR** #301 consolidated public-release identity privacy gate is not protected-main truth until an unchanged reviewed/check-clean head is integrated. It consolidates the identity-column denylist, cell-value scanner, separator/prefix hardening, structured-value fail-closed behavior, and the `IdentityInventoryUnavailable` fail-closed inventory contract required by issue #260. A missing or blank effective restricted-identity inventory must fail closed before public fixture approval rather than being read as "nothing to match".

Merged #249 `authorize_result_export_read` is protected-main delivery-guard evidence in `src/result_export_authorization.rs`: it authorizes the stored participant/result with existing `ReadOwnResult` (ADR-0010 export provenance; ADR-0003 tenant-bound authorization) and then requires the export's `result_snapshot_ref` and copied `participant_ref` to match that exact immutable snapshot. Cross-tenant callers fail closed with the ordinary result-authorization denial before export-binding details are evaluated. No new permission or persistence was introduced.

Merged #256 authorized personal result export HTTP is protected-main transport evidence: `src/result_export_http.rs` and `openapi/result-exports.yaml` bind the stored result to the authenticated participant/resource scope and preserve the immutable export provenance from `src/result_export.rs`.

Merged #77 terminal data-rights completion persistence is protected-main evidence: `src/postgres_data_rights_completion.rs` and `migrations/0024_data_rights_completion.sql` persist request-bound completion evidence/time and immutable tenant-bound `data_rights_retained_scope_evidence` rows for deletion scopes that remain legally retained. Exact replay is idempotent; operation, completion identity/time, tenant, request kind/state, and retained-scope rebinding fail closed. This slice does not claim dependent-system execution has completed merely because local terminal evidence exists.

Closed-unmerged #220 public research-fixture identity-column rejection is no longer an active lane; its fail-closed goal survives as open issue #260, which requires a fresh independently reviewed slice that fails closed when the authoritative restricted-identity inventory is absent or blank.

Merged #231 personal result export is protected-main domain evidence. `ResultExport::from_snapshot` copies the stored construct scores, standard errors, dispositions, owner `participant_ref`, and version provenance into a JSON document and a human-readable report. Approved limitation text is required so the report cannot imply diagnosis, employment fitness, or a type score. Padded export aliases are rejected at this boundary without rewriting shared reference trimming used by consent and other domains. The snapshot is not mutated. HTTP `POST /v1/results/{result_ref}/exports` remains Target/active #256. Do not fold unrelated persistence into this domain slice.

Merged #225 anonymous-session resource authorization compares the verified actor to the supplied participant tenant/owner and session and applies a lifecycle command only after that check. The command entry point does not accept a caller-built `ResourceScope` and does not claim the aggregates were store-loaded. Persist/reload of `assessment_participant` remains Target. Append-only identity-link history persistence remains a later slice. HTTP transport remains outside this slice. Persist-backed session HTTP, exclusive outbox delivery leases, longitudinal observation clocks/membership, and claim-next scoring-job poll are already on protected main.

**Active PR** consent authorization write-path composition is not protected-main truth until an unchanged reviewed/check-clean head is integrated. `persist_authorized_consent_ledger` requires `ManageOwnConsent` before calling `persist_consent_ledger`, so a foreign participant, foreign tenant, missing participant identity, or numeric tenant fails closed and inserts no ledger or event row. `persist_authorized_anonymous_consent_ledger` requires a current anonymous assessment session bound to the same participant, so an expired session, unknown time, or foreign ledger inserts no row. Service-operation consent still does not create research contribution. Low-level `persist_consent_ledger` remains an adapter-test entry and does not emit outbox rows. Prefer this anonymous-session composition head over authorize-only #145 and owner-only #170. Prefer consent/outbox landing head #142 over #134/`729a2bd`, #123/`6643041`, #120/`3f72446`, #112/`040bcf7`, and #70/`3180620`. HTTP `POST /v1/consents` remains Target.

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
| Personal export delivery authorization | ADR-0010, ADR-0003 |
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
