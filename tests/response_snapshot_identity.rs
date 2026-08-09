//! Durable response-snapshot identity must fail closed before persistence.

use psychometrics_commons_runtime::response::{ResponseLedger, WriteError};
use psychometrics_commons_runtime::session::SessionState;

#[test]
fn durable_snapshot_reference_must_not_be_blank() {
    let ledger = ResponseLedger::new("session_ref");

    let error = ledger
        .freeze_as(SessionState::Completed, "   ")
        .unwrap_err();

    assert_eq!(error, WriteError::EmptyReference);
}
