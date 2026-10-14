# ADR-0019: Scientific publication and score-interpretation evidence gates

- Status: Accepted
- Date: 2026-08-09
- Deciders: ContextualWisdomLab Psychometrics Commons maintainers
- Scope: instrument publication, scoring/calibration/norm references, locale/intended-use evidence, scoreability, DIF/invariance, suspension/retirement triggers
- Supersedes: none
- Superseded by: none
- Current/as-built status: protected main implements an immutable instrument-release manifest and publication lifecycle, but publication does not yet enforce the full scientific/content/rights evidence manifest defined here
- Target status: `Review -> Published` is impossible unless the exact version bundle satisfies its versioned, intended-use-specific evidence policy; blocking evidence can suspend/retire new-session eligibility without mutating historical results
- Migration status: pre-gate releases, if any exist when persistence lands, require explicit review/classification; synthetic passing evidence or silent grandfathering is forbidden

## Context

Psychometrics Commons owns instrument publication and serving, while `fast-mlsirm` owns reusable measurement contracts and psychometric numerical computation. A product operator therefore needs a deterministic boundary between “a technically valid manifest exists” and “this exact instrument/scoring/norm/locale combination is scientifically publishable for the claimed use.”

Without an explicit evidence gate, a product could publish a form because its fields are syntactically complete while calibration is weak, scoreability is unsupported, rights/translation evidence is missing, cross-group comparisons are not invariant, or a model fits in-sample without recovery/generalization evidence. Operator discretion must not silently override a mandatory scientific blocker.

## Decision

1. Transitioning an instrument release to `Published` requires an **approved scientific publication evidence record** appropriate to the instrument's exact intended use, locale, population, administration mode, scoring/calibration/norm bundle, and claimed comparisons.
2. Psychometrics Commons stores/references the evidence and enforces publication state. It does not recompute psychometric evidence owned by `fast-mlsirm`.
3. Mandatory evidence requirements are versioned by an `evidence_policy_ref` or equivalent policy artifact. Different intended uses may require different evidence, but a less demanding use may not silently inherit a stronger comparison/decision claim.
4. A mandatory failed/unknown gate blocks publication. A product operator cannot convert it to passing by editing a label, narrative, or free-text note. Any policy exception must be explicit, authorized, risk-assessed, versioned, and cannot contradict non-waivable product/safety constraints.
5. Published releases are monitored. Material evidence invalidation may cause `Suspended`/`Retired` state for new sessions without mutating historical results or release bytes.
6. Publication evidence is exact-version bound. Evidence from a predecessor item set, scoring model, norm, locale, or calibration does not automatically transfer.

## Ownership and boundaries

| Responsibility | Owner | Interface | Forbidden coupling |
|---|---|---|---|
| Numerical calibration/recovery/model comparison/DIF/scoreability | fast-mlsirm | immutable evidence artifacts/contracts | product reimplementing numerical kernels |
| Instrument content/translation/right review evidence | psychometrics-commons workflow + accountable reviewer/owner | versioned evidence references | treating LLM/generated content as automatically approved |
| Instrument publication lifecycle | psychometrics-commons | publication commands/state | downstream client directly changing release state |
| Research catalog/release | semantic-data-portal + product release workflow | separate research release contract | treating product publication as research-data approval |
| Identity and reviewer authentication | Keyverse plus product authorization | identity assertion + product role/resource policy | identity administration implicitly granting scientific approval |

## Contract details

A publication evidence record binds at minimum, as applicable:

```text
publication_evidence_ref
evidence_policy_ref
instrument_version_ref
item_version_refs / item-set digest
locale
intended_use_ref
population/administration context
assessment_spec_ref
scoring_version_ref
calibration_reference
norm_version_ref optional
measurement_model_ref
recovery_evidence_refs
fit/model_selection_evidence_refs
scoreability_evidence_refs
DIF/invariance/fairness_evidence_refs
linking/equating_evidence_refs where applicable
translation/content_review_refs where applicable
rights/license_refs
known_limitations_ref
review/approval_refs
created_at / evidence time window
```

