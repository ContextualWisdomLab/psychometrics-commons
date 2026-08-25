# Psychometrics Commons Product Delivery Roadmap

- Status: Delivery baseline
- Date: 2026-08-09
- Principle: finish current actionable PR/issue work before opening conflicting or speculative parallel slices; when current work is blocked by CI, review, provider cooldown, another writer, or a read-only dependency, continue independent bounded repository work; each phase exit criterion is evidence-based

The roadmap is not a release-date promise. It is the dependency order for turning the architecture into a hosted product without moving psychometric numerics, identity, temporal analysis, or research catalog ownership into this repository.

## Phase 0 — Architecture, governance, and contract baseline

Deliverables:

- PRD and TRD;
- accepted ADR baseline and ADR template;
- psychometric measurement governance;
- bounded AI governance;
- Research Commons governance;
- architecture context/container/component views;
- UML-aligned class/state/sequence views;
- logical ERD;
- security/privacy/data-boundary model;
- deployment/operations/recovery model;
- measurable quality-attribute scenarios;
- compliance-readiness evidence model and explicit certification non-claim;
- material risk register;
- canonical product/scientific/identity/research/operations glossary;
- requirements/code/evidence traceability;
- documentation completeness assessment;
- repository governance (`AGENTS.md`, `CLAUDE.md`, changelog discipline);
- repository fitness tests that prevent the required view/governance set and ADR index from silently disappearing.

Exit criteria:

- material ownership and dependency-direction decisions have accepted ADR coverage;
- target and as-built documentation are clearly distinguished;
- no known contradiction across PRD/TRD/ADR/governance/Architecture;
- scientific/AI/research authority boundaries are explicit;
- quality goals are expressed as measurable scenarios rather than unsupported adjectives;
- architecture mitigation, verified control, risk closure, and external certification are not conflated;
- implementation backlog can be derived without inventing domain ownership or relying on scheduler-only memory.

## Phase 1 — Hosted runtime domain core

Deliverables:

- server-authoritative session state machine;
- idempotent response ledger;
- immutable response snapshots;
- version-pinned scoring-dispatch contract;
- immutable result provenance and supersession;
- purpose-specific consent/research-contribution lifecycle;
- durable data-rights lifecycle;
- immutable instrument publication/version lifecycle;
- tenant/resource authorization domain primitives.

Exit criteria:

- exhaustive lifecycle transition tests;
- conflicting replay fails closed;
- exactly 100% owned-production statement and branch coverage, with no meaningless exclusions or tests added only to satisfy a percentage;
- no psychometric numerical duplication;
- no transport/database implementation is required to understand/test domain semantics.

## Phase 2 — Persistence, jobs, and integration consistency

Deliverables:

- PostgreSQL-compatible physical migrations aligned with logical ERD;
- persistence adapters for domain aggregates;
- transactional outbox/inbox;
- durable scoring-job worker;
- bounded retries, cancellation, quarantine, and reconciliation;
- health/readiness capability model;
- migration/rollback/restore test harness.

Exit criteria:

- completion + response snapshot + scoring outbox are atomic;
- concurrent/idempotent replay tests pass against real persistence;
- no direct cross-service database credentials;
- crash between transaction and delivery does not lose or duplicate business effects;
- backup/restore preserves tenant, provenance, linkage, and deduplication invariants.

## Phase 3 — Public/admin API and scoring integration

Deliverables:

- versioned public/admin HTTP API;
- OpenAPI 3.2.x as-built contract;
- RFC 9457-compatible error model;
- exact fast-mlsirm contract adapter;
- deterministic test double for scoring integration;
- asynchronous result retrieval;
- deterministic narrative fallback.

Exit criteria:

- anonymous start → response → completion → score → result works end to end;
- exact contract/digest/version provenance is persisted and returned;
- scoring outage yields pending/retryable state without invented score;
- unsupported/scientific failures fail closed with typed safe errors;
- OpenAPI validation and implementation contract tests pass.

## Phase 4 — Identity, consent, and data-rights integration

Deliverables:

- optional Keyverse token validation adapter;
- anonymous-session credential design;
- secure anonymous-to-account linking;
- resource- and tenant-level authorization;
- purpose-specific consent API;
- export/deletion durable workers and dependent-system propagation;
- restricted research identity linkage persistence.

Exit criteria:

- account linking proves control of both identities and never rewrites historical IDs;
- cross-tenant negative tests pass;
- refusal of research consent does not block personal result;
- data-rights requests are identity-verified, durable, auditable, and preserve explicit retention exceptions;
- NIST/OAuth security requirements applicable to the chosen flow are tested.

## Phase 5 — Research Commons release path

Deliverables:

- research contribution eligibility projection;
- pseudonymized staging snapshot;
- de-identification/privacy-risk review workflow;
- scientific/release approval workflow;
- immutable dataset/release manifest;
- semantic-data-portal registration adapter;
- AsyncAPI 3.1.x contract when durable events are exposed.

