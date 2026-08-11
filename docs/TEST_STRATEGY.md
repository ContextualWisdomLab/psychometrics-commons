# Test Strategy — Psychometrics Commons

- Status: Normative verification strategy
- Date: 2026-08-10
- Applies to: product runtime, persistence, public/admin transports, client contracts, CWL integrations, research release, longitudinal orchestration, and instrument-release evidence
- Source-of-truth rule: this document defines required verification classes; `docs/TRACEABILITY.md` records which evidence exists on a named protected-main baseline.

## 1. Purpose

Psychometrics Commons cannot equate code coverage, one green workflow, or a model-generated review with product correctness. The test system therefore separates structural coverage, domain correctness, scientific validity, security/privacy, interoperability, accessibility, recovery, and release acceptance.

Every product mutation should answer three questions:

1. **Does the implementation satisfy the intended product/domain contract?**
2. **Does it fail closed or degrade safely at trust, concurrency, persistence, dependency, and scientific boundaries?**
3. **Is the evidence bound to the exact source/artifact/version being released?**

## 2. Test pyramid and evidence classes

| Layer | Purpose | Typical evidence |
|---|---|---|
| unit | local invariants and typed failure semantics | deterministic Rust tests |
| property/state machine | replay, transition, ordering and supersession invariants across many cases | property/exhaustive transition tests |
| persistence integration | uniqueness, transaction atomicity, rollback, migration, crash recovery | real supported PostgreSQL tests |
| contract | serialized API/event/scoring/integration behavior | OpenAPI/AsyncAPI/schema or typed contract tests once transports exist |
| service integration | Keyverse, fast-mlsirm, semantic-data-portal, TEPP/Gyeot, orchestrator adapters | version-pinned contract fixtures + bounded live conformance where appropriate |
| security/privacy/tenancy | authorization, isolation, leakage, egress, supply chain | negative tests, SAST, dependency/secret scanning, threat-model cases |
| scientific/measurement | estimator/score/invariance/linking/norm/scoreability evidence | upstream fast-mlsirm recovery artifacts + product publication-gate verification |
| accessibility | participant/researcher/operator interaction without construct-irrelevant barriers | automated checks + keyboard/screen-reader/manual AT E2E |
| failure/recovery | durable behavior through crash, queue, database and dependency failures | failure injection, restore drills, restart/reconciliation |
| product E2E | buyer/participant journey and rights workflows | browser/API E2E against integrated profile |
| release acceptance | one exact protected head/artifact satisfies all required evidence together | signed/provenanced release manifest and acceptance checklist |

No evidence class substitutes for another. 100% statement/branch coverage does not prove tenant isolation, psychometric validity, recovery, accessibility, or correct release provenance.

## 3. TDD contract

For production defects and new behavior:

1. reproduce the intended boundary with the smallest realistic test;
2. observe the expected **RED** caused by the missing/incorrect product behavior rather than by test setup, import, fixture, network, or unrelated dependency failure;
3. implement the narrowest root-cause change;
4. observe focused **GREEN**;
5. run the complete relevant suite and exact-head CI/security/coverage evidence;
6. update traceability/ADR/ERD/UML/contract artifacts when the governing behavior changed.

Tests that never reach the intended production boundary do not count as RED evidence.

## 4. Domain state-machine verification

### Assessment session

Exhaustively verify allowed and denied transitions for:

```text
created -> active <-> paused -> completed -> scoring -> scored -> released
expired | cancelled | invalidated
```

Required properties:

- normal responses accepted only while active;
- exact command replay idempotent;
- conflicting reuse fails closed;
- completion freezes one immutable canonical response snapshot;
- later commands cannot rewind or alter frozen evidence;
- scoring cannot start without durable completed snapshot evidence.

### Instrument publication

Verify Draft/Review/Published/Suspended/Retired semantics, immutable published bytes, exact locale/item/order/digests, reactivation only for the same compatible published evidence, and ADR-0019 publication evidence gates.

### Consent, research contribution, data rights

Verify purpose separation, absence-by-default optional consent, append-only revocation/withdrawal evidence, requester-specific identity verification, explicit retained-scope evidence, and no research eligibility after applicable withdrawal.

### Account linking

Verify proof of both identities, exact replay, conflicting proof reuse, cross-tenant denial, unlink/recovery semantics, and immutable historical participant/session/response/result identifiers according to ADR-0020.

