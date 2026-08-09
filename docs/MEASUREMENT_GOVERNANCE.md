# Psychometric Measurement Governance

- Status: Normative product-governance baseline
- Date: 2026-08-09
- Numerical/source-of-truth owner: `ContextualWisdomLab/fast-mlsirm`
- Product publication/workflow owner: `ContextualWisdomLab/psychometrics-commons`

Psychometrics Commons does not implement psychometric numerical kernels. This document defines **what evidence the hosted product requires before it publishes, scores, compares, or interprets an instrument/model** and how that evidence is referenced. Numerical algorithms, recovery engines, model fitting, diagnostics, item-bank calibration, and reusable measurement contracts remain in `fast-mlsirm`.

## 1. Core principle

A score is publishable only when the intended interpretation and use have evidence appropriate to the measurement model, population, language, administration mode, and decision consequence.

High correlation alone is not estimation accuracy, measurement agreement, fairness, or validity. The product therefore treats correlation as a supplementary relationship statistic rather than the primary accuracy gate.

For simulation/recovery settings, the minimum evidence set is chosen from:

```text
bias
MAE / RMSE
standard-error bias
interval coverage
convergence / failure rate
parameter / score recovery
response-function / information recovery
backend parity
model-selection recovery
```

When latent scales have location/scale, sign, rotation, or latent-space non-identifiability, estimates are aligned to the declared identification/linking convention **before** component-wise recovery error is interpreted.

## 2. Measurement contract and provenance

Every operational scoring path pins exact references sufficient to reproduce its interpretation:

```text
instrument_version_ref
item_version_refs / response_snapshot_ref
assessment_spec_ref
scoring_version_ref
calibration_reference
norm_version_ref optional
engine_artifact_digest
output_schema_version
```

A published result additionally pins narrative/consent provenance in the product layer.

Mutable aliases such as `latest` may be used only for discovery. They are resolved to immutable references before an assessment session or scoring operation becomes identity-bearing state.

## 3. Construct and instrument lifecycle

The product workflow separates:

```text
construct definition
-> assessment/measurement specification
-> item/version bank
-> pilot
-> calibration
-> dimensional/model evaluation
-> DIF/invariance/fairness evaluation
-> linking/equating/norming where applicable
-> scoreability/interpretability decision
-> publication
-> monitoring/drift
-> suspension/retirement/supersession
```

An instrument can be structurally valid as content but not yet operationally publishable because calibration, rights, translation, intended-use, or fairness evidence is incomplete.

## 4. Factor retention versus structural model selection

Factor retention and structural model choice are distinct decisions.

### Factor retention

Candidate primary dimensionalities may be informed by methods appropriate to the response model/data, including parallel analysis, MAP, exploratory MIRT evidence, residual structure, information criteria, bootstrap likelihood comparisons, and held-out prediction.

No single factor-retention heuristic is treated as universally correct across continuous, binary, ordinal, highly correlated, or locally dependent conditions.

### Structural models

The product recognizes that the following answer different measurement questions rather than forming a simple complexity ladder:

- unidimensional;
- correlated multifactor/MIRT;
- higher-order/second-order;
- bifactor;
- testlet/local-dependence models;
- two-tier models;
- multifaceted/rater models;
- latent-space residual-interaction models.

`multifactor` and `multifaceted` are not substitutes: the first concerns multiple substantive latent traits; the second concerns systematic task/rater/occasion facets.

## 5. Model-relation-safe comparison

A product workflow must not choose a statistical test from model names alone. The actual parameterization/constraints determine whether a comparison is:

```text
regular_nested
boundary_nested
nonlinear_constraint_nested
strictly_non_nested
overlapping
indistinguishable
unknown
```

Governance rules:

- regular nested → appropriate LR/robust LR procedure;
- boundary/singular nested → boundary-aware or parametric-bootstrap LR rather than naive chi-square;
- strictly non-nested/overlapping → formal distinguishability before Vuong-style selection;
- unknown relation → fail closed rather than declare a winner;
- repeated/testlet/judge data → use an independence unit/cluster structure appropriate to the design rather than treating every response cell as iid.

Final model choice also requires held-out/cluster-aware prediction, residual-dependence diagnostics, parameter/structure recovery, intended-score interpretability, and fairness/invariance evidence as applicable.

## 6. Bifactor and general-score governance

A bifactor model fitting well is not sufficient evidence to report both a general score and specific subscale scores.

Where bifactor interpretation is proposed, require evidence appropriate to the model and scale, which may include:

- all intended items having the declared general loading pattern;
- ECV and item-level ECV;
- PUC where its structural assumptions are actually satisfied;
- omega hierarchical for the general score;
- omega hierarchical subscale / specific-score reliability where applicable;
- factor determinacy;
- construct replicability `H`;
- stable loading recovery;
- external/incremental validity for scores intended to be interpreted separately.

A descriptive latent-response reliability quantity must not be mislabeled as categorical observed-score reliability.

