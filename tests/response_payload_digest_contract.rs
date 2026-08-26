//! Fail-closed canonical digest coverage for response-event payload identity.

#[path = "response_support/mod.rs"]
mod response_support;

use psychometrics_commons_runtime::response::{ResponseLedger, ResponseWrite, WriteError};
use response_support::{active_session, completed_session};

const VALID_DIGEST: &str =
    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn request(payload_digest: &str) -> ResponseWrite<'_> {
    ResponseWrite {
        server_event_ref: "server_event_alpha",
        client_event_ref: "client_event_alpha",
        item_version_ref: "item_version_alpha",
        payload_digest,
    }
}

#[test]
fn canonical_lowercase_sha256_payload_digest_is_accepted() {
    let session = active_session("session_alpha");
    let mut ledger = ResponseLedger::from_session(&session).unwrap();
    let event = ledger.record(&session, request(VALID_DIGEST)).unwrap();

    assert_eq!(event.payload_digest(), VALID_DIGEST);
}

#[test]
fn blank_payload_digest_preserves_the_existing_empty_reference_error() {
    let session = active_session("session_alpha");
    let mut ledger = ResponseLedger::from_session(&session).unwrap();
    assert_eq!(
        ledger.record(&session, request("")),
        Err(WriteError::EmptyReference)
    );
    assert!(ledger.is_empty());
}

#[test]
fn malformed_nonblank_payload_digests_fail_closed_before_response_mutation() {
    let session = active_session("session_alpha");
    for invalid in [
        "response-alpha",
        "sha256:response-alpha",
        "sha512:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        "sha256:0123456789abcdef",
        "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdeg",
        "sha256:0123456789ABCDEF0123456789abcdef0123456789abcdef0123456789abcdef",
        " sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef ",
    ] {
        let mut ledger = ResponseLedger::from_session(&session).unwrap();
        assert_eq!(
            ledger.record(&session, request(invalid)),
            Err(WriteError::InvalidPayloadDigest),
            "expected malformed payload digest to fail closed: {invalid:?}"
        );
        assert!(ledger.is_empty());
    }
}

#[test]
fn replay_identity_uses_the_exact_canonical_digest() {
    let session = active_session("session_alpha");
    let mut ledger = ResponseLedger::from_session(&session).unwrap();
    let first = ledger.record(&session, request(VALID_DIGEST)).unwrap();
    let completed = completed_session("session_alpha");
    let replay = ledger.record(&completed, request(VALID_DIGEST)).unwrap();

    assert_eq!(replay, first);
    assert_eq!(ledger.len(), 1);
}

#[test]
fn invalid_digest_error_has_a_stable_safe_message() {
    assert_eq!(
        WriteError::InvalidPayloadDigest.to_string(),
        "response payload digest must be canonical lowercase sha256 evidence"
    );
}
