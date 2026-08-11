# Release Acceptance — Psychometrics Commons

- Status: Normative release gate
- Date: 2026-08-10
- Scope: software releases and separately versioned consumer instrument releases
- Principle: a release decision is bound to one exact integrated protected head and its built artifacts. Evidence from predecessor heads, active PRs, skipped checks, drafts, target architecture, or unverifiable external claims does not transfer.

## 1. Software release gate

A software release is permitted only when all applicable evidence is simultaneously satisfied for the unchanged exact integrated protected head.

### Source and review

- protected-main source identity is exact and immutable for the release build;
- zero valid unresolved human or automated review findings;
- qualifying independent non-author review exists where repository/governance policy requires it;
- no self-approval, synthetic approval, stale reviewed-head evidence, or policy weakening;
- open PRs/issues that materially block the release are resolved, explicitly deferred with accepted scope, or documented as non-release-blocking by the governing contract.

### Deterministic CI and coverage

- formatting, compilation, lint, tests, rustdoc/docstrings and package/build checks pass;
- owned production statement and branch coverage meet the repository's exact 100% gate, with line/function/region coverage where tooling exposes it;
- coverage selection is non-vacuous and does not exclude owned behavior merely to satisfy the percentage;
- architecture/documentation fitness tests pass;
- generated/committed lockfiles and dependency manifests match the resolved build.

### Security and supply chain

- SAST, secret scanning and required dependency/security checks pass;
- release dependencies have an SBOM and traceable source/license/provenance evidence appropriate to distribution;
- actions, containers, model artifacts and other mutable build inputs are pinned/attestable where practical;
- no release credential, provider secret, raw assessment response, restricted linkage value or unnecessary PII is embedded in artifacts/logs;
- threat-model release-blocking scenarios have implementation/test evidence or an explicitly accepted residual risk by authorized governance;
- relevant known critical/high vulnerabilities have disposition evidence rather than being hidden by threshold changes.

### Persistence, migration, and recovery

When physical persistence exists:

- clean bootstrap succeeds on every supported persistence profile/version;
- upgrade from every supported prior release path is tested;
- migration/roll-forward/rollback strategy is explicit and tested;
- physical as-built schema is reconciled with the logical ERD/ADR contract;
- tenant/restricted-linkage constraints survive migration;
- backup/restore drill succeeds for the exact deployment profile and ADR-0017 evidence
  records the supported database version, encryption context, restored tenant and
  restricted-linkage/data-rights state, key/secret recovery, duplicate-effect
  prevention, measured restore duration, and recoverable data point; restored
  immutable digests/provenance must reconcile;
- worker/outbox/inbox/job state resumes without duplicate domain effects after restart/restore.

### Runtime and degraded-mode acceptance

- liveness/readiness/capability-health behavior matches `docs/OPERABILITY.md`;
- optional dependency failures degrade only their capability and never fabricate scientific/product evidence;
- scoring outage preserves completed response snapshots and produces no invented score;
- AI outage preserves numeric result retrieval with deterministic narrative fallback when that feature is implemented;
- research-catalog outage does not block personal results;
- TEPP outage does not discard accepted longitudinal observations;
- tenant/resource authorization failures remain fail closed;
- operational telemetry exposes actionable state/backlog/failure class without routine sensitive payload logging.

### API, event, and compatibility evidence

When HTTP/event transports exist:

- as-built OpenAPI/AsyncAPI/schema artifacts match the actual implemented operations/events rather than target-only routes;
- stable problem/error codes preserve safe machine-readable failure semantics;
- idempotency, replay, version negotiation and unsupported-major fail-closed behavior are tested;
- compatibility window and historical result/release readers are verified for supported retained versions;
- no mutable alias such as `latest` is stored as historical provenance.

### Accessibility and product journey

For supported reference clients:

- core assessment and result flow meets the repository's WCAG 2.2 AA target through automated plus manual/assistive-technology evidence;
- keyboard-only assessment/data-rights flows work;
- chart/result information has non-color and text/table equivalents;
- locale resolution never silently changes assessment language;
- anonymous-first core journey and optional account-linking behavior match the PRD;
- refusal of optional research contribution does not block personal result access.

### Documentation and due-diligence consistency

The release must not ship with docs that contradict protected-main behavior. At minimum reconcile:

- PRD/TRD;
- Architecture/C4/UML/ERD;
- relevant ADRs;
- Threat Model, Test Strategy, Operability;
- Measurement/AI/Research Governance;
- Traceability/Roadmap/Documentation Assessment;
- Risk Register/Compliance Readiness;
- AGENTS/CLAUDE/README/CHANGELOG;
- release notes, migration/rollback and operator guidance.

Architecture-defined, implemented, verified, measured-operational, and independently certified/attested claims remain distinct.

## 2. Consumer instrument release gate

