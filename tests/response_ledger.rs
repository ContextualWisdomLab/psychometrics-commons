//! Integration tests for response-event idempotency and immutable snapshots.

use psychometrics_commons_runtime::response::{ResponseLedger, ResponseWrite, WriteError};
use psychometrics_commons_runtime::session::SessionState;

const DIGEST_A: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const DIGEST_B: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const DIGEST_CHANGED: &str =
    "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

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
    let mut ledger = ResponseLedger::new("session_ref").unwrap();

    let first = ledger
        .record(
            SessionState::Active,
            write("event_a", "client_a", "item_v1", DIGEST_A),
        )
        .unwrap();
    let second = ledger
        .record(
            SessionState::Active,
            write("event_b", "client_b", "item_v2", DIGEST_B),
        )
        .unwrap();

    assert_eq!(first.server_event_ref(), "event_a");
    assert_eq!(first.client_event_ref(), "client_a");
    assert_eq!(first.item_version_ref(), "item_v1");
    assert_eq!(first.payload_digest(), DIGEST_A);
    assert_eq!(first.sequence(), 1);
    assert_eq!(second.sequence(), 2);
    assert_eq!(ledger.len(), 2);
    assert!(!ledger.is_empty());
}

#[test]
fn duplicate_client_event_is_idempotent_when_content_matches() {
    let mut ledger = ResponseLedger::new("session_ref").unwrap();
    let original = ledger
        .record(
            SessionState::Active,
            write("event_a", "client_a", "item_v1", DIGEST_A),
        )
        .unwrap();
    let replay = ledger
        .record(
            SessionState::Active,
            write("ignored_new_server_ref", "client_a", "item_v1", DIGEST_A),
        )
        .unwrap();

    assert_eq!(replay, original);
    assert_eq!(ledger.len(), 1);
}

#[test]
fn exact_response_replay_remains_idempotent_after_collection_closes() {
    let mut ledger = ResponseLedger::new("session_ref").unwrap();
    let original = ledger
        .record(
            SessionState::Active,
            write("event_a", "client_a", "item_v1", DIGEST_A),
        )
        .unwrap();

    for state in [
        SessionState::Paused,
        SessionState::Completed,
        SessionState::Scoring,
        SessionState::Scored,
        SessionState::Released,
        SessionState::Expired,
        SessionState::Cancelled,
        SessionState::Invalidated,
    ] {
        let replay = ledger
            .record(
                state,
                write("ignored_new_server_ref", "client_a", "item_v1", DIGEST_A),
            )
            .unwrap();
        assert_eq!(
            replay, original,
            "exact replay must survive state {state:?}"
        );
    }
    assert_eq!(ledger.len(), 1);
}

#[test]
fn conflicting_response_replay_remains_fail_closed_after_collection_closes() {
    let mut ledger = ResponseLedger::new("session_ref").unwrap();
    ledger
        .record(
            SessionState::Active,
            write("event_a", "client_a", "item_v1", DIGEST_A),
        )
        .unwrap();

    for state in [
        SessionState::Paused,
        SessionState::Completed,
        SessionState::Scoring,
        SessionState::Scored,
        SessionState::Released,
        SessionState::Expired,
        SessionState::Cancelled,
        SessionState::Invalidated,
    ] {
        let digest_error = ledger
            .record(
                state,
                write(
                    "ignored_new_server_ref",
                    "client_a",
                    "item_v1",
                    DIGEST_CHANGED,
                ),
            )
            .unwrap_err();
        assert_eq!(digest_error, WriteError::IdempotencyConflict);

        let item_error = ledger
            .record(
                state,
                write("ignored_new_server_ref", "client_a", "item_v2", DIGEST_A),
            )
            .unwrap_err();
        assert_eq!(item_error, WriteError::IdempotencyConflict);
    }
    assert_eq!(ledger.len(), 1);
}

#[test]
fn reused_client_event_with_different_content_fails_closed() {
    let mut ledger = ResponseLedger::new("session_ref").unwrap();
    ledger
        .record(
            SessionState::Active,
            write("event_a", "client_a", "item_v1", DIGEST_A),
        )
        .unwrap();

    let digest_error = ledger
        .record(
            SessionState::Active,
            write("event_b", "client_a", "item_v1", DIGEST_CHANGED),
        )
        .unwrap_err();
    assert_eq!(digest_error, WriteError::IdempotencyConflict);

    let item_error = ledger
        .record(
            SessionState::Active,
            write("event_c", "client_a", "item_v2", DIGEST_A),
        )
        .unwrap_err();
    assert_eq!(item_error, WriteError::IdempotencyConflict);
    assert_eq!(ledger.len(), 1);
}

