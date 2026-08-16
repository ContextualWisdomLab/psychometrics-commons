# ADR-0006: Consent, data rights, and research separation

- Status: Accepted
- Date: 2026-08-09
- Scope: service consent, sensitive-data consent, research contribution, export, deletion, withdrawal

## Context

Using an assessment must not implicitly enroll a participant in research. The product also needs operationally usable data without relying on blanket PII masking, while preventing identity leakage into research datasets.

## Decision

Consent is purpose-specific, versioned, and snapshotted. At minimum the product separates:

1. service terms and processing required to deliver the assessment;
2. optional account persistence and cross-device result storage;
3. optional longitudinal/EMA processing;
4. optional research contribution;
5. optional communications.

No optional consent may be bundled as a condition of receiving the core personal result unless the instrument legitimately requires that processing and the reason is disclosed.

Research contribution begins only after an explicit opt-in referencing a specific consent-form version and research scope. Operational assessment data and research staging data use separate identifiers, schemas, access policies, and encryption contexts.

## Data flow

```text
assessment_session
-> personal_result
-> explicit research_contribution
-> restricted linkage/pseudonymization
-> research_participant
-> de-identification and privacy-risk review
-> immutable dataset_snapshot
-> approved research_release
```

## Data-rights requests

Export and deletion requests are first-class resources with state, requester verification, scope, legal-retention exceptions, evidence, and completion timestamps. Deletion is not represented by a boolean on the participant row.

Withdrawal from research stops future use and future releases according to the consent and law. Already published immutable releases cannot be silently rewritten; the release policy and any feasible withdrawal/withdrawal-notice mechanism must be stated before consent.

## PII strategy

The product does not solve privacy by masking every operational field. It uses:

- identity and domain-data separation;
- purpose-bound access policies;
- field/record-level authorization;
- encryption in transit and at rest;
- tokenized references and a restricted linkage store;
- audited privileged access;
- minimal purpose-specific views;
- deployment/provider policies for residency and retention.

Construct-relevant personal data remains available to authorized workflows when necessary; unauthorized contexts receive no access rather than a misleading masked substitute.

## Invariants

1. `research_contribution` is absent by default.
2. Consent records are immutable; revocation appends a new event.
3. The consent snapshot used for a release is preserved with the release.
4. Public research data contains no Keyverse subject or operational participant reference.
5. Service denial cannot be based on refusal of optional research contribution.
6. Data-rights operations are tenant-scoped and identity-verified.
7. A durable research-consent snapshot projection authorizes identity binding only. A new contribution start must re-check the latest research-purpose `consent_event` for that participant in the same transaction. That event must still be `granted` for the contribution's exact scope, matching `ConsentSnapshot::is_granted` / `active_research_scope`. A later grant or revoke for another scope replaces the prior scope as the live write capability. Exact replay and withdrawal of already stored evidence remain allowed (`tests/postgres_research_contribution_persistence.rs`).
8. A research participant reference cannot equal the bound operational participant and cannot be reused across operational identities (`research_contribution_identity_separation_check`, `research_contribution_research_participant_ref_unique`).

## Failure behavior

If consent verification is unavailable, optional research processing fails closed while core assessment processing may continue under its valid service basis. An ambiguous withdrawal or identity conflict enters manual adjudication; it is not auto-resolved by email or display name.

## Validation

- consent-version and revocation state-machine tests;
- PostgreSQL tests proving a stored research-consent snapshot cannot insert a new contribution after the live grant is revoked, after a later purpose-level grant for another scope, or when no consent ledger was persisted, while exact replay and withdrawal of prior evidence still succeed;
- negative tests proving research jobs reject non-opted-in participants;
- release joinability and rare-combination privacy review;
- export completeness and deletion propagation tests;
- privileged-access audit tests.

## Alternatives rejected

- **One combined consent:** violates purpose separation.
- **PII masking throughout the operational system:** destroys required workflow semantics and creates false safety.
- **Direct export from operational tables:** leaks identity and mutable state.

## Reversal conditions

Revisit individual retention or withdrawal mechanics when a deployment's law or ethics approval imposes stricter requirements. The separation of service and research purposes remains mandatory.

## References

European Parliament & Council of the European Union. (2016). Regulation (EU) 2016/679 of the European Parliament and of the Council of 27 April 2016 on the protection of natural persons with regard to the processing of personal data and on the free movement of such data (General Data Protection Regulation). *Official Journal of the European Union, L 119*, 1–88.

International Organization for Standardization. (2024). *Information technology — Security techniques — Privacy framework* (ISO/IEC 29100:2024).

National Institute of Standards and Technology. (2020). *Security and privacy controls for information systems and organizations* (NIST Special Publication 800-53, Rev. 5). https://doi.org/10.6028/NIST.SP.800-53r5
