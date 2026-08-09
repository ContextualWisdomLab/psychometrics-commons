# CLAUDE.md — Psychometrics Commons

This file is a concise agent-facing index. `AGENTS.md`, accepted ADRs, `docs/PRD.md`, and `docs/TRD.md` are the normative repository instructions; measurement, AI, and research governance documents further constrain their respective domains.

## Repository role

`ContextualWisdomLab/psychometrics-commons` owns the hosted product/runtime and integration composition for:

- instrument publication;
- participant/session/item-delivery/response lifecycle;
- consent and data rights;
- scoring dispatch and immutable result snapshots;
- tenant/resource authorization;
- research-contribution handoff;
- product persistence, APIs, reference clients, deployment and operations.

It does **not** own psychometric numerical kernels, identity credentials, temporal model kernels, public research catalog internals, or generic LLM routing.

## Dependency direction

```text
clients -> psychometrics-commons -> reusable CWL bounded contexts
```

Never reverse this by making `fast-mlsirm`, Keyverse, TEPP, semantic-data-portal, contextual-orchestrator, or another reusable service depend on Psychometrics Commons product internals.

No service may read/write another service's normal application database directly.

## Required reading before material changes

1. `AGENTS.md`
2. `docs/PRD.md`
3. `docs/TRD.md`
4. `docs/MEASUREMENT_GOVERNANCE.md` when measurement/scoring/instrument evidence changes
5. `docs/AI_GOVERNANCE.md` when AI/judge/narrative/provider behavior changes
6. `docs/RESEARCH_GOVERNANCE.md` when contribution/staging/release/access changes
7. `ARCHITECTURE.md`
8. `docs/adr/README.md` and relevant ADRs
9. `docs/TRACEABILITY.md`
10. relevant architecture view under `docs/architecture/`
11. `docs/ROADMAP.md` for dependency-ordered delivery

A change that contradicts an accepted ADR requires a superseding ADR in the same or a prerequisite PR.

## Architecture documents

- `docs/architecture/C4.md` — context, containers, components
- `docs/architecture/UML.md` — domain/state/sequence models
- `docs/architecture/ERD.md` — logical data model and cardinalities
- `docs/architecture/SECURITY_AND_DATA.md` — trust/data/privacy boundaries
- `docs/architecture/DEPLOYMENT_AND_OPERATIONS.md` — profiles, failure, recovery
- `docs/DOCUMENTATION_ASSESSMENT.md` — completeness/gap assessment
- `docs/TRACEABILITY.md` — target versus named protected-main implementation/evidence
- `docs/ROADMAP.md` — dependency-ordered product delivery

Diagrams describe target semantics; they do not prove implementation. `docs/TRACEABILITY.md` distinguishes implemented protected-main code from target architecture.

## Continuation discipline

Do not stop work merely because one useful artifact is complete. A design/doc/diagram, commit, CI start, review request, merge, or blocked branch is intermediate while another safe repository action exists. After a material action, return to the PR/issue/backlog queue. Documentation/design must continue into implementation in the same run when the writer lease and run budget permit.

## Scientific boundary

Psychometric arithmetic and scientific measurement contracts remain in `fast-mlsirm`. Product application code may validate, orchestrate, persist references, and marshal outputs but must not recreate likelihoods, scoring kernels, calibration, DIF, linking, uncertainty, model selection, or other owned numerical behavior.

Measurement publication rules are in `docs/MEASUREMENT_GOVERNANCE.md`. Correlation alone is insufficient evidence of estimation accuracy; use model/intended-use appropriate recovery, uncertainty, fairness/invariance, and scoreability evidence.

LLMs are optional bounded helpers. They cannot change numeric scores, norms, uncertainty, calibration, DIF, or scientific gates. Core scoring/result access must work without AI. See `docs/AI_GOVERNANCE.md`.

## Data, research, and identity boundary

- Keyverse owns authentication/federation/credentials.
- Anonymous assessment is first-class.
- Product authorization is server-side and resource/tenant scoped.
- Operational identity and research pseudonym identity are separate namespaces.
- Public research releases contain no Keyverse subject, operational participant reference, or restricted linkage key.
- Research release governance, staging/privacy/scientific review, access class, withdrawal, correction, and reproducibility are defined in `docs/RESEARCH_GOVERNANCE.md`.
- Do not solve privacy by blanket masking that removes data required for authorized work; use purpose-bound schemas, access controls, encryption, restricted linkage, retention policy, and audit.

## Development quality

- TDD for behavior changes: realistic RED at the intended boundary, minimal fix, GREEN, focused/full verification.
- Beginner-readable public docs/docstrings.
- Exact 100% owned production statement and branch coverage target plus other coverage metrics where tooling exposes them; no meaningless exclusions/tests.
- Database objects use descriptive two-or-more-word `snake_case` names by default.
- Public identifiers are opaque and non-numeric.
- Preserve idempotency, immutable snapshots, exact version/digest provenance, tenant isolation, and fail-closed unknown semantics.
- Use current primary standards/official docs/peer-reviewed research where material and record APA 7 references in authoritative doctoring/ADRs.
- Material ownership/lifecycle/API/event/data/security/deployment changes update affected architecture views and `docs/TRACEABILITY.md` or explicitly prove they are unaffected.

## Automation and model tests

- Model-backed tests use GitHub Secret `NVIDIA_NIM_API_KEY`, preferably through contextual-orchestrator where appropriate.
- Never use `COPILOT_GITHUB_TOKEN` for development automation.
- Preserve independent review-agent credential identity/scope.
- Do not create competing or self-modifying writer workflows.

## Release

Do not release from a feature branch merely because its tests pass. Release only from the exact integrated protected head after required CI, security, coverage, accessibility, packaging, SBOM/provenance, reproducibility, compatibility, independent review, migration/rollback, backup/restore (when applicable), scientific/instrument, and product-acceptance gates pass. GA/SLO/RPO/RTO claims additionally require measured deployment-profile evidence under ADR-0017.
