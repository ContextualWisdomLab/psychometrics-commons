//! Authorization data for one anonymous assessment session.
//!
//! The server creates this context after it validates a short-lived anonymous-session proof.
//! It stores the tenant, participant, assessment session, and a reference to the server record
//! that authorized the session. It does not require a Keyverse account, store a login or bearer
//! secret, or contact an identity provider. A product reference is an opaque identifier owned by
//! Psychometrics Commons; a transport adapter is the HTTP or messaging boundary that turns an
//! external request into a product command. The explicit expiry lets those adapters reject stale
//! authority before forwarding a command to the participant's assessment-session resource.

use crate::reference::normalized_reference;
use std::error::Error;
use std::fmt::{Display, Formatter};

/// Fail-closed validation error for anonymous-session authorization context.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum AnonymousSessionContextError {
    /// A tenant, participant, session, or evidence reference was blank or numeric-only.
    InvalidReference,
    /// The server-authoritative validity boundary was zero.
    InvalidValidityBoundary,
}

impl Display for AnonymousSessionContextError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidReference => {
                "anonymous-session references must be opaque non-numeric values"
            }
            Self::InvalidValidityBoundary => {
                "anonymous-session validity boundary must be greater than zero"
            }
        })
    }
}

impl Error for AnonymousSessionContextError {}

/// Server-created authorization data for one validated anonymous assessment session.
///
/// The context stores exact tenant, participant, and assessment-session references together
/// with an opaque reference to the server-side evidence that authorized them and the time at
/// which that authorization expires. The evidence reference names a server record; it is not a
/// secret that can itself authenticate a caller. Code at an HTTP or messaging boundary must
/// validate the caller's short-lived proof before constructing this context, then use the exact
/// binding and expiry checks below before forwarding a participant-session command.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AnonymousSessionContext {
    tenant_ref: String,
    participant_ref: String,
    session_ref: String,
    authorization_evidence_ref: String,
    valid_until_unix_ms: u64,
}

impl AnonymousSessionContext {
    /// Create a canonical anonymous-session authorization context.
    ///
    /// # Errors
    ///
    /// Returns [`AnonymousSessionContextError::InvalidReference`] when any reference
    /// is not an opaque product reference in canonical spelling, or
    /// [`AnonymousSessionContextError::InvalidValidityBoundary`] when
    /// `valid_until_unix_ms` is zero.
    pub fn new(
        tenant_ref: &str,
        participant_ref: &str,
        session_ref: &str,
        authorization_evidence_ref: &str,
        valid_until_unix_ms: u64,
    ) -> Result<Self, AnonymousSessionContextError> {
        if valid_until_unix_ms == 0 {
            return Err(AnonymousSessionContextError::InvalidValidityBoundary);
        }

        Ok(Self {
            tenant_ref: required_reference(tenant_ref)?.to_owned(),
            participant_ref: required_reference(participant_ref)?.to_owned(),
            session_ref: required_reference(session_ref)?.to_owned(),
            authorization_evidence_ref: required_reference(authorization_evidence_ref)?.to_owned(),
            valid_until_unix_ms,
        })
    }

    /// Return the tenant that owns the anonymous assessment.
    #[must_use]
    pub fn tenant_ref(&self) -> &str {
        &self.tenant_ref
    }

    /// Return the stable operational participant reference.
    #[must_use]
    pub fn participant_ref(&self) -> &str {
        &self.participant_ref
    }

    /// Return the exact assessment session bound to this authorization context.
    #[must_use]
    pub fn session_ref(&self) -> &str {
        &self.session_ref
    }

    /// Return the opaque server-side authorization evidence reference.
    #[must_use]
    pub fn authorization_evidence_ref(&self) -> &str {
        &self.authorization_evidence_ref
    }

    /// Return the server-authoritative exclusive validity boundary in Unix milliseconds.
    #[must_use]
    pub const fn valid_until_unix_ms(&self) -> u64 {
        self.valid_until_unix_ms
    }

    /// Return whether the context is still valid at `now_unix_ms`.
    ///
    /// Zero time is treated as invalid/unknown and therefore fails closed. The validity
    /// boundary is exclusive: a context is expired when `now_unix_ms` equals the boundary.
    #[must_use]
    pub const fn is_valid_at(&self, now_unix_ms: u64) -> bool {
        now_unix_ms != 0 && now_unix_ms < self.valid_until_unix_ms
    }

    /// Return whether tenant, participant, and assessment-session references match exactly.
    ///
    /// Candidates must already be in the canonical opaque-reference spelling that was stored
    /// at construction. Whitespace-padded aliases therefore fail closed instead of being
    /// normalized into an authorization match at the resource boundary.
    #[must_use]
    pub fn matches_binding(
        &self,
        tenant_ref: &str,
        participant_ref: &str,
        session_ref: &str,
    ) -> bool {
        exact_reference_match(&self.tenant_ref, tenant_ref)
            && exact_reference_match(&self.participant_ref, participant_ref)
            && exact_reference_match(&self.session_ref, session_ref)
    }

    /// Return whether the exact anonymous-session binding is valid at one server time.
    ///
    /// Combining binding and lifetime in one predicate prevents callers from checking only
    /// identity or only expiry when deciding whether already-validated anonymous-session
    /// evidence may be forwarded to a participant-owned assessment-session operation.
    #[must_use]
    pub fn is_valid_for_binding_at(
        &self,
        tenant_ref: &str,
        participant_ref: &str,
        session_ref: &str,
        now_unix_ms: u64,
    ) -> bool {
        self.is_valid_at(now_unix_ms)
            && self.matches_binding(tenant_ref, participant_ref, session_ref)
    }
}

fn required_reference(reference: &str) -> Result<&str, AnonymousSessionContextError> {
    match normalized_reference(reference) {
        Some(normalized) if normalized == reference => Ok(reference),
        _ => Err(AnonymousSessionContextError::InvalidReference),
    }
}

fn exact_reference_match(stored: &str, candidate: &str) -> bool {
    normalized_reference(candidate) == Some(candidate) && stored == candidate
}
