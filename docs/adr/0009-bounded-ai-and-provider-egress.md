# ADR-0009: Bounded AI and controlled provider egress

- Status: Accepted
- Date: 2026-08-09
- Scope: result narratives, reflection prompts, translation review, item-generation assistance, external model providers

## Context

AI can improve explanation, authoring, translation review, and reflective experiences, but model outputs are non-deterministic, provider-dependent, and may expose sensitive data. Treating AI as the score source or silently stripping all PII would either invalidate measurement or destroy useful context.

## Decision

Deterministic measurement and numeric scoring remain outside the LLM path. AI is a bounded, optional capability:

- real-time orchestration uses `contextual-orchestrator`;
- high-volume asynchronous work may use `pg-llm-batch`;
- outbound provider access is constrained through EgressWeave or an equivalent exact-authority egress policy;
- prompts, models, routing decisions, tool calls, and outputs are versioned and auditable;
- every AI output is validated against a closed schema before use.

Approved AI use includes narrative wording, reflection-question candidates, translation/content-review candidates, literature summaries, item candidates, and multi-rater observations. AI cannot determine Big Five scores, self-compassion scores, norms, uncertainty, DIF conclusions, release approval, diagnosis, or high-stakes decisions.

## Data-processing policy

PII is not blindly masked when it is required for an authorized assessment or reflection task. Instead each AI task declares:

- allowed data classes and purpose;
- deployment/provider class: local, private endpoint, zero-retention contracted provider, or prohibited;
- residency and retention requirements;
- fields included by explicit policy;
- output retention and audit policy.

A task without an applicable provider policy fails closed. Provider routing cannot downgrade privacy class to satisfy availability or price.

## Narrative safety

The narrative engine receives a versioned numeric/qualitative `ScoreProfile` and approved interpretation rules. It cannot modify source scores. Output must cite the rule/provenance used, avoid clinical or immutable-personality claims, state limitations, and fall back to deterministic localized templates when AI is unavailable or validation fails.

## Provider-output boundary

Provider output is untrusted. The system rejects duplicate JSON keys, non-finite numbers, unknown fields, oversized content, invalid references, provenance mismatch, prompt-injection-derived tool instructions, and evidence spans absent from approved source material.

## Invariants

1. LLM outage does not block assessment completion, numeric scoring, or basic result access.
2. No provider secret reaches clients or logs.
3. No model alias such as `latest` is sufficient provenance.
4. AI-generated content is distinguishable from measured score and approved static content.
5. Human and AI judges are modeled as fallible raters, not truth.
6. Critical policy gates cannot be offset by a high aggregate AI-generated score.

## Validation

- deterministic fallback tests;
- schema and adversarial-output tests;
- provider-family and prompt-version drift evaluation;
- privacy-class routing tests;
- narrative faithfulness to ScoreProfile and rule tests;
- no-score-mutation property tests.

## Alternatives rejected

- **LLM computes final scores:** uncalibrated and irreproducible.
- **Direct SDK calls from each service:** inconsistent egress and audit controls.
- **Blanket PII masking:** may remove construct-relevant context and impair operations.
- **AI required for all results:** creates avoidable availability and vendor lock-in.

## Reversal conditions

A deterministic certified model may move into the measurement core only after it is versioned, reproducible, independently calibrated, and passes the same scientific validation as other scoring engines. It then ceases to be treated as an unconstrained LLM narrative path.
