//! Cross-service integration event, outbox, and inbox semantics.
//!
//! This module defines executable domain behavior for durable integration. Physical
//! persistence adapters must commit local domain changes and outbox rows atomically,
//! then preserve these delivery-attempt and inbox-deduplication invariants. No raw
//! assessment payload is required here: routine integration identity is expressed as
//! opaque tenant/resource references and canonical payload digests.

use crate::reference::normalized_reference;
use std::error::Error;
use std::fmt::{Display, Formatter};

const MAX_EVENT_TYPE_LENGTH: usize = 128;
const MAX_SCHEMA_VERSION_LENGTH: usize = 64;

/// Fail-closed error for cross-service integration evidence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum IntegrationError {
    /// A required opaque reference was blank or numeric-only.
    InvalidReference,
    /// An event type was empty, non-canonical, or exceeded the supported contract bound.
    InvalidEventType,
    /// A schema version was empty, non-canonical, or exceeded the supported contract bound.
    InvalidSchemaVersion,
    /// A payload digest was not canonical SHA-256 evidence.
    InvalidDigest,
    /// A server-authoritative timestamp was zero.
    InvalidTimestamp,
    /// A delivery attempt occurred before prior accepted evidence.
    NonMonotonicTimestamp,
    /// An outbox entry was configured with no permitted delivery attempt.
    InvalidAttemptLimit,
    /// An idempotency identity was reused with conflicting immutable evidence.
    ConflictingReplay,
    /// A new delivery attempt was offered after terminal delivery/quarantine.
    TerminalOutboxState,
    /// A new consumption transition was offered after completion or quarantine.
    TerminalConsumptionState,
    /// A worker tried to claim a consumption that is not pending.
    ConsumptionNotClaimable,
    /// A completion or quarantine used a fencing token that is no longer current.
    StaleConsumptionFence,
    /// A processing claim expiry is not later than its claim time.
    InvalidConsumptionClaimWindow,
    /// Claim expiry was requested before the active processing claim expired.
    ConsumptionClaimStillActive,
    /// Claim expiry was requested for a consumption that is not processing.
    ConsumptionNotProcessing,
}

impl Display for IntegrationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidReference => "integration references must be opaque non-numeric values",
            Self::InvalidEventType => {
                "integration event type must be non-empty, bounded, and canonical"
            }
            Self::InvalidSchemaVersion => {
                "integration schema version must be non-empty, bounded, and canonical"
            }
            Self::InvalidDigest => "integration payload digest must be a canonical sha256 digest",
            Self::InvalidTimestamp => "integration timestamps must be greater than zero",
            Self::NonMonotonicTimestamp => "integration event time must not move backwards",
            Self::InvalidAttemptLimit => "outbox maximum attempts must be greater than zero",
            Self::ConflictingReplay => {
                "integration idempotency identity was replayed with conflicting evidence"
            }
            Self::TerminalOutboxState => {
                "terminal outbox entry cannot accept a new delivery attempt"
            }
            Self::TerminalConsumptionState => {
                "terminal inbox consumption cannot accept a new processing transition"
            }
            Self::ConsumptionNotClaimable => {
                "inbox consumption can be claimed only from the pending state"
            }
            Self::StaleConsumptionFence => {
                "inbox consumption fencing token does not match the current claim"
            }
            Self::InvalidConsumptionClaimWindow => {
                "inbox consumption claim expiry must be later than claim time"
            }
            Self::ConsumptionClaimStillActive => {
                "inbox consumption processing claim has not expired"
            }
            Self::ConsumptionNotProcessing => {
                "inbox consumption claim expiry requires the processing state"
            }
        })
    }
}

impl Error for IntegrationError {}

/// Immutable versioned event envelope emitted by Psychometrics Commons.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IntegrationEvent {
    event_ref: String,
    event_type: String,
    schema_version: String,
    source: String,
    tenant_ref: String,
    subject_ref: String,
    occurred_at_unix_ms: u64,
    correlation_ref: String,
    causation_ref: Option<String>,
    payload_digest: String,
}

