# CEFR Assessment Product v1 Design

- Status: Approved product design for an active Draft implementation stack
- Date: 2026-08-27
- Product owner: `ContextualWisdomLab/psychometrics-commons`
- Shared-contract authority: `ContextualWisdomLab/learning-interoperability-contracts`
- Numerical authority: `ContextualWisdomLab/fast-mlsirm`
- Model-orchestration authority: `ContextualWisdomLab/contextual-orchestrator`

## 1. Goal

Build the first end-to-end CWL product slice that can administer an English A1–B2 language assessment and publish an immutable, domain-level CEFR-aligned result profile without claiming Council of Europe certification or empirical CEFR linking that has not been evidenced.

The initial measured domains are:

- reading reception;
- listening reception;
- written production;
- spoken production.

The first release reports a profile by domain. It does not report one overall level unless an exact immutable assessment blueprint explicitly authorizes overall reporting, every blueprint-required domain is measured, the exact overall-reporting policy is used, and all scientific publication gates pass.

## 2. Product boundary

Psychometrics Commons owns:

- CEFR instrument-family publication and suspension;
- assessment session lifecycle;
- item/task delivery evidence references;
- response and media evidence references;
- human and model rater assignments;
- scoring-job dispatch and fencing;
- immutable scoring-input snapshots;
- immutable result snapshots and superseding corrections;
- participant-facing result exports;
- consent, authorization, audit, retention and data-right workflows;
- product health, operability and release evidence.

Psychometrics Commons does not own:

- CEFR framework or descriptor prose;
- language-specific Reference Level Description content;
- authored task, prompt, rubric, media or accessibility-release bytes;
- psychometric estimation, matrix/vector arithmetic, cut-score estimation, linking, DIF, uncertainty or recovery kernels;
- LLM provider transport or model routing;
- LMS placement, enrollment, completion or credential decisions;
- xAPI statement truth;
- longitudinal growth and contextual-effect estimation.

Those remain in their authoritative CWL bounded contexts.

## 3. Dependency contract

The product consumes a released immutable artifact of `cwl_cefr_language_assessment/v1`. The implementation and release manifest record all of the following:

```text
contract_repository
contract_release_reference
contract_commit_sha
contract_artifact_digest
assessment_blueprint_schema_id
assessment_blueprint_schema_digest
task_specification_schema_id
task_specification_schema_digest
result_snapshot_schema_id
result_snapshot_schema_digest
```

A mutable branch, `latest`, or an unverified network fetch cannot become an identity-bearing dependency. A Draft review may test an exact candidate commit, but production publication requires a released artifact and a digest-verified local copy or generated client.

The shared schema is not copied and reinterpreted in this repository. Product-domain types may project it into Rust structures only through generated or explicitly version-tested adapters.

## 4. Claim-state model

The product preserves four distinct states:

```text
experimental
→ cefr_aligned
→ cefr_linked
→ certification_decision
```

These are not cosmetic labels.

### `experimental`

Research-only assessment evidence. No operational CEFR interpretation claim is made.

### `cefr_aligned`

The immutable blueprint, tasks and rubrics reference the CEFR construct, exact descriptor identities, and an exact target-language profile/RLD authority and immutable revision or dated snapshot. This state makes no empirical examination-linking claim.

### `cefr_linked`

The exact instrument/scoring/cut-score combination additionally pins approved standard-setting and empirical classification/linking-validation evidence. The evidence is specific to the intended population, language, administration mode and use.

### `certification_decision`

The result is already CEFR-linked and additionally pins a legitimate external certification authority, policy and governed decision record. Psychometrics Commons does not create a certification authority merely by storing these references.

Narrative copy, operator labels and LLM output cannot advance claim state.

## 5. Initial assessment profile

The first profile is:

```text
target_language: en
level_range: A1–B2
assessment_purpose: placement and diagnostic feedback
decision_stakes: low to moderate
reporting_scope: domain profile only
```

Out of scope for v1:

- C1 and C2 operational reporting;
- spoken or written interaction as a scored domain;
- mediation as a scored domain;
- plurilingual/pluricultural reporting;
- certification, admission, licensure or employment screening;
- adaptive testing;
- cross-language score comparison;
- causal learning-growth claims.

These exclusions are enforced by the published blueprint and result-export limitations, not only by documentation.

## 6. Domain model

The product introduces the following Rust domain aggregates and immutable records.

### 6.1 `CefrInstrumentFamily`

Defines the product-owned instrument family and intended use. It references the shared framework/profile contract but stores no official descriptor prose.

