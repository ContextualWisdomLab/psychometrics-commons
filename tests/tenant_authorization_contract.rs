//! Contract tests for product-owned tenant and resource authorization.

use psychometrics_commons_runtime::authorization::{
    authorize, AuthorizationContext, AuthorizationError, ProductPermission, ProductRole,
    ResourceScope,
};

fn participant_context() -> AuthorizationContext {
    AuthorizationContext::new(
        "tenant_alpha",
        "subject_alpha",
        Some("participant_alpha"),
        &[ProductRole::Participant],
    )
    .unwrap()
}

#[test]
fn participant_may_manage_only_owned_resources_in_the_authenticated_tenant() {
    let actor = participant_context();
    let own = ResourceScope::participant_owned(
        "tenant_alpha",
        "participant_alpha",
        "result_alpha",
    )
    .unwrap();
    let other = ResourceScope::participant_owned(
        "tenant_alpha",
        "participant_other",
        "result_other",
    )
    .unwrap();
    let foreign = ResourceScope::participant_owned(
        "tenant_beta",
        "participant_alpha",
        "result_beta",
    )
    .unwrap();

    for permission in [
        ProductPermission::ReadOwnResult,
        ProductPermission::ManageOwnSession,
        ProductPermission::ManageOwnDataRights,
    ] {
        assert_eq!(authorize(&actor, &own, permission), Ok(()));
        assert_eq!(
            authorize(&actor, &other, permission),
            Err(AuthorizationError::OwnerMismatch)
        );
        assert_eq!(
            authorize(&actor, &foreign, permission),
            Err(AuthorizationError::CrossTenantDenied)
        );
    }
}

#[test]
fn privileged_product_roles_are_explicit_and_separation_of_duties_is_preserved() {
    let publisher = AuthorizationContext::new(
        "tenant_alpha",
        "subject_publisher",
        None,
        &[ProductRole::InstrumentPublisher],
    )
    .unwrap();
    let steward = AuthorizationContext::new(
        "tenant_alpha",
        "subject_steward",
        None,
        &[ProductRole::ResearchSteward],
    )
    .unwrap();
    let tenant_admin = AuthorizationContext::new(
        "tenant_alpha",
        "subject_admin",
        None,
        &[ProductRole::TenantAdministrator],
    )
    .unwrap();
    let resource = ResourceScope::tenant_scoped("tenant_alpha", "resource_alpha").unwrap();

    assert_eq!(
        authorize(&publisher, &resource, ProductPermission::PublishInstrument),
        Ok(())
    );
    assert_eq!(
        authorize(
            &publisher,
            &resource,
            ProductPermission::ApproveResearchRelease
        ),
        Err(AuthorizationError::MissingRole)
    );

    assert_eq!(
        authorize(
            &steward,
            &resource,
            ProductPermission::ApproveResearchRelease
        ),
        Ok(())
    );
    assert_eq!(
        authorize(&steward, &resource, ProductPermission::PublishInstrument),
        Err(AuthorizationError::MissingRole)
    );

    assert_eq!(
        authorize(&tenant_admin, &resource, ProductPermission::ManageTenant),
        Ok(())
    );
    assert_eq!(
        authorize(
            &tenant_admin,
            &resource,
            ProductPermission::ApproveResearchRelease
        ),
        Err(AuthorizationError::MissingRole)
    );
}

#[test]
fn privileged_roles_never_cross_tenant_boundaries() {
    let publisher = AuthorizationContext::new(
        "tenant_alpha",
        "subject_publisher",
        None,
        &[ProductRole::InstrumentPublisher],
    )
    .unwrap();
    let foreign = ResourceScope::tenant_scoped("tenant_beta", "resource_beta").unwrap();

    assert_eq!(
        authorize(&publisher, &foreign, ProductPermission::PublishInstrument),
        Err(AuthorizationError::CrossTenantDenied)
    );
}

