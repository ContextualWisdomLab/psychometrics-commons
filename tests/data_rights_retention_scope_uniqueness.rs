//! Regression contract for unique data-rights retention exception scopes.

use psychometrics_commons_runtime::data_rights::{
    DataRightsError, DataRightsRequest, DataRightsRequestKind, DataRightsState,
};

fn processing_retention_request() -> DataRightsRequest {
    let mut request = DataRightsRequest::new(
        "rights_request_ref",
        "tenant_ref",
        "participant_ref",
        DataRightsRequestKind::Deletion,
        "participant_product_data",
        1_000,
    )
    .unwrap();
    request
        .verify_identity("identity_verification_ref", 1_100)
        .unwrap();
    request
        .start_processing("rights_operation_ref", 1_200)
        .unwrap();
    request
}

#[test]
fn noncanonical_retained_scope_fails_closed_before_uniqueness() {
    let mut request = processing_retention_request();

    assert_eq!(
        request.complete(
            "completion_ref",
            &[" legal_retention_scope ", "legal_retention_scope"],
            1_300,
        ),
        Err(DataRightsError::InvalidReference)
    );

    assert_eq!(request.state(), DataRightsState::Processing);
    assert_eq!(request.completion_evidence_ref(), None);
    assert_eq!(request.completed_at_unix_ms(), None);
    assert!(request.retained_scope_refs().is_empty());
}

#[test]
fn duplicate_canonical_retained_scopes_fail_closed() {
    let mut request = processing_retention_request();

    assert_eq!(
        request.complete(
            "completion_ref",
            &["legal_retention_scope", "legal_retention_scope"],
            1_300,
        ),
        Err(DataRightsError::DuplicateRetentionScope)
    );

    assert_eq!(request.state(), DataRightsState::Processing);
    assert_eq!(request.completion_evidence_ref(), None);
    assert_eq!(request.completed_at_unix_ms(), None);
    assert!(request.retained_scope_refs().is_empty());
}

#[test]
fn duplicate_retention_error_message_is_stable() {
    assert_eq!(
        DataRightsError::DuplicateRetentionScope.to_string(),
        "data-rights retained scope references must be unique"
    );
}
