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

Published releases are immutable. Editing content creates a new `instrument_version_ref`. Suspension blocks new sessions but does not invalidate existing result provenance. Retirement blocks new sessions permanently unless a new release is published. A created session loaded from durable storage must restore the copied release/version/digest/locale identity without re-checking whether the release currently accepts new sessions. Starting a *new* session must call `AssessmentSession::new` or `AssessmentSession::from_currently_published_manifest` from a currently published release (`created_session_for_start` / `start_created_assessment_session` / `start_created_assessment_session_from_stored_release`). Durable start locks the stored `instrument_release` row (`SELECT … FOR UPDATE`) in the same transaction and fails closed unless stored state is `published` and locale/digest match, or an exact stored start already exists. First insert through `persist_assessment_session` takes the same lock, so a reconstituted Created aggregate cannot insert after later persist Suspend or Retire. Exact replay of an already stored Created row after that later persist stays legal. Load is not authorization. Later lifecycle commands persist as append-only history and replay on load so Activate/Pause/Resume survive process restart.

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

The response ledger is bound to one authoritative `AssessmentSession`. Recording and snapshot-freeze operations derive lifecycle state from that aggregate and reject a different `session_ref`; callers do not supply a detached `SessionState` as authority. Exact replay of an already accepted client event remains idempotent after collection closes, but a new event is accepted only while the bound aggregate is `active`, and a snapshot freezes only while it is `completed`.

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
9. A new session starts only from a currently published release through `created_session_for_start` / `start_created_assessment_session` / `start_created_assessment_session_from_stored_release`. Durable start locks stored `instrument_release.publication_state` in the same transaction. Reconstituting stored identity is load, not start.
10. Exact replay of an already stored start after a later persist Suspend or Retire returns the original session. A new `session_ref` or rebound participant/release identity after that later persist fails closed.
11. First insert through `persist_assessment_session` locks stored `instrument_release.publication_state` in the same transaction. A reconstituted Created aggregate cannot insert after later persist Suspend or Retire. When that lock finds a missing or unpublished release, persist still classifies an exact stored Created row as duplicate so a concurrent retry cannot miss the committed first insert. Exact replay of an already stored Created row after that later persist stays legal.
12. Response acceptance and snapshot freeze consult the bound `AssessmentSession`; a detached lifecycle enum or a different session aggregate cannot authorize either operation.

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
- response-session authority tests (`created_session_cannot_be_presented_as_active_by_the_caller`, `active_session_cannot_be_presented_as_completed_for_snapshot_freeze`, `only_the_bound_assessment_session_can_operate_the_ledger`);
- crash testing between transaction and event publication;
- pause/resume and offline replay tests;
- stale shorter command-history persist fail-closed tests (`stale_shorter_command_history_cannot_rewind_paused_projection`);
- concurrent header-row lock tests (`command_persist_locks_session_header_until_caller_commits`);
- published-release start-boundary tests (`start_uses_published_release_and_never_reconstitution`, `start_rejects_unpublished_release_and_locale_mismatch`, `start_persists_published_release_and_rejects_unpublished_before_insert`, `start_from_stored_release_uses_database_publication_state`, `start_from_published_snapshot_matches_new_and_rejects_locale_mismatch`, `start_replays_exact_session_after_stored_release_is_suspended`, `exact_start_identity_matches_stored_session_and_rejects_rebind`, `persist_rejects_reconstituted_first_insert_after_stored_suspend`, `persist_replays_exact_created_row_after_stored_suspend`, `persist_rejects_first_insert_when_stored_release_is_missing`, `persist_maps_unpublished_stored_release_to_first_insert_seal`, `first_insert_seal_replays_only_publication_boundary_errors`);
- immutable snapshot and supersession tests;
- end-to-end scoring dispatch contract tests.

## Alternatives rejected

- **Mutable session row with arbitrary status updates:** weak auditability and race safety.
- **Synchronous scoring inside completion transaction:** increases lock time and couples availability.
- **Client-side canonical session state:** unsafe across devices and reconnects.

## Reversal conditions

Revisit the storage implementation if event volume demands a different backend, but retain state semantics, idempotency, immutable snapshots, and outbox guarantees.

## Standards basis

The hosted session machine exists so administration, completion, scoring dispatch, and result release stay server-authoritative. The *Standards for Educational and Psychological Testing* treat test administration, scoring, reporting, and the rights of test takers as professional obligations: participants must encounter a controlled administration, scores must be produced from the intended procedure, and a released result must match the evidence that supports it (American Educational Research Association [AERA], American Psychological Association [APA], & National Council on Measurement in Education [NCME], 2014). Kane (2013) requires that a released result carry an interpretation/use argument rather than an ad hoc status change.

Numeric scoring is dispatched to the ADR-0004/`fast-mlsirm` kernel only after an immutable response snapshot exists (Embretson & Reise, 2000; Lord, 1980). The session machine does not compute IRT estimates. When the published instrument uses the upstream multilevel latent-space item-response model, that engine is Jeon et al. (2021) as consumed through `fast-mlsirm`.

## References

American Educational Research Association, American Psychological Association, & National Council on Measurement in Education. (2014). *Standards for educational and psychological testing*. American Educational Research Association. https://www.testingstandards.net/

Embretson, S. E., & Reise, S. P. (2000). *Item response theory for psychologists*. Lawrence Erlbaum Associates.

Fowler, M. (2005, December 12). *Event sourcing*. https://martinfowler.com/eaaDev/EventSourcing.html

Hohpe, G., & Woolf, B. (2003). *Enterprise integration patterns: Designing, building, and deploying messaging solutions*. Addison-Wesley.

Jeon, M., Jin, I. H., Schweinberger, M., & Baugh, S. (2021). Mapping unobserved item-respondent interactions: A latent space item response model with interaction map. *Psychometrika, 86*(2), 378–403. https://doi.org/10.1007/s11336-021-09776-z

Kane, M. T. (2013). Validating the interpretations and uses of test scores. *Journal of Educational Measurement, 50*(1), 1–73. https://doi.org/10.1111/jedm.12000

Lord, F. M. (1980). *Applications of item response theory to practical testing problems*. Lawrence Erlbaum Associates.

PostgreSQL Global Development Group. (2026). *Explicit locking*. In *PostgreSQL 18 documentation*. https://www.postgresql.org/docs/18/explicit-locking.html

PostgreSQL Global Development Group. (2026). *PostgreSQL 18 documentation*. https://www.postgresql.org/docs/18/index.html
