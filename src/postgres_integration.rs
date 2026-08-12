//! `PostgreSQL` 18 persistence adapter for integration evidence.
//!
//! The adapter receives a caller-owned `PostgreSQL` client or transaction. It never
//! owns credentials or a cross-service connection. The migration and write paths
//! preserve the immutable integration-event, outbox, and inbox contracts defined
//! by [`crate::integration`].

use crate::integration::{DeliveryOutcome, InboxDisposition, IntegrationEvent, OutboxState};
use crate::reference::normalized_reference;
use postgres::{GenericClient, Transaction};
use std::error::Error;
use std::fmt::{Display, Formatter};

const INTEGRATION_MIGRATION: &str = include_str!("../migrations/0001_integration_delivery.sql");
const OUTBOX_REPLAY_QUERY: &str = "SELECT EXISTS (\
     SELECT 1 FROM integration_outbox \
     WHERE event_ref = $1 AND event_type = $2 AND schema_version = $3 \
       AND source_ref = $4 AND tenant_ref = $5 AND subject_ref = $6 \
       AND occurred_at_unix_ms = $7 AND correlation_ref = $8 \
       AND causation_ref IS NOT DISTINCT FROM $9 \
       AND payload_digest = $10 AND max_attempts = $11\
 )";
const INBOX_REPLAY_QUERY: &str = "SELECT EXISTS (\
     SELECT 1 FROM integration_inbox \
     WHERE consumer_ref = $1 AND source_ref = $2 AND tenant_ref = $3 \
       AND source_event_ref = $4 AND event_type = $5 AND schema_version = $6 \
       AND subject_ref = $7 AND payload_digest = $8\
 )";

/// Outcome of inserting immutable persistence evidence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum PersistenceDisposition {
    /// The immutable evidence was inserted for the first time.
    Inserted,
    /// Identical immutable evidence already existed.
    Duplicate,
}

/// Durable result of recording one outbox delivery attempt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DeliveryAttemptPersistence {
    disposition: PersistenceDisposition,
    outbox_state: OutboxState,
}

impl DeliveryAttemptPersistence {
    /// Return whether this call inserted new evidence or replayed exact evidence.
    #[must_use]
    pub const fn disposition(self) -> PersistenceDisposition {
        self.disposition
    }

    /// Return the durable outbox state after applying or replaying the attempt.
    #[must_use]
    pub const fn outbox_state(self) -> OutboxState {
        self.outbox_state
    }
}

/// Composite durable identity for one persisted outbox event.
///
/// The three references form the database key used to lock, replay, and advance one
/// outbox entry. Validation remains fail-closed in [`record_outbox_delivery_attempt`]
/// so constructing this lightweight value never bypasses persistence checks.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OutboxPersistenceIdentity<'a> {
    source: &'a str,
    tenant: &'a str,
    event: &'a str,
}

impl<'a> OutboxPersistenceIdentity<'a> {
    /// Group the source, tenant, and event references that identify one durable outbox row.
    #[must_use]
    pub const fn new(source_ref: &'a str, tenant_ref: &'a str, event_ref: &'a str) -> Self {
        Self {
            source: source_ref,
            tenant: tenant_ref,
            event: event_ref,
        }
    }
}

/// Fail-closed persistence error for integration evidence.
#[derive(Debug)]
#[non_exhaustive]
pub enum PersistenceError {
    /// An adapter input reference was blank or numeric-only.
    InvalidReference,
    /// A server-authoritative timestamp was zero.
    InvalidTimestamp,
    /// The configured outbox delivery-attempt limit was zero.
    InvalidAttemptLimit,
    /// A runtime value cannot be represented by the bounded `PostgreSQL` column.
    ValueOutOfRange,
    /// The caller's transaction isolation cannot preserve this adapter's replay semantics.
    UnsupportedIsolationLevel,
    /// An idempotency identity already exists with different immutable evidence.
    ConflictingReplay,
    /// A delivery attempt references an outbox entry that does not exist.
    OutboxNotFound,
    /// A delivery attempt timestamp precedes the latest accepted outbox evidence.
    NonMonotonicTimestamp,
    /// A new delivery attempt was requested after the outbox became terminal.
    TerminalOutboxState,
    /// Stored outbox state does not match the migration-defined state vocabulary.
    InvalidStoredState,
    /// `PostgreSQL` rejected or could not execute the persistence operation.
    Database(postgres::Error),
}