impl IntegrationEvent {
    /// Construct one immutable tenant- and subject-bound event envelope.
    ///
    /// The tenant reference is part of immutable event evidence rather than
    /// transport metadata so consumers cannot detach a source event from the
    /// tenant/resource authority under which it was emitted.
    ///
    /// # Errors
    ///
    /// Returns [`IntegrationError`] when required references, event type, schema
    /// version, timestamp, optional causation reference, or digest is invalid.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        event_ref: &str,
        event_type: &str,
        schema_version: &str,
        source: &str,
        tenant_ref: &str,
        subject_ref: &str,
        occurred_at_unix_ms: u64,
        correlation_ref: &str,
        causation_ref: Option<&str>,
        payload_digest: &str,
    ) -> Result<Self, IntegrationError> {
        if occurred_at_unix_ms == 0 {
            return Err(IntegrationError::InvalidTimestamp);
        }
        let event_type = bounded_label(
            event_type,
            MAX_EVENT_TYPE_LENGTH,
            IntegrationError::InvalidEventType,
        )?;
        let schema_version = bounded_label(
            schema_version,
            MAX_SCHEMA_VERSION_LENGTH,
            IntegrationError::InvalidSchemaVersion,
        )?;
        if !valid_sha256_digest(payload_digest) {
            return Err(IntegrationError::InvalidDigest);
        }
        let causation_ref = causation_ref
            .map(required_reference)
            .transpose()?
            .map(str::to_owned);

        Ok(Self {
            event_ref: required_reference(event_ref)?.to_owned(),
            event_type: event_type.to_owned(),
            schema_version: schema_version.to_owned(),
            source: required_reference(source)?.to_owned(),
            tenant_ref: required_reference(tenant_ref)?.to_owned(),
            subject_ref: required_reference(subject_ref)?.to_owned(),
            occurred_at_unix_ms,
            correlation_ref: required_reference(correlation_ref)?.to_owned(),
            causation_ref,
            payload_digest: payload_digest.to_owned(),
        })
    }

    /// Return the opaque source event reference.
    #[must_use]
    pub fn event_ref(&self) -> &str {
        &self.event_ref
    }

    /// Return the versioned domain event type.
    #[must_use]
    pub fn event_type(&self) -> &str {
        &self.event_type
    }

    /// Return the event payload schema version.
    #[must_use]
    pub fn schema_version(&self) -> &str {
        &self.schema_version
    }

    /// Return the emitting bounded-context source identifier.
    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Return the tenant whose product resource emitted this event.
    #[must_use]
    pub fn tenant_ref(&self) -> &str {
        &self.tenant_ref
    }

    /// Return the opaque event subject reference.
    #[must_use]
    pub fn subject_ref(&self) -> &str {
        &self.subject_ref
    }

    /// Return the server-authoritative occurrence time.
    #[must_use]
    pub const fn occurred_at_unix_ms(&self) -> u64 {
        self.occurred_at_unix_ms
    }

    /// Return the request/workflow correlation reference.
    #[must_use]
    pub fn correlation_ref(&self) -> &str {
        &self.correlation_ref
    }

    /// Return the optional causation reference.
    #[must_use]
    pub fn causation_ref(&self) -> Option<&str> {
        self.causation_ref.as_deref()
    }

    /// Return the canonical payload digest.
    #[must_use]
    pub fn payload_digest(&self) -> &str {
        &self.payload_digest
    }

    /// Return a copy of this envelope with a different already-validated event identity.
    ///
    /// Callers that derived `event_ref` from the same opaque-reference rules as
    /// [`IntegrationEvent::new`] can replace only the identity without re-validating
    /// tenant, schema, or digest evidence.
    #[must_use]
    pub(crate) fn with_event_ref(&self, event_ref: String) -> Self {
        Self {
            event_ref,
            event_type: self.event_type.clone(),
            schema_version: self.schema_version.clone(),
            source: self.source.clone(),
            tenant_ref: self.tenant_ref.clone(),
            subject_ref: self.subject_ref.clone(),
            occurred_at_unix_ms: self.occurred_at_unix_ms,
            correlation_ref: self.correlation_ref.clone(),
            causation_ref: self.causation_ref.clone(),
            payload_digest: self.payload_digest.clone(),
        }
    }
}

