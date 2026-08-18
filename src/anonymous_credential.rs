//! Short-lived anonymous assessment credential evidence.
//!
//! Psychometrics Commons must support anonymous participation without moving credential
//! ownership into Keyverse. A transport adapter may issue a high-entropy bearer proof, hash it
//! before it enters this domain boundary, and persist only the canonical SHA-256 digest together
//! with the exact tenant, participant, and assessment-session binding. Raw bearer proofs are not
//! accepted or stored here.
//!
//! This module intentionally does not generate randomness or hash secrets. Those are transport
//! and secret-handling responsibilities that require a reviewed cryptographic implementation.
//! The domain contract instead makes the server-authoritative lifetime, exact resource binding,
//! stored proof hash, and append-only revocation evidence explicit so a later HTTP adapter cannot
//! silently widen anonymous-session authority.

use crate::anonymous_session::AnonymousSessionContext;
use crate::reference::normalized_reference;
use std::error::Error;
use std::fmt::{Display, Formatter};

/// Fail-closed validation or lifecycle error for anonymous credential evidence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum AnonymousCredentialError {
    /// A credential, tenant, participant, or session reference was not an opaque reference.
    InvalidReference,
    /// The stored proof digest was not canonical lowercase `sha256:<64 hex>` evidence.
    InvalidDigest,
    /// A server-authoritative issuance, expiry, or revocation timestamp was zero or impossible.
    InvalidTimestamp,
    /// The exclusive expiry boundary did not occur after issuance.
    InvalidLifetime,
    /// A revocation replay tried to replace already-recorded immutable revocation evidence.
    ConflictingRevocation,
    /// The presented proof hash, resource references, or server time failed authorization.
    Unauthorized,
}

impl Display for AnonymousCredentialError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidReference => {
                "anonymous credential references must be exact canonical opaque non-numeric values"
            }
            Self::InvalidDigest => {
                "anonymous credential proof digest must be canonical lowercase SHA-256 evidence"
            }
            Self::InvalidTimestamp => {
                "anonymous credential server timestamps must be positive and ordered"
            }
            Self::InvalidLifetime => {
                "anonymous credential expiry must occur strictly after issuance"
            }
            Self::ConflictingRevocation => {
                "anonymous credential revocation evidence cannot be replaced"
            }
            Self::Unauthorized => {
                "present a current exact digest for this tenant, participant, and session"
            }
        })
    }
}

impl Error for AnonymousCredentialError {}

/// Immutable anonymous-session authority plus append-only revocation evidence.
///
/// The record stores only a SHA-256 hash of the bearer proof, called the proof digest. The raw
/// credential must remain outside application persistence and routine logs. The resource binding
/// is the exact tenant, participant, and assessment-session identity that the proof may authorize.
/// Authorization succeeds only when the caller presents that exact stored proof hash and resource
/// binding at a server time inside the issuance and expiry window and before any recorded
/// revocation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AnonymousCredential {
    credential_ref: String,
    tenant_ref: String,
    participant_ref: String,
    session_ref: String,
    proof_digest: String,
    issued_at_unix_ms: u64,
    expires_at_unix_ms: u64,
    revoked_at_unix_ms: Option<u64>,
}

impl AnonymousCredential {
    /// Create one exactly spelled server-side anonymous credential record.
    ///
    /// Resource references must already use their canonical spelling. The proof must already have
    /// been hashed by the trusted credential-issuance boundary. This constructor accepts only
    /// canonical lowercase SHA-256 digest evidence and therefore never receives the raw bearer
    /// secret.
    ///
    /// # Errors
    ///
    /// Returns [`AnonymousCredentialError::InvalidReference`] for malformed or noncanonical
    /// product references, [`AnonymousCredentialError::InvalidDigest`] for noncanonical digest
    /// evidence, [`AnonymousCredentialError::InvalidTimestamp`] when either lifetime boundary is
    /// zero, or [`AnonymousCredentialError::InvalidLifetime`] when expiry is not strictly after
    /// issuance.
    pub fn new(
        credential_ref: &str,
        tenant_ref: &str,
        participant_ref: &str,
        session_ref: &str,
        proof_digest: &str,
        issued_at_unix_ms: u64,
        expires_at_unix_ms: u64,
    ) -> Result<Self, AnonymousCredentialError> {
        let credential_ref = required_reference(credential_ref)?;
        let tenant_ref = required_reference(tenant_ref)?;
        let participant_ref = required_reference(participant_ref)?;
        let session_ref = required_reference(session_ref)?;
        if canonical_sha256_digest(proof_digest) != Some(proof_digest) {
            return Err(AnonymousCredentialError::InvalidDigest);
        }
        if issued_at_unix_ms == 0 || expires_at_unix_ms == 0 {
            return Err(AnonymousCredentialError::InvalidTimestamp);
        }
        if expires_at_unix_ms <= issued_at_unix_ms {
            return Err(AnonymousCredentialError::InvalidLifetime);
        }

        Ok(Self {
            credential_ref: credential_ref.to_owned(),
            tenant_ref: tenant_ref.to_owned(),
            participant_ref: participant_ref.to_owned(),
            session_ref: session_ref.to_owned(),
            proof_digest: proof_digest.to_owned(),
            issued_at_unix_ms,
            expires_at_unix_ms,
            revoked_at_unix_ms: None,
        })
    }

