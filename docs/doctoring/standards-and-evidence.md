# Standards and Evidence Baseline

- Status: Living doctoring record
- Last reviewed: 2026-08-16
- Scope: Psychometrics Commons product, hosted runtime, reference clients, optional AI, identity integration, and assessment governance

This record identifies authoritative standards and primary guidance that materially constrain product design. It is not a certification claim. Each implementation PR that relies on one of these sources must translate the source into a concrete requirement, test, control, or ADR rather than citing it decoratively.

## Educational and psychological testing

The product's core scientific governance follows the *Standards for Educational and Psychological Testing*. The standards frame validity as evidence supporting intended score interpretations and uses and treat reliability/precision, fairness, test development, administration, scoring, reporting, and documentation as connected responsibilities rather than independent quality badges.

Product consequences:

- an instrument release states intended score interpretations and prohibited/unsupported uses;
- scoring and norms are versioned and reproducible;
- precision/uncertainty is not hidden behind a point estimate;
- translated forms and group comparisons require evidence appropriate to the intended comparison;
- automated or AI-assisted components do not become the source of numeric truth without the same validation obligations as other scoring mechanisms;
- correlation with another score is supporting evidence at most, not proof of accuracy or validity.

## Web accessibility

WCAG 2.2 is the reference accessibility standard for supported web/reference clients. W3C published the current WCAG 2.2 Recommendation on 12 December 2024 and advises using WCAG 2.2 for future applicability. The product targets Level AA while also treating assessment-specific presentation changes as potential measurement changes rather than purely cosmetic changes.

Product consequences:

- keyboard completion, focus order/visibility, labels, errors, re-authentication/timeout behavior, target size, contrast, reflow, status messages, and accessible authentication are release-test areas;
- charts require equivalent text or tabular interpretation;
- timing accommodations are part of instrument-version evidence when timing can affect the response process;
- automated accessibility checks are supplemented by manual and assistive-technology acceptance testing.

## Digital identity and federation

NIST SP 800-63 Revision 4 is the current NIST Digital Identity Guidelines suite. The final Revision 4 was published in July 2025 and supersedes Revision 3. The suite covers identity proofing, authentication, authenticator management, federation, assertions, security/privacy, and customer-experience considerations.

Psychometrics Commons does not implement an identity provider; Keyverse owns identity and federation. NIST SP 800-63-4 and its A/B/C companion volumes are therefore integration and assurance guidance rather than a reason to duplicate identity functions in this repository.

Product consequences:

- validate issuer, audience, signature, expiry, and protocol anti-replay properties on federated identity assertions;
- keep anonymous participation first-class when identity proofing is unnecessary for the intended use;
- account linking requires control of both identities and never rewrites historical assessment identifiers;
- product roles such as research steward remain separate from identity-provider administration;
- passkey/authenticator and federation assurance remain Keyverse responsibilities exposed through explicit claims/contracts.

## Information security management

ISO/IEC 27001:2022 is the current published edition of the information security management system requirements standard, with Amendment 1:2024 published as an amendment. Psychometrics Commons uses it as management-system and evidence-organization guidance; this repository does not claim ISO/IEC 27001 certification.

Product consequences:

- security responsibilities are mapped to accountable owners and bounded contexts;
- risk treatment, change control, access control, incident/vulnerability handling, supplier/dependency evidence, backup/restore, continuity, and audit evidence are designed as ongoing processes;
- release gates include secret scanning, static analysis, dependency/supply-chain evidence, SBOM/provenance, migration/rollback, and recovery tests;
- cross-service database access and ambient credentials are prohibited by architecture, not merely discouraged by documentation.

## AI management, risk, and impact

Optional AI capabilities are governed by three complementary ISO/IEC references:

- ISO/IEC 42001:2023 — AI management-system requirements;
- ISO/IEC 23894:2023 — guidance for AI risk management;
- ISO/IEC 42005:2025 — guidance for AI system impact assessment.

