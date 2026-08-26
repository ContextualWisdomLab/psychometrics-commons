//! Anonymous-first participant identity and optional account-link lifecycle semantics.
//!
//! Psychometrics Commons owns the stable participant reference; Keyverse owns credentials and
//! authentication proof. The **current projection** is only the external issuer/subject link that
//! callers may use now. It is not the participant's historical identity. A successful link appends
//! immutable audit evidence: a recorded event whose identity, proof references, subject, issuer,
//! and server time are never edited in place. Ending a link appends a second event and clears only
//! that current projection; it does not delete the participant, prior link evidence, or results.
//!
//! Opaque participant, tenant, identity, event, and proof references use their exact caller-supplied
//! spelling. Leading or trailing Unicode whitespace is not silently normalized into another valid
//! identity because that would let two wire spellings name the same authorization or audit record.
//!
//! **Replay** means receiving a command whose event reference was already recorded. An exact
//! replay is idempotent: the prior outcome is reused and the same logical event is not processed a
//! second time. Reusing that reference with different evidence fails closed. A later relink is a
//! new append-only link event. Historical replays are recognized from history and therefore cannot
//! resurrect an ended link or revoke a newer current link. Server-authoritative lifecycle time may
//! stay equal between events but must never move backwards.

use crate::reference::normalized_reference;
use std::error::Error;
use std::fmt::{Display, Formatter};

/// Fail-closed error returned by participant identity-link lifecycle operations.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum AccountLinkError {
    /// A participant, tenant, issuer, subject, event, proof, or lifecycle evidence reference was invalid.
    InvalidReference,
    /// A server-authoritative timestamp was zero.
    InvalidTimestamp,
    /// Initial account linking was recorded before the participant existed.
    NonMonotonicTimestamp,
    /// A later identity-link lifecycle event would move backwards in time.
    NonMonotonicLifecycleTimestamp,
    /// The same proof reference was offered for both independent control claims.
    ProofReferenceReuse,
    /// The same account-link event reference was reused with different evidence.
    ConflictingReplay,
    /// The same link-end event reference was reused with different evidence.
    ConflictingLinkEndReplay,
    /// An already-linked participant was offered a new account-link identity.
    AlreadyLinked,
    /// A link-end operation was requested while no current identity link existed.
    NotLinked,
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
            Self::NonMonotonicLifecycleTimestamp => {
                "participant identity-link lifecycle time must not move backwards"
            }
            Self::ProofReferenceReuse => {
                "anonymous and authenticated account-link proofs must use distinct references"
            }
            Self::ConflictingReplay => {
                "participant account-link event was replayed with conflicting evidence"
            }
            Self::ConflictingLinkEndReplay => {
                "participant identity-link end event was replayed with conflicting evidence"
            }
            Self::AlreadyLinked => "participant is already linked and cannot be rebound in place",
            Self::NotLinked => "participant has no current identity link to end",
        })
    }
}

impl Error for AccountLinkError {}

/// Immutable audit evidence for one successful participant account link.
///
/// Immutable audit evidence is the append-only record used to explain what identity-link command
/// was accepted later. [`Self::link_event_ref`] is the idempotency reference: an exact retry with
/// the same event reference and evidence reuses this event instead of processing a second logical
/// link. The event stores only opaque proof references, never credentials or proof bytes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccountLinkEvent {
    link_event_ref: String,
    issuer_ref: String,
    subject_ref: String,
    anonymous_proof_ref: String,
    authenticated_proof_ref: String,
    linked_at_unix_ms: u64,
}

impl AccountLinkEvent {
    /// Return the opaque event reference used for idempotent replay detection.
    #[must_use]
    pub fn link_event_ref(&self) -> &str {
        &self.link_event_ref
    }

    /// Return the identity issuer that scopes the external subject reference.
    #[must_use]
    pub fn issuer_ref(&self) -> &str {
        &self.issuer_ref
    }

    /// Return the authenticated account identifier within its issuer.
    #[must_use]
    pub fn subject_ref(&self) -> &str {
        &self.subject_ref
    }

    /// Return the reference proving control of the anonymous participant session.
    #[must_use]
    pub fn anonymous_proof_ref(&self) -> &str {
        &self.anonymous_proof_ref
    }

    /// Return the reference proving control of the authenticated account.
    #[must_use]
    pub fn authenticated_proof_ref(&self) -> &str {
        &self.authenticated_proof_ref
    }

    /// Return the server-authoritative time at which the link was recorded.
    #[must_use]
    pub const fn linked_at_unix_ms(&self) -> u64 {
        self.linked_at_unix_ms
    }
}

