# Requirements and Architecture Traceability

- Status: Normative traceability index
- Date: 2026-08-15
- Evaluated protected-main implementation baseline: `cc5850a0d1eacbbf16d03075534fce460a8286e6`

This file is the canonical bridge from product requirements and accepted architecture to code, tests, migrations, open implementation work, and release evidence. It deliberately separates protected-main truth from active-PR evidence. An open PR, target diagram, conversation, or scheduler plan is never promoted to shipped behavior.

## 1. Maturity vocabulary

Use only these repository maturity classes in this index:

- **IMPLEMENTED_ON_PROTECTED_MAIN** — executable source plus its governing tests/migrations are present on the exact protected-main baseline named above.
- **IMPLEMENTED_ON_ACTIVE_PR** — executable evidence exists on an open PR but is not protected-main truth.
- **PARTIAL** — part of the required lifecycle or evidence exists, but an essential persistence, transport, integration, scientific, operational, accessibility, or release boundary remains incomplete.
- **ACCEPTED_ARCHITECTURE** — an accepted ADR/governance contract exists without the required executable implementation.
- **PLANNED** — Roadmap/product work is intended but has not reached accepted executable architecture.
- **RESEARCH_ONLY** — research evidence exists but is not a product implementation claim.
- **SUPERSEDED** — prior evidence is historical and replaced by a newer accepted implementation/decision.
- **OUT_OF_SCOPE** — another bounded context owns the capability or the product explicitly excludes it.

Every future maturity promotion must cite source/test/migration/contract evidence on one exact protected-main commit. Active PR identifiers may be recorded as volatile evidence but cannot make a protected-main row implemented.

## 2. Protected-main evidence inventory

Protected main `cc5850a0d1eacbbf16d03075534fce460a8286e6` contains executable product/domain modules for anonymous session context, authorization, consent/research contribution, data rights, health/readiness, immutable instrument publication, integration delivery semantics, item-delivery evidence, deterministic narrative identity/provenance primitives, participant identity linking, responses and immutable response snapshots, results, scoring requests/jobs, assessment sessions, and style-assignment provenance.

The physical PostgreSQL migration set on this exact baseline is:

```text
migrations/0001_integration_delivery.sql
migrations/0002_scoring_job_state.sql
migrations/0003_data_rights_propagation.sql
migrations/0005_consent_lifecycle.sql
migrations/0006_instrument_release.sql
migrations/0010_response_snapshot.sql
migrations/0011_scoring_request.sql
migrations/0012_integration_consumption.sql
migrations/0015_data_rights_identity_verification.sql
```

Corresponding protected-main adapters include `src/postgres_integration.rs`, `src/postgres_scoring_job.rs`, `src/postgres_data_rights.rs`, `src/postgres_consent.rs`, `src/postgres_instrument_release.rs`, `src/postgres_response_snapshot.rs`, `src/postgres_scoring_request.rs`, and `src/postgres_health.rs`. This inventory is intentionally narrower than the logical ERD: missing product aggregates are not implied to be persisted.

## 3. Product requirement traceability

