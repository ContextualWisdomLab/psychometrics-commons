# Bounded AI Governance

- Status: Normative product-governance baseline
- Date: 2026-08-09
- Generic orchestration owner: `ContextualWisdomLab/contextual-orchestrator`
- Bulk model-execution owner: `ContextualWisdomLab/pg-llm-batch`
- External egress security owner: `ContextualWisdomLab/EgressWeave`
- Hosted product policy owner: `ContextualWisdomLab/psychometrics-commons`

AI is an optional bounded product capability. It can improve explanation, reflection, authoring, translation review, item/rubric workflows, and research assistance, but it is not the psychometric numerical source of truth and is not allowed to silently change scientific or policy decisions owned by deterministic/versioned product contracts.

## 1. Non-negotiable separation

AI may **consume** a pinned scientific result. It may not invent or overwrite:

- Big Five/facet or other psychometric scores;
- calibration/item parameters;
- norms/percentiles;
- uncertainty intervals;
- DIF/invariance conclusions;
- linking/equating results;
- model relation/selection evidence;
- instrument publication evidence;
- research consent;
- data-rights completion;
- research-release approval;
- high-stakes diagnosis/employment/admission/credit/insurance/legal decisions.

When a task needs one of those outcomes, the owning deterministic/scientific workflow supplies it as input or rejects the task.

## 2. Allowed initial AI tasks

Subject to per-task data/provider policy, AI may support:

- wording a personality narrative from a pinned ScoreProfile and approved interpretation rules;
- generating optional reflection prompts/exercises from allowed result fields;
- researcher-facing literature or evidence summaries;
- item/rubric/blueprint candidate generation in upstream governed measurement workflows;
- translation candidate generation and comparison, never automatic publication;
- authoring/review assistance in the Measurement Workbench;
- multi-rater observations where an LLM is explicitly modeled as a fallible judge;
- data-card/release-document drafting from approved release metadata;
- participant-facing explanation of product concepts, limitations, and provenance where the answer is bounded to approved sources.

## 3. Deterministic fallback

Core assessment completion, numeric scoring, result retrieval, consent/data-rights functions, and basic result interpretation must not require an LLM.

A narrative-capable release therefore contains an approved deterministic localized fallback. If AI routing, provider access, output validation, or policy authorization fails, the product returns the deterministic interpretation rather than blocking or fabricating content.

## 4. AI task contract

Every task is identity-bearing/versioned and declares at least:

```text
ai_task_ref
task_type
purpose
input_schema_version
output_schema_version
allowed_data_classes
exact score/result/narrative rule references as applicable
model/routing policy reference
provider/deployment privacy class
residency policy
retention policy
maximum input/output size
timeout and bounded retry policy
tool/access-list policy
provenance requirements
```

The model is not allowed to broaden its own purpose, data projection, provider class, tool list, or retry budget.

## 5. Provider and routing policy

Psychometrics Commons does not implement a second generic model router. Real-time orchestration uses `contextual-orchestrator`; high-volume asynchronous model work may use `pg-llm-batch`. External network calls pass through EgressWeave or an equivalent reviewed exact-authority boundary for the deployment.

Provider/model selection respects the task's privacy/security class. A task may not downgrade from a private/local/zero-retention requirement to an arbitrary public provider merely because the preferred provider is slow, unavailable, or more expensive.

Where the owning `contextual-orchestrator` uses test-time compute allocation, role-specialized reasoning effort, or single-model versus multi-agent orchestration, Psychometrics Commons consumes the versioned route/result contract rather than reproducing the orchestration logic here.

## 6. Sensitive data and PII

Do not apply blanket PII masking that removes construct-relevant or operationally required context. Instead:

1. declare the exact purpose;
2. authorize the participant/tenant/research scope;
3. project only fields required for that task;
4. enforce the provider/deployment, residency, retention, and contractual policy;
5. preserve an audit/provenance record that does not itself leak the sensitive payload;
6. deny the task if no approved route can process that data class.

Sensitive free text receives the highest applicable classification derived from its source and purpose. It is not assumed safe merely because it has been pseudonymized.

## 7. Provider output is untrusted

Provider output crosses an untrusted-content boundary.

Validation rejects, as applicable:

- malformed or duplicate JSON keys;
- unknown required fields/semantics;
- invalid enum/reference values;
- non-finite numeric values;
- output exceeding resource bounds;
- evidence citations not present in the approved source set;
- provenance/model/result identifiers inconsistent with the request;
- tool instructions embedded in participant/item/source content;
- score/norm/scientific values not supplied by the pinned source-of-truth contract;
- clinical/high-stakes claims prohibited by product policy.

A validation failure does not partially apply the output.

## 8. Narrative contract

Personality Style and narrative are presentation artifacts, not latent-trait estimates.

