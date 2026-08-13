# Requirements and Architecture Traceability

- Status: Normative traceability index
- Date: 2026-08-13
- Evaluated protected-main implementation baseline: `8964280d63a8aa59bdb847e449190b3e67425aff`

This document prevents product requirements, architecture decisions, governance, code, and release evidence from drifting independently. It names one exact protected-main baseline and separates protected-main implementation from active-PR evidence and target architecture. An active PR, target diagram, conversation decision, scheduler plan, or successful isolated check is not shipped truth.

## 1. Status vocabulary

The canonical maturity vocabulary used by current delivery work maps to the repository's established human-readable terms as follows:

- **IMPLEMENTED_ON_PROTECTED_MAIN / Implemented** — source and realistic tests or executable evidence exist on the named protected-main baseline.
- **PARTIAL / Partially implemented** — a bounded contract or subset exists on protected main, but transport, persistence, integration, lifecycle, scientific, operational, or release evidence remains incomplete.
- **IMPLEMENTED_ON_ACTIVE_PR / Active PR** — source/evidence exists only on a currently open PR and is not protected-main truth.
- **ACCEPTED_ARCHITECTURE / PLANNED / Target** — required by accepted architecture or delivery planning but not implemented on the named protected-main baseline.
- **External dependency** — owned by another CWL bounded context and consumed through a versioned contract; it is not reimplemented here.
- **RESEARCH_ONLY** — research evidence or an experimental path that is not production product authority.
- **SUPERSEDED** — replaced by an accepted later decision or implementation contract.
- **OUT_OF_SCOPE** — deliberately outside Psychometrics Commons ownership.

Documentation-family assessments use `PRESENT_CURRENT`, `PRESENT_STALE`, `PARTIAL`, `MISSING`, `NOT_APPLICABLE`, or `SUPERSEDED`. These labels describe documentation fitness, not product maturity.

## 2. Product requirement traceability