/// Immutable audit evidence that a previously current identity link ended.
///
/// This record never edits the earlier [`AccountLinkEvent`]. `link_end_event_ref` is its
/// idempotency reference, `linked_event_ref` identifies the exact historical link that ended, and
/// `evidence_ref` points to the authorization/audit evidence held outside this domain object.
/// Exact replay therefore confirms the already-recorded event rather than appending a duplicate.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccountLinkEndEvent {
    link_end_event_ref: String,
    linked_event_ref: String,
    evidence_ref: String,
    ended_at_unix_ms: u64,
}

impl AccountLinkEndEvent {
    /// Return the opaque idempotency reference for this link-end event.
    #[must_use]
    pub fn link_end_event_ref(&self) -> &str {
        &self.link_end_event_ref
    }

    /// Return the historical account-link event ended by this lifecycle event.
    #[must_use]
    pub fn linked_event_ref(&self) -> &str {
        &self.linked_event_ref
    }

    /// Return the opaque authorization/audit evidence reference for ending the link.
    #[must_use]
    pub fn evidence_ref(&self) -> &str {
        &self.evidence_ref
    }

    /// Return the server-authoritative time at which the current link ended.
    #[must_use]
    pub const fn ended_at_unix_ms(&self) -> u64 {
        self.ended_at_unix_ms
    }
}

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
    link_history: Vec<AccountLinkEvent>,
    link_end_history: Vec<AccountLinkEndEvent>,
}

impl ParticipantRecord {
    /// Create an anonymous participant record with stable product identity.
    ///
    /// # Errors
    ///
    /// Returns [`AccountLinkError::InvalidReference`] for an invalid participant or tenant
    /// reference and [`AccountLinkError::InvalidTimestamp`] when creation time is zero.
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
            link_history: Vec::new(),
            link_end_history: Vec::new(),
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

    /// Return the identity issuer for the currently linked subject, when present.
    #[must_use]
    pub fn linked_issuer_ref(&self) -> Option<&str> {
        self.linked_issuer_ref.as_deref()
    }

    /// Return the current authenticated account identifier within its issuer, when present.
    #[must_use]
    pub fn linked_subject_ref(&self) -> Option<&str> {
        self.linked_subject_ref.as_deref()
    }

    /// Return the current account-link event reference, when linked.
    #[must_use]
    pub fn link_event_ref(&self) -> Option<&str> {
        self.link_event_ref.as_deref()
    }

    /// Return anonymous-session proof evidence for the current link.
    #[must_use]
    pub fn anonymous_proof_ref(&self) -> Option<&str> {
        self.anonymous_proof_ref.as_deref()
    }

    /// Return authenticated-subject proof evidence for the current link.
    #[must_use]
    pub fn authenticated_proof_ref(&self) -> Option<&str> {
        self.authenticated_proof_ref.as_deref()
    }

    /// Return the server-authoritative time of the current account link, when linked.
    #[must_use]
    pub const fn linked_at_unix_ms(&self) -> Option<u64> {
        self.linked_at_unix_ms
    }

    /// Return append-only successful account-link history.
    #[must_use]
    pub fn link_history(&self) -> &[AccountLinkEvent] {
        &self.link_history
    }

    /// Return append-only successful link-end history.
    #[must_use]
    pub fn link_end_history(&self) -> &[AccountLinkEndEvent] {
        &self.link_end_history
    }

    /// Link this stable participant identity to one issuer-scoped authenticated subject.
    ///
    /// Processing follows these steps:
    ///
    /// 1. require exact opaque-reference spelling and reject zero time;
    /// 2. look through the full history for `link_event_ref`; an exact historical replay is a
    ///    no-op, while the same reference with changed evidence fails closed;
    /// 3. reject a new event while another link is currently projected instead of silently
    ///    rebinding the participant in place;
    /// 4. require independent anonymous/authenticated proof references and monotonic time; and
    /// 5. append the new immutable event, then expose it as the current projection.
    ///
    /// A later relink is therefore possible only after a separately recorded link-end event, and
    /// its server time cannot precede that prior lifecycle event.
    ///
    /// # Errors
    ///
    /// Returns [`AccountLinkError`] for invalid evidence, proof-reference reuse, zero/backward
    /// time, conflicting replay, or attempted in-place rebinding.
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