The narrative input contains:

- pinned ScoreProfile/facet values permitted for presentation;
- uncertainty/limitations;
- exact narrative-rule version;
- approved interpretation units and prohibited claims;
- locale.

The narrative renderer may choose wording but cannot choose a hidden psychometric type, alter source scores, invent a diagnosis, or turn a probabilistic/continuous tendency into an immutable essence claim.

Near-boundary profiles may explicitly present adjacent/mixed styles according to the deterministic mapping rule rather than asking the LLM to decide a type.

## 9. LLM-as-a-Judge governance

When LLMs evaluate items, responses, translations, rubrics, RAG outputs, or other artifacts, their judgments are observations, not truth by definition.

The evaluation design records:

```text
judge model/provider/version
judge prompt/version
criterion/rubric version
input/evidence references
occasion/order/seed settings where relevant
verdict/score state
abstention/failure state
evidence/provenance
```

The measurement workflow considers evaluator severity, discrimination, criterion-specific bias, prompt/order effects, range restriction, model-family dependence, drift, design connectedness, and disagreement. High human/AI correlation alone does not establish accuracy or validity.

Benchmark criteria generated from candidate answers must use candidate-blind or cross-fitted designs appropriate to the evaluation goal; the same candidate cannot be used to discover a favorable criterion and then be scored by it as if independent.

## 10. Artificial-crowd and generated-content use

LLM-generated respondents/items/ratings can accelerate pilot calibration and validation but do not automatically replace human or real-user evidence.

Synthetic/Artificial Crowd evidence is labelled as such and is used for:

- candidate screening;
- initial difficulty/discrimination range estimation;
- perturbation/anchor behavior;
- recovery/selection stress tests;
- identifying unstable judges or items.

Operational publication still requires evidence appropriate to the intended real population/use. A model crowd cannot prove human construct validity by itself.

## 11. Reflection safety

Reflection prompts are optional and should encourage exploration rather than diagnosis or treatment.

The product does not infer self-compassion, perfectionism, emotion regulation, attachment, psychopathology, or other constructs from Big Five unless a separately validated model explicitly supports that use and the product scope permits it. Initially, reflection constructs are measured through their own approved instruments.

Participant input that suggests acute harm or clinical need is not silently transformed into a diagnosis. Any future safety escalation product must have a separately accepted scope, legal/clinical governance, localization, human-support and failure-mode architecture before release.

## 12. Monitoring and drift

Track, by exact task/routing/model/prompt version where applicable:

- schema validation/rejection rate;
- deterministic fallback rate;
- provider/model latency and failure class;
- output length/resource use;
- narrative rule-faithfulness failures;
- prohibited-claim/adversarial-test failures;
- judge agreement/severity/drift metrics;
- human override/adjudication rate where applicable;
- provider/privacy policy denial count;
- input/output distribution changes relevant to validated task use.

A provider/model upgrade creates new evidence. Old evaluation results are not automatically transferred to a changed model or prompt.

## 13. Security testing

Required tests evolve with implemented tasks and include:

- prompt injection in assessment/item/evidence content remains inert data;
- oversized/recursive/malformed provider output;
- duplicate JSON keys and non-finite values;
- invalid result/provenance reference injection;
- unapproved provider/host request denied by egress policy;
- cross-tenant task/result reference access denied;
- sensitive input omitted when task policy does not allow it;
- secrets absent from model payloads, client payloads, error bodies, and logs;
- AI outage/denial still permits deterministic core result flow;
- model result cannot mutate source numeric score/result snapshot.

## 14. Model-backed test credentials

Repository and organization automation that genuinely needs live model calls uses GitHub Secret `NVIDIA_NIM_API_KEY`, preferably through `contextual-orchestrator` when the integration is relevant. `COPILOT_GITHUB_TOKEN` is not used for these tests/development agents. Independent review-agent credential identity/scope is preserved.

Deterministic CI gates remain separate from bounded live-model conformance tests so provider outages or rate limits do not masquerade as product-correctness evidence.

## 15. Release evidence

An AI-backed product capability is releaseable only when the exact integrated version has:

- task/schema/version contract;
- permitted data/provider/residency/retention policy;
- deterministic fallback where required;
- adversarial and malformed-output tests;
- provenance/audit behavior;
- quality/faithfulness evaluation appropriate to the task;
- privacy/security review;
- cost/resource bounds;
- model/prompt/routing version evidence;
- rollback/disable policy that does not alter deterministic scientific semantics.

## 16. Ownership summary

Psychometrics Commons owns **whether and for what purpose** AI is allowed in this product and how AI output may affect product state.

It does not own generic routing, bulk model execution, provider egress kernels, or psychometric evaluation kernels. Those remain reusable bounded contexts consumed through explicit contracts.