/// Delivery outcome recorded for one outbox attempt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum DeliveryOutcome {
    /// The consumer transport accepted the event.
    Delivered,
    /// A bounded retry may succeed without changing immutable event evidence.
    RetryableFailure,
    /// Retrying the same event cannot succeed without operator/code intervention.
    PermanentFailure,
}

/// Current delivery state of one outbox entry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum OutboxState {
    /// Delivery is pending or retryable within the bounded attempt budget.
    Pending,
    /// Delivery succeeded and no new attempts are legal.
    Delivered,
    /// Delivery is quarantined and requires reconciliation/operator action.
    Quarantined,
}

impl OutboxState {
    const fn is_terminal(self) -> bool {
        matches!(self, Self::Delivered | Self::Quarantined)
    }
}

/// Immutable evidence for one outbox delivery attempt.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeliveryAttempt {
    attempt_ref: String,
    outcome: DeliveryOutcome,
    occurred_at_unix_ms: u64,
    cause_code: Option<String>,
}

impl DeliveryAttempt {
    /// Return the opaque attempt idempotency reference.
    #[must_use]
    pub fn attempt_ref(&self) -> &str {
        &self.attempt_ref
    }

    /// Return the recorded delivery outcome.
    #[must_use]
    pub const fn outcome(&self) -> DeliveryOutcome {
        self.outcome
    }

    /// Return the server-authoritative attempt time.
    #[must_use]
    pub const fn occurred_at_unix_ms(&self) -> u64 {
        self.occurred_at_unix_ms
    }

    /// Return an optional safe machine failure-cause code.
    #[must_use]
    pub fn cause_code(&self) -> Option<&str> {
        self.cause_code.as_deref()
    }
}

/// Product-owned outbox entry with bounded retry and quarantine semantics.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OutboxEntry {
    event: IntegrationEvent,
    state: OutboxState,
    max_attempts: usize,
    attempts: Vec<DeliveryAttempt>,
    latest_event_at_unix_ms: u64,
}

impl OutboxEntry {
    /// Create a pending outbox entry for an immutable event.
    ///
    /// # Errors
    ///
    /// Returns [`IntegrationError::InvalidAttemptLimit`] when `max_attempts` is zero.
    pub fn new(event: IntegrationEvent, max_attempts: usize) -> Result<Self, IntegrationError> {
        if max_attempts == 0 {
            return Err(IntegrationError::InvalidAttemptLimit);
        }
        let occurred_at_unix_ms = event.occurred_at_unix_ms();
        Ok(Self {
            event,
            state: OutboxState::Pending,
            max_attempts,
            attempts: Vec::new(),
            latest_event_at_unix_ms: occurred_at_unix_ms,
        })
    }

    /// Return the immutable event being delivered.
    #[must_use]
    pub const fn event(&self) -> &IntegrationEvent {
        &self.event
    }

    /// Return the current delivery state.
    #[must_use]
    pub const fn state(&self) -> OutboxState {
        self.state
    }

    /// Return the configured maximum number of delivery attempts.
    #[must_use]
    pub const fn max_attempts(&self) -> usize {
        self.max_attempts
    }

    /// Return the number of distinct accepted attempts.
    #[must_use]
    pub fn attempt_count(&self) -> usize {
        self.attempts.len()
    }

    /// Return accepted delivery attempts in server-authoritative order.
    #[must_use]
    pub fn attempts(&self) -> &[DeliveryAttempt] {
        &self.attempts
    }

