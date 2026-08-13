//! Regression contract for canonical ordering of retained data-rights scopes.

use psychometrics_commons_runtime::data_rights::{DataRightsRequest, DataRightsRequestKind};

fn processing_deletion_request() -> DataRightsRequest {
    let mut request = DataRightsRequest::new(
        "deletion_request_order_ref",
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
        .start_processing("deletion_operation_ref", 1_200)
        .unwrap();
    request
}

#[test]
fn retained_scope_set_replay_is_order_independent() {
    let mut request = processing_deletion_request();

    request
        .complete(
            "deletion_completion_ref",
            &["zeta_retention_scope", "alpha_retention_scope"],
            1_300,
        )
        .unwrap();

    request
        .complete(
            "deletion_completion_ref",
            &["alpha_retention_scope", "zeta_retention_scope"],
            1_300,
        )
        .expect("the same retained scope set must replay idempotently regardless of input order");

    assert_eq!(
        request.retained_scope_refs(),
        &[
            "alpha_retention_scope".to_owned(),
            "zeta_retention_scope".to_owned()
        ]
    );
}