If specific factors are unstable or add no reliable/valid information beyond the general factor, the product does not expose specific-score claims merely because the model contains those parameters.

## 7. Testlet and local dependence

Items/probes sharing a passage, scenario, question/context/answer instance, rater occasion, or other stimulus may violate local independence.

The product requires residual/local-dependence assessment appropriate to the measurement design. Known common-stimulus dependence is modeled as testlet/nuisance structure when evidence supports it rather than being misinterpreted as another substantive trait.

A latent-space interaction is considered only **after** substantive dimensions, testlets/local dependence, and rater/occasion effects that are supported by design have been considered. Latent space is a residual interaction layer, not a replacement for omitted substantive factors.

## 8. Multilevel, cross-classified, and multiple-membership structure

The product must not reduce clustered or overlapping contextual structure to individual-level observations merely for convenience.

Where data arise from participants/items/raters nested or cross-classified within organizations, teams, projects, contexts, schools, customers, languages, or other units, the measurement specification explicitly evaluates whether multilevel, cross-classified, or multiple-membership modeling is required.

Multiple membership is represented explicitly when a response/observation belongs to more than one relevant context. A `primary_group` shortcut cannot silently replace the scientifically intended structure.

This is an explicit guard against atomistic interpretation of higher-level effects.

## 9. Time and longitudinal measurement

Time is part of the measurement design when observations repeat or item/rater/system behavior drifts.

Psychometrics Commons preserves event-time/version context and delegates temporal/event/multilevel modeling to TEPP where that is the owning analytical boundary. `fast-mlsirm` may own reusable psychometric drift/rater/item measurement methods; TEPP owns broader temporal/event analytical artifacts according to the accepted boundaries.

Relevant evidence may include:

- longitudinal invariance;
- item/rater drift;
- occasion effects;
- within-person versus between-person separation;
- delayed availability and event ordering;
- time-varying context/multiple membership.

A cross-sectional score claim is not silently generalized to change-over-time interpretation.

## 10. DIF, invariance, language, and fairness

DIF/invariance evaluation follows the intended comparison.

Potential grouping/context variables include, where scientifically and ethically justified:

- language/locale;
- administration mode/accessibility accommodation;
- population/region;
- domain/context;
- item/testlet form;
- rater/judge family or version;
- relevant demographic group for fairness evaluation.

A Korean and English instrument may both be published for within-locale reflection without claiming cross-locale comparability. Shared norms or direct cross-locale comparison require linking/anchor stability, DIF/invariance, score/uncertainty recovery, and content/translation evidence appropriate to the claim.

Fairness is not established by equal aggregate mean scores alone.

## 11. Linking, equating, and norms

A norm or linked score is an immutable versioned scientific artifact.

Operational publication requires evidence for:

- anchor identity and stability;
- linking/equating method and assumptions;
- population definition;
- effective dates/data collection context;
- uncertainty;
- subgroup/locale applicability;
- drift monitoring and retirement criteria.

Updating a norm does not rewrite a historical result. A new norm/scoring version produces a superseding result if rescoring is deliberately requested and allowed.

## 12. CAT and ATA

Computerized adaptive testing and automated test assembly are optional serving capabilities, not default assumptions.

Before enabling an adaptive/assembled form, require:

- sufficiently calibrated item bank;
- item information/reliability evidence in the intended trait region;
- content/construct constraints;
- exposure controls;
- item-security policy;
- stopping-rule accuracy/reliability evidence;
- simulation showing score/selection behavior and edge cases;
- linking/version evidence when forms evolve.

An adaptive algorithm cannot select an unapproved/pilot/quarantined item merely because it is statistically informative.

## 13. Rotation governance

For exploratory factor solutions, no rotation criterion is claimed to be universally optimal.

Rotation selection is evidence- and purpose-dependent. Where multiple criteria are compared, criterion objective values with different meanings/scales are not directly treated as a universal leaderboard.

Evidence may include:

- deterministic multi-start/basin stability;
- bootstrap loading congruence after sign/permutation alignment;
- split-sample reproducibility;
- target agreement when theory supplies a target;
- degeneracy/factor-correlation checks;
- true-loading recovery in representative simulations;
- interpretation/complexity policy appropriate to the use.

A finite multi-start search is described as the best observed solution, not a proof of global optimality.

## 14. Human and AI judges as raters

Human and LLM judges are fallible raters, not ground truth by definition.

Where ratings are used in scoring/calibration/evaluation, the design considers as applicable:

- rater/judge severity;
- rater/judge discrimination;
- criterion-specific bias;
- threshold/range restriction;
- position/order/prompt effects;
- version/time drift;
- design connectedness;
- disagreement caused by legitimate stakeholder perspective versus noise.

A single human raw score is not automatically treated as a true score; multiple-rater/measurement or explicit gold-standard justification is required for claims that depend on truth.

## 15. Automated scoring governance

Automated scoring is a measurement system, not merely a predictor.

An automated-scoring path must identify:

