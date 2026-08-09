# ADR-0013: Multilingual accessibility and measurement invariance

- Status: Accepted
- Date: 2026-08-09
- Scope: locale versions, translation, DIF/invariance, result narratives, accessibility

## Context

The initial product targets Korean and English and is expected to expand. Treating translation as a UI-string concern can alter item meaning, response processes, factor structure, norms, and narrative interpretation. Accessibility failures can also change the construct being measured by adding irrelevant barriers.

## Decision

A translated assessment is a distinct `instrument_version`, linked to but not identical with the source version. Locale-specific item text, instructions, examples, response labels, timing assumptions, scoring applicability, narrative rules, and validation evidence are versioned together.

Translation publication requires documented linguistic review, construct review, and empirical measurement evidence appropriate to the intended score use. Machine translation may create candidates but cannot directly publish an operational item.

The product targets WCAG 2.2 AA for supported user-facing clients. Accessibility accommodations and presentation modes are recorded when relevant to score interpretation, but are not used to penalize participants.

## Measurement invariance and DIF

Before cross-language score comparison or a shared norm is enabled, the release must evaluate:

- configural/structural comparability appropriate to the model;
- item/threshold DIF by language and relevant groups;
- linking-anchor stability;
- score and uncertainty recovery;
- differential missingness and accessibility-mode effects;
- narrative equivalence and cultural appropriateness.

Failure does not necessarily block offering the locale for within-locale self-reflection, but it blocks unsupported cross-locale comparisons and shared norms. The product must surface that limitation.

## Locale contract

Clients send a BCP 47 locale. The server resolves an exact published instrument locale and never silently falls back to another language for assessment items. UI chrome may fall back according to client policy, but assessment content requires explicit participant confirmation before any fallback.

## Accessibility invariants

1. Core assessment is keyboard operable and screen-reader compatible.
2. Focus order, labels, errors, timing controls, and contrast are tested.
3. Time limits, if any, have an accommodation policy and are part of the instrument version.
4. Visual result charts have equivalent text/tabular descriptions.
5. Accessibility mode does not change item order or scoring unless the instrument explicitly versions that form.
6. Automated narrative never uses stigmatizing or diagnostic language outside an approved validated use.

## Failure modes

Missing locale content blocks session creation for that locale rather than mixing versions. Translation-review or DIF failure leaves the locale in draft/pilot status. Accessibility regression blocks release of the affected client.

## Validation

- automated and manual accessibility checks;
- locale contract and no-silent-fallback tests;
- translation provenance and reviewer approval;
- language DIF/invariance and linking tests;
- result-narrative equivalence review;
- assistive-technology end-to-end testing for reference clients.

## Alternatives rejected

- **Translate strings in the client only:** loses scientific versioning and server-side provenance.
- **Use one global norm immediately:** unsupported before invariance/linking evidence.
- **Allow silent English fallback:** changes the assessment without informed choice.

## Reversal conditions

Specific locales or comparison claims may be expanded or restricted as evidence changes. The requirement to version translated instruments and validate intended comparisons remains.
