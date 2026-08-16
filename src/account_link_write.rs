//! Hosted dual-proof account-link write, unlink, and returning-account recovery.
//!
//! HTTP and messaging adapters validate anonymous-session and Keyverse proofs,
//! then call these commands. This module does not parse tokens or open a socket.
//! After restore, it inspects current-projection drift before authorization so
//! a stale unique enforcer cannot accept a new account link. It then authorizes
//! both proofs, persists append-only identity-link history, and recovers the
//! same product-owned participant from a still-valid authenticated account
//! proof. Recover keeps that record only when the reconstructed current
//! tenant, issuer, and subject still match the proof, so a concurrent
//! unlink+relink cannot hand back a rebound participant. Unlink reloads
//! stored history before authorization so a stale in-memory record cannot
//! end a rebound current binding. A returning account may unlink from
//! unterminated history even while restore inspect still reports drift;
//! persist then clears that participant's derived current projection.

use crate::account_link::{
    link_authenticated_account, AccountLinkAuthorizationError, AuthenticatedAccountControl,
};
use crate::anonymous_session::AnonymousSessionContext;
use crate::participant::ParticipantRecord;
use crate::postgres_participant_identity_link::{
    inspect_identity_link_current_projection_drift, load_participant_by_current_identity_subject,
    load_participant_identity_history, persist_participant_identity_history,
    IdentityLinkPersistenceDisposition, IdentityLinkPersistenceError,
};
use postgres::Transaction;
use std::error::Error;
use std::fmt::{Display, Formatter};

/// Fail-closed error for the hosted account-link write, unlink, and recover commands.
#[derive(Debug)]
#[non_exhaustive]
pub enum AccountLinkWriteError {
    /// Dual-proof authorization rejected the link, unlink, or recover attempt.
    Authorization(AccountLinkAuthorizationError),
    /// Durable identity-link persistence or reload rejected the command.
    Persistence(IdentityLinkPersistenceError),
    /// Restore inspect found a missing or stale unique enforcer.
    CurrentProjectionDrift,
    /// The authenticated proof is not the participant's current identity link.
    ///
    /// A rebound or unused account must not end another subject's current
    /// binding. Exact replay of an already-recorded unlink for this proof is
    /// accepted separately.
    NoCurrentBinding,
}

impl Display for AccountLinkWriteError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Authorization(error) => error.fmt(formatter),
            Self::Persistence(error) => error.fmt(formatter),
            Self::CurrentProjectionDrift => formatter.write_str(
                "operators must run restore reconcile before accepting new account-link writes",
            ),
            Self::NoCurrentBinding => formatter.write_str(
                "this authenticated account is not the participant's current identity link",
            ),
        }
    }
}

