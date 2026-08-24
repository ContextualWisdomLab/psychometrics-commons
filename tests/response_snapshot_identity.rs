//! Durable response-snapshot identity must fail closed before persistence.

#[path = "response_support/mod.rs"]
mod response_support;

use psychometrics_commons_runtime::response::{ResponseLedger, WriteError};
use response_support::completed_session;

#[test]
fn durable_snapshot_reference_must_be_opaque() {
    let session = completed_session("session_ref");
    let ledger = ResponseLedger::from_session(&session).unwrap();

    for snapshot_ref in ["   ", "12345", "1.25e3", "１２３４５"] {
        let error = ledger.freeze_as(&session, snapshot_ref).unwrap_err();
        assert_eq!(error, WriteError::InvalidReference);
    }
}

#[test]
fn durable_snapshot_reference_is_trimmed_before_becoming_identity() {
    let session = completed_session("session_ref");
    let ledger = ResponseLedger::from_session(&session).unwrap();
    let snapshot = ledger.freeze_as(&session, " snapshot_ref_a ").unwrap();

    assert_eq!(snapshot.snapshot_ref(), Some("snapshot_ref_a"));
}
