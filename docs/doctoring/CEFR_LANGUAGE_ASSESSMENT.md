# CEFR language-assessment doctoring

- Status: Evidence basis for the review-only consumer boundary
- Date: 2026-08-28
- Product owner: `ContextualWisdomLab/psychometrics-commons`
- External contract owner: `ContextualWisdomLab/learning-interoperability-contracts`

## Use in this product

The CEFR Companion Volume is used as the framework reference for separating
language-activity domains and for distinguishing a construct/profile reference
from an empirical linking or certification claim. Psychometrics Commons stores
only opaque references and immutable evidence identities. It does not copy
official descriptor prose, translations, authored tasks, raw responses, audio,
or provider payloads into the shared contract.

The upstream repository owns the JSON schemas and executable validator. The
consumer boundary pins the exact Draft commit and raw schema SHA-256 digests;
the profile namespace is `cwl_cefr_language_assessment/v1`, while result
envelopes declare `cwl_cefr_language_assessment/result_snapshot/v1`. The pin is
review evidence only until upstream publishes a released artifact.

## APA 7 reference

Council of Europe. (2020). *Common European framework of reference for
languages: Learning, teaching, assessment—Companion volume*. Council of Europe.
https://rm.coe.int/cefr-companion-volume-with-new-descriptors-2020/16809ea0d4

## Evidence boundary

The framework reference does not by itself validate an examination provider's
CEFR link or certification claim. Those claims require exact standard-setting,
empirical linking/classification-validation, and governed certification
evidence. This distinction is represented by `CefrClaimStatus` and enforced by
the product-side result-binding tests.
