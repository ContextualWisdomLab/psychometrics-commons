//! Purpose-specific consent evidence and research-contribution lifecycle.
//!
//! Consent is represented as an append-only event ledger. A snapshot derives
//! the latest decision for each purpose without erasing earlier evidence, and
//! research contribution can begin only from a snapshot containing an explicit
//! active research grant with a versioned research scope.

use crate::reference::canonical_opaque_reference;
use std::error::Error;
use std::fmt::{Display, Formatter};

/// Purpose controlled by one independently revocable consent decision.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum ConsentPurpose {
    /// Processing required to operate the requested assessment service.
    ServiceOperation,
    /// Optional persistence of results across sessions or devices.
    AccountPersistence,
    /// Optional longitudinal or EMA/ESM observation processing.
    LongitudinalObservation,
    /// Optional contribution of assessment data to a declared research scope.
    ResearchContribution,
    /// Optional product or research communications.
    Communications,
}

/// Append-only decision recorded for one consent purpose.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ConsentDecision {
    /// The participant explicitly granted the purpose.
    Granted,
    /// The participant explicitly revoked a prior grant.
    Revoked,
}

/// Borrowed input for one immutable consent event.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConsentEventInput<'a> {
    /// Opaque server-side event reference used for idempotent replay.
    pub event_ref: &'a str,
    /// Purpose affected by this decision.
    pub purpose: ConsentPurpose,
    /// Granted or revoked decision.
    pub decision: ConsentDecision,
    /// Exact consent-form version shown for this purpose.
    pub consent_form_version_ref: &'a str,
    /// Required research scope only for [`ConsentPurpose::ResearchContribution`].
    pub research_scope_ref: Option<&'a str>,
    /// Server-authoritative event time as Unix milliseconds.
    pub occurred_at_unix_ms: u64,
}

/// One immutable normalized consent event.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConsentEvent {
    event_ref: String,
    purpose: ConsentPurpose,
    decision: ConsentDecision,
    consent_form_version_ref: String,
    research_scope_ref: Option<String>,
    occurred_at_unix_ms: u64,
}

impl ConsentEvent {
    /// Return the opaque event reference.
    #[must_use]
    pub fn event_ref(&self) -> &str {
        &self.event_ref
    }

    /// Return the consent purpose changed by this event.
    #[must_use]
    pub const fn purpose(&self) -> ConsentPurpose {
        self.purpose
    }

    /// Return whether the purpose was granted or revoked.
    #[must_use]
    pub const fn decision(&self) -> ConsentDecision {
        self.decision
    }

    /// Return the exact consent-form version shown for this event.
    #[must_use]
    pub fn consent_form_version_ref(&self) -> &str {
        &self.consent_form_version_ref
    }

    /// Return the research scope when this is a research-purpose event.
    #[must_use]
    pub fn research_scope_ref(&self) -> Option<&str> {
        self.research_scope_ref.as_deref()
    }

    /// Return the server-authoritative event time as Unix milliseconds.
    #[must_use]
    pub const fn occurred_at_unix_ms(&self) -> u64 {
        self.occurred_at_unix_ms
    }
}

/// Immutable derived view of latest consent decisions at one ledger point.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConsentSnapshot {
    snapshot_ref: String,
    participant_ref: String,
    events: Vec<ConsentEvent>,
}

impl ConsentSnapshot {
    /// Return the opaque immutable snapshot reference.
    #[must_use]
    pub fn snapshot_ref(&self) -> &str {
        &self.snapshot_ref
    }

    /// Return the operational participant reference bound to the snapshot.
    #[must_use]
    pub fn participant_ref(&self) -> &str {
        &self.participant_ref
    }

    /// Return how many append-only consent events are represented.
    #[must_use]
    pub fn event_count(&self) -> usize {
        self.events.len()
    }

    /// Return whether the latest decision for `purpose` is an active grant.
    #[must_use]
    pub fn is_granted(&self, purpose: ConsentPurpose) -> bool {
        self.latest_event(purpose)
            .is_some_and(|event| event.decision == ConsentDecision::Granted)
    }

    /// Return the consent-form version for an active grant, if present.
    #[must_use]
    pub fn active_form_version(&self, purpose: ConsentPurpose) -> Option<&str> {
        self.latest_event(purpose).and_then(|event| {
            (event.decision == ConsentDecision::Granted)
                .then_some(event.consent_form_version_ref.as_str())
        })
    }

