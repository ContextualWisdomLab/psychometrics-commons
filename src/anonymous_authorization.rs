//! Product authorization for an already-verified anonymous assessment session.
//!
//! An anonymous participant receives a short-lived proof when an assessment session is created.
//! Another part of the application verifies that proof and builds an [`AnonymousSessionContext`].
//! This module does **not** read or verify the raw secret. Instead, it answers a narrower question:
//! "May this verified anonymous session act on this exact assessment-session resource right now?"
//!
//! The answer is deliberately limited. The verified session may act only on the one assessment
//! session named in its context. It cannot be reused to read results, change consent, exercise data
//! rights, administer a tenant, or access another participant's session.
//!
//! Transports that already loaded a participant and session should call
//! [`authorize_anonymous_session_command`]. That function builds the resource from those stored
//! records so a caller cannot invent a matching tenant/owner/session triple and then command a
//! different loaded session.

use crate::anonymous_session::AnonymousSessionContext;
use crate::authorization::{ResourceKind, ResourceScope};
use crate::participant::ParticipantRecord;
use crate::session::AssessmentSession;
use std::error::Error;
use std::fmt::{Display, Formatter};

/// Fail-closed authorization error for a verified anonymous assessment session.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum AnonymousResourceAuthorizationError {
    /// The caller did not provide a positive time obtained from the trusted server clock.
    InvalidTimestamp,
    /// The anonymous session had reached or passed its exclusive expiry time.
    Expired,
    /// The target resource belonged to another tenant.
    CrossTenantDenied,
    /// Anonymous-session access was requested for a resource other than an assessment session.
    ResourceKindMismatch,
    /// The target session belonged to another operational participant.
    OwnerMismatch,
    /// The target assessment-session reference differed from the session named by the proof.
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

/// Allow a verified anonymous participant to act on one exact assessment session.
///
/// Callers provide three values:
///
/// - `actor`: an [`AnonymousSessionContext`] created only after the short-lived anonymous proof has
///   already been verified;
/// - `resource`: the [`ResourceScope`] for the assessment session the caller wants to use; and
/// - `now_unix_ms`: the current time from the application's trusted server clock, not a client clock.
///
/// For example, if the verified context names tenant `tenant_alpha`, participant
/// `participant_alpha`, and session `session_alpha`, this function allows access only to the
/// `session_alpha` assessment-session resource owned by that same participant in that same tenant.
/// A result resource or `session_beta` is denied even when the same caller presents the context.
///
/// References in `actor` and `resource` are already in their validated, exact spelling because
/// their constructors reject non-canonical forms. This function therefore compares the exact
/// values instead of trimming, normalizing, or guessing aliases.
///
/// Checks run in a stable fail-closed order: trusted server time, expiry, tenant, resource kind,
/// participant owner, then session identity. This order is part of the error contract used by
/// transports when more than one supplied property is wrong.
///
/// # Errors
///
/// Returns [`AnonymousResourceAuthorizationError`] when the trusted time is invalid, the verified
/// anonymous session has expired, or the requested resource differs from the exact
/// tenant/participant/session binding described above.
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

/// Allow a verified anonymous participant to command one loaded assessment session.
///
/// Callers provide four values:
///
/// - `actor`: an [`AnonymousSessionContext`] created only after the short-lived anonymous proof has
///   already been verified;
/// - `participant`: the [`ParticipantRecord`] loaded from the product store for that command;
/// - `session`: the [`AssessmentSession`] loaded from the product store for that command; and
/// - `now_unix_ms`: the current time from the application's trusted server clock, not a client clock.
///
/// The function builds the resource from those loaded records. It does **not** accept a
/// caller-invented tenant, owner, or session reference. For example, a proof for
/// `session_alpha` / `participant_alpha` in `tenant_alpha` is allowed only when the loaded
/// participant is that same person in that same tenant and the loaded session is `session_alpha`
/// owned by that person. A session owned by `participant_beta`, or `session_beta` owned by the
/// same person, is denied.
///
/// # Errors
///
/// Returns [`AnonymousResourceAuthorizationError`] when trusted time is invalid, the verified
/// anonymous session has expired, the loaded participant belongs to another tenant, the loaded
/// session belongs to another participant, or the loaded session is not the session named by the
/// proof.
pub fn authorize_anonymous_session_command(
    actor: &AnonymousSessionContext,
    participant: &ParticipantRecord,
    session: &AssessmentSession,
    now_unix_ms: u64,
) -> Result<(), AnonymousResourceAuthorizationError> {
    if now_unix_ms == 0 {
        return Err(AnonymousResourceAuthorizationError::InvalidTimestamp);
    }
    if !actor.is_valid_at(now_unix_ms) {
        return Err(AnonymousResourceAuthorizationError::Expired);
    }
    if session.participant_ref() != participant.participant_ref() {
        return Err(AnonymousResourceAuthorizationError::OwnerMismatch);
    }
    let resource = ResourceScope::participant_owned(
        ResourceKind::AssessmentSession,
        participant.tenant_ref(),
        participant.participant_ref(),
        session.session_ref(),
    )
    .map_err(|_| AnonymousResourceAuthorizationError::SessionMismatch)?;
    authorize_anonymous_session(actor, &resource, now_unix_ms)
}