The physical representation may use a manifest with digests rather than one row. All referenced artifacts are immutable/versioned or themselves provenance-bound. The publication command binds the exact `evidence_policy_ref` and publication-evidence identity/digest; changing required evidence after publication creates new review evidence and, where release semantics change, a new/superseding release rather than mutating historical justification invisibly.

## Minimum evidence classes

The exact required classes depend on the use, but publication policy considers:

### Content and rights
- construct/instrument rationale;
- item/source rights and permitted commercial/research use;
- locale/translation/cultural-adaptation review where applicable;
- item/content review and intended population/use.

### Calibration and recovery
- sample/data-generation context;
- model convergence/failure diagnostics;
- bias/RMSE/coverage or other appropriate recovery evidence when simulation/known truth exists;
- score/parameter uncertainty;
- numerical/backend parity where multiple backends are served.

### Structural model and scoreability
- factor retention versus structural-model decision evidence;
- relation-safe model comparison, held-out evidence and residual/local-dependence diagnostics where applicable;
- score reliability/determinacy/scoreability appropriate to the model;
- no reporting of specific/general score unsupported by scoreability evidence.

### Comparison/fairness
- DIF/invariance evidence for claimed population/locale/time/mode comparisons;
- linking/equating/anchor stability if scores/forms are put on a common scale;
- norm population/collection/effective context if normative interpretation is exposed.

### Complex design
- testlet/local dependence;
- multilevel/cross-classified/multiple-membership structure;
- rater/facet behavior;
- time/longitudinal/drift assumptions, when relevant to the intended interpretation.

## Intended-use levels

A product can support a narrower use without supporting every possible use. Examples:

- within-locale self-reflection;
- cross-locale descriptive comparison;
- longitudinal change interpretation;
- adaptive testing;
- institutional research administration.

The publication record identifies what is supported. It must not imply clinical/high-stakes use when that use is out of scope or unvalidated.

## Data and persistence impact

The logical instrument publication model must store/reference:

- `evidence_policy_ref`;
- `publication_evidence_ref` or manifest digest;
- approval/review evidence necessary for the publication command;
- exact intended-use/limitations refs already pinned by the release manifest.

Published result snapshots remain tied to the exact release/scoring/norm evidence used at administration time. Later evidence policy changes do not retroactively relabel historical results as validated under the new policy.

When physical persistence is implemented, publication evidence may be stored as normalized records or an immutable manifest plus references, but the database must enforce exact release/evidence identity and prevent an operator from changing a published release's bound evidence in place.

## Invariants

1. `Published` requires all mandatory policy gates known and passing for the exact version bundle.
2. Unknown/not-run evidence is not passing.
3. Rights/locale/scientific evidence references cannot point to a different item/version bundle without explicit compatible linking evidence.
4. A narrative/AI result cannot satisfy a missing measurement gate.
5. The release's claimed intended use is a subset of uses supported by its evidence record.
6. A suspended/retired release cannot start new sessions; historical results remain readable with their original limitations/provenance.
7. Evidence review/approval is auditable and distinct from Keyverse identity administration.
8. A critical scientific/privacy/security finding can block/suspend publication even when aggregate quality metrics are high.
9. A policy version or evidence digest mismatch is a publication conflict, not an implicit policy upgrade.
10. Evidence from a different locale, calibration, norm, item set, model, or response mode is non-transferable unless the policy explicitly references validated linking/compatibility evidence.

## Failure and degraded modes

- Missing/unresolvable evidence artifact: publication fails closed.
- fast-mlsirm evidence path unavailable: publication/republication waits; already published historical results remain intact unless a separate incident policy suspends new sessions.
- Evidence becomes invalid due to drift/new finding: enter review and, when policy requires, suspend new sessions; do not mutate past results.
- Norm becomes obsolete: new normative interpretation requires a new norm/result version; do not silently swap norms for historical results.
- Locale-specific evidence fails: block only the unsupported locale/version or comparison claim where scientifically appropriate; do not automatically disable unrelated validated forms.
- Rights/license status becomes invalid for new administrations: suspend/retire affected new-session eligibility while preserving lawful historical provenance and applying the governing content/legal process.
- Evidence conflict or unverifiable approval: publication remains blocked and safe operator diagnostics identify the missing evidence class/reference without exposing sensitive calibration data.

