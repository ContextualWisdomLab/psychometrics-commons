# Changelog

All notable product and architecture changes are recorded here. Releases use immutable Git tags and provenance; entries move out of **Unreleased** only when the exact integrated protected head satisfies the repository's release gates.

## Unreleased

### Added

- Initial Psychometrics Commons product requirements covering the IPIP-based Big Five consumer vertical slice, reflective modules, longitudinal participation, Research Commons, and Measurement Workbench.
- Technical requirements for hosted runtime ownership, state machines, idempotent response/scoring flows, identity and tenant boundaries, consent and data-rights workflows, research pseudonymization and release, event/outbox integration, accessibility, multilingual instrument versions, deployment profiles, and release evidence.
- Detailed Architecture Decision Record governance and ADR-0001 through ADR-0013 for product ownership, headless clients, Keyverse integration, fast-mlsirm measurement ownership, hosted runtime lifecycle, consent/research separation, research-release boundaries, longitudinal analysis, bounded AI, immutable provenance, deployment profiles, legacy-R exclusion, and multilingual accessibility/invariance.
- Authoritative architecture map linking product modules to their owning CWL bounded contexts and defining failure-degradation behavior and architecture fitness functions.
