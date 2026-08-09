//! Integration contract for participant export and deletion requests.

use psychometrics_commons_runtime::data_rights::{
    DataRightsError, DataRightsRequest, DataRightsRequestKind, DataRightsState,
};

#[test]
fn export_request_requires_verified_identity_before_processing() {
    let mut request = DataRightsRequest::new(
        " export_request_ref ",
        " participant_ref ",
        DataRightsRequestKind::Export,
        " account_data_scope ",
        1_000,
    )
    .unwrap();

    assert_eq!(request.request_ref(), "export_request_ref");
    assert_eq!(request.participant_ref(), "participant_ref");
    assert_eq!(request.scope_ref(), "account_data_scope");
    assert_eq!(request.kind(), DataRightsRequestKind::Export);
    assert_eq!(request.state(), DataRightsState::Requested);
    assert_eq!(
        request.start_processing("operation_ref", 1_100),
        Err(DataRightsError::IdentityVerificationRequired)
    );

    request
        .verify_identity("verification_evidence_ref", 1_050)
        .unwrap();
    assert_eq!(request.state(), DataRightsState::IdentityVerified);
    request.start_processing("operation_ref", 1_100).unwrap();
    assert_eq!(request.state(), DataRightsState::Processing);
    request.complete("completion_evidence_ref", &[], 1_200).unwrap();
    assert_eq!(request.state(), DataRightsState::Completed);
    assert_eq!(request.retained_scope_refs(), &[] as &[String]);
}

#[test]
fn deletion_completion_preserves_legal_retention_exceptions() {
    let mut request = DataRightsRequest::new(
        "deletion_request_ref",
        "participant_ref",
        DataRightsRequestKind::Deletion,
        "participant_product_data",
        2_000,
    )
    .unwrap();
    request.verify_identity("identity_evidence_ref", 2_050).unwrap();
    request.start_processing("deletion_operation_ref", 2_100).unwrap();
    request
        .complete(
            "deletion_completion_ref",
            &[" tax_record_retention ", "audit_evidence_retention"],
            2_200,
        )
        .unwrap();

    assert_eq!(request.state(), DataRightsState::PartiallyCompleted);
    assert_eq!(
        request.retained_scope_refs(),
        &["tax_record_retention".to_owned(), "audit_evidence_retention".to_owned()]
    );
}

#[test]
fn export_completion_rejects_deletion_retention_exceptions() {
    let mut request = DataRightsRequest::new(
        "export_request_ref",
        "participant_ref",
        DataRightsRequestKind::Export,
        "account_data_scope",
        3_000,
    )
    .unwrap();
    request.verify_identity("verification_ref", 3_050).unwrap();
    request.start_processing("operation_ref", 3_100).unwrap();

    assert_eq!(
        request.complete(
            "completion_ref",
            &["legal_retention_scope"],
            3_200,
        ),
        Err(DataRightsError::RetentionExceptionNotAllowed)
    );
}

#[test]
fn lifecycle_is_monotonic_and_terminal_states_are_fail_closed() {
    let mut request = DataRightsRequest::new(
        "deletion_request_ref",
        "participant_ref",
        DataRightsRequestKind::Deletion,
        "participant_data_scope",
        4_000,
    )
    .unwrap();

    assert_eq!(
        request.verify_identity("verification_ref", 3_999),
        Err(DataRightsError::NonMonotonicTimestamp)
    );
    request.verify_identity("verification_ref", 4_000).unwrap();
    request.start_processing("operation_ref", 4_000).unwrap();
    request.complete("completion_ref", &[], 4_000).unwrap();
    assert_eq!(request.state(), DataRightsState::Completed);
    assert_eq!(
        request.start_processing("second_operation_ref", 4_100),
        Err(DataRightsError::InvalidTransition)
    );
}

#[test]
fn invalid_or_numeric_only_public_references_fail_closed() {
    assert_eq!(
        DataRightsRequest::new(
            "12345",
            "participant_ref",
            DataRightsRequestKind::Export,
            "account_data_scope",
            5_000,
        ),
        Err(DataRightsError::InvalidReference)
    );
    assert_eq!(
        DataRightsRequest::new(
            "request_ref",
            " ",
            DataRightsRequestKind::Export,
            "account_data_scope",
            5_000,
        ),
        Err(DataRightsError::InvalidReference)
    );
    assert_eq!(
        DataRightsRequest::new(
            "request_ref",
            "participant_ref",
            DataRightsRequestKind::Export,
            "account_data_scope",
            0,
        ),
        Err(DataRightsError::InvalidTimestamp)
    );
}

#[test]
fn public_error_messages_are_stable() {
    let cases = [
        (DataRightsError::InvalidReference, "data-rights references must be opaque non-numeric values"),
        (DataRightsError::InvalidTimestamp, "data-rights timestamps must be greater than zero"),
        (DataRightsError::NonMonotonicTimestamp, "data-rights event time must not move backwards"),
        (DataRightsError::IdentityVerificationRequired, "identity verification is required before data-rights processing"),
        (DataRightsError::RetentionExceptionNotAllowed, "retention exceptions are valid only for deletion requests"),
        (DataRightsError::InvalidTransition, "data-rights request transition is not allowed from the current state"),
    ];
    for (error, expected) in cases {
        assert_eq!(error.to_string(), expected);
    }
}
