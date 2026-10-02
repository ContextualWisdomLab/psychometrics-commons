# Product Requirements Document — Psychometrics Commons

- Status: Implementation baseline
- Version: 0.1
- Date: 2026-08-09
- Product owner: ContextualWisdomLab

## 1. Product definition

Psychometrics Commons is a headless psychometric assessment and research platform organized around the lifecycle:

```text
Measure -> Understand -> Reflect -> Observe Over Time -> Contribute to Science
```

The consumer product must make scientifically defensible assessment understandable without converting continuous measurement into unsupported personality typing. The research product must preserve immutable instrument, scoring, norm, consent, and release provenance so results and datasets remain reproducible.

## 2. Primary users

### Individual participant

Wants to complete an assessment with low friction, understand a continuous profile, retain or export results when desired, and separately decide whether to contribute data to research.

### Longitudinal participant

Wants optional repeated EMA/ESM observations and within-person change insights without confusing those changes with between-person traits.

### Researcher and instrument developer

Wants versioned instrument publication, pilot/calibration evidence, DIF/invariance analysis, norms, reproducible data releases, and machine-readable provenance.

### Product operator and data steward

Wants explicit publication, consent, data-rights, release-approval, security, and audit workflows without direct cross-service database access.

### Institutional integrator

Wants headless APIs, replaceable clients, identity federation, self-hosted or residency-constrained deployment, and stable versioned contracts.

## 3. Consumer MVP

### 3.1 Assessment

The first supported consumer assessment is an IPIP-based Big Five instrument family.

Required capabilities:

- Korean and English instrument versions;
- anonymous participation by default;
- optional Keyverse account linking;
- Quick and Deep assessment paths;
- continuous Big Five scores as the measurement source of truth;
- facet-level results where the published instrument supports them;
- uncertainty and interpretation limitations;
- pause/resume and deterministic recovery from client reconnect;
- immutable result snapshots;
- JSON export and a human-readable report export.

### 3.2 Personality narrative

Personality Style is a presentation layer, not a psychometric score.

Requirements:

- the continuous/facet profile remains visible;
- mapping rules are separately versioned from scoring;
- adjacent or mixed styles are allowed near boundaries;
- no claim of MBTI equivalence and no forced 16-type design;
- narrative text must be grounded in a pinned ScoreProfile and approved interpretation rules;
- deterministic localized fallback text is mandatory when AI is unavailable or rejected;
- narratives must not imply diagnosis, fixed essence, treatment need, employment fitness, or other unsupported high-stakes conclusions.

### 3.3 Reflection modules

Self-compassion and future reflection constructs are separate instruments rather than inferences from Big Five.

Operational publication requires:

- item and instrument rights evidence;
- translation provenance where localized;
- intended factor/measurement structure evidence;
- scoring and interpretation provenance;
- explicit participant choice to enter the module.

## 4. Longitudinal product

Longitudinal participation is optional and separately consented.

Required behavior:

- Gyeot-compatible EMA/ESM collection;
- offline-first client recovery;
- preservation of observed, recorded, received, available, and validity times where relevant;
- preservation of timezone/civil-time context;
- explicit multiple-membership/context references;
- TEPP-backed temporal/event/multilevel analysis rather than product-side duplicate numerical models;
- output that distinguishes within-person change from between-person differences.

## 5. Research Commons

Research contribution is never implicit in service use.

Required flow:

```text
personal result
-> explicit research opt-in
-> pseudonymization boundary
-> research participant
-> de-identification/privacy-risk review
-> immutable dataset snapshot
-> scientific/release approval
-> semantic-data-portal registration
```

A release bundle must include, as applicable:

- Parquet and CSV data;
- codebook and variable dictionary;
- data card and known limitations;
- license and consent scope;
- exact instrument, item, scoring, calibration, and norm versions;
- privacy review evidence;
- citation metadata;
- cryptographic checksums and supersession metadata.

Public release bundles must not contain Keyverse subject references, operational participant references, or restricted linkage keys.

## 6. Measurement Workbench

The Workbench must support the governed lifecycle of an instrument without copying psychometric numerical kernels into this repository.

Target capabilities:

- construct and instrument definition;
- item/version management;
- translation and cultural-adaptation review;
- pilot status and calibration evidence references;
- scoring policy and norm version activation;
- DIF/invariance evidence;
- linking/equating evidence;
- CAT/ATA configuration where supported by fast-mlsirm;
- publish, suspend, retire, and supersede workflows;
- research-release approval handoff.

Inkspan may provide authoring/editor primitives but is not a source of truth for published instrument state.

### 6.1 CEFR language-assessment consumer boundary

The first language-assessment consumer is an English A1-B2 placement profile. It
consumes the exact `cwl_cefr_language_assessment/v1` profile from
`learning-interoperability-contracts` through immutable commit and schema
digests. Its result envelope declares the exact
`cwl_cefr_language_assessment/result_snapshot/v1` contract version. The
consumer stores only opaque product references and four required domains:
reading reception, listening reception, written production, and spoken
production.