## Security, privacy, and tenancy

Scientific evidence may contain sensitive research/sample information. Product publication paths use safe evidence references and role-scoped views. Participant-level calibration datasets are not exposed to public clients merely because an evidence record is public/internal.

Tenant-specific instruments/evidence remain tenant-scoped. A tenant cannot approve another tenant's evidence or reuse private calibration data by reference without explicit authorization. Approval identities are authenticated but product-scoped authorization determines who may approve scientific/content/rights gates.

Evidence manifests and operator error surfaces expose safe identifiers/digests/statuses rather than participant-level data, raw restricted evidence, provider secrets, or unbounded reviewer notes.

## Deployment and operations impact

Publication is an operational gate with observable failure reasons safe for operators. Evidence resolution/validation health is separate from participant result-read availability.

Monitoring should detect published releases whose referenced evidence artifact becomes unavailable, compromised, revoked, or superseded by a blocking finding. The Hosted and Enterprise profiles require runbooks for publication suspension/retirement and evidence-registry outage; the Community profile may use a local evidence registry but must enforce the same publication semantics.

## Migration and rollback

Existing pre-gate releases cannot be automatically marked compliant. A migration must classify them as needing evidence review, grandfathered only under an explicit time-bounded policy approved before use, suspended, or retired. Synthetic “passed” evidence is forbidden.

Rolling back publication-policy code must not make a release eligible if its current mandatory evidence is unknown/failed under the stored policy version. Prefer roll-forward/compatibility adapters for persisted policy semantics.

A new stricter evidence policy applies to new/reviewed publication decisions according to its explicit compatibility/effective policy; historical results keep the policy/evidence provenance under which they were administered.

## Architecture-view impact

- `ARCHITECTURE.md`: instrument publication remains product-owned but scientifically gated.
- `docs/architecture/C4.md`: ownership unchanged.
- `docs/architecture/UML.md`: publication `review -> published` implies evidence gate.
- `docs/architecture/ERD.md`: physical model must carry evidence policy/manifest references when persistence is implemented.
- `docs/architecture/SECURITY_AND_DATA.md`: evidence datasets remain classified and purpose-bound.
- `docs/architecture/DEPLOYMENT_AND_OPERATIONS.md`: publication-evidence health and suspension are operator concerns.
- `docs/MEASUREMENT_GOVERNANCE.md`: detailed evidence requirements remain authoritative policy guidance.
- `docs/TRACEABILITY.md`: publication requirement maps to this ADR and remains partial until code enforces it.
- `docs/RISK_REGISTER.md`: weak/unvalidated consumer instrument is a material open risk until evidence exists.

## Validation and release evidence

- publication state-machine tests with passed/failed/unknown gate matrices;
- exact-version/digest mismatch tests;
- intended-use subset tests;
- rights/translation evidence required for configured consumer forms;
- no operator/narrative bypass of mandatory scientific gate;
- suspension/retirement behavior when evidence is invalidated;
- tenant/role negative authorization tests;
- migration tests for legacy release evidence state;
- product acceptance proving result views expose relevant limitations/provenance;
- consumer instrument fixtures proving the exact published form's evidence policy is complete before new-session eligibility.

## Alternatives considered

### Treat valid instrument manifest as publishable by default

Rejected. Schema validity is not scientific validity, rights clearance, or intended-use evidence.

### Put publication approval entirely in fast-mlsirm

Rejected. fast-mlsirm owns reusable science/contracts, not hosted product publication, identity, rights, locale, consent, or product workflow.

### Let an admin waive any failed evidence gate

Rejected as a universal policy. It defeats fail-closed scientific governance and can create misleading user claims. Explicit limited policy exceptions, if ever allowed, require a separate reviewed governance decision and cannot override out-of-scope/high-stakes constraints.

