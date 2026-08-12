//! Regression coverage for opaque response-event reference validation.

use psychometrics_commons_runtime::response::{ResponseLedger, ResponseWrite, WriteError};
use psychometrics_commons_runtime::session::SessionState;

fn write<'a>(
    server_event_ref: &'a str,
    client_event_ref: &'a str,
    item_version_ref: &'a str,
) -> ResponseWrite<'a> {
    ResponseWrite {
        server_event_ref,
        client_event_ref,
        item_version_ref,
        payload_digest: "sha256:response-payload-a",
    }
}

#[test]
fn response_identity_references_reject_numeric_like_values() {
    for request in [
        write("12345", "client_event_a", "item_version_a"),
        write("server_event_a", "1.25e3", "item_version_a"),
        write("server_event_a", "client_event_a", "１２３４５"),
    ] {
        let mut ledger = ResponseLedger::new("session_ref");
        assert_eq!(
            ledger.record(SessionState::Active, request),
            Err(WriteError::InvalidReference)
        );
        assert!(ledger.is_empty());
    }
}

#[test]
fn response_identity_references_are_canonicalized_before_idempotency_checks() {
    let mut ledger = ResponseLedger::new("session_ref");
    let original = ledger
        .record(
            SessionState::Active,
            write(" server_event_a ", " client_event_a ", " item_version_a "),
        )
        .unwrap();

    assert_eq!(original.server_event_ref(), "server_event_a");
    assert_eq!(original.client_event_ref(), "client_event_a");
    assert_eq!(original.item_version_ref(), "item_version_a");

    let replay = ledger
        .record(
            SessionState::Completed,
            write("ignored_server_ref", "client_event_a", "item_version_a"),
        )
        .unwrap();
    assert_eq!(replay, original);
    assert_eq!(ledger.len(), 1);
}