impl Error for AccountLinkWriteError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Authorization(error) => Some(error),
            Self::Persistence(error) => Some(error),
            Self::CurrentProjectionDrift | Self::NoCurrentBinding => None,
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
/// Inspect runs first. When the derived unique enforcer is missing or stale,
/// the in-memory participant is left unchanged and the operator must run
/// [`crate::postgres_participant_identity_link::reconcile_identity_link_current_projections`].
/// Only a clean inspect proceeds to dual-proof authorization and persist.
/// Exact replay of the same evidence is idempotent.
///
/// If persist fails after authorization, the caller must drop the in-memory
/// participant. The transaction remains caller-owned.
///
/// # Errors
///
/// Returns [`AccountLinkWriteError::CurrentProjectionDrift`] when restore
/// inspect reports drift, [`AccountLinkWriteError::Authorization`] when
/// dual-proof checks fail, and [`AccountLinkWriteError::Persistence`] when
/// durable write or uniqueness checks fail.
pub fn persist_authorized_account_link(
    transaction: &mut Transaction<'_>,
    participant: &mut ParticipantRecord,
    anonymous_control: &AnonymousSessionContext,
    authenticated_control: &AuthenticatedAccountControl,
    link_event_ref: &str,
    linked_at_unix_ms: u64,
) -> Result<IdentityLinkPersistenceDisposition, AccountLinkWriteError> {
    let drift = inspect_identity_link_current_projection_drift(transaction)?;
    if !drift.accepts_new_account_link_writes() {
        return Err(AccountLinkWriteError::CurrentProjectionDrift);
    }
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

fn ended_link_matches_authenticated_account(
    participant: &ParticipantRecord,
    authenticated_control: &AuthenticatedAccountControl,
    link_end_event_ref: &str,
) -> bool {
    let Some(end) = participant
        .link_end_history()
        .iter()
        .find(|event| event.link_end_event_ref() == link_end_event_ref)
    else {
        return false;
    };
    participant.link_history().iter().any(|link| {
        link.link_event_ref() == end.linked_event_ref()
            && link.issuer_ref() == authenticated_control.issuer_ref()
            && link.subject_ref() == authenticated_control.subject_ref()
    })
}

/// End the current identity link when the authenticated proof still matches it.
///
/// A buyer who is signed in with the current Keyverse account can disconnect
/// that account. The command first rejects expired or unknown-time proofs. A
/// participant currently bound to another tenant, issuer, or subject fails
/// closed so unlink cannot take over a rebound identity. After a successful
/// unlink, exact replay of the same end event is idempotent.
///
/// # Errors
///
/// Returns [`AccountLinkWriteError::Authorization`] when the proof is expired,
/// the unlink time is unknown, the proof belongs to another tenant, or the
/// participant lifecycle rejects the end event.
/// Returns [`AccountLinkWriteError::NoCurrentBinding`] when the proof is not
/// the current binding and the event is not an exact historical replay of that
/// proof's ended link.
pub fn authorize_account_unlink(
    participant: &mut ParticipantRecord,
    authenticated_control: &AuthenticatedAccountControl,
    link_end_event_ref: &str,
    ended_at_unix_ms: u64,
) -> Result<(), AccountLinkWriteError> {
    if ended_at_unix_ms == 0 {
        return Err(AccountLinkAuthorizationError::InvalidTimestamp.into());
    }
    require_recoverable_account(authenticated_control, ended_at_unix_ms)?;
    if participant.tenant_ref() != authenticated_control.tenant_ref() {
        return Err(AccountLinkAuthorizationError::CrossTenantDenied.into());
    }

    let currently_matches = participant.linked_issuer_ref()
        == Some(authenticated_control.issuer_ref())
        && participant.linked_subject_ref() == Some(authenticated_control.subject_ref());
    if participant.linked_subject_ref().is_some() && !currently_matches {
        return Err(AccountLinkWriteError::NoCurrentBinding);
    }

    participant
        .record_link_end(
            link_end_event_ref,
            authenticated_control.proof_evidence_ref(),
            ended_at_unix_ms,
        )
        .map_err(AccountLinkAuthorizationError::Participant)?;

    if !ended_link_matches_authenticated_account(
        participant,
        authenticated_control,
        link_end_event_ref,
    ) {
        return Err(AccountLinkWriteError::NoCurrentBinding);
    }
    Ok(())
}

/// Reload stored history, authorize unlink, and persist the append-only end.
///
/// Inspect runs first so corrupt history still fails closed, but restore drift
/// does not block disconnect: history remains the source of truth. The
/// caller-owned participant is replaced with the stored history before
/// authorization so a stale in-memory record cannot end a rebound current
/// binding. After persist, recover with the same proof returns `None`.
///
/// # Errors
///
/// Returns [`AccountLinkWriteError::Authorization`] or
/// [`AccountLinkWriteError::NoCurrentBinding`] from
/// [`authorize_account_unlink`], and [`AccountLinkWriteError::Persistence`]
/// when stored history cannot be loaded or written. A participant that was
/// never persisted returns [`AccountLinkWriteError::NoCurrentBinding`].
pub fn persist_authorized_account_unlink(
    transaction: &mut Transaction<'_>,
    participant: &mut ParticipantRecord,
    authenticated_control: &AuthenticatedAccountControl,
    link_end_event_ref: &str,
    ended_at_unix_ms: u64,
) -> Result<IdentityLinkPersistenceDisposition, AccountLinkWriteError> {
    inspect_identity_link_current_projection_drift(transaction)?;
    if ended_at_unix_ms == 0 {
        return Err(AccountLinkAuthorizationError::InvalidTimestamp.into());
    }
    require_recoverable_account(authenticated_control, ended_at_unix_ms)?;
    let Some(mut loaded) = load_participant_identity_history(
        transaction,
        participant.participant_ref(),
        participant.tenant_ref(),
    )?
    else {
        return Err(AccountLinkWriteError::NoCurrentBinding);
    };
    authorize_account_unlink(
        &mut loaded,
        authenticated_control,
        link_end_event_ref,
        ended_at_unix_ms,
    )?;
    let disposition = persist_participant_identity_history(transaction, &loaded)?;
    *participant = loaded;
    Ok(disposition)
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
