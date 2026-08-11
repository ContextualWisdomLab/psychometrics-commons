# Architecture Decision Records

This directory is the authoritative decision log for Psychometrics Commons. ADRs govern implementation when README, PRD, TRD, issue text, and code comments disagree. A later accepted ADR may supersede an earlier one; silent architectural drift is not permitted.

## Required quality

Every ADR must identify:

1. the concrete problem and decision drivers;
2. the owning repository and bounded context for every affected responsibility;
3. allowed dependency direction and forbidden coupling;
4. public API, event, data, and versioning contracts;
5. invariants that tests and production controls must enforce;
6. fail-closed behavior and degraded-mode behavior;
7. privacy, security, tenancy, and data-residency consequences;
8. deployment, migration, rollback, and compatibility strategy;
9. measurable acceptance evidence and release gates;
10. alternatives rejected and conditions that would reverse the decision.

A statement such as “use Keyverse” or “make it headless” is not sufficient unless the ADR defines the integration boundary, failure behavior, and ownership consequences. The full required structure is in [0000-template.md](0000-template.md), including target/as-built status, data/persistence, deployment/operations, architecture-view impact, follow-up, and traceability.

## Status lifecycle

- `Proposed`: under review; implementation may be exploratory only.
- `Accepted`: normative and implementation-ready.
- `Deprecated`: still present for compatibility but no longer preferred.
- `Superseded`: replaced by a named ADR.
- `Rejected`: considered and intentionally not adopted.

## Decision index

| ADR | Decision | Status |
|---|---|---|
| [0001](0001-product-repository-and-bounded-contexts.md) | Product repository and bounded-context ownership | Accepted |
| [0002](0002-headless-clients-and-reference-applications.md) | Headless clients and replaceable reference applications | Accepted |
| [0003](0003-keyverse-identity-and-anonymous-participation.md) | Keyverse identity with anonymous participation | Accepted |
| [0004](0004-fast-mlsirm-measurement-and-scoring-contract.md) | fast-mlsirm as measurement and scoring source of truth | Accepted |
| [0005](0005-hosted-assessment-runtime-state-machine.md) | Hosted assessment runtime state machine | Accepted |
| [0006](0006-consent-data-rights-and-research-separation.md) | Consent, data rights, and research separation | Accepted |
| [0007](0007-semantic-data-portal-research-release-boundary.md) | Research-release boundary with semantic-data-portal | Accepted |
| [0008](0008-gyeot-and-tepp-longitudinal-boundary.md) | Gyeot and TEPP longitudinal boundary | Accepted |
| [0009](0009-bounded-ai-and-provider-egress.md) | Bounded AI and controlled provider egress | Accepted |
| [0010](0010-versioned-provenance-and-immutable-results.md) | Versioned provenance and immutable results | Accepted |
| [0011](0011-deployment-profiles-and-service-integration.md) | Deployment profiles and service integration | Accepted |
| [0012](0012-exclude-legacy-r-packages-from-product-dependencies.md) | Exclude legacy R packages from product dependencies | Accepted |
| [0013](0013-multilingual-accessibility-and-measurement-invariance.md) | Multilingual accessibility and measurement invariance | Accepted |
| [0014](0014-api-and-event-contract-representation.md) | Machine-readable HTTP and event contract representation | Accepted |
| [0015](0015-persistence-and-transaction-boundaries.md) | Product persistence and transaction boundaries | Accepted |
| [0016](0016-architecture-description-and-traceability.md) | Architecture description viewpoints and traceability | Accepted |
| [0017](0017-operational-recovery-and-ga-evidence.md) | Operational recovery and GA evidence contract | Accepted |
| [0018](0018-continuous-scores-and-narrative-separation.md) | Continuous psychometric scores and narrative-style separation | Accepted |
| [0019](0019-scientific-publication-evidence-gates.md) | Scientific publication and score-interpretation evidence gates | Accepted |
| [0020](0020-append-only-participant-identity-link-history.md) | Append-only participant identity-link history | Accepted |

Use [0000-template.md](0000-template.md) for new decisions.

Architecture decisions are linked to current protected-main implementation status in [`../TRACEABILITY.md`](../TRACEABILITY.md). An accepted ADR defines normative intent; it does not by itself prove that the described transport, persistence, deployment, scientific evidence, or operational control has already been implemented.
