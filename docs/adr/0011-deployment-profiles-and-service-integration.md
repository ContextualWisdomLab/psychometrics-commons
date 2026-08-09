# ADR-0011: Deployment profiles and service integration

- Status: Accepted
- Date: 2026-08-09
- Scope: Community, Hosted, Enterprise, service communication and persistence

## Context

The platform must run as a small community/research deployment, a CWL-operated hosted service, and an enterprise/customer-self-hosted system. A design that assumes every CWL service is always present would prevent standalone use; a single-process implementation without contracts would prevent modular composition.

## Decision

Psychometrics Commons supports three canonical deployment profiles over the same domain contracts:

### Community profile

- Psychometrics Commons runtime;
- upstream PostgreSQL 18.x operational store per ADR-0015;
- `fast-mlsirm` local package or co-located scoring service;
- simple standalone client;
- anonymous mode available;
- optional integrations disabled cleanly.

### Hosted profile

- CWL-operated deployment;
- Keyverse, semantic-data-portal, contextual-orchestrator, TEPP, Gyeot and reporting integrations as enabled capabilities;
- upstream PostgreSQL 18.x initial operational store per ADR-0015;
- managed observability, release provenance, and research-release workflows.

### Enterprise profile

- customer/self-hosted or contractually operated deployment;
- customer identity federation through Keyverse-compatible claims or another explicitly approved identity adapter;
- explicit data residency, retention, encryption, and provider policies;
- customer frontend or embed client permitted;
- offline/private model and scoring deployment supported where contracted;
- responsibility split for database, network, backup/recovery, observability, and provider operations is explicit rather than inferred from the profile name.

The canonical profile names are `Community profile`, `Hosted profile`, and `Enterprise profile`. Descriptions such as “research deployment,” “CWL hosted,” or “self-hosted” describe operating examples, not additional domain profiles.

## Integration pattern

Each service owns its database. Synchronous APIs handle user-facing queries and commands requiring immediate validation. Domain events handle durable cross-service propagation. Transactional outbox/inbox and idempotent consumers are required for state-changing integration.

Events use stable event IDs, source, type, schema version, occurred time, tenant/subject reference where applicable, correlation reference, and canonical payload digest according to ADR-0014. Consumers validate tenant/resource binding and use durable pending/processing/completed/quarantine evidence according to ADR-0014/ADR-0015; receipt alone is not side-effect completion.

## Configuration and capability discovery

Capabilities are explicitly configured and reported. The product does not infer that a dependency exists from DNS or environment-variable presence. Startup validates mandatory dependencies for the selected profile; optional capabilities are marked unavailable with typed health detail.

The initial database support claim is upstream PostgreSQL 18.x. A managed service/fork/wire-compatible implementation is not called supported until its exact required behavior passes the persistence/concurrency/recovery capability matrix in ADR-0015.

## Invariants

1. Community profile works without g7, TEPP, AI, or semantic-data-portal.
2. No profile changes the meaning of measurement contracts or scores.
3. Cross-service writes are idempotent and no service shares application tables.
4. Secrets are supplied by the deployment secret manager and never committed or echoed.
5. Tenant context is authenticated and propagated explicitly; no default tenant is used for state-changing calls.
6. Optional-integration failure is capability-scoped.
7. A deployment label does not broaden a database/provider/security compatibility claim beyond verified adapters.
8. Community/Hosted/Enterprise terminology is used consistently in product/technical/release documentation.

## Rollout and rollback

Database migrations are backward-compatible for at least one application deployment window. Destructive migration requires verified backup/restore and a separate release gate. Events are forward/backward tested across the supported compatibility window. Feature flags may disable a new integration but cannot silently alter scoring semantics.

A deployment moving from Community to Hosted or Enterprise preserves immutable resource/scoring provenance and domain semantics. Enabling a capability must not mutate historical results to make them appear created under the new profile.

## Validation

- profile-specific deployment tests;
- service unavailability and network-partition tests;
- outbox/inbox duplicate, tenant-binding, processing-state and ordering tests;
- tenant-isolation tests;
- migration rollback/restore drills;
- Community standalone installation acceptance;
- Hosted deployment acceptance;
- Enterprise/self-hosted responsibility and upgrade acceptance when offered;
- unsupported database/provider adapter rejection until explicit conformance evidence exists.

## Alternatives rejected

- **All services mandatory:** breaks standalone operation.
- **Shared database:** bypasses bounded contexts and independent scaling.
- **Synchronous calls for all propagation:** fragile and tightly coupled.
- **Different domain models per deployment profile:** makes results non-portable.
- **Treat every PostgreSQL-wire-compatible store as interchangeable:** transaction, locking, DDL, migration, proxy and recovery semantics require evidence rather than a label.

## Reversal conditions

A profile may be retired based on product strategy, but remaining profiles must retain the same versioned domain contracts and historical-result portability. Database support may expand only through the ADR-0015 conformance contract.
