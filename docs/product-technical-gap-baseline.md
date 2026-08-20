# Product and Technical Gap Baseline

- Status: current delivery snapshot, not a release or certification claim
- Snapshot date: 2026-08-20 (Asia/Seoul)
- Evaluated protected-main head: `5544149c` (`feat(integration): enforce event-bound publisher acknowledgements (#252)`)
- Repository: `ContextualWisdomLab/psychometrics-commons`
- Scope: buyer-visible product journey, product/runtime implementation, architecture evidence, open PRs/issues, and next executable loop

## How to read this baseline

This document is the current gap snapshot for delivery decisions. It does not replace the PRD, TRD, ADRs, governance documents, or release gates. A domain type, diagram, active PR, green check, or scheduler decision is not protected-main product evidence until the exact reviewed head is integrated and `main` is fetched again.

Evidence status uses these meanings:

- **Protected main** — source, tests, and migrations are present on the evaluated protected-main head.
- **Active PR** — the change is visible on an open PR but is not product truth yet.
- **Target** — required by the product/technical architecture but not implemented on the evaluated head.
- **Unverified** — the repository states or expects the evidence, but this snapshot did not observe the required runtime, deployment, scientific, or independent-assessment result.

The authority order is accepted/superseding ADR → PRD/TRD → measurement, AI, and research governance → quality/security/compliance/risk constraints → architecture views → machine-readable contracts → code, migrations, tests, and operational evidence. A lower layer cannot promote a target into shipped behavior.

## Observed protected-main baseline

The evaluated head contains a Rust product-runtime library with PostgreSQL adapters and a single as-built HTTP family:

| Area | Observed evidence | Buyer meaning |
|---|---|---|
| Assessment lifecycle | `src/session.rs`, response ledger/snapshot, scoring job, result snapshot, and instrument publication contracts | The core lifecycle is modeled with fail-closed replay and provenance rules. |
| Session transport | `src/session_http.rs`, `openapi/sessions.yaml`, `POST /v1/sessions`, and `GET /v1/sessions/{session_ref}` | A buyer can start/reload a created session contract, but this is not yet the complete assessment journey. |
| Persistence | Migrations `0001`–`0019` and PostgreSQL adapters for integration, consent, data rights, instruments, responses, results, scoring, sessions, and health | Durable slices exist, but remaining aggregates, recovery drills, and deployment evidence are not closed. |
| Scoring boundary | Version-pinned scoring request/result contracts in `src/scoring.rs`, `src/scoring_job.rs`, and PostgreSQL adapters | Numeric kernels remain correctly outside this repository; a live `fast-mlsirm` execution proof is still absent from protected main. |
| Result export | `src/result_export.rs` is on protected main through the merged result-export work; HTTP export delivery remains an active/target transport lane | The report data model exists, but a buyer-facing authorized download is not yet one protected-main end-to-end flow. |
| Authorization and identity | Domain authorization, anonymous/session identity contracts, and initial account-link primitives exist | Transport, durable identity-link history, and complete Keyverse integration remain incomplete. |
| Longitudinal evidence | `src/longitudinal_observation.rs` preserves validity, recorded, received, ingested, and membership evidence | The temporal contract is present; enrollment persistence, HTTP collection, and Gyeot/TEPP orchestration remain incomplete. |
| Research release | `src/research_release.rs` contains release-gate domain contracts; public-fixture privacy work is on active PR #220 | Publication must still prove privacy inventory availability, release persistence, scientific review, and semantic-data-portal registration. |
| Narrative and locale | Deterministic narrative/style primitives exist; Korean/English report work is active | A complete localized, evidence-bound result experience is not yet proven on protected main. |
| Reference client/design system | No frontend or Storybook inventory is present in this repository snapshot | The product is not yet a buyer-demonstrable accessible client. No Figma file ID is invented; an actual design artifact is required before an ADR can record one. |

## Priority gaps