    /// Record one delivery attempt using `attempt_ref` as the idempotency key.
    ///
    /// Exact replay remains idempotent even after terminal delivery or quarantine.
    /// A permanent failure quarantines immediately. Retryable failures quarantine
    /// when the configured attempt budget is exhausted.
    ///
    /// # Errors
    ///
    /// Returns [`IntegrationError`] for invalid attempt evidence, conflicting replay,
    /// backward time, or a new attempt after a terminal state.
    pub fn record_attempt(
        &mut self,
        attempt_ref: &str,
        outcome: DeliveryOutcome,
        occurred_at_unix_ms: u64,
        cause_code: Option<&str>,
    ) -> Result<OutboxState, IntegrationError> {
        let attempt_ref = required_reference(attempt_ref)?;
        if occurred_at_unix_ms == 0 {
            return Err(IntegrationError::InvalidTimestamp);
        }
        let cause_code = cause_code
            .map(required_reference)
            .transpose()?
            .map(str::to_owned);

        if let Some(existing) = self
            .attempts
            .iter()
            .find(|attempt| attempt.attempt_ref == attempt_ref)
        {
            return if existing.outcome == outcome
                && existing.occurred_at_unix_ms == occurred_at_unix_ms
                && existing.cause_code == cause_code
            {
                Ok(self.state)
            } else {
                Err(IntegrationError::ConflictingReplay)
            };
        }
        if self.state.is_terminal() {
            return Err(IntegrationError::TerminalOutboxState);
        }
        if occurred_at_unix_ms < self.latest_event_at_unix_ms {
            return Err(IntegrationError::NonMonotonicTimestamp);
        }

        self.attempts.push(DeliveryAttempt {
            attempt_ref: attempt_ref.to_owned(),
            outcome,
            occurred_at_unix_ms,
            cause_code,
        });
        self.latest_event_at_unix_ms = occurred_at_unix_ms;
        self.state = match outcome {
            DeliveryOutcome::Delivered => OutboxState::Delivered,
            DeliveryOutcome::PermanentFailure => OutboxState::Quarantined,
            DeliveryOutcome::RetryableFailure if self.attempts.len() >= self.max_attempts => {
                OutboxState::Quarantined
            }
            DeliveryOutcome::RetryableFailure => OutboxState::Pending,
        };
        Ok(self.state)
    }
}

/// Result of applying inbox deduplication to one source event.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum InboxDisposition {
    /// The consumer had not previously accepted this tenant-bound source event.
    Accepted,
    /// The exact immutable event evidence was already accepted by this consumer.
    Duplicate,
}

/// Immutable consumer receipt used for tenant-bound inbox deduplication.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InboxReceipt {
    consumer_ref: String,
    source_ref: String,
    tenant_ref: String,
    source_event_ref: String,
    event_type: String,
    schema_version: String,
    subject_ref: String,
    payload_digest: String,
    received_at_unix_ms: u64,
}

impl InboxReceipt {
    /// Return the consumer identity used for deduplication scope.
    #[must_use]
    pub fn consumer_ref(&self) -> &str {
        &self.consumer_ref
    }

    /// Return the upstream bounded-context/source identity.
    #[must_use]
    pub fn source_ref(&self) -> &str {
        &self.source_ref
    }

    /// Return the tenant bound to the accepted source event.
    #[must_use]
    pub fn tenant_ref(&self) -> &str {
        &self.tenant_ref
    }

    /// Return the upstream source event reference.
    #[must_use]
    pub fn source_event_ref(&self) -> &str {
        &self.source_event_ref
    }

    /// Return the accepted event type.
    #[must_use]
    pub fn event_type(&self) -> &str {
        &self.event_type
    }

    /// Return the accepted event schema version.
    #[must_use]
    pub fn schema_version(&self) -> &str {
        &self.schema_version
    }

    /// Return the tenant-scoped resource subject of the accepted event.
    #[must_use]
    pub fn subject_ref(&self) -> &str {
        &self.subject_ref
    }

    /// Return the immutable payload digest accepted for this event.
    #[must_use]
    pub fn payload_digest(&self) -> &str {
        &self.payload_digest
    }

