# ADR-0016: Architecture description viewpoints and traceability

- Status: Accepted
- Date: 2026-08-09
- Deciders: ContextualWisdomLab Psychometrics Commons maintainers
- Scope: architecture-description governance, diagrams, target/as-built distinction, requirements-to-implementation traceability
- Supersedes: none
- Superseded by: none
- Current/as-built status: protected main has PRD, TRD, root architecture and ADR baseline; the multi-view architecture pack introduced by this change is active-PR documentation until merged
- Target status: one discoverable, multi-view, code-current architecture graph with machine-checkable traceability and explicit target-versus-as-built evidence
- Migration status: documentation-only migration; no product data migration is required, but old or contradictory architecture claims must be superseded/updated rather than left as parallel authority

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
- `docs/MEASUREMENT_GOVERNANCE.md` — scientific publication/model interpretation evidence;
- `docs/AI_GOVERNANCE.md` — bounded model authority/provider policy;
- `docs/RESEARCH_GOVERNANCE.md` — research contribution/release governance;
- `docs/TRACEABILITY.md` — PRD/TRD/ADR/source/test status mapping;
- `docs/ROADMAP.md` — dependency-ordered delivery and exit criteria;
- `docs/DOCUMENTATION_ASSESSMENT.md` — documented completeness/evidence gaps.

## Ownership and boundaries

| Responsibility | Owner | Interface | Forbidden coupling |
|---|---|---|---|
| Product requirements | psychometrics-commons product governance | `docs/PRD.md` | diagrams silently redefining product acceptance |
| Technical contracts | psychometrics-commons architecture/engineering governance | `docs/TRD.md` | implementation-specific shortcuts overriding required semantics |
| Material architecture decisions | accountable maintainers/deciders | accepted/superseding ADRs | silent architectural drift |
| Architecture viewpoints | psychometrics-commons | version-controlled Mermaid/text views | proprietary diagram as sole source of truth |
| Implementation evidence | owning source/test/migration modules | code/tests/contracts/releases | target diagrams presented as shipped evidence |
| External bounded-context behavior | owning CWL repository | versioned external contract/reference | copying another service's internal schema into local authority |

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

`docs/TRACEABILITY.md` records implementation status against a named protected-main baseline. Status changes require source/test evidence. Active PR state is not promoted to protected-main implementation until the exact change is integrated.

## Model kinds

### Context/container/component

Used for responsibility, system boundary, deployment-unit, and dependency questions.

### UML-aligned models

Used for domain structure, state machines, and interaction sequences. Mermaid is the repository-native renderer, but the semantic intent follows OMG UML 2.5.1 where applicable.

### ERD

Defines logical data ownership/cardinality/immutability, not necessarily one-table-per-class physical DDL. Conceptual/target entities must not be presented as persisted until migrations exist.

### Security/data view

Defines trust boundaries, classifications, prohibited flows, privacy and identity separation.

### Deployment/operations view

Defines profile composition, capability degradation, observability, backup/restore, migration and release evidence.

## Contract details

Every architecture view must identify its status and scope and link to the normative decisions/requirements it explains. A material concept uses the same canonical name across PRD/TRD/ADR/UML/ERD/code unless an adapter mapping is explicit.

Traceability records distinguish at minimum:

```text
Implemented
Partially implemented
Target
External dependency
```

When an active PR is relevant, the traceability note may identify it as active work but must not mark the behavior implemented on protected main before merge. Unstable SHAs/run IDs belong in dated evidence, PRs, or release records rather than timeless architecture prose except when a named baseline is deliberately being assessed.

## Data and persistence impact

This ADR does not itself create or change product persistence. `docs/architecture/ERD.md` is a logical target until physical migrations exist. When persistence lands, schema/migration evidence must be linked from traceability and validated against the logical invariants rather than inferred from the diagram.

Architecture metadata/traceability may be stored as repository files; no application database is required for documentation governance.

## Invariants

1. An accepted ADR cannot be silently contradicted by a lower-authority diagram or implementation comment.
2. A target component/entity/API/event cannot be represented as implemented without source/test/migration/contract evidence on the named protected-main/release baseline.
3. Every numbered ADR is indexed with its status.
4. Required architecture entry points are discoverable from README/root Architecture or their documented index.
5. A material ownership/lifecycle/API/event/entity/transaction/security/deployment change updates affected viewpoints or records an explicit reason that the view is unaffected.
6. External CWL bounded contexts remain references/contracts, never local cross-service foreign keys or copied source-of-truth internals.
7. A documentation fitness test may prove structure/consistency, but it cannot substitute for human semantic architecture review or operational evidence.

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
- consumer/research release acceptance criterion;
- scientific publication or AI authority rule.

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