impl Display for PersistenceError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidReference => "persistence references must be opaque non-numeric values",
            Self::InvalidTimestamp => "persistence timestamps must be greater than zero",
            Self::InvalidAttemptLimit => "outbox maximum attempts must be greater than zero",
            Self::ValueOutOfRange => "persistence value exceeds the supported PostgreSQL range",
            Self::UnsupportedIsolationLevel => {
                "PostgreSQL integration persistence requires read committed isolation"
            }
            Self::ConflictingReplay => {
                "persistence idempotency identity was replayed with conflicting evidence"
            }
            Self::OutboxNotFound => "delivery attempt references an unknown outbox entry",
            Self::NonMonotonicTimestamp => {
                "delivery attempt timestamp precedes the latest outbox evidence"
            }
            Self::TerminalOutboxState => "terminal outbox state rejects new delivery attempts",
            Self::InvalidStoredState => "stored outbox state violates the persistence contract",
            Self::Database(_) => "PostgreSQL persistence operation failed",
        })
    }
}

impl Error for PersistenceError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Database(error) => Some(error),
            _ => None,
        }
    }
}

impl From<postgres::Error> for PersistenceError {
    fn from(error: postgres::Error) -> Self {
        Self::Database(error)
    }
}

/// Apply the idempotent integration-evidence migration to a `PostgreSQL` connection.
///
/// The caller owns the connection, transaction policy, credentials, TLS policy, and
/// deployment routing. Re-applying this migration is safe because it creates only
/// the bounded first-slice integration tables when absent.
///
/// # Errors
///
/// Returns the `PostgreSQL` error if the migration cannot be applied.
pub fn apply_integration_migration(client: &mut impl GenericClient) -> Result<(), postgres::Error> {
    client.batch_execute(INTEGRATION_MIGRATION)
}

/// Insert an immutable integration event into the durable outbox.
///
/// The durable identity is `(source_ref, tenant_ref, event_ref)`, because an event
/// reference is not globally unique across bounded-context sources or tenants. A
/// repeated scoped identity is idempotent only when every immutable event field and
/// `max_attempts` match the existing row. Reuse with different evidence fails closed.
/// The function uses the caller-owned client/transaction, so a domain mutation and
/// this outbox insert can be committed atomically by the caller.
///
/// The current insert-then-inspect replay algorithm requires `PostgreSQL` `READ
/// COMMITTED` isolation. That isolation refreshes the statement snapshot after an
/// `ON CONFLICT DO NOTHING` wait so the exact conflicting row can be inspected.
/// Stronger transaction isolation is rejected rather than misclassifying a replay.
///
/// # Errors
///
/// Returns [`PersistenceError`] for an invalid attempt limit, a value outside the
/// supported `PostgreSQL` integer range, unsupported transaction isolation,
/// conflicting replay, or a database failure.
pub fn enqueue_outbox_event(
    client: &mut impl GenericClient,
    event: &IntegrationEvent,
    max_attempts: usize,
) -> Result<PersistenceDisposition, PersistenceError> {
    if max_attempts == 0 {
        return Err(PersistenceError::InvalidAttemptLimit);
    }
    let occurred_at_unix_ms = postgres_bigint(event.occurred_at_unix_ms())?;
    let max_attempts =
        i32::try_from(max_attempts).map_err(|_| PersistenceError::ValueOutOfRange)?;
    require_read_committed(client)?;
    let causation_ref = event.causation_ref();
    let params: &[&(dyn postgres::types::ToSql + Sync)] = &[
        &event.event_ref(),
        &event.event_type(),
        &event.schema_version(),
        &event.source(),
        &event.tenant_ref(),
        &event.subject_ref(),
        &occurred_at_unix_ms,
        &event.correlation_ref(),
        &causation_ref,
        &event.payload_digest(),
        &max_attempts,
    ];

    let inserted = client.execute(
        "INSERT INTO integration_outbox (\
             event_ref, event_type, schema_version, source_ref, tenant_ref, subject_ref,\
             occurred_at_unix_ms, correlation_ref, causation_ref, payload_digest,\
             max_attempts, current_state, latest_event_at_unix_ms\
         ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, 'pending', $7) \
         ON CONFLICT (source_ref, tenant_ref, event_ref) DO NOTHING",
        params,
    )?;
    if inserted == 1 {
        return Ok(PersistenceDisposition::Inserted);
    }

    let exact_replay_row = client.query_one(OUTBOX_REPLAY_QUERY, params)?;
    let exact_replay: bool = exact_replay_row.get(0);
    if exact_replay {
        Ok(PersistenceDisposition::Duplicate)
    } else {
        Err(PersistenceError::ConflictingReplay)
    }
}

