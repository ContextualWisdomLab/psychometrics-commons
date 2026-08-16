//! Consented longitudinal program enrollment for Gyeot-collected EMA/ESM.
//!
//! This module owns product enrollment, purpose-specific consent binding, and
//! explicit multiple-membership context. It does not collect mobile observations,
//! implement TEPP temporal or multilevel kernels, or rewrite historical
//! enrollment evidence after withdrawal.

use crate::consent::{ConsentPurpose, ConsentSnapshot};
use crate::participant::ParticipantRecord;
use crate::reference::normalized_reference;
use std::collections::HashSet;
use std::error::Error;
use std::fmt::{Display, Formatter};

/// Lifecycle state of one consented longitudinal program enrollment.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum EnrollmentState {
    /// The participant is enrolled and Gyeot may collect observations.
    Enrolled,
    /// Collection is paused while the enrollment and membership evidence remain.
    Paused,
    /// The participant left the program. Historical enrollment evidence remains.
    Withdrawn,
}

/// Borrowed input for one new longitudinal enrollment.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LongitudinalEnrollmentInput<'a> {
    /// Opaque enrollment identity minted by the product runtime.
    pub enrollment_ref: &'a str,
    /// Tenant that owns the participant and program.
    pub tenant_ref: &'a str,
    /// Operational participant who granted longitudinal observation consent.
    pub participant_ref: &'a str,
    /// Versioned EMA/ESM program the participant is joining.
    pub program_ref: &'a str,
    /// Collection-system reference. Gyeot owns collection; this is not a TEPP id.
    pub collection_system_ref: &'a str,
    /// Explicit multiple-membership contexts. Duplicates are rejected.
    pub membership_context_refs: &'a [&'a str],
    /// Server-authoritative enrollment time as Unix milliseconds.
    pub enrolled_at_unix_ms: u64,
}

/// Product-owned enrollment that authorizes Gyeot collection for one program.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LongitudinalEnrollment {
    enrollment_ref: String,
    tenant_ref: String,
    participant_ref: String,
    program_ref: String,
    collection_system_ref: String,
    consent_snapshot_ref: String,
    membership_context_refs: Vec<String>,
    state: EnrollmentState,
    enrolled_at_unix_ms: u64,
    latest_event_ref: Option<String>,
    latest_event_at_unix_ms: u64,
}

/// Fail-closed error for longitudinal enrollment commands.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum LongitudinalEnrollmentError {
    /// A required reference was blank or numeric-like after normalization.
    EmptyReference,
    /// The consent snapshot belongs to a different operational participant.
    ParticipantMismatch,
    /// Longitudinal observation consent is missing or revoked.
    LongitudinalConsentRequired,
    /// Enrollment time is zero or not after the authorizing consent grant.
    InvalidStartTime,
    /// The same membership context was declared more than once.
    DuplicateMembershipContext,
    /// The requested pause, resume, or withdraw is not legal in this state.
    InvalidTransition,
    /// A later command used a server time at or before the last event.
    NonMonotonicTimestamp,
    /// The enrollment is already withdrawn and the new evidence does not match.
    AlreadyWithdrawn,
    /// The caller tenant does not own the participant record.
    CrossTenantDenied,
}

impl Display for LongitudinalEnrollmentError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::EmptyReference => {
                "copy an opaque enrollment, tenant, participant, program, collection-system, membership, or event reference instead of a blank or numeric id"
            }
            Self::ParticipantMismatch => {
                "use the consent snapshot that belongs to this participant"
            }
            Self::LongitudinalConsentRequired => {
                "ask the participant to grant longitudinal observation consent before enrollment"
            }
            Self::InvalidStartTime => {
                "enroll only after the longitudinal consent grant, with a non-zero server time"
            }
            Self::DuplicateMembershipContext => {
                "declare each membership context once; do not collapse duplicates into one group"
            }
            Self::InvalidTransition => {
                "use pause only while enrolled, resume only while paused, and withdraw from an open enrollment"
            }
            Self::NonMonotonicTimestamp => {
                "use a later server time than the last enrollment event"
            }
            Self::AlreadyWithdrawn => {
                "this enrollment is already withdrawn; replay the same withdrawal evidence or start a new enrollment"
            }
            Self::CrossTenantDenied => {
                "enroll this participant only under the tenant that owns the participant record"
            }
        })
    }
}

