//! Regression contracts for opaque data-rights references and terminal replays.

use psychometrics_commons_runtime::data_rights::{
    DataRightsError, DataRightsRequest, DataRightsRequestKind, DataRightsState,
};

#[test]
fn numeric_reference_forms_are_rejected_fail_closed() {
    for invalid_reference in ["-1", "+1", "1.5", "1e3", "１２３", "١٢٣"] {
        assert_eq!(
            DataRightsRequest::new(
                invalid_reference,
                "tenant_ref",
                "participant_ref",
                DataRightsRequestKind::Export,
                "account_data_scope",
                1_000,
            ),
            Err(DataRightsError::InvalidReference),
            "numeric-looking public reference {invalid_reference:?} must be rejected",
        );
    }

    let valid = DataRightsRequest::new(
        "request_123",
        "tenant_ref",
        "participant_ref",
        DataRightsRequestKind::Export,
        "account_data_scope",
        1_000,
    )
    .unwrap();
    assert_eq!(valid.request_ref(), "request_123");
}

#[test]
fn exact_replays_after_partial_completion_do_not_reopen_the_request() {
    let mut request = DataRightsRequest::new(
        "deletion_request_ref",
        "tenant_ref",
        "participant_ref",
        DataRightsRequestKind::Deletion,
        "participant_data_scope",
        2_000,
    )
    .unwrap();

    request.verify_identity("verification_ref", 2_100).unwrap();
    request.start_processing("operation_ref", 2_200).unwrap();
    request
        .complete("completion_ref", &["legal_retention_scope"], 2_300)
        .unwrap();
    assert_eq!(request.state(), DataRightsState::PartiallyCompleted);

    request.verify_identity("verification_ref", 2_100).unwrap();
    assert_eq!(request.state(), DataRightsState::PartiallyCompleted);
    assert_eq!(request.completed_at_unix_ms(), Some(2_300));

    request.start_processing("operation_ref", 2_200).unwrap();
    assert_eq!(request.state(), DataRightsState::PartiallyCompleted);
    assert_eq!(request.completed_at_unix_ms(), Some(2_300));
}
