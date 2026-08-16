# ADR-0020: Persist participant identity links as append-only history

- Status: Accepted
- Date: 2026-08-10
- Deciders: Psychometrics Commons maintainers
- Scope: Psychometrics Commons participant identity-link lifecycle, Keyverse boundary, data-rights and research-identity separation
- Supersedes: none
- Superseded by: none
- Extends: ADR-0003
- Related: ADR-0006, ADR-0007, ADR-0010, ADR-0015

## Context

Psychometrics Commons is anonymous-first. A participant may later attach a Keyverse account, but identity federation remains Keyverse-owned and operational assessment identity must remain independent from the research identity namespace.

The protected-main domain primitive in `src/participant.rs` already enforces part of the first boundary: an optional first account link does not replace the product-owned `participant_ref`, separate anonymous/authenticated proof references are required, exact event replay is idempotent, conflicting replay fails closed, and a second in-place link is rejected. Protected main does **not** yet persist an identity issuer alongside the provider-scoped subject, so it cannot claim issuer-scoped external identity as shipped behavior.

Active PR #29 (`fix: scope participant account links by identity issuer`) adds that missing domain-level issuer binding: issuer and provider-scoped subject are validated and stored together, issuer substitution changes replay identity and fails closed, and public docstrings explain the boundary. This is `IMPLEMENTED_ON_ACTIVE_PR`, not `IMPLEMENTED_ON_PROTECTED_MAIN`, until the exact reviewed PR head is merged. It still does not implement append-only persistence, unlink/relink/recovery transport, or Keyverse token verification.

A nullable current subject link on a participant projection is therefore useful as an application view, but it is insufficient as the future physical persistence model. In-place replacement would lose who linked or unlinked an account, when the relationship changed, why it changed, and which historical sessions/results were valid under which operational identity context. It would also make identity recovery vulnerable to accidental historical rewrites and would encourage coupling product records to an identity-provider object lifecycle.

This ADR is a mixture of current and target state. Protected main provides stable participant identity plus an issuer-scoped fail-closed first-link primitive, including dual-proof authorization at the application boundary. Append-only persistence is Active PR work. HTTP unlink/relink/recovery transport, live Keyverse token verification, and backup/restore evidence remain target behavior until corresponding source, migrations, tests, and release evidence are merged.

### Implementation status

- `IMPLEMENTED_ON_PROTECTED_MAIN`: stable product-owned `participant_ref`; issuer-scoped first subject link; distinct proof references; exact-replay idempotency; conflicting replay rejection; no silent second link; dual-proof authorization in `src/account_link.rs`.
- `IMPLEMENTED_ON_ACTIVE_PR`: PR #133 adds append-only `participant_identity_link` / `participant_identity_link_end` persistence, derived current-link projection, lifecycle-order persist of a complete unlink+relink aggregate, restart reload, current-subject lookup from unterminated history, and exact-replay reconciliation of a missing or stale current projection through `src/postgres_participant_identity_link.rs`. Prefer #133 over #124 and #114.
- `PLANNED`: durable HTTP transport, unlink/relink/recovery operator commands, concurrency arbitration beyond the participant row lock, data-rights execution, backup/restore evidence, and live Keyverse verification.

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

The exact physical columns and indexes are deferred until the migration exists; this ADR defines lifecycle and ownership semantics, not fabricated DDL.

### Link lifecycle

1. Anonymous participation creates or uses the stable product-owned participant identity without a Keyverse dependency.
2. The first successful account attachment appends an Active identity-link record. It does not mutate historical assessment evidence.
3. Unlink, recovery, or account replacement appends a new lifecycle record and/or explicit revocation/supersession record. It never edits an old link into a different identity.
4. A current-account view may project the latest valid link for application convenience, but the projection is derivable and not the evidence source of truth.
5. Ambiguous concurrent active links fail closed until an explicit recovery/merge rule resolves them.

## Ownership and boundaries

| Responsibility | Owner | Interface | Forbidden coupling |
|---|---|---|---|
| Stable product participant identity and identity-link history | Psychometrics Commons | Product domain contract; future hosted API/event contracts when implemented | Replacing `participant_id` with an IdP subject; mutable history |
| Authentication/federation and external subject lifecycle | Keyverse | Opaque issuer/subject references through a versioned service contract | Direct Keyverse database access from Commons |
| Research pseudonyms and restricted linkage | Psychometrics Commons research boundary under ADR-0006/0007 | Restricted research-linkage contract | Reusing operational or Keyverse identifiers as public research identities |
| Participant export/deletion execution | Psychometrics Commons with dependency adapters | Data-rights workflow | Silent deletion or rewriting of scientifically retained historical evidence |

