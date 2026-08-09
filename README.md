# Psychometrics Commons

CWL Psychometrics Commons is the headless product repository for public psychometric assessment, reflective self-understanding, longitudinal observation, and consent-governed research data contribution.

```text
Measure -> Understand -> Reflect -> Observe Over Time -> Contribute to Science
```

This repository owns the hosted product runtime and integration composition. It consumes reusable measurement contracts and numerical capabilities from `ContextualWisdomLab/fast-mlsirm`; identity and federation from Keyverse; temporal/event analysis from TEPP; and research release/catalog capabilities from `semantic-data-portal`.

## Architecture boundary

Psychometrics Commons owns product APIs, instrument publication, participant/session lifecycle, response events, consent and data-rights workflows, scoring dispatch, immutable result snapshots, product persistence, resource authorization, reference-client composition, deployment profiles, research-contribution handoff, observability, and service integration.

It does **not** duplicate psychometric numerical kernels, identity credentials, temporal model kernels, public research catalog internals, or generic LLM orchestration. `g7` is an optional replaceable reference client rather than a platform dependency.

## Documentation

- [Product Requirements](docs/PRD.md) — users, consumer MVP, longitudinal and research experiences, acceptance criteria, exclusions, and release policy.
- [Technical Requirements](docs/TRD.md) — APIs/events/data contracts, state machines, identity/tenancy, idempotency, failure modes, security/privacy, accessibility, deployment, and release gates.
- [Architecture](ARCHITECTURE.md) — bounded contexts, dependency direction, runtime modules, data domains, integration consistency, and architecture fitness functions.
- [Architecture Decision Records](docs/adr/README.md) — authoritative material decisions and the required ADR quality contract.
- [Changelog](CHANGELOG.md) — unreleased and released product/architecture changes.

An accepted ADR must define concrete ownership, interfaces, invariants, failure behavior, security/privacy/tenancy boundaries, migration and rollback, validation evidence, alternatives, and reversal conditions. Material implementation that contradicts an accepted ADR requires an explicit superseding decision rather than silent architectural drift.
