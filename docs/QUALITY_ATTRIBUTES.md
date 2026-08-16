# Quality Attribute Scenarios

- Status: Normative non-functional baseline
- Date: 2026-08-09
- Scope: hosted runtime, reference clients, persistence, integrations, research release, and operational profiles

This document converts broad quality goals into **stimulus → environment → response → measurable evidence** scenarios. Exact commercial SLO/RPO/RTO values remain deployment-profile evidence governed by ADR-0017; they are not invented here.

## 1. Scientific correctness and reproducibility

### QA-SCI-01 — Score replay

- **Stimulus:** Recompute a published result from the exact immutable response snapshot and pinned AssessmentSpec/scoring/calibration/norm/engine artifacts.
- **Environment:** Supported retained version bundle.
- **Response:** The scoring path reproduces the result within the documented numerical tolerance or returns a typed reproducibility failure.
- **Evidence:** golden replay tests, artifact digest verification, engine/version provenance.

### QA-SCI-02 — Scientific failure is fail-closed

- **Stimulus:** scoring returns non-identification, non-finite estimate, insufficient anchors, unsupported contract, or scoreability failure.
- **Response:** no substitute score/norm/type is invented; the result remains pending/failed with a typed scientific state.
- **Evidence:** fast-mlsirm contract tests + product adapter failure-injection tests.

### QA-SCI-03 — Historical result stability

- **Stimulus:** a scoring, norm, calibration, or narrative version changes.
- **Response:** historical result snapshots remain unchanged; deliberate rescoring creates a superseding snapshot.
- **Evidence:** immutable persistence constraints and supersession tests.

## 2. Reliability and consistency

### QA-REL-01 — Idempotent response replay

- **Stimulus:** the same `client_event_ref` with identical evidence is retried after network uncertainty.
- **Response:** the original accepted outcome/sequence is returned without creating a duplicate event.
- **Conflict:** reuse with different evidence fails closed.
- **Evidence:** concurrency/idempotency tests against real persistence.

### QA-REL-02 — Completion is crash-safe

- **Stimulus:** process fails immediately before/after session completion transaction/outbox commit.
- **Response:** either no completion is committed, or Completed + immutable response snapshot + scoring outbox are all durably present; no partial state.
- **Evidence:** transactional failure-injection/crash tests.

### QA-REL-03 — Duplicate message delivery

- **Stimulus:** broker retries a domain event.
- **Response:** inbox deduplication prevents duplicate externally visible side effects.
- **Evidence:** duplicate/reordered delivery tests.

## 3. Availability and graceful degradation

### QA-AVL-01 — AI unavailable

- **Stimulus:** contextual-orchestrator/provider is unavailable or denied by egress policy.
- **Response:** numeric score and basic result remain available with deterministic localized narrative fallback.
- **Evidence:** end-to-end outage test.

### QA-AVL-02 — Research catalog unavailable

- **Stimulus:** semantic-data-portal registration fails transiently.
- **Response:** personal assessment/results are unaffected; approved release registration remains durable/retryable without changing manifest identity.
- **Evidence:** integration failure/reconciliation test.

### QA-AVL-03 — Scoring unavailable

- **Stimulus:** fast-mlsirm scoring dependency becomes unavailable after completion.
- **Response:** completed response snapshot remains durable; scoring job waits/retries; no invented score.
- **Evidence:** worker/job state and recovery test. Active PR #168 adds `load_scoring_request` plus `tests/postgres_scoring_request_reload.rs` so a dispatched version pin survives process restart without inventing a score; live worker composition remains Target.

## 4. Security

### QA-SEC-01 — Cross-tenant object reference

- **Stimulus:** an authenticated user submits another tenant's valid opaque `result_ref`, `session_ref`, contribution, or data-rights ref.
- **Response:** server denies access without revealing sensitive existence/details.
- **Evidence:** negative integration/API tests.

### QA-SEC-02 — Anonymous token replay/abuse

- **Stimulus:** expired, wrong-audience, tampered, or replayed anonymous credential is submitted.
- **Response:** request fails closed; valid established resource history is not mutated.
- **Evidence:** credential validation tests.

### QA-SEC-03 — Provider egress attempt

- **Stimulus:** an AI/tool path attempts an unapproved host/authority or redirect.
- **Response:** EgressWeave/equivalent policy denies it; application cannot bypass through a direct network client.
- **Evidence:** integration/security contract tests.

### QA-SEC-04 — Supply-chain/release integrity

- **Stimulus:** release candidate contains unpinned/unknown artifact or mismatched provenance/SBOM digest.
- **Response:** release gate fails.
- **Evidence:** reproducible-build/provenance/SBOM verification.

## 5. Privacy and data rights

### QA-PRV-01 — Research identity leakage

- **Stimulus:** build a public release candidate containing Keyverse subject, operational participant ref, linkage ref/key, or a prohibited field.
- **Response:** release validation fails closed before registration/publication.
- **Evidence:** adversarial release fixtures and schema/policy tests.

### QA-PRV-02 — Research refusal

- **Stimulus:** participant refuses/withdraws research contribution while retaining valid service use.
- **Response:** personal result remains available; future research eligibility follows the scope/withdrawal policy.
- **Evidence:** product + release-pipeline tests.

### QA-PRV-03 — Restore after deletion

- **Stimulus:** restore a backup predating a completed deletion request.
- **Response:** deletion/retention reconciliation prevents silent user-visible resurrection of data before recovery is accepted.
- **Evidence:** backup/restore deletion-reconciliation drill.

## 6. Accessibility

### QA-ACC-01 — Full keyboard assessment

- **Stimulus:** participant uses only keyboard interaction on a supported reference client.
- **Response:** can select instrument, answer, navigate, review errors, complete, and inspect result without keyboard trap.
- **Evidence:** automated + manual WCAG 2.2 AA acceptance and assistive-technology testing.

