# ADR-0005: Hosted assessment runtime state machine

- Status: Accepted
- Date: 2026-08-09
- Scope: instrument release, sessions, item delivery, response events, scoring dispatch, results

## Context

A hosted assessment product requires transactional lifecycle rules that do not belong in a numerical library. Ad hoc controller logic would create duplicate responses, ambiguous completion, non-reproducible scoring, and unsafe pause/resume behavior.

## Decision

Psychometrics Commons implements an explicit hosted assessment runtime with append-only domain events and constrained state transitions.

### Instrument release

```text
draft -> review -> published -> suspended -> retired
```

Published releases are immutable. Editing content creates a new `instrument_version_ref`. Suspension blocks new sessions but does not invalidate existing result provenance. Retirement blocks new sessions permanently unless a new release is published.

### Assessment session

```text
created -> active <-> paused -> completed -> scoring -> scored -> released
```

Terminal alternatives: `expired`, `cancelled`, `invalidated`.

Only the runtime may transition session state. Clients request commands; they do not submit the target state directly.

Session creation copies the published release's `instrument_release_ref`, `instrument_version_ref`, content digest, locale, and ordered `item_version_refs`. Those values are immutable session provenance. Later publication suspend/retire blocks *new* sessions and never rewrites an already-created session.

### Item-delivery authority

Item-delivery evidence is product-runtime state, not psychometric item-selection arithmetic. `fast-mlsirm` remains the only owner of selection, calibration, and scoring kernels.

As-built domain contract in `src/item_delivery.rs` and `src/session.rs`:

- `ItemDeliveryLedger::from_session(&AssessmentSession, &InstrumentReleaseManifest)` is the only constructor. Callers cannot create a ledger from a bare `session_ref` or a detached lifecycle enum.
- The manifest must match the session's exact release reference, instrument version, content digest, locale, and ordered item-version set. Any isolated mismatch, including a reused digest with a reordered, reduced, or enlarged item set, fails closed as `SessionReleaseMismatch`.
- Allowed item versions are copied from the session aggregate, not from the caller-supplied manifest.
- `deliver(&AssessmentSession, ItemDeliveryRequest)` authorizes both ownership and lifecycle from the same aggregate. A caller cannot present `SessionState::Active` for a `Created` session.
- Exact replay of an accepted `delivery_ref` remains idempotent after the session leaves `Active`. Conflicting replay fails closed. Unknown or non-active states fail closed for new logical deliveries.
- Same `session_ref` with different published-release provenance, including a reused digest that enlarges, shrinks, or reorders the pinned item-version set, is `SessionMismatch`, not an idempotent replay.

This decision is as-built for the in-process domain API. Durable PostgreSQL item-delivery persistence currently compares tenant, session, release reference, digest, locale, and allowed items; it does not yet persist `instrument_version_ref`. Until that column exists, version-only rebinding is closed in the domain ledger and remains an explicit persistence gap.

## Response-event contract

Each response event contains:

- opaque event reference;
- session reference;
- item-version reference;
- server-assigned monotonic session sequence;
- client event reference/idempotency key;
- observed and received timestamps;
- response payload or encrypted object reference;
- locale and presentation context;
- schema version.

Duplicate client event references return the original outcome. Conflicting reuse of an idempotency key is rejected.

## Completion and scoring

Completion atomically freezes a `response_snapshot_ref`. Later corrections create a superseding snapshot through an audited adjudication workflow; they never mutate the original. Scoring is asynchronous and dispatched through an outbox after the completion transaction commits.

A released result references exactly one response snapshot and one scoring result. Narrative generation may complete later; the numeric result remains available with deterministic fallback text.

## Concurrency invariants

1. A session has one active lease/version for state-changing commands.
2. Response sequence is monotonic and unique within a session.
3. Completion cannot occur while required responses are unresolved unless the instrument policy explicitly allows missingness.
4. Scoring cannot begin before the response snapshot is durable.
5. Repeated completion/scoring commands are idempotent.
6. No client-provided timestamp determines authoritative ordering.

## Failure modes

- Transient database or queue failure: command fails or outbox retries; no partially committed state.
- Poison scoring job: quarantined after bounded attempts with typed terminal state.
- Client offline: responses replay by idempotency key on reconnect.
- Session expiry during offline work: policy determines grace/adjudication; the server does not silently accept after expiry.
- Dependency outage: session recording continues when safe; scoring waits without losing the snapshot.

## Data ownership

Runtime tables are private to Psychometrics Commons. Downstream consumers receive events or exports, never direct SQL access. Database objects use descriptive two-or-more-word `snake_case` names. Public identifiers are opaque and non-numeric.

## Validation

- property-based state-machine tests;
- concurrent duplicate and out-of-order response tests;
- crash testing between transaction and event publication;
- pause/resume and offline replay tests;
- immutable snapshot and supersession tests;
- end-to-end scoring dispatch contract tests;
- `tests/item_delivery_session_authority.rs` rejects detached lifecycle forgery, isolated release/version/digest/locale/item-set mismatches, same-`session_ref` / different-release delivery, and same-`session_ref` delivery after a reused digest rebinds the item set;
- `tests/session_release_binding.rs` proves session creation copies the published item-version set.

## Alternatives rejected

- **Mutable session row with arbitrary status updates:** weak auditability and race safety.
- **Synchronous scoring inside completion transaction:** increases lock time and couples availability.
- **Client-side canonical session state:** unsafe across devices and reconnects.
- **Honor-system content digest without comparing the item-version set:** a caller can reuse a well-formed SHA-256 string and enlarge or reorder the administered form.
- **Detached `SessionState` on `deliver`:** lets a caller claim `Active` while the aggregate is still `Created`, paused, or completed.

## Reversal conditions

Revisit the storage implementation if event volume demands a different backend, but retain state semantics, idempotency, immutable snapshots, and outbox guarantees. Revisit the item-delivery constructor only if a later transport still receives an authoritative `AssessmentSession` loaded by the server; never restore caller-supplied lifecycle enums or honor-system item sets.

## References

American Educational Research Association, American Psychological Association, & National Council on Measurement in Education. (2014). *Standards for educational and psychological testing*. American Educational Research Association.

International Organization for Standardization. (2022). *Information security, cybersecurity and privacy protection — Information security management systems — Requirements* (ISO/IEC 27001:2022).