| Product / requirement family | Governing contract | Maturity on evaluated protected main | Executable evidence / remaining boundary |
|---|---|---|---|
| Product journey: Measure -> Understand -> Reflect -> Observe Over Time -> Contribute to Science | PRD; Product Experience | PARTIAL | Core measurement/session/result primitives exist; public transport, reference client, longitudinal orchestration, and research release journey remain incomplete. |
| Anonymous-first participation | PRD §3; ADR-0003 | PARTIAL | `src/anonymous_session.rs` and session primitives provide resource-bound anonymous context; credential issuance/verification and public HTTP flow remain future transport work. |
| Immutable instrument publication | TRD; ADR-0005, ADR-0019 | IMPLEMENTED_ON_PROTECTED_MAIN | `src/instrument.rs`, `migrations/0006_instrument_release.sql`, `src/postgres_instrument_release.rs`; every real instrument still needs its own rights/locale/scientific evidence before release. |
| Session state machine bound to exact release/locale | TRD; ADR-0005 | PARTIAL | Domain lifecycle in `src/session.rs` is executable; created-session persistence and hosted HTTP lifecycle are not on this baseline. |
| Sequence-aware item delivery evidence | TRD; ADR-0005, ADR-0010 | PARTIAL | `src/item_delivery.rs` is protected-main domain evidence; durable item-delivery ledger remains active implementation work, not shipped truth. |
| Idempotent response events | TRD §6; ADR-0010 | PARTIAL | `src/response.rs` enforces response identity/digest semantics; durable response-event ledger is not on this baseline. |
| Immutable completed response snapshot | TRD §5-8; ADR-0010, ADR-0015 | IMPLEMENTED_ON_PROTECTED_MAIN | `src/response.rs`, `migrations/0010_response_snapshot.sql`, `src/postgres_response_snapshot.rs`, real PostgreSQL regression coverage. |
| Version-pinned scoring request | TRD §8; ADR-0004, ADR-0010 | IMPLEMENTED_ON_PROTECTED_MAIN | `src/scoring.rs`, `migrations/0011_scoring_request.sql`, `src/postgres_scoring_request.rs`; live fast-mlsirm execution remains external integration work. |
| Durable async scoring job/retry/quarantine/fencing | TRD §8; ADR-0015 | IMPLEMENTED_ON_PROTECTED_MAIN | `src/scoring_job.rs`, `migrations/0002_scoring_job_state.sql`, `src/postgres_scoring_job.rs`; transaction compositions tying snapshots/dispatch/results/outbox are still active work. |
| Immutable result provenance | TRD §9; ADR-0010 | PARTIAL | `src/result.rs` domain semantics exist; result persistence/serving is not protected-main truth. |
| Continuous/facet scores as scientific source of truth | PRD; Measurement Governance; ADR-0018 | ACCEPTED_ARCHITECTURE | fast-mlsirm owns psychometric numerics. Commons must consume immutable numerical evidence rather than duplicate kernels. |
| Personality Style as separately versioned presentation mapping | PRD; ADR-0018 | PARTIAL | Protected main has deterministic style/narrative provenance primitives; first scientifically governed mapping and participant-facing fallback bundle are not both shipped. No MBTI-equivalence claim is permitted. |
| Self-compassion and future reflection as independent constructs | PRD; Measurement Governance | ACCEPTED_ARCHITECTURE | They must be independently measured instruments, never inferred from Big Five. |
| Purpose-specific consent, research opt-in separate from service use | PRD §5; ADR-0006 | IMPLEMENTED_ON_PROTECTED_MAIN | `src/consent.rs`, `migrations/0005_consent_lifecycle.sql`, `src/postgres_consent.rs`; cross-service propagation composition remains active work. |
| Participant export/deletion request + request-specific identity verification | PRD; TRD §13; ADR-0006 | IMPLEMENTED_ON_PROTECTED_MAIN | `src/data_rights.rs`, `migrations/0003_data_rights_propagation.sql`, `migrations/0015_data_rights_identity_verification.sql`, `src/postgres_data_rights.rs`; processing/completion persistence and real dependent-system execution are not on this baseline. |
| Optional Keyverse linking is append-only and cannot rewrite historical identity | ADR-0003, ADR-0020 | PARTIAL | `src/participant.rs` protects domain identity history semantics; durable append-only identity-link persistence is active work. Keyverse credentials remain OUT_OF_SCOPE. |
| Tenant/task authorization | TRD §11; Security/Threat Model | PARTIAL | `src/authorization.rs` provides fail-closed product authorization primitives; hosted transport/policy adapter and cross-tenant E2E proof remain incomplete. |
| Transactional outbox/inbox with receipt distinct from effect completion | ADR-0014, ADR-0015 | IMPLEMENTED_ON_PROTECTED_MAIN | `migrations/0001_integration_delivery.sql`, `migrations/0012_integration_consumption.sql`, `src/integration.rs`, `src/postgres_integration.rs`; broader aggregate transaction compositions and live external side effects remain incomplete. |
| Operation-scoped health/readiness | ADR-0011, ADR-0017; Operability | IMPLEMENTED_ON_PROTECTED_MAIN | `src/health.rs` and `src/postgres_health.rs`; hosted probes/metrics/deployment acceptance remain future evidence. |
| Korean/English exact-locale assessment versions | ADR-0013, ADR-0019 | PARTIAL | Locale pinning is implemented in instrument/session domain contracts; real Korean/English form rights, translation, linking/invariance/DIF evidence, accessibility, and serving remain incomplete. |
| WCAG 2.2 AA reference client | PRD; Quality Attributes | PLANNED | No reference client implementation is protected-main truth. |
| Research contribution and withdrawal | ADR-0006, ADR-0007 | PARTIAL | Domain lifecycle exists in `src/consent.rs`; consent-bound durable research contribution persistence and restricted staging/release pipeline are not on this baseline. |
| Public research catalog/release registration | ADR-0007 | OUT_OF_SCOPE | `semantic-data-portal` owns immutable public catalog/release registration. Commons owns product-side review/handoff evidence only. |
| EMA/ESM collection | ADR-0008 | OUT_OF_SCOPE | Gyeot owns collection; Commons owns enrollment/consent/normalized-ingestion/reference orchestration only. |
| Temporal/event/multilevel/multiple-membership analysis | ADR-0008 | OUT_OF_SCOPE | TEPP owns analysis kernels. Commons must not copy them. |
| Measurement Workbench | PRD §6; ADR-0004, ADR-0019 | PLANNED | Must surface reusable fast-mlsirm AssessmentSpec/Rubric/Blueprint/item-bank/scoring/calibration evidence plus Inkspan/RankWeave integrations without duplicating kernels. |
| Enterprise issue prioritization / causal expected-intervention-value | Product boundary | OUT_OF_SCOPE | Remains downstream unless a future superseding/accepted ADR explicitly changes ownership. |
| Community/Hosted/Enterprise deployment profiles, backup/restore, SBOM/provenance | ADR-0011, ADR-0015, ADR-0017 | PARTIAL | Runtime/persistence evidence exists, but profile packaging, deployed recovery drills, measured availability/recovery claims, and GA acceptance are incomplete. |