    /// Return the first server-authoritative receive time.
    #[must_use]
    pub const fn received_at_unix_ms(&self) -> u64 {
        self.received_at_unix_ms
    }
}

/// In-memory domain ledger specifying tenant-bound consumer inbox behavior.
///
/// Persistence adapters may use a unique database constraint rather than this data
/// structure, but must preserve the same
/// `(consumer_ref, source_ref, tenant_ref, source_event_ref)` identity. For an
/// existing identity, event type, schema version, subject and payload digest are
/// immutable evidence and any difference fails closed. Event references are not
/// assumed to be globally unique across upstream sources or tenants.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct IntegrationInbox {
    receipts: Vec<InboxReceipt>,
}

impl IntegrationInbox {
    /// Create an empty consumer inbox ledger.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            receipts: Vec::new(),
        }
    }

    /// Return whether the inbox has accepted no source event.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.receipts.is_empty()
    }

    /// Return the number of unique consumer/source/tenant/event receipts.
    #[must_use]
    pub fn len(&self) -> usize {
        self.receipts.len()
    }

    /// Return accepted consumer receipts.
    #[must_use]
    pub fn receipts(&self) -> &[InboxReceipt] {
        &self.receipts
    }

    /// Accept or deduplicate one immutable tenant-bound source event.
    ///
    /// Accepting the complete [`IntegrationEvent`] prevents transport adapters from
    /// supplying a tenant independently from the immutable source envelope. The
    /// deduplication key is `(consumer_ref, source, tenant_ref, event_ref)`. An exact
    /// replay is a duplicate even when transport redelivers it later. Reuse of that
    /// key with a different event type, schema version, subject or payload digest
    /// fails closed rather than applying last write.
    ///
    /// # Errors
    ///
    /// Returns [`IntegrationError`] for an invalid consumer identity, zero receive
    /// timestamp, or conflicting replay evidence.
    pub fn accept_event(
        &mut self,
        consumer_ref: &str,
        event: &IntegrationEvent,
        received_at_unix_ms: u64,
    ) -> Result<InboxDisposition, IntegrationError> {
        let consumer_ref = required_reference(consumer_ref)?;
        if received_at_unix_ms == 0 {
            return Err(IntegrationError::InvalidTimestamp);
        }

        if let Some(existing) = self.receipts.iter().find(|receipt| {
            receipt.consumer_ref == consumer_ref
                && receipt.source_ref == event.source()
                && receipt.tenant_ref == event.tenant_ref()
                && receipt.source_event_ref == event.event_ref()
        }) {
            return if existing.event_type == event.event_type()
                && existing.schema_version == event.schema_version()
                && existing.subject_ref == event.subject_ref()
                && existing.payload_digest == event.payload_digest()
            {
                Ok(InboxDisposition::Duplicate)
            } else {
                Err(IntegrationError::ConflictingReplay)
            };
        }

        self.receipts.push(InboxReceipt {
            consumer_ref: consumer_ref.to_owned(),
            source_ref: event.source().to_owned(),
            tenant_ref: event.tenant_ref().to_owned(),
            source_event_ref: event.event_ref().to_owned(),
            event_type: event.event_type().to_owned(),
            schema_version: event.schema_version().to_owned(),
            subject_ref: event.subject_ref().to_owned(),
            payload_digest: event.payload_digest().to_owned(),
            received_at_unix_ms,
        });
        Ok(InboxDisposition::Accepted)
    }
}

/// Processing state for one inbox side-effect consumption record.
///
/// Inbox receipt is not side-effect completion. A consumption record starts
/// pending and becomes completed only after local or verified external
/// completion evidence exists.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ConsumptionState {
    /// The inbox receipt exists but the required side effect has not started.
    Pending,
    /// A worker holds a fenced claim while performing a recoverable side effect.
    Processing,
    /// The required side effect completed and verified completion evidence exists.
    Completed,
    /// Automatic processing stopped and operator reconciliation is required.
    Quarantined,
}

