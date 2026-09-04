//! Terminal-state contract for durable data-rights workflows.

use psychometrics_commons_runtime::data_rights::{
    DataRightsError, DataRightsRequest, DataRightsRequestKind, DataRightsState,
};

fn deletion_request(request_ref: &str, requested_at_unix_ms: u64) -> DataRightsRequest {
    DataRightsRequest::new(
        request_ref,
        "tenant_ref",
        "participant_ref",
        DataRightsRequestKind::Deletion,
        "participant_data_scope",
        requested_at_unix_ms,
    )
    .unwrap()
}

#[test]
fn rejected_request_preserves_durable_evidence_and_is_terminal() {
    let mut request = deletion_request("rejected_request_ref", 10_000);

    request.reject("rejection_evidence_ref", 10_100).unwrap();
    assert_eq!(request.state(), DataRightsState::Rejected);
    assert_eq!(
        request.rejection_evidence_ref(),
        Some("rejection_evidence_ref")
    );
    assert_eq!(request.rejected_at_unix_ms(), Some(10_100));

    request.reject("rejection_evidence_ref", 10_100).unwrap();
    assert_eq!(
        request.reject("different_rejection_ref", 10_100),
        Err(DataRightsError::ConflictingReplay)
    );
    assert_eq!(
        request.reject("rejection_evidence_ref", 10_101),
        Err(DataRightsError::ConflictingReplay)
    );
    assert_eq!(
        request.verify_identity("verification_ref", 10_200),
        Err(DataRightsError::InvalidTransition)
    );
    assert_eq!(
        request.start_processing("operation_ref", 10_200),
        Err(DataRightsError::InvalidTransition)
    );
    assert_eq!(
        request.complete("completion_ref", &[], 10_200),
        Err(DataRightsError::InvalidTransition)
    );
}

#[test]
fn identity_verified_request_can_be_rejected_but_processing_request_cannot() {
    let mut verified = deletion_request("verified_rejection_ref", 11_000);
    verified
        .verify_identity("verification_ref", 11_050)
        .unwrap();
    verified.reject("policy_rejection_ref", 11_100).unwrap();
    assert_eq!(verified.state(), DataRightsState::Rejected);

    let mut processing = deletion_request("processing_rejection_ref", 12_000);
    processing
        .verify_identity("verification_ref", 12_050)
        .unwrap();
    processing
        .start_processing("operation_ref", 12_100)
        .unwrap();
    assert_eq!(
        processing.reject("late_rejection_ref", 12_200),
        Err(DataRightsError::InvalidTransition)
    );
}

#[test]
fn processing_failure_preserves_durable_evidence_and_is_terminal() {
    let mut request = deletion_request("failed_request_ref", 13_000);
    request.verify_identity("verification_ref", 13_050).unwrap();
    request.start_processing("operation_ref", 13_100).unwrap();

    request.fail("failure_evidence_ref", 13_200).unwrap();
    assert_eq!(request.state(), DataRightsState::Failed);
    assert_eq!(request.failure_evidence_ref(), Some("failure_evidence_ref"));
    assert_eq!(request.failed_at_unix_ms(), Some(13_200));

    request.fail("failure_evidence_ref", 13_200).unwrap();
    assert_eq!(
        request.fail("different_failure_ref", 13_200),
        Err(DataRightsError::ConflictingReplay)
    );
    assert_eq!(
        request.fail("failure_evidence_ref", 13_201),
        Err(DataRightsError::ConflictingReplay)
    );
    assert_eq!(
        request.complete("completion_ref", &[], 13_300),
        Err(DataRightsError::InvalidTransition)
    );
    assert_eq!(
        request.reject("rejection_ref", 13_300),
        Err(DataRightsError::InvalidTransition)
    );
}

#[test]
fn failure_is_valid_only_after_processing_and_terminal_commands_fail_closed() {
    let mut requested = deletion_request("requested_failure_ref", 14_000);
    assert_eq!(
        requested.fail("failure_ref", 14_100),
        Err(DataRightsError::InvalidTransition)
    );

    let mut verified = deletion_request("verified_failure_ref", 15_000);
    verified
        .verify_identity("verification_ref", 15_050)
        .unwrap();
    assert_eq!(
        verified.fail("failure_ref", 15_100),
        Err(DataRightsError::InvalidTransition)
    );

    let mut completed = deletion_request("completed_rejection_ref", 16_000);
    completed
        .verify_identity("verification_ref", 16_050)
        .unwrap();
    completed.start_processing("operation_ref", 16_100).unwrap();
    completed.complete("completion_ref", &[], 16_200).unwrap();
    assert_eq!(
        completed.reject("rejection_ref", 16_300),
        Err(DataRightsError::InvalidTransition)
    );
    assert_eq!(
        completed.fail("failure_ref", 16_300),
        Err(DataRightsError::InvalidTransition)
    );
}

#[test]
fn terminal_evidence_rejects_invalid_references_and_non_monotonic_time() {
    let mut rejection = deletion_request("rejection_validation_ref", 17_000);
    assert_eq!(
        rejection.reject("12345", 17_100),
        Err(DataRightsError::InvalidReference)
    );
    assert_eq!(
        rejection.reject(" rejection_ref ", 17_100),
        Err(DataRightsError::InvalidReference)
    );
    assert_eq!(
        rejection.reject("rejection_ref", 0),
        Err(DataRightsError::InvalidTimestamp)
    );
    assert_eq!(
        rejection.reject("rejection_ref", 16_999),
        Err(DataRightsError::NonMonotonicTimestamp)
    );

    let mut failure = deletion_request("failure_validation_ref", 18_000);
    failure.verify_identity("verification_ref", 18_050).unwrap();
    failure.start_processing("operation_ref", 18_100).unwrap();
    assert_eq!(
        failure.fail("12345", 18_200),
        Err(DataRightsError::InvalidReference)
    );
    assert_eq!(
        failure.fail(" failure_ref ", 18_200),
        Err(DataRightsError::InvalidReference)
    );
    assert_eq!(
        failure.fail("failure_ref", 0),
        Err(DataRightsError::InvalidTimestamp)
    );
    assert_eq!(
        failure.fail("failure_ref", 18_099),
        Err(DataRightsError::NonMonotonicTimestamp)
    );
}