impl Error for LongitudinalEnrollmentError {}

impl LongitudinalEnrollment {
    /// Enroll one participant in a Gyeot-collected program after consent.
    ///
    /// Research refusal does not block personal EMA/ESM enrollment. Membership
    /// contexts stay distinct so later TEPP analysis is not flattened to one
    /// primary group.
    ///
    /// # Errors
    ///
    /// Returns [`LongitudinalEnrollmentError`] when a reference is invalid, the
    /// participant record does not own the tenant or participant, the snapshot
    /// belongs to another participant, longitudinal consent is missing or
    /// revoked, enrollment time is not after the grant, or a membership context
    /// is duplicated.
    pub fn enroll(
        input: LongitudinalEnrollmentInput<'_>,
        participant: &ParticipantRecord,
        snapshot: &ConsentSnapshot,
    ) -> Result<Self, LongitudinalEnrollmentError> {
        let enrollment_ref = required_reference(input.enrollment_ref)?;
        let tenant_ref = required_reference(input.tenant_ref)?;
        let participant_ref = required_reference(input.participant_ref)?;
        let program_ref = required_reference(input.program_ref)?;
        let collection_system_ref = required_reference(input.collection_system_ref)?;
        if participant.participant_ref() != participant_ref
            || snapshot.participant_ref() != participant_ref
        {
            return Err(LongitudinalEnrollmentError::ParticipantMismatch);
        }
        if participant.tenant_ref() != tenant_ref {
            return Err(LongitudinalEnrollmentError::CrossTenantDenied);
        }
        let granted_at = snapshot
            .active_granted_at(ConsentPurpose::LongitudinalObservation)
            .ok_or(LongitudinalEnrollmentError::LongitudinalConsentRequired)?;
        if input.enrolled_at_unix_ms == 0 || input.enrolled_at_unix_ms <= granted_at {
            return Err(LongitudinalEnrollmentError::InvalidStartTime);
        }
        let membership_context_refs = unique_memberships(input.membership_context_refs)?;

        Ok(Self {
            enrollment_ref: enrollment_ref.to_owned(),
            tenant_ref: tenant_ref.to_owned(),
            participant_ref: participant_ref.to_owned(),
            program_ref: program_ref.to_owned(),
            collection_system_ref: collection_system_ref.to_owned(),
            consent_snapshot_ref: snapshot.snapshot_ref().to_owned(),
            membership_context_refs,
            state: EnrollmentState::Enrolled,
            enrolled_at_unix_ms: input.enrolled_at_unix_ms,
            latest_event_ref: None,
            latest_event_at_unix_ms: input.enrolled_at_unix_ms,
        })
    }

    /// Return the opaque enrollment identity.
    #[must_use]
    pub fn enrollment_ref(&self) -> &str {
        &self.enrollment_ref
    }

    /// Return the tenant that owns this enrollment.
    #[must_use]
    pub fn tenant_ref(&self) -> &str {
        &self.tenant_ref
    }

    /// Return the operational participant bound to this enrollment.
    #[must_use]
    pub fn participant_ref(&self) -> &str {
        &self.participant_ref
    }

    /// Return the versioned program the participant joined.
    #[must_use]
    pub fn program_ref(&self) -> &str {
        &self.program_ref
    }

    /// Return the Gyeot collection-system reference.
    #[must_use]
    pub fn collection_system_ref(&self) -> &str {
        &self.collection_system_ref
    }

    /// Return the consent snapshot that authorized enrollment.
    #[must_use]
    pub fn consent_snapshot_ref(&self) -> &str {
        &self.consent_snapshot_ref
    }

    /// Return explicit membership contexts in declaration order.
    #[must_use]
    pub fn membership_context_refs(&self) -> &[String] {
        &self.membership_context_refs
    }

