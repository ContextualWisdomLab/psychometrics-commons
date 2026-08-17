//! Authoritative authorization composition for participant-owned result reads.
//!
//! Generic [`crate::authorization::ResourceScope`] values remain useful at adapter
//! boundaries, but result access must not trust a caller to repeat tenant or owner
//! identifiers correctly. This module derives those values from the product-owned
//! participant and immutable result records before applying the generic permission
//! check, so a caller cannot turn its own identity into the ownership metadata of a
//! different result.

use crate::authorization::{
    authorize, AuthorizationContext, AuthorizationError, ProductPermission, ResourceKind,
    ResourceScope,
};
use crate::participant::ParticipantRecord;
use crate::result::ResultSnapshot;

/// Authorize the authenticated participant to read one immutable result snapshot.
///
/// The resource tenant is taken from `participant`, while the resource owner and
/// resource identity are taken from `result`. The participant record must first
/// identify the same participant as the result snapshot. Callers therefore cannot
/// authorize a result by supplying a generic scope populated from the actor instead
/// of from the stored resource.
///
/// # Errors
///
/// Returns [`AuthorizationError::OwnerMismatch`] when the supplied participant
/// record does not own the result, or another [`AuthorizationError`] when the
/// authenticated tenant/participant does not satisfy `ReadOwnResult`.
pub fn authorize_result_read(
    actor: &AuthorizationContext,
    participant: &ParticipantRecord,
    result: &ResultSnapshot,
) -> Result<(), AuthorizationError> {
    if participant.participant_ref() != result.participant_ref() {
        return Err(AuthorizationError::OwnerMismatch);
    }

    authorize_bound_result(
        actor,
        participant.tenant_ref(),
        result.participant_ref(),
        result.result_snapshot_ref(),
    )
}

#[allow(clippy::question_mark)]
fn authorize_bound_result(
    actor: &AuthorizationContext,
    tenant_ref: &str,
    participant_ref: &str,
    result_ref: &str,
) -> Result<(), AuthorizationError> {
    let resource = match ResourceScope::participant_owned(
        ResourceKind::Result,
        tenant_ref,
        participant_ref,
        result_ref,
    ) {
        Ok(resource) => resource,
        Err(error) => return Err(error),
    };
    authorize(actor, &resource, ProductPermission::ReadOwnResult)
}

#[cfg(test)]
mod tests {
    use super::authorize_bound_result;
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
    fn bound_result_rejects_invalid_resource_references() {
        let actor = actor();

        for (tenant_ref, participant_ref, result_ref) in [
            ("", "participant_alpha", "result_alpha"),
            ("tenant_alpha", "", "result_alpha"),
            ("tenant_alpha", "participant_alpha", ""),
        ] {
            assert_eq!(
                authorize_bound_result(&actor, tenant_ref, participant_ref, result_ref),
                Err(AuthorizationError::InvalidReference)
            );
        }
    }

    #[test]
    fn bound_result_authorizes_the_exact_stored_identity() {
        let actor = actor();
        assert_eq!(
            authorize_bound_result(&actor, "tenant_alpha", "participant_alpha", "result_alpha",),
            Ok(())
        );
    }
}
