//! Authorization for one participant-owned data-rights request using stored identity.
//!
//! A [`DataRightsRequest`] is the authoritative stored record for its tenant, participant
//! owner, and request identifier. That identifier is *opaque*: callers treat it as an
//! issued label and must not infer meaning or sequence from it. API or persistence boundary
//! code (often called an *adapter*) must use those stored values instead of rebuilding the
//! authorization target from URL fields, request bodies, or the authenticated actor.
//!
//! Authorization evaluates a *resource scope*: the tenant, participant owner, resource
//! kind, and exact request identifier that together describe what is being accessed. Using
//! the stored request prevents a *confused deputy* defect, where trusted server code is
//! tricked into acting on a different request because caller-controlled identity fields were
//! substituted for the stored ones. Invalid or incomplete bindings *fail closed*: access is
//! denied rather than guessed or defaulted.
//!
//! The architecture boundary and unchanged ownership are documented in
//! `docs/architecture/DATA_RIGHTS_AUTHORIZATION.md`. This module adds no credential,
//! permission, lifecycle, or database ownership; it composes the existing product-owned
//! `ManageOwnDataRights` permission with the stored data-rights request.

use crate::authorization::{
    authorize, AuthorizationContext, AuthorizationError, ProductPermission, ResourceKind,
    ResourceScope,
};
use crate::data_rights::DataRightsRequest;

/// Authorize the authenticated participant to manage one stored data-rights request.
///
/// The stored request is the domain record (sometimes called an *aggregate*) that owns the
/// tenant, participant, and request identifiers used here. The function builds the
/// authorization target only from that record, then evaluates [`ProductPermission::ManageOwnDataRights`].
/// It does not trust copies of those identifiers supplied by a caller.
///
/// # Errors
///
/// Returns [`AuthorizationError`] when the authenticated tenant or participant does not own
/// the stored request, participant identity is missing, an identifier is invalid, or another
/// authorization invariant cannot be proven. In all such cases access is denied instead of
/// falling back to a guessed/default identity.
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

    #[test]
    fn bound_request_authorizes_the_exact_stored_identity() {
        let actor = actor();
        assert_eq!(
            authorize_bound_data_rights_request(
                &actor,
                "tenant_alpha",
                "participant_alpha",
                "data_rights_request_alpha",
            ),
            Ok(())
        );
    }
}
