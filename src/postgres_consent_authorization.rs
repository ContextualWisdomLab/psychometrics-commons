//! Authorize participant-owned consent before any durable ledger write.
//!
//! Domain authorization stays in [`crate::authorization`]. This adapter is the
//! product write-path gate: `ManageOwnConsent` must succeed against the
//! participant ledger before [`persist_authorized_consent_ledger`] or
//! [`persist_authorized_anonymous_consent_ledger`] may insert evidence.
//! Anonymous assessment is first-class: a current
//! [`crate::anonymous_session::AnonymousSessionContext`] may persist the
//! participant's own ledger after expiry and binding checks, without inventing
//! a Keyverse subject. Low-level [`crate::postgres_consent::persist_consent_ledger`]
//! stays available for adapter isolation tests. Outbox tail composition and HTTP
//! transport remain later slices.

use crate::anonymous_session::AnonymousSessionContext;
use crate::authorization::{
    authorize, AuthorizationContext, AuthorizationError, ProductPermission, ProductRole,
    ResourceKind, ResourceScope,
};
use crate::consent::ConsentLedger;
use crate::postgres_consent::{
    persist_consent_ledger, ConsentPersistenceDisposition, ConsentPersistenceError,
};
use postgres::Transaction;
use std::error::Error;
use std::fmt::{Display, Formatter};

/// Fail-closed error for authorized consent persistence.
#[derive(Debug)]
#[non_exhaustive]
pub enum AuthorizedConsentPersistenceError {
    /// The authenticated actor may not manage this participant consent ledger.
    Authorization(AuthorizationError),
    /// The anonymous-session proof is expired or the server time is unknown.
    AnonymousSessionExpired,
    /// The anonymous-session proof does not belong to this consent ledger.
    AnonymousBindingMismatch,
    /// The owner was authorized, but durable persistence failed closed.
    Persistence(ConsentPersistenceError),
}

impl Display for AuthorizedConsentPersistenceError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Authorization(_) => {
                "consent persistence requires the authenticated participant to manage their own ledger"
            }
            Self::AnonymousSessionExpired => {
                "anonymous-session proof expired; start or resume the assessment, then record consent again"
            }
            Self::AnonymousBindingMismatch => {
                "anonymous-session proof does not belong to this consent ledger; open the matching assessment, then record consent there"
            }
            Self::Persistence(_) => "authorized consent persistence failed after owner authorization",
        })
    }
}

impl Error for AuthorizedConsentPersistenceError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Authorization(error) => Some(error),
            Self::Persistence(error) => Some(error),
            Self::AnonymousSessionExpired | Self::AnonymousBindingMismatch => None,
        }
    }
}

/// Authorize one participant to persist their own purpose-specific consent ledger.
///
/// `consent_tenant_ref` is the tenant that owns the consent resource. It must already
/// be in canonical opaque-reference spelling. The actor must be that tenant's
/// operational participant who owns `ledger`. This check does not infer research
/// consent from service consent and does not write durable state.
///
/// # Errors
///
/// Returns [`AuthorizedConsentPersistenceError::Authorization`] when the tenant
/// reference is invalid, the actor is in another tenant, the actor lacks a
/// participant identity, or the actor does not own the ledger.
pub fn authorize_consent_propagation(
    actor: &AuthorizationContext,
    ledger: &ConsentLedger,
    consent_tenant_ref: &str,
) -> Result<(), AuthorizedConsentPersistenceError> {
    let resource = ResourceScope::participant_owned(
        ResourceKind::ConsentLedger,
        consent_tenant_ref,
        ledger.participant_ref(),
        ledger.participant_ref(),
    )
    .map_err(AuthorizedConsentPersistenceError::Authorization)?;
    authorize(actor, &resource, ProductPermission::ManageOwnConsent)
        .map_err(AuthorizedConsentPersistenceError::Authorization)
}

