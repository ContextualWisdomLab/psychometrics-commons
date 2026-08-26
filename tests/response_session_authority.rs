//! Regression tests for server-authoritative response-ledger session binding.
//!
//! Response evidence must consult the assessment-session aggregate. Callers must
//! not be able to forge lifecycle state by passing a detached `SessionState`.

#[path = "response_support/mod.rs"]
mod response_support;

use psychometrics_commons_runtime::response::{ResponseLedger, ResponseWrite, WriteError};
use psychometrics_commons_runtime::session::SessionState;
use response_support::{active_session, created_session};

const PAYLOAD_DIGEST: &str =
    "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

fn write() -> ResponseWrite<'static> {
    ResponseWrite {
        server_event_ref: "event_response_authority_001",
        client_event_ref: "client_response_authority_001",
        item_version_ref: "item_version_001",
        payload_digest: PAYLOAD_DIGEST,
    }
}

#[test]
fn created_session_cannot_be_presented_as_active_by_the_caller() {
    let session = created_session("session_response_authority");
    let mut ledger = ResponseLedger::from_session(&session).unwrap();

    assert_eq!(session.state(), SessionState::Created);
    assert_eq!(
        ledger.record(&session, write()),
        Err(WriteError::SessionNotActive(SessionState::Created)),
        "a caller must not record responses against a Created assessment session"
    );
    assert!(ledger.is_empty());
}

#[test]
fn active_session_cannot_be_presented_as_completed_for_snapshot_freeze() {
    let session = active_session("session_response_freeze_authority");
    let ledger = ResponseLedger::from_session(&session).unwrap();

    assert_eq!(session.state(), SessionState::Active);
    assert_eq!(
        ledger.freeze(&session),
        Err(WriteError::SnapshotRequiresCompleted(SessionState::Active)),
        "a caller must not freeze a snapshot before the assessment session completes"
    );
}

#[test]
fn only_the_bound_assessment_session_can_operate_the_ledger() {
    let session = active_session("session_response_bound");
    let other = active_session("session_response_other");
    let mut ledger = ResponseLedger::from_session(&session).unwrap();

    assert_eq!(
        ledger.record(&other, write()),
        Err(WriteError::SessionMismatch)
    );
    assert!(ledger.is_empty());
    assert!(ledger.record(&session, write()).is_ok());
    assert_eq!(ledger.freeze(&other), Err(WriteError::SessionMismatch));
    assert_eq!(
        ledger.freeze_as(&other, "snapshot_response_bound"),
        Err(WriteError::SessionMismatch)
    );
}
