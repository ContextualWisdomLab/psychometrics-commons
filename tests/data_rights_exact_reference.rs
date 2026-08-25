//! Regression contract for invisible-alias rejection in data-rights references.
//!
//! Data-rights references authorize privacy-sensitive export/deletion work and form
//! durable replay identities. Unicode default-ignorable characters can make two byte-
//! distinct references appear identical in logs or participant-facing artifacts, so the
//! shared opaque-reference boundary must reject them instead of silently accepting aliases.

use psychometrics_commons_runtime::data_rights::{
    DataRightsError, DataRightsRequest, DataRightsRequestKind,
};

fn deletion_request() -> DataRightsRequest {
    DataRightsRequest::new(
        "request_ref_alpha",
        "tenant_ref_alpha",
        "participant_ref_alpha",
        DataRightsRequestKind::Deletion,
        "participant_data_scope",
        1_000,
    )
    .unwrap()
}

#[test]
fn request_creation_rejects_default_ignorable_aliases_in_owned_references() {
    let cases = [
        (
            "request\u{200b}_ref_alpha",
            "tenant_ref_alpha",
            "participant_ref_alpha",
            "participant_data_scope",
        ),
        (
            "request_ref_alpha",
            "tenant\u{200d}_ref_alpha",
            "participant_ref_alpha",
            "participant_data_scope",
        ),
        (
            "request_ref_alpha",
            "tenant_ref_alpha",
            "participant\u{fe0f}_ref_alpha",
            "participant_data_scope",
        ),
        (
            "request_ref_alpha",
            "tenant_ref_alpha",
            "participant_ref_alpha",
            "participant\u{202e}_data_scope",
        ),
    ];

    for (request_ref, tenant_ref, participant_ref, scope_ref) in cases {
        assert_eq!(
            DataRightsRequest::new(
                request_ref,
                tenant_ref,
                participant_ref,
                DataRightsRequestKind::Deletion,
                scope_ref,
                1_000,
            ),
            Err(DataRightsError::InvalidReference),
            "default-ignorable alias must fail closed: {request_ref:?} {tenant_ref:?} {participant_ref:?} {scope_ref:?}",
        );
    }
}

#[test]
fn lifecycle_replays_reject_invisible_evidence_and_scope_aliases() {
    let mut request = deletion_request();
    request
        .verify_identity("verification_evidence_alpha", 1_100)
        .unwrap();
    assert_eq!(
        request.verify_identity("verification\u{2060}_evidence_alpha", 1_100),
        Err(DataRightsError::InvalidReference)
    );

    request
        .start_processing("operation_ref_alpha", 1_200)
        .unwrap();
    assert_eq!(
        request.start_processing("operation\u{00ad}_ref_alpha", 1_200),
        Err(DataRightsError::InvalidReference)
    );

    request
        .complete(
            "completion_evidence_alpha",
            &["legal_retention_scope"],
            1_300,
        )
        .unwrap();
    assert_eq!(
        request.complete(
            "completion\u{034f}_evidence_alpha",
            &["legal_retention_scope"],
            1_300,
        ),
        Err(DataRightsError::InvalidReference)
    );
    assert_eq!(
        request.complete(
            "completion_evidence_alpha",
            &["legal\u{180e}_retention_scope"],
            1_300,
        ),
        Err(DataRightsError::InvalidReference)
    );
}

#[test]
fn terminal_replay_evidence_rejects_invisible_aliases() {
    let mut rejected = deletion_request();
    rejected.reject("rejection_evidence_alpha", 1_100).unwrap();
    assert_eq!(
        rejected.reject("rejection\u{061c}_evidence_alpha", 1_100),
        Err(DataRightsError::InvalidReference)
    );

    let mut failed = deletion_request();
    failed
        .verify_identity("verification_evidence_alpha", 1_100)
        .unwrap();
    failed
        .start_processing("operation_ref_alpha", 1_200)
        .unwrap();
    failed.fail("failure_evidence_alpha", 1_300).unwrap();
    assert_eq!(
        failed.fail("failure\u{e0001}_evidence_alpha", 1_300),
        Err(DataRightsError::InvalidReference)
    );
}

#[test]
fn visible_multilingual_reference_material_remains_valid() {
    let request = DataRightsRequest::new(
        "request_ref_가나다",
        "tenant_ref_東京",
        "participant_ref_éclair",
        DataRightsRequestKind::Export,
        "scope_ref_데이터",
        2_000,
    )
    .unwrap();

    assert_eq!(request.request_ref(), "request_ref_가나다");
    assert_eq!(request.tenant_ref(), "tenant_ref_東京");
    assert_eq!(request.participant_ref(), "participant_ref_éclair");
    assert_eq!(request.scope_ref(), "scope_ref_데이터");
}
