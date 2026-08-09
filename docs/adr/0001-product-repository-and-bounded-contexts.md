# ADR-0001: Product repository and bounded-context ownership

- Status: Accepted
- Date: 2026-08-09
- Deciders: ContextualWisdomLab maintainers
- Scope: `psychometrics-commons`, `fast-mlsirm`, Keyverse, TEPP, Gyeot, `semantic-data-portal`, `contextual-orchestrator`
- Supersedes: the earlier proposal to incubate the hosted runtime under `fast-mlsirm/services/assessment_runtime`

## Context

Psychometrics Commons needs a hosted product runtime, public APIs, product persistence, UI composition, consent workflows, and integrations. `fast-mlsirm` already owns reusable psychometric contracts and numerical kernels. Placing the hosted product inside `fast-mlsirm` would couple a general Rust/Python measurement library to product-specific HTTP, database, identity, deployment, and release concerns. It would also make other products consume Psychometrics Commons transitively when they only need measurement capabilities.

## Decision

`ContextualWisdomLab/psychometrics-commons` is the product repository and the sole owner of the hosted assessment runtime and integration composition.

`fast-mlsirm` remains a domain-neutral measurement dependency. The product may import or call versioned `fast-mlsirm` contracts and services; `fast-mlsirm` must never import Psychometrics Commons code, database models, HTTP types, or deployment configuration.

No service may read or write another service's database directly. Integration occurs through versioned API contracts, immutable artifacts, or events.

## Ownership and boundaries

| Responsibility | Owner | Interface | Explicitly not owned |
|---|---|---|---|
| Instrument publication, sessions, item delivery, responses, consent snapshots, result access | psychometrics-commons | public API and domain events | psychometric formulas and credentials |
| AssessmentSpec, RubricSpecification, scoring contracts, calibration and diagnostics | fast-mlsirm | Rust/Python package and optional service API | participant sessions and product DB |
| Authentication, federation, passkeys, account linking | Keyverse | OIDC/OAuth tokens and claims | assessment authorization and consent |
| EMA/ESM collection | Gyeot | sync API and observation events | temporal model estimation |
| Temporal/event/relationship analysis | TEPP | analysis job and artifact contracts | client collection and product sessions |
| Dataset catalog, release metadata, provenance, discovery | semantic-data-portal | release registration API/event | operational response store |
| Real-time AI orchestration | contextual-orchestrator | bounded task API | deterministic score calculation |

## Dependency rules

Allowed:

```text
clients -> psychometrics-commons -> fast-mlsirm
psychometrics-commons -> Keyverse / TEPP / semantic-data-portal / contextual-orchestrator
```

Forbidden:

```text
fast-mlsirm -> psychometrics-commons
TEPP -> psychometrics-commons database
semantic-data-portal -> operational response database
client -> fast-mlsirm internal kernel ABI
```

## Invariants

1. `fast-mlsirm` builds, tests, and releases without Psychometrics Commons.
2. Psychometrics Commons can replace a dependency with a contract-compatible implementation.
3. Cross-repository types are serialized through an explicit versioned schema; ORM entities never cross the boundary.
4. Product-specific fields cannot be added to `fast-mlsirm` unless they are demonstrably reusable across at least two independent assessment domains.
5. A repository rename or service outage cannot make historical result snapshots uninterpretable because exact contract and artifact versions are preserved.

## Failure behavior

A dependency outage blocks only the capability that dependency owns. Existing result retrieval remains available when scoring or AI is unavailable. AI unavailability falls back to deterministic narrative templates. Research-release unavailability queues an outbox event; it does not block personal results. Identity unavailability does not terminate an already established anonymous session.

## Migration

Any hosted-runtime code created under `fast-mlsirm/services/assessment_runtime` solely because of the former plan must be migrated or superseded. Reusable contracts and numerical code stay in `fast-mlsirm`; HTTP routes, product persistence, consent, identity mapping, and UI move to this repository. Migration requires contract tests proving equivalent requests and result provenance before the old path is removed.

## Validation and release evidence

- repository dependency graph test rejects reverse imports;
- consumer-driven contract tests cover each cross-repository integration;
- no cross-service database credentials are present;
- standalone `fast-mlsirm` package and Psychometrics Commons product builds both pass;
- failure-injection tests prove capability-scoped degradation.

## Alternatives rejected

- **Hosted runtime in fast-mlsirm:** rejected because it reverses the dependency direction and couples library releases to product operations.
- **Everything in g7:** rejected because a CMS must remain a replaceable client.
- **Shared monorepo/database across all CWL services:** rejected because it destroys independent deployment and bounded ownership.

## Reversal conditions

Revisit only if organizational ownership, release cadence, and deployment are permanently consolidated into one product and independent library consumption is no longer required. Even then, database ownership and contract boundaries must remain explicit.