### Use a single permanent global evidence checklist

Rejected. Evidence requirements depend on intended use, locale, response model, comparisons, adaptive use, and scientific evolution. The policy itself must be versioned.

## Consequences

Positive:

- publication status becomes scientifically meaningful rather than syntactic;
- evidence is reusable/auditable without moving numerical kernels into the product;
- unsupported comparison/use claims fail closed;
- instrument suspension can respond to new evidence without erasing historical provenance.

Costs:

- publication requires evidence registry/workflow/role UX;
- upstream fast-mlsirm artifact/version compatibility must be managed;
- product operators need clear explanations of why a gate blocks publication.

## Follow-up work

- define the first `publication_evidence_policy` for IPIP Big Five within-locale self-reflection;
- decide and document exact rights/content source for the initial English/Korean form;
- define translation/content-review workflow and evidence refs;
- integrate fast-mlsirm calibration/recovery/scoreability artifacts;
- extend the instrument-release domain/persistence to bind evidence policy and approved evidence manifest before `Publish` succeeds;
- add operator UX for missing/failed evidence without allowing bypass.

## Traceability

- Product requirements: `docs/PRD.md` consumer MVP, Measurement Workbench, measurement acceptance and release policy.
- Technical requirements: `docs/TRD.md` instrument publication, scoring, multilingual, security/privacy and release gates.
- Scientific policy: `docs/MEASUREMENT_GOVERNANCE.md`.
- Current source baseline: `src/instrument.rs` implements immutable release/lifecycle mechanics but not this full evidence gate; `docs/TRACEABILITY.md` must classify that distinction explicitly.
- Architecture: `ARCHITECTURE.md`, `docs/architecture/UML.md`, `docs/architecture/ERD.md`, `docs/architecture/SECURITY_AND_DATA.md`.
- Delivery/risk: `docs/ROADMAP.md`, `docs/RISK_REGISTER.md`, `docs/DOCUMENTATION_ASSESSMENT.md`.

## Reversal conditions

The evidence classes/policy language will evolve with instruments and scientific standards. The principle that product publication requires exact-version, intended-use-appropriate, auditable scientific/content/right evidence and cannot be satisfied by a narrative/admin assertion remains unless superseded by a stronger validated governance model.

## Standards basis

Publication and score-interpretation gates exist because a technically valid manifest is not scientific validity. The *Standards for Educational and Psychological Testing* require validity, reliability/precision, fairness, and score-reporting evidence appropriate to the claimed use (American Educational Research Association [AERA], American Psychological Association [APA], & National Council on Measurement in Education [NCME], 2014). Kane (2013) is the interpretation/use argument: an operator label or narrative cannot convert unknown or failed evidence into a supported score use.

Recovery, DIF/invariance, linking, and scoreability evidence are item-response-theory and measurement methods (Embretson & Reise, 2000; Lord, 1980). Numerical evidence is produced by `fast-mlsirm`. When a release uses the upstream multilevel latent-space item-response model, that engine is Jeon et al. (2021) as consumed through `fast-mlsirm`; this product does not reimplement it.

## References

American Educational Research Association, American Psychological Association, & National Council on Measurement in Education. (2014). *Standards for educational and psychological testing*. American Educational Research Association. https://www.testingstandards.net/

Embretson, S. E., & Reise, S. P. (2000). *Item response theory for psychologists*. Lawrence Erlbaum Associates.

Jeon, M., Jin, I. H., Schweinberger, M., & Baugh, S. (2021). Mapping unobserved item-respondent interactions: A latent space item response model with interaction map. *Psychometrika, 86*(2), 378–403. https://doi.org/10.1007/s11336-021-09776-z

Kane, M. T. (2013). Validating the interpretations and uses of test scores. *Journal of Educational Measurement, 50*(1), 1–73. https://doi.org/10.1111/jedm.12000

Lord, F. M. (1980). *Applications of item response theory to practical testing problems*. Lawrence Erlbaum Associates.
