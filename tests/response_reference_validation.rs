//! Regression coverage for opaque response-event reference validation.

#[path = "response_support/mod.rs"]
mod response_support;

use psychometrics_commons_runtime::response::{ResponseLedger, ResponseWrite, WriteError};
use response_support::{active_session, completed_session};

const PAYLOAD_DIGEST: &str =
    "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn write<'a>(
    server_event_ref: &'a str,
    client_event_ref: &'a str,
    item_version_ref: &'a str,
) -> ResponseWrite<'a> {
    ResponseWrite {
        server_event_ref,
        client_event_ref,
        item_version_ref,
        payload_digest: PAYLOAD_DIGEST,
    }
}

#[test]
fn response_ledger_session_reference_must_be_opaque() {
    for session_ref in ["", "   ", "12345", "1.25e3", "１２３４５"] {
        assert_eq!(
            ResponseLedger::new(session_ref),
            Err(WriteError::InvalidReference)
        );
    }
}

#[test]
fn response_identity_references_reject_numeric_like_values() {
    let session = active_session("session_ref");
    for request in [
        write("12345", "client_event_a", "item_version_a"),
        write("server_event_a", "1.25e3", "item_version_a"),
        write("server_event_a", "client_event_a", "１２３４５"),
    ] {
        let mut ledger = ResponseLedger::from_session(&session).unwrap();
        assert_eq!(
            ledger.record(&session, request),
            Err(WriteError::InvalidReference)
        );
        assert!(ledger.is_empty());
    }
}

#[test]
fn response_identity_references_are_canonicalized_before_idempotency_checks() {
    let session = active_session("session_ref");
    let mut ledger = ResponseLedger::from_session(&session).unwrap();
    let original = ledger
        .record(
            &session,
            write(" server_event_a ", " client_event_a ", " item_version_a "),
        )
        .unwrap();

    assert_eq!(original.server_event_ref(), "server_event_a");
    assert_eq!(original.client_event_ref(), "client_event_a");
    assert_eq!(original.item_version_ref(), "item_version_a");

    let completed = completed_session("session_ref");
    let replay = ledger
        .record(
            &completed,
            write("ignored_server_ref", "client_event_a", "item_version_a"),
        )
        .unwrap();
    assert_eq!(replay, original);
    assert_eq!(ledger.len(), 1);
    assert_eq!(
        ledger
            .freeze_as(&completed, "snapshot_ref_a")
            .unwrap()
            .session_ref(),
        "session_ref"
    );
}
