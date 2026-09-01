# Psychometrics Commons

Psychometrics Commons is the ContextualWisdomLab product runtime for public psychometric assessment, reflective self-understanding, longitudinal observation, and consent-governed research contribution.

[Ask DeepWiki](https://deepwiki.com/ContextualWisdomLab/psychometrics-commons) · [Repository](https://github.com/ContextualWisdomLab/psychometrics-commons) · [Releases](https://github.com/ContextualWisdomLab/psychometrics-commons/releases)

## Start here

- [Product overview and architecture boundary](../README.md)
- [Product requirements](PRD.md)
- [Technical requirements](TRD.md)
- [Architecture](../ARCHITECTURE.md)
- [Architecture views](architecture/README.md)
- [Measurement governance](MEASUREMENT_GOVERNANCE.md)
- [Research Commons governance](RESEARCH_GOVERNANCE.md)
- [Security threat model](THREAT_MODEL.md)
- [Release acceptance](RELEASE_ACCEPTANCE.md)
- [Traceability](TRACEABILITY.md)

## Product responsibility

Psychometrics Commons owns the hosted assessment product boundary: instrument publication, participant and session lifecycles, response evidence, consent and data-rights workflows, scoring dispatch, immutable result snapshots, longitudinal observation, product persistence, authorization, deployment profiles, and governed research-contribution handoff.

Reusable psychometric numerical kernels remain owned by `fast-mlsirm`; identity and federation remain owned by Keyverse; temporal and event-modeling capabilities remain owned by TEPP; public research catalog and release registration remain owned by `semantic-data-portal`. Psychometrics Commons integrates those authorities rather than copying them.

## Evidence and release status

Repository documentation distinguishes protected-main implementation evidence from target architecture and active pull-request work. Product or scientific capability should be treated as shipped only when the protected branch and release evidence support that claim. The [product and technical gap baseline](product-technical-gap-baseline.md), [traceability map](TRACEABILITY.md), and [release acceptance contract](RELEASE_ACCEPTANCE.md) provide the current evidence model.

## Governance

Measurement, privacy, research contribution, security, operational readiness, and release claims are governed as separate evidence boundaries. Assessment results remain tied to immutable provenance and explicit limitations; research contribution is consent-governed and does not transfer public-catalog ownership into this repository.

## Documentation status

This page is a small public landing surface. Detailed product, architecture, scientific, privacy, security, operational, and release contracts stay versioned with the source so they can be reviewed against the implementation they describe.