    /// Return the current enrollment lifecycle state.
    #[must_use]
    pub const fn state(&self) -> EnrollmentState {
        self.state
    }

    /// Return when enrollment began as Unix milliseconds.
    #[must_use]
    pub const fn enrolled_at_unix_ms(&self) -> u64 {
        self.enrolled_at_unix_ms
    }

    /// Return the latest pause, resume, or withdraw event reference.
    #[must_use]
    pub fn latest_event_ref(&self) -> Option<&str> {
        self.latest_event_ref.as_deref()
    }

    /// Return the latest enrollment-event time as Unix milliseconds.
    #[must_use]
    pub const fn latest_event_at_unix_ms(&self) -> u64 {
        self.latest_event_at_unix_ms
    }

    /// Authorize Gyeot collection from the current consent snapshot.
    ///
    /// Enrollment state alone is not enough. A later longitudinal revoke, a
    /// snapshot for another participant, a pause, or a withdrawal fail closed.
    ///
    /// # Errors
    ///
    /// Returns [`LongitudinalEnrollmentError`] when the snapshot belongs to
    /// another participant, the enrollment is paused or withdrawn, or
    /// longitudinal observation consent is missing or revoked.
    pub fn authorize_collection(
        &self,
        snapshot: &ConsentSnapshot,
    ) -> Result<(), LongitudinalEnrollmentError> {
        if snapshot.participant_ref() != self.participant_ref {
            return Err(LongitudinalEnrollmentError::ParticipantMismatch);
        }
        match self.state {
            EnrollmentState::Withdrawn => Err(LongitudinalEnrollmentError::AlreadyWithdrawn),
            EnrollmentState::Paused => Err(LongitudinalEnrollmentError::InvalidTransition),
            EnrollmentState::Enrolled => snapshot
                .active_granted_at(ConsentPurpose::LongitudinalObservation)
                .ok_or(LongitudinalEnrollmentError::LongitudinalConsentRequired)
                .map(|_| ()),
        }
    }

    /// Pause collection while keeping enrollment and membership evidence.
    ///
    /// Exact replay of the same pause evidence is idempotent.
    ///
    /// # Errors
    ///
    /// Returns [`LongitudinalEnrollmentError`] for a blank event reference, a
    /// withdrawn enrollment, a pause that is not later than the last event, or
    /// a pause attempted while already paused with different evidence.
    pub fn pause(
        &self,
        event_ref: &str,
        paused_at_unix_ms: u64,
    ) -> Result<Self, LongitudinalEnrollmentError> {
        let event_ref = required_reference(event_ref)?;
        match self.state {
            EnrollmentState::Withdrawn => Err(LongitudinalEnrollmentError::AlreadyWithdrawn),
            EnrollmentState::Paused => {
                self.exact_replay(event_ref, paused_at_unix_ms, EnrollmentState::Paused)
            }
            EnrollmentState::Enrolled => {
                self.advance(event_ref, paused_at_unix_ms, EnrollmentState::Paused)
            }
        }
    }

    /// Resume collection after a pause.
    ///
    /// Exact replay of the same resume evidence is idempotent.
    ///
    /// # Errors
    ///
    /// Returns [`LongitudinalEnrollmentError`] for a blank event reference, a
    /// withdrawn enrollment, a resume that is not later than the last event, or
    /// a resume attempted while already enrolled with different evidence.
    pub fn resume(
        &self,
        event_ref: &str,
        resumed_at_unix_ms: u64,
    ) -> Result<Self, LongitudinalEnrollmentError> {
        let event_ref = required_reference(event_ref)?;
        match self.state {
            EnrollmentState::Withdrawn => Err(LongitudinalEnrollmentError::AlreadyWithdrawn),
            EnrollmentState::Enrolled => {
                self.exact_replay(event_ref, resumed_at_unix_ms, EnrollmentState::Enrolled)
            }
            EnrollmentState::Paused => {
                self.advance(event_ref, resumed_at_unix_ms, EnrollmentState::Enrolled)
            }
        }
    }