#[test]
fn server_event_reference_cannot_identify_two_different_events() {
    let mut ledger = ResponseLedger::new("session_ref").unwrap();
    ledger
        .record(
            SessionState::Active,
            write("event_a", "client_a", "item_v1", DIGEST_A),
        )
        .unwrap();

    let error = ledger
        .record(
            SessionState::Active,
            write("event_a", "client_b", "item_v2", DIGEST_B),
        )
        .unwrap_err();

    assert_eq!(error, WriteError::ServerReferenceConflict);
    assert_eq!(ledger.len(), 1);
}

#[test]
fn non_active_session_cannot_accept_response_events() {
    let mut ledger = ResponseLedger::new("session_ref").unwrap();

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
            .record(state, write("event_a", "client_a", "item_v1", DIGEST_A))
            .unwrap_err();
        assert_eq!(error, WriteError::SessionNotActive(state));
    }
    assert!(ledger.is_empty());
}

#[test]
fn response_identity_references_and_payload_digest_fail_closed_when_blank() {
    let mut ledger = ResponseLedger::new("session_ref").unwrap();

    for request in [
        write("", "client_a", "item_v1", DIGEST_A),
        write("event_a", "", "item_v1", DIGEST_A),
        write("event_a", "client_a", "", DIGEST_A),
    ] {
        assert_eq!(
            ledger.record(SessionState::Active, request).unwrap_err(),
            WriteError::InvalidReference
        );
    }

    assert_eq!(
        ledger
            .record(
                SessionState::Active,
                write("event_a", "client_a", "item_v1", "   "),
            )
            .unwrap_err(),
        WriteError::EmptyReference
    );
}

#[test]
fn completed_session_freezes_a_deterministic_immutable_snapshot() {
    let mut ledger = ResponseLedger::new("session_ref").unwrap();
    ledger
        .record(
            SessionState::Active,
            write("event_a", "client_a", "item_v1", DIGEST_A),
        )
        .unwrap();
    ledger
        .record(
            SessionState::Active,
            write("event_b", "client_b", "item_v2", DIGEST_B),
        )
        .unwrap();

    let snapshot = ledger.freeze(SessionState::Completed).unwrap();
    assert_eq!(snapshot.session_ref(), "session_ref");
    assert_eq!(snapshot.event_count(), 2);
    assert_eq!(snapshot.last_sequence(), Some(2));
    assert_eq!(snapshot.event_refs(), ["event_a", "event_b"]);
    assert_eq!(snapshot.item_version_refs(), ["item_v1", "item_v2"]);
    assert_eq!(snapshot.payload_digests(), [DIGEST_A, DIGEST_B]);
    assert_eq!(ledger.freeze(SessionState::Completed).unwrap(), snapshot);
}

#[test]
fn snapshot_freeze_requires_completed_session() {
    let ledger = ResponseLedger::new("session_ref").unwrap();

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
    let ledger = ResponseLedger::new("session_ref").unwrap();
    let snapshot = ledger.freeze(SessionState::Completed).unwrap();

    assert_eq!(snapshot.event_count(), 0);
    assert_eq!(snapshot.last_sequence(), None);
    assert!(snapshot.event_refs().is_empty());
    assert!(snapshot.item_version_refs().is_empty());
    assert!(snapshot.payload_digests().is_empty());
}

#[test]
fn write_errors_have_stable_human_readable_context() {
    assert_eq!(
        WriteError::SessionNotActive(SessionState::Paused).to_string(),
        "session Paused cannot accept response events"
    );
    assert_eq!(
        WriteError::InvalidReference.to_string(),
        "response identity references must be opaque non-numeric values"
    );
    assert_eq!(
        WriteError::EmptyReference.to_string(),
        "response payload digest must not be empty"
    );
    assert_eq!(
        WriteError::InvalidPayloadDigest.to_string(),
        "response payload digest must be canonical lowercase sha256 evidence"
    );
    assert_eq!(
        WriteError::IdempotencyConflict.to_string(),
        "client event reference was already used for different response content"
    );
    assert_eq!(
        WriteError::ServerReferenceConflict.to_string(),
        "server event reference was already used by another response event"
    );
    assert_eq!(
        WriteError::SnapshotRequiresCompleted(SessionState::Active).to_string(),
        "response snapshot requires Completed session state, found Active"
    );
    assert_eq!(
        WriteError::CorruptSnapshotEvidence.to_string(),
        "stored response snapshot sequence does not match the reconstructed prefix"
    );
}
