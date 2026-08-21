//! Regression contract for exact data-rights resource and evidence references.
//!
//! Data-rights references authorize privacy-sensitive export/deletion work and form
//! durable replay identities. Callers must therefore present the exact issued spelling;
//! surrounding Unicode whitespace is an alias, not a canonicalization request.

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
fn request_creation_rejects_whitespace_aliases_in_every_owned_reference() {
    let cases = [
        (
            " request_ref_alpha ",
            "tenant_ref_alpha",
            "participant_ref_alpha",
            "participant_data_scope",
        ),
        (
            "request_ref_alpha",
            "\u{00a0}tenant_ref_alpha",
            "participant_ref_alpha",
            "participant_data_scope",
        ),
        (
            "request_ref_alpha",
            "tenant_ref_alpha",
            "participant_ref_alpha\u{2003}",
            "participant_data_scope",
        ),
        (
            "request_ref_alpha",
            "tenant_ref_alpha",
            "participant_ref_alpha",
            "\u{202f}participant_data_scope\u{202f}",
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
            "padded alias must fail closed: {request_ref:?} {tenant_ref:?} {participant_ref:?} {scope_ref:?}",
        );
    }
}

#[test]
fn lifecycle_replays_reject_padded_evidence_and_scope_aliases() {
    let mut request = deletion_request();
    request
        .verify_identity("verification_evidence_alpha", 1_100)
        .unwrap();
    assert_eq!(
        request.verify_identity(" verification_evidence_alpha ", 1_100),
        Err(DataRightsError::InvalidReference)
    );

    request
        .start_processing("operation_ref_alpha", 1_200)
        .unwrap();
    assert_eq!(
        request.start_processing("\u{00a0}operation_ref_alpha", 1_200),
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
            " completion_evidence_alpha ",
            &["legal_retention_scope"],
            1_300,
        ),
        Err(DataRightsError::InvalidReference)
    );
    assert_eq!(
        request.complete(
            "completion_evidence_alpha",
            &["\u{2003}legal_retention_scope"],
            1_300,
        ),
        Err(DataRightsError::InvalidReference)
    );
}

#[test]
fn terminal_replay_evidence_rejects_padded_aliases() {
    let mut rejected = deletion_request();
    rejected.reject("rejection_evidence_alpha", 1_100).unwrap();
    assert_eq!(
        rejected.reject("rejection_evidence_alpha\u{202f}", 1_100),
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
        failed.fail("\u{00a0}failure_evidence_alpha\u{00a0}", 1_300),
        Err(DataRightsError::InvalidReference)
    );
}