Minimum identity:

```text
cefr_instrument_family_ref
product_tenant_ref
contract_release_ref
target_language
intended_use_code
claim_status_code
```

### 6.2 `CefrInstrumentRelease`

An immutable publication candidate or published release. It binds:

```text
instrument_release_ref
cefr_instrument_family_ref
assessment_blueprint_ref
assessment_blueprint_digest
task_release_manifest_ref
task_release_manifest_digest
rubric_release_manifest_ref
rubric_release_manifest_digest
scoring_profile_ref
scoring_engine_artifact_digest
cut_score_revision_ref
publication_evidence_bundle_ref
locale_code
published_at
```

Only a release with complete rights, language-profile, content, scientific and accessibility evidence may enter `published` state. Suspension prevents new sessions but does not mutate historical sessions or results.

### 6.3 `CefrAssessmentSession`

The session pins the exact release before it becomes active. State transitions are:

```text
created
→ active
→ paused
→ active
→ completed
→ sealed
```

Terminal alternatives are `cancelled`, `expired` and `invalidated`. A session cannot accept responses unless active. A sealed session cannot be reopened or modified.

### 6.4 `CefrEvidenceReference`

Stores only product-authorized metadata and opaque evidence identity:

```text
evidence_ref
evidence_kind
source_authority
source_record_ref
source_digest
source_version
observed_at
received_at
```

Evidence kinds include selected response, constructed response, original audio, transcript, acoustic-feature bundle, interaction-turn bundle, human rating and model rating. Raw response text, audio bytes, transcripts and model output remain in their authorized owning stores and are not duplicated into shared result envelopes.

### 6.5 `CefrRaterAssignment`

Binds a human or model rater to an exact response/task/rubric criterion set. It records rater family, rater/model version, assignment policy, blind-condition flags and adjudication role. A model rater is evidence, not score authority.

### 6.6 `CefrScoringInputSnapshot`

An immutable, content-addressed scoring input created only after the assessment session is sealed. It binds exact response/evidence references, task and rubric revisions, rater observations, missingness, accommodation and administration context. It contains no mutable alias.

### 6.7 `CefrScoringJob`

A durable asynchronous job with bounded retries, lease expiry and monotonically increasing fencing token. Stale workers cannot publish a result after a newer claim or cancellation.

### 6.8 `CefrResultSnapshot`

An immutable product-owned result conforming to the released shared schema. It carries bounded domain summaries:

- measurement status;
- reported level code;
- level probabilities;
- credible-level set;
- standard error;
- descriptor-coverage references;
- human-review state;
- limitations;
- exact blueprint, instrument, scoring and cut-score versions;
- standard-setting/linking/certification references only when authorized.

It does not carry raw scores, response-level calculations, item/person/rater parameter arrays, likelihood traces or provider payloads.

### 6.9 `CefrResultSupersession`

Corrections create a new immutable result linked to the exact predecessor and reason/evidence record. Historical results are never updated in place.

## 7. Persistence design

PostgreSQL 18 is the initial durable store. Authoritative product facts are normalized to third normal form. Database objects use two-or-more-word `snake_case` names.

Initial relations:

```text
cefr_instrument_family
cefr_instrument_release
cefr_session_record
cefr_evidence_reference
cefr_rater_assignment
cefr_scoring_input_snapshot
cefr_scoring_job
cefr_result_snapshot
cefr_result_domain
cefr_result_supersession
cefr_audit_event
```

Every tenant-scoped table contains `tenant_id`. Composite primary/foreign-key relationships include `tenant_id`. PostgreSQL row-level security is enabled and forced. The application role is `NOSUPERUSER NOBYPASSRLS` and cannot own migrations, disable RLS or create schema objects.

High-volume evidence and audit tables are indexed by tenant plus time and designed for future partitioning without changing semantic identity. Raw media and source payloads are not stored in these relations.

Every write command has an explicit idempotency key and request digest. Same-key/same-digest replay returns the original record. Same-key/different-digest replay fails closed. UPSERT behavior is explicit and tested per command; unrestricted `ON CONFLICT DO UPDATE` is prohibited for immutable records.

## 8. Scoring and numerical boundary

All result-affecting psychometric arithmetic is delegated to a version-compatible Rust artifact from `fast-mlsirm`. The adapter passes a validated scoring-input snapshot and receives a signed or digest-bound result artifact.

The numerical authority is responsible for:

- multidimensional IRT/MIRT or another approved measurement model;
- dichotomous and polytomous response processes;
- passage/audio testlet and local-dependence effects;
- task, criterion, rater, rater-family, occasion and scoring-engine facets;
- item/task/rater calibration;
- standard error and level probability computation;
- cut-score application and classification uncertainty;
- form linking and anchor stability;
- DIF/invariance and drift diagnostics;
- true-parameter and classification recovery.

Psychometrics Commons validates identity, provenance, lifecycle and schema compatibility. It does not recompute or repair numerical output. Failure to import or validate the exact Rust scoring artifact is a typed scientific failure; no score is invented.

CEFR level labels are ordinal. The product never converts them to equally spaced integers and averages them.

## 9. Human and LLM rater workflow

Writing and speaking require criterion-level observations from governed rater assignments.

The recommended initial design is:

```text
sealed response evidence
→ blind human rating
→ blind model-rater family A
→ blind model-rater family B
→ criterion observations
→ fast-mlsirm many-facet calibration
→ disagreement/uncertainty/OOD policy
→ human adjudication when required
→ immutable result snapshot
```

Every model call uses `contextual-orchestrator`; direct provider SDK fallback is prohibited. The product stores only bounded orchestration evidence references and exact model/prompt/rubric revisions, not raw chain-of-thought or provider payloads.

Human review is required when an explicit policy condition is met, including:

- critical criterion failure;
- missing or contradictory evidence;
- rater disagreement beyond the calibrated threshold;
- large classification uncertainty near a cut score;
- out-of-distribution evidence;
- unsupported language/accommodation context;
- model/rater version outside the approved calibration set;
- required adjudication for the intended use.

An unexplained confidence value is not a review policy.

## 10. Spoken-language evidence boundary

Spoken production cannot be reduced to transcript-only scoring. The scoring input may reference:

- immutable original-audio evidence;
- ASR transcript and confidence evidence;
- acoustic/phonological feature evidence;
- fluency and timing evidence;
- task-fulfilment and discourse evidence;
- interaction-turn evidence when a future interaction profile is enabled.

Each derived artifact records source audio identity, extractor/model version, digest and observation time. The product does not silently treat ASR text as the original response or a model-generated acoustic score as ground truth.

## 11. APIs

The first transport is a versioned HTTP API with OpenAPI generated and verified from the Rust transport implementation.

Minimum command/query surface:

```text
POST /v1/tenants/{tenant_id}/cefr/instrument-releases
POST /v1/tenants/{tenant_id}/cefr/sessions
POST /v1/tenants/{tenant_id}/cefr/sessions/{session_ref}/evidence-references
POST /v1/tenants/{tenant_id}/cefr/sessions/{session_ref}/seal
POST /v1/tenants/{tenant_id}/cefr/scoring-jobs
GET  /v1/tenants/{tenant_id}/cefr/scoring-jobs/{job_ref}
GET  /v1/tenants/{tenant_id}/cefr/results/{result_ref}
POST /v1/tenants/{tenant_id}/cefr/results/{result_ref}/exports
POST /v1/tenants/{tenant_id}/cefr/results/{result_ref}/supersessions
```

Transport DTOs are not reused as domain entities. Tenant, actor, authorization scope, correlation ID and idempotency key are validated before state-changing work.

## 12. Events and integrations

Durable outbox events include:

```text
cefr.instrument_release.published
cefr.session.created
cefr.session.sealed
cefr.scoring_job.queued
cefr.scoring_job.failed
cefr.result.published
cefr.result.superseded
cefr.human_review.requested
cefr.human_review.completed
```

Event envelopes use the released Learning Interoperability Contracts event schema. Consumers receive opaque references and bounded summaries. Cross-service database access is prohibited.

The LMS may consume a released CEFR result reference for placement. The LRS may record assessment activity through its released xAPI profile. TEPP may consume immutable longitudinal observations. None of these consumers may mutate or recalculate the result.

## 13. Security and privacy

Threat controls include:

- Keyverse-issued identity verification and tenant/task authorization;
- forced PostgreSQL RLS and composite tenant keys;
- purpose-bound access to raw responses and audio;
- encryption in transit and at rest;
- immutable audit events with correlation and causation IDs;
- content-addressed evidence and scoring artifacts;
- egress allowlists and secret resolution through approved registries;
- bounded body/media metadata size and rate/concurrency limits;
- no raw model output, response text or media in logs;
- participant export/deletion workflows that preserve legal-retention evidence without inventing anonymization claims;
- separation of operational participant identity from research-release identity.

PII is protected by authorization, encryption, purpose limitation, audit and retention controls rather than blanket masking that makes assessment operations unusable.