/// Persist one participant-owned consent ledger after `ManageOwnConsent` succeeds.
///
/// Authorization runs before any insert. A foreign participant, foreign tenant,
/// missing participant identity, or numeric tenant returns without writing.
/// Exact replay and conflicting event identity stay in
/// [`persist_consent_ledger`]. This function does not emit an outbox row.
///
/// # Errors
///
/// Returns [`AuthorizedConsentPersistenceError::Authorization`] when the actor
/// may not manage `ledger`, or [`AuthorizedConsentPersistenceError::Persistence`]
/// when the authorized write fails closed.
pub fn persist_authorized_consent_ledger(
    actor: &AuthorizationContext,
    ledger: &ConsentLedger,
    consent_tenant_ref: &str,
    transaction: &mut Transaction<'_>,
) -> Result<ConsentPersistenceDisposition, AuthorizedConsentPersistenceError> {
    authorize_consent_propagation(actor, ledger, consent_tenant_ref)?;
    persist_consent_ledger(transaction, ledger)
        .map_err(AuthorizedConsentPersistenceError::Persistence)
}

/// Authorize one current anonymous assessment session to persist its own ledger.
///
/// The session must still be valid at `now_unix_ms`. Zero time and an exclusive
/// validity boundary fail closed. The ledger participant must match the
/// anonymous session exactly; whitespace aliases are not accepted. This check
/// does not invent a Keyverse subject, infer research consent from service
/// consent, or write durable state.
///
/// # Errors
///
/// Returns [`AuthorizedConsentPersistenceError::AnonymousSessionExpired`] when
/// the session is expired or the server time is unknown,
/// [`AuthorizedConsentPersistenceError::AnonymousBindingMismatch`] when the
/// ledger belongs to another participant, or
/// [`AuthorizedConsentPersistenceError::Authorization`] when the derived
/// owner check fails closed.
pub fn authorize_anonymous_consent_propagation(
    anonymous: &AnonymousSessionContext,
    ledger: &ConsentLedger,
    now_unix_ms: u64,
) -> Result<(), AuthorizedConsentPersistenceError> {
    if !anonymous.is_valid_at(now_unix_ms) {
        return Err(AuthorizedConsentPersistenceError::AnonymousSessionExpired);
    }
    if anonymous.participant_ref() != ledger.participant_ref() {
        return Err(AuthorizedConsentPersistenceError::AnonymousBindingMismatch);
    }
    let actor = AuthorizationContext::new(
        anonymous.tenant_ref(),
        anonymous.authorization_evidence_ref(),
        Some(anonymous.participant_ref()),
        &[ProductRole::Participant],
    )
    .map_err(AuthorizedConsentPersistenceError::Authorization)?;
    authorize_consent_propagation(&actor, ledger, anonymous.tenant_ref())
}

/// Persist one participant-owned consent ledger after anonymous-session checks.
///
/// Expiry and exact participant binding run before any insert. An expired
/// session, unknown time, or foreign ledger returns without writing. This
/// function does not emit an outbox row.
///
/// # Errors
///
/// Returns [`AuthorizedConsentPersistenceError::AnonymousSessionExpired`] or
/// [`AuthorizedConsentPersistenceError::AnonymousBindingMismatch`] when the
/// anonymous session may not manage `ledger`,
/// [`AuthorizedConsentPersistenceError::Authorization`] when the derived owner
/// check fails closed, or
/// [`AuthorizedConsentPersistenceError::Persistence`] when the authorized write
/// fails closed.
pub fn persist_authorized_anonymous_consent_ledger(
    anonymous: &AnonymousSessionContext,
    ledger: &ConsentLedger,
    now_unix_ms: u64,
    transaction: &mut Transaction<'_>,
) -> Result<ConsentPersistenceDisposition, AuthorizedConsentPersistenceError> {
    authorize_anonymous_consent_propagation(anonymous, ledger, now_unix_ms)?;
    persist_consent_ledger(transaction, ledger)
        .map_err(AuthorizedConsentPersistenceError::Persistence)
}
