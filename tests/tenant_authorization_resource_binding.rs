//! Regression tests for permission/resource-kind and ownership-scope binding.

use psychometrics_commons_runtime::authorization::{
    authorize, AuthorizationContext, AuthorizationError, ProductPermission, ProductRole,
    ResourceKind, ResourceScope,
};

#[test]
fn resource_kind_rejects_the_wrong_ownership_shape() {
    assert_eq!(
        ResourceScope::tenant_scoped(
            ResourceKind::Result,
            "tenant_alpha",
            "result_alpha"
        ),
        Err(AuthorizationError::ResourceOwnershipMismatch)
    );
    assert_eq!(
        ResourceScope::participant_owned(
            ResourceKind::InstrumentRelease,
            "tenant_alpha",
            "participant_alpha",
            "instrument_release_alpha"
        ),
        Err(AuthorizationError::ResourceOwnershipMismatch)
    );
}

#[test]
fn permission_cannot_be_reused_for_a_different_resource_kind() {
    let participant = AuthorizationContext::new(
        "tenant_alpha",
        "subject_alpha",
        Some("participant_alpha"),
        &[ProductRole::Participant],
    )
    .unwrap();
    let result = ResourceScope::participant_owned(
        ResourceKind::Result,
        "tenant_alpha",
        "participant_alpha",
        "result_alpha",
    )
    .unwrap();

    assert_eq!(
        authorize(
            &participant,
            &result,
            ProductPermission::ManageOwnSession
        ),
        Err(AuthorizationError::ResourceKindMismatch)
    );
}