impl ConsumptionState {
    const fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Quarantined)
    }
}

/// Product-owned inbox consumption distinct from receipt deduplication.
///
/// One consumption names the durable side-effect work for one inbox receipt.
/// Local effects may move directly from pending to completed. Non-local
/// effects claim a fencing token in the processing state and complete only
/// after verified evidence exists.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InboxConsumption {
    consumer_ref: String,
    source_ref: String,
    tenant_ref: String,
    source_event_ref: String,
    consumption_ref: String,
    side_effect_ref: String,
    state: ConsumptionState,
    fencing_token: u64,
    latest_event_at_unix_ms: u64,
    claim_expires_at_unix_ms: u64,
    completion_evidence_ref: Option<String>,
    cause_code: Option<String>,
}

impl InboxConsumption {
    /// Create a pending consumption for one accepted inbox receipt.
    ///
    /// # Errors
    ///
    /// Returns [`IntegrationError`] when any identity is invalid or the recorded
    /// timestamp is zero.
    pub fn pending(
        consumer_ref: &str,
        source_ref: &str,
        tenant_ref: &str,
        source_event_ref: &str,
        consumption_ref: &str,
        side_effect_ref: &str,
        recorded_at_unix_ms: u64,
    ) -> Result<Self, IntegrationError> {
        if recorded_at_unix_ms == 0 {
            return Err(IntegrationError::InvalidTimestamp);
        }
        Ok(Self {
            consumer_ref: required_reference(consumer_ref)?.to_owned(),
            source_ref: required_reference(source_ref)?.to_owned(),
            tenant_ref: required_reference(tenant_ref)?.to_owned(),
            source_event_ref: required_reference(source_event_ref)?.to_owned(),
            consumption_ref: required_reference(consumption_ref)?.to_owned(),
            side_effect_ref: required_reference(side_effect_ref)?.to_owned(),
            state: ConsumptionState::Pending,
            fencing_token: 0,
            latest_event_at_unix_ms: recorded_at_unix_ms,
            claim_expires_at_unix_ms: 0,
            completion_evidence_ref: None,
            cause_code: None,
        })
    }

    /// Return the consumer identity that owns this consumption.
    #[must_use]
    pub fn consumer_ref(&self) -> &str {
        &self.consumer_ref
    }

    /// Return the upstream source identity.
    #[must_use]
    pub fn source_ref(&self) -> &str {
        &self.source_ref
    }

    /// Return the tenant bound to the inbox receipt.
    #[must_use]
    pub fn tenant_ref(&self) -> &str {
        &self.tenant_ref
    }

    /// Return the upstream source event identity.
    #[must_use]
    pub fn source_event_ref(&self) -> &str {
        &self.source_event_ref
    }

    /// Return the opaque consumption work identity.
    #[must_use]
    pub fn consumption_ref(&self) -> &str {
        &self.consumption_ref
    }

    /// Return the durable side-effect or external idempotency identity.
    #[must_use]
    pub fn side_effect_ref(&self) -> &str {
        &self.side_effect_ref
    }

    /// Return the current consumption processing state.
    #[must_use]
    pub const fn state(&self) -> ConsumptionState {
        self.state
    }

    /// Return the current stale-worker fencing token.
    #[must_use]
    pub const fn fencing_token(&self) -> u64 {
        self.fencing_token
    }

    /// Return the latest accepted consumption-event time.
    #[must_use]
    pub const fn latest_event_at_unix_ms(&self) -> u64 {
        self.latest_event_at_unix_ms
    }

    /// Return the current processing-claim expiry, when a worker holds a fence.
    #[must_use]
    pub const fn claim_expires_at_unix_ms(&self) -> Option<u64> {
        if self.claim_expires_at_unix_ms == 0 {
            None
        } else {
            Some(self.claim_expires_at_unix_ms)
        }
    }

