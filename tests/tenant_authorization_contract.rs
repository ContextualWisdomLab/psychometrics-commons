//! Contract tests for product-owned tenant and resource authorization.

use psychometrics_commons_runtime::authorization::{
    authorize, AuthorizationContext, AuthorizationError, ProductPermission, ProductRole,
    ResourceKind, ResourceScope,
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
    let cases = [
        (
            ProductPermission::ReadOwnResult,
            ResourceKind::Result,
            "result_alpha",
            "result_other",
            "result_beta",
        ),
        (
            ProductPermission::ManageOwnSession,
            ResourceKind::AssessmentSession,
            "session_alpha",
            "session_other",
            "session_beta",
        ),
        (
            ProductPermission::ManageOwnDataRights,
            ResourceKind::DataRightsRequest,
            "data_rights_alpha",
            "data_rights_other",
            "data_rights_beta",
        ),
    ];

    for (permission, resource_kind, own_ref, other_ref, foreign_ref) in cases {
        let own = ResourceScope::participant_owned(
            resource_kind,
            "tenant_alpha",
            "participant_alpha",
            own_ref,
        )
        .unwrap();
        let other = ResourceScope::participant_owned(
            resource_kind,
            "tenant_alpha",
            "participant_other",
            other_ref,
        )
        .unwrap();
        let foreign = ResourceScope::participant_owned(
            resource_kind,
            "tenant_beta",
            "participant_alpha",
            foreign_ref,
        )
        .unwrap();

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
    let instrument = ResourceScope::tenant_scoped(
        ResourceKind::InstrumentRelease,
        "tenant_alpha",
        "instrument_release_alpha",
    )
    .unwrap();
    let research = ResourceScope::tenant_scoped(
        ResourceKind::ResearchRelease,
        "tenant_alpha",
        "research_release_alpha",
    )
    .unwrap();
    let tenant = ResourceScope::tenant_scoped(
        ResourceKind::TenantConfiguration,
        "tenant_alpha",
        "tenant_configuration_alpha",
    )
    .unwrap();

    assert_eq!(
        authorize(
            &publisher,
            &instrument,
            ProductPermission::PublishInstrument
        ),
        Ok(())
    );
    assert_eq!(
        authorize(
            &publisher,
            &research,
            ProductPermission::ApproveResearchRelease
        ),
        Err(AuthorizationError::MissingRole)
    );

    assert_eq!(
        authorize(
            &steward,
            &research,
            ProductPermission::ApproveResearchRelease
        ),
        Ok(())
    );
    assert_eq!(
        authorize(&steward, &instrument, ProductPermission::PublishInstrument),
        Err(AuthorizationError::MissingRole)
    );

    assert_eq!(
        authorize(&tenant_admin, &tenant, ProductPermission::ManageTenant),
        Ok(())
    );
    assert_eq!(
        authorize(
            &tenant_admin,
            &research,
            ProductPermission::ApproveResearchRelease
        ),
        Err(AuthorizationError::MissingRole)
    );
}

#[test]
fn permission_and_resource_kind_must_match_before_ownership_or_role_evaluation() {
    let participant = participant_context();
    let result = ResourceScope::participant_owned(
        ResourceKind::Result,
        "tenant_alpha",
        "participant_alpha",
        "result_alpha",
    )
    .unwrap();
    assert_eq!(
        authorize(&participant, &result, ProductPermission::ManageOwnSession),
        Err(AuthorizationError::ResourceKindMismatch)
    );

    let publisher = AuthorizationContext::new(
        "tenant_alpha",
        "subject_publisher",
        None,
        &[ProductRole::InstrumentPublisher],
    )
    .unwrap();
    assert_eq!(
        authorize(&publisher, &result, ProductPermission::PublishInstrument),
        Err(AuthorizationError::ResourceKindMismatch)
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
    let foreign = ResourceScope::tenant_scoped(
        ResourceKind::InstrumentRelease,
        "tenant_beta",
        "instrument_release_beta",
    )
    .unwrap();

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
        ResourceKind::AssessmentSession,
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
    }

    for invalid in ["", "   ", "12345"] {
        assert_eq!(
            ResourceScope::tenant_scoped(
                ResourceKind::InstrumentRelease,
                invalid,
                "instrument_release_alpha"
            ),
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
        ResourceScope::participant_owned(
            ResourceKind::Result,
            "tenant_alpha",
            "12345",
            "result_alpha"
        ),
        Err(AuthorizationError::InvalidReference)
    );
    assert_eq!(
        ResourceScope::tenant_scoped(
            ResourceKind::InstrumentRelease,
            "tenant_alpha",
            "12345"
        ),
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
        ResourceKind::Result,
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
    assert_eq!(resource.kind(), ResourceKind::Result);
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
            AuthorizationError::ResourceOwnershipMismatch,
            "resource kind is not valid for this ownership scope",
        ),
        (
            AuthorizationError::ResourceKindMismatch,
            "permission is not valid for this resource kind",
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
