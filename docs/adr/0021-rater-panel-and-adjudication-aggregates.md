# ADR-0021: Rater panel, observation request, and adjudication aggregates

Status: **Proposed**  
Date: 2026-08-29

## Context

Psychometrics Commons owns hosted assessment lifecycle, consent, response,
scoring dispatch, and immutable result publication. It must now coordinate human,
model, and algorithmic raters without absorbing provider execution or
psychometric estimation.

A single mutable `rating` record would conflate four different facts:

1. which exact rater configuration was assigned;
2. whether a provider invocation succeeded, abstained, or failed;
3. which numerical calibration artifact was produced; and
4. whether a separate human adjudication changed an operational disposition.

That model would allow a review action to rewrite source observations, discard
provider failures from the denominator, treat repeated calls as independent
raters, and couple hosted persistence to provider or `fast-mlsirm` internals.

The domain-neutral published language is being established upstream by
`ContextualWisdomLab/fast-mlsirm` PR #1603. Observation creation is separately
owned by `contextual-orchestrator` PR #917. This product therefore needs a
workflow model that references those artifacts without importing either
context's entities or calculations.

## Decision

Create three separate aggregate roots.

### `RaterPanelDefinition`

Owns one immutable panel revision and its product workflow rules:

- stable panel and panel-revision references;
- external calibration-design reference;
- exact rater-configuration assignments;
- repeat index within the same configuration;
- blind-allocation group;
- optional anchor-response references;
- `Draft -> Published -> Retired` lifecycle.

Only a draft panel may change. Publication requires at least one assignment and
freezes assignments and anchors. A repeated invocation changes the repeat index;
it does not create a new independent rater configuration.

### `ObservationRequest`

Owns one request to one panel assignment for one response-evidence reference and
one unique criterion set. Its lifecycle is:

```text
Pending -> Dispatched -> Received
                      -> Failed
```

`Received` stores only the immutable external invocation reference. `Failed`
stores an explicit failure reference so provider timeout, malformed output, and
other failed attempts remain in the operational and validation denominator.

### `AdjudicationCase`

Owns a separate review transaction over at least two immutable source invocation
references. Its lifecycle is:

```text
Open -> Resolved
     -> Dismissed
```

Resolution records an immutable resolution artifact reference. It never edits,
deletes, averages, or replaces the source invocations.

## Context relationships

```text
contextual-orchestrator
  Rater Observation
        | invocation/failure references
        v
psychometrics-commons
  Rater Panel + Observation Request + Adjudication
        | observation panel / scoring request artifacts
        v
fast-mlsirm
  Measurement Calibration
        | parameter and score artifacts
        v
psychometrics-commons
  immutable result publication
        |
        v
TEPP
  temporal monitoring
```

The same product may call upstream and downstream contexts, but each integration
crosses a versioned contract. Service databases and internal Rust/Python domain
types are never shared.

## Invariants

- all public references are exact opaque values and use the repository's shared
  reference guard;
- assignment identities are unique within a panel revision;
- `(rater_configuration_ref, repeat_index)` is unique within a panel revision;
- anchor references are unique;
- published and retired panels are immutable;
- criterion sets are non-empty and unique;
- only dispatched requests may become received or failed;
- a received request cannot later fail, and a failed request cannot later
  receive an invocation;
- adjudication requires at least two unique source invocations;
- resolved or dismissed cases cannot be changed;
- none of these aggregates calculates a score, threshold, placement, or final
  decision.

## Consequences

### Benefits

- transaction boundaries match actual business facts;
- source observations remain immutable and auditable;
- failed calls remain measurable denominator events;
- panel revisions can be reproduced independently of provider and estimator
  implementations;
- future domain profiles, including language assessment, reuse the same hosted
  workflow without becoming the product core.

### Costs

- persistence migrations and outbox events are still required in a later PR;
- scoring dispatch must translate panel/request projections to a released
  `fast-mlsirm` contract;
- the UI must represent failure and abstention rather than hiding them behind a
  single confidence indicator;
- cross-repository contract tests are required before production release.

## Alternatives considered

1. **One rating aggregate containing assignment, output, score, and review.**
   Rejected because it crosses multiple authorities and permits destructive
   review updates.
2. **Store rater panels in `fast-mlsirm`.** Rejected because assignment,
   authorization, persistence, and adjudication are hosted product concerns.
3. **Let `contextual-orchestrator` persist panels.** Rejected because the
   provider gateway must not become the assessment system of record.
4. **Treat every repeated model call as a new rater.** Rejected because it
   overstates independent evidence and prevents configuration-level variance
   estimation.

## Verification

The Rust module tests every lifecycle branch, duplicate constraint, invalid
reference, failed-request denominator state, and source-preserving adjudication
transition. Follow-up persistence work must add PostgreSQL uniqueness,
transactional outbox, tenant isolation, clean-install and upgrade rehearsal, and
consumer-driven contract tests against the released upstream schemas.

## References

American Educational Research Association, American Psychological Association,
& National Council on Measurement in Education. (2014). *Standards for
educational and psychological testing*. American Educational Research
Association.

Evans, E. (2003). *Domain-driven design: Tackling complexity in the heart of
software*. Addison-Wesley.

Vernon, V. (2013). *Implementing domain-driven design*. Addison-Wesley.
