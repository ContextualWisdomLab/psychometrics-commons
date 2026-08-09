# AGENTS.md — Psychometrics Commons

## Repository role

This repository owns the **hosted Psychometrics Commons product**: public/admin APIs, instrument publication, participant/session lifecycle, item delivery, response events, consent and data-rights workflows, immutable result snapshots, product persistence, UI/reference-client composition, and integrations.

`ContextualWisdomLab/fast-mlsirm` is a read-only upstream dependency during work in this repository and owns reusable AssessmentSpec/Rubric/Scoring contracts and psychometric numerical kernels. Do not recreate those kernels or move product HTTP/database concerns into `fast-mlsirm`.

## Execution loop

For every run: refetch exact repository and PR state → inspect reviews/checks/security findings → fix valid findings test-first → rerun exact-head validation → merge only when protection and independent-review requirements are satisfied → continue with the next executable product gap. Do not terminate with status narration while safe work remains.

## ADR quality contract

An ADR is not complete when it only names a technology or gives a one-paragraph preference. Every proposed or accepted ADR must be implementation-ready and include:

1. concrete context, constraints, assumptions, and decision drivers;
2. owning repository/service/bounded context for every responsibility;
3. allowed dependency direction and explicitly forbidden coupling;
4. API, event, schema, identifier, idempotency, ordering, timeout, retry, and error contracts;
5. invariants with named tests or controls that enforce them;
6. fail-closed, degraded-mode, recovery, and poison-message behavior;
7. authentication, authorization, tenancy, privacy, residency, encryption, retention, and audit consequences;
8. migration, compatibility window, rollback triggers, and rollback mechanics;
9. measurable scientific, security, accessibility, contract, and operational release evidence;
10. rejected alternatives, accepted risks, follow-up work, and objective reversal conditions.

Use `docs/adr/0000-template.md`. One-line ADRs, aspirational prose without ownership, and decisions that omit failure behavior are not acceptable. A material implementation that contradicts an accepted ADR must first add a superseding ADR in the same PR or a prerequisite PR.

## Architecture invariants

- g7 is a replaceable reference client, not a product dependency.
- Keyverse owns identity/federation; this repository owns domain authorization, anonymous participation, consent, and data rights.
- Cross-service integration uses versioned APIs/events/artifacts; no cross-service database access.
- Measurement and scoring remain Rust-first through `fast-mlsirm`; LLMs cannot alter numeric scores, norms, uncertainty, DIF, or scientific gates.
- Operational identity, research identity, and public release data remain separated.
- Gyeot collects EMA/ESM; TEPP owns temporal/event/multiple-membership analysis; semantic-data-portal owns research catalog/release presentation.
- Published instruments, results, manifests, and research releases are immutable and content-addressed.
- `kaefa`, `aFIPC`, and `nonnest2` are not product runtime or validation-oracle dependencies.

## Quality requirements

- Beginner-readable public documentation and docstrings.
- Exact owned-production statement and branch coverage target: 100%, with realistic tests rather than exclusions.
- Database object names use descriptive two-or-more-word `snake_case` by default; public IDs are opaque and non-numeric.
- Psychometric features require true-parameter recovery, bias/RMSE, interval coverage, invariance/DIF, numerical-boundary, and backend-parity evidence as applicable; correlation alone is insufficient.
- Use current primary standards, official documentation, and peer-reviewed research. Record APA 7 references in authoritative doctoring/ADRs when they materially support a decision.
- Never bypass branch protection, synthesize approval, invent credentials, weaken gates, or use `COPILOT_GITHUB_TOKEN`. Model-backed tests use `NVIDIA_NIM_API_KEY` and preferably contextual-orchestrator when applicable.