Keyverse is a dependency, not a source of truth for historical product identity. No application-level cross-service database join, cascading foreign key, or hidden shared identity table is permitted.

## Contract details

The protected-main machine-readable contract is the Rust domain surface in `src/participant.rs`: it supports stable participant identity plus a first optional subject-link boundary. PR #29 strengthens that same surface so the external account is represented by issuer plus provider-scoped subject. No hosted identity-link HTTP API, durable transport event, or physical identity-link schema is claimed by either state.

Target transport/persistence contracts must preserve the following semantics when implemented:

- public/product references are opaque and non-numeric;
- identity issuer plus provider-scoped subject is treated as immutable external evidence for one appended link record;
- command retries use an explicit idempotency identity so an exact replay is harmless while conflicting evidence fails closed;
- lifecycle changes carry a versioned operation/schema contract and do not mutate a previous identity-link record;
- concurrency must prevent two ambiguous current links from becoming authoritative for the same participant/tenant without an explicit recovery decision;
- ordering uses server-authoritative accepted/effective evidence rather than client clocks alone;
- retryable dependency failure may delay linking but may not partially rewrite participant history;
- invalid references, tenant mismatch, conflicting replay, ambiguous concurrent active links, and unverified recovery authority are fail-closed errors.

When the first hosted API is implemented, its OpenAPI contract becomes required evidence. When the first durable identity-link event transport is implemented, its AsyncAPI-equivalent machine contract becomes required evidence. When the first physical migration is implemented, the as-built schema/migration evidence becomes required. None is fabricated ahead of implementation.

## Data and persistence impact

The logical ERD adds `participant_identity_link` as a one-to-many history from a stable product participant. At most one link may be authoritative as the current active projection under the applicable tenant/lifecycle rule, while historical rows remain immutable evidence.

The entity contains operational identity data and therefore remains outside public research releases. Research pseudonyms and restricted research linkage remain separate data classes and namespaces. Data-rights processing must distinguish the current external relationship, legally retained audit evidence, and immutable scientific records rather than treating the entire graph as one deletable identity row.

Physical table/column/index choices, retention periods, encryption implementation, and uniqueness syntax remain implementation-gated. `docs/architecture/ERD.md` is the logical target view; it is not evidence that a corresponding table already exists.

## Invariants

- `participant_id` remains stable across link, unlink, recovery, and replacement.
- A historical session/result never has its participant identity rewritten because of an account-link lifecycle change.
- An existing identity-link history record is immutable; later state is represented by appended evidence.
- A current-link projection is derived from history and is never the evidence source of truth.
- Keyverse issuer/subject references never become product-domain primary keys.
- Research releases contain neither Keyverse subject references nor operational `participant_id` values.
- Tenant context is explicit; no default tenant is inferred for a write.
- Exact idempotent replay is safe; conflicting replay or ambiguous concurrency fails closed.

These invariants require unit/state-machine tests at the domain layer and transaction/concurrency/restore tests once persistence exists.

## Failure and degraded modes

If Keyverse is unavailable, anonymous assessment remains usable and account linking is unavailable or queued only through an explicitly implemented durable command path; the product must not invent a successful link. A retry after an uncertain result must use the same idempotency identity so exact replay cannot create a second logical link.

Conflicting issuer/subject evidence, tenant mismatch, concurrent competing active links, missing recovery authority, or malformed references fail closed. Recovery operators must append compensating/revocation evidence rather than editing historical rows. A poison or permanently invalid integration message is quarantined by the integration boundary rather than repeatedly applying identity changes.

A failure after local transaction commit but before external acknowledgement is recovered by replaying durable evidence; a failure before commit leaves no authoritative new link. These are target transaction requirements until physical persistence and transport are implemented.

## Security, privacy, and tenancy

Identity-link evidence is sensitive operational identity data. Access requires tenant- and task-bound authorization under the repository authorization contract. Link/recovery operations require authenticated authority appropriate to the future transport and must be auditable without exposing unnecessary assessment payloads.

The system must prevent cross-tenant BOLA/IDOR, issuer/subject substitution, recovery takeover, and research re-identification. External references remain opaque; public research exports must statically and dynamically prove absence of operational/Keyverse identifiers. Encryption, residency, retention, and accepted-risk evidence are profile- and implementation-specific and must not be claimed from this ADR alone.

