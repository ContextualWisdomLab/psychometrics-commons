# ADR-0021: Persist and reload live measurement sessions

- Status: Accepted
- Date: 2026-08-18
- Deciders: ContextualWisdomLab Psychometrics Commons maintainers
- Scope: Psychometrics Commons-owned live session membership, consent records, audit events, and export-snapshot pointers
- Supersedes: none
- Superseded by: none
- Current/as-built status: this change implements PostgreSQL 18 persist/reload for the live measurement-session aggregate; it is not protected-main truth until the reviewed head is integrated
- Target status: none for this slice; identity-link history, command HTTP, and score kernels remain outside this decision
- Migration status: additive `migrations/0020_measurement_session.sql`; no backfill of historical identity-link or score rows

## Context

After #225, persist/reload of a live measurement session remained the standing product Target. Buyers already grant purpose-specific consent and expect a later process or request to continue from that grant. In-memory aggregates disappear when the writer dies. Closed restore-reconcile work (#159, #158, #147, #133, #114, #124) is not a current landing.

This repository owns provenance, sessions, consent, audit, participant identity, and export pointers (ADR-0004, ADR-0007). It does not own IRT, linking, scoring kernels, or Keyverse credentials. Blanket PII masking would remove the exact consent and membership evidence authorized work needs (ADR-0003).

Assumptions: the caller supplies a purpose-bound 32-byte AES-256-GCM key that never enters logs; PostgreSQL 18 is the operational store (ADR-0015); identity-link history remains a later slice and is not this decision.

## Decision

1. Psychometrics Commons persists one live `MeasurementSession` as `assessment_participant`, `measurement_session`, `session_membership`, `session_consent_record`, `session_audit_event`, and `export_snapshot_pointer`.
2. Persist and reload require `ProductPermission::ManageOwnSession` on the stored tenant/owner/session. Consent or data-rights permissions cannot be reused as the persist purpose.
3. Consent and audit payloads are sealed with AES-256-GCM. Additional authenticated data binds `measurement_session_persist`, the session reference, the field name, and the event reference. A wrong key or purpose fails closed.
4. Reload after process death restores byte-for-byte `provenance_bytes` for membership, consent, audit, and the export pointer. A granted `service_operation` consent remains granted so the buyer continues without re-consenting.
5. The export pointer stores snapshot/request references and a SHA-256 artifact digest. It does not store numeric scores.
6. Identity-link history, IRT, linking, and scoring kernels are forbidden in this adapter. Closed #159 is not reopened.

This decision describes as-built library behavior on this change. HTTP for this aggregate is not added here.

## Ownership and boundaries

| Responsibility | Owner | Interface | Forbidden coupling |
|---|---|---|---|
| Live session membership, consent, audit, export pointer | psychometrics-commons | `persist_measurement_session` / `load_measurement_session` | fast-mlsirm kernels, Keyverse credential stores |
| Purpose-limited authorization | psychometrics-commons | `authorize_measurement_session` using `ManageOwnSession` | caller-built scopes that swap consent or export permission |
| Purpose-bound encryption | psychometrics-commons + deployment key manager | `SessionEncryptionKey` | logging key bytes; PII masking of authorized fields |
| Identity-link history | later slice | ADR-0020 | this migration/adapter |
| Numeric scores / IRT / linking | fast-mlsirm | versioned scoring contracts | persisting scores in `export_snapshot_pointer` |

## Contract details

- Identifiers: opaque two-or-more-word `snake_case` references; public IDs remain non-numeric.
- Idempotency: exact replay of the same session evidence returns `Duplicate`. Rebinding tenant, owner, time, membership, consent ciphertext, audit evidence, or export digest returns `ConflictingReplay`.
- Isolation: persist and reload require `READ COMMITTED`. Stronger isolation fails closed.
- Ordering: membership sorts by `participant_ref`; consent and audit sort by `event_ref`.
- Timeouts/retries: the caller owns the transaction. A writer crash after commit is recovered by reload on a new connection.
- Errors: unauthorized, unsupported isolation, conflicting replay, value out of range, domain sealing failure, or database failure. Missing sessions return `None` after a successful header lookup.
- HTTP/AsyncAPI: none in this slice. Existing `POST /v1/sessions` start/reload remains the created-session identity family and does not replace this aggregate.

## Data and persistence impact

New 3NF relations in `migrations/0020_measurement_session.sql`:

- `assessment_participant` `(participant_ref, tenant_ref, created_at_unix_ms)`
- `measurement_session` `(session_ref, tenant_ref, owner_participant_ref, created_at_unix_ms)`
- `session_membership` `(session_ref, participant_ref, enrolled_at_unix_ms)`
- `session_consent_record` `(session_ref, event_ref, participant_ref, encryption_nonce, ciphertext_payload)`
- `session_audit_event` `(session_ref, event_ref, actor_ref, occurred_at_unix_ms, encryption_nonce, ciphertext_payload)`
- `export_snapshot_pointer` `(session_ref, snapshot_ref, request_ref, content_digest, created_at_unix_ms)`

Cardinality: one session has one owner, one-or-more members, zero-or-more consent and audit events, and zero-or-one export pointer. Transaction boundary is the caller transaction (ADR-0015). Retention: audit and consent ciphertext are retained with the session until a later data-rights deletion slice executes. `docs/architecture/ERD.md` and `docs/architecture/AS_BUILT_SCHEMA.md` record this physical slice.

## Invariants