/// Persist one immutable outbox delivery attempt and atomically advance delivery state.
///
/// The caller must supply the transaction so insertion of immutable attempt evidence
/// and the corresponding outbox-state transition cannot commit independently. The
/// outbox row is locked before replay classification or mutation, serializing adapter
/// calls for one [`OutboxPersistenceIdentity`]. Exact attempt replay remains
/// idempotent even after terminal delivery or quarantine. A different replay of the
/// same `attempt_ref`, backward time, unknown outbox, or new attempt after a terminal
/// state fails closed.
///
/// Retryable failure keeps the outbox pending while automatic-attempt budget remains;
/// exhausting that budget quarantines the outbox. Delivered attempts terminally mark
/// the outbox delivered, while permanent failures quarantine immediately.
///
/// # Errors
///
/// Returns [`PersistenceError`] for invalid references/timestamps, unsupported
/// transaction isolation, unknown or terminal outbox state, backward event time,
/// conflicting replay evidence, invalid stored state, or a database failure.
pub fn record_outbox_delivery_attempt(
    transaction: &mut Transaction<'_>,
    identity: OutboxPersistenceIdentity<'_>,
    attempt_ref: &str,
    outcome: DeliveryOutcome,
    occurred_at_unix_ms: u64,
    cause_code: Option<&str>,
) -> Result<DeliveryAttemptPersistence, PersistenceError> {
    let source_ref = required_persistence_reference(identity.source)?;
    let tenant_ref = required_persistence_reference(identity.tenant)?;
    let event_ref = required_persistence_reference(identity.event)?;
    let attempt_ref = required_persistence_reference(attempt_ref)?;
    if occurred_at_unix_ms == 0 {
        return Err(PersistenceError::InvalidTimestamp);
    }
    let cause_code = cause_code.map(required_persistence_reference).transpose()?;
    let occurred_at_unix_ms = postgres_bigint(occurred_at_unix_ms)?;
    require_read_committed(transaction)?;

    let outbox_row = transaction
        .query_opt(
            "SELECT max_attempts, current_state, latest_event_at_unix_ms \
             FROM integration_outbox \
             WHERE source_ref = $1 AND tenant_ref = $2 AND event_ref = $3 \
             FOR UPDATE",
            &[&source_ref, &tenant_ref, &event_ref],
        )?
        .ok_or(PersistenceError::OutboxNotFound)?;
    let max_attempts: i32 = outbox_row.get(0);
    let stored_state: String = outbox_row.get(1);
    let current_state = parse_outbox_state(&stored_state)?;
    let latest_event_at_unix_ms: i64 = outbox_row.get(2);

    let outcome_name = delivery_outcome_name(outcome);
    if let Some(existing_attempt) = transaction.query_opt(
        "SELECT delivery_outcome, occurred_at_unix_ms, cause_code \
         FROM integration_delivery_attempt \
         WHERE source_ref = $1 AND tenant_ref = $2 AND event_ref = $3 AND attempt_ref = $4",
        &[&source_ref, &tenant_ref, &event_ref, &attempt_ref],
    )? {
        let existing_outcome: String = existing_attempt.get(0);
        let existing_occurred_at_unix_ms: i64 = existing_attempt.get(1);
        let existing_cause_code: Option<String> = existing_attempt.get(2);
        if existing_outcome == outcome_name
            && existing_occurred_at_unix_ms == occurred_at_unix_ms
            && existing_cause_code.as_deref() == cause_code
        {
            return Ok(DeliveryAttemptPersistence {
                disposition: PersistenceDisposition::Duplicate,
                outbox_state: current_state,
            });
        }
        return Err(PersistenceError::ConflictingReplay);
    }

    if current_state != OutboxState::Pending {
        return Err(PersistenceError::TerminalOutboxState);
    }
    if occurred_at_unix_ms < latest_event_at_unix_ms {
        return Err(PersistenceError::NonMonotonicTimestamp);
    }

    let attempt_count: i64 = transaction
        .query_one(
            "WITH inserted_attempt AS (\
                 INSERT INTO integration_delivery_attempt (\
                     source_ref, tenant_ref, event_ref, attempt_ref, delivery_outcome,\
                     occurred_at_unix_ms, cause_code\
                 ) VALUES ($1, $2, $3, $4, $5, $6, $7) \
                 RETURNING 1\
             ) \
             SELECT count(*) + (SELECT count(*) FROM inserted_attempt) \
             FROM integration_delivery_attempt \
             WHERE source_ref = $1 AND tenant_ref = $2 AND event_ref = $3",
            &[
                &source_ref,
                &tenant_ref,
                &event_ref,
                &attempt_ref,
                &outcome_name,
                &occurred_at_unix_ms,
                &cause_code,
            ],
        )?
        .get(0);
    let next_state = match outcome {
        DeliveryOutcome::Delivered => OutboxState::Delivered,
        DeliveryOutcome::PermanentFailure => OutboxState::Quarantined,
        DeliveryOutcome::RetryableFailure if attempt_count >= i64::from(max_attempts) => {
            OutboxState::Quarantined
        }
        DeliveryOutcome::RetryableFailure => OutboxState::Pending,
    };
    let next_state_name = outbox_state_name(next_state);
    transaction.execute(
        "UPDATE integration_outbox \
         SET current_state = $4, latest_event_at_unix_ms = $5 \
         WHERE source_ref = $1 AND tenant_ref = $2 AND event_ref = $3",
        &[
            &source_ref,
            &tenant_ref,
            &event_ref,
            &next_state_name,
            &occurred_at_unix_ms,
        ],
    )?;

    Ok(DeliveryAttemptPersistence {
        disposition: PersistenceDisposition::Inserted,
        outbox_state: next_state,
    })
}

