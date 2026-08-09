# ADR-0016: Architecture description viewpoints and traceability

- Status: Accepted
- Date: 2026-08-09
- Scope: architecture-description governance, diagrams, target/as-built distinction, requirements-to-implementation traceability
- Supersedes: none

## Context

Psychometrics Commons has detailed textual PRD/TRD/ADRs, but commercial due diligence and multi-team implementation require several complementary architecture views: stakeholders and concerns, system context, containers, components, domain structures, lifecycle/state behavior, sequence behavior, logical data relationships, security/data boundaries, and deployment/operations.

Without an explicit governance rule, diagrams can become decorative and stale, or a diagram may silently contradict an accepted ADR. Conversely, treating code as the only source of truth hides intended future interfaces and rationale.

## Decision

The repository maintains an architecture description organized by stakeholder concerns and viewpoints in the spirit of ISO/IEC/IEEE 42010:2022.

The mandatory architecture artifact set is:

- `docs/PRD.md` — product intent and acceptance;
- `docs/TRD.md` — technical contracts;
- `ARCHITECTURE.md` — architecture map and authority/index;
- `docs/adr/` — material architecture decisions;
- `docs/architecture/C4.md` — system context, containers, components;
- `docs/architecture/UML.md` — structural and behavioral UML-aligned views;
- `docs/architecture/ERD.md` — logical product-owned data model;
- `docs/architecture/SECURITY_AND_DATA.md` — trust, privacy, classification, threat/data flows;
- `docs/architecture/DEPLOYMENT_AND_OPERATIONS.md` — target deployment, degraded modes, recovery evidence;
- `docs/TRACEABILITY.md` — PRD/TRD/ADR/source/test status mapping;
- `docs/ROADMAP.md` — dependency-ordered delivery and exit criteria.

## Authority hierarchy

When artifacts disagree:

1. accepted or superseding ADR governs architecture ownership/decision;
2. PRD governs intended product behavior and acceptance;
3. TRD governs technical contract;
4. Architecture/view documents explain/model those contracts;
5. machine-readable as-built contracts and migrations must conform to the above;
6. code/tests/operations that contradict higher-level normative artifacts are defects until the governing artifact is deliberately superseded.

No diagram can silently override an ADR or product acceptance requirement.

## Target versus as-built

Every architecture/model document must make clear whether it is:

- **Normative target** — intended semantics and required future shape;
- **As-built** — implemented and verified on a named protected-main baseline/release;
- **Mixed** — target architecture with explicitly identified implementation status.

Target diagrams may show not-yet-implemented components only when labeled accordingly. They cannot be used as release evidence.

`docs/TRACEABILITY.md` records implementation status against a named protected-main baseline. Status changes require source/test evidence.

## Model kinds

### Context/container/component

Used for responsibility, system boundary, deployment-unit, and dependency questions.

### UML-aligned models

Used for domain structure, state machines, and interaction sequences. Mermaid is the repository-native renderer, but the semantic intent follows OMG UML 2.5.1 where applicable.

### ERD

Defines logical data ownership/cardinality/immutability, not necessarily one-table-per-class physical DDL.

### Security/data view

Defines trust boundaries, classifications, prohibited flows, privacy and identity separation.

### Deployment/operations view

Defines profile composition, capability degradation, observability, backup/restore, migration and release evidence.

## Traceability rule

A material change to any of the following requires a traceability update or an explicit reason why no mapping changes:

- owned bounded context;
- lifecycle state or transition;
- public/admin operation family;
- event family;
- logical entity/cardinality;
- persistent transaction boundary;
- cross-service dependency;
- trust/privacy/identity boundary;
- deployment profile or recovery contract;
- consumer/research release acceptance criterion.

## Diagram quality rules

- diagrams use actual domain names rather than vague `Service A` labels;
- arrows declare dependency/data/control meaning in accompanying text when ambiguous;
- external systems are clearly outside the product-owned persistence boundary;
- diagrams do not imply direct database access where only an API/event reference exists;
- state diagrams show fail-closed terminal alternatives where material;
- sequence diagrams include immutable snapshot/version binding at scientific boundaries;
- sensitive identity/research linkage boundaries are visibly distinguished;
- diagrams remain readable in GitHub without proprietary tooling.

## Failure and degraded modes

Documentation drift is treated as a release-quality defect when it can change implementation or operator interpretation.

If a diagram is stale but code/ADR is correct, update or remove the diagram; do not rationalize the contradiction. If implementation intentionally changes the architecture, create/supersede the relevant ADR first or in the same reviewed change.

## Validation

CI should progressively enforce:

- linked architecture files exist;
- Mermaid/diagram syntax is renderable where automated validation is available;
- ADR index contains every ADR and status;
- traceability paths reference real source/test files;
- machine-readable API/event/schema artifacts are linked when implemented;
- physical schema fitness checks compare required logical ERD invariants once migrations exist.

Human architecture review remains necessary for semantic contradictions that syntax checks cannot detect.

## Alternatives considered

### One large `ARCHITECTURE.md`

Rejected. It becomes difficult to review, reason about, and keep synchronized across product, data, security, behavior, and operations concerns.

### Diagrams only

Rejected. Visual models without normative prose, acceptance, failure modes, and rationale are insufficient.

### Code-only documentation

Rejected. Code cannot by itself express product intent, rejected alternatives, future dependency boundaries, or due-diligence concerns.

### Proprietary diagram tool as sole source

Rejected. Figma may be used for product design, but architecture governance needs reviewable text/source in the repository.

## Consequences

Positive:

- a reviewer can navigate architecture by concern;
- target and implemented state are not conflated;
- architectural drift becomes visible;
- product, scientific, privacy, and operational decisions share a traceable evidence chain.

Cost:

- material changes require multi-artifact maintenance;
- CI/tooling for diagram/schema/traceability fitness must grow with implementation.

## Reversal conditions

The notation or file decomposition may change if a better repository-native architecture-description framework is adopted. The requirements for multiple stakeholder viewpoints, explicit authority, target/as-built distinction, and traceability remain.

## References

International Organization for Standardization. (2022). *ISO/IEC/IEEE 42010:2022 Software, systems and enterprise—Architecture description* (2nd ed.). ISO.

Object Management Group. (2017). *OMG Unified Modeling Language (OMG UML), Version 2.5.1*.
