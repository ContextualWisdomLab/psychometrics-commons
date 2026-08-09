//! `PostgreSQL` 18 persistence adapter for integration evidence.
//!
//! The adapter receives a caller-owned `PostgreSQL` client or transaction. It never
//! owns credentials or a cross-service connection. The migration and write paths
//! preserve the immutable integration-event, outbox, and inbox contracts defined
//! by [`crate::integration`].

use crate::integration::{InboxDisposition, IntegrationEvent};
use crate::reference::normalized_reference;
use postgres::GenericClient;
use std::error::Error;
use std::fmt::{Display, Formatter};

const INTEGRATION_MIGRATION: &str = include_str!("../migrations/0001_integration_delivery.sql");

/// Outcome of inserting immutable persistence evidence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum PersistenceDisposition {
    /// The immutable evidence was inserted for the first time.
    Inserted,
    /// Identical immutable evidence already existed.
    Duplicate,
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
    /// An idempotency identity already exists with different immutable evidence.
    ConflictingReplay,
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
            Self::ConflictingReplay => {
                "persistence idempotency identity was replayed with conflicting evidence"
            }
            Self::Database(_) => "PostgreSQL persistence operation failed",
        })
    }
}

impl Error for PersistenceError {}

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
/// A repeated `event_ref` is idempotent only when every immutable event field and
/// `max_attempts` match the existing row. Reuse with different evidence fails
/// closed. The function uses the caller-owned client/transaction, so a domain
/// mutation and this outbox insert can be committed atomically by the caller.
///
/// # Errors
///
/// Returns [`PersistenceError`] for an invalid attempt limit, a value outside the
/// supported `PostgreSQL` integer range, conflicting replay, or a database failure.
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
         ON CONFLICT (event_ref) DO NOTHING",
        params,
    )?;
    if inserted == 1 {
        return Ok(PersistenceDisposition::Inserted);
    }

    let exact_replay: bool = client
        .query_one(
            "SELECT EXISTS (\
                 SELECT 1 FROM integration_outbox \
                 WHERE event_ref = $1 AND event_type = $2 AND schema_version = $3 \
                   AND source_ref = $4 AND tenant_ref = $5 AND subject_ref = $6 \
                   AND occurred_at_unix_ms = $7 AND correlation_ref = $8 \
                   AND causation_ref IS NOT DISTINCT FROM $9 \
                   AND payload_digest = $10 AND max_attempts = $11\
             )",
            params,
        )?
        .get(0);
    if exact_replay {
        Ok(PersistenceDisposition::Duplicate)
    } else {
        Err(PersistenceError::ConflictingReplay)
    }
}

/// Accept or deduplicate one immutable tenant-bound event in a durable inbox.
///
/// The durable deduplication identity is
/// `(consumer_ref, source_ref, tenant_ref, source_event_ref)`. The first accepted
/// receive timestamp is retained. A later exact replay is reported as duplicate;
/// a replay with different event type, schema version, subject, or payload digest
/// fails closed.
///
/// # Errors
///
/// Returns [`PersistenceError`] for invalid consumer identity, invalid/out-of-range
/// receive time, conflicting replay evidence, or database failure.
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
    let params: &[&(dyn postgres::types::ToSql + Sync)] = &[
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
        params,
    )?;
    if inserted == 1 {
        return Ok(InboxDisposition::Accepted);
    }

    let exact_replay: bool = client
        .query_one(
            "SELECT EXISTS (\
                 SELECT 1 FROM integration_inbox \
                 WHERE consumer_ref = $1 AND source_ref = $2 AND tenant_ref = $3 \
                   AND source_event_ref = $4 AND event_type = $5 AND schema_version = $6 \
                   AND subject_ref = $7 AND payload_digest = $8\
             )",
            &params[..8],
        )?
        .get(0);
    if exact_replay {
        Ok(InboxDisposition::Duplicate)
    } else {
        Err(PersistenceError::ConflictingReplay)
    }
}

fn postgres_bigint(value: u64) -> Result<i64, PersistenceError> {
    i64::try_from(value).map_err(|_| PersistenceError::ValueOutOfRange)
}
