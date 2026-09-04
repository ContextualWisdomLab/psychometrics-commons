# CEFR Assessment Product v1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a tenant-safe Psychometrics Commons vertical that publishes an immutable English A1–B2 CEFR-aligned domain profile through the released CWL interoperability contract and an exact Rust scoring artifact.

**Architecture:** Psychometrics Commons owns product lifecycle, persistence, authorization, evidence references, asynchronous scoring dispatch and immutable result snapshots. Shared schemas come from `learning-interoperability-contracts`, result-affecting arithmetic comes from `fast-mlsirm`, and model-rater calls come only through `contextual-orchestrator`.

**Tech Stack:** Rust, PostgreSQL 18, JSON Schema Draft 2020-12 consumer validation, existing Psychometrics Commons domain/persistence patterns, GitHub Actions, k6 for later load evidence.

**Spec:** `docs/superpowers/specs/2026-08-27-cefr-assessment-product-v1-design.md`

## Global Constraints

- Initial target is English A1–B2 with reading reception, listening reception, written production and spoken production.
- Initial reporting scope is domain profile only; overall reporting fails closed unless the immutable blueprint authorizes it.
- Mutable contract, blueprint, task, rubric, scoring, cut-score and RLD/profile aliases are prohibited.
- Official CEFR descriptor prose and translations are never copied into repository or product result envelopes.
- Raw task content, response text, transcripts, audio bytes, provider payloads, model output and PII remain outside the shared result envelope.
- Every result-affecting psychometric, vector, matrix, cut-score, uncertainty, linking, DIF and recovery calculation is Rust-owned in `fast-mlsirm`.
- LLM rating calls route only through `contextual-orchestrator`; LLM output is fallible rater evidence and never final score authority.
- PostgreSQL objects use two-or-more-word `snake_case`, authoritative facts are in third normal form, and tenant-scoped relations use composite tenant keys plus forced RLS.
- The application role is `NOSUPERUSER NOBYPASSRLS` and does not own migrations.
- Production statement, branch, edge-case and public-doc coverage are each 100%; a zero denominator cannot pass.
- `cefr_aligned`, `cefr_linked` and `certification_decision` remain distinct governed states.
- No step claims Council of Europe endorsement, external certification or regulated-decision readiness.

---

### Task 1: Verify and expose the released CEFR contract dependency

**Files:**
- Create: `src/cefr_contract.rs`
- Modify: `src/lib.rs`
- Create: `tests/cefr_contract.rs`
- Create: `contracts/cefr/README.md`
- Create: `contracts/cefr/release-manifest.json`
- Modify: `Cargo.toml`

**Interfaces:**
- Produces `CefrContractManifest`, `CefrContractError`, `verify_cefr_contract_manifest`, and schema identifiers consumed by every later task.
- Consumes a local digest-verified release manifest; no runtime network fetch.

- [ ] **Step 1: Write failing manifest-validation tests**

```rust
#[test]
fn rejects_mutable_contract_reference() {
    let manifest = fixture_manifest_with_release("latest");
    assert_eq!(
        verify_cefr_contract_manifest(&manifest),
        Err(CefrContractError::MutableReleaseReference)
    );
}

#[test]
fn rejects_schema_digest_mismatch() {
    let mut manifest = valid_manifest();
    manifest.result_snapshot_schema_digest = "sha256:00".into();
    assert_eq!(
        verify_cefr_contract_manifest(&manifest),
        Err(CefrContractError::InvalidDigest)
    );
}
```

- [ ] **Step 2: Run the focused test and confirm RED**

Run:

```bash
cargo test --test cefr_contract --locked
```

Expected: compile failure because the CEFR contract module and types do not exist.

- [ ] **Step 3: Implement closed Rust types and verification**

```rust
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CefrContractManifest {
    pub contract_release_reference: String,
    pub contract_commit_sha: String,
    pub contract_artifact_digest: String,
    pub assessment_blueprint_schema_id: String,
    pub assessment_blueprint_schema_digest: String,
    pub task_specification_schema_id: String,
    pub task_specification_schema_digest: String,
    pub result_snapshot_schema_id: String,
    pub result_snapshot_schema_digest: String,
}

pub fn verify_cefr_contract_manifest(
    manifest: &CefrContractManifest,
) -> Result<(), CefrContractError>;
```

Reject empty/control-bearing values, mutable aliases, non-canonical lowercase SHA-256 digests, non-40/64-hex commit identities, unexpected schema IDs and duplicate schema digests. Do not download schemas at runtime.

- [ ] **Step 4: Add the exact reviewed manifest and consumer documentation**