/// Accept or deduplicate one immutable tenant-bound event in a durable inbox.
///
/// The durable deduplication identity is
/// `(consumer_ref, source_ref, tenant_ref, source_event_ref)`. The first accepted
/// receive timestamp is retained. A later exact replay is reported as duplicate;
/// a replay with different event type, schema version, subject, or payload digest
/// fails closed.
///
/// The current insert-then-inspect replay algorithm requires `PostgreSQL` `READ
/// COMMITTED` isolation for the same statement-snapshot reason as
/// [`enqueue_outbox_event`].
///
/// # Errors
///
/// Returns [`PersistenceError`] for invalid consumer identity, invalid/out-of-range
/// receive time, unsupported transaction isolation, conflicting replay evidence,
/// or database failure.
pub fn accept_inbox_event(
    client: &mut impl GenericClient,
    consumer_ref: &str,
    event: &IntegrationEvent,
    received_at_unix_ms: u64,
) -> Result<InboxDisposition, PersistenceError> {
    let consumer_ref =
        normalized_reference(consumer_ref).ok_or(PersistenceError::InvalidReference)?;
    if received_at_unix_ms == 0 {
        return Err(PersistenceError::InvalidTimestamp);
    }
    let received_at_unix_ms = postgres_bigint(received_at_unix_ms)?;
    require_read_committed(client)?;

    let replay_params: &[&(dyn postgres::types::ToSql + Sync)] = &[
        &consumer_ref,
        &event.source(),
        &event.tenant_ref(),
        &event.event_ref(),
        &event.event_type(),
        &event.schema_version(),
        &event.subject_ref(),
        &event.payload_digest(),
    ];
    let insert_params: &[&(dyn postgres::types::ToSql + Sync)] = &[
        &consumer_ref,
        &event.source(),
        &event.tenant_ref(),
        &event.event_ref(),
        &event.event_type(),
        &event.schema_version(),
        &event.subject_ref(),
        &event.payload_digest(),
        &received_at_unix_ms,
    ];

    let inserted = client.execute(
        "INSERT INTO integration_inbox (\
             consumer_ref, source_ref, tenant_ref, source_event_ref, event_type, schema_version,\
             subject_ref, payload_digest, received_at_unix_ms\
         ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9) \
         ON CONFLICT (consumer_ref, source_ref, tenant_ref, source_event_ref) DO NOTHING",
        insert_params,
    )?;
    if inserted == 1 {
        return Ok(InboxDisposition::Accepted);
    }

    let exact_replay_row = client.query_one(INBOX_REPLAY_QUERY, replay_params)?;
    let exact_replay: bool = exact_replay_row.get(0);
    if exact_replay {
        Ok(InboxDisposition::Duplicate)
    } else {
        Err(PersistenceError::ConflictingReplay)
    }
}

