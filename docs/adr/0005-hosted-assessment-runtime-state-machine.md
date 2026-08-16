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

Published releases are immutable. Editing content creates a new `instrument_version_ref`. Suspension blocks new sessions but does not invalidate existing result provenance. Retirement blocks new sessions permanently unless a new release is published. A created session loaded from durable storage must restore the copied release/version/digest/locale identity without re-checking whether the release currently accepts new sessions. Later lifecycle commands persist as append-only history and replay on load so Activate/Pause/Resume survive process restart.

### Assessment session

```text
created -> active <-> paused -> completed -> scoring -> scored -> released
```

Terminal alternatives: `expired`, `cancelled`, `invalidated`.

Only the runtime may transition session state. Clients request commands; they do not submit the target state directly.

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
7. Persisting session command history is append-only. A shorter in-memory history than already stored is conflicting replay and must not rewind the current-state projection.
8. Command-history persist locks the `assessment_session` header row (`SELECT … FOR UPDATE`) before inserting or counting commands, so a concurrent shorter-history writer cannot count a prefix under `READ COMMITTED` and then overwrite a later projection.

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
- stale shorter command-history persist fail-closed tests (`stale_shorter_command_history_cannot_rewind_paused_projection`);
- concurrent header-row lock tests (`command_persist_locks_session_header_until_caller_commits`);
- immutable snapshot and supersession tests;
- end-to-end scoring dispatch contract tests.

## Alternatives rejected

- **Mutable session row with arbitrary status updates:** weak auditability and race safety.
- **Synchronous scoring inside completion transaction:** increases lock time and couples availability.
- **Client-side canonical session state:** unsafe across devices and reconnects.

## Reversal conditions

Revisit the storage implementation if event volume demands a different backend, but retain state semantics, idempotency, immutable snapshots, and outbox guarantees.

## References

Fowler, M. (2005, December 12). *Event sourcing*. https://martinfowler.com/eaaDev/EventSourcing.html

Hohpe, G., & Woolf, B. (2003). *Enterprise integration patterns: Designing, building, and deploying messaging solutions*. Addison-Wesley.

PostgreSQL Global Development Group. (2026). *PostgreSQL 18 documentation*. https://www.postgresql.org/docs/18/index.html

PostgreSQL Global Development Group. (2026). *Explicit locking*. In *PostgreSQL 18 documentation*. https://www.postgresql.org/docs/18/explicit-locking.html
