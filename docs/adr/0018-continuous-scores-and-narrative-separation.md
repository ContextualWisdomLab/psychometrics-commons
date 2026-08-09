# ADR-0018: Continuous psychometric scores and narrative-style separation

- Status: Accepted
- Date: 2026-08-09
- Scope: consumer Big Five results, Personality Style mapping, narrative generation, result provenance, client presentation
- Supersedes: none

## Context

The consumer product needs an accessible, memorable explanation layer without weakening the scientific meaning of continuous Big Five/facet scores. Type-style experiences can improve comprehension and recall, but converting continuous psychometric traits into an unofficial MBTI clone, hiding borderline profiles behind a hard category, or allowing an LLM to invent a type would create a second uncalibrated source of truth.

The product therefore needs an explicit architectural separation between **measurement** and **presentation/narrative**.

## Decision

1. Continuous Big Five and facet scores from the pinned fast-mlsirm scoring contract are the measurement source of truth.
2. `Personality Style` is a **separately versioned presentation mapping**, not a psychometric latent trait/type parameter.
3. Style assignment is deterministic for a given pinned ScoreProfile + mapping version. An LLM may render approved prose but may not decide or modify the style/score.
4. The product does not clone the official 16 MBTI types, use protected MBTI branding as a scientific equivalence claim, or present its styles as MBTI scores.
5. Borderline/near-prototype profiles may present adjacent or mixed styles according to the versioned mapping rather than forcing a single categorical identity.
6. Detailed result surfaces retain the underlying continuous/facet profile, uncertainty, limitations, and measurement provenance even when a narrative style is shown first.
7. Narrative/rule changes do not rewrite historical numeric results. A deliberate rerender/reinterpretation creates a new narrative/result-presentation snapshot or superseding result reference according to the product versioning contract.

## Ownership and boundaries

| Responsibility | Owner | Interface | Forbidden coupling |
|---|---|---|---|
| Big Five/facet numerical scoring and uncertainty | fast-mlsirm | pinned ScoreProfile/scoring contract | narrative/LLM modifying score |
| Narrative mapping rules/version | psychometrics-commons | versioned deterministic mapping contract | hidden unversioned prompt-only classification |
| Optional prose generation | contextual-orchestrator | bounded narrative task | treating model output as psychometric type truth |
| Result UI | psychometrics-commons reference clients | result API/resource | hiding all continuous evidence behind type label |

## Contract details

A narrative operation binds at minimum:

```text
result_snapshot_ref
score_profile_ref or immutable score payload digest
instrument_version_ref
scoring_version_ref
norm_version_ref optional
narrative_version_ref
locale
approved interpretation-rule references
```

The mapping output may contain:

```text
primary_style_ref
adjacent_style_refs[]
style_distance_or_membership evidence when the approved mapping defines it
interpretation_unit_refs[]
limitations
```

The exact mathematical mapping remains versioned product presentation logic. It cannot silently change when prose wording/model changes.

## Data and persistence impact

`result_snapshot` stores `narrative_version_ref` and enough immutable score provenance to reproduce the presentation mapping. If separately persisted, generated narrative content is immutable/versioned and references the exact result/mapping/model provenance.

A style name is not stored as the sole representation of participant personality. Export includes the underlying score/provenance appropriate to the participant-facing format.

## Invariants

1. Equal ScoreProfile + mapping version + locale produces the same deterministic style assignment.
2. LLM disabled/unavailable still yields approved deterministic result interpretation.
3. Changing an LLM/model/prompt cannot change numeric score or deterministic style assignment.
4. Style mapping cannot claim psychometric precision beyond the source ScoreProfile and uncertainty.
5. Adjacent/mixed style behavior is covered at exact boundary fixtures.
6. The client can show why the style appeared using underlying dimensions/approved interpretation units rather than generic Barnum prose only.
7. User feedback such as “not like me” may be collected as product/research feedback with consent but does not retroactively mutate the measured score.

## Failure and degraded modes

- Missing/unsupported narrative version: numeric result remains available; narrative fails with typed capability error.
- AI renderer failure/invalid output: use deterministic localized interpretation.
- Missing required score/profile provenance: fail closed; do not infer a style from partial text or user identity.
- Mapping rule contradiction/unknown semantics: block publication of that narrative version rather than choose an arbitrary style.

## Security, privacy, and tenancy

Narrative tasks receive only the product-authorized projection of result fields. They do not require direct identity credentials or unrestricted assessment response content. Tenant/resource authorization applies to result/narrative access.

If model rendering uses sensitive reflection content in later features, AI data/provider policies apply separately; Big Five narrative rendering should prefer structured score/profile data over raw participant response text.

## Deployment and operations impact

Narrative capability is optional. Health/readiness reports deterministic narrative availability separately from optional AI rendering availability. AI outage cannot mark the core result capability unavailable.

## Migration and rollback

Existing results gain a new narrative mapping only through explicit rerender/rescoring/product action that records the new version. Rollback of a bad narrative release restores the prior approved narrative version for new rendering; it does not mutate old persisted numeric results or erase supersession history.

## Architecture-view impact

- `ARCHITECTURE.md`: narrative layer must remain visibly downstream of ScoreProfile.
- `docs/architecture/C4.md`: no ownership change.
- `docs/architecture/UML.md`: result/narrative sequences must preserve source score binding.
- `docs/architecture/ERD.md`: `result_snapshot.narrative_version_ref` remains required.
- `docs/architecture/SECURITY_AND_DATA.md`: AI/result projection remains bounded.
- `docs/architecture/DEPLOYMENT_AND_OPERATIONS.md`: deterministic fallback/degraded mode remains required.
- `docs/TRACEABILITY.md`: narrative requirement maps to this ADR.
- `docs/ROADMAP.md`: consumer product phase uses this decision.

## Validation and release evidence

- deterministic mapping unit/property tests;
- boundary/near-boundary mixed-style fixtures;
- no-score-mutation tests across narrative/AI versions;
- Korean/English localized deterministic output tests;
- narrative faithfulness to approved score/rule inputs;
- prohibited diagnostic/MBTI-equivalence copy checks in reference-client acceptance;
- accessibility and result-explanation tests for the presentation surface.

## Alternatives considered

### Hard-code four dichotomies and 16 MBTI-like types

Rejected. It discards continuous information, creates artificial thresholds, and encourages an unsupported equivalence claim.

### Let an LLM read the response/result and choose a type

Rejected. It is non-deterministic, difficult to calibrate/reproduce, and creates a competing score source.

### Provide continuous scores only

Scientifically clean but rejected as the only consumer experience because the product intentionally includes an accessible narrative layer. The narrative remains subordinate to measured scores.

## Consequences

Positive:

- consumer accessibility without changing psychometric source of truth;
- deterministic/replayable style assignment;
- honest boundary/mixed profiles;
- optional AI prose without vendor lock-in to scientific meaning.

Costs:

- narrative rules require their own versioning, QA, localization, and empirical utility/calibration research;
- clients must present both accessible narrative and deeper continuous evidence.

## Follow-up work

- define the first original Personality Style mapping with explicit prototype/rule rationale;
- create boundary/mixed-profile fixture bank;
- design Result Explorer explanations showing source dimensions and uncertainty;
- evaluate Barnum susceptibility, perceived usefulness, calibration, and “not like me” feedback separately from score validity.

## Reversal conditions

If empirical evidence shows the narrative mapping is harmful, misleading, or adds no user value, the style layer can be retired while retaining continuous results. A future validated categorical instrument could be added only as its own independently measured construct, not as a silent reinterpretation of Big Five.
