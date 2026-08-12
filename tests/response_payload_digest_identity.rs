//! Regression coverage for exact response-payload digest identity.

use psychometrics_commons_runtime::response::{ResponseLedger, ResponseWrite, WriteError};
use psychometrics_commons_runtime::session::SessionState;

fn write(payload_digest: &str) -> ResponseWrite<'_> {
    ResponseWrite {
        server_event_ref: "server_event_a",
        client_event_ref: "client_event_a",
        item_version_ref: "item_version_a",
        payload_digest,
    }
}

#[test]
fn nonblank_payload_digest_whitespace_is_not_canonicalized_into_replay_identity() {
    let mut ledger = ResponseLedger::new("session_ref").unwrap();
    let original = ledger
        .record(SessionState::Active, write("sha256:response-payload-a"))
        .unwrap();

    assert_eq!(
        ledger.record(
            SessionState::Completed,
            write(" sha256:response-payload-a "),
        ),
        Err(WriteError::IdempotencyConflict)
    );
    assert_eq!(original.payload_digest(), "sha256:response-payload-a");
    assert_eq!(ledger.len(), 1);
}