    /// Withdraw from the program without erasing enrollment evidence.
    ///
    /// Exact replay of the same withdrawal evidence is idempotent.
    ///
    /// # Errors
    ///
    /// Returns [`LongitudinalEnrollmentError`] for a blank event reference, a
    /// withdrawal that is not later than the last event, or a second
    /// conflicting withdrawal.
    pub fn withdraw(
        &self,
        event_ref: &str,
        withdrawn_at_unix_ms: u64,
    ) -> Result<Self, LongitudinalEnrollmentError> {
        let event_ref = required_reference(event_ref)?;
        if self.state == EnrollmentState::Withdrawn {
            return self.exact_replay(event_ref, withdrawn_at_unix_ms, EnrollmentState::Withdrawn);
        }
        self.advance(event_ref, withdrawn_at_unix_ms, EnrollmentState::Withdrawn)
    }

    fn exact_replay(
        &self,
        event_ref: &str,
        event_at_unix_ms: u64,
        expected_state: EnrollmentState,
    ) -> Result<Self, LongitudinalEnrollmentError> {
        if self.state == expected_state
            && self.latest_event_ref.as_deref() == Some(event_ref)
            && self.latest_event_at_unix_ms == event_at_unix_ms
        {
            return Ok(self.clone());
        }
        if self.state == EnrollmentState::Withdrawn {
            return Err(LongitudinalEnrollmentError::AlreadyWithdrawn);
        }
        if event_at_unix_ms <= self.latest_event_at_unix_ms {
            return Err(LongitudinalEnrollmentError::NonMonotonicTimestamp);
        }
        Err(LongitudinalEnrollmentError::InvalidTransition)
    }

    fn advance(
        &self,
        event_ref: &str,
        event_at_unix_ms: u64,
        target: EnrollmentState,
    ) -> Result<Self, LongitudinalEnrollmentError> {
        if event_at_unix_ms <= self.latest_event_at_unix_ms {
            return Err(LongitudinalEnrollmentError::NonMonotonicTimestamp);
        }
        Ok(self.with_event(event_ref, event_at_unix_ms, target))
    }

    fn with_event(&self, event_ref: &str, event_at_unix_ms: u64, state: EnrollmentState) -> Self {
        let mut next = self.clone();
        next.state = state;
        next.latest_event_ref = Some(event_ref.to_owned());
        next.latest_event_at_unix_ms = event_at_unix_ms;
        next
    }
}

fn required_reference(reference: &str) -> Result<&str, LongitudinalEnrollmentError> {
    normalized_reference(reference).ok_or(LongitudinalEnrollmentError::EmptyReference)
}

fn unique_memberships(
    membership_context_refs: &[&str],
) -> Result<Vec<String>, LongitudinalEnrollmentError> {
    let mut seen = HashSet::with_capacity(membership_context_refs.len());
    let mut normalized = Vec::with_capacity(membership_context_refs.len());
    for membership_ref in membership_context_refs {
        let membership_ref = required_reference(membership_ref)?;
        if !seen.insert(membership_ref.to_owned()) {
            return Err(LongitudinalEnrollmentError::DuplicateMembershipContext);
        }
        normalized.push(membership_ref.to_owned());
    }
    Ok(normalized)
}

#[cfg(test)]
mod enrollment_error_source_tests {
    use super::LongitudinalEnrollmentError;
    use std::error::Error;

    #[test]
    fn enrollment_errors_carry_no_nested_source() {
        for error in [
            LongitudinalEnrollmentError::EmptyReference,
            LongitudinalEnrollmentError::ParticipantMismatch,
            LongitudinalEnrollmentError::LongitudinalConsentRequired,
            LongitudinalEnrollmentError::InvalidStartTime,
            LongitudinalEnrollmentError::DuplicateMembershipContext,
            LongitudinalEnrollmentError::InvalidTransition,
            LongitudinalEnrollmentError::NonMonotonicTimestamp,
            LongitudinalEnrollmentError::AlreadyWithdrawn,
            LongitudinalEnrollmentError::CrossTenantDenied,
        ] {
            assert!(error.source().is_none());
        }
    }
}
