//! Regression coverage for exact response-payload digest identity.

use psychometrics_commons_runtime::response::{ResponseLedger, ResponseWrite, WriteError};
use psychometrics_commons_runtime::session::SessionState;

const DIGEST_A: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn write(payload_digest: &str) -> ResponseWrite<'_> {
    ResponseWrite {
        server_event_ref: "server_event_a",
        client_event_ref: "client_event_a",
        item_version_ref: "item_version_a",
        payload_digest,
    }
}

#[test]
fn nonblank_payload_digest_whitespace_is_rejected_before_replay_classification() {
    let mut ledger = ResponseLedger::new("session_ref").unwrap();
    let original = ledger
        .record(SessionState::Active, write(DIGEST_A))
        .unwrap();

    assert_eq!(
        ledger.record(
            SessionState::Completed,
            write(" sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa "),
        ),
        Err(WriteError::InvalidPayloadDigest)
    );
    assert_eq!(original.payload_digest(), DIGEST_A);
    assert_eq!(ledger.len(), 1);
}