    /// Return the active research scope only while research consent is granted.
    #[must_use]
    pub fn active_research_scope(&self) -> Option<&str> {
        self.active_research_authorization()
            .map(|(scope_ref, _)| scope_ref)
    }

    fn active_research_authorization(&self) -> Option<(&str, u64)> {
        self.latest_event(ConsentPurpose::ResearchContribution)
            .and_then(|event| {
                (event.decision == ConsentDecision::Granted)
                    .then_some(event.research_scope_ref.as_deref())
                    .flatten()
                    .map(|scope_ref| (scope_ref, event.occurred_at_unix_ms))
            })
    }

    fn latest_event(&self, purpose: ConsentPurpose) -> Option<&ConsentEvent> {
        self.events
            .iter()
            .rev()
            .find(|event| event.purpose == purpose)
    }
}

/// Append-only in-memory domain ledger defining consent-event semantics.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConsentLedger {
    participant_ref: String,
    events: Vec<ConsentEvent>,
}

impl ConsentLedger {
    /// Create an empty purpose-specific consent ledger for one participant.
    ///
    /// # Errors
    ///
    /// Returns [`ConsentWriteError::EmptyReference`] when `participant_ref` is
    /// blank, whitespace-padded, control-bearing, or numeric-like.
    pub fn new(participant_ref: &str) -> Result<Self, ConsentWriteError> {
        let participant_ref = required_reference(participant_ref)?;
        Ok(Self {
            participant_ref: participant_ref.to_owned(),
            events: Vec::new(),
        })
    }

    /// Return the number of immutable consent events retained by the ledger.
    #[must_use]
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// Return whether no consent event has been recorded.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// Return the operational participant bound to this ledger.
    #[must_use]
    pub fn participant_ref(&self) -> &str {
        &self.participant_ref
    }

    /// Return accepted consent events in server-authoritative order.
    #[must_use]
    pub fn events(&self) -> &[ConsentEvent] {
        &self.events
    }

    /// Record a normalized consent event or replay an identical prior event.
    ///
    /// Research-purpose events require a non-empty research scope. Other
    /// purposes reject a research scope so purpose boundaries cannot be blurred.
    /// New events must not move server-authoritative event time backwards;
    /// identical retries remain idempotent even after later events exist.
    ///
    /// # Errors
    ///
    /// Returns a [`ConsentWriteError`] for invalid references/timestamps,
    /// research-scope misuse, conflicting reuse of an event reference, or a new
    /// event whose server-authoritative time predates the latest accepted event.
    pub fn record(
        &mut self,
        input: ConsentEventInput<'_>,
    ) -> Result<ConsentEvent, ConsentWriteError> {
        let event_ref = required_reference(input.event_ref)?;
        let form_version_ref = required_reference(input.consent_form_version_ref)?;
        if input.occurred_at_unix_ms == 0 {
            return Err(ConsentWriteError::InvalidTimestamp);
        }

        let research_scope_ref = match (input.purpose, input.research_scope_ref) {
            (ConsentPurpose::ResearchContribution, Some(scope_ref)) => {
                Some(required_reference(scope_ref)?.to_owned())
            }
            (ConsentPurpose::ResearchContribution, None) => {
                return Err(ConsentWriteError::ResearchScopeRequired);
            }
            (_, Some(_)) => return Err(ConsentWriteError::ResearchScopeNotAllowed),
            (_, None) => None,
        };

        let candidate = ConsentEvent {
            event_ref: event_ref.to_owned(),
            purpose: input.purpose,
            decision: input.decision,
            consent_form_version_ref: form_version_ref.to_owned(),
            research_scope_ref,
            occurred_at_unix_ms: input.occurred_at_unix_ms,
        };

        if let Some(existing) = self
            .events
            .iter()
            .find(|event| event.event_ref == candidate.event_ref)
        {
            if existing == &candidate {
                return Ok(existing.clone());
            }
            return Err(ConsentWriteError::EventReferenceConflict);
        }

        if self
            .events
            .last()
            .is_some_and(|event| candidate.occurred_at_unix_ms < event.occurred_at_unix_ms)
        {
            return Err(ConsentWriteError::NonMonotonicTimestamp);
        }

        self.events.push(candidate.clone());
        Ok(candidate)
    }

