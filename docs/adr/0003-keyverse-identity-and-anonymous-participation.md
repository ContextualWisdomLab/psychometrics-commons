# ADR-0003: Keyverse identity and anonymous participation

- Status: Accepted
- Date: 2026-08-09
- Scope: authentication, account linking, anonymous sessions, authorization references

## Context

The platform must support low-friction anonymous assessment, optional persistent accounts, enterprise federation, and research participation without duplicating an identity provider or leaking operational identity into research releases.

## Decision

Keyverse owns authentication, federation, credentials, passkeys, and account linking. Psychometrics Commons validates Keyverse-issued tokens and owns product authorization, participant records, consent, and resource access.

Anonymous assessment is a first-class mode. An anonymous participant receives a cryptographically random public reference and a short-lived session credential. Account linking is optional and creates a mapping from the assessment participant to a Keyverse subject; it does not rewrite historical response or result identifiers.

Research participants use a separate pseudonymous identifier generated behind the research boundary. Keyverse subject identifiers and assessment participant identifiers are prohibited from public release bundles.

## Identity records

```text
assessment_participant
- participant_ref
- keyverse_subject_ref nullable
- account_linked_at nullable
- participant_status

research_participant
- research_participant_ref
- research_program_ref
- pseudonym_key_version
```

The mapping between operational and research identities is stored in a restricted linkage store, not in release datasets.

## Authorization

Keyverse claims establish authenticated subject and coarse scopes. Psychometrics Commons performs resource-level decisions for instrument administration, result ownership, research roles, data export, deletion, and release approval. A Keyverse administrator is not automatically a Psychometrics Commons research data steward.

Anonymous session commands are a product-owned gate after the short-lived proof has already been verified. This slice is an as-built library: `authorize_anonymous_session_command` / `apply_anonymous_session_command` compare the verified actor to the supplied `ParticipantRecord` and `AssessmentSession`. They do not accept a caller-built `ResourceScope`. They do not prove those records were loaded from the store. Persist/reload of `assessment_participant` remains Target. HTTP transport remains Target on protected main; the active session-reload implementation adds an explicit host-verified authority boundary and participant-bound persistence lookup and must not be treated as protected-main evidence until merged and refetched.

Fail-closed classification order is a product contract: trusted server time, exclusive expiry, supplied-participant tenant, session/participant ownership, actor participant, then session identity. Named tests: `anonymous_command_authorization_fails_closed_for_zero_or_expired_server_time`, `anonymous_command_authorization_rejects_compound_foreign_tenant_and_inconsistent_supplied_pair_as_cross_tenant`, and `anonymous_command_authorization_rejects_actor_when_supplied_participant_and_session_agree`.

Trusted server time and exclusive authenticator validity follow NIST SP 800-63-4 (Temoshok et al., 2025). That publication does not specify the tenant-then-owner-then-session error order.

The lower-level `authorize_anonymous_session(actor, resource, now)` check remains for callers that already hold a stored assessment-session `ResourceScope`. It is not sufficient by itself for a command against a different supplied session.

## Invariants

1. Core anonymous assessment does not require a Keyverse account.
2. Token validation uses issuer, audience, expiry, signature, and nonce/state checks as applicable.
3. Service authorization fails closed when keys or claims are invalid.
4. Account linking requires proof of control of both the anonymous session and the authenticated account.
5. Account merge and unlink operations are append-only audited.
6. Research-release data cannot be joined to Keyverse using fields present in the release bundle.

## Failure modes

- Keyverse outage: new authenticated sessions may be unavailable, but anonymous sessions and already validated short-lived sessions continue within their validity window.
- JWKS rotation: cached keys are refreshed with bounded retry; unknown key IDs fail closed.
- Account-link conflict: no automatic last-write-wins; the operation enters adjudication.
- Deleted account: operational obligations and legal retention are evaluated separately from account credential removal.

## Privacy approach

The product does not rely on blanket PII masking that destroys operational utility. It uses identity separation, purpose-bound schemas, field-level authorization, encryption, audited privileged views, and tokenized references. Construct-relevant personal data is processed only under an explicit purpose and access policy.

## Validation

- token validation and audience-confusion tests;
- cross-tenant authorization tests;
- anonymous command-path tests that classify tenant before ownership and leave the session unmutated on authorization failure;
- anonymous-to-account linking replay and conflict tests;
- research-release joinability tests;
- account deletion/export end-to-end tests.

## Alternatives rejected

- **Build a new IdP:** duplicate and high-risk.
- **Require accounts for all tests:** harms access and introduces unnecessary PII.
- **Use Keyverse roles as all product authorization:** too coarse and couples domain governance to identity administration.

## Reversal conditions

Revisit if Keyverse cannot meet a deployment's residency or federation requirements. A replacement must remain OIDC-compatible and preserve subject-mapping semantics without moving credentials into the product database.

## References

Temoshok, D., Proud-Madruga, D., Choong, Y.-Y., Galluzzo, R., Gupta, S., LaSalle, C., Lefkovitz, N., & Regenscheid, A. (2025). *Digital identity guidelines* (NIST Special Publication 800-63-4). National Institute of Standards and Technology. https://doi.org/10.6028/NIST.SP.800-63-4
