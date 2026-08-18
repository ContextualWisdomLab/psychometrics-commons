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
//! Transports that already hold a participant and session should call
//! [`authorize_anonymous_session_command`]. That function compares the verified actor to the
//! supplied records so a matching invented [`ResourceScope`] cannot authorize a different
//! supplied session. It does not prove the records came from the product store.

use crate::anonymous_session::AnonymousSessionContext;
use crate::authorization::{ResourceKind, ResourceScope};
use crate::participant::ParticipantRecord;
use crate::session::{AssessmentSession, SessionCommand, SessionState, TransitionError};
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

/// Allow a verified anonymous participant to command one supplied assessment session.
///
/// Callers provide four values:
///
/// - `actor`: an [`AnonymousSessionContext`] created only after the short-lived anonymous proof has
///   already been verified;
/// - `participant`: the [`ParticipantRecord`] the caller supplies for that command;
/// - `session`: the [`AssessmentSession`] the caller supplies for that command; and
/// - `now_unix_ms`: the current time from the application's trusted server clock, not a client clock.
///
/// The function compares the actor to those supplied records. It does **not** accept a
/// caller-built [`ResourceScope`]. It does not prove the records were loaded from the product
/// store; a transport can still construct both aggregates from the proof.
/// Persist/reload of live measurement sessions is implemented. Append-only
/// identity-link history persist remains a later slice. This gate still does not
/// prove the records were store-loaded. For example, a proof for
/// `session_alpha` /
/// `participant_alpha` in `tenant_alpha` is allowed only when the supplied participant is that
/// same person in that same tenant and the supplied session is `session_alpha` owned by that
/// person. A session owned by `participant_beta`, or `session_beta` owned by the same person,
/// is denied.
///
/// Checks run in a stable fail-closed order: trusted server time, expiry, supplied-participant
/// tenant, session/participant ownership, actor participant, then session identity.
/// Tenant is classified before ownership so a foreign-tenant record that also disagrees on
/// participant identity is reported as [`AnonymousResourceAuthorizationError::CrossTenantDenied`].
///
/// # Errors
///
/// Returns [`AnonymousResourceAuthorizationError`] when trusted time is invalid, the verified
/// anonymous session has expired, the supplied participant belongs to another tenant, the
/// supplied session belongs to another participant, or the supplied session is not the session
/// named by the proof.
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
    if actor.tenant_ref() != participant.tenant_ref() {
        return Err(AnonymousResourceAuthorizationError::CrossTenantDenied);
    }
    if session.participant_ref() != participant.participant_ref()
        || actor.participant_ref() != participant.participant_ref()
    {
        return Err(AnonymousResourceAuthorizationError::OwnerMismatch);
    }
    if actor.session_ref() != session.session_ref() {
        return Err(AnonymousResourceAuthorizationError::SessionMismatch);
    }
    Ok(())
}

/// Fail-closed error for applying a session command after anonymous authorization.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum AnonymousSessionCommandError {
    /// The verified anonymous session was not allowed to command the supplied session.
    Authorization(AnonymousResourceAuthorizationError),
    /// Authorization succeeded, but the lifecycle command was not legal for the current state.
    Transition(TransitionError),
}

impl Display for AnonymousSessionCommandError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Authorization(error) => Display::fmt(error, formatter),
            Self::Transition(error) => Display::fmt(error, formatter),
        }
    }
}

impl Error for AnonymousSessionCommandError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Authorization(error) => Some(error),
            Self::Transition(error) => Some(error),
        }
    }
}

/// Apply one session command only after the supplied session is authorized.
///
/// Call this from an HTTP or messaging adapter after the short-lived anonymous proof has been
/// verified. Pass the participant and session records the caller holds. This function does not
/// prove the records were loaded from the product store. Authorization runs first. If it fails,
/// the session is left unchanged. If it succeeds, the existing session lifecycle rules decide
/// whether the command may change state.
///
/// For example, a current proof for `session_alpha` may activate that supplied session. The same
/// proof cannot activate `session_beta`, and an expired proof cannot activate `session_alpha` even
/// though `Activate` is otherwise legal from `Created`.
///
/// # Errors
///
/// Returns [`AnonymousSessionCommandError::Authorization`] when the supplied records are not the
/// exact current anonymous session, or [`AnonymousSessionCommandError::Transition`] when the
/// command is not legal from the current lifecycle state.
pub fn apply_anonymous_session_command(
    actor: &AnonymousSessionContext,
    participant: &ParticipantRecord,
    session: &mut AssessmentSession,
    command_ref: &str,
    sequence: u64,
    command: SessionCommand,
    now_unix_ms: u64,
) -> Result<SessionState, AnonymousSessionCommandError> {
    authorize_anonymous_session_command(actor, participant, session, now_unix_ms)
        .map_err(AnonymousSessionCommandError::Authorization)?;
    session
        .apply_command(command_ref, sequence, command)
        .map_err(AnonymousSessionCommandError::Transition)
}
