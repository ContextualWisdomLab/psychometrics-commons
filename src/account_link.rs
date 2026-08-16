//! Dual-proof authorization for optional anonymous-to-account linking.
//!
//! ADR-0003 requires proof of control of both the anonymous assessment session and the
//! authenticated account before a stable Psychometrics Commons participant can be linked to a
//! Keyverse-owned identity. This application boundary composes already-validated evidence from
//! those two independent trust domains. It does not parse identity tokens, accept credentials, or
//! rewrite participant history.

use crate::anonymous_session::AnonymousSessionContext;
use crate::participant::{AccountLinkError, ParticipantRecord};
use crate::reference::normalized_reference;
use std::error::Error;
use std::fmt::{Display, Formatter};

/// Fail-closed account-link authorization error.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum AccountLinkAuthorizationError {
    /// Authenticated account-control evidence contained an invalid opaque reference.
    InvalidReference,
    /// Authenticated account-control evidence had no positive server validity boundary.
    InvalidValidityBoundary,
    /// The server-authoritative link time was zero.
    InvalidTimestamp,
    /// The anonymous-session proof was no longer valid at the link boundary.
    AnonymousSessionExpired,
    /// The anonymous-session proof belonged to another tenant or participant.
    AnonymousBindingMismatch,
    /// The authenticated account-control proof was no longer valid at the link boundary.
    AuthenticatedProofExpired,
    /// Authenticated account control belonged to another tenant.
    CrossTenantDenied,
    /// Participant lifecycle validation rejected the proposed immutable link event.
    Participant(AccountLinkError),
}

impl Display for AccountLinkAuthorizationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidReference => {
                "authenticated account-control references must be canonical opaque non-numeric values"
            }
            Self::InvalidValidityBoundary => {
                "authenticated account-control validity must end after Unix epoch zero"
            }
            Self::InvalidTimestamp => "account-link server time must be greater than zero",
            Self::AnonymousSessionExpired => {
                "anonymous-session control proof is not valid at the account-link time"
            }
            Self::AnonymousBindingMismatch => {
                "anonymous-session control proof does not belong to this participant and tenant"
            }
            Self::AuthenticatedProofExpired => {
                "authenticated account-control proof is not valid at the account-link time"
            }
            Self::CrossTenantDenied => {
                "authenticated account control does not belong to the participant tenant"
            }
            Self::Participant(_) => "participant identity-link lifecycle rejected the link event",
        })
    }
}

impl Error for AccountLinkAuthorizationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Participant(error) => Some(error),
            Self::InvalidReference
            | Self::InvalidValidityBoundary
            | Self::InvalidTimestamp
            | Self::AnonymousSessionExpired
            | Self::AnonymousBindingMismatch
            | Self::AuthenticatedProofExpired
            | Self::CrossTenantDenied => None,
        }
    }
}

/// Server-validated proof that an authenticated subject currently controls one account.
///
/// Keyverse remains the credential and federation owner. A transport/authentication adapter may
/// create this value only after issuer, audience, signature, expiry, and other applicable token
/// validation succeeds. The domain retains opaque audit evidence, not token bytes or credentials.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthenticatedAccountControl {
    tenant_ref: String,
    issuer_ref: String,
    subject_ref: String,
    proof_evidence_ref: String,
    valid_until_unix_ms: u64,
}

impl AuthenticatedAccountControl {
    /// Create authenticated-account control evidence from a trusted validation boundary.
    ///
    /// References must already use their exact canonical spelling. Leading or trailing whitespace
    /// is rejected rather than silently trimmed so byte-distinct aliases cannot collapse to the
    /// same identity or audit evidence at this authorization boundary.
    ///
    /// # Errors
    ///
    /// Returns [`AccountLinkAuthorizationError::InvalidReference`] when tenant, issuer, subject,
    /// or proof evidence is blank, numeric-like, or not already in canonical spelling, and
    /// [`AccountLinkAuthorizationError::InvalidValidityBoundary`] when the validity boundary is
    /// zero.
    pub fn new(
        tenant_ref: &str,
        issuer_ref: &str,
        subject_ref: &str,
        proof_evidence_ref: &str,
        valid_until_unix_ms: u64,
    ) -> Result<Self, AccountLinkAuthorizationError> {
        if valid_until_unix_ms == 0 {
            return Err(AccountLinkAuthorizationError::InvalidValidityBoundary);
        }
        Ok(Self {
            tenant_ref: required_reference(tenant_ref)?.to_owned(),
            issuer_ref: required_reference(issuer_ref)?.to_owned(),
            subject_ref: required_reference(subject_ref)?.to_owned(),
            proof_evidence_ref: required_reference(proof_evidence_ref)?.to_owned(),
            valid_until_unix_ms,
        })
    }

