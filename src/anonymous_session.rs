//! First-class anonymous assessment authorization context.
//!
//! Anonymous assessment must not require a Keyverse account. This module carries only
//! normalized product references and a reference to server-side authorization evidence;
//! it never stores authentication secrets or performs identity-provider work. The
//! short-lived evidence lifetime is explicit so transport adapters can fail closed before
//! forwarding commands into participant-owned session resources.

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

/// Server-derived product context for one anonymous assessment session.
///
/// The context binds one tenant, participant and assessment session to opaque
/// authorization evidence and its server-authoritative validity boundary. It is a
/// product authorization input, not a bearer secret and not an identity-provider
/// credential. Callers are responsible for validating the presented short-lived
/// session proof before constructing this context.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AnonymousSessionContext {
    tenant_ref: String,
    participant_ref: String,
    session_ref: String,
    authorization_evidence_ref: String,
    valid_until_unix_ms: u64,
}

impl AnonymousSessionContext {
    /// Create a normalized anonymous-session authorization context.
    ///
    /// # Errors
    ///
    /// Returns [`AnonymousSessionContextError::InvalidReference`] when any reference
    /// is not an opaque product reference, or
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
    /// Inputs are normalized with the same opaque-reference contract used at construction.
    /// A malformed reference therefore fails closed instead of matching a stored binding.
    #[must_use]
    pub fn matches_binding(
        &self,
        tenant_ref: &str,
        participant_ref: &str,
        session_ref: &str,
    ) -> bool {
        normalized_reference(tenant_ref) == Some(self.tenant_ref.as_str())
            && normalized_reference(participant_ref) == Some(self.participant_ref.as_str())
            && normalized_reference(session_ref) == Some(self.session_ref.as_str())
    }
}

fn required_reference(reference: &str) -> Result<&str, AnonymousSessionContextError> {
    normalized_reference(reference).ok_or(AnonymousSessionContextError::InvalidReference)
}