If automated diagram/traceability tooling is unavailable, repository-native text/Mermaid and human review remain authoritative; lack of optional rendering tooling does not authorize dropping the required semantic content.

## Security, privacy, and tenancy

Architecture documents must not contain real credentials, participant data, restricted linkage values, or confidential tenant payloads. Examples use synthetic references. Security/privacy views must preserve purpose and authority boundaries rather than simplifying them away for visual neatness.

Tenant boundaries, restricted linkage, and external provider/data-class restrictions are material architecture concerns and therefore require view/traceability updates when changed.

## Deployment and operations impact

Documentation is packaged/versioned with source and reviewed through the same protected repository workflow. Operators must be able to identify the current release's applicable architecture, compatibility, migration, recovery, and runbook references without reconstructing chat history.

A deployment diagram that describes a target profile is not an as-built topology record. Exact environment topology, regions, secrets, backup systems, SLO/RPO/RTO, and runtime evidence are recorded only when deployed and verified.

## Migration and rollback

The documentation migration introduces/updates the canonical view graph without changing product runtime data. Existing strong documents are linked/consolidated instead of duplicated. Obsolete claims are removed or explicitly superseded.

Rollback of a documentation-only change is safe only if it does not reintroduce a contradiction with already integrated source/ADR behavior. If implementation has advanced to depend on a new contract, roll forward the documentation rather than reverting to stale architecture.

## Architecture-view impact

This ADR governs all files under `docs/architecture/`, the architecture index in `ARCHITECTURE.md`, ADR indexing, and `docs/TRACEABILITY.md`. Changes to this governance rule must update `docs/DOCUMENTATION_ASSESSMENT.md` and documentation fitness tests.

## Validation and release evidence

CI should progressively enforce:

- linked architecture files exist and are non-empty;
- Mermaid/diagram syntax is renderable where automated validation is available;
- ADR index contains every numbered ADR and status;
- required ADR metadata/section contract is preserved;
- traceability paths reference real source/test files or explicitly labelled target/external work;
- machine-readable API/event/schema artifacts are linked when implemented;
- physical schema fitness checks compare required logical ERD invariants once migrations exist;
- canonical terminology/profile names do not drift across normative documents.

Human architecture review remains necessary for semantic contradictions that syntax checks cannot detect.

## Alternatives considered

### One large `ARCHITECTURE.md`

Rejected. It becomes difficult to review, reason about, and keep synchronized across product, data, security, behavior, scientific, research, and operations concerns.

### Diagrams only

Rejected. Visual models without normative prose, acceptance, failure modes, and rationale are insufficient.

### Code-only documentation

Rejected. Code cannot by itself express product intent, rejected alternatives, future dependency boundaries, or due-diligence concerns.

### Proprietary diagram tool as sole source

Rejected. Figma may be used for product design, but architecture governance needs reviewable text/source in the repository.

## Consequences

Positive:

- a reviewer can navigate architecture by concern;
- target, active-PR work, and implemented state are not conflated;
- architectural drift becomes visible;
- product, scientific, privacy, and operational decisions share a traceable evidence chain.

Cost:

- material changes require multi-artifact maintenance;
- CI/tooling for diagram/schema/traceability fitness must grow with implementation.

## Follow-up work

- strengthen documentation fitness tests beyond file existence to metadata/status/canonical-name consistency;
- update traceability whenever protected main advances across a documented lifecycle or capability;
- add as-built OpenAPI/AsyncAPI/schema/deployment evidence only when corresponding implementations exist;
- periodically remove superseded/stale views rather than accumulating parallel authority.

## Traceability

- Product intent: `docs/PRD.md`.
- Technical contracts: `docs/TRD.md`.
- Architecture map/index: `ARCHITECTURE.md`, `docs/architecture/README.md`.
- Architecture views: `docs/architecture/C4.md`, `docs/architecture/UML.md`, `docs/architecture/ERD.md`, `docs/architecture/SECURITY_AND_DATA.md`, `docs/architecture/DEPLOYMENT_AND_OPERATIONS.md`.
- Status evidence: `docs/TRACEABILITY.md`, `docs/DOCUMENTATION_ASSESSMENT.md`.
- Fitness test: `tests/documentation_architecture_contract.rs`.

## Reversal conditions

The notation or file decomposition may change if a better repository-native architecture-description framework is adopted. The requirements for multiple stakeholder viewpoints, explicit authority, target/as-built distinction, and traceability remain.

## References

International Organization for Standardization. (2022). *ISO/IEC/IEEE 42010:2022 Software, systems and enterprise—Architecture description* (2nd ed.). ISO.

Object Management Group. (2017). *OMG Unified Modeling Language (OMG UML), Version 2.5.1*.