### QA-ACC-02 — Non-visual result comprehension

- **Stimulus:** charts/visual profile cannot be perceived visually.
- **Response:** equivalent ordered text/table explanation communicates score, uncertainty, and limitations without relying on color alone.
- **Evidence:** screen-reader/manual acceptance.

## 7. Multilingual measurement integrity

### QA-I18N-01 — Exact locale resolution

- **Stimulus:** participant requests a locale for which no exact published assessment form exists.
- **Response:** assessment item flow does not silently fall back to another language; client obtains an explicit unavailable/choice response.
- **Evidence:** API/client locale tests.

### QA-I18N-02 — Cross-locale comparison gate

- **Stimulus:** product/report requests shared norm/cross-locale comparison before required linking/invariance evidence exists.
- **Response:** comparison is rejected or explicitly limited to the supported claim.
- **Evidence:** instrument publication policy tests.

## 8. Performance and resource safety

Performance goals are workload/profile-specific. The architecture nevertheless requires bounded behavior.

### QA-PERF-01 — Interactive request budget

- **Stimulus:** normal session/response/result metadata request under the profile's declared supported load.
- **Response:** meets that profile's versioned latency/resource SLO without invoking unnecessary psychometric/AI work on the request thread.
- **Evidence:** repeatable load test tied to deployment profile.

### QA-PERF-02 — Scoring burst/backpressure

- **Stimulus:** scoring requests arrive faster than workers can process them.
- **Response:** durable queue/backpressure preserves requests, exposes queue age, applies bounded concurrency, and avoids unbounded memory/thread growth.
- **Evidence:** load/failure test.

### QA-PERF-03 — Bounded external/model payloads

- **Stimulus:** input or provider output exceeds task/resource limits.
- **Response:** rejected before unbounded allocation/processing; safe error/metric emitted.
- **Evidence:** boundary/adversarial tests.

## 9. Scalability

### QA-SCL-01 — Stateless API scaling

- **Stimulus:** add runtime API instances.
- **Response:** session consistency and idempotency remain governed by durable store/locks/constraints rather than process-local memory.
- **Evidence:** multi-instance integration/concurrency test before hosted scale claims.

### QA-SCL-02 — Worker scaling

- **Stimulus:** add job workers.
- **Response:** leases/claims/idempotency prevent duplicate committed scoring/export/release side effects.
- **Evidence:** concurrent worker tests.

## 10. Interoperability and portability

### QA-INT-01 — Replace reference client

- **Stimulus:** g7 is absent and standalone/external client uses supported API.
- **Response:** core assessment/results/data rights work without g7 internals.
- **Evidence:** non-g7 end-to-end reference path.

### QA-INT-02 — Optional service absent

- **Stimulus:** Community profile runs without TEPP, semantic-data-portal, AI, g7.
- **Response:** startup/readiness marks optional capabilities unavailable while core flow remains usable.
- **Evidence:** Community profile installation/acceptance test.

### QA-INT-03 — Contract version change

- **Stimulus:** client or service uses unsupported required contract major version.
- **Response:** fail closed with explicit compatibility problem; unknown required semantics are not silently ignored.
- **Evidence:** OpenAPI/AsyncAPI/adapter compatibility tests when transports exist.

## 11. Maintainability and modularity

### QA-MNT-01 — Reverse dependency guard

- **Stimulus:** a change attempts to make fast-mlsirm depend on Psychometrics Commons product code or introduces direct external service DB access.
- **Response:** architecture/repository test/review gate fails.
- **Evidence:** dependency/credential fitness tests plus review.

### QA-MNT-02 — Architecture drift

- **Stimulus:** material lifecycle/data/trust/deployment change omits the affected ADR/view/traceability update.
- **Response:** architecture documentation fitness/review blocks merge.
- **Evidence:** repository tests + PR review checklist.

## 12. Observability and auditability

### QA-OBS-01 — Trace a participant operation safely

- **Stimulus:** operator investigates failed session completion/scoring.
- **Response:** correlation/resource/job refs and safe failure classes permit reconstruction of control flow without exposing raw response/credential/restricted linkage in routine logs.
- **Evidence:** synthetic incident/log fixture test.

### QA-OBS-02 — Prove release provenance

- **Stimulus:** buyer/researcher/operator audits one released result/dataset.
- **Response:** immutable manifests connect source version, scoring/calibration/norm/narrative or release metadata and artifact digests without participant identity leakage.
- **Evidence:** provenance replay/verification test.

## 13. Recoverability

### QA-REC-01 — Restore current release

- **Stimulus:** restore supported release data into a clean recovery environment.
- **Response:** meets the profile's measured RPO/RTO, verifies immutable digests, tenant/linkage boundaries, deduplication and deletion reconciliation before service acceptance.
- **Evidence:** real restore drill governed by ADR-0017.

## 14. Quality-attribute conflict policy

When qualities conflict, do not hide the trade-off in implementation.

Examples:

- latency versus deeper psychometric/AI computation → keep interactive command durable, move optional/heavy work async;
- privacy versus operational utility → purpose-bound access/separation, not destructive blanket masking;
- availability versus scientific correctness → queue/retry or return pending; never invent a score;
- feature convenience versus bounded-context independence → stable adapter/API instead of direct database coupling;
- richer cross-language comparison versus invariance uncertainty → limit the claim rather than overstate comparability.

A material unresolved trade-off requires an ADR.

## 15. Release use

The release process converts applicable scenarios into measured gates. A scenario that has no implemented surface yet remains target architecture and cannot be reported as a passing control. GA deployment-profile commitments are populated only when the actual topology/workload/recovery mechanisms exist and have current evidence.
