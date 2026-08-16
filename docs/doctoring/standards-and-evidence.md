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

## Research de-identification and public-release leakage

Public research releases must not become joinable to operational assessment
identity. ISO/IEC 20889:2018, confirmed in 2024, classifies pseudonymization and
other de-identification techniques so a public fixture can carry a
program-scoped research identity without carrying the operational participant or
the restricted linkage key. ISO/IEC 27559:2022 supplies the lifecycle framework
for identifying and mitigating re-identification risk around that fixture. These
references constrain public packaging. They do not authorize blanket PII masking
that would stop authorized research, scoring, or data-rights work.

Product consequences:

- a public-release fixture is scanned for operational, Keyverse, and
  restricted-linkage column names and cell values before packaging;
- `research_participant_ref` remains a public research identity and is not
  treated as an operational `participant_ref`;
- authorized research keeps the restricted mapping outside the public package;
- residual re-identification and joinability review remain required after direct
  identifiers are removed.

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
  temporal, multilevel, cross-classified, or multiple-membership analysis.

## Evidence maintenance rules

1. Review this baseline when a referenced standard is revised, withdrawn, superseded, or materially amended.
2. Prefer the latest final published standard over drafts unless a draft is explicitly being evaluated as future-facing research.
3. Record the exact version/date in ADRs and release evidence when a requirement depends on a particular edition.
4. Do not claim certification, conformance, or regulatory compliance merely because a design cites a standard.
5. For psychometric methods, complement standards with primary methodological papers and true-parameter recovery evidence maintained by `fast-mlsirm`.
6. For implementation details of external protocols/libraries, use the current official primary documentation and pin behavior through contract tests.

## References — APA 7th

American Educational Research Association, American Psychological Association, & National Council on Measurement in Education. (2014). *Standards for educational and psychological testing*. American Educational Research Association. https://www.testingstandards.net/

International Organization for Standardization. (2018). *ISO/IEC 20889:2018 Privacy enhancing data de-identification terminology and classification of techniques* (confirmed 2024). https://www.iso.org/standard/69373.html

International Organization for Standardization. (2019). *ISO 8601-1:2019 Date and time—Representations for information interchange—Part 1: Basic rules* (with Amendment 1:2022). https://www.iso.org/standard/70907.html

International Organization for Standardization. (2022a). *ISO/IEC 27001:2022 Information security, cybersecurity and privacy protection—Information security management systems—Requirements* (3rd ed.). https://www.iso.org/standard/27001

International Organization for Standardization. (2022b). *ISO/IEC 27559:2022 Information security, cybersecurity and privacy protection—Privacy enhancing data de-identification framework*. https://www.iso.org/standard/71677.html

International Organization for Standardization. (2023a). *ISO/IEC 23894:2023 Information technology—Artificial intelligence—Guidance on risk management*. https://www.iso.org/standard/77304.html

International Organization for Standardization. (2023b). *ISO/IEC 42001:2023 Information technology—Artificial intelligence—Management system*. https://www.iso.org/standard/42001

International Organization for Standardization. (2024). *ISO/IEC 27001:2022/Amd 1:2024 Information security, cybersecurity and privacy protection—Information security management systems—Requirements—Amendment 1: Climate action changes*. https://www.iso.org/standard/27001

International Organization for Standardization. (2025). *ISO/IEC 42005:2025 Information technology—Artificial intelligence (AI)—AI system impact assessment*. https://www.iso.org/standard/42005

Temoshok, D., Proud-Madruga, D., Choong, Y.-Y., Galluzzo, R., Gupta, S., LaSalle, C., Lefkovitz, N., & Regenscheid, A. (2025). *Digital identity guidelines* (NIST Special Publication 800-63-4). National Institute of Standards and Technology. https://doi.org/10.6028/NIST.SP.800-63-4

World Wide Web Consortium. (2024). *Web Content Accessibility Guidelines (WCAG) 2.2* (W3C Recommendation, 12 December 2024). https://www.w3.org/TR/WCAG22/

World Wide Web Consortium. (2013). *PROV-DM: The PROV data model* (W3C Recommendation, 30 April 2013). https://www.w3.org/TR/prov-dm/
