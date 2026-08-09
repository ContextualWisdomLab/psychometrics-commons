# AGENTS.md — Psychometrics Commons

## Repository role

This repository owns the **hosted Psychometrics Commons product**: public/admin APIs, instrument publication, participant/session lifecycle, item delivery, response events, consent and data-rights workflows, immutable result snapshots, product persistence, UI/reference-client composition, and integrations.

`ContextualWisdomLab/fast-mlsirm` is a read-only upstream dependency during work in this repository and owns reusable AssessmentSpec/Rubric/Scoring contracts and psychometric numerical kernels. Do not recreate those kernels or move product HTTP/database concerns into `fast-mlsirm`.

## Execution loop

For every run: refetch exact repository and PR state → inspect reviews/checks/security findings → fix valid findings test-first → rerun exact-head validation → merge only when protection and independent-review requirements are satisfied → continue with the next executable product gap.

**Do not stop after the first useful action.** A commit, PR update, documentation improvement, diagram, design review, CI start, review request, green focused test, merge, blocked branch, or dependency diagnosis is an intermediate state whenever another safe executable item exists. Before ending, sweep open PRs/issues, Draft work, protected main, architecture/traceability gaps, product backlog, security/privacy/accessibility/operations, and release evidence. If any safe item can advance under the current writer lease, continue working rather than producing a status recap.

Waiting is local to the affected branch/action. CI, reviewer latency, provider cooldown, external approval, or a read-only dependency must not freeze unrelated repository work.

## Documentation and architecture completeness contract

The repository uses multiple architecture and governance viewpoints so product intent, measurement evidence, implementation, AI, research, security, data, and operations do not collapse into one stale document.

Required architecture/documentation artifacts are:

- `docs/PRD.md`
- `docs/TRD.md`
- `docs/MEASUREMENT_GOVERNANCE.md`
- `docs/AI_GOVERNANCE.md`
- `docs/RESEARCH_GOVERNANCE.md`
- `ARCHITECTURE.md`
- `docs/adr/README.md` and applicable ADRs
- `docs/architecture/C4.md`
- `docs/architecture/UML.md`
- `docs/architecture/ERD.md`
- `docs/architecture/SECURITY_AND_DATA.md`
- `docs/architecture/DEPLOYMENT_AND_OPERATIONS.md`
- `docs/TRACEABILITY.md`
- `docs/ROADMAP.md`
- `docs/DOCUMENTATION_ASSESSMENT.md`
- `CHANGELOG.md`

A material change to bounded-context ownership, lifecycle states, public/admin operation families, events, logical entities/cardinalities, transaction boundaries, psychometric publication criteria, AI authority, research-release policy, identity/privacy/trust boundaries, deployment/recovery semantics, or product acceptance must update the affected artifacts or explicitly demonstrate why their mappings are unchanged.

Architecture diagrams may describe target semantics, but diagrams are not as-built proof. `docs/TRACEABILITY.md` distinguishes current protected-main implementation from targets. Do not fabricate OpenAPI, AsyncAPI, physical DDL, deployed topology, SLO/RPO/RTO, certification, or integration evidence before the corresponding implementation/operation exists.

Documentation work is support for implementation, not a reason to stop. When documentation gaps are fixed and executable product work remains, continue into implementation in the same run when the writer lease and run budget permit.

## ADR quality contract

An ADR is not complete when it only names a technology or gives a one-paragraph preference. Every proposed or accepted ADR must be implementation-ready and include:

1. concrete context, constraints, assumptions, and decision drivers;
2. whether the decision describes as-built behavior, target behavior, or a migration with explicit gaps;
3. owning repository/service/bounded context for every responsibility;
4. allowed dependency direction and explicitly forbidden coupling;
5. API, event, schema, identifier, idempotency, ordering, timeout, retry, and error contracts;
6. data/persistence/cardinality/transaction impact;
7. invariants with named tests or controls that enforce them;
8. fail-closed, degraded-mode, recovery, and poison-message behavior;
9. authentication, authorization, tenancy, privacy, residency, encryption, retention, and audit consequences;
10. deployment/operations impact, migration, compatibility window, rollback/roll-forward mechanics;
11. affected architecture views and traceability;
12. measurable scientific, security, privacy, accessibility, contract, recovery, and operational release evidence;
13. rejected alternatives, accepted risks, concrete follow-up work, and objective reversal conditions.

Use `docs/adr/0000-template.md`. One-line ADRs, aspirational prose without ownership, decisions that omit failure behavior, and decisions that leave contradictory diagrams/data models behind are not acceptable. A material implementation that contradicts an accepted ADR must first add a superseding ADR in the same PR or a prerequisite PR.

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