## 5. Idempotency, concurrency, and ordering

Every externally retryable write requires tests for:

- exact duplicate replay;
- same idempotency identity with changed content;
- concurrent duplicate submissions;
- out-of-order network arrival;
- restart between acceptance and response;
- cross-tenant and cross-resource key collisions;
- server-authoritative sequence allocation;
- client-clock skew and duplicate offline synchronization.

A last-write-wins implementation is prohibited where it would rewrite scientific, consent, identity, or research-release evidence.

## 6. Persistence and migration tests

Physical persistence tests use a **real supported PostgreSQL version**; an in-memory fake cannot establish SQL constraints, transaction isolation, locking, rollback, or migration compatibility.

When each logical entity becomes persisted, test as applicable:

- clean bootstrap from empty database;
- idempotent migration application where designed;
- exact constraint/uniqueness behavior;
- caller-owned local transaction atomicity;
- rollback after domain and outbox mutation;
- concurrent conflicting writes;
- forward migration from each supported prior version;
- rollback or documented roll-forward-only behavior;
- backup taken on supported version and restored into the intended recovery profile;
- restored digest/provenance equality;
- tenant and restricted-linkage authorization after restore;
- no mutation of published immutable scientific artifacts during schema migration.

The logical ERD is checked against as-built migrations once those migrations exist. Silent divergence is a release defect.

## 7. Transactional outbox/inbox and workers

Required failure-injection points include:

1. before local business transaction commits;
2. after business mutation but before outbox insert attempt;
3. after committed outbox but before transport publication;
4. duplicate transport delivery;
5. after inbox `pending` creation;
6. after `processing` claim but before local/external side effect;
7. after external side effect but before completion evidence persistence;
8. process crash/restart;
9. bounded retry exhaustion and quarantine;
10. reconciliation after downstream recovery.

Receipt must never be treated as completion of an effect that has not occurred. Stable downstream idempotency/evidence is required for non-local effects.

## 8. Authorization, tenancy, and identity security tests

For every product resource family test:

- unauthenticated/invalid token behavior where authentication is required;
- valid subject, wrong tenant;
- valid tenant, wrong resource ownership;
- guessed opaque references;
- role confusion between Keyverse administration, instrument publishing, research approval, tenant administration, and participant ownership;
- no implicit/default tenant on writes;
- sharing token audience/resource mismatch;
- account-link replay/conflict/cross-tenant attempts;
- restricted research linkage inaccessible to normal analytics/product paths.

Authorization tests must exercise server-side decision code and, after persistence/API exists, the real query/mutation boundary rather than controller mocks only.

## 9. Privacy and research-release tests

Every release pipeline requires both automated and review evidence.

Automated negative fixtures must detect and reject at least:

- Keyverse subject references;
- operational participant references;
- identity-linkage references/keys;
- service/provider credentials;
- unapproved direct identifiers;
- variables outside the release allowlist/scope;
- manifest/dataset digest mismatch;
- consent-scope mismatch;
- contribution that is withdrawn or otherwise ineligible under the disclosed policy.

Privacy-risk review additionally evaluates rare combinations, longitudinal sparsity/uniqueness, small cells, free-text risk and linkage with public auxiliary information. A green identifier regex is not de-identification proof.

Release rebuild tests must reproduce artifact digests from the immutable dataset snapshot and exact transformation/version bundle.

## 10. Scoring and scientific validity evidence

Psychometric numerical tests belong in `fast-mlsirm`; Psychometrics Commons verifies that the exact approved evidence is pinned and enforced for the intended instrument/use.

A product publication/scoring path requires, as applicable:

- true-parameter and/or score bias, MAE/RMSE;
- standard error/interval coverage;
- convergence/failure rates and numerical-boundary behavior;
- CPU/GPU parity for enabled backends;
- factor-retention and structural-model comparison evidence appropriate to the actual relation/boundary;
- residual dependence/testlet evidence;
- multilevel/cross-classified/multiple-membership recovery when the design contains those structures;
- DIF/invariance evidence for intended group/language comparisons;
- linking/equating anchor stability;
- norm sample/version/effective-population evidence;
- scoreability before bifactor general/specific score interpretation;
- CAT/ATA safety/precision/content constraints when enabled.

**Correlation alone is not accepted as parameter recovery, score agreement, uncertainty calibration, or validity evidence.**

Product tests must prove that missing/expired/wrong-version evidence blocks publication or unsupported score use rather than invoking a simpler fallback model silently.

