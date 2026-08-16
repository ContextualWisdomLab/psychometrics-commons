//! Hosted dual-proof account-link write and returning-account recovery.
//!
//! HTTP and messaging adapters validate anonymous-session and Keyverse proofs,
//! then call these commands. This module does not parse tokens or open a socket.
//! It authorizes the in-memory participant, persists append-only identity-link
//! history, and recovers the same product-owned participant from a still-valid
//! authenticated account proof.

use crate::account_link::{
    link_authenticated_account, AccountLinkAuthorizationError, AuthenticatedAccountControl,
};
use crate::anonymous_session::AnonymousSessionContext;
use crate::participant::ParticipantRecord;
use crate::postgres_participant_identity_link::{
    load_participant_by_current_identity_subject, persist_participant_identity_history,
    IdentityLinkPersistenceDisposition, IdentityLinkPersistenceError,
};
use postgres::Transaction;
use std::error::Error;
use std::fmt::{Display, Formatter};

/// Fail-closed error for the hosted account-link write and recover commands.
#[derive(Debug)]
#[non_exhaustive]
pub enum AccountLinkWriteError {
    /// Dual-proof authorization rejected the link or recover attempt.
    Authorization(AccountLinkAuthorizationError),
    /// Durable identity-link persistence or reload rejected the command.
    Persistence(IdentityLinkPersistenceError),
}

impl Display for AccountLinkWriteError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Authorization(error) => error.fmt(formatter),
            Self::Persistence(error) => error.fmt(formatter),
        }
    }
}

impl Error for AccountLinkWriteError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Authorization(error) => Some(error),
            Self::Persistence(error) => Some(error),
        }
    }
}

impl From<AccountLinkAuthorizationError> for AccountLinkWriteError {
    fn from(error: AccountLinkAuthorizationError) -> Self {
        Self::Authorization(error)
    }
}

impl From<IdentityLinkPersistenceError> for AccountLinkWriteError {
    fn from(error: IdentityLinkPersistenceError) -> Self {
        Self::Persistence(error)
    }
}

/// Reject an expired or unknown-time authenticated account proof before lookup.
///
/// A returning login must still hold a current account-control proof. This check
/// runs before history lookup so an expired proof cannot probe whether a
/// participant already exists.
///
/// # Errors
///
/// Returns [`AccountLinkAuthorizationError::InvalidTimestamp`] when server time
/// is zero, and
/// [`AccountLinkAuthorizationError::AuthenticatedProofExpired`] when the proof
/// is not valid at `now_unix_ms`.
pub fn require_recoverable_account(
    authenticated_control: &AuthenticatedAccountControl,
    now_unix_ms: u64,
) -> Result<(), AccountLinkWriteError> {
    if now_unix_ms == 0 {
        return Err(AccountLinkAuthorizationError::InvalidTimestamp.into());
    }
    if !authenticated_control.is_valid_at(now_unix_ms) {
        return Err(AccountLinkAuthorizationError::AuthenticatedProofExpired.into());
    }
    Ok(())
}

/// Authorize a dual-proof account link and persist the resulting history.
///
/// The in-memory participant is linked only after both proofs are current and
/// tenant-bound. Persist then writes that history and reconciles the derived
/// current projection. Exact replay of the same evidence is idempotent.
///
/// If persist fails, the caller must drop the in-memory participant. The
/// transaction remains caller-owned.
///
/// # Errors
///
/// Returns [`AccountLinkWriteError::Authorization`] when dual-proof checks
/// fail, and [`AccountLinkWriteError::Persistence`] when durable write or
/// uniqueness checks fail.
pub fn persist_authorized_account_link(
    transaction: &mut Transaction<'_>,
    participant: &mut ParticipantRecord,
    anonymous_control: &AnonymousSessionContext,
    authenticated_control: &AuthenticatedAccountControl,
    link_event_ref: &str,
    linked_at_unix_ms: u64,
) -> Result<IdentityLinkPersistenceDisposition, AccountLinkWriteError> {
    link_authenticated_account(
        participant,
        anonymous_control,
        authenticated_control,
        link_event_ref,
        linked_at_unix_ms,
    )?;
    Ok(persist_participant_identity_history(
        transaction,
        participant,
    )?)
}

/// Keep a recovered participant only when its current binding matches the proof.
///
/// Subject lookup can race with unlink+relink under `READ COMMITTED`. A
/// still-valid proof for one issuer-scoped subject must not receive a
/// participant that is now bound to another tenant, issuer, or subject. A
/// missing or rebound record returns `None` so the caller cannot take over
/// another account's current identity.
#[must_use]
pub fn accept_recovered_participant_for_authenticated_account(
    participant: Option<ParticipantRecord>,
    authenticated_control: &AuthenticatedAccountControl,
) -> Option<ParticipantRecord> {
    let participant = participant?;
    let matches_current_binding = participant.tenant_ref() == authenticated_control.tenant_ref()
        && participant.linked_issuer_ref() == Some(authenticated_control.issuer_ref())
        && participant.linked_subject_ref() == Some(authenticated_control.subject_ref());
    matches_current_binding.then_some(participant)
}

/// Recover the participant currently bound to a still-valid authenticated account.
///
/// The proof supplies tenant, issuer, and subject. A missing current link
/// returns `None` so a valid unused account is not turned into a participant.
/// After load, the current tenant/issuer/subject must still match the proof so
/// a concurrent unlink+relink cannot hand back a rebound participant.
///
/// # Errors
///
/// Returns [`AccountLinkWriteError::Authorization`] when the proof is expired
/// or the recover time is unknown, and
/// [`AccountLinkWriteError::Persistence`] when stored history cannot be loaded.
pub fn recover_participant_for_authenticated_account(
    transaction: &mut Transaction<'_>,
    authenticated_control: &AuthenticatedAccountControl,
    now_unix_ms: u64,
) -> Result<Option<ParticipantRecord>, AccountLinkWriteError> {
    require_recoverable_account(authenticated_control, now_unix_ms)?;
    let loaded = load_participant_by_current_identity_subject(
        transaction,
        authenticated_control.tenant_ref(),
        authenticated_control.issuer_ref(),
        authenticated_control.subject_ref(),
    )?;
    Ok(accept_recovered_participant_for_authenticated_account(
        loaded,
        authenticated_control,
    ))
}