| Requirement | Governing contract | Protected-main / active-work evidence | Remaining obligation |
|---|---|---|---|
| Anonymous core assessment | PRD §3.1/§9.1; TRD §5/§10; ADR-0002/0003/0005 | **PARTIAL:** server-authoritative session primitives exist in `src/session.rs`; participant identity exists in `src/participant.rs` | anonymous credential and public HTTP journey |
| Pause/resume | TRD §5; ADR-0005 | **IMPLEMENTED_ON_PROTECTED_MAIN:** `src/session.rs` | persistence/API concurrency evidence |
| Sequence-aware item delivery | TRD §5–7; ADR-0005/0010 | **IMPLEMENTED_ON_PROTECTED_MAIN:** `src/item_delivery.rs` | persistence and serving orchestration |
| Idempotent response events | TRD §6; ADR-0005/0010 | **IMPLEMENTED_ON_PROTECTED_MAIN:** `src/response.rs` | durable repository and concurrent replay evidence |
| Immutable response snapshot before scoring | TRD §5–8; ADR-0005/0010 | **IMPLEMENTED_ON_PROTECTED_MAIN:** `src/response.rs` | atomic session-completion persistence |
| Version-pinned scoring dispatch | TRD §8; ADR-0004/0010 | **IMPLEMENTED_ON_PROTECTED_MAIN:** `src/scoring.rs` | exact live fast-mlsirm adapter; fast-mlsirm remains an **External dependency** |
| Bounded asynchronous scoring with fencing | TRD §8; ADR-0015 | **PARTIAL:** `src/scoring_job.rs`, `migrations/0002_scoring_job_state.sql`, and `src/postgres_scoring_job.rs` are protected-main truth after merged PR #31; enqueue and initial worker claim are durable | **Active PR #36** adds successful-completion/permanent-failure terminal persistence; retry scheduling/reclaim, lease-expiry recovery, crash/restart reconciliation, and live scoring execution remain Target |
| Immutable result provenance | TRD §9; ADR-0004/0010 | **IMPLEMENTED_ON_PROTECTED_MAIN:** immutable domain snapshot in `src/result.rs` | product result persistence and serving transport |
| Deterministic Personality Style identity | PRD §3.2/§9.5; ADR-0018 | **PARTIAL:** `src/narrative.rs` defines behavior-affecting identity, BCP 47 locale validation, canonical serialization, and SHA-256 assignment key on protected main | actual style mapping and deterministic fallback delivery remain Target; **Active PR #40** hardens digest-bearing provenance inputs |
| Continuous/facet scores remain scientific source of truth | ADR-0004/0018; Measurement Governance | **PARTIAL:** product result and presentation-identity boundaries exist; numerical authority remains fast-mlsirm **External dependency** | end-to-end no-score-mutation and deterministic fallback acceptance |
| Immutable instrument publication lifecycle | TRD §7; ADR-0005/0010/0019 | **IMPLEMENTED_ON_PROTECTED_MAIN:** `src/instrument.rs` | persistence/API integration and real instrument evidence bundles |
| Intended-use scientific/rights/locale publication evidence | Measurement Governance; ADR-0013/0019 | **IMPLEMENTED_ON_PROTECTED_MAIN:** release-bound evidence gate in `src/instrument.rs` | each actual instrument must supply its own evidence |
| Optional Keyverse account linking | TRD §10; ADR-0003/0020 | **PARTIAL:** first-link domain primitive in `src/participant.rs`; Keyverse is an **External dependency** | append-only unlink/relink/recovery persistence, audit, transport |
| Tenant/resource authorization | TRD §11; ADR-0001/0003 | **PARTIAL:** fail-closed domain gates in `src/authorization.rs` | repository/route integration and cross-tenant E2E negatives |
| Purpose-specific consent | TRD §12; ADR-0006 | **PARTIAL:** `src/consent.rs` | persistence and API execution |
| Research contribution and withdrawal | TRD §12/§14–15; ADR-0006/0007 | **PARTIAL:** contribution lifecycle in `src/consent.rs` | restricted staging/linkage and release pipeline |
| Participant export/deletion | TRD §13; ADR-0006 | **PARTIAL:** `src/data_rights.rs` | durable dependent-system execution and evidence |
| Research identity separation | TRD §14; ADR-0003/0006/0007/0020 | **PARTIAL:** architecture and contribution identity separation exist | restricted linkage persistence and access-control evidence |
| Research release manifest/gate | TRD §15; ADR-0007/0010 | Target on protected main; semantic-data-portal is an **External dependency** | **Active PR #41** adds only the product-side release-evidence validator; staging, privacy workflow, immutable release persistence and portal registration remain Target |
| Durable outbox/inbox semantics | TRD §19–20; ADR-0014/0015 | **PARTIAL:** `src/integration.rs`, `migrations/0001_integration_delivery.sql`, `src/postgres_integration.rs` | full side-effect/crash/reconciliation lifecycle |
| Operation-scoped health | ADR-0011/0017; Operability | **PARTIAL:** `src/health.rs` | HTTP probes, live observations, metrics, measured deployment evidence |
| Korean/English exact locale versions | TRD §28; ADR-0013/0019 | **PARTIAL:** instrument locale and narrative BCP 47 contracts exist | real Korean/English forms, rights, translation, linking/DIF/invariance evidence and serving |
| WCAG 2.2 AA reference client | PRD §9.10; ADR-0002/0013 | Target | implemented reference client plus automated/manual/assistive-technology acceptance |
| EMA/ESM longitudinal flow | TRD §16; ADR-0008 | Gyeot and TEPP are **External dependency** owners | Commons enrollment, normalized ingestion and orchestration adapter |
| Measurement Workbench | PRD §6; ADR-0001/0004/0019 | Target; fast-mlsirm/Inkspan/RankWeave are **External dependency** owners | product orchestration without numerical/authoring/search kernel duplication |
| Community/Hosted/Enterprise profiles | ADR-0011/0017 | Target | packaging, tenancy/residency, observability, backup/restore, SBOM/provenance and measured recovery evidence |

## 3. Technical invariant traceability