    /// Freeze the current append-only consent evidence into an immutable snapshot.
    ///
    /// # Errors
    ///
    /// Returns [`ConsentWriteError::EmptyReference`] for a blank snapshot
    /// reference.
    pub fn snapshot_as(&self, snapshot_ref: &str) -> Result<ConsentSnapshot, ConsentWriteError> {
        let snapshot_ref = required_reference(snapshot_ref)?;
        Ok(ConsentSnapshot {
            snapshot_ref: snapshot_ref.to_owned(),
            participant_ref: self.participant_ref.clone(),
            events: self.events.clone(),
        })
    }
}

/// Fail-closed error returned while recording or snapshotting consent evidence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ConsentWriteError {
    /// A required reference is blank, noncanonical, or numeric-like.
    EmptyReference,
    /// Research consent did not declare a research scope.
    ResearchScopeRequired,
    /// A non-research purpose attempted to carry a research scope.
    ResearchScopeNotAllowed,
    /// A consent event used an invalid zero timestamp.
    InvalidTimestamp,
    /// An event reference was reused for different immutable content.
    EventReferenceConflict,
    /// A new event attempted to move server-authoritative time backwards.
    NonMonotonicTimestamp,
}

impl Display for ConsentWriteError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::EmptyReference => "consent references must be exact opaque non-numeric values without surrounding whitespace or unsafe control characters",
            Self::ResearchScopeRequired => "research consent requires a research scope",
            Self::ResearchScopeNotAllowed => {
                "research scope is allowed only for research-contribution consent"
            }
            Self::InvalidTimestamp => "consent event timestamp must be greater than zero",
            Self::EventReferenceConflict => {
                "consent event reference was already used for different evidence"
            }
            Self::NonMonotonicTimestamp => {
                "consent event timestamp must not precede the latest accepted event"
            }
        })
    }
}

impl Error for ConsentWriteError {}

/// Lifecycle state of an explicitly opted-in research contribution.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ResearchContributionState {
    /// Research contribution may enter approved downstream staging workflows.
    Active,
    /// Future research use is blocked according to the applicable withdrawal policy.
    Withdrawn,
}

/// Immutable research-contribution state derived from explicit research consent.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResearchContribution {
    contribution_ref: String,
    research_participant_ref: String,
    consent_snapshot_ref: String,
    research_scope_ref: String,
    state: ResearchContributionState,
    started_at_unix_ms: u64,
    withdrawal_event_ref: Option<String>,
    withdrawn_at_unix_ms: Option<u64>,
}

impl ResearchContribution {
    /// Begin research contribution from an explicit active research-consent snapshot.
    ///
    /// # Errors
    ///
    /// Returns [`ResearchContributionError::ResearchConsentRequired`] unless the
    /// supplied snapshot contains an active research grant,
    /// [`ResearchContributionError::OperationalIdentityReuse`] when the research
    /// identity reuses the normalized operational participant reference, an
    /// empty-reference error for invalid new identities, or an invalid-start-time
    /// error when the contribution start is zero or predates the authorizing
    /// research consent.
    pub fn from_snapshot(
        contribution_ref: &str,
        research_participant_ref: &str,
        snapshot: &ConsentSnapshot,
        started_at_unix_ms: u64,
    ) -> Result<Self, ResearchContributionError> {
        let contribution_ref = research_reference(contribution_ref)?;
        let research_participant_ref = research_reference(research_participant_ref)?;
        if research_participant_ref == snapshot.participant_ref {
            return Err(ResearchContributionError::OperationalIdentityReuse);
        }
        let (research_scope_ref, research_granted_at_unix_ms) = snapshot
            .active_research_authorization()
            .ok_or(ResearchContributionError::ResearchConsentRequired)?;
        if started_at_unix_ms == 0 || started_at_unix_ms < research_granted_at_unix_ms {
            return Err(ResearchContributionError::InvalidStartTime);
        }

        Ok(Self {
            contribution_ref: contribution_ref.to_owned(),
            research_participant_ref: research_participant_ref.to_owned(),
            consent_snapshot_ref: snapshot.snapshot_ref.clone(),
            research_scope_ref: research_scope_ref.to_owned(),
            state: ResearchContributionState::Active,
            started_at_unix_ms,
            withdrawal_event_ref: None,
            withdrawn_at_unix_ms: None,
        })
    }

    /// Return the opaque research-contribution reference.
    #[must_use]
    pub fn contribution_ref(&self) -> &str {
        &self.contribution_ref
    }

    /// Return the pseudonymous research participant, never the operational identity.
    #[must_use]
    pub fn research_participant_ref(&self) -> &str {
        &self.research_participant_ref
    }