Material trust-boundary changes must remain aligned with `docs/architecture/SECURITY_AND_DATA.md` and `docs/THREAT_MODEL.md`.

## Deployment and operations impact

The protected-main subject-link primitive adds no new runtime dependency, and PR #29's issuer-scoped domain strengthening also adds no runtime dependency. Future persistence adds a Commons-owned database entity/projection; future live linking adds a Keyverse capability dependency but anonymous assessment must not require Keyverse readiness.

Operational health must distinguish core assessment capability from optional account-link capability. Logs/metrics must expose link/recovery outcome classes, idempotent duplicates, conflicts, authorization failures, and dependency failures without leaking raw subject identifiers. Backup/restore drills must prove identity-link history and its current projection reconcile after restore.

No universal SLO, RPO, or RTO is established here; measured deployment-profile evidence remains governed by ADR-0017 and `docs/OPERABILITY.md`.

## Migration and rollback

Initial implementation must create append-only persistence without rewriting historical participant/session/result identifiers. If a legacy current-link projection exists, bootstrap must either derive history from trustworthy evidence with provenance or leave the relationship unlinked for explicit recovery; it must not invent historical timestamps or actors.

During any compatibility window, old readers may consume a derived current-link projection while new writes append history. Dual-write is acceptable only if one transactionally authoritative history is defined and reconciliation proves projection consistency.

A schema migration may be rolled back only before new append-only evidence depends on it and only when the rollback preserves all committed identity-link records. Once lifecycle history has been accepted, deleting or mutating that evidence is not a true rollback; recovery uses roll-forward migration or compensating append-only records. Rollback triggers include violated immutability, tenant leakage, unreconciled current-link projection, or inability to restore history reliably.

## Architecture-view impact

- `ARCHITECTURE.md` — ownership remains consistent; update if the bounded-context boundary changes.
- `docs/architecture/C4.md` — Keyverse remains an external dependency; update when a concrete hosted adapter/container is implemented.
- `docs/architecture/UML.md` — identity-link lifecycle is required and must stay aligned with link/recovery behavior.
- `docs/architecture/ERD.md` — logical `participant_identity_link` history and cardinality are required; physical DDL remains implementation-gated.
- `docs/architecture/SECURITY_AND_DATA.md` — operational/research identity separation and trust boundaries must remain aligned.
- `docs/architecture/DEPLOYMENT_AND_OPERATIONS.md` — update when live Keyverse capability or physical persistence changes readiness/recovery behavior.
- `docs/TRACEABILITY.md` — distinguish the protected-main first-link primitive, PR #29 issuer-scoped active-PR behavior, and target append-only persistence/transport.
- `docs/ROADMAP.md` — retain identity-link persistence/recovery evidence in the implementation queue until merged and verified.

## Validation and release evidence

Before account-link persistence is considered GA-complete, exact-head evidence must demonstrate:

- first-link success and exact idempotent replay behavior;
- rejection of conflicting replay and concurrent competing link attempts;
- append-only unlink/relink/recovery history;
- no mutation of historical session/result participant references;
- tenant isolation and fail-closed issuer/subject validation;
- transaction rollback and crash/retry behavior without duplicate effects;
- data-rights propagation under ordinary deletion and legal-retention cases;
- no operational/Keyverse identifier leakage into public research-release fixtures;
- migration/roll-forward compatibility and backup/restore preservation of link history plus derived current projection;
- security tests for account-link/recovery takeover and cross-tenant access;
- exact deployment-profile recovery evidence before any GA/SLO/RPO/RTO claim involving this persistence.

Protected main satisfies the domain-level issuer-scoped first-link portion of this decision, including dual-proof authorization. Active PR persist must apply each link and then its matching ends in one transaction so a restart can write a complete unlink+relink aggregate. HTTP transport, live Keyverse verification, operator recovery commands, and backup/restore evidence remain target work until separately implemented, reviewed, and merged.

## Alternatives considered

### Mutate a nullable Keyverse subject directly on the participant row

Rejected because it destroys historical link/unlink/recovery evidence, couples product identity to an external provider lifecycle, and makes accidental historical rewrites easy. It could become a derived cache/projection, but never the source of truth while the append-only requirement remains.

### Use the Keyverse subject as the participant primary key

Rejected because it breaks anonymous-first operation, provider replaceability, research/operational namespace separation, and historical stability. No foreseeable evidence makes this suitable for the current product contract.