## 14. Failure behavior

The system fails closed when:

- a shared contract artifact or digest cannot be verified;
- a blueprint/task/result violates its exact schema;
- the target-language profile/RLD reference uses a mutable revision;
- a task is not authorized by the immutable blueprint;
- evidence is missing, duplicated, cross-session or cross-tenant;
- a session is not sealed before scoring;
- a scoring job uses a stale fence;
- `fast-mlsirm` is unavailable, incompatible or returns invalid provenance;
- an LLM route bypasses `contextual-orchestrator`;
- a result disagrees with its blueprint/instrument/scoring/cut-score versions;
- an overall result is unauthorized or required domains are incomplete;
- a linked/certification claim lacks exact evidence;
- result publication time precedes the evidence/scoring observation it claims to include.

No automatic repair, probability renormalization, level inference, schema downgrade or provider fallback can convert an invalid result into a publishable one.

## 15. Validation strategy

Development follows TDD with realistic and adversarial fixtures.

### Domain and state tests

- valid and invalid publication transitions;
- exact locale and release pinning;
- session pause/resume/seal and terminal-state rejection;
- idempotent replay and conflicting reuse;
- result supersession without mutation;
- stale-worker fencing;
- human-review state machine;
- cross-tenant and cross-session reference rejection.

### Scientific integration tests

- exact `fast-mlsirm` artifact compatibility;
- valid four-domain profile import;
- missing/inconclusive domain result;
- probability, credible-set and uncertainty validation;
- wrong blueprint/instrument/scoring/cut-score version;
- profile-only unauthorized overall result;
- linked/certification evidence gates;
- human/model rater provenance and adjudication;
- true-parameter/classification recovery evidence references.

### Database and operability tests

- PostgreSQL 18 migration, rollback and reapply;
- `NOSUPERUSER NOBYPASSRLS` application role;
- forced RLS and direct cross-tenant reads/writes;
- concurrent idempotency conflicts;
- backup/restore and forward migration;
- tenant skew and hot-partition evidence;
- outbox crash/retry and poison-message recovery;
- bounded asynchronous API and k6 load tests;
- metrics, trace, audit and alert evidence.

### Coverage gates

Production statement coverage, branch coverage, edge-case coverage and public API documentation coverage are each 100%. A zero denominator is reported explicitly and cannot satisfy a production gate. Rust numeric integration tests run against the compiled native artifact; Python fallback arithmetic is not accepted for production scoring.

## 16. Release and claim gates

A software release and an instrument publication are separate approvals.

A software release requires exact-head code review, all protected checks, SBOM/provenance, migrations, recovery and security evidence.

An instrument publication additionally requires exact rights, locale, accessibility, content, scoring-artifact, recovery, rater, DIF/fairness and intended-use evidence.

A `cefr_linked` publication additionally requires the approved standard-setting and empirical linking/classification-validation bundle for that exact instrument/scoring/cut-score combination.

No repository document, LLM output, green unit test or synthetic recovery run alone establishes Council of Europe endorsement, external certification, operational validity or a regulated-decision claim.

## 17. Standards and research basis

The implementation traces decisions to:

- Council of Europe. (2020). *Common European Framework of Reference for Languages: Learning, teaching, assessment—Companion volume*.
- Council of Europe. (2009). *Relating language examinations to the Common European Framework of Reference for Languages: A manual*.
- Association of Language Testers in Europe. (2026). *Manual for language test development and examining: For use with the CEFR*.
- American Educational Research Association, American Psychological Association, & National Council on Measurement in Education. (2014). *Standards for educational and psychological testing*.
- the exact target-language RLD/profile authority and immutable revision or dated source snapshot referenced by each blueprint.

These sources govern framework interpretation, examination development, linking evidence, validity, reliability/precision, fairness and reporting. They do not transfer ownership of official descriptor prose into this repository.

## 18. Delivery sequence

1. Add versioned shared-contract dependency verification and Rust product-domain types.
2. Add PostgreSQL persistence, forced RLS, idempotency and instrument/session state.
3. Add evidence-reference and sealed scoring-input snapshots.
4. Add durable scoring-job fencing and exact `fast-mlsirm` adapter.
5. Add immutable result snapshots and exports.
6. Add human/model rater assignment and adjudication lifecycle.
7. Add durable events and LMS/LRS/TEPP handoff contract tests.
8. Add recovery, security, load, UX/accessibility and release evidence.

Each slice is independently reviewable and keeps unsupported claims explicit.