//! Exact-spelling contracts for participant data-rights identity and evidence references.

use psychometrics_commons_runtime::data_rights::{
    DataRightsError, DataRightsRequest, DataRightsRequestKind,
};

#[test]
fn request_creation_rejects_padded_public_reference_aliases() {
    let cases = [
        (" request_ref ", "tenant_ref", "participant_ref", "account_data_scope"),
        ("request_ref", "\u{00a0}tenant_ref\u{00a0}", "participant_ref", "account_data_scope"),
        ("request_ref", "tenant_ref", "\u{2003}participant_ref\u{2003}", "account_data_scope"),
        ("request_ref", "tenant_ref", "participant_ref", "\u{202f}account_data_scope\u{202f}"),
    ];

    for (request_ref, tenant_ref, participant_ref, scope_ref) in cases {
        assert_eq!(
            DataRightsRequest::new(
                request_ref,
                tenant_ref,
                participant_ref,
                DataRightsRequestKind::Export,
                scope_ref,
                1_000,
            ),
            Err(DataRightsError::InvalidReference),
            "caller-supplied identity aliases must not be normalized into a different resource",
        );
    }
}

#[test]
fn lifecycle_commands_reject_padded_evidence_aliases() {
    let mut request = DataRightsRequest::new(
        "deletion_request_ref",
        "tenant_ref",
        "participant_ref",
        DataRightsRequestKind::Deletion,
        "participant_data_scope",
        2_000,
    )
    .unwrap();

    assert_eq!(
        request.verify_identity(" verification_evidence_ref ", 2_100),
        Err(DataRightsError::InvalidReference)
    );
    request
        .verify_identity("verification_evidence_ref", 2_100)
        .unwrap();

    assert_eq!(
        request.start_processing("\u{3000}operation_ref\u{3000}", 2_200),
        Err(DataRightsError::InvalidReference)
    );
    request.start_processing("operation_ref", 2_200).unwrap();

    assert_eq!(
        request.complete(" completion_evidence_ref ", &[], 2_300),
        Err(DataRightsError::InvalidReference)
    );
    assert_eq!(
        request.complete(
            "completion_evidence_ref",
            &["\u{00a0}legal_retention_scope\u{00a0}"],
            2_300,
        ),
        Err(DataRightsError::InvalidReference)
    );
}

#[test]
fn visible_multilingual_references_remain_valid() {
    let request = DataRightsRequest::new(
        "삭제요청_ref",
        "테넌트_ref",
        "참여자_ref",
        DataRightsRequestKind::Deletion,
        "개인정보_scope",
        3_000,
    )
    .unwrap();

    assert_eq!(request.request_ref(), "삭제요청_ref");
    assert_eq!(request.tenant_ref(), "테넌트_ref");
    assert_eq!(request.participant_ref(), "참여자_ref");
    assert_eq!(request.scope_ref(), "개인정보_scope");
}