```text
assessment/rubric contract
response/prompt version
rater/scoring-engine identity + version
criterion observations and evidence
calibration model/version
validation/fairness gates
adjudication/human-review policy
monitoring/drift policy
reporting contract
```

People and AI scorers can be represented through the same observation contracts while retaining their identity/type and calibration evidence.

Human review routing is driven by explicit policy such as disagreement, uncertainty, critical criterion failure, insufficient evidence, out-of-distribution input, or adjudication requirements—not by an unexplained model confidence number alone.

## 16. Rubric, blueprint, and governed item-bank lifecycle

Rubric-based evaluation treats the rubric as a versioned measurement specification rather than mutable prompt prose.

The reusable upstream lifecycle is:

```text
RubricSpecification
-> Blueprint / generation contract
-> candidate item generation
-> structural + semantic/evidence screening
-> artificial/human crowd pilot
-> Rust-backed psychometric calibration
-> governed item bank
-> adaptive assembly where appropriate
-> DIF/drift/exposure monitoring
-> rubric/item revision through new immutable versions
```

For reference-free/RAG/LLM evaluation domains, candidate-blind evidence-grounded criterion generation is the default benchmark design. Candidate-aware criterion discovery requires separation/cross-fitting so the responses used to discover criteria are not scored by the same adaptively discovered rubric as if it were independent.

Atomic verifiable criteria are preferred where they improve construct clarity and rater agreement; broad holistic scores may be derived/reported only when their measurement relationship is governed and validated.

This lifecycle is owned as reusable measurement/item-bank functionality by `fast-mlsirm`; Psychometrics Commons Workbench consumes it through versioned contracts and owns publication/authorization/workflow state.

## 17. Reference-free RAG / enterprise issue evaluation boundary

Psychometrics Commons is not initially shipping a RAG benchmark or enterprise issue-ranking product, but its Workbench must remain compatible with the reusable measurement contracts developed for those domains.

The governing principles are preserved:

- “reference-free” does not mean “truth-free”;
- groundedness, world correctness, and completeness are different constructs;
- LLM judge output is an error-prone observation subject to rater calibration;
- question/probe difficulty and discrimination matter;
- testlet/local dependence from shared question/context/answer must be modeled when relevant;
- absolute ratings and pairwise comparisons may share a latent trait but provide different information;
- business priority/utility is a decision layer and must not be conflated with psychometric discrimination.

Domain-specific adapters remain outside the hosted core unless separately accepted as product scope.

## 18. Publication decision record

Every published instrument/scoring/norm combination must have a durable evidence record that answers:

1. What construct and use are claimed?
2. Which population/locale/administration mode is supported?
3. Which measurement model and identification are used?
4. What recovery/fit/reliability/scoreability evidence supports the score?
5. What DIF/invariance/linking/norm evidence supports comparisons?
6. What missingness/testlet/multilevel/time/facet structures were considered?
7. What known limitations remain?
8. Which exact fast-mlsirm artifact/version produced the evidence?
9. Which evidence would trigger suspension, recalibration, or retirement?

The product operator cannot override a failed mandatory scientific gate by editing a narrative or publication label.

## 19. Monitoring and drift

Operational monitoring separates:

- software/runtime drift;
- item parameter drift;
- rater/judge drift;
- population/norm drift;
- language/translation drift;
- response-pattern/local-dependence change;
- scoring-model/calibration version change.

A drift alert is evidence for investigation, not automatic proof that a construct changed. Remediation can include recalibration, linking, subgroup-specific parameters, suspension, new version publication, or retirement according to the governing evidence.

## 20. Key references

Bland, J. M., & Altman, D. G. (1986). Statistical methods for assessing agreement between two methods of clinical measurement. *The Lancet, 327*(8476), 307–310. https://doi.org/10.1016/S0140-6736(86)90837-8

Cai, L. (2010). A two-tier full-information item factor analysis model with applications. *Psychometrika, 75*, 581–612.

Kane, M. T. (2013). Validating the interpretations and uses of test scores. *Journal of Educational Measurement, 50*(1), 1–73. https://doi.org/10.1111/jedm.12000

Rijmen, F. (2010). Formal relations and an empirical comparison among the bi-factor, the testlet, and a second-order multidimensional IRT model. *Journal of Educational Measurement, 47*, 361–372.

Rodriguez, A., Reise, S. P., & Haviland, M. G. (2016). Evaluating bifactor models: Calculating and interpreting statistical indices. *Psychological Methods, 21*(2), 137–150.

Schneider, L., Chalmers, R. P., Debelak, R., & Merkle, E. C. (2020). Model selection of nested and non-nested item response models using Vuong tests. *Multivariate Behavioral Research, 55*(5), 664–684.

Svetina, D., Valdivia, A., Underhill, S., Dai, S., & Wang, X. (2017). Parameter recovery in multidimensional item response theory models under complexity and nonnormality. *Applied Psychological Measurement, 41*(7), 530–544. https://doi.org/10.1177/0146621617707507
