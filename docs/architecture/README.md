# Psychometrics Commons Architecture Views

This directory contains repository-native architecture views for different stakeholder concerns. **Normative** means the document describes a rule or target behavior that implementations are expected to follow; it does not mean the behavior is already deployed. An **ADR** records an architecture decision and its rationale. The **PRD** defines intended product behavior and acceptance. The **TRD** defines technical contracts and constraints. These views explain those contracts, but they are not substitutes for executable code, migrations, API schemas, or release evidence.

## Start here

If you are new to the repository, read [`../GLOSSARY.md`](../GLOSSARY.md) first for canonical terminology, then [`../../ARCHITECTURE.md`](../../ARCHITECTURE.md) for the overall map. Continue with the PRD/TRD for product and technical requirements, review relevant ADRs for the decisions behind them, and finally use the architecture view below that matches the question you are investigating. `TRACEABILITY.md` tells you which target contracts are actually implemented on the named protected-main baseline.

## View index

| View | Primary questions answered |
|---|---|
| [`C4.md`](C4.md) | Who uses the product? Which external CWL bounded contexts exist? What are the target product containers/components and dependency directions? |
| [`UML.md`](UML.md) | What are the main domain concepts, lifecycle states, and interaction sequences? |
| [`ERD.md`](ERD.md) | What product-owned entities/relationships/cardinalities/immutability and restricted linkage must persistence preserve? |
| [`RESPONSE_EVENT_PERSISTENCE.md`](RESPONSE_EVENT_PERSISTENCE.md) | How does active-PR response-event persistence preserve the accepted mid-session prefix, exact replay identity, distinct clocks, restart reconstruction, and recovery evidence without claiming protected-main delivery? |
| [`SECURITY_AND_DATA.md`](SECURITY_AND_DATA.md) | Where are trust/data/privacy boundaries? What data classes and threats govern identity, research, AI, and tenancy? |
| [`DEPLOYMENT_AND_OPERATIONS.md`](DEPLOYMENT_AND_OPERATIONS.md) | How do Community, Hosted, and Enterprise profiles compose? How do capability degradation, observability, backup/restore, migration, and GA recovery evidence work? |

Related authoritative artifacts:

- [`../../ARCHITECTURE.md`](../../ARCHITECTURE.md) — overall architecture map and decision-governance entry point
- [`../PRD.md`](../PRD.md) — intended product behavior and acceptance
- [`../TRD.md`](../TRD.md) — detailed technical contracts
- [`../MEASUREMENT_GOVERNANCE.md`](../MEASUREMENT_GOVERNANCE.md) — measurement publication/evidence, model selection, recovery, fairness, automated scoring and item-bank governance
- [`../AI_GOVERNANCE.md`](../AI_GOVERNANCE.md) — bounded AI tasks, deterministic fallback, provider/privacy and judge governance
- [`../RESEARCH_GOVERNANCE.md`](../RESEARCH_GOVERNANCE.md) — research contribution, identity separation, release, withdrawal and reproducibility governance
- [`../QUALITY_ATTRIBUTES.md`](../QUALITY_ATTRIBUTES.md) — measurable quality-attribute scenarios and evidence requirements
- [`../COMPLIANCE_READINESS.md`](../COMPLIANCE_READINESS.md) — assurance/control evidence maturity and certification non-claim
- [`../RISK_REGISTER.md`](../RISK_REGISTER.md) — material architecture/product/scientific/security/privacy/operational risks and evidence state
- [`../GLOSSARY.md`](../GLOSSARY.md) — canonical terminology across architecture, APIs, code, UI and research artifacts
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
6. **Measurement, AI, and research policies remain separate.** A model or AI capability cannot weaken scientific publication, participant consent, privacy, or research-release gates defined by their governing documents.
7. **Risk/quality/compliance claims require evidence level.** Architecture mitigation or readiness does not equal implemented control, exact-release verification, risk closure, or external certification.
