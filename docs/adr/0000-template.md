# ADR-NNNN: Decision title

- Status: Proposed
- Date: YYYY-MM-DD
- Deciders: names or accountable team
- Scope: repositories, services, APIs, data classes
- Supersedes: none
- Superseded by: none

## Context

Describe the concrete problem, current state, constraints, and why a decision is required now. Include measurable decision drivers and explicitly identify assumptions that remain uncertain.

## Decision

State the decision in testable language. Identify the owning bounded context, dependency direction, and responsibilities that are explicitly out of scope.

## Ownership and boundaries

| Responsibility | Owner | Interface | Forbidden coupling |
|---|---|---|---|
| Example | repository/service | API/event/contract | direct DB access, reverse import, hidden shared state |

## Contract details

Specify request/response or event schemas, identifiers, idempotency, version negotiation, consistency model, ordering, timeout, retry, and error taxonomy. Link to OpenAPI, AsyncAPI, JSON Schema, protobuf, or Rust/Python types when they exist.

## Invariants

List properties that must always hold and the tests or controls that enforce them.

## Failure and degraded modes

Define fail-closed cases, retryable cases, partial availability, recovery, poison-message handling, and what users see.

## Security, privacy, and tenancy

Define authentication, authorization, data classification, encryption, purpose limitation, residency, audit, and cross-tenant protections. Do not use masking as a substitute for a viable operational data model.

## Migration and rollback

Define bootstrap, data migration, dual-read/write if applicable, compatibility window, rollback trigger, and rollback mechanics.

## Validation and release evidence

List required unit, integration, contract, security, accessibility, recovery, performance, and scientific validation. Include exact release blockers.

## Alternatives considered

For each alternative, state why it was rejected and what evidence could make it viable later.

## Consequences

Separate positive consequences, costs, operational burdens, and deliberately accepted risks.

## Reversal conditions

State objective conditions that require this decision to be revisited.

## Traceability

Link PRD/TRD requirements, issues, pull requests, diagrams, runbooks, standards, and research evidence.
