//! Purpose-bound participant data-rights request lifecycle.
//!
//! Export and deletion requests are explicit domain resources rather than flags on
//! participant records. Identity verification is required before processing, and
//! deletion completion may preserve named legal-retention exceptions without
//! pretending that retained evidence was deleted.

use crate::reference::normalized_reference;
use std::error::Error;
use std::fmt::{Display, Formatter};

/// Participant data-rights request type.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum DataRightsRequestKind {
    /// Produce a participant-scoped export of eligible product data.
    Export,
    /// Delete eligible participant data while preserving declared legal exceptions.
    Deletion,
}

/// Current lifecycle state of a participant data-rights request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum DataRightsState {
    /// Request was accepted but requester identity has not yet been verified.
    Requested,
    /// Requester identity was verified for this request.
    IdentityVerified,
    /// The requested export or deletion operation is in progress.
    Processing,
    /// All requested eligible data was processed without retention exceptions.
    Completed,
    /// Deletion completed except for explicitly retained legal/audit scopes.
    PartiallyCompleted,
}

/// Fail-closed error returned by data-rights request operations.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum DataRightsError {
    /// A reference was blank or numeric-only instead of an opaque identifier.
    InvalidReference,
    /// A timestamp was zero.
    InvalidTimestamp,
    /// A lifecycle event timestamp moved backwards.
    NonMonotonicTimestamp,
    /// Processing was attempted before identity verification.
    IdentityVerificationRequired,
    /// A retention exception was supplied for a non-deletion request.
    RetentionExceptionNotAllowed,
    /// The requested lifecycle transition is not valid from the current state.
    InvalidTransition,
}

impl Display for DataRightsError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidReference => {
                "data-rights references must be opaque non-numeric values"
            }
            Self::InvalidTimestamp => "data-rights timestamps must be greater than zero",
            Self::NonMonotonicTimestamp => "data-rights event time must not move backwards",
            Self::IdentityVerificationRequired => {
                "identity verification is required before data-rights processing"
            }
            Self::RetentionExceptionNotAllowed => {
                "retention exceptions are valid only for deletion requests"
            }
            Self::InvalidTransition => {
                "data-rights request transition is not allowed from the current state"
            }
        })
    }
}

impl Error for DataRightsError {}

/// Product-owned lifecycle resource for one participant export or deletion request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DataRightsRequest {
    request_ref: String,
    participant_ref: String,
    kind: DataRightsRequestKind,
    scope_ref: String,
    state: DataRightsState,
    requested_at_unix_ms: u64,
    latest_event_at_unix_ms: u64,
    verification_evidence_ref: Option<String>,
    operation_ref: Option<String>,
    completion_evidence_ref: Option<String>,
    retained_scope_refs: Vec<String>,
}

impl DataRightsRequest {
    /// Create a participant-scoped export or deletion request.
    ///
    /// # Errors
    ///
    /// Returns [`DataRightsError::InvalidReference`] when any reference is blank
    /// or numeric-only, and [`DataRightsError::InvalidTimestamp`] when
    /// `requested_at_unix_ms` is zero.
    pub fn new(
        request_ref: &str,
        participant_ref: &str,
        kind: DataRightsRequestKind,
        scope_ref: &str,
        requested_at_unix_ms: u64,
    ) -> Result<Self, DataRightsError> {
        if requested_at_unix_ms == 0 {
            return Err(DataRightsError::InvalidTimestamp);
        }
        Ok(Self {
            request_ref: required_reference(request_ref)?.to_owned(),
            participant_ref: required_reference(participant_ref)?.to_owned(),
            kind,
            scope_ref: required_reference(scope_ref)?.to_owned(),
            state: DataRightsState::Requested,
            requested_at_unix_ms,
            latest_event_at_unix_ms: requested_at_unix_ms,
            verification_evidence_ref: None,
            operation_ref: None,
            completion_evidence_ref: None,
            retained_scope_refs: Vec::new(),
        })
    }

    /// Return the opaque request reference.
    #[must_use]
    pub fn request_ref(&self) -> &str {
        &self.request_ref
    }

    /// Return the operational participant reference scoped to this request.
    #[must_use]
    pub fn participant_ref(&self) -> &str {
        &self.participant_ref
    }

    /// Return the versioned/purpose-bound data scope requested by the participant.
    #[must_use]
    pub fn scope_ref(&self) -> &str {
        &self.scope_ref
    }

    /// Return whether this request is for export or deletion.
    #[must_use]
    pub const fn kind(&self) -> DataRightsRequestKind {
        self.kind
    }

    /// Return the current lifecycle state.
    #[must_use]
    pub const fn state(&self) -> DataRightsState {
        self.state
    }

    /// Return the server-authoritative request time as Unix milliseconds.
    #[must_use]
    pub const fn requested_at_unix_ms(&self) -> u64 {
        self.requested_at_unix_ms
    }

