//! Regression tests for data-rights access bound to the stored request identity.

use psychometrics_commons_runtime::authorization::{
    AuthorizationContext, AuthorizationError, ProductRole,
};
use psychometrics_commons_runtime::data_rights::{DataRightsRequest, DataRightsRequestKind};
use psychometrics_commons_runtime::data_rights_authorization::authorize_data_rights_request;

fn request() -> DataRightsRequest {
    DataRightsRequest::new(
        "data_rights_request_alpha",
        "tenant_alpha",
        "participant_alpha",
        DataRightsRequestKind::Export,
        "participant_export_scope_v1",
        1_786_240_000_000,
    )
    .unwrap()
}

fn actor(tenant_ref: &str, participant_ref: Option<&str>) -> AuthorizationContext {
    AuthorizationContext::new(
        tenant_ref,
        "subject_alpha",
        participant_ref,
        &[ProductRole::Participant],
    )
    .unwrap()
}

#[test]
fn data_rights_access_uses_tenant_owner_and_identity_from_the_stored_request() {
    let actor = actor("tenant_alpha", Some("participant_alpha"));

    assert_eq!(authorize_data_rights_request(&actor, &request()), Ok(()));
}

#[test]
fn data_rights_access_rejects_cross_tenant_actor_context() {
    let actor = actor("tenant_beta", Some("participant_alpha"));

    assert_eq!(
        authorize_data_rights_request(&actor, &request()),
        Err(AuthorizationError::CrossTenantDenied)
    );
}

#[test]
fn data_rights_access_rejects_a_different_authenticated_participant() {
    let actor = actor("tenant_alpha", Some("participant_beta"));

    assert_eq!(
        authorize_data_rights_request(&actor, &request()),
        Err(AuthorizationError::OwnerMismatch)
    );
}

#[test]
fn data_rights_access_requires_an_operational_participant_identity() {
    let actor = actor("tenant_alpha", None);

    assert_eq!(
        authorize_data_rights_request(&actor, &request()),
        Err(AuthorizationError::ParticipantIdentityRequired)
    );
}