    /// Return the tenant asserted by the validated authenticated context.
    #[must_use]
    pub fn tenant_ref(&self) -> &str {
        &self.tenant_ref
    }

    /// Return the validated identity issuer reference.
    #[must_use]
    pub fn issuer_ref(&self) -> &str {
        &self.issuer_ref
    }

    /// Return the validated issuer-scoped authenticated subject reference.
    #[must_use]
    pub fn subject_ref(&self) -> &str {
        &self.subject_ref
    }

    /// Return opaque audit evidence for the successful authenticated proof validation.
    #[must_use]
    pub fn proof_evidence_ref(&self) -> &str {
        &self.proof_evidence_ref
    }

    /// Return the exclusive server-authoritative validity boundary in Unix milliseconds.
    #[must_use]
    pub const fn valid_until_unix_ms(&self) -> u64 {
        self.valid_until_unix_ms
    }

    /// Return whether authenticated account-control evidence is valid at one server time.
    #[must_use]
    pub const fn is_valid_at(&self, now_unix_ms: u64) -> bool {
        now_unix_ms > 0 && now_unix_ms < self.valid_until_unix_ms
    }
}

/// Link an anonymous-first participant to an authenticated account using two current proofs.
///
/// Authorization order deliberately rejects invalid time, anonymous binding/expiry, authenticated
/// expiry, and cross-tenant identity before invoking the append-only participant lifecycle. A
/// successful call records the anonymous authorization evidence and authenticated proof evidence
/// as distinct audit references through [`ParticipantRecord::link_account`]. The participant
/// reference is never replaced.
///
/// Exact replay semantics remain owned by [`ParticipantRecord`]: reusing the same event reference
/// with exactly the same evidence is idempotent, while conflicting replay fails closed.
///
/// # Errors
///
/// Returns [`AccountLinkAuthorizationError`] for zero server time, invalid or mismatched proof
/// evidence, expired evidence, cross-tenant authenticated control, or participant lifecycle
/// rejection.
pub fn link_authenticated_account(
    participant: &mut ParticipantRecord,
    anonymous_control: &AnonymousSessionContext,
    authenticated_control: &AuthenticatedAccountControl,
    link_event_ref: &str,
    linked_at_unix_ms: u64,
) -> Result<(), AccountLinkAuthorizationError> {
    if linked_at_unix_ms == 0 {
        return Err(AccountLinkAuthorizationError::InvalidTimestamp);
    }
    if anonymous_control.tenant_ref() != participant.tenant_ref()
        || anonymous_control.participant_ref() != participant.participant_ref()
    {
        return Err(AccountLinkAuthorizationError::AnonymousBindingMismatch);
    }
    if !anonymous_control.is_valid_at(linked_at_unix_ms) {
        return Err(AccountLinkAuthorizationError::AnonymousSessionExpired);
    }
    if !authenticated_control.is_valid_at(linked_at_unix_ms) {
        return Err(AccountLinkAuthorizationError::AuthenticatedProofExpired);
    }
    if authenticated_control.tenant_ref() != participant.tenant_ref() {
        return Err(AccountLinkAuthorizationError::CrossTenantDenied);
    }

    participant
        .link_account(
            link_event_ref,
            authenticated_control.issuer_ref(),
            authenticated_control.subject_ref(),
            anonymous_control.authorization_evidence_ref(),
            authenticated_control.proof_evidence_ref(),
            linked_at_unix_ms,
        )
        .map_err(AccountLinkAuthorizationError::Participant)
}

fn required_reference(reference: &str) -> Result<&str, AccountLinkAuthorizationError> {
    let normalized =
        normalized_reference(reference).ok_or(AccountLinkAuthorizationError::InvalidReference)?;
    if normalized != reference {
        return Err(AccountLinkAuthorizationError::InvalidReference);
    }
    Ok(normalized)
}