The initial consumer is profile-only and permits `cefr_aligned` claims. A
profile-only result may contain a unique measured subset of the required
domains; insufficient or unmeasured domains remain explicit in the upstream
result envelope. It does not authorize an overall level, `cefr_linked`, or
`certification_decision` claim until the exact blueprint and governed
standard-setting/linking or certification evidence are available. The upstream
contract validator remains the schema authority; Psychometrics Commons verifies
its evidence identity and product bindings.

## 7. Product boundaries

### Psychometrics Commons owns

- hosted runtime and product APIs;
- instrument publication lifecycle;
- participant/session lifecycle;
- item delivery and response events;
- consent/data-rights workflows;
- scoring dispatch and immutable result snapshots;
- resource authorization and tenant context;
- product persistence/migrations;
- research-contribution handoff;
- reference-client composition and deployment profiles.

### Psychometrics Commons does not own

- psychometric formulas or numerical kernels (`fast-mlsirm`);
- credentials/federation implementation (Keyverse);
- temporal/event model kernels (TEPP);
- public research catalog internals (`semantic-data-portal`);
- generic LLM routing (`contextual-orchestrator`);
- bulk model execution (`pg-llm-batch`);
- external-call security kernel (EgressWeave).

## 8. Initial exclusions

The first product release does not provide:

- clinical diagnosis or screening claims;
- treatment or medication recommendations;
- employment, promotion, admission, insurance, credit, or legal decisions;
- official MBTI assessment or claims of MBTI equivalence;
- unvalidated IQ or aptitude conclusions;
- analysis of a third person's mental state without an appropriate product and legal basis;
- research reuse without explicit research contribution consent.

## 9. Functional acceptance criteria

The consumer vertical slice is not release-ready until all of the following are demonstrated on one integrated protected head:

1. an anonymous participant can start, pause, resume, complete, score, and retrieve a result;
2. duplicate commands and response submissions are idempotent;
3. completion freezes an immutable response snapshot before scoring;
4. every result pins instrument, response snapshot, AssessmentSpec, scoring, calibration, optional norm, narrative, consent, and engine provenance;
5. LLM unavailability does not block numeric scoring or basic result access;
6. refusal of research contribution does not block the personal result;
7. account linking does not rewrite historical participant/result identifiers;
8. research-release preparation cannot export operational identity references;
9. Korean/English sessions resolve an exact published locale version without silent content fallback;
10. supported reference clients meet the WCAG 2.2 AA acceptance target.
11. the English A1-B2 profile rejects mismatched contract/version/schema,
    blueprint, evidence, domain, claim, and overall-reporting bindings.

## 10. Measurement acceptance criteria

A published scoring path must rely on fast-mlsirm evidence appropriate to the model and intended use, including as applicable:

- true-parameter bias and RMSE;
- interval coverage and convergence;
- score/parameter recovery;
- linking-anchor stability;
- DIF/invariance evidence;
- numerical-boundary behavior;
- CPU/GPU parity where a GPU path is used;
- scoreability evidence before reporting bifactor general or specific scores.

Correlation alone is not accepted as evidence of estimation accuracy or score validity.

## 11. Security and privacy acceptance criteria

- services do not share application databases;
- public identifiers are opaque and non-numeric;
- identity, operational assessment data, restricted linkage data, and research-release data use separate authorization boundaries;
- optional AI processing has explicit purpose, provider class, residency, retention, and allowed-data policy;
- no client receives service credentials;
- cross-tenant access is denied by server-side authorization and covered by negative tests;
- data export/deletion requests are durable domain resources with verified requester identity and audited outcomes.

## 12. Success metrics

Product metrics:

- assessment start-to-completion rate;
- Quick-to-Deep conversion;
- reflection opt-in rate;
- result-understanding/usefulness feedback;
- longitudinal return rate.

Scientific/measurement metrics:

- parameter and score recovery error;
- interval coverage;
- DIF/invariance performance;
- scoring reproducibility;
- published-version replay success.

Research metrics:

- explicit research opt-in rate;
- proportion of contributions eligible for release after privacy review;
- release-package completeness;
- reproducible dataset rebuild success.

Operational metrics:

- session resume success;
- scoring completion/failure classification;
- result-read availability during optional dependency outages;
- export/deletion SLA compliance;
- cross-service outbox/inbox delivery and duplicate-suppression health.

## 13. Release policy

A release requires exact-head CI, security, coverage, packaging, SBOM/provenance, reproducibility, compatibility, accessibility, independent review, migration/rollback, and product acceptance evidence. Instrument releases additionally require rights, translation/version, scoring/calibration/norm, and intended-use evidence.
