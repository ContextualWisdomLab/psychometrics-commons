//! Authoritative authorization composition for participant-owned data-rights requests.
//!
//! A data-rights request already stores its tenant, participant owner, and opaque
//! request identity. Adapters must authorize against those stored values rather than
//! reconstructing a generic resource scope from request parameters or the actor.
//! Doing so keeps export/deletion access purpose-bound and prevents a confused
//! deputy from rebinding a request to the caller's own identity.

use crate::authorization::{
    authorize, AuthorizationContext, AuthorizationError, ProductPermission, ResourceKind,
    ResourceScope,
};
use crate::data_rights::DataRightsRequest;

/// Authorize the authenticated participant to manage one stored data-rights request.
///
/// Tenant, participant owner, and resource identity are read directly from the
/// request aggregate before `ManageOwnDataRights` is evaluated.
///
/// # Errors
///
/// Returns [`AuthorizationError`] when the authenticated tenant or participant does
/// not own the stored request, when participant identity is missing, or when a
/// fail-closed authorization invariant is violated.
pub fn authorize_data_rights_request(
    actor: &AuthorizationContext,
    request: &DataRightsRequest,
) -> Result<(), AuthorizationError> {
    authorize_bound_data_rights_request(
        actor,
        request.tenant_ref(),
        request.participant_ref(),
        request.request_ref(),
    )
}

#[allow(clippy::question_mark)]
fn authorize_bound_data_rights_request(
    actor: &AuthorizationContext,
    tenant_ref: &str,
    participant_ref: &str,
    request_ref: &str,
) -> Result<(), AuthorizationError> {
    let resource = match ResourceScope::participant_owned(
        ResourceKind::DataRightsRequest,
        tenant_ref,
        participant_ref,
        request_ref,
    ) {
        Ok(resource) => resource,
        Err(error) => return Err(error),
    };
    authorize(actor, &resource, ProductPermission::ManageOwnDataRights)
}

#[cfg(test)]
mod tests {
    use super::authorize_bound_data_rights_request;
    use crate::authorization::{AuthorizationContext, AuthorizationError, ProductRole};

    fn actor() -> AuthorizationContext {
        AuthorizationContext::new(
            "tenant_alpha",
            "subject_alpha",
            Some("participant_alpha"),
            &[ProductRole::Participant],
        )
        .unwrap()
    }

    #[test]
    fn bound_request_rejects_invalid_resource_references() {
        let actor = actor();

        for (tenant_ref, participant_ref, request_ref) in [
            ("", "participant_alpha", "data_rights_request_alpha"),
            ("tenant_alpha", "", "data_rights_request_alpha"),
            ("tenant_alpha", "participant_alpha", ""),
        ] {
            assert_eq!(
                authorize_bound_data_rights_request(
                    &actor,
                    tenant_ref,
                    participant_ref,
                    request_ref,
                ),
                Err(AuthorizationError::InvalidReference)
            );
        }
    }
}