    /// Return the identity-verification evidence reference when verification occurred.
    #[must_use]
    pub fn verification_evidence_ref(&self) -> Option<&str> {
        self.verification_evidence_ref.as_deref()
    }

    /// Return the durable operation reference after processing starts.
    #[must_use]
    pub fn operation_ref(&self) -> Option<&str> {
        self.operation_ref.as_deref()
    }

    /// Return the durable completion evidence reference after completion.
    #[must_use]
    pub fn completion_evidence_ref(&self) -> Option<&str> {
        self.completion_evidence_ref.as_deref()
    }

    /// Return deletion scopes retained for a declared legal or audit obligation.
    #[must_use]
    pub fn retained_scope_refs(&self) -> &[String] {
        &self.retained_scope_refs
    }

    /// Verify requester identity for this specific data-rights request.
    ///
    /// Equal server timestamps are accepted; only backward time is rejected.
    ///
    /// # Errors
    ///
    /// Returns a [`DataRightsError`] when evidence is invalid, event time moves
    /// backwards, or the request has already advanced beyond `Requested`.
    pub fn verify_identity(
        &mut self,
        verification_evidence_ref: &str,
        verified_at_unix_ms: u64,
    ) -> Result<(), DataRightsError> {
        if self.state != DataRightsState::Requested {
            return Err(DataRightsError::InvalidTransition);
        }
        let evidence_ref = required_reference(verification_evidence_ref)?;
        self.validate_event_time(verified_at_unix_ms)?;
        self.verification_evidence_ref = Some(evidence_ref.to_owned());
        self.latest_event_at_unix_ms = verified_at_unix_ms;
        self.state = DataRightsState::IdentityVerified;
        Ok(())
    }

    /// Start durable processing after requester identity has been verified.
    ///
    /// # Errors
    ///
    /// Returns [`DataRightsError::IdentityVerificationRequired`] when processing
    /// begins before identity verification, or another [`DataRightsError`] for
    /// invalid evidence, time, or lifecycle state.
    pub fn start_processing(
        &mut self,
        operation_ref: &str,
        started_at_unix_ms: u64,
    ) -> Result<(), DataRightsError> {
        if self.state == DataRightsState::Requested {
            return Err(DataRightsError::IdentityVerificationRequired);
        }
        if self.state != DataRightsState::IdentityVerified {
            return Err(DataRightsError::InvalidTransition);
        }
        let operation_ref = required_reference(operation_ref)?;
        self.validate_event_time(started_at_unix_ms)?;
        self.operation_ref = Some(operation_ref.to_owned());
        self.latest_event_at_unix_ms = started_at_unix_ms;
        self.state = DataRightsState::Processing;
        Ok(())
    }

    /// Complete a processed request and preserve any deletion retention exceptions.
    ///
    /// `retained_scope_refs` is valid only for deletion. A non-empty retained set
    /// results in [`DataRightsState::PartiallyCompleted`] so the product never
    /// represents legally retained data as deleted.
    ///
    /// # Errors
    ///
    /// Returns [`DataRightsError::RetentionExceptionNotAllowed`] for export
    /// retention exceptions, or another [`DataRightsError`] for invalid evidence,
    /// time, retained scope references, or lifecycle state.
    pub fn complete(
        &mut self,
        completion_evidence_ref: &str,
        retained_scope_refs: &[&str],
        completed_at_unix_ms: u64,
    ) -> Result<(), DataRightsError> {
        if self.state != DataRightsState::Processing {
            return Err(DataRightsError::InvalidTransition);
        }
        if self.kind != DataRightsRequestKind::Deletion && !retained_scope_refs.is_empty() {
            return Err(DataRightsError::RetentionExceptionNotAllowed);
        }
        let completion_ref = required_reference(completion_evidence_ref)?;
        self.validate_event_time(completed_at_unix_ms)?;
        let normalized_retention = retained_scope_refs
            .iter()
            .map(|reference| required_reference(reference).map(str::to_owned))
            .collect::<Result<Vec<_>, _>>()?;

        self.completion_evidence_ref = Some(completion_ref.to_owned());
        self.retained_scope_refs = normalized_retention;
        self.latest_event_at_unix_ms = completed_at_unix_ms;
        self.state = if self.retained_scope_refs.is_empty() {
            DataRightsState::Completed
        } else {
            DataRightsState::PartiallyCompleted
        };
        Ok(())
    }

    fn validate_event_time(&self, event_at_unix_ms: u64) -> Result<(), DataRightsError> {
        if event_at_unix_ms == 0 {
            return Err(DataRightsError::InvalidTimestamp);
        }
        if event_at_unix_ms < self.latest_event_at_unix_ms {
            return Err(DataRightsError::NonMonotonicTimestamp);
        }
        Ok(())
    }
}

fn required_reference(reference: &str) -> Result<&str, DataRightsError> {
    normalized_reference(reference).ok_or(DataRightsError::InvalidReference)
}
