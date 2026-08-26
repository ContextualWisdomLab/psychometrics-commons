//! Regression coverage for exact response-payload digest identity.

#[path = "response_support/mod.rs"]
mod response_support;

use psychometrics_commons_runtime::response::{ResponseLedger, ResponseWrite, WriteError};
use response_support::{active_session, completed_session};

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
    let session = active_session("session_ref");
    let mut ledger = ResponseLedger::from_session(&session).unwrap();
    let original = ledger.record(&session, write(DIGEST_A)).unwrap();
    let completed = completed_session("session_ref");

    assert_eq!(
        ledger.record(
            &completed,
            write(" sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa "),
        ),
        Err(WriteError::InvalidPayloadDigest)
    );
    assert_eq!(original.payload_digest(), DIGEST_A);
    assert_eq!(ledger.len(), 1);
}