`contracts/cefr/release-manifest.json` records the released artifact, exact commit and schema digests. A Draft stack may use a candidate artifact only when its status is explicitly `review_candidate`; instrument publication must reject that status.

- [ ] **Step 5: Run focused and workspace verification**

```bash
cargo fmt --all -- --check
cargo test --test cefr_contract --locked
cargo test --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps --locked
```

Expected: all pass.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml src/lib.rs src/cefr_contract.rs tests/cefr_contract.rs contracts/cefr
git commit -m "feat: verify CEFR contract release"
```

### Task 2: Add CEFR instrument and session lifecycle primitives

**Files:**
- Create: `src/cefr_instrument.rs`
- Create: `src/cefr_session.rs`
- Modify: `src/lib.rs`
- Create: `tests/cefr_instrument.rs`
- Create: `tests/cefr_session.rs`

**Interfaces:**
- Consumes `CefrContractManifest` from Task 1.
- Produces `CefrInstrumentRelease`, `CefrPublicationState`, `CefrAssessmentSession`, `CefrSessionState`, and exact transition methods.

- [ ] **Step 1: Write publication-gate tests**

```rust
#[test]
fn aligned_release_requires_exact_language_profile_and_rights_evidence() {
    let release = release_without_rights_evidence();
    assert_eq!(
        release.publish(valid_publication_command()),
        Err(CefrInstrumentError::MissingRightsEvidence)
    );
}

#[test]
fn linked_release_requires_standard_setting_and_empirical_validation() {
    let release = linked_release_without_linking_evidence();
    assert_eq!(
        release.publish(valid_publication_command()),
        Err(CefrInstrumentError::MissingLinkingEvidence)
    );
}
```

- [ ] **Step 2: Write session-state tests**

```rust
#[test]
fn sealed_session_rejects_new_evidence() {
    let session = completed_session().seal().unwrap();
    assert_eq!(
        session.record_evidence(valid_evidence_reference()),
        Err(CefrSessionError::SessionSealed)
    );
}
```

Cover create/start/pause/resume/complete/seal/cancel/expire/invalidate, exact release/locale pinning, terminal-state rejection and deterministic event identity.

- [ ] **Step 3: Run focused tests and confirm RED**

```bash
cargo test --test cefr_instrument --test cefr_session --locked
```

Expected: compile failure for missing types.

- [ ] **Step 4: Implement minimal immutable domain types**

Use private fields plus constructors and transition methods. Exact references are nonempty, trimmed, control-free and nonnumeric. State transitions consume `self` or return a new immutable record so historical states cannot be mutated accidentally.

- [ ] **Step 5: Run focused, workspace, Clippy and rustdoc gates**

```bash
cargo fmt --all -- --check
cargo test --test cefr_instrument --test cefr_session --locked
cargo test --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps --locked
```

- [ ] **Step 6: Commit**

```bash
git add src/lib.rs src/cefr_instrument.rs src/cefr_session.rs tests/cefr_instrument.rs tests/cefr_session.rs
git commit -m "feat: add CEFR instrument and session lifecycle"
```

### Task 3: Add normalized PostgreSQL persistence and forced tenant isolation

**Files:**
- Create: `migrations/0013_cefr_assessment_product.sql`
- Create: `migrations/0013_cefr_assessment_product.rollback.sql`
- Create: `src/postgres_cefr.rs`
- Modify: `src/lib.rs`
- Create: `tests/postgres_cefr.rs`
- Modify: `.github/workflows/quality.yml`

**Interfaces:**
- Consumes Task 2 domain identities and states.
- Produces tenant-safe repositories for instrument releases, sessions, evidence references, scoring snapshots/jobs and results.

- [ ] **Step 1: Write migration catalog tests**

Assert exactly these relations exist with forced RLS:

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

Assert the application role owns none of them, cannot disable RLS, and cannot create objects in the application schema.

- [ ] **Step 2: Write tenant and idempotency tests**

Cover:

```text
same tenant + same idempotency key + same digest -> original row
same tenant + same key + different digest -> conflict
other tenant + known object id -> zero rows / rejected write
cross-tenant composite FK -> rejected
immutable result update/delete -> rejected
```

- [ ] **Step 3: Run the PostgreSQL test and confirm RED**

```bash
cargo test --test postgres_cefr --locked
```

Expected: missing migration/repository failure.

- [ ] **Step 4: Implement 3NF schema, forced RLS and append-only triggers**

Use composite tenant keys for every tenant-scoped relationship. Add explicit unique indexes for command identity. Avoid unrestricted UPSERT on immutable tables. Add tenant/time indexes for evidence, results and audit records.

- [ ] **Step 5: Implement repository methods with explicit transactions**

```rust
pub trait CefrRepository {
    fn publish_instrument_release(
        &self,
        command: &PublishCefrInstrumentRelease,
    ) -> Result<CefrInstrumentRelease, CefrPersistenceError>;