1. Persist/reload authorization is `ManageOwnSession` only. Tests: `authorization_is_purpose_limited_to_manage_own_session`, `unauthorized_or_wrong_key_cannot_read_or_rewrite_the_session`.
2. Exact replay is idempotent; rebinding fails closed. Test: `exact_replay_is_duplicate_and_rebinding_fails_closed`.
3. Process death does not require re-consent. Test: `persist_then_process_death_reloads_consent_audit_and_membership`.
4. A wrong encryption key cannot open ciphertext. Test: `unauthorized_or_wrong_key_cannot_read_or_rewrite_the_session`.
5. No score, IRT, or identity-link columns exist in migration 0020. Control: schema review plus this ADR.

## Failure and degraded modes

- Unauthorized persist/reload: fail closed; ciphertext is not returned.
- Wrong key / AAD: `Domain(SealingFailed)`.
- Missing session: `Ok(None)`.
- Missing relation or database error: `Database`.
- Stronger isolation: `UnsupportedIsolationLevel`.
- Poison/partial writes: caller rolls back the transaction; committed sessions reload as a whole.
- Users see no invented consent grant and are not asked to re-consent when reload succeeds.

## Security, privacy, and tenancy

- Authentication remains Keyverse. This slice consumes `AuthorizationContext`.
- Authorization is tenant- and owner-bound. Cross-tenant persist/reload is denied before ciphertext is opened on reload after the header is visible.
- Encryption: AES-256-GCM at the application boundary with a purpose-bound key. This is not a SOC 2/CSAP certification claim.
- Purpose limitation: persist purpose is `measurement_session_persist`. Audit events carry an explicit `purpose_ref`.
- Residency: keys and the PostgreSQL store follow the deployment profile (ADR-0011/0017).
- Audit: append-only `session_audit_event` rows. Retention follows the operational audit schedule; records are not PII-masked.
- No blanket masking of participant, consent-form, or audit action fields needed for authorized work.

## Deployment and operations impact

- New required relations for session persist/reload readiness probes that declare them.
- Backup/restore must include the six relations and the encryption-key material needed to open ciphertext (ADR-0017). Losing the key makes consent/audit unrestorable without re-consent, which this decision forbids as a silent recovery path.
- No SLO/RPO/RTO numbers are invented here.
- `docs/architecture/DEPLOYMENT_AND_OPERATIONS.md` is unchanged except that this aggregate becomes part of the product database once merged.

## Migration and rollback

- Bootstrap: apply `0020_measurement_session.sql` (idempotent `CREATE TABLE IF NOT EXISTS`).
- No dual-read/write. Empty tables are valid.
- Rollback: drop the six new relations only when no committed buyer sessions exist; otherwise roll forward. Ciphertext cannot be reconstructed after a drop.
- Compatibility window: library callers that do not call the new functions are unaffected.

## Architecture-view impact

- `ARCHITECTURE.md`: unchanged ownership; commons still does not own scoring kernels.
- `docs/architecture/C4.md`: unchanged container boundary.
- `docs/architecture/UML.md`: command gate still compares supplied records; persist/reload is a separate library path.
- `docs/architecture/ERD.md`: physical persist/reload of live measurement sessions recorded.
- `docs/architecture/SECURITY_AND_DATA.md`: persist/reload implemented; identity-link remains later.
- `docs/architecture/DEPLOYMENT_AND_OPERATIONS.md`: no profile change.
- `docs/TRACEABILITY.md`: this slice is the current persist/reload landing; #159 is not.
- `docs/ROADMAP.md`: no new kernel work.

## Validation and release evidence

- Domain tests in `src/measurement_session.rs`.
- Real PostgreSQL tests in `tests/postgres_measurement_session.rs`.
- Documentation fitness in `tests/documentation_architecture_contract.rs`.
- Release blockers: independent non-author review, required CI, 100% owned line/branch coverage, no score kernel, no #159 reopen.
- This ADR is implementation evidence for the library slice, not deployed SOC 2/CSAP attestation.

## Alternatives considered

- Reuse only `assessment_session` lifecycle rows: rejected; that aggregate does not store consent, audit, membership, or export pointers.
- Persist identity-link history here: rejected; ADR-0020 and closed #52 remain a later slice.
- Mask consent/audit fields: rejected; masking paralyzes authorized reload and re-consent checks.
- Store scores on the export pointer: rejected; ADR-0004 keeps numeric truth in fast-mlsirm.

## Consequences

Positive: buyers survive writer death without re-consenting; audit and export pointers remain reconstructable; CSAP/SOC 2-oriented purpose limitation, encryption, and audit exist as implementation evidence.

Costs: deployments must manage a purpose-bound AES key; another migration is added.

Accepted risk: application-level encryption does not replace disk/TLS encryption or an external attestation.

## Follow-up work

- Identity-link history persist (ADR-0020) remains a later slice.
- HTTP for this aggregate remains Target.
- Data-rights deletion of sealed consent/audit rows remains a later slice.
- Do not start score, IRT, linking, or #159 work from this decision.

## Reversal conditions

Revisit if PostgreSQL 18 is no longer the operational store, if AES-256-GCM is withdrawn as the sealing algorithm, or if a later accepted ADR moves session membership out of this repository.

## Traceability

- PRD §3.1 / §9 pause-resume and consent-preserving continuation
- TRD §5 server-authoritative session state; TRD §12 consent; TRD §13 export
- ADR-0003, ADR-0004, ADR-0005, ADR-0006, ADR-0007, ADR-0010, ADR-0015
- `src/measurement_session.rs`, `src/postgres_measurement_session.rs`, `migrations/0020_measurement_session.sql`
- `tests/postgres_measurement_session.rs`
- W3C PROV-DM (Moreau & Missier, 2013) for session provenance; NIST SP 800-53 Rev. 5 AU-11 (National Institute of Standards and Technology, 2020) for audit retention
