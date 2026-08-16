//! Contract tests for fail-closed anonymous assessment-session resource authorization.

use psychometrics_commons_runtime::anonymous_authorization::{
    authorize_anonymous_session, AnonymousResourceAuthorizationError,
};
use psychometrics_commons_runtime::anonymous_session::AnonymousSessionContext;
use psychometrics_commons_runtime::authorization::{ResourceKind, ResourceScope};

fn anonymous_context() -> AnonymousSessionContext {
    AnonymousSessionContext::new(
        "tenant_alpha",
        "participant_alpha",
        "session_alpha",
        "anonymous_authorization_evidence_alpha",
        2_000,
    )
    .unwrap()
}

fn session_resource(
    tenant_ref: &str,
    participant_ref: &str,
    session_ref: &str,
) -> ResourceScope {
    ResourceScope::participant_owned(
        ResourceKind::AssessmentSession,
        tenant_ref,
        participant_ref,
        session_ref,
    )
    .unwrap()
}

#[test]
fn current_anonymous_authority_may_manage_only_its_exact_session_resource() {
    let context = anonymous_context();
    let resource = session_resource("tenant_alpha", "participant_alpha", "session_alpha");

    assert_eq!(authorize_anonymous_session(&context, &resource, 1_500), Ok(()));
}

#[test]
fn anonymous_authority_fails_closed_for_zero_or_expired_server_time() {
    let context = anonymous_context();
    let resource = session_resource("tenant_alpha", "participant_alpha", "session_alpha");

    assert_eq!(
        authorize_anonymous_session(&context, &resource, 0),
        Err(AnonymousResourceAuthorizationError::InvalidTimestamp)
    );
    assert_eq!(
        authorize_anonymous_session(&context, &resource, 2_000),
        Err(AnonymousResourceAuthorizationError::Expired)
    );
    assert_eq!(
        authorize_anonymous_session(&context, &resource, 2_001),
        Err(AnonymousResourceAuthorizationError::Expired)
    );
}

#[test]
fn anonymous_authority_never_crosses_tenant_or_participant_ownership() {
    let context = anonymous_context();
    let foreign_tenant = session_resource("tenant_beta", "participant_alpha", "session_alpha");
    let foreign_owner = session_resource("tenant_alpha", "participant_beta", "session_alpha");

    assert_eq!(
        authorize_anonymous_session(&context, &foreign_tenant, 1_500),
        Err(AnonymousResourceAuthorizationError::CrossTenantDenied)
    );
    assert_eq!(
        authorize_anonymous_session(&context, &foreign_owner, 1_500),
        Err(AnonymousResourceAuthorizationError::OwnerMismatch)
    );
}

#[test]
fn anonymous_authority_is_bound_to_one_exact_assessment_session() {
    let context = anonymous_context();
    let other_session = session_resource("tenant_alpha", "participant_alpha", "session_beta");

    assert_eq!(
        authorize_anonymous_session(&context, &other_session, 1_500),
        Err(AnonymousResourceAuthorizationError::SessionMismatch)
    );
}

#[test]
fn anonymous_session_proof_cannot_be_reused_for_other_participant_resources() {
    let context = anonymous_context();
    let result = ResourceScope::participant_owned(
        ResourceKind::Result,
        "tenant_alpha",
        "participant_alpha",
        "result_alpha",
    )
    .unwrap();

    assert_eq!(
        authorize_anonymous_session(&context, &result, 1_500),
        Err(AnonymousResourceAuthorizationError::ResourceKindMismatch)
    );
}

#[test]
fn anonymous_resource_authorization_errors_are_stable_and_safe() {
    let cases = [
        (
            AnonymousResourceAuthorizationError::InvalidTimestamp,
            "anonymous resource authorization requires positive server time",
        ),
        (
            AnonymousResourceAuthorizationError::Expired,
            "anonymous session authority is expired",
        ),
        (
            AnonymousResourceAuthorizationError::CrossTenantDenied,
            "anonymous session authority does not match the resource tenant",
        ),
        (
            AnonymousResourceAuthorizationError::ResourceKindMismatch,
            "anonymous session authority is limited to its assessment-session resource",
        ),
        (
            AnonymousResourceAuthorizationError::OwnerMismatch,
            "anonymous session authority does not match the resource participant",
        ),
        (
            AnonymousResourceAuthorizationError::SessionMismatch,
            "anonymous session authority does not match the resource session",
        ),
    ];

    for (error, expected) in cases {
        assert_eq!(error.to_string(), expected);
    }
}
