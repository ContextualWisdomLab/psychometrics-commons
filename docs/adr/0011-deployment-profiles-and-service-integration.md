# ADR-0011: Deployment profiles and service integration

- Status: Accepted
- Date: 2026-08-09
- Scope: community, hosted, enterprise, self-hosted, service communication and persistence

## Context

The platform must run as a small research deployment, a CWL hosted service, and an enterprise/self-hosted system. A design that assumes every CWL service is always present would prevent standalone use; a single-process implementation without contracts would prevent modular composition.

## Decision

Psychometrics Commons supports three deployment profiles over the same domain contracts:

### Community/research

- Psychometrics Commons runtime;
- PostgreSQL-compatible operational store;
- `fast-mlsirm` local package or co-located scoring service;
- simple standalone client;
- anonymous mode available;
- optional integrations disabled cleanly.

### CWL hosted

- Keyverse, semantic-data-portal, contextual-orchestrator, TEPP, Gyeot and reporting integrations as enabled capabilities;
- managed observability, release provenance, and research-release workflows.

### Enterprise/self-hosted

- customer identity federation through Keyverse-compatible claims;
- explicit data residency, retention, encryption, and provider policies;
- customer frontend or embed client permitted;
- offline/private model and scoring deployment supported where contracted.

## Integration pattern

Each service owns its database. Synchronous APIs handle user-facing queries and commands requiring immediate validation. Domain events handle durable cross-service propagation. Transactional outbox/inbox and idempotent consumers are required for state-changing integration.

Events use stable event IDs, source, type, schema version, occurred time, subject reference, correlation reference, and payload digest. Consumers persist deduplication state before applying side effects.

## Configuration and capability discovery

Capabilities are explicitly configured and reported. The product does not infer that a dependency exists from DNS or environment-variable presence. Startup validates mandatory dependencies for the selected profile; optional capabilities are marked unavailable with typed health detail.

## Invariants

1. Community profile works without g7, TEPP, AI, or semantic-data-portal.
2. No profile changes the meaning of measurement contracts or scores.
3. Cross-service writes are idempotent and no service shares application tables.
4. Secrets are supplied by the deployment secret manager and never committed or echoed.
5. Tenant context is authenticated and propagated explicitly; no default tenant is used for state-changing calls.
6. Optional-integration failure is capability-scoped.

## Rollout and rollback

Database migrations are backward-compatible for at least one application deployment window. Destructive migration requires verified backup/restore and a separate release gate. Events are forward/backward tested across the supported compatibility window. Feature flags may disable a new integration but cannot silently alter scoring semantics.

## Validation

- profile-specific deployment tests;
- service unavailability and network-partition tests;
- outbox/inbox duplicate and ordering tests;
- tenant-isolation tests;
- migration rollback/restore drills;
- self-hosted installation and upgrade acceptance.

## Alternatives rejected

- **All services mandatory:** breaks standalone operation.
- **Shared database:** bypasses bounded contexts and independent scaling.
- **Synchronous calls for all propagation:** fragile and tightly coupled.
- **Different domain models per deployment profile:** makes results non-portable.

## Reversal conditions

A profile may be retired based on product strategy, but remaining profiles must retain the same versioned domain contracts and historical-result portability.