## 4. Scientific and quality gates

| Gate | Maturity | Protected-main evidence / missing proof |
|---|---|---|
| Numeric psychometric claims consume fast-mlsirm evidence | ACCEPTED_ARCHITECTURE | ADR-0004/Measurement Governance; live adapter remains incomplete. |
| AI cannot mutate numeric scores, uncertainty, calibration, norms, DIF, or scientific gates | ACCEPTED_ARCHITECTURE | AI Governance / ADR-0009 / ADR-0018; adversarial hosted-product proof remains future evidence. |
| Cross-locale claims require exact locale plus linking/invariance/DIF evidence | ACCEPTED_ARCHITECTURE | ADR-0013/0019 and Measurement Governance; real bilingual release evidence remains incomplete. |
| Human/AI/LLM judges are fallible raters, not truth | ACCEPTED_ARCHITECTURE | Measurement/AI Governance; any model-backed product feature must preserve this boundary. |
| Owned production coverage target is exact 100% statement + branch | PARTIAL | CI/coverage contracts exist; every active head and integrated main must prove exact policy before release. |
| Public IDs are opaque/non-numeric | IMPLEMENTED_ON_PROTECTED_MAIN | Shared/domain validation exists; active hardening work may further restrict canonical spelling and is not promoted here. |
| Owned DB names are descriptive two-or-more-word snake_case | IMPLEMENTED_ON_PROTECTED_MAIN | Current protected-main migration objects follow repository naming policy; future migrations remain gated. |
| No direct cross-service application-database access | ACCEPTED_ARCHITECTURE | ADR-0001/0015; deployment credential evidence is still required for GA. |

## 5. Active-PR evidence is not shipped truth

### Active implementation work that is not protected-main truth

