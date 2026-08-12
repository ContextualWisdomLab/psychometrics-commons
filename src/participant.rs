//! Anonymous-first participant identity and optional account-link semantics.
//!
//! Psychometrics Commons owns stable operational participant references. Keyverse
//! owns credentials and authentication proof. Linking records an identity issuer
//! together with a provider-scoped subject: the subject is an account identifier
//! that is only unique within that issuer. A proof-of-control reference points to
//! durable evidence that the caller controlled an identity; it is not the credential
//! itself. Linking never replaces the historical product-owned participant identifier.
//! Exact replay is idempotent, meaning the same event with the same evidence succeeds
//! without changing state again. Invalid or conflicting evidence fails closed: it is
//! rejected without changing the existing link. Rebinding means replacing an already
//! linked issuer/subject pair, which this primitive does not allow silently.

use crate::reference::normalized_reference;
use std::error::Error;
use std::fmt::{Display, Formatter};

/// Fail-closed error returned by participant creation or account linking.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum AccountLinkError {
    /// A participant, tenant, issuer, subject, event, or proof reference was invalid.
    InvalidReference,
    /// A server-authoritative timestamp was zero.
    InvalidTimestamp,
    /// Account linking was recorded before the participant existed.
    NonMonotonicTimestamp,
    /// The same proof reference was offered for both independent control claims.
    ProofReferenceReuse,
    /// The same account-link event reference was reused with different evidence.
    ConflictingReplay,
    /// An already-linked participant was offered a new account-link identity.
    AlreadyLinked,
}

impl Display for AccountLinkError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidReference => {
                "participant account-link references must be opaque non-numeric values"
            }
            Self::InvalidTimestamp => {
                "participant account-link timestamps must be greater than zero"
            }
            Self::NonMonotonicTimestamp => {
                "participant account-link time must not precede participant creation"
            }
            Self::ProofReferenceReuse => {
                "anonymous and authenticated account-link proofs must use distinct references"
            }
            Self::ConflictingReplay => {
                "participant account-link event was replayed with conflicting evidence"
            }
            Self::AlreadyLinked => "participant is already linked and cannot be rebound in place",
        })
    }
}

impl Error for AccountLinkError {}

/// Stable product-owned participant identity with optional issuer-scoped account linkage.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParticipantRecord {
    participant_ref: String,
    tenant_ref: String,
    created_at_unix_ms: u64,
    linked_issuer_ref: Option<String>,
    linked_subject_ref: Option<String>,
    link_event_ref: Option<String>,
    anonymous_proof_ref: Option<String>,
    authenticated_proof_ref: Option<String>,
    linked_at_unix_ms: Option<u64>,
}

impl ParticipantRecord {
    /// Create an anonymous participant record with stable product identity.
    ///
    /// # Errors
    ///
    /// Returns [`AccountLinkError::InvalidReference`] when participant or tenant
    /// reference is blank/numeric-only and [`AccountLinkError::InvalidTimestamp`]
    /// when `created_at_unix_ms` is zero.
    pub fn new_anonymous(
        participant_ref: &str,
        tenant_ref: &str,
        created_at_unix_ms: u64,
    ) -> Result<Self, AccountLinkError> {
        if created_at_unix_ms == 0 {
            return Err(AccountLinkError::InvalidTimestamp);
        }
        Ok(Self {
            participant_ref: required_reference(participant_ref)?.to_owned(),
            tenant_ref: required_reference(tenant_ref)?.to_owned(),
            created_at_unix_ms,
            linked_issuer_ref: None,
            linked_subject_ref: None,
            link_event_ref: None,
            anonymous_proof_ref: None,
            authenticated_proof_ref: None,
            linked_at_unix_ms: None,
        })
    }

    /// Return the stable operational participant reference.
    #[must_use]
    pub fn participant_ref(&self) -> &str {
        &self.participant_ref
    }

    /// Return the tenant that owns this participant record.
    #[must_use]
    pub fn tenant_ref(&self) -> &str {
        &self.tenant_ref
    }

    /// Return the server-authoritative participant creation time.
    #[must_use]
    pub const fn created_at_unix_ms(&self) -> u64 {
        self.created_at_unix_ms
    }

    /// Return the identity issuer for the linked authenticated subject, when present.
    #[must_use]
    pub fn linked_issuer_ref(&self) -> Option<&str> {
        self.linked_issuer_ref.as_deref()
    }