### Store identity history only in Keyverse

Rejected because Psychometrics Commons must explain which external identity evidence was authoritative for its own participant lifecycle without cross-service database access. A future cryptographically verifiable Keyverse event ledger could reduce duplicated metadata, but Commons would still need product-owned acceptance/provenance evidence.

### Rewrite historical product records during account recovery

Rejected because recovery would alter scientific and audit provenance. Reconsideration would require a fundamentally different legal/scientific identity model and a superseding ADR with migration evidence.

## Consequences

### Positive

- Account attachment cannot rewrite historical assessment/result identity.
- Recovery, unlink, and relink operations become auditable and testable.
- Keyverse remains replaceable and independently deployable.
- Anonymous participation remains a complete first-class product path.
- Research identity separation remains explicit.
- A future physical schema can enforce current-link/concurrency constraints without overloading the participant projection.

### Costs and operational burden

- The persistence layer needs one additional logical entity plus a current-link projection/query.
- Recovery and unlink flows require explicit lifecycle semantics rather than a single-row update.
- Data-rights propagation must account for identity-link evidence separately from scientific result evidence.
- Operators need reconciliation and recovery procedures for ambiguous or failed link transitions.

### Accepted risks

Until persistence/transport are implemented, protected main provides only the stable subject-link domain boundary and PR #29 provides only an active-branch issuer-scoped strengthening, not an end-to-end recovery/audit capability. Neither state is evidence that append-only identity persistence is GA-complete.

## Follow-up work

- Psychometrics Commons: implement the physical append-only identity-link migration and repository transaction boundary.
- Psychometrics Commons: add unlink/relink/recovery commands with explicit idempotency, authority, and audit evidence.
- Psychometrics Commons: add Keyverse adapter contract without direct database coupling.
- Psychometrics Commons: integrate data-rights propagation and restricted research-linkage separation tests.
- Psychometrics Commons: add transaction/concurrency/crash/backup/restore and public-release leakage tests.
- Documentation owners: reconcile OpenAPI, AsyncAPI, physical ERD/schema, UML, Traceability, Threat Model, and Operations documents when the corresponding implementations become real.

## Reversal conditions

Revisit or supersede this decision if any of the following becomes true:

- product identity no longer supports anonymous participation;
- a legal requirement mandates destructive removal incompatible with the documented retained-evidence model and cannot be satisfied by severing current external identity plus lawful retention controls;
- the identity provider supplies a formally accepted immutable event ledger that changes which system must retain product-side link history;
- measured performance/operability evidence shows the append-only history plus projection cannot meet accepted product requirements and an alternative preserves equivalent audit/scientific integrity;
- research/operational identity boundaries materially change under a superseding privacy/research-governance decision.

Any reversal requires a superseding ADR and an explicit migration/rollback or roll-forward plan that preserves already accepted historical evidence.

## Traceability

- Product requirements: `docs/PRD.md` anonymous participation, optional account linking, research contribution, and data-rights requirements.
- Technical requirements: `docs/TRD.md` identity, tenant authorization, consent/data-rights, persistence, and integration contracts.
- Protected-main domain evidence: `src/participant.rs`, `src/account_link.rs`, and their contract tests on the protected-main baseline named by `docs/TRACEABILITY.md`.
- Active-PR persistence evidence: PR #133 `migrations/0022_participant_identity_link.sql` and `src/postgres_participant_identity_link.rs` remain `IMPLEMENTED_ON_ACTIVE_PR` until merged.
- Logical data view: `docs/architecture/ERD.md`.
- Behavioral view: `docs/architecture/UML.md`.
- Security/privacy views: `docs/architecture/SECURITY_AND_DATA.md`, `docs/THREAT_MODEL.md`.
- Operations/recovery: ADR-0017 and `docs/OPERABILITY.md`.
- Maturity/status mapping: `docs/TRACEABILITY.md` and `docs/ROADMAP.md`.
- Machine-readable transport artifacts: none claimed until a hosted identity-link API exists.
- Physical-schema artifacts: Active PR `migrations/0022_participant_identity_link.sql` is not protected-main truth.

## References

International Organization for Standardization & International Electrotechnical Commission. (2019). *IT security and privacy—A framework for identity management—Part 1: Terminology and concepts* (ISO/IEC 24760-1:2019).

National Institute of Standards and Technology. (2025). *Digital identity guidelines* (NIST Special Publication 800-63-4). https://doi.org/10.6028/NIST.SP.800-63-4
