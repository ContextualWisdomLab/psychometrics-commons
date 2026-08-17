# ADR-0018: Continuous psychometric scores and narrative-style separation

- Status: Accepted
- Date: 2026-08-09
- Deciders: ContextualWisdomLab Psychometrics Commons maintainers
- Scope: consumer Big Five results, Personality Style mapping, narrative generation, result provenance, client presentation
- Supersedes: none
- Superseded by: none
- Current/as-built status: continuous score/result provenance primitives are implemented on protected main; the consumer Personality Style mapping and narrative runtime are not yet implemented
- Target status: deterministic versioned style assignment plus deterministic localized narrative fallback, with optional bounded AI prose rendering
- Migration status: no persisted style-assignment records exist yet; the first implementation must introduce the canonical assignment identity without synthetic backfill claims

## Context

The consumer product needs an accessible, memorable explanation layer without weakening the scientific meaning of continuous Big Five/facet scores. Type-style experiences can improve comprehension and recall, but converting continuous psychometric traits into an unofficial MBTI clone, hiding borderline profiles behind a hard category, or allowing an LLM to invent a type would create a second uncalibrated source of truth.

The product therefore needs an explicit architectural separation between **measurement** and **presentation/narrative**.

## Decision

1. Continuous Big Five and facet scores from the pinned fast-mlsirm scoring contract are the measurement source of truth.
2. `Personality Style` is a **separately versioned presentation mapping**, not a psychometric latent trait/type parameter.
3. Style assignment is deterministic for one **canonical style-assignment key**. The key binds the immutable ScoreProfile identity or canonical score-payload digest, instrument version, scoring version, optional norm version, style-mapping version, approved interpretation-rule bundle digest, and locale. Any other input capable of changing assignment behavior must become an explicit versioned/digested key component before release.
4. An LLM may render approved prose but may not decide or modify the style/score.
5. The product does not clone the official 16 MBTI types, use protected MBTI branding as a scientific equivalence claim, or present its styles as MBTI scores.
6. Borderline/near-prototype profiles may present adjacent or mixed styles according to the versioned mapping rather than forcing a single categorical identity.
7. Detailed result surfaces retain the underlying continuous/facet profile, uncertainty, limitations, and measurement provenance even when a narrative style is shown first.
8. Narrative/rule changes do not rewrite historical numeric results. A deliberate rerender/reinterpretation creates a new narrative/result-presentation snapshot or superseding result reference according to the product versioning contract.

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
score_profile_ref or immutable canonical score_payload_digest
instrument_version_ref
scoring_version_ref
norm_version_ref optional
style_mapping_version_ref
interpretation_rule_bundle_digest
narrative_version_ref
locale
```

The deterministic style-assignment identity is computed from a canonical UTF-8 serialization of the behavior-affecting fields:

```text
style_assignment_key = sha256(
  score_profile_ref_or_digest,
  instrument_version_ref,
  scoring_version_ref,
  norm_version_ref_or_explicit_none,
  style_mapping_version_ref,
  interpretation_rule_bundle_digest,
  locale
)
```

The implementation must define one unambiguous canonical serialization before hashing; concatenating strings without field names/lengths is forbidden. A future additional behavior-affecting field requires a mapping-contract version change and inclusion in the canonical key. Model/provider/prompt identity is **not** part of style assignment because optional AI can change wording only; it belongs to separately versioned narrative-rendering provenance.

The mapping output may contain:

```text
style_assignment_key
primary_style_ref
adjacent_style_refs[]
style_distance_or_membership evidence when the approved mapping defines it
interpretation_unit_refs[]
limitations
```

The exact mathematical mapping remains versioned product presentation logic. It cannot silently change when prose wording/model changes.

## Data and persistence impact

`result_snapshot` stores `narrative_version_ref` and enough immutable score provenance to reproduce the presentation mapping. A persisted style/narrative artifact additionally stores or can recompute the canonical `style_assignment_key` from immutable referenced inputs. If generated narrative content is separately persisted, it is immutable/versioned and references the exact result, style-assignment key, mapping/rules, locale, and optional model-rendering provenance.

A style name is not stored as the sole representation of participant personality. Export includes the underlying score/provenance appropriate to the participant-facing format.

## Invariants

1. Identical canonical style-assignment keys always produce exactly the same primary/adjacent style assignment.
2. Any difference in a behavior-affecting score, instrument, scoring, norm, mapping, interpretation-rule, or locale input is represented by a distinguishable canonical key before assignment.
3. LLM disabled/unavailable still yields approved deterministic result interpretation.
4. Changing an LLM/model/prompt cannot change numeric score or deterministic style assignment.
5. Style mapping cannot claim psychometric precision beyond the source ScoreProfile and uncertainty.
6. Adjacent/mixed style behavior is covered at exact boundary fixtures.
7. The client can show why the style appeared using underlying dimensions/approved interpretation units rather than generic Barnum prose only.
8. User feedback such as “not like me” may be collected as product/research feedback with consent but does not retroactively mutate the measured score.

## Failure and degraded modes

- Missing/unsupported narrative or style-mapping version: numeric result remains available; narrative fails with typed capability error.
- AI renderer failure/invalid output: use deterministic localized interpretation.
- Missing required score/profile provenance or canonical key component: fail closed; do not infer a style from partial text or user identity.
- Mapping rule contradiction, digest mismatch, or unknown semantics: block publication of that mapping/narrative version rather than choose an arbitrary style.
- Canonical-key recomputation mismatch for persisted evidence: treat the narrative/style artifact as unverifiable and do not silently reuse it.

## Security, privacy, and tenancy

Narrative tasks receive only the product-authorized projection of result fields. They do not require direct identity credentials or unrestricted assessment response content. Tenant/resource authorization applies to result/narrative access.

If model rendering uses sensitive reflection content in later features, AI data/provider policies apply separately; Big Five narrative rendering should prefer structured score/profile data over raw participant response text.

## Deployment and operations impact

Narrative capability is optional. Health/readiness reports deterministic narrative availability separately from optional AI rendering availability. AI outage cannot mark the core result capability unavailable. Digest/key validation failures are observable as typed narrative-provenance failures without logging the sensitive score payload.

## Migration and rollback

Existing results gain a new narrative mapping only through explicit rerender/rescoring/product action that records the new version and canonical assignment inputs. No pre-existing result is claimed to have an historical style assignment unless the exact source inputs can be bound and verified. Rollback of a bad narrative release restores the prior approved mapping/narrative version for new rendering; it does not mutate old persisted numeric results or erase supersession history.

## Architecture-view impact

- `ARCHITECTURE.md`: narrative layer must remain visibly downstream of ScoreProfile.
- `docs/architecture/C4.md`: no ownership change.
- `docs/architecture/UML.md`: result/narrative sequences must preserve source score and mapping-key binding.
- `docs/architecture/ERD.md`: `result_snapshot.narrative_version_ref` remains required; a future persisted style artifact must carry canonical assignment provenance.
- `docs/architecture/SECURITY_AND_DATA.md`: AI/result projection remains bounded.
- `docs/architecture/DEPLOYMENT_AND_OPERATIONS.md`: deterministic fallback/degraded mode remains required.
- `docs/TRACEABILITY.md`: narrative requirement maps to this ADR.
- `docs/ROADMAP.md`: consumer product phase uses this decision.

## Validation and release evidence

- deterministic mapping unit/property tests;
- canonical-key serialization/digest tests, including locale/rule/norm/version changes;
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
- clients must present both accessible narrative and deeper continuous evidence;
- canonical style-assignment identity must be maintained when behavior-affecting inputs evolve.

## Follow-up work

- define the first original Personality Style mapping with explicit prototype/rule rationale;
- define the canonical style-assignment serialization schema and test vectors before persistence/API implementation;
- create boundary/mixed-profile fixture bank;
- design Result Explorer explanations showing source dimensions and uncertainty;
- evaluate Barnum susceptibility, perceived usefulness, calibration, and “not like me” feedback separately from score validity.

## Traceability

- Product requirements: `docs/PRD.md` consumer narrative and result acceptance sections.
- Technical requirements: `docs/TRD.md` result snapshot, version compatibility, AI task, multilingual, and release requirements.
- Architecture: `ARCHITECTURE.md`, `docs/architecture/UML.md`, `docs/architecture/ERD.md`.
- AI policy: `docs/AI_GOVERNANCE.md`.
- Delivery/evidence: `docs/TRACEABILITY.md`, `docs/ROADMAP.md`, `docs/RISK_REGISTER.md`.

## Reversal conditions

If empirical evidence shows the narrative mapping is harmful, misleading, or adds no user value, the style layer can be retired while retaining continuous results. A future validated categorical instrument could be added only as its own independently measured construct, not as a silent reinterpretation of Big Five.

## Standards basis

Continuous Big Five and facet scores remain the measurement source of truth because validity and score reporting attach to the intended interpretation of those scores, not to a later presentation label (American Educational Research Association [AERA], American Psychological Association [APA], & National Council on Measurement in Education [NCME], 2014; Kane, 2013). Personality Style is a versioned presentation mapping. It is not a latent trait, and an LLM may not decide or alter the numeric result.

Those numeric scores come from the pinned `fast-mlsirm` item-response-theory kernel (Embretson & Reise, 2000; Lord, 1980). When that kernel is the upstream multilevel latent-space item-response implementation, the scientific source is Jeon et al. (2021). This product does not reimplement that model.

## References

American Educational Research Association, American Psychological Association, & National Council on Measurement in Education. (2014). *Standards for educational and psychological testing*. American Educational Research Association. https://www.testingstandards.net/

Embretson, S. E., & Reise, S. P. (2000). *Item response theory for psychologists*. Lawrence Erlbaum Associates.

Jeon, M., Jin, I. H., Schweinberger, M., & Baugh, S. (2021). Mapping unobserved item-respondent interactions: A latent space item response model with interaction map. *Psychometrika, 86*(2), 378–403. https://doi.org/10.1007/s11336-021-09776-z

Kane, M. T. (2013). Validating the interpretations and uses of test scores. *Journal of Educational Measurement, 50*(1), 1–73. https://doi.org/10.1111/jedm.12000

Lord, F. M. (1980). *Applications of item response theory to practical testing problems*. Lawrence Erlbaum Associates.
