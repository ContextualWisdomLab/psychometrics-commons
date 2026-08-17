# Psychometrics Commons Risk Register

- Status: Architecture/product risk baseline
- Date: 2026-08-09
- Rule: risk status must be tied to current evidence; architecture intent alone does not close a risk

This register tracks material product, scientific, privacy, security, operational, integration, and commercial risks that can invalidate the intended user journey or due-diligence posture. It is not a substitute for GitHub security findings or project issues; concrete defects should still receive their own issue/PR and exact-head evidence.

## Rating vocabulary

- **impact:** critical / high / medium / low
- **likelihood:** high / medium / low / unknown
- **state:** open / mitigated_by_architecture / implementation_in_progress / evidence_required / accepted / closed

`mitigated_by_architecture` means the design has a control but production evidence does not yet prove it.

## Current material risks

| Risk | Impact | Likelihood | State | Primary control / next evidence |
|---|---|---|---|---|
| Psychometrics Commons duplicates fast-mlsirm scientific kernels and creates divergent scores | critical | low | mitigated_by_architecture | ADR-0001/0004, dependency direction, scoring adapter contract; add dependency fitness tests |
| Hosted runtime returns fallback/invented score during fast-mlsirm outage/scientific failure | critical | medium | mitigated_by_architecture | typed fail-closed scoring contract, durable pending job; end-to-end failure injection required |
| Instrument content changes without version change and historical result becomes unreproducible | critical | low | implementation_in_progress | immutable publication/version contract, content digest, result provenance; persistence constraints pending |
| Session/response race creates duplicate or inconsistent scoring evidence | high | medium | implementation_in_progress | idempotent response ledger + immutable snapshot; real DB concurrency/atomic outbox tests pending |
| Cross-tenant object reference exposes another user's session/result/research/data-rights state | critical | medium | evidence_required | tenant/resource authorization architecture; transport/persistence negative tests pending |
| Anonymous-to-account link permits account takeover/history theft | critical | medium | evidence_required | dual proof-of-control + Keyverse validation; adapter and adversarial tests pending |
| Keyverse identity role is confused with product/research authorization | high | medium | mitigated_by_architecture | separate domain authorization and separation-of-duties policy; integration tests pending |
| Research release contains operational/Keyverse/linkage identifier | critical | medium | implementation_in_progress | restricted linkage persistence Active PR #187 plus program-scoped `public_research_identity` load; adversarial release pipeline pending |
| High-dimensional/longitudinal release is re-identifiable despite removed direct identifiers | critical | medium | mitigated_by_architecture | privacy-risk review, access class, rare-combination/joinability checks; operational process/evidence pending |
| Research consent is bundled with service use or future release exceeds consent scope | critical | low | implementation_in_progress | purpose-specific consent/research contribution domain contract; UI/API/snapshot enforcement pending |
| Restore resurrects data previously deleted by valid data-rights request | high | medium | mitigated_by_architecture | ADR-0017 deletion reconciliation; real backup/restore drill pending |
| AI provider receives disallowed sensitive/PII data or wrong residency/retention class | critical | medium | mitigated_by_architecture | task data/provider policy + EgressWeave; deployed routing/policy tests pending |
| AI narrative changes scientific score or makes diagnostic/immutable-person claims | high | medium | mitigated_by_architecture | ScoreProfile/rule binding + deterministic fallback + output validation; client/adversarial tests pending |
| LLM judge is treated as ground truth and evaluator severity/bias/drift contaminates validation | high | medium | mitigated_by_architecture | measurement/AI governance; fast-mlsirm rater calibration/evaluation evidence required |
| Big Five style presentation becomes an unofficial MBTI clone/equivalence claim | medium | medium | mitigated_by_architecture | original versioned narrative mapping; review client copy and names before release |
| Self-compassion/other reflection scale is published without adequate rights or locale validation | high | medium | open | rights/translation/validation release gate; do not publish until evidence exists |
| Korean/English scores are directly compared without invariance/linking evidence | high | medium | mitigated_by_architecture | locale-specific versions + DIF/invariance/linking gate; study evidence pending |
| General/facet/subscale score is exposed because model fit is good although scoreability is poor | high | medium | mitigated_by_architecture | measurement governance (ECV/omega/H/determinacy etc. where applicable); publication evidence pending |
| Multilevel/multiple-membership/time structure is collapsed and produces atomistic interpretation | high | medium | mitigated_by_architecture | fast-mlsirm/TEPP boundary + measurement governance; dataset-specific modeling evidence required |
| Shared-question/item local dependence is misread as substantive trait | high | medium | mitigated_by_architecture | testlet/local-dependence and model-comparison policy; recovery/fit evidence required |
| Bifactor/latent-space/complex model selected only by in-sample fit | high | medium | mitigated_by_architecture | relation-safe LR/Vuong, held-out prediction, residuals, recovery, scoreability; upstream evidence required |
| Factor rotation is marketed as globally optimal/universally best | medium | medium | mitigated_by_architecture | best-observed multi-start + stability/recovery policy; no universal criterion claim |
| Published research/score artifacts mutate in place after correction | high | low | mitigated_by_architecture | content addressing + supersession; physical DB/object-store constraints pending |
| Cross-service direct database access creates hidden coupling/privacy blast radius | high | medium | mitigated_by_architecture | ADR-0001/0015; credential/dependency fitness tests pending |
| Outbox/inbox replay duplicates external release/deletion/scoring side effects | high | medium | evidence_required | transactional outbox/inbox design; persistence/concurrency/recovery implementation pending |
| Optional dependency outage is reported as total product outage or blocks personal results | medium | medium | mitigated_by_architecture | capability-scoped readiness/degradation; deployment failure tests pending |
| Community profile silently depends on g7/AI/TEPP/portal | high | low | mitigated_by_architecture | ADR-0002/0011 + deployment profile contract; install/end-to-end proof pending |
| OpenAPI/AsyncAPI target docs are published before implementation and mislead integrators | medium | medium | mitigated_by_architecture | ADR-0014 as-built-only contract rule; CI gate pending |
| Logical ERD drifts from physical migrations | high | medium | evidence_required | ADR-0015 + planned schema fitness test when migrations exist |
| Operational SLO/RPO/RTO is promised without measurements | high | medium | mitigated_by_architecture | ADR-0017 explicitly prohibits invented universal values |
| Backup job succeeds but restore/provenance/tenant isolation fails | critical | medium | evidence_required | real restore drill and QA-REC scenarios pending |
| Routine logs expose raw responses, tokens or restricted linkage | critical | medium | mitigated_by_architecture | safe refs/digests and redaction rules; log/adversarial tests pending |
| Security/compliance readiness language is mistaken for SOC 2/CSAP certification | high | medium | mitigated_by_architecture | explicit evidence-state model and non-claim; external assessment required |
| Documentation becomes target-only and is mistaken for implemented product maturity | high | medium | implementation_in_progress | named-baseline TRACEABILITY + docs fitness tests; update after every protected-main feature merge |
| ADR/diagram drift causes two teams to implement incompatible contracts | high | medium | implementation_in_progress | ADR-0016, architecture view set, traceability test; semantic review still required |
| 100% coverage is gamed with trivial tests/exclusions rather than realistic behavior | high | medium | mitigated_by_architecture | AGENTS quality contract + realistic state/concurrency/security/failure tests; review/CI enforcement required |
| Public launch uses a psychometrically weak IPIP form/norm/translation for convenience | critical | medium | open | exact instrument rights/content/translation/calibration/norm/intended-use release bundle required |
| Buyer-visible product remains a collection of domain contracts without usable Public Assessment/Result Explorer | high | high | open | Roadmap Phase 2–6; deliver executable end-to-end reference client/API before product-readiness claim |
| Measurement Workbench becomes a second measurement source of truth instead of consuming fast-mlsirm | high | medium | mitigated_by_architecture | Workbench authoring/publishing downstream only; adapter/contract tests pending |
| Enterprise features create a monolithic central control plane that breaks standalone Community deployment | high | medium | mitigated_by_architecture | identical domain contracts across deployment profiles; Community acceptance required |
| No real customer/participant outcome or research reuse evidence supports acquisition-scale product value | high | high | open | post-product external adoption, ROI, validation/reuse metrics; architecture cannot close this risk |

## Risk-treatment rules

1. A risk marked `mitigated_by_architecture` is not release evidence.
2. Critical/high risks require a named test, issue, release gate, or explicitly accepted rationale before GA.
3. A security/privacy/scientific risk cannot be offset by unrelated feature completeness or aggregate quality score.
4. An unavailable optional dependency may be mitigated through degraded mode only if the degraded behavior is explicitly safe and tested.
5. A risk discovered in another owned bounded context is handed to that repository/owner; Psychometrics Commons does not duplicate its implementation to hide the dependency defect.
6. If an architecture assumption proves false, supersede the relevant ADR and update risk/traceability rather than stacking local exceptions.
7. Product/valuation claims require external evidence; internal architecture maturity is not converted into a monetary valuation assertion.

## Review cadence

Review this register when:

- a major product phase is completed;
- a material ADR is accepted/superseded;
- a new external provider/bounded context is enabled;
- a new data class or research release type is introduced;
- an incident or high-severity finding occurs;
- a deployment profile moves toward GA;
- an instrument/locale is proposed for consumer publication;
- before a major release or due-diligence package is prepared.

Closed/accepted risks retain historical evidence; do not delete the rationale merely to make the active register look smaller.