    /// Return the immutable consent snapshot authorizing this contribution.
    #[must_use]
    pub fn consent_snapshot_ref(&self) -> &str {
        &self.consent_snapshot_ref
    }

    /// Return the explicit research scope authorized by the consent snapshot.
    #[must_use]
    pub fn research_scope_ref(&self) -> &str {
        &self.research_scope_ref
    }

    /// Return whether contribution is active or withdrawn.
    #[must_use]
    pub const fn state(&self) -> ResearchContributionState {
        self.state
    }

    /// Return when contribution began as Unix milliseconds.
    #[must_use]
    pub const fn started_at_unix_ms(&self) -> u64 {
        self.started_at_unix_ms
    }

    /// Return the immutable withdrawal event reference after withdrawal.
    #[must_use]
    pub fn withdrawal_event_ref(&self) -> Option<&str> {
        self.withdrawal_event_ref.as_deref()
    }

    /// Return the withdrawal time after withdrawal.
    #[must_use]
    pub const fn withdrawn_at_unix_ms(&self) -> Option<u64> {
        self.withdrawn_at_unix_ms
    }

    /// Return a new withdrawn state while preserving the original contribution.
    ///
    /// Replaying the exact same withdrawal is idempotent. Any different attempt
    /// after withdrawal fails closed so an immutable withdrawal cannot be changed.
    ///
    /// # Errors
    ///
    /// Returns a [`ResearchContributionError`] for a blank withdrawal reference,
    /// a withdrawal not later than contribution start, or a second conflicting
    /// withdrawal after the contribution is already withdrawn.
    pub fn withdraw(
        &self,
        withdrawal_event_ref: &str,
        withdrawn_at_unix_ms: u64,
    ) -> Result<Self, ResearchContributionError> {
        let withdrawal_event_ref = research_reference(withdrawal_event_ref)?;
        if withdrawn_at_unix_ms <= self.started_at_unix_ms {
            return Err(ResearchContributionError::InvalidWithdrawalTime);
        }

        if self.state == ResearchContributionState::Withdrawn {
            if self.withdrawal_event_ref.as_deref() == Some(withdrawal_event_ref)
                && self.withdrawn_at_unix_ms == Some(withdrawn_at_unix_ms)
            {
                return Ok(self.clone());
            }
            return Err(ResearchContributionError::AlreadyWithdrawn);
        }

        let mut withdrawn = self.clone();
        withdrawn.state = ResearchContributionState::Withdrawn;
        withdrawn.withdrawal_event_ref = Some(withdrawal_event_ref.to_owned());
        withdrawn.withdrawn_at_unix_ms = Some(withdrawn_at_unix_ms);
        Ok(withdrawn)
    }
}

/// Fail-closed research-contribution lifecycle error.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ResearchContributionError {
    /// A required contribution or research-participant reference is blank, noncanonical, or numeric-like.
    EmptyReference,
    /// The supplied consent snapshot has no active explicit research grant.
    ResearchConsentRequired,
    /// Research identity reused the operational participant reference.
    OperationalIdentityReuse,
    /// Contribution start time is zero or predates the authorizing consent.
    InvalidStartTime,
    /// Withdrawal time is not later than the contribution start.
    InvalidWithdrawalTime,
    /// A withdrawn contribution received a different second withdrawal.
    AlreadyWithdrawn,
}

impl Display for ResearchContributionError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::EmptyReference => "research contribution references must be exact opaque non-numeric values without surrounding whitespace or unsafe control characters",
            Self::ResearchConsentRequired => {
                "research contribution requires active explicit research consent"
            }
            Self::OperationalIdentityReuse => {
                "research participant reference must differ from the operational participant"
            }
            Self::InvalidStartTime => "research contribution start time must be greater than zero",
            Self::InvalidWithdrawalTime => {
                "research withdrawal time must be later than contribution start"
            }
            Self::AlreadyWithdrawn => {
                "research contribution has already been withdrawn with different evidence"
            }
        })
    }
}

impl Error for ResearchContributionError {}

fn required_reference(reference: &str) -> Result<&str, ConsentWriteError> {
    canonical_opaque_reference(reference).ok_or(ConsentWriteError::EmptyReference)
}

fn research_reference(reference: &str) -> Result<&str, ResearchContributionError> {
    canonical_opaque_reference(reference).ok_or(ResearchContributionError::EmptyReference)
}