**Active PR** #80 documentation rebaseline is not protected-main truth until an unchanged reviewed/check-clean head is integrated. It reconciles protected-main evidence vocabulary and architecture mappings after recent persistence merges; it does not promote open persistence or recovery slices to shipped implementation.

At this documentation reconciliation, open work includes additional persistence/recovery/transaction-composition slices for item delivery, results, participant identity links, response events, sessions, research release/contribution evidence, scoring orchestration/completion, consent propagation, data-rights processing/completion, recovery acceptance, deterministic narrative rendering, and current fail-closed/idempotency hardening. Those changes are **IMPLEMENTED_ON_ACTIVE_PR** only after their exact current heads contain executable source/tests; their details must be re-fetched from GitHub rather than copied into protected-main claims.

The previous documentation reconciliation PR #67 targeted an older protected-main baseline and is **SUPERSEDED** by this baseline reconciliation. Closing or superseding an obsolete documentation PR does not itself promote any product behavior.

## 6. Architecture and bounded-context ownership

| Capability | Owner / contract | Traceability status |
|---|---|---|
| Hosted public/admin APIs, instrument publication, participant/session/item-delivery/response lifecycle, product auth/tenant context, consent/data rights, scoring dispatch, immutable product result, product persistence, research handoff, reference clients, deployment/operator workflows | psychometrics-commons | PARTIAL; bounded ownership is accepted, implementation varies by row above. |
| Psychometric numerics / reusable AssessmentSpec, Rubric, scoring/calibration evidence | fast-mlsirm | OUT_OF_SCOPE implementation; External dependency contract. |
| Identity/federation credentials | Keyverse | OUT_OF_SCOPE implementation; optional append-only product link only. |
| EMA/ESM collection | Gyeot | OUT_OF_SCOPE implementation; Commons orchestrates product enrollment/ingestion boundary. |
| Temporal/event/multilevel analysis | TEPP | OUT_OF_SCOPE implementation; External dependency. |
| Public research catalog/release registration | semantic-data-portal | OUT_OF_SCOPE implementation; External dependency. |
| Bounded realtime AI / bulk AI / outbound egress | contextual-orchestrator / pg-llm-batch / EgressWeave | OUT_OF_SCOPE implementation; optional adapters only. |
| Authoring/search/reporting | Inkspan / RankWeave / Clearfolio | OUT_OF_SCOPE implementation; optional composition only. |

No row authorizes direct cross-service application-database access or hosted-runtime implementation inside fast-mlsirm.

## 7. GA evidence still required

Protected main is not GA merely because individual domain or persistence slices are implemented. GA requires one exact integrated protected head with repository-required independent review and checks plus, as applicable: full hosted lifecycle acceptance; live fast-mlsirm adapter and typed failure paths; tenant authorization/cross-tenant negatives; consent/data-rights execution; restricted research staging/privacy/scientific review and semantic-data-portal handoff; bilingual rights/translation/invariance/accessibility evidence; reference client; deployment profiles; observability; backup/restore and degraded-mode exercises; packaging; SBOM/provenance/reproducibility; migration/rollback evidence; and release-specific scientific/right/norm/scoreability/narrative evidence.

No SLO, RPO/RTO, certification, rights clearance, deployed topology, physical schema not present in migrations, or external integration status may be invented in this index.

## 8. Evidence update rule

For each material change, reviewers must reconcile in the same workstream when applicable:

1. PRD/TRD requirement and accepted ADR/governance decision;
2. source/module/API/event/schema or migration evidence;
3. realistic tests, including scientific/tenant/privacy/recovery boundaries where relevant;
4. architecture views (`ARCHITECTURE.md`, C4/UML/ERD/AS_BUILT_SCHEMA) when as-built topology/schema/flow changes;
5. this traceability row and Roadmap maturity; and
6. release/operational evidence without promoting active-PR behavior to protected-main truth.

Standards and research evidence are curated in `docs/doctoring/standards-and-evidence.md` and the governance documents. Their presence supports a decision; it does not replace executable product acceptance evidence.