ISO/IEC 42005:2025 was published on 28 May 2025 and is the newest of these references. These sources support explicit lifecycle governance, traceability, risk/impact assessment, and monitoring for AI used in the product. They do not justify allowing LLM output to replace psychometric scoring evidence.

Product consequences:

- every AI task declares purpose, data class, model/provider policy, residency, retention, output schema, timeout/retry, and provenance;
- provider/model aliases are resolved to an auditable version or artifact before use where the integration supports it;
- deterministic scoring remains independent from optional generative narrative;
- AI output is untrusted until schema, provenance, evidence/reference, size, and finiteness validation succeeds;
- higher-impact AI changes require an impact/risk assessment proportional to intended use and affected participants;
- model/prompt/provider drift is observable and does not silently alter historical results.

## Temporal and provenance evidence

Longitudinal observations distinguish validity time from source-recorded time,
platform receipt time, and durable-ingestion time. ISO 8601-1:2019 remains the
current published representation baseline after its 2024 review; W3C PROV-DM
supplies the provenance vocabulary for entities, activities, agents, and their
creation/use/end times. These references constrain representation and lineage;
they do not authorize inventing temporal values or treating a source timestamp as
platform-authoritative.

Product consequences:

- point observations preserve a validity interval and do not silently replace an
  interval with a receipt timestamp;
- source and platform timestamps are retained as separate evidence fields;
- clock skew, impossible ordering, and unknown precision are typed validation
  outcomes rather than reasons to rewrite source history;
- analysis-set digests bind the exact observations and time semantics consumed by
  temporal, multilevel, cross-classified, or multiple-membership analysis;
- multiple-membership weights stay explicit and complete so a `primary_group`
  shortcut cannot recreate the atomistic fallacy Robinson (1950) described;
- within-person change is not inferred from a between-person snapshot
  (Curran & Bauer, 2011; Hamaker & Wichers, 2017);
- multiple-membership / multiple-classification structure is retained for TEPP
  rather than flattened in the product ingest (Browne et al., 2001).

## Operational persistence locking

Scoring-job claim-next uses upstream PostgreSQL 18 row locking so a worker that
only knows its own identity can take the oldest due job without guessing a
`scoring_job_ref`. `SELECT ... FOR UPDATE SKIP LOCKED` is the documented lock
clause that lets concurrent pollers skip a row another transaction already
locked (PostgreSQL Global Development Group, 2026a). Claim classification
requires `READ COMMITTED` so a skipped row becomes visible after the owning
transaction commits (PostgreSQL Global Development Group, 2026b). This is
orchestration locking only. It does not move psychometric arithmetic into the
product database.

Product consequences:

- `claim_next_scoring_job` returns the stored request pin and fencing lease, or
  `None` when no job is due;
- two concurrent workers cannot both lease the same due row;
- a retry-scheduled row stays unclaimed until its persisted due time;
- an empty due set does not invent a score.

## Public HTTP problem details

Implemented public session HTTP uses RFC 9110 status semantics and RFC 9457
problem details. OpenAPI 3.2.0 is the as-built contract vocabulary for that
family. Problem details name the next buyer action and must not echo raw
request bodies, SQL, or provider text.

Product consequences:

- `POST /v1/sessions` and `GET /v1/sessions/{session_ref}` are described by
  `openapi/sessions.yaml` in the same change that implements them;
- unpublished or mismatched catalog starts return 409 with publish-or-repair
  guidance;
- missing sessions return 404 that tells the buyer to POST the same
  Idempotency-Key.

## Evidence maintenance rules

1. Review this baseline when a referenced standard is revised, withdrawn, superseded, or materially amended.
2. Prefer the latest final published standard over drafts unless a draft is explicitly being evaluated as future-facing research.
3. Record the exact version/date in ADRs and release evidence when a requirement depends on a particular edition.
4. Do not claim certification, conformance, or regulatory compliance merely because a design cites a standard.
5. For psychometric methods, complement standards with primary methodological papers and true-parameter recovery evidence maintained by `fast-mlsirm`.
6. For implementation details of external protocols/libraries, use the current official primary documentation and pin behavior through contract tests.

