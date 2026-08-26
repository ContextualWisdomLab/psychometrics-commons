//! Integration contract for participant export and deletion requests.

use psychometrics_commons_runtime::data_rights::{
    DataRightsError, DataRightsRequest, DataRightsRequestKind, DataRightsState,
};

fn new_request(
    request_ref: &str,
    participant_ref: &str,
    kind: DataRightsRequestKind,
    scope_ref: &str,
    requested_at_unix_ms: u64,
) -> Result<DataRightsRequest, DataRightsError> {
    DataRightsRequest::new(
        request_ref,
        "tenant_ref",
        participant_ref,
        kind,
        scope_ref,
        requested_at_unix_ms,
    )
}

#[test]
fn export_request_requires_verified_identity_before_processing() {
    let mut request = new_request(
        "export_request_ref",
        "participant_ref",
        DataRightsRequestKind::Export,
        "account_data_scope",
        1_000,
    )
    .unwrap();

    assert_eq!(request.request_ref(), "export_request_ref");
    assert_eq!(request.tenant_ref(), "tenant_ref");
    assert_eq!(request.participant_ref(), "participant_ref");
    assert_eq!(request.scope_ref(), "account_data_scope");
    assert_eq!(request.kind(), DataRightsRequestKind::Export);
    assert_eq!(request.state(), DataRightsState::Requested);
    assert_eq!(request.requested_at_unix_ms(), 1_000);
    assert_eq!(request.verification_evidence_ref(), None);
    assert_eq!(request.verified_at_unix_ms(), None);
    assert_eq!(request.operation_ref(), None);
    assert_eq!(request.processing_started_at_unix_ms(), None);
    assert_eq!(request.completion_evidence_ref(), None);
    assert_eq!(request.completed_at_unix_ms(), None);
    assert_eq!(
        request.start_processing("operation_ref", 1_100),
        Err(DataRightsError::IdentityVerificationRequired)
    );

    request
        .verify_identity("verification_evidence_ref", 1_050)
        .unwrap();
    assert_eq!(request.state(), DataRightsState::IdentityVerified);
    assert_eq!(
        request.verification_evidence_ref(),
        Some("verification_evidence_ref")
    );
    assert_eq!(request.verified_at_unix_ms(), Some(1_050));
    request.start_processing("operation_ref", 1_100).unwrap();
    assert_eq!(request.state(), DataRightsState::Processing);
    assert_eq!(request.operation_ref(), Some("operation_ref"));
    assert_eq!(request.processing_started_at_unix_ms(), Some(1_100));
    request
        .complete("completion_evidence_ref", &[], 1_200)
        .unwrap();
    assert_eq!(request.state(), DataRightsState::Completed);
    assert_eq!(
        request.completion_evidence_ref(),
        Some("completion_evidence_ref")
    );
    assert_eq!(request.completed_at_unix_ms(), Some(1_200));
    assert!(request.retained_scope_refs().is_empty());
}

#[test]
fn deletion_completion_preserves_legal_retention_exceptions() {
    let mut request = new_request(
        "deletion_request_ref",
        "participant_ref",
        DataRightsRequestKind::Deletion,
        "participant_product_data",
        2_000,
    )
    .unwrap();
    request
        .verify_identity("identity_evidence_ref", 2_050)
        .unwrap();
    request
        .start_processing("deletion_operation_ref", 2_100)
        .unwrap();
    request
        .complete(
            "deletion_completion_ref",
            &["tax_record_retention", "audit_evidence_retention"],
            2_200,
        )
        .unwrap();

    assert_eq!(request.state(), DataRightsState::PartiallyCompleted);
    assert_eq!(
        request.retained_scope_refs(),
        &[
            "audit_evidence_retention".to_owned(),
            "tax_record_retention".to_owned()
        ]
    );
}

#[test]
fn export_completion_rejects_deletion_retention_exceptions() {
    let mut request = new_request(
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
        request.complete("completion_ref", &["legal_retention_scope"], 3_200),
        Err(DataRightsError::RetentionExceptionNotAllowed)
    );
}

#[test]
fn lifecycle_is_monotonic_and_invalid_transitions_fail_closed() {
    let mut request = new_request(
        "deletion_request_ref",
        "participant_ref",
        DataRightsRequestKind::Deletion,
        "participant_data_scope",
        4_000,
    )
    .unwrap();

    assert_eq!(
        request.verify_identity("verification_ref", 0),
        Err(DataRightsError::InvalidTimestamp)
    );
    assert_eq!(
        request.verify_identity("verification_ref", 3_999),
        Err(DataRightsError::NonMonotonicTimestamp)
    );
    request.verify_identity("verification_ref", 4_000).unwrap();
    assert_eq!(
        request.start_processing(" ", 4_001),
        Err(DataRightsError::InvalidReference)
    );
    assert_eq!(
        request.start_processing("operation_ref", 3_999),
        Err(DataRightsError::NonMonotonicTimestamp)
    );
    request.start_processing("operation_ref", 4_000).unwrap();
    assert_eq!(
        request.complete("completion_ref", &[], 0),
        Err(DataRightsError::InvalidTimestamp)
    );
    assert_eq!(
        request.complete("completion_ref", &[], 3_999),
        Err(DataRightsError::NonMonotonicTimestamp)
    );
    assert_eq!(
        request.complete(" ", &[], 4_000),
        Err(DataRightsError::InvalidReference)
    );
    assert_eq!(
        request.complete("completion_ref", &["12345"], 4_000),
        Err(DataRightsError::InvalidReference)
    );
    request.complete("completion_ref", &[], 4_000).unwrap();
    assert_eq!(request.state(), DataRightsState::Completed);

    let mut not_processing = new_request(
        "second_request_ref",
        "participant_ref",
        DataRightsRequestKind::Deletion,
        "participant_data_scope",
        4_100,
    )
    .unwrap();
    assert_eq!(
        not_processing.complete("completion_ref", &[], 4_200),
        Err(DataRightsError::InvalidTransition)
    );
}