| Priority | Gap and buyer-visible consequence | Current evidence | Smallest next executable slice | Completion evidence |
|---|---|---|---|---|
| P0 | **Complete public assessment journey.** A buyer cannot yet rely on one protected-main path from anonymous start through item delivery, response submission, completion, scoring, result read, and authorized export. | Session HTTP is protected main; response/item/command/result/export HTTP lanes are split across active PRs or Target. | Stack and merge the smallest compatible HTTP slices in dependency order: start/reload → delivery/response/command → result read → export. Keep one OpenAPI contract per implemented family and RFC 9457 errors. | Browser or real HTTP E2E on exact protected head; persisted restart/replay; no invented score; authorized cross-tenant negatives; OpenAPI contract validation. |
| P0 | **Live scientific scoring and instrument evidence.** The runtime has scoring contracts but no shipped, rights-cleared IPIP Big Five instrument release with real `fast-mlsirm` execution evidence. | `src/scoring.rs` intentionally delegates numerics; traceability marks live integration and instrument-specific evidence Target. | Add a versioned adapter contract and one rights/locale/scientific evidence bundle without copying upstream kernels. | True-parameter recovery, bias/MAE/RMSE, uncertainty coverage, boundary/parity evidence as applicable, exact artifact digest, and end-to-end result provenance. |
| P0 | **Research public-release fail-closed inventory.** An absent identity inventory currently risks being treated as “nothing to match.” | Issue #260 depends on active PR #220 and requires a distinct `inventory unavailable` outcome. | Merge #220 when independently reviewed, then implement #260 as a stacked slice; never duplicate #220 on main or query another service database. | RED absent/blank inventory; GREEN non-empty no-match; forbidden operational/Keyverse/linkage match; pseudonym allowed; raw identifiers absent from errors. |
| P1 | **Durable identity and authorization journey.** Anonymous participation, account linking, recovery, and personal resource access are not one proven transport/persistence boundary. | Domain primitives exist; multiple identity/session PRs are open and participant persistence/link history remains incomplete. | Consolidate the narrowest append-only participant/link persistence and transport slice after exact dependency review. | Proof separation, issuer/subject/tenant binding, unlink/relink recovery, historical identity immutability, and cross-tenant HTTP tests. |
| P1 | **Longitudinal product loop.** Buyers cannot yet enroll, persist, reload, and submit observations into an owned Commons→Gyeot/TEPP handoff. | Observation semantics are protected main; #248 and #262 are active lanes; enrollment/persistence/HTTP are Target. | Merge the observation identity correction first, then add enrollment/observation persistence with explicit source and membership keys. | Restart/replay, duplicate identity rejection, temporal ordering, multiple-membership preservation, tenant/consent gates, and bounded handoff evidence. |
| P1 | **Localized, accessible result experience.** A result model is not a usable product without exact locale behavior, limitations, accessibility, and non-color equivalents. | Report locale work is active PR #259; no reference client or Storybook inventory is protected main. | Finish exact Korean/English deterministic reports, then build the smallest accessible reference flow using native HTML/CSS and a token inventory. | WCAG 2.2 AA automated plus keyboard/assistive-technology checks, locale no-fallback tests, text/table equivalents, and snapshot provenance parity. |
| P1 | **Operational confidence.** The code has health/retry/recovery contracts, but no measured deployment-profile restore, SLO/RPO/RTO, or incident exercise evidence. | `docs/OPERABILITY.md`, ADR-0017, and release acceptance explicitly keep these as evidence gates. | Run one Compose/PostgreSQL bootstrap, upgrade, backup/restore, and crash/replay drill for the supported profile. | Exact artifact manifest, measured recovery results, deduplication/provenance reconciliation, and current runbook evidence. |
| P2 | **Architecture/documentation freshness.** Current documents still contain older protected-main hashes and stale Active-PR wording for merged result export work. | `docs/TRACEABILITY.md` names older baselines and describes #231 as active; `docs/DOCUMENTATION_ASSESSMENT.md` names `748876…`; README/CHANGELOG repeat the old status. | Reconcile those references in a documentation PR and keep this snapshot as the current gap index. | Documentation fitness tests pass and every status names an exact current head or active PR. |
| P2 | **Database scale and naming proof.** Existing migrations use descriptive multi-word table names, but there is no single automated proof for all object names, 3NF/cardinality reconciliation, or hot-partition behavior. | Current migration inventory is observable; ERD is logical and several future aggregates have no physical migration. | Add a bounded migration/schema contract check when the next physical slice lands; document partition keys only where measured workload requires them. | Clean/upgrade migration tests, ERD reconciliation, naming check, tenant uniqueness checks, and measured contention/partition evidence. |
| P2 | **Design-system and buyer onboarding evidence.** No Figma/Storybook artifacts exist in this backend-focused repository, so visual consistency and onboarding are not reviewable. | No frontend or Storybook files were observed in the protected-main tree. | Use a separate reference-client/design-system boundary when the first client slice is authorized; record the real Figma File ID in its ADR. | Storybook inventory, design-token tests, keyboard/interaction/i18n checks, and a published usable client; do not fabricate a Figma ID. |

