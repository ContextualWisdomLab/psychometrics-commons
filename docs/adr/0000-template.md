# ADR-NNNN: Decision title

- Status: Proposed
- Date: YYYY-MM-DD
- Deciders: names or accountable team
- Scope: repositories, services, APIs, data classes
- Supersedes: none
- Superseded by: none

## Context

Describe the concrete problem, current state, constraints, and why a decision is required now. Include measurable decision drivers and explicitly identify assumptions that remain uncertain.

State whether the decision describes current as-built behavior, target behavior, a migration between the two, or a mixture with clearly identified implementation gaps.

## Decision

State the decision in testable language. Identify the owning bounded context, dependency direction, and responsibilities that are explicitly out of scope.

## Ownership and boundaries

| Responsibility | Owner | Interface | Forbidden coupling |
|---|---|---|---|
| Example | repository/service | API/event/contract | direct DB access, reverse import, hidden shared state |

## Contract details

Specify request/response or event schemas, identifiers, idempotency, version negotiation, consistency model, ordering, timeout, retry, and error taxonomy. Link to OpenAPI, AsyncAPI, JSON Schema, protobuf, Rust/Python types, migrations, or other machine-readable contracts when they actually exist.

Do not create a speculative as-built contract for an unimplemented transport merely to satisfy documentation completeness. Target contracts must be labelled as target/non-deployed.

## Data and persistence impact

State which logical entities, relationships, cardinalities, immutable artifacts, transaction boundaries, data classifications, retention rules, and system-of-record references change. Identify whether `docs/architecture/ERD.md`, persistence migrations, or data-rights behavior must change.

If there is no data/persistence impact, state why.

## Invariants

List properties that must always hold and the tests or controls that enforce them.

## Failure and degraded modes

Define fail-closed cases, retryable cases, partial availability, recovery, poison-message handling, and what users see.

## Security, privacy, and tenancy

Define authentication, authorization, data classification, encryption, purpose limitation, residency, audit, and cross-tenant protections. Do not use masking as a substitute for a viable operational data model.

Identify trust-boundary or data-flow changes and update `docs/architecture/SECURITY_AND_DATA.md` when material.

## Deployment and operations impact

Define capability/dependency changes, health/readiness semantics, observability, backup/restore, migration, SLO/RPO/RTO implications, operator runbooks, and whether `docs/architecture/DEPLOYMENT_AND_OPERATIONS.md` changes.

If none, state why.

## Migration and rollback

Define bootstrap, data migration, dual-read/write if applicable, compatibility window, rollback trigger, and rollback mechanics. Distinguish true rollback from roll-forward/compensation when the operation is not actually reversible.

## Architecture-view impact

Review each version-controlled viewpoint and list whether it must change:

- `ARCHITECTURE.md`
- `docs/architecture/C4.md`
- `docs/architecture/UML.md`
- `docs/architecture/ERD.md`
- `docs/architecture/SECURITY_AND_DATA.md`
- `docs/architecture/DEPLOYMENT_AND_OPERATIONS.md`
- `docs/TRACEABILITY.md`
- `docs/ROADMAP.md`

For each unaffected view, a short reason is sufficient. A material change may not silently leave a contradictory diagram behind.

## Validation and release evidence

List required unit, integration, contract, security, privacy, tenancy, accessibility, recovery, performance, migration, backup/restore, and scientific validation as applicable. Include exact release blockers and distinguish implementation evidence from target-design evidence.

## Alternatives considered

For each alternative, state why it was rejected and what evidence could make it viable later.

## Consequences

Separate positive consequences, costs, operational burdens, and deliberately accepted risks.

## Follow-up work

List concrete implementation or evidence work created by the decision, with owning bounded context/repository where known. Avoid open-ended “improve later” tasks.

## Reversal conditions

State objective conditions that require this decision to be revisited.

## Traceability

Link PRD/TRD requirements, issues, pull requests, source/tests, machine-readable contracts, diagrams, runbooks, standards, and research evidence. Update `docs/TRACEABILITY.md` when the decision changes current/target mappings.
