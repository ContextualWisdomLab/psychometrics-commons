//! Fail-closed product authorization for validated anonymous assessment sessions.
//!
//! Anonymous participation is a first-class product path, but a validated anonymous-session
//! proof is intentionally narrower than an authenticated participant identity. This module binds
//! already-validated short-lived anonymous authority to exactly one participant-owned assessment
//! session. It cannot authorize result access, consent, data-rights, tenant administration, or any
//! other product resource.

use crate::anonymous_session::AnonymousSessionContext;
use crate::authorization::{ResourceKind, ResourceScope};
use std::error::Error;
use std::fmt::{Display, Formatter};

/// Fail-closed authorization error for a validated anonymous assessment session.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum AnonymousResourceAuthorizationError {
    /// The server-authoritative authorization time was zero or otherwise unknown.
    InvalidTimestamp,
    /// The short-lived anonymous-session authority was no longer valid.
    Expired,
    /// The target resource belonged to another tenant.
    CrossTenantDenied,
    /// Anonymous-session authority was presented for a non-session resource.
    ResourceKindMismatch,
    /// The target session belonged to another operational participant.
    OwnerMismatch,
    /// The target assessment-session reference differed from the proof binding.
    SessionMismatch,
}

impl Display for AnonymousResourceAuthorizationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidTimestamp => {
                "anonymous resource authorization requires positive server time"
            }
            Self::Expired => "anonymous session authority is expired",
            Self::CrossTenantDenied => {
                "anonymous session authority does not match the resource tenant"
            }
            Self::ResourceKindMismatch => {
                "anonymous session authority is limited to its assessment-session resource"
            }
            Self::OwnerMismatch => {
                "anonymous session authority does not match the resource participant"
            }
            Self::SessionMismatch => {
                "anonymous session authority does not match the resource session"
            }
        })
    }
}

impl Error for AnonymousResourceAuthorizationError {}

/// Authorize one participant-owned assessment-session resource using validated anonymous proof.
///
/// This boundary deliberately has no generic permission parameter. A validated anonymous-session
/// proof grants only authority over its exact assessment-session resource. Adding another resource
/// or operation therefore requires a new explicit authorization contract rather than silently
/// inheriting future authenticated-participant permissions.
///
/// The server time is checked before any resource metadata so an unknown time cannot be treated as
/// current authority. Tenant, resource kind, participant owner, and exact session identity are then
/// checked in that order. All comparisons use canonical values that were already validated by
/// [`AnonymousSessionContext`] and [`ResourceScope`] constructors.
///
/// # Errors
///
/// Returns [`AnonymousResourceAuthorizationError`] when server time is invalid, the proof has
/// expired, or the target resource differs from the exact tenant/participant/session binding.
pub fn authorize_anonymous_session(
    actor: &AnonymousSessionContext,
    resource: &ResourceScope,
    now_unix_ms: u64,
) -> Result<(), AnonymousResourceAuthorizationError> {
    if now_unix_ms == 0 {
        return Err(AnonymousResourceAuthorizationError::InvalidTimestamp);
    }
    if !actor.is_valid_at(now_unix_ms) {
        return Err(AnonymousResourceAuthorizationError::Expired);
    }
    if actor.tenant_ref() != resource.tenant_ref() {
        return Err(AnonymousResourceAuthorizationError::CrossTenantDenied);
    }
    if resource.kind() != ResourceKind::AssessmentSession {
        return Err(AnonymousResourceAuthorizationError::ResourceKindMismatch);
    }
    if resource.owner_participant_ref() != Some(actor.participant_ref()) {
        return Err(AnonymousResourceAuthorizationError::OwnerMismatch);
    }
    if resource.resource_ref() != actor.session_ref() {
        return Err(AnonymousResourceAuthorizationError::SessionMismatch);
    }
    Ok(())
}
