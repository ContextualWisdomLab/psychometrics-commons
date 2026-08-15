//! Regression coverage for opaque response-event reference validation.

use psychometrics_commons_runtime::response::{ResponseLedger, ResponseWrite, WriteError};
use psychometrics_commons_runtime::session::SessionState;

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
    for session_ref in [
        "",
        "   ",
        "12345",
        "1.25e3",
        "１２３４５",
        " session_ref",
        "session_ref ",
        "session\nref",
    ] {
        assert_eq!(
            ResponseLedger::new(session_ref),
            Err(WriteError::InvalidReference)
        );
    }
}

#[test]
fn response_identity_references_reject_numeric_like_values() {
    for request in [
        write("12345", "client_event_a", "item_version_a"),
        write("server_event_a", "1.25e3", "item_version_a"),
        write("server_event_a", "client_event_a", "１２３４５"),
    ] {
        let mut ledger = ResponseLedger::new("session_ref").unwrap();
        assert_eq!(
            ledger.record(SessionState::Active, request),
            Err(WriteError::InvalidReference)
        );
        assert!(ledger.is_empty());
    }
}

#[test]
fn response_identity_references_reject_whitespace_aliases_before_idempotency_checks() {
    let mut ledger = ResponseLedger::new("session_ref").unwrap();
    let original = ledger
        .record(
            SessionState::Active,
            write("server_event_a", "client_event_a", "item_version_a"),
        )
        .unwrap();

    for request in [
        write(" server_event_b ", "client_event_b", "item_version_a"),
        write("server_event_b", " client_event_a ", "item_version_a"),
        write("server_event_b", "client_event_b", " item_version_a "),
        write("server\nevent_b", "client_event_b", "item_version_a"),
    ] {
        assert_eq!(
            ledger.record(SessionState::Active, request),
            Err(WriteError::InvalidReference)
        );
    }

    let replay = ledger
        .record(
            SessionState::Completed,
            write("ignored_server_ref", "client_event_a", "item_version_a"),
        )
        .unwrap();
    assert_eq!(replay, original);
    assert_eq!(ledger.len(), 1);
    assert_eq!(
        ledger
            .freeze_as(SessionState::Completed, "snapshot_ref_a")
            .unwrap()
            .session_ref(),
        "session_ref"
    );
}
