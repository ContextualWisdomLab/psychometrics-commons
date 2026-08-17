//! Durable response-snapshot identity must fail closed before persistence.

use psychometrics_commons_runtime::response::{ResponseLedger, WriteError};
use psychometrics_commons_runtime::session::SessionState;

#[test]
fn durable_snapshot_reference_must_be_opaque() {
    let ledger = ResponseLedger::new("session_ref").unwrap();

    for snapshot_ref in ["   ", "12345", "1.25e3", "１２３４５"] {
        let error = ledger
            .freeze_as(SessionState::Completed, snapshot_ref)
            .unwrap_err();
        assert_eq!(error, WriteError::InvalidReference);
    }
}

#[test]
fn durable_snapshot_reference_rejects_whitespace_aliases() {
    let ledger = ResponseLedger::new("session_ref").unwrap();

    for snapshot_ref in [
        " snapshot_ref_a",
        "snapshot_ref_a ",
        "\tsnapshot_ref_a",
        "snapshot_ref_a\n",
        "\u{00a0}snapshot_ref_a",
    ] {
        assert_eq!(
            ledger.freeze_as(SessionState::Completed, snapshot_ref),
            Err(WriteError::InvalidReference),
            "snapshot identity must reject non-canonical spelling {snapshot_ref:?}",
        );
    }

    let snapshot = ledger
        .freeze_as(SessionState::Completed, "snapshot_ref_a")
        .unwrap();
    assert_eq!(snapshot.snapshot_ref(), Some("snapshot_ref_a"));
}