## Current open PR inventory

Snapshot source: GitHub REST `GET /repos/ContextualWisdomLab/psychometrics-commons/pulls?state=open` after fetching remotes on 2026-08-20. These are all open PRs observed at snapshot time; every one still requires exact-head review/check verification before merge. `Ready` means GitHub did not mark it Draft, not that it is merge-ready.

| PR | Title | Head branch | Head | Draft state | Updated |
|---:|---|---|---|---|---|
| [77](https://github.com/ContextualWisdomLab/psychometrics-commons/pull/77) | feat(data-rights): persist terminal completion evidence | `feat/data-rights-completion-persistence-20260814` | `f711966` | Ready | 2026-08-20 |
| [87](https://github.com/ContextualWisdomLab/psychometrics-commons/pull/87) | feat(api): add safe RFC 9457 problem-details primitive | `feat/api-problem-details-contract-20260816` | `135a411` | Ready | 2026-08-20 |
| [101](https://github.com/ContextualWisdomLab/psychometrics-commons/pull/101) | feat(research): persist immutable release approval evidence | `cursor/bc-47f93277-ffe5-4ddf-bb77-ce962ce10d26-d732` | `e0ef158` | Ready | 2026-08-16 |
| [103](https://github.com/ContextualWisdomLab/psychometrics-commons/pull/103) | fix(health): apply backlog indexes through the product API | `cursor/bc-914e2d45-c04c-4746-ae84-8ca84386214a-0afd` | `276b6c0` | Ready | 2026-08-17 |
| [107](https://github.com/ContextualWisdomLab/psychometrics-commons/pull/107) | fix(response): bind record and freeze to assessment session | `cursor/bc-96085f62-6f25-4040-b068-89d9e751c3b3-4543` | `2cfada3` | Ready | 2026-08-17 |
| [138](https://github.com/ContextualWisdomLab/psychometrics-commons/pull/138) | feat(session): start created sessions from published releases | `cursor/bc-0c5af809-ebff-45b2-9fcf-dfa3c8fbdbd9-bcce` | `7c24084` | Ready | 2026-08-17 |
| [139](https://github.com/ContextualWisdomLab/psychometrics-commons/pull/139) | fix(health): name scoring-job index migrations in the as-built map | `cursor/bc-03f12a2f-298e-42d6-913c-a684930556c3-3259` | `21aa6be` | Draft | 2026-08-16 |
| [141](https://github.com/ContextualWisdomLab/psychometrics-commons/pull/141) | feat(narrative): assign Personality Style from scored Big Five | `cursor/bc-d6b92d35-17a8-489b-ac92-f58f1a03fb8a-2575` | `af8cb36` | Draft | 2026-08-16 |
| [142](https://github.com/ContextualWisdomLab/psychometrics-commons/pull/142) | fix(consent): stop documenting event-identity tail order | `cursor/bc-67661417-bfc3-45ad-bb83-dcde94bd10d5-3ec5` | `47568be` | Draft | 2026-08-16 |
| [143](https://github.com/ContextualWisdomLab/psychometrics-commons/pull/143) | feat(health): expose oldest expired scoring lease time | `cursor/bc-5498bcb8-4644-4395-b017-c5ecc66bc82b-c5b4` | `dde0072` | Ready | 2026-08-16 |
| [146](https://github.com/ContextualWisdomLab/psychometrics-commons/pull/146) | fix(session): lock session header before command persist | `cursor/bc-e2b9e666-1491-442b-bf75-83390ace0217-c8b2` | `fac56ce` | Draft | 2026-08-17 |
| [148](https://github.com/ContextualWisdomLab/psychometrics-commons/pull/148) | fix(identity): restore current projection and unique open subjects | `cursor/bc-4f4b37b8-066a-425b-94a0-da7893c16bc4-7f6a` | `094cb3b` | Draft | 2026-08-16 |
| [150](https://github.com/ContextualWisdomLab/psychometrics-commons/pull/150) | fix(research): map 0017 3NF split and drop dead withdrawal arm | `cursor/bc-7e41ca1d-7e1b-47e6-b8c1-bb100b8faee4-47f4` | `ea432ed` | Draft | 2026-08-16 |
| [151](https://github.com/ContextualWisdomLab/psychometrics-commons/pull/151) | feat(response): reload persisted snapshots after restart | `cursor/bc-d530728b-176b-4722-9648-abf3ece155c5-bc43` | `d817b1b` | Draft | 2026-08-16 |
| [152](https://github.com/ContextualWisdomLab/psychometrics-commons/pull/152) | feat(anonymous): persist short-lived credential evidence | `cursor/bc-4f13e8f0-5769-4662-b66f-397cc9362f14-2440` | `53412c6` | Ready | 2026-08-17 |
| [156](https://github.com/ContextualWisdomLab/psychometrics-commons/pull/156) | fix(consent): fail closed on reordered stored history | `cursor/bc-d83b661e-5d9c-4fb0-b796-c41f5a0ee573-b36d` | `f98e79b` | Ready | 2026-08-17 |
| [157](https://github.com/ContextualWisdomLab/psychometrics-commons/pull/157) | fix(result): fail closed on cyclic session supersession | `cursor/bc-6faf48ca-02bf-4b48-b7a4-e1bec74889bd-a83a` | `821b6a4` | Ready | 2026-08-17 |
| [165](https://github.com/ContextualWisdomLab/psychometrics-commons/pull/165) | feat(api): list startable published instruments over HTTP | `cursor/bc-11fac89c-a1eb-4ce8-a8b7-8915440515e1-fda7` | `f2f6f83` | Draft | 2026-08-16 |
| [178](https://github.com/ContextualWisdomLab/psychometrics-commons/pull/178) | test(identity): free ended subject after inspect and reconcile | `cursor/bc-504b6712-2007-48c0-884d-72a5128725f4-de7a` | `4e4079f` | Draft | 2026-08-16 |
| [181](https://github.com/ContextualWisdomLab/psychometrics-commons/pull/181) | feat(consent): persist anonymous session consent after expiry checks | `cursor/bc-2bd37ab0-7380-4dbf-87a0-b4547b9b40b1-b92f` | `a6fc258` | Draft | 2026-08-16 |
| [185](https://github.com/ContextualWisdomLab/psychometrics-commons/pull/185) | fix(item-delivery): reject deliver after item-set rebinding | `cursor/fbb857d9-64db-4130-8ff4-546ba5119582-9baf` | `1c86a95` | Ready | 2026-08-17 |
| [187](https://github.com/ContextualWisdomLab/psychometrics-commons/pull/187) | fix(research): drop public load by restricted linkage ref | `cursor/bc-43b9a267-9080-4e46-94f4-38ee5dd2921b-db96` | `1ea088a` | Ready | 2026-08-17 |
| [189](https://github.com/ContextualWisdomLab/psychometrics-commons/pull/189) | fix(result): publish snapshots only after scoring has begun | `cursor/bc-0a71352a-c557-4c0b-b9a7-5985bb33e145-cdcf` | `f3e542d` | Draft | 2026-08-16 |
| [191](https://github.com/ContextualWisdomLab/psychometrics-commons/pull/191) | fix(item-delivery): persist instrument version on ledger headers | `cursor/bc-30f29b9b-b8ba-4cd9-9f0b-94e9188d3d7f-5279` | `779b916` | Draft | 2026-08-16 |
| [194](https://github.com/ContextualWisdomLab/psychometrics-commons/pull/194) | feat(scoring): reload scoring requests and fail closed on stored blanks | `cursor/bc-e5748777-ea80-4239-9c12-2dc41aa75d1a-6fd0` | `2630652` | Ready | 2026-08-17 |
| [195](https://github.com/ContextualWisdomLab/psychometrics-commons/pull/195) | feat(api): record active-session responses over HTTP | `cursor/bc-7c52ddb8-b212-4e20-8952-48ee9b2996ad-cc98` | `6ca9d4e` | Draft | 2026-08-16 |
| [197](https://github.com/ContextualWisdomLab/psychometrics-commons/pull/197) | feat(api): record and reload item deliveries over HTTP | `cursor/bc-6cfd0f1c-ffa5-4261-a184-5c52078726ae-ba61` | `bf31c09` | Draft | 2026-08-16 |
| [204](https://github.com/ContextualWisdomLab/psychometrics-commons/pull/204) | feat(api): apply participant session commands over HTTP | `cursor/bc-9e22fdc1-4731-45da-9bdc-30221e6e9f7c-f246` | `22d1786` | Draft | 2026-08-16 |
| [206](https://github.com/ContextualWisdomLab/psychometrics-commons/pull/206) | feat(identity): persist authorized unlink from current proof | `cursor/bc-d86b0da6-c79e-4916-b8c2-ddebb8968717-dd31` | `ca29865` | Draft | 2026-08-16 |
| [211](https://github.com/ContextualWisdomLab/psychometrics-commons/pull/211) | fix(reference): name exact-spelling alias rejections | `cursor/bc-f7eff17f-f304-4816-92dd-f4fa768e41be-d399` | `83f2ee2` | Draft | 2026-08-16 |
| [213](https://github.com/ContextualWisdomLab/psychometrics-commons/pull/213) | test(instrument): prove suspend omit and same-locale catalog order | `cursor/bc-87f8e8e3-5f2a-44af-bd9a-bd92b36bb413-5f65` | `7ff4ce4` | Draft | 2026-08-16 |
| [220](https://github.com/ContextualWisdomLab/psychometrics-commons/pull/220) | fix(research): reject governance identity columns in public fixtures | `cursor/bc-29f73790-3b36-480a-a8bf-0ff4a3c071b9-0ce2` | `014f8dd` | Ready | 2026-08-20 |
| [221](https://github.com/ContextualWisdomLab/psychometrics-commons/pull/221) | fix(response): continue from reload and fail closed on gapped receipts | `cursor/bc-eeb726de-a42d-47c9-b890-9d57e5704a16-669b` | `f59de6b` | Ready | 2026-08-17 |
| [224](https://github.com/ContextualWisdomLab/psychometrics-commons/pull/224) | fix(item-delivery): reject padded persist tenant aliases | `cursor/bc-931c3cb6-d987-4641-83f8-df2093c7ccfb-9844` | `7954246` | Draft | 2026-08-17 |
| [226](https://github.com/ContextualWisdomLab/psychometrics-commons/pull/226) | fix(longitudinal): gate collection on current ledger and tenant | `cursor/bc-6f3c2357-3cd4-4eee-8738-683dcba4a0fb-e8ca` | `8c54fcc` | Draft | 2026-08-16 |
| [227](https://github.com/ContextualWisdomLab/psychometrics-commons/pull/227) | feat(session): lock stored publication during start | `cursor/bc-d042267d-8de0-4748-9d62-d8b9790a8778-c268` | `ac9d49e` | Draft | 2026-08-16 |
| [229](https://github.com/ContextualWisdomLab/psychometrics-commons/pull/229) | Cite AERA/APA/NCME 2014 and IRT sources on scoring, session, and consent docs | `cursor/measurement-standards-citations-59f0` | `febedea` | Ready | 2026-08-17 |
| [230](https://github.com/ContextualWisdomLab/psychometrics-commons/pull/230) | feat(identity): persist, recover, and unlink account links over HTTP | `cursor/bc-5f82af06-0fbb-4577-afde-d256caefb689-19d7` | `549e912` | Draft | 2026-08-16 |
| [236](https://github.com/ContextualWisdomLab/psychometrics-commons/pull/236) | test(identity): prove same-subject relink drops ended capability | `cursor/bc-add682ec-6f5d-45f4-9946-2cf6e666449b-9b92` | `2d4676e` | Draft | 2026-08-16 |
| [240](https://github.com/ContextualWisdomLab/psychometrics-commons/pull/240) | ci(release): fail closed on missing license evidence | `chore/release-legal-readiness-20260817` | `0f07570` | Ready | 2026-08-20 |
| [242](https://github.com/ContextualWisdomLab/psychometrics-commons/pull/242) | feat(audit): add append-only purpose-bound audit evidence | `feat/immutable-audit-evidence-20260817` | `b26bb7a` | Ready | 2026-08-19 |
| [245](https://github.com/ContextualWisdomLab/psychometrics-commons/pull/245) | fix(narrative): require canonical published rule references | `fix/narrative-canonical-rule-refs-20260817` | `f6f49cf` | Ready | 2026-08-19 |
| [246](https://github.com/ContextualWisdomLab/psychometrics-commons/pull/246) | docs(traceability): rebaseline shipped persist to exact 0c695b9 | `cursor/docs-rebaseline-aac99d0b-8c11` | `6b006c8` | Ready | 2026-08-18 |
| [247](https://github.com/ContextualWisdomLab/psychometrics-commons/pull/247) | feat(health): replay operator probes onto current protected main | `cursor/health-http-probes-main-8ed8` | `e68fe96` | Ready | 2026-08-18 |
| [248](https://github.com/ContextualWisdomLab/psychometrics-commons/pull/248) | feat(longitudinal): persist immutable observation evidence | `feat/longitudinal-observation-persistence-20260818` | `b472cea` | Ready | 2026-08-20 |
| [249](https://github.com/ContextualWisdomLab/psychometrics-commons/pull/249) | feat(result): authorize personal export delivery | `automation/result-export-authorization-20260818` | `a940700` | Ready | 2026-08-20 |
| [250](https://github.com/ContextualWisdomLab/psychometrics-commons/pull/250) | feat(participant): reconcile anonymous base persistence | `automation/participant-base-reconcile-20260818` | `2842506` | Ready | 2026-08-17 |
| [251](https://github.com/ContextualWisdomLab/psychometrics-commons/pull/251) | feat(scoring): enforce request-bound engine adapter results | `feat/scoring-engine-adapter-20260818` | `4bc9153` | Ready | 2026-08-19 |
| [253](https://github.com/ContextualWisdomLab/psychometrics-commons/pull/253) | feat(anonymous): reconcile credential-bound session authority | `fix/reconcile-anonymous-session-context-20260818` | `daf7d28` | Ready | 2026-08-20 |
| [254](https://github.com/ContextualWisdomLab/psychometrics-commons/pull/254) | fix(api): reject ambiguous HTTP request framing | `fix/session-http-framing-20260818` | `096038c` | Ready | 2026-08-20 |
| [255](https://github.com/ContextualWisdomLab/psychometrics-commons/pull/255) | feat: persist and reload live measurement sessions | `cursor/measurement-session-persist-reload-6a63` | `4509cb8` | Ready | 2026-08-19 |
| [256](https://github.com/ContextualWisdomLab/psychometrics-commons/pull/256) | feat(api): expose authorized personal result exports | `automation/result-export-http-20260818` | `197adfa` | Ready | 2026-08-19 |
| [257](https://github.com/ContextualWisdomLab/psychometrics-commons/pull/257) | feat(api): expose authorized immutable result reads | `feat/result-read-http-20260818` | `b649602` | Draft | 2026-08-19 |
| [258](https://github.com/ContextualWisdomLab/psychometrics-commons/pull/258) | ci(rust): refresh nightly coverage and track stable compiler | `agent/rust-toolchain-refresh-2026-08-19` | `2f1c404` | Ready | 2026-08-20 |
| [259](https://github.com/ContextualWisdomLab/psychometrics-commons/pull/259) | feat(result): render exact Korean and English personal reports | `automation/result-report-locale-20260819` | `44884af` | Ready | 2026-08-20 |
| [261](https://github.com/ContextualWisdomLab/psychometrics-commons/pull/261) | feat(assessment): bind Quick and Deep paths to immutable releases | `feat/assessment-path-contract-20260820` | `e472cd9` | Ready | 2026-08-20 |
| [262](https://github.com/ContextualWisdomLab/psychometrics-commons/pull/262) | fix(longitudinal): reject reused Commons observation identities | `fix/longitudinal-record-identity-20260820` | `d759ece` | Ready | 2026-08-20 |

## Current issue inventory

The only open repository issue observed in this snapshot is [#260](https://github.com/ContextualWisdomLab/psychometrics-commons/issues/260), **Fail closed when public-release identity inventory is unavailable**. It depends on #220. Its required distinction is:

- `inventory unavailable` when the effective restricted-identity inventory is absent or blank;
- `forbidden identity` when a supplied authoritative inventory matches a fixture cell;
- allowed `research_participant_ref` public namespace when no restricted identity is matched.

The implementation must consume an authorized product-owned inventory input, never another service database, and must not echo raw identifiers. #220 is green on its observed checks but still open with `REVIEW_REQUIRED`; neither it nor #260 is protected-main truth.

## Architecture and research basis

The gap ranking is derived from the current PRD, TRD, accepted ADRs, measurement/AI/research governance, quality attributes, release acceptance, and `docs/doctoring/standards-and-evidence.md`. The governing scientific constraints are:

- validity and intended-use evidence, precision, fairness, and reporting remain connected responsibilities;
- true-parameter recovery, bias/MAE/RMSE, uncertainty coverage, invariance/DIF, linking, scoreability, numerical-boundary behavior, and backend parity are used where applicable; correlation alone is insufficient;
- multilevel, cross-classified, multiple-membership, and temporal evidence stays explicit to avoid atomistic and temporal-leakage errors;
- AI is optional and bounded; it cannot mutate numeric scores, norms, uncertainty, DIF, invariance, or publication gates;
- Keyverse, fast-mlsirm, Gyeot, TEPP, semantic-data-portal, and contextual-orchestrator remain separate bounded contexts with versioned APIs/events/artifacts;
- SOC 2/CSAP readiness is evidence organization, not certification.

The current APA 7 sources are maintained in [`docs/doctoring/standards-and-evidence.md`](doctoring/standards-and-evidence.md), including AERA/APA/NCME (2014), Browne et al. (2001), Curran and Bauer (2011), Hamaker and Wichers (2017), Robinson (1950), ISO/IEC/IEEE 42010:2022, ISO/IEC 25010:2023, ISO/IEC 42005:2025, NIST SP 800-63-4 (2025), RFC 9457 (2023), OpenAPI 3.2.0, PostgreSQL 18 documentation, WCAG 2.2, and W3C PROV-DM. Add or revise citations there when a new material decision depends on a source not already recorded.

## Execution loop

For each open PR, in dependency and buyer-impact order:

1. Refetch `origin` and the exact PR head.
2. Inspect review comments, unresolved threads, required checks, security/SBOM/provenance findings, and base drift.
3. Fix valid findings test-first on the correct branch; respect concurrent remote commits and never force-push over them.
4. Re-run exact-head format, lint, tests, rustdoc, owned statement/branch coverage, security, SBOM, provenance, and relevant real PostgreSQL/E2E evidence.
5. Merge only after required checks, independent non-author review, and protected-main policy are satisfied; never self-approve or bypass a rule.
6. Fetch the new protected head, update traceability and this baseline when semantics changed, then take the next safe product gap.

CI/review latency is local to the affected branch. While a required check or review is pending, work can continue on documentation reconciliation, independent product slices, issue dependencies, scientific evidence, and operational tests. A green check does not prove a merge, runtime deployment, scientific validity, or commercial readiness.

## Explicit non-claims

This snapshot does not claim a released version, a production deployment, a published instrument, live Keyverse/Gyeot/TEPP/semantic-data-portal integration, SOC 2/CSAP certification, a Figma artifact, a Storybook client, a universal SLA/SLO/RPO/RTO, or a $20B valuation. Those claims require the evidence gates named above.