    /// Return the opaque server-side credential record reference.
    #[must_use]
    pub fn credential_ref(&self) -> &str {
        &self.credential_ref
    }

    /// Return the tenant that owns the authorized anonymous session.
    #[must_use]
    pub fn tenant_ref(&self) -> &str {
        &self.tenant_ref
    }

    /// Return the stable operational participant reference bound to the credential.
    #[must_use]
    pub fn participant_ref(&self) -> &str {
        &self.participant_ref
    }

    /// Return the exact assessment-session reference bound to the credential.
    #[must_use]
    pub fn session_ref(&self) -> &str {
        &self.session_ref
    }

    /// Return the canonical digest of the bearer proof, never the raw bearer proof.
    #[must_use]
    pub fn proof_digest(&self) -> &str {
        &self.proof_digest
    }

    /// Return the inclusive server-authoritative issuance time in Unix milliseconds.
    #[must_use]
    pub const fn issued_at_unix_ms(&self) -> u64 {
        self.issued_at_unix_ms
    }

    /// Return the exclusive server-authoritative expiry time in Unix milliseconds.
    #[must_use]
    pub const fn expires_at_unix_ms(&self) -> u64 {
        self.expires_at_unix_ms
    }

    /// Return immutable revocation evidence when the credential has been revoked.
    #[must_use]
    pub const fn revoked_at_unix_ms(&self) -> Option<u64> {
        self.revoked_at_unix_ms
    }

    /// Return whether the credential was valid at one server-authoritative time.
    ///
    /// Zero time and times before issuance fail closed. Expiry is exclusive. When revocation has
    /// been recorded, the revocation boundary is also exclusive authority: historical times before
    /// revocation remain representable, while the credential is invalid at and after revocation.
    #[must_use]
    pub const fn is_valid_at(&self, now_unix_ms: u64) -> bool {
        if now_unix_ms == 0
            || now_unix_ms < self.issued_at_unix_ms
            || now_unix_ms >= self.expires_at_unix_ms
        {
            return false;
        }
        match self.revoked_at_unix_ms {
            Some(revoked_at_unix_ms) => now_unix_ms < revoked_at_unix_ms,
            None => true,
        }
    }

    /// Return whether one already-hashed proof is authorized for the exact resource binding.
    ///
    /// The presented digest is the SHA-256 hash of the caller's bearer proof. The resource binding
    /// is the tenant, participant, and session that proof is allowed to access. All four values
    /// must already use their canonical spelling. This prevents whitespace-padded aliases from
    /// widening resource authority. Digest comparison is performed without early exit after
    /// canonical validation so comparison work does not reveal a matching prefix.
    #[must_use]
    pub fn authorizes(
        &self,
        presented_proof_digest: &str,
        tenant_ref: &str,
        participant_ref: &str,
        session_ref: &str,
        now_unix_ms: u64,
    ) -> bool {
        self.is_valid_at(now_unix_ms)
            && exact_reference_match(&self.tenant_ref, tenant_ref)
            && exact_reference_match(&self.participant_ref, participant_ref)
            && exact_reference_match(&self.session_ref, session_ref)
            && canonical_sha256_digest(presented_proof_digest) == Some(presented_proof_digest)
            && constant_time_equal(
                self.proof_digest.as_bytes(),
                presented_proof_digest.as_bytes(),
            )
    }

    /// Record append-only credential revocation evidence.
    ///
    /// An exact replay of the same revocation timestamp is idempotent. A different later replay
    /// fails closed rather than rewriting the original server evidence.
    ///
    /// # Errors
    ///
    /// Returns [`AnonymousCredentialError::InvalidTimestamp`] when revocation is zero or precedes
    /// issuance, or [`AnonymousCredentialError::ConflictingRevocation`] when immutable revocation
    /// evidence already exists with a different timestamp.
    pub fn revoke(&mut self, revoked_at_unix_ms: u64) -> Result<(), AnonymousCredentialError> {
        if revoked_at_unix_ms == 0 || revoked_at_unix_ms < self.issued_at_unix_ms {
            return Err(AnonymousCredentialError::InvalidTimestamp);
        }
        match self.revoked_at_unix_ms {
            Some(existing) if existing == revoked_at_unix_ms => Ok(()),
            Some(_) => Err(AnonymousCredentialError::ConflictingRevocation),
            None => {
                self.revoked_at_unix_ms = Some(revoked_at_unix_ms);
                Ok(())
            }
        }
    }

