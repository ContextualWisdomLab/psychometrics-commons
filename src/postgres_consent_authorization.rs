//! Authorize participant-owned consent before any durable ledger or outbox write.
//!
//! Domain authorization stays in [`crate::authorization`]. This adapter is the
//! product write-path gate: `ManageOwnConsent` must succeed against the
//! participant ledger before a later composition may persist consent evidence.
//! HTTP transport remains a later slice.

use crate::authorization::{
    authorize, AuthorizationContext, AuthorizationError, ProductPermission, ResourceKind,
    ResourceScope,
};
use crate::consent::ConsentLedger;
use std::error::Error;
use std::fmt::{Display, Formatter};

/// Fail-closed error for authorized consent persistence.
#[derive(Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum AuthorizedConsentPersistenceError {
    /// The authenticated actor may not manage this participant consent ledger.
    Authorization(AuthorizationError),
}

impl Display for AuthorizedConsentPersistenceError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Authorization(_) => {
                "consent persistence requires the authenticated participant to manage their own ledger"
            }
        })
    }
}

impl Error for AuthorizedConsentPersistenceError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Authorization(error) => Some(error),
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