## References — APA 7th

American Educational Research Association, American Psychological Association, & National Council on Measurement in Education. (2014). *Standards for educational and psychological testing*. American Educational Research Association. https://www.testingstandards.net/

Browne, W. J., Goldstein, H., & Rasbash, J. (2001). Multiple membership multiple classification (MMMC) models. *Statistical Modelling, 1*(2), 103–124. https://doi.org/10.1177/1471082X0100100202

Curran, P. J., & Bauer, D. J. (2011). The disaggregation of within-person and between-person effects in longitudinal models of change. *Annual Review of Psychology, 62*, 583–619. https://doi.org/10.1146/annurev.psych.093008.100356

Hamaker, E. L., & Wichers, M. (2017). No time like the present: Discovering the hidden dynamics in intensive longitudinal data. *Current Directions in Psychological Science, 26*(1), 10–15. https://doi.org/10.1177/0963721416666518

Robinson, W. S. (1950). Ecological correlations and the behavior of individuals. *American Sociological Review, 15*(3), 351–357. https://doi.org/10.2307/2087176

International Organization for Standardization. (2022). *ISO/IEC 27001:2022 Information security, cybersecurity and privacy protection—Information security management systems—Requirements* (3rd ed.). https://www.iso.org/standard/27001

International Organization for Standardization. (2023a). *ISO/IEC 23894:2023 Information technology—Artificial intelligence—Guidance on risk management*. https://www.iso.org/standard/77304.html

International Organization for Standardization. (2023b). *ISO/IEC 42001:2023 Information technology—Artificial intelligence—Management system*. https://www.iso.org/standard/42001

International Organization for Standardization. (2024). *ISO/IEC 27001:2022/Amd 1:2024 Information security, cybersecurity and privacy protection—Information security management systems—Requirements—Amendment 1: Climate action changes*. https://www.iso.org/standard/27001

International Organization for Standardization. (2025). *ISO/IEC 42005:2025 Information technology—Artificial intelligence (AI)—AI system impact assessment*. https://www.iso.org/standard/42005

International Organization for Standardization. (2019). *ISO 8601-1:2019 Date and time—Representations for information interchange—Part 1: Basic rules* (with Amendment 1:2022). https://www.iso.org/standard/70907.html

PostgreSQL Global Development Group. (2026a). *SELECT*. https://www.postgresql.org/docs/18/sql-select.html

PostgreSQL Global Development Group. (2026b). *Transaction isolation*. https://www.postgresql.org/docs/18/transaction-iso.html

Temoshok, D., Proud-Madruga, D., Choong, Y.-Y., Galluzzo, R., Gupta, S., LaSalle, C., Lefkovitz, N., & Regenscheid, A. (2025). *Digital identity guidelines* (NIST Special Publication 800-63-4). National Institute of Standards and Technology. https://doi.org/10.6028/NIST.SP.800-63-4

World Wide Web Consortium. (2024). *Web Content Accessibility Guidelines (WCAG) 2.2* (W3C Recommendation, 12 December 2024). https://www.w3.org/TR/WCAG22/

World Wide Web Consortium. (2013). *PROV-DM: The PROV data model* (W3C Recommendation, 30 April 2013). https://www.w3.org/TR/prov-dm/

Fielding, R., Nottingham, M., & Reschke, J. (Eds.). (2022). *HTTP semantics* (RFC 9110). RFC Editor. https://doi.org/10.17487/RFC9110

Nottingham, M., Wilde, E., & Miller, S. (2023). *Problem details for HTTP APIs* (RFC 9457). RFC Editor. https://doi.org/10.17487/RFC9457

OpenAPI Initiative. (2024). *OpenAPI specification v3.2.0*. Linux Foundation. https://spec.openapis.org/oas/v3.2.0.html