    /// Create the exact anonymous-session context this credential authorizes now.
    ///
    /// A transport calls this after hashing the caller's bearer proof. The proof digest is that
    /// SHA-256 hash. The binding is the exact tenant, participant, and assessment session the proof
    /// may access. When all of those values and the current server time match this credential, the
    /// method returns an [`AnonymousSessionContext`] naming this server-side credential record as
    /// its authorization evidence. The context's initial expiry is the earlier of credential
    /// expiry or any revocation already recorded. Raw proof material never enters the context.
    ///
    /// A context is an immutable snapshot. If this credential is revoked later, callers must use
    /// [`AnonymousCredential::authorizes_session_context_at`] with the current credential record
    /// before forwarding another protected operation.
    ///
    /// # Errors
    ///
    /// Returns [`AnonymousCredentialError::Unauthorized`] when the presented proof hash, tenant,
    /// participant, session, or server time does not currently authorize this credential.
    ///
    /// # Panics
    ///
    /// Panics only if an already-authorized credential somehow lacks a valid session-context
    /// binding. [`AnonymousCredential::new`] rejects those inputs, so a panic means an internal
    /// invariant was broken rather than a caller mistake.
    pub fn session_context(
        &self,
        presented_proof_digest: &str,
        tenant_ref: &str,
        participant_ref: &str,
        session_ref: &str,
        now_unix_ms: u64,
    ) -> Result<AnonymousSessionContext, AnonymousCredentialError> {
        if !self.authorizes(
            presented_proof_digest,
            tenant_ref,
            participant_ref,
            session_ref,
            now_unix_ms,
        ) {
            return Err(AnonymousCredentialError::Unauthorized);
        }
        Ok(AnonymousSessionContext::new(
            self.tenant_ref(),
            self.participant_ref(),
            self.session_ref(),
            self.credential_ref(),
            self.authority_expires_at_unix_ms(),
        )
        .expect("an authorized credential already carries valid session-context inputs"))
    }

    /// Return whether this current credential still authorizes an existing session context.
    ///
    /// The session context is a snapshot made after an earlier proof check. Its embedded expiry
    /// cannot change if this credential is revoked later. A server therefore resolves the
    /// context's `authorization_evidence_ref` to the current credential record and calls this
    /// method before each protected operation. Authorization succeeds only while the current
    /// credential itself is valid, the context names this exact credential record, and the
    /// context still names this credential's tenant, participant, and assessment session.
    #[must_use]
    pub fn authorizes_session_context_at(
        &self,
        context: &AnonymousSessionContext,
        now_unix_ms: u64,
    ) -> bool {
        self.is_valid_at(now_unix_ms)
            && self.credential_ref == context.authorization_evidence_ref()
            && context.is_valid_for_binding_at(
                &self.tenant_ref,
                &self.participant_ref,
                &self.session_ref,
                now_unix_ms,
            )
    }

    const fn authority_expires_at_unix_ms(&self) -> u64 {
        match self.revoked_at_unix_ms {
            Some(revoked_at_unix_ms) if revoked_at_unix_ms < self.expires_at_unix_ms => {
                revoked_at_unix_ms
            }
            _ => self.expires_at_unix_ms,
        }
    }
}

fn required_reference(reference: &str) -> Result<&str, AnonymousCredentialError> {
    match normalized_reference(reference) {
        Some(normalized) if normalized == reference => Ok(reference),
        _ => Err(AnonymousCredentialError::InvalidReference),
    }
}

fn exact_reference_match(stored: &str, candidate: &str) -> bool {
    normalized_reference(candidate) == Some(candidate) && stored == candidate
}

fn canonical_sha256_digest(digest: &str) -> Option<&str> {
    let hexadecimal = digest.strip_prefix("sha256:")?;
    if hexadecimal.len() == 64
        && hexadecimal
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        Some(digest)
    } else {
        None
    }
}

fn constant_time_equal(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut difference = 0_u8;
    for (&left_byte, &right_byte) in left.iter().zip(right) {
        difference |= left_byte ^ right_byte;
    }
    difference == 0
}

#[cfg(test)]
mod tests {
    use super::{canonical_sha256_digest, constant_time_equal};

    #[test]
    fn digest_hex_accepts_digits_and_proof_comparison_rejects_length_mismatch() {
        assert_eq!(
            canonical_sha256_digest(
                "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
            ),
            Some("sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef")
        );
        assert!(!constant_time_equal(b"proof", b"p"));
        assert!(constant_time_equal(b"proof", b"proof"));
    }
}