| Invariant | Evidence on evaluated protected main | Missing or active evidence |
|---|---|---|
| Server-authoritative lifecycle | `src/session.rs` | persistence/API concurrency |
| Only Active accepts new responses | `src/session.rs`, `src/response.rs` tests | transport rejection |
| Conflicting response replay fails closed | `src/response.rs` | database uniqueness/concurrency |
| Snapshot is immutable and completion-bound | `src/response.rs` | atomic persistence with session completion/outbox |
| Scoring pins durable snapshot/version identity | `src/scoring.rs` | live adapter and durable request/result transaction |
| Stale scoring worker cannot own a newer lease | `src/scoring_job.rs`, `src/postgres_scoring_job.rs`, `migrations/0002_scoring_job_state.sql` | terminal persistence is **Active PR #36**; retry/reclaim/expiry/crash recovery Target |
| Scientific failure cannot fabricate a score | `src/scoring.rs` typed dispositions | live adapter failure injection |
| Historical result does not mutate | `src/result.rs` | persistent snapshot/supersession chain and API tests |
| Narrative cannot mutate scientific scores | ADR-0018 plus `src/narrative.rs` identity/key boundary | full mapping/fallback/no-score-mutation E2E; exact digest hardening is **Active PR #40** |
| Published instrument identity/evidence is immutable | `src/instrument.rs` | persistence/API and real release bundles |
| Account linking does not rewrite historical participant/result identity | `src/participant.rs`; ADR-0020 | append-only history persistence and recovery |
| Authorization is task-, tenant-, and resource-bound | `src/authorization.rs` | transport/repository policy integration |
| Research consent is separate from service use | `src/consent.rs` | public journey negative test |
| Data-rights completion is evidence-backed | `src/data_rights.rs` | durable worker/dependency propagation/restore |
| No cross-service application DB access | accepted architecture/ADR-0015 | deployment credential fitness evidence |
| Initial operational persistence is upstream PostgreSQL 18.x | `migrations/0001_integration_delivery.sql`, `src/postgres_integration.rs`, `migrations/0002_scoring_job_state.sql`, `src/postgres_scoring_job.rs` | remaining aggregate persistence, terminal/recovery flows, migration/restore acceptance |
| Tenant/source-scoped outbox/inbox identity | integration migration/adapter | completed side-effect/crash/poison-message evidence |
| Liveness differs from operation readiness | `src/health.rs` | live probes and measured thresholds |
| No operational IDs in public research release | Research Governance/architecture only | **Active PR #41** validates release evidence only; actual leakage-negative release fixture remains Target |
| Exact locale has no silent assessment fallback | `src/instrument.rs`; `src/narrative.rs` BCP 47 validation | real localized forms/client serving and cross-locale scientific evidence |
| GA/recovery claims require measured evidence | ADR-0017; Release Acceptance | deployed SLO/RPO/RTO/restore/incident evidence |

## 4. Source and migration map

Current protected-main Rust/product persistence surface on `8964280d63a8aa59bdb847e449190b3e67425aff` includes:

```text
src/lib.rs
├── authorization.rs        # fail-closed tenant/task/resource authorization
├── consent.rs              # purpose-specific consent + research contribution
├── data_rights.rs          # export/deletion lifecycle and retention evidence
├── health.rs               # operation-scoped liveness/readiness capability contract
├── instrument.rs           # immutable publication + evidence gate
├── integration.rs          # outbox/inbox/retry/quarantine domain contracts
├── item_delivery.rs        # sequence-aware delivery evidence
├── narrative.rs            # deterministic Personality Style provenance identity/key
├── participant.rs          # stable product participant + optional Keyverse first link
├── postgres_integration.rs # PostgreSQL integration delivery persistence
├── postgres_scoring_job.rs # PostgreSQL scoring enqueue/initial-lease persistence
├── reference.rs            # opaque-reference normalization
├── response.rs             # idempotent response ledger + immutable response snapshot
├── result.rs               # immutable scientific/product result snapshot
├── scoring.rs              # version-pinned scoring dispatch/result contract
├── scoring_job.rs          # retry/quarantine lifecycle and worker fencing
└── session.rs              # server-authoritative session transitions

migrations/
├── 0001_integration_delivery.sql
└── 0002_scoring_job_state.sql
```

