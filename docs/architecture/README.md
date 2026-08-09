# Psychometrics Commons Architecture Views

This directory contains repository-native architecture views for different stakeholder concerns. The views are normative explanations of accepted ADR/PRD/TRD contracts, but they are not a substitute for executable code, migrations, API schemas, or release evidence.

## View index

| View | Primary questions answered |
|---|---|
| [`C4.md`](C4.md) | Who uses the product? Which external CWL bounded contexts exist? What are the target product containers/components and dependency directions? |
| [`UML.md`](UML.md) | What are the main domain concepts, lifecycle states, and interaction sequences? |
| [`ERD.md`](ERD.md) | What product-owned entities/relationships/cardinalities/immutability and restricted linkage must persistence preserve? |
| [`SECURITY_AND_DATA.md`](SECURITY_AND_DATA.md) | Where are trust/data/privacy boundaries? What data classes and threats govern identity, research, AI, and tenancy? |
| [`DEPLOYMENT_AND_OPERATIONS.md`](DEPLOYMENT_AND_OPERATIONS.md) | How do Community, Hosted, and Enterprise profiles compose? How do capability degradation, observability, backup/restore, migration, and GA recovery evidence work? |

Related authoritative artifacts:

- [`../../ARCHITECTURE.md`](../../ARCHITECTURE.md) — overall architecture map and decision-governance entry point
- [`../PRD.md`](../PRD.md) — intended product behavior and acceptance
- [`../TRD.md`](../TRD.md) — detailed technical contracts
- [`../adr/README.md`](../adr/README.md) — authoritative architecture decisions
- [`../TRACEABILITY.md`](../TRACEABILITY.md) — target-to-implementation/evidence mapping
- [`../ROADMAP.md`](../ROADMAP.md) — delivery dependency order and exit criteria
- [`../DOCUMENTATION_ASSESSMENT.md`](../DOCUMENTATION_ASSESSMENT.md) — documentation completeness assessment and remaining GA evidence gaps

## Modeling rules

1. **Target is not as-built.** A target component, API, event, or entity is not considered implemented until source/test/migration/contract evidence exists and is reflected in `TRACEABILITY.md`.
2. **ADRs outrank diagrams.** If a view conflicts with an accepted/superseding ADR, the view is stale and must be fixed.
3. **No cross-service database inference.** A reference to an external system means an API/event/artifact contract unless an ADR explicitly states otherwise.
4. **Diagrams remain reviewable in GitHub.** Mermaid is used for repository-native rendering; external visual tools may supplement but not replace the version-controlled source.
5. **Material changes update affected views.** Lifecycle, ownership, entity/cardinality, trust boundary, deployment/recovery, or public interface changes require either the corresponding view update or an explicit traceability statement explaining why no view changes.