        if let Some(existing) = self
            .link_history
            .iter()
            .find(|event| event.link_event_ref == link_event_ref)
        {
            return if existing.issuer_ref == issuer_ref
                && existing.subject_ref == subject_ref
                && existing.anonymous_proof_ref == anonymous_proof_ref
                && existing.authenticated_proof_ref == authenticated_proof_ref
                && existing.linked_at_unix_ms == linked_at_unix_ms
            {
                Ok(())
            } else {
                Err(AccountLinkError::ConflictingReplay)
            };
        }

        if self.link_event_ref.is_some() {
            return Err(AccountLinkError::AlreadyLinked);
        }
        if anonymous_proof_ref == authenticated_proof_ref {
            return Err(AccountLinkError::ProofReferenceReuse);
        }
        if linked_at_unix_ms < self.created_at_unix_ms {
            return Err(AccountLinkError::NonMonotonicTimestamp);
        }
        if let Some(previous_end) = self.link_end_history.last() {
            if linked_at_unix_ms < previous_end.ended_at_unix_ms {
                return Err(AccountLinkError::NonMonotonicLifecycleTimestamp);
            }
        }

        self.link_history.push(AccountLinkEvent {
            link_event_ref: link_event_ref.to_owned(),
            issuer_ref: issuer_ref.to_owned(),
            subject_ref: subject_ref.to_owned(),
            anonymous_proof_ref: anonymous_proof_ref.to_owned(),
            authenticated_proof_ref: authenticated_proof_ref.to_owned(),
            linked_at_unix_ms,
        });
        self.linked_issuer_ref = Some(issuer_ref.to_owned());
        self.linked_subject_ref = Some(subject_ref.to_owned());
        self.link_event_ref = Some(link_event_ref.to_owned());
        self.anonymous_proof_ref = Some(anonymous_proof_ref.to_owned());
        self.authenticated_proof_ref = Some(authenticated_proof_ref.to_owned());
        self.linked_at_unix_ms = Some(linked_at_unix_ms);
        Ok(())
    }

    /// Record that the current external identity link ended while preserving all history.
    ///
    /// The method first recognizes exact historical replay by `link_end_event_ref`. That check is
    /// deliberately performed before examining the current projection: replaying an older
    /// already-recorded link-end event after a later relink must be a no-op, otherwise a delayed
    /// duplicate could incorrectly clear the newer identity link. A genuinely new event requires
    /// a current link, must not move server-authoritative lifecycle time backwards, appends an
    /// immutable link-end record that points to the exact link event being ended, and only then
    /// clears the current external projection.
    ///
    /// # Errors
    ///
    /// Returns [`AccountLinkError`] for invalid evidence, zero/backward time, conflicting replay,
    /// or when no current link exists.
    pub fn record_link_end(
        &mut self,
        link_end_event_ref: &str,
        evidence_ref: &str,
        ended_at_unix_ms: u64,
    ) -> Result<(), AccountLinkError> {
        let link_end_event_ref = required_reference(link_end_event_ref)?;
        let evidence_ref = required_reference(evidence_ref)?;
        if ended_at_unix_ms == 0 {
            return Err(AccountLinkError::InvalidTimestamp);
        }

        if let Some(existing) = self
            .link_end_history
            .iter()
            .find(|event| event.link_end_event_ref == link_end_event_ref)
        {
            return if existing.evidence_ref == evidence_ref
                && existing.ended_at_unix_ms == ended_at_unix_ms
            {
                Ok(())
            } else {
                Err(AccountLinkError::ConflictingLinkEndReplay)
            };
        }

        let Some(linked_event_ref) = self.link_event_ref.as_deref() else {
            return Err(AccountLinkError::NotLinked);
        };
        let linked_at_unix_ms = self.linked_at_unix_ms.unwrap_or(self.created_at_unix_ms);
        if ended_at_unix_ms < linked_at_unix_ms {
            return Err(AccountLinkError::NonMonotonicLifecycleTimestamp);
        }

        self.link_end_history.push(AccountLinkEndEvent {
            link_end_event_ref: link_end_event_ref.to_owned(),
            linked_event_ref: linked_event_ref.to_owned(),
            evidence_ref: evidence_ref.to_owned(),
            ended_at_unix_ms,
        });
        self.linked_issuer_ref = None;
        self.linked_subject_ref = None;
        self.link_event_ref = None;
        self.anonymous_proof_ref = None;
        self.authenticated_proof_ref = None;
        self.linked_at_unix_ms = None;
        Ok(())
    }
}

fn required_reference(reference: &str) -> Result<&str, AccountLinkError> {
    match normalized_reference(reference) {
        Some(normalized) if normalized == reference => Ok(reference),
        Some(_) | None => Err(AccountLinkError::InvalidReference),
    }
}