    /// Return verified completion evidence after a successful side effect.
    #[must_use]
    pub fn completion_evidence_ref(&self) -> Option<&str> {
        self.completion_evidence_ref.as_deref()
    }

    /// Return the optional machine cause recorded for quarantine.
    #[must_use]
    pub fn cause_code(&self) -> Option<&str> {
        self.cause_code.as_deref()
    }

    /// Claim a pending consumption for recoverable non-local processing.
    ///
    /// Returns the new fencing token. A later completion or quarantine must
    /// present that token. Claiming never inherits a previous worker's fence.
    /// The claim expires at `expires_at_unix_ms`; expiry recovery returns the
    /// row to pending without transferring that fence.
    ///
    /// # Errors
    ///
    /// Returns [`IntegrationError`] for a zero timestamp, an empty claim
    /// window, backward time, a non-pending state, or a terminal consumption.
    pub fn begin_processing(
        &mut self,
        observed_at_unix_ms: u64,
        expires_at_unix_ms: u64,
    ) -> Result<u64, IntegrationError> {
        if observed_at_unix_ms == 0 || expires_at_unix_ms == 0 {
            return Err(IntegrationError::InvalidTimestamp);
        }
        if expires_at_unix_ms <= observed_at_unix_ms {
            return Err(IntegrationError::InvalidConsumptionClaimWindow);
        }
        if self.state.is_terminal() {
            return Err(IntegrationError::TerminalConsumptionState);
        }
        if self.state != ConsumptionState::Pending {
            return Err(IntegrationError::ConsumptionNotClaimable);
        }
        if observed_at_unix_ms < self.latest_event_at_unix_ms {
            return Err(IntegrationError::NonMonotonicTimestamp);
        }
        self.fencing_token += 1;
        self.state = ConsumptionState::Processing;
        self.latest_event_at_unix_ms = observed_at_unix_ms;
        self.claim_expires_at_unix_ms = expires_at_unix_ms;
        Ok(self.fencing_token)
    }

    /// Recover an expired processing claim without transferring its fence.
    ///
    /// The row returns to pending and keeps the last issued fencing token so
    /// the next claim increments it. The expired worker cannot complete or
    /// quarantine with that token; a later local completion uses fence `0`.
    ///
    /// # Errors
    ///
    /// Returns [`IntegrationError`] for a zero timestamp, a consumption that
    /// is not processing, or a claim that has not yet expired.
    pub fn expire_processing(
        &mut self,
        observed_at_unix_ms: u64,
    ) -> Result<ConsumptionState, IntegrationError> {
        if observed_at_unix_ms == 0 {
            return Err(IntegrationError::InvalidTimestamp);
        }
        if self.state != ConsumptionState::Processing {
            return Err(IntegrationError::ConsumptionNotProcessing);
        }
        if observed_at_unix_ms < self.claim_expires_at_unix_ms {
            return Err(IntegrationError::ConsumptionClaimStillActive);
        }
        self.state = ConsumptionState::Pending;
        self.claim_expires_at_unix_ms = 0;
        self.latest_event_at_unix_ms = observed_at_unix_ms;
        Ok(ConsumptionState::Pending)
    }