## 11. Narrative and AI verification

The narrative layer is tested separately from scientific scoring.

Required properties:

- exact ScoreProfile + style-assignment key/rules/locale yields deterministic base interpretation;
- adjacent/mixed style semantics are stable at boundaries;
- narrative version changes never mutate historical numeric results;
- no MBTI-equivalence, diagnosis, fixed-essence, treatment or high-stakes suitability claim enters approved default output;
- contextual-orchestrator/model outage returns deterministic localized fallback;
- malicious/non-schema/non-finite/oversized/provenance-mismatched provider output is rejected;
- model output cannot call privileged actions or alter score/norm/uncertainty/DIF/publication/release gates;
- data/provider/residency/retention policy refuses an ineligible model route rather than privacy-downgrading.

Model-backed conformance uses `NVIDIA_NIM_API_KEY` through approved GitHub Secrets and preferably contextual-orchestrator when applicable. It never replaces deterministic CI authority.

## 12. Longitudinal tests

Gyeot/Commons/TEPP contracts require:

- offline observation creation and idempotent replay;
- daylight-saving and timezone transitions;
- clock skew and impossible ordering flags;
- validity-time interval plus source-recorded, platform-received, and platform-ingested
  timestamp preservation;
- explicit multiple-membership context and weight validation;
- no silent single-group collapse;
- deterministic immutable analysis input snapshot refs;
- within-person versus between-person recovery fixtures in the analytical owner;
- TEPP outage leaves observations durable and retryable without fabricated insight.

## 13. Multilingual and accessibility tests

Assessment content cannot silently fall back to another language. Each published locale resolves a pinned instrument version.

Cross-language comparison/shared-norm features remain disabled until required linking/invariance/DIF evidence is accepted.

Supported reference clients target WCAG 2.2 AA and require, at minimum:

- keyboard-only complete assessment/results/data-rights flow;
- programmatic labels, errors, status changes, heading/landmark semantics;
- predictable focus management;
- target size and non-drag alternatives as applicable;
- zoom/reflow and non-color-only result communication;
- text/table equivalents for charts;
- accessible authentication/linking behavior;
- screen-reader/manual assistive-technology E2E before GA.

Accessibility accommodations that may alter the response process are captured as instrument/presentation evidence rather than hidden as cosmetic variation.

## 14. Performance and reliability

Do not invent universal latency/SLO numbers before a deployment profile is measured. Performance tests establish profile-specific evidence for:

- session start/response/complete under expected concurrency;
- PostgreSQL contention and pool exhaustion;
- outbox/inbox backlog and recovery;
- scoring queue throughput and bounded retry;
- large response/release payload limits;
- data-rights batch behavior;
- longitudinal ingestion bursts;
- optional dependency timeout/circuit/degradation behavior.

A performance optimization that changes scientific results, ordering, security semantics, or failure classification is a correctness regression.

## 15. Coverage policy

Owned production code targets exact 100% statement and branch coverage, plus line/function/region coverage where tooling exposes it.

Coverage gates must be non-vacuous: they fail if the selected production set unexpectedly contains zero relevant units. Tests may not exclude difficult production branches merely to reach a percentage. Generated/vendor/external code may be excluded only by explicit repository policy and must not hide owned behavior.

## 16. Exact-head and release evidence

A check result is evidence only for the exact source/artifact it actually tested. Stale, predecessor-head, synthetic-only, skipped-required, queued, cancelled, rate-limited, infrastructure-only or model-only evidence is not equivalent to exact-head success.

Release acceptance collects all applicable classes on one exact integrated protected head and then verifies the released artifact matches its source/provenance/SBOM. A green feature PR or a green documentation PR alone cannot authorize release.

## 17. References

International Organization for Standardization & International Electrotechnical Commission. (2023). *ISO/IEC 25010:2023 Systems and software engineering—Systems and software Quality Requirements and Evaluation (SQuaRE)—Product quality model*.

National Institute of Standards and Technology. (2022). *Secure Software Development Framework (SSDF) Version 1.1: Recommendations for mitigating the risk of software vulnerabilities* (NIST SP 800-218). https://doi.org/10.6028/NIST.SP.800-218

Open Worldwide Application Security Project. (2025). *Application Security Verification Standard 5.0.0*.

World Wide Web Consortium. (2024). *Web Content Accessibility Guidelines (WCAG) 2.2*.