Still-Target product-owned implementation includes session/response/result/consent/data-rights persistence, public/admin HTTP transport, live fast-mlsirm/Keyverse/Gyeot/TEPP/semantic-data-portal adapters, full Personality Style mapping/fallback delivery, research staging/release persistence, longitudinal normalized ingestion, identity-link history persistence, runtime probes/metrics, scoring retry/reclaim/expiry/crash recovery, and Measurement Workbench orchestration.

### Active implementation work that is not protected-main truth

- **Active PR** #36 (`feat: persist PostgreSQL scoring terminal outcomes`) is not protected-main truth; it adds successful-completion and permanent-failure/quarantine persistence while preserving fencing and immutable result evidence.
- **Active PR** #40 (`fix(narrative): require canonical SHA-256 provenance digests`) is not protected-main truth; it hardens the digest-bearing deterministic Personality Style identity fields and does not change numeric psychometric authority.
- **Active PR** #41 (`feat: validate Research Commons publication evidence`) is not protected-main truth; it adds a bounded product-side release-evidence validator but does not stage/publish data or register semantic-data-portal releases.

PR #31 is merged and must no longer be represented as Active PR evidence. Its migration and initial PostgreSQL scoring-job lease adapter are protected-main truth on the evaluated baseline.

## 5. Documentation-family reconciliation

| Artifact family | Assessment on this baseline | Notes |
|---|---|---|
| PRD | PRESENT_CURRENT | journey, bounded contexts and acceptance remain consistent with current code |
| TRD | PRESENT_CURRENT | implementation is still partial; prose transport requirements are not deployed APIs |
| Root Architecture + C4 | PRESENT_CURRENT | target/mixed architecture, not deployment proof |
| ADR index/set | PARTIAL | accepted decisions remain authoritative, but ADR-0015's embedded current/as-built status prose predates merged PostgreSQL slices and requires current-state maintenance without changing the accepted decision |
| UML | PRESENT_CURRENT | target behavior remains distinct from shipped evidence |
| Logical ERD | PRESENT_CURRENT | logical target model is not physical DDL proof |
| Security/Threat Model/privacy | PRESENT_CURRENT | controls remain evidence-gated where implementation is incomplete |
| Measurement Governance | PRESENT_CURRENT | fast-mlsirm retains numerical authority |
| AI Governance | PRESENT_CURRENT | optional AI cannot mutate scientific numeric truth |
| Research Governance | PRESENT_CURRENT | release execution remains partial/Target |
| Product Experience | PRESENT_CURRENT | consumer implementation remains incomplete |
| Quality/Risk/Compliance Readiness | PRESENT_CURRENT | readiness is not certification |
| Traceability | PRESENT_CURRENT | this file is reconciled to the named protected-main baseline and live PR set |
| Roadmap | PRESENT_CURRENT | dependency order remains valid |
| Documentation Assessment | PRESENT_STALE | evaluated baseline and prior active-PR descriptions lag current protected main and require follow-up reconciliation |
| OpenAPI/AsyncAPI | NOT_APPLICABLE as as-built evidence | add with first implemented HTTP/durable message surfaces; do not fabricate target operations as deployed |
| Physical schema | PARTIAL | migrations 0001/0002 are real; remaining logical entities are not yet physical schema |

## 6. ADR traceability by concern

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

## 7. Governance and evidence artifact traceability

