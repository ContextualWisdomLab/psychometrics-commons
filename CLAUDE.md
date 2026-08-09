# CLAUDE.md — Psychometrics Commons

This file is a concise agent-facing index. `AGENTS.md`, accepted ADRs, `docs/PRD.md`, and `docs/TRD.md` are the normative repository instructions; measurement, AI, research, quality, risk, and compliance-readiness documents further constrain their respective domains.

## Repository role

`ContextualWisdomLab/psychometrics-commons` owns the hosted product/runtime and integration composition for instrument publication; participant/session/item-delivery/response lifecycle; consent/data rights; scoring dispatch and immutable results; tenant/resource authorization; research-contribution handoff; persistence, APIs, reference clients, deployment and operations.

It does **not** own psychometric numerical kernels, identity credentials, temporal model kernels, public research catalog internals, or generic LLM routing.

## Dependency direction

```text
clients -> psychometrics-commons -> reusable CWL bounded contexts
```

Never reverse this by making `fast-mlsirm`, Keyverse, TEPP, semantic-data-portal, contextual-orchestrator, or another reusable service depend on Psychometrics Commons product internals. No service may read/write another service's normal application database directly.

## Required reading before material changes

1. `AGENTS.md`
2. `docs/PRD.md`
3. `docs/TRD.md`
4. `docs/MEASUREMENT_GOVERNANCE.md` for measurement/scoring/instrument evidence
5. `docs/AI_GOVERNANCE.md` for AI/judge/narrative/provider behavior
6. `docs/RESEARCH_GOVERNANCE.md` for research contribution/staging/release/access
7. `docs/QUALITY_ATTRIBUTES.md` for measurable non-functional scenarios
8. `docs/RISK_REGISTER.md` for material risks/evidence state
9. `docs/COMPLIANCE_READINESS.md` for assurance/certification-readiness evidence boundaries
10. `docs/GLOSSARY.md` for canonical terminology
11. `ARCHITECTURE.md`
12. `docs/adr/README.md` and relevant ADRs
13. `docs/TRACEABILITY.md`
14. relevant architecture view under `docs/architecture/`
15. `docs/ROADMAP.md`

A change that contradicts an accepted ADR requires a superseding ADR in the same or a prerequisite PR.

## Architecture documents

- `docs/architecture/C4.md` — context, containers, components
- `docs/architecture/UML.md` — domain/state/sequence models
- `docs/architecture/ERD.md` — logical data model/cardinalities
- `docs/architecture/SECURITY_AND_DATA.md` — trust/data/privacy boundaries
- `docs/architecture/DEPLOYMENT_AND_OPERATIONS.md` — profiles, failure, recovery
- `docs/DOCUMENTATION_ASSESSMENT.md` — completeness/gap assessment
- `docs/TRACEABILITY.md` — target versus named protected-main implementation/evidence
- `docs/ROADMAP.md` — dependency-ordered delivery

Diagrams describe target semantics; they do not prove implementation. `TRACEABILITY.md` distinguishes implemented protected-main code from target architecture. `RISK_REGISTER.md` distinguishes architecture mitigation from evidence-backed risk closure. `COMPLIANCE_READINESS.md` distinguishes architecture, implementation, verification, and external assessment.

## Continuation discipline

Do not stop because one useful artifact is complete. A design/doc/diagram, commit, CI start, review request, merge, or blocked branch is intermediate while another safe repository action exists. After a material action, return to the PR/issue/backlog queue. Documentation/design must continue into implementation in the same run when writer lease and run budget permit.

## Scientific boundary

Psychometric arithmetic and scientific measurement contracts remain in `fast-mlsirm`. Product application code may validate, orchestrate, persist references, and marshal outputs but must not recreate likelihoods, scoring kernels, calibration, DIF, linking, uncertainty, model selection, or other owned numerical behavior.

Measurement publication rules are in `docs/MEASUREMENT_GOVERNANCE.md`. Correlation alone is insufficient evidence of estimation accuracy; use intended-use appropriate recovery, uncertainty, fairness/invariance, and scoreability evidence.

LLMs are optional bounded helpers. They cannot change numeric scores, norms, uncertainty, calibration, DIF, or scientific gates. Core scoring/result access must work without AI. See `docs/AI_GOVERNANCE.md`.

## Data, research, and identity boundary

- Keyverse owns authentication/federation/credentials.
- Anonymous assessment is first-class.
- Product authorization is server-side and resource/tenant scoped.
- Operational identity and research pseudonym identity are separate namespaces.
- Public research releases contain no Keyverse subject, operational participant reference, or restricted linkage key.
- Research release governance is defined in `docs/RESEARCH_GOVERNANCE.md`.
- Do not solve privacy by blanket masking that removes data required for authorized work; use purpose-bound schemas, access controls, encryption, restricted linkage, retention policy, and audit.

## Development quality

- TDD: realistic RED at intended boundary, minimal fix, GREEN, focused/full verification.
- Beginner-readable public docs/docstrings.
- Exact 100% owned production statement/branch coverage target plus other metrics where tooling exposes them; no meaningless exclusions/tests.
- Database objects use descriptive two-or-more-word `snake_case`; public IDs are opaque/non-numeric.
- Preserve idempotency, immutable snapshots, exact version/digest provenance, tenant isolation, and fail-closed unknown semantics.
- Material ownership/lifecycle/API/event/data/scientific/AI/research/security/deployment changes update affected architecture/governance/traceability/risk artifacts or explicitly prove they are unaffected.
- Use current primary standards/official docs/peer-reviewed research where material and record APA 7 references in authoritative documentation/ADRs.

## Automation and model tests

- Model-backed tests use GitHub Secret `NVIDIA_NIM_API_KEY`, preferably through contextual-orchestrator where appropriate.
- Never use `COPILOT_GITHUB_TOKEN` for development automation.
- Preserve independent review-agent credential identity/scope.
- Do not create competing or self-modifying writer workflows.

## Release

Do not release from a feature branch merely because its tests pass. Release only from exact integrated protected head after required CI, security, coverage, accessibility, packaging, SBOM/provenance, reproducibility, compatibility, independent review, migration/rollback, backup/restore (when applicable), scientific/instrument, and product-acceptance gates pass. GA/SLO/RPO/RTO claims additionally require measured deployment-profile evidence under ADR-0017. Compliance-readiness architecture is never represented as an external SOC 2/CSAP attestation or certification.
