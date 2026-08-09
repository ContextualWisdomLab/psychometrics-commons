//! Cross-service integration event, outbox, and inbox semantics.
//!
//! This module defines executable domain behavior for durable integration. Physical
//! persistence adapters must commit local domain changes and outbox rows atomically,
//! then preserve these delivery-attempt and inbox-deduplication invariants. No raw
//! assessment payload is required here: routine integration identity is expressed as
//! opaque references and canonical payload digests.

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
    /// An event type was empty or exceeded the supported contract bound.
    InvalidEventType,
    /// A schema version was empty or exceeded the supported contract bound.
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
}

impl Display for IntegrationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidReference => "integration references must be opaque non-numeric values",
            Self::InvalidEventType => "integration event type must be non-empty and bounded",
            Self::InvalidSchemaVersion => {
                "integration schema version must be non-empty and bounded"
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
    subject_ref: String,
    occurred_at_unix_ms: u64,
    correlation_ref: String,
    causation_ref: Option<String>,
    payload_digest: String,
}

impl IntegrationEvent {
    /// Construct one immutable event envelope.
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
    /// The consumer had not previously accepted this source event.
    Accepted,
    /// The exact immutable event evidence was already accepted by this consumer.
    Duplicate,
}

/// Immutable consumer receipt used for inbox deduplication.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InboxReceipt {
    consumer_ref: String,
    source_ref: String,
    source_event_ref: String,
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

    /// Return the upstream source event reference.
    #[must_use]
    pub fn source_event_ref(&self) -> &str {
        &self.source_event_ref
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

/// In-memory domain ledger specifying consumer inbox deduplication behavior.
///
/// Persistence adapters may use a unique database constraint rather than this data
/// structure, but must preserve the same
/// `(consumer_ref, source_ref, source_event_ref)` identity and digest-conflict
/// semantics. Event references are not assumed to be globally unique across all
/// upstream bounded contexts.
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

    /// Return the number of unique consumer/source/event receipts.
    #[must_use]
    pub fn len(&self) -> usize {
        self.receipts.len()
    }

    /// Return accepted consumer receipts.
    #[must_use]
    pub fn receipts(&self) -> &[InboxReceipt] {
        &self.receipts
    }

    /// Accept or deduplicate one immutable source event for a consumer.
    ///
    /// The deduplication key is `(consumer_ref, source_ref, source_event_ref)`. An
    /// exact digest replay is a duplicate even when transport redelivers it later;
    /// a different digest under the same source-scoped identity fails closed rather
    /// than applying last write. The same event reference from a different upstream
    /// source is independent and cannot suppress legitimate delivery.
    ///
    /// # Errors
    ///
    /// Returns [`IntegrationError`] for invalid consumer/source/event identity,
    /// digest, timestamp, or a conflicting replay.
    pub fn accept(
        &mut self,
        consumer_ref: &str,
        source_ref: &str,
        source_event_ref: &str,
        payload_digest: &str,
        received_at_unix_ms: u64,
    ) -> Result<InboxDisposition, IntegrationError> {
        let consumer_ref = required_reference(consumer_ref)?;
        let source_ref = required_reference(source_ref)?;
        let source_event_ref = required_reference(source_event_ref)?;
        if received_at_unix_ms == 0 {
            return Err(IntegrationError::InvalidTimestamp);
        }
        if !valid_sha256_digest(payload_digest) {
            return Err(IntegrationError::InvalidDigest);
        }

        if let Some(existing) = self.receipts.iter().find(|receipt| {
            receipt.consumer_ref == consumer_ref
                && receipt.source_ref == source_ref
                && receipt.source_event_ref == source_event_ref
        }) {
            return if existing.payload_digest == payload_digest {
                Ok(InboxDisposition::Duplicate)
            } else {
                Err(IntegrationError::ConflictingReplay)
            };
        }

        self.receipts.push(InboxReceipt {
            consumer_ref: consumer_ref.to_owned(),
            source_ref: source_ref.to_owned(),
            source_event_ref: source_event_ref.to_owned(),
            payload_digest: payload_digest.to_owned(),
            received_at_unix_ms,
        });
        Ok(InboxDisposition::Accepted)
    }
}

fn bounded_label(
    value: &str,
    max_length: usize,
    error: IntegrationError,
) -> Result<&str, IntegrationError> {
    let normalized = value.trim();
    if normalized.is_empty() || normalized.len() > max_length {
        Err(error)
    } else {
        Ok(normalized)
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
