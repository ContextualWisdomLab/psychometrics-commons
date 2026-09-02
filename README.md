# Psychometrics Commons

[![Ask DeepWiki](https://deepwiki.com/badge.svg)](https://deepwiki.com/ContextualWisdomLab/psychometrics-commons)

**Evidence-bound psychometric assessment, longitudinal observation, and consent-governed research workflows.**

Psychometrics Commons is the hosted product and integration boundary for turning versioned instruments and measurement evidence into participant-facing assessment workflows without duplicating the numerical kernels that belong in specialist measurement libraries.

```text
Measure → Understand → Reflect → Observe Over Time → Contribute to Science
```

The product is designed for participants, researchers and instrument developers, product operators/data stewards, and institutional integrators that need explicit provenance, authorization, consent, and release boundaries around psychometric workflows.

## What this repository owns

Psychometrics Commons owns product lifecycle and hosting semantics, including:

- instrument publication and version identity;
- participant and assessment-session lifecycle;
- response and item-delivery evidence;
- consent and data-rights workflows;
- scoring dispatch and immutable result snapshots;
- result authorization, reporting, and export-domain contracts;
- longitudinal observation records;
- product-owned PostgreSQL persistence and recovery boundaries;
- research-contribution handoff and release evidence;
- health, authorization, integration-delivery, and operator-facing runtime primitives.

It deliberately does **not** become the authority for every adjacent concern:

| Concern | Authority |
| --- | --- |
| Reusable psychometric numerical kernels and measurement computation | [`fast-mlsirm`](https://github.com/ContextualWisdomLab/fast-mlsirm) |
| Identity and federation | Keyverse |
| Temporal/event and longitudinal-model computation | [`TEPP`](https://github.com/ContextualWisdomLab/TEPP) |
| Research catalog and public release registration | [`semantic-data-portal`](https://github.com/ContextualWisdomLab/semantic-data-portal) |
| Generic provider/LLM orchestration | [`contextual-orchestrator`](https://github.com/ContextualWisdomLab/contextual-orchestrator) |
| Optional replaceable reference-client composition | `g7` |

Integration happens through explicit, versioned boundaries rather than cross-service application-table access or copied implementation code.

## Current maturity

This repository is an **implementation-stage headless runtime**, not a published end-user application release. `Cargo.toml` currently identifies source version `0.1.0` and `publish = false`, and there is currently no GitHub release for this repository.

Protected `main` is the shipped-source authority. PRDs, ADRs, diagrams, gap ledgers, and open pull requests can describe target or candidate behavior, but they are not release evidence on their own. Use the [traceability map](docs/TRACEABILITY.md), [product/technical gap baseline](docs/product-technical-gap-baseline.md), and [release acceptance contract](docs/RELEASE_ACCEPTANCE.md) when deciding whether a capability is implemented, candidate, or still planned.

## Evaluate the source

The crate is a Rust library/runtime foundation rather than a single install-and-run binary. For a repository-level evaluation, use the same toolchain and database boundary exercised by CI.

### Prerequisites

- Rust `1.97.1` with `rustfmt` and `clippy`;
- PostgreSQL 18 for the full persistence/integration test suite;
- Python 3 for the repository coverage-contract tests.

Set a disposable PostgreSQL test connection through `TEST_DATABASE_URL`, then run:

```bash
cargo fmt --all -- --check
cargo check --locked --all-targets
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
python3 -m unittest discover -s tests -p 'test_*.py' -v
cargo doc --no-deps
```

Do not point test execution at production data. The repository CI uses an ephemeral PostgreSQL database and treats exact-head hosted checks as integration evidence.

## Product principles

### Continuous measurement stays primary

Psychometrics Commons is designed around continuous scores, uncertainty, versioned scoring/norm provenance, and explicit interpretation limitations. Presentation narratives must not silently become new psychometric scores or unsupported personality types.

### Evidence before authority

Caller-provided participant, tenant, result, consent, or release identifiers are not authority merely because they appear in a request. Product operations bind decisions to stored identity, authorization, provenance, and lifecycle evidence and fail closed when that evidence is missing or inconsistent.

### Research contribution is a separate choice

Using the assessment product does not implicitly enroll a participant in research. Research contribution requires its own consent and pseudonymization/privacy/release path; public research catalog ownership remains outside this repository.

### Integration does not erase ownership

External identity, measurement, temporal, catalog, and orchestration systems remain separate authorities. Psychometrics Commons composes them into product workflows without copying their databases or numerical kernels.

## Architecture at a glance

The Rust crate exposes product-domain and persistence modules for participant/session state, instruments, responses, consent, data rights, scoring, results, longitudinal observations, authorization, health, and integration delivery. PostgreSQL adapters persist product-owned durable state. External systems connect through explicit contracts and anti-corruption boundaries.

For the full bounded-context and data model, see [ARCHITECTURE.md](ARCHITECTURE.md), the [architecture view index](docs/architecture/README.md), and the [logical ERD](docs/architecture/ERD.md).

## Security, privacy, and scientific integrity

The repository separates several evidence classes that should not be collapsed into a single “ready” claim:

- **security:** identity, tenancy, authorization, secrets, dependency and supply-chain controls;
- **privacy:** consent, data rights, research separation, retention and restricted linkage evidence;
- **scientific validity:** instrument rights/provenance, scoring policy, calibration, DIF/invariance, norms, uncertainty and interpretation limitations;
- **operability:** migration, backup/restore, failure recovery, degraded modes and observable health;
- **release:** exact source identity, required checks, reproducibility, artifacts, SBOM/provenance, and post-publication verification.

Start with the [Threat Model](docs/THREAT_MODEL.md), [Measurement Governance](docs/MEASUREMENT_GOVERNANCE.md), [Research Commons Governance](docs/RESEARCH_GOVERNANCE.md), and [Operability and Recovery](docs/OPERABILITY.md).

## Documentation

| Need | Start here |
| --- | --- |
| Product scope and users | [PRD](docs/PRD.md) |
| Runtime/API/data requirements | [TRD](docs/TRD.md) |
| Product and integration architecture | [ARCHITECTURE.md](ARCHITECTURE.md) |
| Current implementation vs gaps | [Product/Technical Gap Baseline](docs/product-technical-gap-baseline.md) |
| Requirement-to-implementation evidence | [Traceability](docs/TRACEABILITY.md) |
| Measurement publication governance | [Measurement Governance](docs/MEASUREMENT_GOVERNANCE.md) |
| Research contribution/release governance | [Research Governance](docs/RESEARCH_GOVERNANCE.md) |
| Security boundary | [Threat Model](docs/THREAT_MODEL.md) |
| Testing and evidence | [Test Strategy](docs/TEST_STRATEGY.md) |
| Release decision | [Release Acceptance](docs/RELEASE_ACCEPTANCE.md) |
| Architecture decisions | [ADR Index](docs/adr/README.md) |
| Public documentation landing | [docs/index.md](docs/index.md) |

## Contributing and support

Before changing a product boundary, read [AGENTS.md](AGENTS.md), [CLAUDE.md](CLAUDE.md), the relevant ADRs, and the current gap/traceability evidence. Keep product lifecycle semantics in this repository and reusable psychometric numerical work in its owning measurement library.

For defects or product gaps, use this repository's GitHub issues with a reproducible failing case and the affected contract/evidence boundary. Security-sensitive reports should follow the repository's documented security process rather than include secrets or participant data in a public issue.

## License

ContextualWisdomLab-authored Psychometrics Commons source and documentation are licensed under the [Apache License 2.0](LICENSE). `Cargo.toml` carries the same `Apache-2.0` identifier.

Third-party dependencies and external services retain their own terms; the repository license does not relicense them. The current direct Rust PostgreSQL dependency comes from the `rust-postgres` project, which is offered under MIT or Apache-2.0 terms. Future dependency, asset, dataset, model, or copied-source changes remain subject to separate commercial-license and provenance review.
