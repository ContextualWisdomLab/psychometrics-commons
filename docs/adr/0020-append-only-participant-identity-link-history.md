# ADR-0020: Persist participant identity links as append-only history

- Status: Accepted
- Date: 2026-08-10
- Deciders: Psychometrics Commons maintainers
- Extends: ADR-0003
- Related: ADR-0006, ADR-0007, ADR-0010, ADR-0015

## Context

Psychometrics Commons is anonymous-first. A participant may later attach a Keyverse account, but identity federation remains Keyverse-owned and operational assessment identity must remain independent from the research identity namespace.

The protected-main domain primitive in `src/participant.rs` already enforces an important first boundary: an optional first account link does not replace the product-owned `participant_id`, blank issuer/subject references fail closed, and a second in-place link is rejected. The module also reserves unlink/relink/account-recovery semantics for an explicit audited lifecycle rather than silent mutation.

A nullable `keyverse_subject_ref` on a current participant projection is therefore useful as an application view, but it is insufficient as the future physical persistence model. In-place replacement would lose who linked or unlinked an account, when the relationship changed, why it changed, and which historical sessions/results were valid under which operational identity context. It would also make identity recovery vulnerable to accidental historical rewrites and would encourage coupling product records to an IdP object lifecycle.

## Decision

Persistent account attachment is modeled as a product-owned **append-only participant identity-link history**. Historical participant, session, response, scoring, and result identifiers are never rewritten when an external account is linked, unlinked, recovered, or replaced.

The logical persistence contract introduces `participant_identity_link` with, at minimum:

- opaque `identity_link_id`;
- product-owned `participant_id` and `tenant_id`;
- normalized `identity_issuer`;
- opaque provider-scoped `identity_subject_ref`;
- `link_state` and an effective timestamp;
- append-only supersession/revocation reference where applicable;
- actor/evidence reference appropriate to the operation;
- immutable creation timestamp and reason/evidence metadata needed for audit.

The exact physical columns and indexes are deferred until the migration exists; this ADR defines the lifecycle and ownership contract, not fabricated DDL.

### Link lifecycle

1. Anonymous participation creates/uses the stable product-owned participant identity without a Keyverse dependency.
2. The first successful account attachment appends an Active identity-link record. It does not mutate historical assessment evidence.
3. Unlink, recovery, or account replacement appends a new lifecycle record and/or explicit revocation/supersession record. It never edits an old link into a different identity.
4. A current-account view may project the latest valid link for application convenience, but the projection is derivable and not the evidence source of truth.
5. Ambiguous concurrent active links fail closed until an explicit recovery/merge rule resolves them.

### Namespace and deletion boundaries

- `participant_id` is the Psychometrics Commons operational identity key and remains stable across optional account attachment.
- Keyverse issuer/subject references are opaque external references, not domain primary keys and not cascading foreign keys to an IdP database.
- Research releases never expose Keyverse subject references or the operational `participant_id`.
- Research pseudonyms and restricted linkage records remain in the research namespace governed by ADR-0006/0007; `participant_identity_link` is not reused as a public research identity table.
- Participant-rights workflows may deactivate or sever the current external-account relationship subject to lawful retention, while preserving the minimum append-only evidence required by the accepted deletion/retention policy. They must not silently rewrite historical scientific records.

## Consequences

### Positive

- Account attachment cannot rewrite historical assessment/result identity.
- Recovery, unlink, and relink operations become auditable and testable.
- Keyverse remains replaceable and independently deployable.
- Anonymous participation remains a complete first-class product path.
- Research identity separation remains explicit.
- A future physical schema can enforce unique/current-link and concurrency constraints without overloading `assessment_participant`.

### Costs

- The persistence layer needs one additional logical entity plus a current-link projection/query.
- Recovery and unlink flows require explicit lifecycle semantics rather than a single-row update.
- Data-rights propagation must account for identity-link evidence separately from scientific result evidence.

## Verification

Before account-link persistence is considered GA-complete, exact-head evidence must demonstrate:

- first-link success and idempotent replay behavior;
- rejection of conflicting concurrent link attempts;
- append-only unlink/relink/recovery history;
- no mutation of historical session/result participant references;
- tenant isolation and fail-closed issuer/subject validation;
- data-rights propagation under ordinary deletion and legal-retention cases;
- no operational/Keyverse identifier leakage into public research-release fixtures;
- migration/rollback and backup/restore preservation of link history.

The current `src/participant.rs` first-link primitive satisfies only the domain-level first-link portion of this decision. Persistence, transport, recovery, unlink/relink, and audit evidence remain target work until implemented and merged to protected main.