#[test]
fn exact_lifecycle_replays_are_idempotent_and_conflicts_are_rejected() {
    let mut request = new_request(
        "deletion_request_ref",
        "participant_ref",
        DataRightsRequestKind::Deletion,
        "participant_data_scope",
        6_000,
    )
    .unwrap();

    request.verify_identity("verification_ref", 6_100).unwrap();
    assert_eq!(
        request.verify_identity(" verification_ref ", 6_100),
        Err(DataRightsError::InvalidReference)
    );
    request.verify_identity("verification_ref", 6_100).unwrap();
    assert_eq!(
        request.verify_identity("verification_ref", 6_101),
        Err(DataRightsError::ConflictingReplay)
    );
    assert_eq!(
        request.verify_identity("different_verification_ref", 6_100),
        Err(DataRightsError::ConflictingReplay)
    );

    request.start_processing("operation_ref", 6_200).unwrap();
    assert_eq!(
        request.start_processing(" operation_ref ", 6_200),
        Err(DataRightsError::InvalidReference)
    );
    request.start_processing("operation_ref", 6_200).unwrap();
    assert_eq!(
        request.start_processing("operation_ref", 6_201),
        Err(DataRightsError::ConflictingReplay)
    );
    assert_eq!(
        request.start_processing("different_operation_ref", 6_200),
        Err(DataRightsError::ConflictingReplay)
    );

    request
        .complete("completion_ref", &["legal_retention_scope"], 6_300)
        .unwrap();
    assert_eq!(
        request.complete(" completion_ref ", &["legal_retention_scope"], 6_300),
        Err(DataRightsError::InvalidReference)
    );
    assert_eq!(
        request.complete("completion_ref", &[" legal_retention_scope "], 6_300),
        Err(DataRightsError::InvalidReference)
    );
    request
        .complete("completion_ref", &["legal_retention_scope"], 6_300)
        .unwrap();
    assert_eq!(
        request.complete("completion_ref", &["different_retention_scope"], 6_300,),
        Err(DataRightsError::ConflictingReplay)
    );
    assert_eq!(
        request.complete(
            "different_completion_ref",
            &["legal_retention_scope"],
            6_300,
        ),
        Err(DataRightsError::ConflictingReplay)
    );
    assert_eq!(
        request.complete("completion_ref", &["legal_retention_scope"], 6_301,),
        Err(DataRightsError::ConflictingReplay)
    );

    request.verify_identity("verification_ref", 6_100).unwrap();
    request.start_processing("operation_ref", 6_200).unwrap();
}

#[test]
fn invalid_or_numeric_only_public_references_fail_closed() {
    assert_eq!(
        new_request(
            "12345",
            "participant_ref",
            DataRightsRequestKind::Export,
            "account_data_scope",
            5_000,
        ),
        Err(DataRightsError::InvalidReference)
    );
    assert_eq!(
        new_request(
            "request_ref",
            " ",
            DataRightsRequestKind::Export,
            "account_data_scope",
            5_000,
        ),
        Err(DataRightsError::InvalidReference)
    );
    assert_eq!(
        new_request(
            "request_ref",
            "participant_ref",
            DataRightsRequestKind::Export,
            "12345",
            5_000,
        ),
        Err(DataRightsError::InvalidReference)
    );
    assert_eq!(
        new_request(
            "request_ref",
            "participant_ref",
            DataRightsRequestKind::Export,
            "account_data_scope",
            0,
        ),
        Err(DataRightsError::InvalidTimestamp)
    );
    assert_eq!(
        DataRightsRequest::new(
            "request_ref",
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
            "participant_ref",
            DataRightsRequestKind::Export,
            "account_data_scope",
            5_000,
        ),
        Err(DataRightsError::InvalidReference)
    );

    let mut request = new_request(
        "request_ref",
        "participant_ref",
        DataRightsRequestKind::Deletion,
        "participant_data_scope",
        5_000,
    )
    .unwrap();
    assert_eq!(
        request.verify_identity("12345", 5_100),
        Err(DataRightsError::InvalidReference)
    );
}

#[test]
fn public_error_messages_are_stable() {
    let cases = [
        (
            DataRightsError::InvalidReference,
            "data-rights references must use exact opaque non-numeric spelling without surrounding whitespace or unsafe controls",
        ),
        (
            DataRightsError::InvalidTimestamp,
            "data-rights timestamps must be greater than zero",
        ),
        (
            DataRightsError::NonMonotonicTimestamp,
            "data-rights event time must not move backwards",
        ),
        (
            DataRightsError::IdentityVerificationRequired,
            "identity verification is required before data-rights processing",
        ),
        (
            DataRightsError::RetentionExceptionNotAllowed,
            "retention exceptions are valid only for deletion requests",
        ),
        (
            DataRightsError::ConflictingReplay,
            "data-rights lifecycle reference was replayed with conflicting evidence",
        ),
        (
            DataRightsError::InvalidTransition,
            "data-rights request transition is not allowed from the current state",
        ),
    ];
    for (error, expected) in cases {
        assert_eq!(error.to_string(), expected);
    }
}