A software release does not automatically authorize any instrument version for operational publication. Every published instrument/version/locale needs an immutable evidence bundle governed by ADR-0019.

Required evidence, as applicable to the intended use:

### Rights and content provenance

- item/instrument redistribution and operational-use rights;
- exact item/version/content digests;
- source/adaptation provenance;
- translation/adaptation reviewer provenance;
- locale-specific instructions, response scales and presentation rules;
- known limitations and prohibited use.

### Measurement and scoring evidence

- exact AssessmentSpec/scoring/calibration version/digest;
- appropriate true-parameter and/or score recovery evidence including bias/MAE/RMSE;
- uncertainty/SE/interval coverage where reported;
- convergence/failure and numerical-boundary behavior;
- factor-retention and structural-model relation evidence appropriate to the actual candidate models;
- residual dependence/testlet evidence where relevant;
- scoreability before interpreting bifactor general/specific scores;
- DIF/invariance evidence for claimed group/language comparison;
- linking/equating anchor stability for linked forms;
- norm population, sampling limitations, version and effective scope;
- CPU/GPU parity if a GPU numerical path is used;
- CAT/ATA precision/content/exposure/safety evidence if enabled.

Correlation alone is not sufficient evidence of parameter recovery, score agreement, uncertainty calibration or intended-use validity.

### Narrative evidence

If Personality Style/narrative is enabled:

- continuous/facet score profile remains the scientific source;
- style-assignment key and mapping/rule version are immutable and replayable;
- mixed/adjacent boundary semantics are tested;
- deterministic localized fallback exists;
- optional AI rendering is constrained to approved measured evidence and cannot change numeric scores, uncertainty, norms, DIF or publication gates;
- no unsupported MBTI-equivalence, diagnosis, fixed-essence, treatment or high-stakes suitability claim.

### Multilingual and accessibility evidence

- exact BCP 47 locale/version publication;
- no silent item-content fallback;
- linguistic/construct review evidence;
- cross-language comparison/shared norm disabled unless linking/invariance/DIF evidence supports it;
- accessibility accommodations/presentation changes with potential response-process impact are versioned/evaluated, not hidden as cosmetic differences.

## 3. Research dataset release gate

A Research Commons release additionally requires:

- explicit eligible research contribution scope;
- research-domain pseudonyms only;
- prohibited operational/Keyverse/linkage identifiers absent;
- de-identification/privacy-risk review including rare combinations and free-text risk where relevant;
- scientific/data-quality review;
- immutable dataset snapshot and transformation/codebook/variable-dictionary/data-card/license/consent-scope manifests;
- exact instrument/item/scoring/calibration/norm provenance;
- citation metadata and cryptographic checksums;
- access-class authorization rules for controlled releases;
- supersession/correction semantics rather than in-place replacement of published bytes;
- semantic-data-portal registration digest reconciliation.

## 4. Release blockers

The following are release blockers unless explicitly not applicable by the governing scope:

- required check queued/pending/skipped/cancelled/failed/absent;
- unresolved valid review/security finding;
- unsupported or stale source/base evidence;
- coverage below policy or vacuous coverage selection;
- migration/restore failure;
- cross-tenant authorization failure;
- known operational identifier leakage into a research-release fixture;
- unsupported scientific/score-use claim;
- missing rights evidence for an instrument being operationally published;
- missing translation/invariance evidence for a cross-locale comparison claim;
- missing scoreability evidence for a score being interpreted;
- reproducibility/provenance digest mismatch;
- release artifact not matching the exact accepted source/build provenance;
- P0 security/privacy/scientific/data-integrity risk without authorized acceptance;
- documentation claiming GA/certification/SLO capabilities not supported by measured/current evidence.

## 5. Post-release verification

After publishing:

1. fetch and verify released artifact/package/container digests;
2. verify provenance/SBOM bind to the expected protected head;
3. execute minimum installation/startup/readiness smoke tests for each supported distribution profile;
4. verify migrations/read compatibility where applicable;
5. verify release notes/CHANGELOG/version endpoints agree;
6. monitor for immediate security/runtime regressions;
7. preserve the release-evidence manifest for acquisition/reproducibility review.

A release is not complete because a tag or GitHub Release object exists.

## 6. References

International Organization for Standardization & International Electrotechnical Commission. (2023). *ISO/IEC 25010:2023 Systems and software engineering—Systems and software Quality Requirements and Evaluation (SQuaRE)—Product quality model*.

National Institute of Standards and Technology. (2022). *Secure Software Development Framework (SSDF) Version 1.1: Recommendations for mitigating the risk of software vulnerabilities* (NIST SP 800-218). https://doi.org/10.6028/NIST.SP.800-218

Open Worldwide Application Security Project. (2025). *Application Security Verification Standard 5.0.0*.

World Wide Web Consortium. (2024). *Web Content Accessibility Guidelines (WCAG) 2.2*.