| Concern | Authoritative artifact | Current evidence status |
|---|---|---|
| Product intent | `docs/PRD.md` | protected-main normative baseline |
| Technical contract | `docs/TRD.md` | normative; implementation still partial |
| Measurement/scientific publication | `docs/MEASUREMENT_GOVERNANCE.md` | product governance; numerical kernels remain external |
| Continuous score / narrative interpretation | ADR-0018 + `docs/AI_GOVERNANCE.md` | deterministic identity/key **PARTIAL** on protected main; mapping/fallback Target |
| Instrument scientific publication gate | ADR-0019 + Measurement Governance | policy gate implemented; actual instrument evidence is release input |
| Research contribution/release | `docs/RESEARCH_GOVERNANCE.md` | contribution lifecycle partial; PR #41 is active-only validator evidence |
| Nonfunctional evidence | `docs/QUALITY_ATTRIBUTES.md`, `docs/TEST_STRATEGY.md`, `docs/OPERABILITY.md` | scenarios are verified only where executable evidence exists |
| Assurance readiness | `docs/COMPLIANCE_READINESS.md` | no external SOC 2/CSAP certification claim |
| Material risk | `docs/RISK_REGISTER.md` | open until verified closure or authorized acceptance |
| Architecture views | `docs/architecture/*` | target/mixed views, not as-built proof |
| Release authority | `docs/RELEASE_ACCEPTANCE.md` | software, instrument and research release gates remain distinct |
| Delivery order | `docs/ROADMAP.md` | protected-main delivery baseline |

## 8. Whole-product reconciliation gate

The durable product definition remains **Scientific Trait Core + Accessible Narrative + Reflective Capacities + Longitudinal Context + Open Science**, expressed as **Measure → Understand → Reflect → Observe Over Time → Contribute to Science**.

IPIP Big Five is the first consumer family. Continuous/facet scores and uncertainty remain scientific truth; Personality Style is a separately versioned presentation mapping and is not MBTI equivalence. Self-compassion and future-reflection constructs are independent instruments. Anonymous participation is first-class; optional Keyverse linking never rewrites historical product identity. Research contribution is a separate purpose-specific opt-in. Gyeot owns EMA/ESM collection, TEPP owns temporal/event/multilevel/multiple-membership analysis, and semantic-data-portal owns public research catalog/release registration. Psychometrics Commons orchestrates its own participant/session/consent/persistence/release-handoff responsibilities without copying those kernels. Optional AI cannot mutate numeric scores, calibration, norms, DIF, uncertainty or scientific publication gates.

Enterprise issue-prioritization/causal expected-intervention-value logic remains OUT_OF_SCOPE absent a future accepted ADR.

## 9. Machine-readable contract gate

Prose API/event families in the TRD are architecture requirements, not evidence of an implemented transport.

When the first HTTP API is implemented, the same workstream must add and validate an OpenAPI 3.2.x document matching only actually implemented operations and RFC 9457-compatible problem responses where applicable. When durable message transport is implemented, the same workstream must add and validate an AsyncAPI 3.1.x document for actually produced/consumed channels and message schemas, including ADR-0014 digest, tenant/resource, deduplication, replay and quarantine semantics.

Target or future contracts may be documented as non-deployed design material, but cannot satisfy as-built or release acceptance.

## 10. Traceability maintenance gate

A PR that materially changes any of the following must update this document or prove no traceability change is needed:

- domain ownership or dependency direction;
- lifecycle states/transitions;
- public/admin API or event family;
- persistent logical entity/cardinality/transaction boundary;
- scientific publication or score-interpretation rule;
- AI/judge/provider authority;
- research contribution/release/access rule;
- identity/security/privacy/tenant/trust boundary;
- database support, migration or recovery semantics;
- quality/release/recovery claim;
- material risk/evidence state;
- consumer/research acceptance criterion.

CI must continue validating required documentation paths, ADR indexing, status vocabulary and target-versus-as-built discipline. When machine-readable contracts or migrations exist, documentation references must resolve to real artifacts without promoting unmerged PR behavior to protected-main truth.

## 11. References

Nottingham, M., Wilde, E., & Dalal, S. (2023). *Problem Details for HTTP APIs* (RFC 9457). Internet Engineering Task Force. https://doi.org/10.17487/RFC9457

OpenAPI Initiative. (2025). *OpenAPI Specification, Version 3.2.0*.

AsyncAPI Initiative. (2026). *AsyncAPI Specification, Version 3.1.0*.