fn required_persistence_reference(reference: &str) -> Result<&str, PersistenceError> {
    normalized_reference(reference).ok_or(PersistenceError::InvalidReference)
}

fn delivery_outcome_name(outcome: DeliveryOutcome) -> &'static str {
    match outcome {
        DeliveryOutcome::Delivered => "delivered",
        DeliveryOutcome::RetryableFailure => "retryable_failure",
        DeliveryOutcome::PermanentFailure => "permanent_failure",
    }
}

fn outbox_state_name(state: OutboxState) -> &'static str {
    match state {
        OutboxState::Pending => "pending",
        OutboxState::Delivered => "delivered",
        OutboxState::Quarantined => "quarantined",
    }
}

fn parse_outbox_state(state: &str) -> Result<OutboxState, PersistenceError> {
    match state {
        "pending" => Ok(OutboxState::Pending),
        "delivered" => Ok(OutboxState::Delivered),
        "quarantined" => Ok(OutboxState::Quarantined),
        _ => Err(PersistenceError::InvalidStoredState),
    }
}

fn require_read_committed(client: &mut impl GenericClient) -> Result<(), PersistenceError> {
    let row = client.query_one("SHOW transaction_isolation", &[])?;
    let isolation_level: String = row.get(0);
    if isolation_level == "read committed" {
        Ok(())
    } else {
        Err(PersistenceError::UnsupportedIsolationLevel)
    }
}

fn postgres_bigint(value: u64) -> Result<i64, PersistenceError> {
    i64::try_from(value).map_err(|_| PersistenceError::ValueOutOfRange)
}

#[cfg(test)]
mod tests {
    use super::{parse_outbox_state, PersistenceError};
    use crate::integration::OutboxState;

    #[test]
    fn stored_outbox_state_parser_is_fail_closed() {
        assert_eq!(parse_outbox_state("pending").unwrap(), OutboxState::Pending);
        assert_eq!(
            parse_outbox_state("delivered").unwrap(),
            OutboxState::Delivered
        );
        assert_eq!(
            parse_outbox_state("quarantined").unwrap(),
            OutboxState::Quarantined
        );
        assert!(matches!(
            parse_outbox_state("unexpected"),
            Err(PersistenceError::InvalidStoredState)
        ));
    }
}
