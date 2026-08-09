//! Integration tests for response-event idempotency and immutable snapshots.

use psychometrics_commons_runtime::response::{
    ResponseLedger, ResponseWrite, WriteError,
};
use psychometrics_commons_runtime::session::SessionState;

fn write<'a>(
    server_event_ref: &'a str,
    client_event_ref: &'a str,
    item_version_ref: &'a str,
    payload_digest: &'a str,
) -> ResponseWrite<'a> {
    ResponseWrite {
        server_event_ref,
        client_event_ref,
        item_version_ref,
        payload_digest,
    }
}

#[test]
fn active_session_assigns_monotonic_server_sequences() {
    let mut ledger = ResponseLedger::new("session_ref");

    let first = ledger
        .record(
            SessionState::Active,
            write("event_a", "client_a", "item_v1", "sha256:aaa"),
        )
        .unwrap();
    let second = ledger
        .record(
            SessionState::Active,
            write("event_b", "client_b", "item_v2", "sha256:bbb"),
        )
        .unwrap();

    assert_eq!(first.sequence(), 1);
    assert_eq!(second.sequence(), 2);
    assert_eq!(ledger.len(), 2);
    assert!(!ledger.is_empty());
}

#[test]
fn duplicate_client_event_is_idempotent_when_content_matches() {
    let mut ledger = ResponseLedger::new("session_ref");
    let original = ledger
        .record(
            SessionState::Active,
            write("event_a", "client_a", "item_v1", "sha256:aaa"),
        )
        .unwrap();
    let replay = ledger
        .record(
            SessionState::Active,
            write("ignored_new_server_ref", "client_a", "item_v1", "sha256:aaa"),
        )
        .unwrap();

    assert_eq!(replay, original);
    assert_eq!(ledger.len(), 1);
}

#[test]
fn reused_client_event_with_different_content_fails_closed() {
    let mut ledger = ResponseLedger::new("session_ref");
    ledger
        .record(
            SessionState::Active,
            write("event_a", "client_a", "item_v1", "sha256:aaa"),
        )
        .unwrap();

    let digest_error = ledger
        .record(
            SessionState::Active,
            write("event_b", "client_a", "item_v1", "sha256:changed"),
        )
        .unwrap_err();
    assert_eq!(digest_error, WriteError::IdempotencyConflict);

    let item_error = ledger
        .record(
            SessionState::Active,
            write("event_c", "client_a", "item_v2", "sha256:aaa"),
        )
        .unwrap_err();
    assert_eq!(item_error, WriteError::IdempotencyConflict);
    assert_eq!(ledger.len(), 1);
}

#[test]
fn non_active_session_cannot_accept_response_events() {
    let mut ledger = ResponseLedger::new("session_ref");

    for state in [
        SessionState::Created,
        SessionState::Paused,
        SessionState::Completed,
        SessionState::Scoring,
        SessionState::Scored,
        SessionState::Released,
        SessionState::Expired,
        SessionState::Cancelled,
        SessionState::Invalidated,
    ] {
        let error = ledger
            .record(
                state,
                write("event_a", "client_a", "item_v1", "sha256:aaa"),
            )
            .unwrap_err();
        assert_eq!(error, WriteError::SessionNotActive(state));
    }
    assert!(ledger.is_empty());
}

#[test]
fn server_and_client_references_must_be_non_empty() {
    let mut ledger = ResponseLedger::new("session_ref");

    for request in [
        write("", "client_a", "item_v1", "sha256:aaa"),
        write("event_a", "", "item_v1", "sha256:aaa"),
        write("event_a", "client_a", "", "sha256:aaa"),
        write("event_a", "client_a", "item_v1", ""),
    ] {
        assert_eq!(
            ledger.record(SessionState::Active, request).unwrap_err(),
            WriteError::EmptyReference
        );
    }
}

#[test]
fn completed_session_freezes_a_deterministic_immutable_snapshot() {
    let mut ledger = ResponseLedger::new("session_ref");
    ledger
        .record(
            SessionState::Active,
            write("event_a", "client_a", "item_v1", "sha256:aaa"),
        )
        .unwrap();
    ledger
        .record(
            SessionState::Active,
            write("event_b", "client_b", "item_v2", "sha256:bbb"),
        )
        .unwrap();

    let snapshot = ledger.freeze(SessionState::Completed).unwrap();
    assert_eq!(snapshot.session_ref(), "session_ref");
    assert_eq!(snapshot.event_count(), 2);
    assert_eq!(snapshot.last_sequence(), Some(2));
    assert_eq!(snapshot.event_refs(), ["event_a", "event_b"]);
    assert_eq!(snapshot.item_version_refs(), ["item_v1", "item_v2"]);
    assert_eq!(snapshot.payload_digests(), ["sha256:aaa", "sha256:bbb"]);
    assert_eq!(ledger.freeze(SessionState::Completed).unwrap(), snapshot);
}

#[test]
fn snapshot_freeze_requires_completed_session() {
    let ledger = ResponseLedger::new("session_ref");

    for state in [
        SessionState::Created,
        SessionState::Active,
        SessionState::Paused,
        SessionState::Scoring,
        SessionState::Scored,
        SessionState::Released,
        SessionState::Expired,
        SessionState::Cancelled,
        SessionState::Invalidated,
    ] {
        assert_eq!(
            ledger.freeze(state).unwrap_err(),
            WriteError::SnapshotRequiresCompleted(state)
        );
    }
}

#[test]
fn empty_completed_session_has_an_explicit_empty_snapshot() {
    let ledger = ResponseLedger::new("session_ref");
    let snapshot = ledger.freeze(SessionState::Completed).unwrap();

    assert_eq!(snapshot.event_count(), 0);
    assert_eq!(snapshot.last_sequence(), None);
    assert!(snapshot.event_refs().is_empty());
    assert!(snapshot.item_version_refs().is_empty());
    assert!(snapshot.payload_digests().is_empty());
}