Exit criteria:

- public release fixture contains no Keyverse subject, operational participant reference, or linkage key;
- manifest digest/release ID replay is idempotent and conflicting digest fails closed;
- release package contains codebook, variable dictionary, data card, license, consent scope, exact measurement provenance, privacy review, citation metadata, and checksums;
- portal outage does not affect personal results and registration reconciles later.

## Phase 6 — Multilingual consumer product

Deliverables:

- IPIP-based Big Five Korean and English published instrument versions with rights/provenance;
- Quick and Deep paths;
- continuous/facet ScoreProfile;
- uncertainty and limitations;
- versioned Personality Style mapping;
- deterministic Korean/English narratives;
- standalone Public Assessment client;
- Result Explorer;
- JSON and human-readable report export (domain copy is protected main through merged #231; HTTP transport remains here);

Exit criteria:

- no silent item-language fallback;
- translation/construct review evidence is attached to each locale version;
- any cross-locale comparison/shared norm is enabled only after linking/DIF/invariance/recovery evidence;
- WCAG 2.2 AA acceptance for the supported reference client;
- AI disabled mode completes the whole core result journey.

## Phase 7 — Reflection and longitudinal experience

Deliverables:

- independently measured reflection module framework;
- rights/validation gate for self-compassion or other constructs;
- Gyeot enrollment/sync integration;
- temporal observation contract;
- TEPP analysis job/artifact adapter;
- optional LifeOS review/goal integration.

Exit criteria:

- reflection constructs are not inferred from Big Five;
- repeated observations preserve observed/recorded/received/available time and timezone context;
- multiple membership is not collapsed;
- outputs distinguish within-person change from between-person variation;
- longitudinal withdrawal/retention behavior is tested.

## Phase 8 — Measurement Workbench

Deliverables:

- construct/instrument/item authoring and review;
- Inkspan authoring adapter;
- instrument/item/version publication workflow;
- translation/cultural-adaptation workflow;
- calibration, norm, DIF/invariance, linking/equating evidence references;
- CAT/ATA policy reference where supported by fast-mlsirm;
- RankWeave-backed research/instrument discovery where useful;
- immutable approval and audit history.

Exit criteria:

- Workbench cannot publish an instrument without required evidence for its intended use;
- publication is versioned/immutable and can be suspended/retired without changing history;
- authoring tools do not become the product system of record;
- fast-mlsirm remains independently usable and product-specific workflows remain downstream.

## Phase 9 — Enterprise/self-hosted readiness

Deliverables:

- deployment packaging and upgrade path;
- tenant isolation and SSO/federation policy;
- residency, retention, encryption, secret-manager integration;
- backup/restore and disaster-recovery drills;
- metrics/traces/alerts and operator runbooks;
- SBOM, signed/reproducible provenance, vulnerability management;
- customer-owned client/embed integration;
- release rollback/roll-forward procedures;
- scope-specific assurance evidence registry for applicable SOC 2/CSAP readiness work.

Exit criteria:

- profile-specific SLO/RPO/RTO defined and measured;
- restore drill passes on current schema/application version;
- isolation/security/failure-injection/migration/accessibility/release gates pass on exact integrated head;
- critical/high risk register items are closed with evidence or explicitly accepted by the authorized risk owner;
- architecture/control evidence supports a current external assurance assessment without claiming certification prematurely.

## Phase 10 — GA release evidence

GA is not inferred from feature count.

Required evidence on one exact protected release head:

- product acceptance journey;
- all required CI/security/coverage/accessibility/packaging gates;
- instrument rights and intended-use validation;
- measurement recovery/invariance/provenance evidence;
- OpenAPI/AsyncAPI contract validation for deployed surfaces;
- migration and backup/restore evidence;
- SBOM, signed/reproducible build provenance;
- independent review;
- release runbooks and verified rollback/roll-forward;
- measured operational SLO/RPO/RTO;
- no unresolved P0 product/security/privacy/scientific finding;
- no unaccepted critical/high risk that invalidates the release claim;
- assurance/readiness claims limited to evidence actually held for the scoped deployment.

Only then is version bump/release appropriate.

## Continuous product-gap loop

When all current PRs/issues are genuinely non-actionable or exhausted, inspect in this order:

1. broken/unfinished end-to-end participant journey;
2. scientific/provenance correctness gap;
3. security/privacy/tenant/data-rights gap;
4. persistence/recovery/reliability gap;
5. accessibility/multilingual gap;
6. Research Commons reproducibility gap;
7. Workbench productivity gap;
8. enterprise deployment/operator/assurance gap;
9. buyer-visible UX/onboarding gap;
10. measured customer-value/ROI evidence gap.

Select the smallest bounded slice that materially reduces the highest-ranked gap, implement it test-first, validate exact head, merge when allowed, then immediately return to the PR queue.