    /// Return the authenticated account identifier within its issuer, when present.
    ///
    /// The same subject text may legitimately identify different accounts under
    /// different issuers, so callers must interpret it together with
    /// [`Self::linked_issuer_ref`].
    #[must_use]
    pub fn linked_subject_ref(&self) -> Option<&str> {
        self.linked_subject_ref.as_deref()
    }

    /// Return the account-link event idempotency reference, when linked.
    ///
    /// Reusing this event reference with exactly the same evidence is a safe no-op;
    /// reusing it with changed evidence is rejected.
    #[must_use]
    pub fn link_event_ref(&self) -> Option<&str> {
        self.link_event_ref.as_deref()
    }

    /// Return proof that the caller controlled the anonymous participant session.
    #[must_use]
    pub fn anonymous_proof_ref(&self) -> Option<&str> {
        self.anonymous_proof_ref.as_deref()
    }

    /// Return proof that the caller controlled the authenticated Keyverse subject.
    #[must_use]
    pub fn authenticated_proof_ref(&self) -> Option<&str> {
        self.authenticated_proof_ref.as_deref()
    }

    /// Return the server-authoritative account-link time, when linked.
    #[must_use]
    pub const fn linked_at_unix_ms(&self) -> Option<u64> {
        self.linked_at_unix_ms
    }

    /// Link this stable participant identity to one issuer-scoped authenticated subject.
    ///
    /// The issuer and subject together identify the external account; the subject is
    /// only unique within its issuer. Both proof references are mandatory and must
    /// differ because they point to separate evidence that the caller controlled the
    /// anonymous participant and the authenticated account. Replaying the exact same
    /// event and evidence is idempotent: it succeeds without changing state again.
    /// Reusing the event with changed issuer, subject, proof, or time evidence fails
    /// closed, meaning the operation returns an error and leaves the existing link
    /// unchanged. Once linked, a different event also cannot silently rebind, or
    /// replace, the participant's issuer/subject pair; future unlink/relink policy
    /// requires an explicit audited lifecycle.
    ///
    /// # Errors
    ///
    /// Returns [`AccountLinkError`] for invalid evidence, proof-identity reuse,
    /// zero/backward time, conflicting event replay, or attempted in-place rebinding.
    pub fn link_account(
        &mut self,
        link_event_ref: &str,
        issuer_ref: &str,
        subject_ref: &str,
        anonymous_proof_ref: &str,
        authenticated_proof_ref: &str,
        linked_at_unix_ms: u64,
    ) -> Result<(), AccountLinkError> {
        let link_event_ref = required_reference(link_event_ref)?;
        let issuer_ref = required_reference(issuer_ref)?;
        let subject_ref = required_reference(subject_ref)?;
        let anonymous_proof_ref = required_reference(anonymous_proof_ref)?;
        let authenticated_proof_ref = required_reference(authenticated_proof_ref)?;
        if linked_at_unix_ms == 0 {
            return Err(AccountLinkError::InvalidTimestamp);
        }

        if let Some(existing_event_ref) = self.link_event_ref.as_deref() {
            if existing_event_ref == link_event_ref {
                return if self.linked_issuer_ref.as_deref() == Some(issuer_ref)
                    && self.linked_subject_ref.as_deref() == Some(subject_ref)
                    && self.anonymous_proof_ref.as_deref() == Some(anonymous_proof_ref)
                    && self.authenticated_proof_ref.as_deref() == Some(authenticated_proof_ref)
                    && self.linked_at_unix_ms == Some(linked_at_unix_ms)
                {
                    Ok(())
                } else {
                    Err(AccountLinkError::ConflictingReplay)
                };
            }
            return Err(AccountLinkError::AlreadyLinked);
        }

        if anonymous_proof_ref == authenticated_proof_ref {
            return Err(AccountLinkError::ProofReferenceReuse);
        }
        if linked_at_unix_ms < self.created_at_unix_ms {
            return Err(AccountLinkError::NonMonotonicTimestamp);
        }

        self.linked_issuer_ref = Some(issuer_ref.to_owned());
        self.linked_subject_ref = Some(subject_ref.to_owned());
        self.link_event_ref = Some(link_event_ref.to_owned());
        self.anonymous_proof_ref = Some(anonymous_proof_ref.to_owned());
        self.authenticated_proof_ref = Some(authenticated_proof_ref.to_owned());
        self.linked_at_unix_ms = Some(linked_at_unix_ms);
        Ok(())
    }
}

fn required_reference(reference: &str) -> Result<&str, AccountLinkError> {
    normalized_reference(reference).ok_or(AccountLinkError::InvalidReference)
}