    /// Mark the required side effect complete with verified evidence.
    ///
    /// A local effect may complete directly from pending with fencing token `0`.
    /// A claimed worker must present the current fence. The authorizing fence is
    /// recorded so exact replay of the same evidence, time, and fence remains
    /// idempotent after an expired claim's leftover token.
    ///
    /// # Errors
    ///
    /// Returns [`IntegrationError`] for invalid evidence, backward time, a stale
    /// fence, conflicting completed evidence, or a quarantined consumption.
    pub fn complete(
        &mut self,
        observed_at_unix_ms: u64,
        completion_evidence_ref: &str,
        expected_fence: u64,
    ) -> Result<ConsumptionState, IntegrationError> {
        let completion_evidence_ref = required_reference(completion_evidence_ref)?;
        if observed_at_unix_ms == 0 {
            return Err(IntegrationError::InvalidTimestamp);
        }
        if self.state == ConsumptionState::Completed {
            return if self.completion_evidence_ref.as_deref() == Some(completion_evidence_ref)
                && self.latest_event_at_unix_ms == observed_at_unix_ms
                && self.fencing_token == expected_fence
            {
                Ok(ConsumptionState::Completed)
            } else {
                Err(IntegrationError::ConflictingReplay)
            };
        }
        if self.state == ConsumptionState::Quarantined {
            return Err(IntegrationError::TerminalConsumptionState);
        }
        if !self.fence_authorizes(expected_fence) {
            return Err(IntegrationError::StaleConsumptionFence);
        }
        if observed_at_unix_ms < self.latest_event_at_unix_ms {
            return Err(IntegrationError::NonMonotonicTimestamp);
        }
        self.state = ConsumptionState::Completed;
        self.fencing_token = expected_fence;
        self.claim_expires_at_unix_ms = 0;
        self.latest_event_at_unix_ms = observed_at_unix_ms;
        self.completion_evidence_ref = Some(completion_evidence_ref.to_owned());
        Ok(ConsumptionState::Completed)
    }

    /// Quarantine a pending or processing consumption for operator action.
    ///
    /// The authorizing fence is recorded so exact replay of the same cause,
    /// time, and fence remains idempotent after an expired claim's leftover
    /// token. Completion evidence is never invented by quarantine.
    ///
    /// # Errors
    ///
    /// Returns [`IntegrationError`] for invalid cause evidence, backward time, a
    /// stale fence, conflicting quarantine evidence, or a completed consumption.
    pub fn quarantine(
        &mut self,
        observed_at_unix_ms: u64,
        cause_code: &str,
        expected_fence: u64,
    ) -> Result<ConsumptionState, IntegrationError> {
        let cause_code = required_reference(cause_code)?;
        if observed_at_unix_ms == 0 {
            return Err(IntegrationError::InvalidTimestamp);
        }
        if self.state == ConsumptionState::Quarantined {
            return if self.cause_code.as_deref() == Some(cause_code)
                && self.latest_event_at_unix_ms == observed_at_unix_ms
                && self.fencing_token == expected_fence
            {
                Ok(ConsumptionState::Quarantined)
            } else {
                Err(IntegrationError::ConflictingReplay)
            };
        }
        if self.state == ConsumptionState::Completed {
            return Err(IntegrationError::TerminalConsumptionState);
        }
        if !self.fence_authorizes(expected_fence) {
            return Err(IntegrationError::StaleConsumptionFence);
        }
        if observed_at_unix_ms < self.latest_event_at_unix_ms {
            return Err(IntegrationError::NonMonotonicTimestamp);
        }
        self.state = ConsumptionState::Quarantined;
        self.fencing_token = expected_fence;
        self.claim_expires_at_unix_ms = 0;
        self.latest_event_at_unix_ms = observed_at_unix_ms;
        self.cause_code = Some(cause_code.to_owned());
        Ok(ConsumptionState::Quarantined)
    }

    fn fence_authorizes(&self, expected_fence: u64) -> bool {
        if self.state == ConsumptionState::Pending {
            expected_fence == 0
        } else {
            expected_fence == self.fencing_token
        }
    }
}

fn bounded_label(
    value: &str,
    max_length: usize,
    error: IntegrationError,
) -> Result<&str, IntegrationError> {
    if value.is_empty()
        || value.len() > max_length
        || value
            .chars()
            .any(|character| character.is_whitespace() || character.is_control())
    {
        Err(error)
    } else {
        Ok(value)
    }
}

fn required_reference(reference: &str) -> Result<&str, IntegrationError> {
    normalized_reference(reference).ok_or(IntegrationError::InvalidReference)
}

fn valid_sha256_digest(digest: &str) -> bool {
    let Some(hex) = digest.strip_prefix("sha256:") else {
        return false;
    };
    hex.len() == 64
        && hex
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