    fn create_session(
        &self,
        command: &CreateCefrSession,
    ) -> Result<CefrAssessmentSession, CefrPersistenceError>;

    fn append_evidence_reference(
        &self,
        command: &AppendCefrEvidenceReference,
    ) -> Result<CefrEvidenceReference, CefrPersistenceError>;
}
```

Repository methods set tenant context transaction-locally and map uniqueness/RLS/check violations to stable product errors without leaking SQL details.

- [ ] **Step 6: Add real migration, rollback and reapply workflow evidence**

The workflow provisions separate migrator and application roles, applies the migration, runs the exact tests, rolls back to zero CEFR relations, reapplies and verifies the same policies/indexes.

- [ ] **Step 7: Verify**

```bash
cargo fmt --all -- --check
cargo test --test postgres_cefr --locked
cargo test --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps --locked
```

- [ ] **Step 8: Commit**

```bash
git add migrations src/lib.rs src/postgres_cefr.rs tests/postgres_cefr.rs .github/workflows/quality.yml
git commit -m "feat: persist tenant-safe CEFR assessment state"
```

### Task 4: Seal scoring inputs and add fenced asynchronous scoring jobs

**Files:**
- Create: `src/cefr_scoring_input.rs`
- Create: `src/cefr_scoring_job.rs`
- Create: `src/postgres_cefr_scoring_job.rs`
- Modify: `src/lib.rs`
- Create: `tests/cefr_scoring_input.rs`
- Create: `tests/postgres_cefr_scoring_job.rs`

**Interfaces:**
- Consumes sealed sessions and evidence references from Tasks 2–3.
- Produces content-addressed `CefrScoringInputSnapshot` and durable fenced `CefrScoringJob`.

- [ ] **Step 1: Write scoring-input invariants**

Reject an unsealed session, duplicate evidence identity, cross-session evidence, mutable task/rubric/scoring references, missing required domain, unapproved rater version, and noncanonical digest.

- [ ] **Step 2: Write fenced-job tests**

```rust
#[test]
fn stale_worker_cannot_complete_newer_claim() {
    let first = claimed_job(1);
    let reclaimed = first.expire_and_reclaim("worker-b", 2).unwrap();
    assert_eq!(
        reclaimed.complete("worker-a", 1, valid_result_artifact()),
        Err(CefrScoringJobError::StaleFence)
    );
}
```

Cover enqueue, claim-next, lease expiry, retry, quarantine, cancellation, terminal failure and crash recovery.

- [ ] **Step 3: Confirm RED**

```bash
cargo test --test cefr_scoring_input --test postgres_cefr_scoring_job --locked
```

- [ ] **Step 4: Implement immutable snapshot identity**

Canonical identity includes tenant, session, exact evidence/task/rubric/rater references, missingness and administration/accommodation context. Hash deterministic canonical bytes; do not hash unordered database iteration.

- [ ] **Step 5: Implement transactional job lifecycle**

Use monotonic fencing tokens and `SELECT ... FOR UPDATE SKIP LOCKED` for bounded claim-next. Inbox receipt is not job completion. Completion persists the result artifact reference and outbox event in one transaction.

- [ ] **Step 6: Verify and commit**

```bash
cargo fmt --all -- --check
cargo test --test cefr_scoring_input --test postgres_cefr_scoring_job --locked
cargo test --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps --locked
git add src tests
git commit -m "feat: add fenced CEFR scoring jobs"
```

### Task 5: Add exact fast-mlsirm scoring adapter and immutable results

**Files:**
- Create: `src/cefr_scoring_adapter.rs`
- Create: `src/cefr_result.rs`
- Create: `src/postgres_cefr_result.rs`
- Modify: `src/lib.rs`
- Create: `tests/cefr_scoring_adapter.rs`
- Create: `tests/postgres_cefr_result.rs`

**Interfaces:**
- Consumes `CefrScoringInputSnapshot`, exact contract manifest and exact `fast-mlsirm` artifact identity.
- Produces validated immutable `CefrResultSnapshot`, domain results and supersession records.

- [ ] **Step 1: Write adapter failure tests**

Reject missing native artifact, capability-version mismatch, wrong scoring artifact digest, source-version mismatch, invalid schema, probability mass not equal to one, reported level outside probability/credible sets, invented score on an unmeasured domain, unauthorized overall result and publication-before-observation.

- [ ] **Step 2: Write valid four-domain profile test**

The test fixture returns reading/listening/written-production/spoken-production evidence with level probabilities, credible sets, standard errors, descriptor-coverage references and `overall_result.reporting_status_code = not_reported`.

- [ ] **Step 3: Confirm RED**

```bash
cargo test --test cefr_scoring_adapter --test postgres_cefr_result --locked
```

- [ ] **Step 4: Implement fail-closed adapter**

```rust
pub trait CefrScoringEngine {
    fn score(
        &self,
        input: &CefrScoringInputSnapshot,
    ) -> Result<CefrScoringArtifact, CefrScoringAdapterError>;
}
```

The production implementation verifies the exact `fast-mlsirm` capability/artifact contract. It never performs fallback arithmetic in Psychometrics Commons and never repairs invalid output.

- [ ] **Step 5: Implement immutable persistence and supersession**

Persist result header and domain rows transactionally with audit/outbox records. Corrections create a new snapshot and `cefr_result_supersession`; update/delete triggers protect historical results.

- [ ] **Step 6: Verify and commit**

```bash
cargo fmt --all -- --check
cargo test --test cefr_scoring_adapter --test postgres_cefr_result --locked
cargo test --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps --locked
git add src tests
git commit -m "feat: publish immutable CEFR result profiles"
```

### Task 6: Add human/model rater assignments and adjudication

**Files:**
- Create: `src/cefr_rater.rs`
- Create: `src/cefr_human_review.rs`
- Create: `src/contextual_orchestrator_cefr.rs`
- Modify: `src/lib.rs`
- Create: `tests/cefr_rater.rs`
- Create: `tests/cefr_human_review.rs`
- Create: `tests/contextual_orchestrator_cefr.rs`

**Interfaces:**
- Consumes evidence/task/rubric references and approved model/rater calibration identities.
- Produces criterion observations, bounded orchestration evidence and human-review decisions used by the scoring snapshot.

- [ ] **Step 1: Write rater-assignment and observation tests**

Cover blind assignment, duplicate criterion, unapproved model version, rubric/category-anchor mismatch, missing evidence reference, range restriction, monotonic threshold failure and cross-session replay.

- [ ] **Step 2: Write orchestrator boundary tests**

A production adapter without `contextual_orchestrator_contract == "contextual-orchestrator-contract-v1"` fails at construction. Provider timeout, malformed structured output, unsupported parameter, partial boundary result and exhausted route remain explicit failures in the denominator.

- [ ] **Step 3: Write human-review policy tests**

Cover critical criterion, rater disagreement, high classification uncertainty, OOD evidence, unsupported accommodation/language context, unapproved rater family and mandatory adjudication.

- [ ] **Step 4: Confirm RED and implement minimal contracts**

```bash
cargo test --test cefr_rater --test cefr_human_review --test contextual_orchestrator_cefr --locked
```

Implement source-text-free records and exact provenance. Do not store chain-of-thought, raw provider output, response text, transcript or audio.

- [ ] **Step 5: Verify and commit**

```bash
cargo fmt --all -- --check
cargo test --test cefr_rater --test cefr_human_review --test contextual_orchestrator_cefr --locked
cargo test --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps --locked
git add src tests
git commit -m "feat: govern CEFR rater and review evidence"
```

### Task 7: Add versioned HTTP, audit, export and integration events

**Files:**
- Create: `src/bin/cefr_api.rs`
- Create: `src/cefr_api.rs`
- Create: `src/cefr_export.rs`
- Create: `openapi/cefr-v1.yaml`
- Create: `contracts/events/cefr-events-v1.json`
- Create: `tests/cefr_api.rs`
- Create: `tests/cefr_export.rs`
- Modify: `.github/workflows/quality.yml`

**Interfaces:**
- Consumes all prior product-domain and persistence services.
- Produces authenticated tenant-scoped commands/queries, participant exports and durable event envelopes for LMS/LRS/TEPP consumers.

- [ ] **Step 1: Write transport tests before routes**

Cover missing/invalid tenant authorization, malformed UUID/reference, body limit, correlation ID, idempotency key, duplicate replay, cross-tenant read/write, typed scientific failure, async `202` scoring response and bounded polling.

- [ ] **Step 2: Write export tests**

Machine and human-readable exports contain the same domain levels, probabilities, uncertainty, versions, limitations and owner identity. They contain no raw response/audio/task/provider/scoring internals and no invented overall level.

- [ ] **Step 3: Write event-envelope tests**

Validate each event against the released shared event schema. Outbox payloads include opaque references, correlation/causation and bounded status only.

- [ ] **Step 4: Confirm RED and implement routes**

Use existing HTTP/auth patterns. Transport DTOs convert into domain commands; database transactions do not span external scoring/model calls.

- [ ] **Step 5: Generate and diff-check OpenAPI**

The committed OpenAPI is generated or deterministically checked against route DTOs. An undocumented route or field fails CI.

- [ ] **Step 6: Verify and commit**

```bash
cargo fmt --all -- --check
cargo test --test cefr_api --test cefr_export --locked
cargo test --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps --locked
git diff --check
git add src openapi contracts tests .github/workflows/quality.yml
git commit -m "feat: expose CEFR assessment product API"
```

### Task 8: Complete operational, scientific and documentation gates

**Files:**
- Modify: `docs/PRD.md`
- Modify: `docs/TRD.md`
- Modify: `ARCHITECTURE.md`
- Modify: `docs/MEASUREMENT_GOVERNANCE.md`
- Modify: `docs/AI_GOVERNANCE.md`
- Modify: `docs/THREAT_MODEL.md`
- Modify: `docs/OPERABILITY.md`
- Modify: `docs/TEST_STRATEGY.md`
- Modify: `docs/TRACEABILITY.md`
- Modify: `docs/ROADMAP.md`
- Modify: `docs/RISK_REGISTER.md`
- Modify: `CHANGELOG.md`
- Modify or create: `docs/product-technical-gap-baseline.md`
- Create: `docs/adr/cefr-assessment-product-v1.md`
- Create: `docs/doctoring/CEFR_ASSESSMENT_PRODUCT.md`
- Create: `scripts/postgres_cefr_recovery_rehearsal.sh`
- Create: `load/k6_cefr_assessment.js`

**Interfaces:**
- Consumes all implementation and test evidence.
- Produces release, recovery, security, load, scientific and buyer-review traceability for the exact PR head.

- [ ] **Step 1: Add recovery rehearsal**

Create anonymized realistic tenant-skew data, dump/restore it into a disposable PostgreSQL 18 database, verify all RLS policies/forced relations, immutable result digests, audit/outbox rows and a forward migration.

- [ ] **Step 2: Add asynchronous k6 journey**

Exercise session creation, evidence-reference append, sealing, scoring enqueue/poll and result retrieval with measured latency/error/concurrency. Do not use synthetic benchmark results as production scores.

- [ ] **Step 3: Complete coverage evidence**

Run statement and branch coverage with nonzero denominators and fail under 100%. Add explicit edge-case inventory and public API/rustdoc coverage checks.

- [ ] **Step 4: Update all normative documents**

Map each requirement and ADR to exact code, migration, test and CI evidence. Preserve explicit Target/Active PR/Implemented statuses. Add APA 7th references and no-certification/non-linking limitations.

- [ ] **Step 5: Run the exact-head release gate**

```bash
cargo fmt --all -- --check
cargo test --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps --locked
cargo llvm-cov --workspace --all-features --branch --fail-under-lines 100 --fail-under-branches 100
shellcheck scripts/postgres_cefr_recovery_rehearsal.sh
scripts/postgres_cefr_recovery_rehearsal.sh
k6 run load/k6_cefr_assessment.js
git diff --check
```

Expected: every command passes against the exact PR head. A missing tool or unavailable real dependency is reported as a release blocker rather than silently skipped.

- [ ] **Step 6: Commit**

```bash
git add docs ARCHITECTURE.md CHANGELOG.md scripts load
git commit -m "docs: complete CEFR product evidence baseline"
```

### Task 9: Open and govern the Draft implementation stack

**Files:**
- No file changes unless exact-head review finds a valid defect.

**Interfaces:**
- Consumes the verified exact head from Tasks 1–8.
- Produces a dependency-ordered PR stack with explicit merge gates and consumer handoff.

- [ ] **Step 1: Verify head/base and unrelated-diff absence**

Compare the exact head against protected `main`; verify no unrelated product vertical is mixed into the stack.

- [ ] **Step 2: Open or update the Draft PR**

The PR body records exact head, released contract digest, exact `fast-mlsirm` and `contextual-orchestrator` compatibility identities, RED→GREEN evidence, current limitations and dependency stack.

- [ ] **Step 3: Resolve all valid review findings**

Verify each finding against current code, fix only valid defects, rerun exact-head checks and resolve threads with evidence. Do not suppress warnings or bypass protection.

- [ ] **Step 4: Promote only when dependencies and gates are complete**

Mark Ready only after the released contract dependency exists, all required Checks pass, no blocking thread remains, and the vertical has non-author semantic review. Merge through the ordinary protected path only.