#[test]
fn anonymous_or_nonparticipant_context_cannot_claim_participant_owned_authority() {
    let actor = AuthorizationContext::new(
        "tenant_alpha",
        "anonymous_subject",
        None,
        &[ProductRole::Participant],
    )
    .unwrap();
    let resource = ResourceScope::participant_owned(
        "tenant_alpha",
        "participant_alpha",
        "session_alpha",
    )
    .unwrap();

    assert_eq!(
        authorize(&actor, &resource, ProductPermission::ManageOwnSession),
        Err(AuthorizationError::ParticipantIdentityRequired)
    );
}

#[test]
fn malformed_or_numeric_references_fail_closed() {
    for tenant_ref in ["", "   ", "12345"] {
        assert_eq!(
            AuthorizationContext::new(
                tenant_ref,
                "subject_alpha",
                Some("participant_alpha"),
                &[ProductRole::Participant],
            ),
            Err(AuthorizationError::InvalidReference)
        );
        assert_eq!(
            ResourceScope::tenant_scoped(tenant_ref, "resource_alpha"),
            Err(AuthorizationError::InvalidReference)
        );
    }

    assert_eq!(
        AuthorizationContext::new(
            "tenant_alpha",
            "12345",
            Some("participant_alpha"),
            &[ProductRole::Participant],
        ),
        Err(AuthorizationError::InvalidReference)
    );
    assert_eq!(
        AuthorizationContext::new(
            "tenant_alpha",
            "subject_alpha",
            Some("12345"),
            &[ProductRole::Participant],
        ),
        Err(AuthorizationError::InvalidReference)
    );
    assert_eq!(
        ResourceScope::participant_owned("tenant_alpha", "12345", "resource_alpha"),
        Err(AuthorizationError::InvalidReference)
    );
    assert_eq!(
        ResourceScope::tenant_scoped("tenant_alpha", "12345"),
        Err(AuthorizationError::InvalidReference)
    );
}

#[test]
fn context_and_resource_metadata_are_auditable_without_identity_role_confusion() {
    let actor = AuthorizationContext::new(
        " tenant_alpha ",
        " subject_alpha ",
        Some(" participant_alpha "),
        &[
            ProductRole::Participant,
            ProductRole::Participant,
            ProductRole::InstrumentPublisher,
        ],
    )
    .unwrap();
    let resource = ResourceScope::participant_owned(
        " tenant_alpha ",
        " participant_alpha ",
        " result_alpha ",
    )
    .unwrap();

    assert_eq!(actor.tenant_ref(), "tenant_alpha");
    assert_eq!(actor.subject_ref(), "subject_alpha");
    assert_eq!(actor.participant_ref(), Some("participant_alpha"));
    assert!(actor.has_role(ProductRole::Participant));
    assert!(actor.has_role(ProductRole::InstrumentPublisher));
    assert!(!actor.has_role(ProductRole::ResearchSteward));
    assert_eq!(actor.roles().len(), 2);

    assert_eq!(resource.tenant_ref(), "tenant_alpha");
    assert_eq!(resource.resource_ref(), "result_alpha");
    assert_eq!(resource.owner_participant_ref(), Some("participant_alpha"));
}

#[test]
fn authorization_errors_have_stable_safe_messages() {
    let cases = [
        (
            AuthorizationError::InvalidReference,
            "authorization references must be opaque non-numeric values",
        ),
        (
            AuthorizationError::CrossTenantDenied,
            "resource tenant does not match the authenticated tenant",
        ),
        (
            AuthorizationError::ParticipantIdentityRequired,
            "participant identity is required for participant-owned authorization",
        ),
        (
            AuthorizationError::OwnerMismatch,
            "resource owner does not match the authenticated participant",
        ),
        (
            AuthorizationError::MissingRole,
            "authenticated product roles do not permit this operation",
        ),
    ];

    for (error, expected) in cases {
        assert_eq!(error.to_string(), expected);
    }
}
