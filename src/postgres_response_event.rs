//! `PostgreSQL` 18 persistence for in-progress response events.
//!
//! Completed snapshots remain in [`crate::postgres_response_snapshot`]. This
//! adapter stores the accepted event ledger so a two-item path can continue
//! after process restart. It does not store response bodies and does not score.
//! The caller owns the connection, credentials, and transaction boundary.
//! Replay requires `READ COMMITTED` so a concurrent insert that wins a unique-key
//! race is visible to the exact-replay classifier.

use crate::reference::normalized_reference;
use crate::response::{ResponseEvent, ResponseLedger};
use postgres::Transaction;
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const RESPONSE_EVENT_MIGRATION: &str = include_str!("../migrations/0020_response_event.sql");

/// Outcome of persisting one response-event ledger.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ResponseEventPersistenceDisposition {
    /// At least one new event row was inserted.
    Inserted,
    /// The same immutable event evidence already existed.
    Duplicate,
}

/// Fail-closed error for durable response-event persistence.
#[derive(Debug)]
#[non_exhaustive]
pub enum ResponseEventPersistenceError {
    /// A session or event identity was blank, numeric-like, or unbound.
    InvalidReference,
    /// Event identity was replayed with different immutable evidence.
    ConflictingReplay,
    /// A server sequence was reused by another event identity.
    SequenceConflict,
    /// A sequence cannot be represented by the bounded database column.
    InvalidSequence,
    /// Observed or received time was zero, inverted, or out of range.
    InvalidTimestamp,
    /// Response-event persistence requires `PostgreSQL` `READ COMMITTED` isolation.
    UnsupportedIsolationLevel,
    /// Stored rows could not be rebuilt into a domain ledger.
    InvalidStoredIdentity,
    /// `PostgreSQL` rejected or could not execute the persistence operation.
    Database(postgres::Error),
}

impl Display for ResponseEventPersistenceError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidReference => {
                "response event persistence references must be opaque durable values"
            }
            Self::ConflictingReplay => {
                "response event identity was replayed with conflicting evidence"
            }
            Self::SequenceConflict => {
                "response event sequence was reused by a different event identity"
            }
            Self::InvalidSequence => "response event sequence exceeds the PostgreSQL bigint range",
            Self::InvalidTimestamp => {
                "response event observed time must be positive and not after received time"
            }
            Self::UnsupportedIsolationLevel => {
                "response event persistence requires read committed isolation"
            }
            Self::InvalidStoredIdentity => {
                "stored response events could not be rebuilt into a ledger"
            }
            Self::Database(_) => "PostgreSQL response-event persistence failed",
        })
    }
}

impl Error for ResponseEventPersistenceError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Database(error) => Some(error),
            _ => None,
        }
    }
}

impl From<postgres::Error> for ResponseEventPersistenceError {
    fn from(error: postgres::Error) -> Self {
        Self::Database(error)
    }
}

/// Apply the idempotent response-event migration to a `PostgreSQL` connection.
///
/// # Errors
///
/// Returns the `PostgreSQL` error if the migration cannot be applied.
pub fn apply_response_event_migration(
    client: &mut impl postgres::GenericClient,
) -> Result<(), postgres::Error> {
    client.batch_execute(RESPONSE_EVENT_MIGRATION)
}

/// Persist every accepted event in one response ledger.
///
/// `event_times` is `(observed_at_unix_ms, received_at_unix_ms)` aligned with
/// [`ResponseLedger::events`]. Exact replay is idempotent. Rebinding client,
/// server, item, digest, sequence, or session evidence fails closed.
///
/// # Errors
///
/// Returns [`ResponseEventPersistenceError`] for invalid identity, inverted
/// time, unsupported isolation, conflicting replay, sequence reuse, or a
/// database failure.
pub fn persist_response_ledger(
    transaction: &mut Transaction<'_>,
    ledger: &ResponseLedger,
    event_times: &[(u64, u64)],
) -> Result<ResponseEventPersistenceDisposition, ResponseEventPersistenceError> {
    require_read_committed(transaction)?;
    if event_times.len() != ledger.len() {
        return Err(ResponseEventPersistenceError::InvalidTimestamp);
    }
    let session_ref = required_reference(ledger.session_ref())?;
    let mut inserted_any = false;
    for (event, (observed_at_unix_ms, received_at_unix_ms)) in
        ledger.events().iter().zip(event_times.iter().copied())
    {
        if persist_one_event(
            transaction,
            session_ref,
            event,
            observed_at_unix_ms,
            received_at_unix_ms,
        )? {
            inserted_any = true;
        }
    }
    Ok(if inserted_any {
        ResponseEventPersistenceDisposition::Inserted
    } else {
        ResponseEventPersistenceDisposition::Duplicate
    })
}

/// Load the accepted event ledger for one session after process restart.
///
/// Missing sessions return [`None`]. Stored order is `server_sequence`.
///
/// # Errors
///
/// Returns [`ResponseEventPersistenceError`] for unsupported isolation, a
/// malformed session reference, invalid stored identity, or a database failure.
pub fn load_response_ledger(
    transaction: &mut Transaction<'_>,
    session_ref: &str,
) -> Result<Option<ResponseLedger>, ResponseEventPersistenceError> {
    require_read_committed(transaction)?;
    let session_ref = required_reference(session_ref)?;
    let rows = transaction.query(
        "SELECT response_event_ref, client_event_ref, item_version_ref, \
                payload_digest, server_sequence \
         FROM response_event \
         WHERE session_ref = $1 \
         ORDER BY server_sequence",
        &[&session_ref],
    )?;
    if rows.is_empty() {
        return Ok(None);
    }
    let mut events = Vec::with_capacity(rows.len());
    for row in rows {
        let sequence = postgres_loaded_sequence(row.get(4))?;
        let event = ResponseEvent::from_persisted(
            row.get::<_, String>(0).as_str(),
            row.get::<_, String>(1).as_str(),
            row.get::<_, String>(2).as_str(),
            row.get::<_, String>(3).as_str(),
            sequence,
        )
        .map_err(|_| ResponseEventPersistenceError::InvalidStoredIdentity)?;
        events.push(event);
    }
    ResponseLedger::from_persisted(session_ref, events)
        .map(Some)
        .map_err(|_| ResponseEventPersistenceError::InvalidStoredIdentity)
}

fn persist_one_event(
    transaction: &mut Transaction<'_>,
    session_ref: &str,
    event: &ResponseEvent,
    observed_at_unix_ms: u64,
    received_at_unix_ms: u64,
) -> Result<bool, ResponseEventPersistenceError> {
    let response_event_ref = required_reference(event.server_event_ref())?;
    let client_event_ref = required_reference(event.client_event_ref())?;
    let item_version_ref = required_reference(event.item_version_ref())?;
    let server_sequence = postgres_sequence(event.sequence())?;
    let observed_at = postgres_timestamptz(observed_at_unix_ms)?;
    let received_at = postgres_timestamptz(received_at_unix_ms)?;
    if observed_at_unix_ms > received_at_unix_ms {
        return Err(ResponseEventPersistenceError::InvalidTimestamp);
    }
    let row = match transaction.query_one(
        "WITH inserted AS (\
             INSERT INTO response_event (\
                 response_event_ref, session_ref, client_event_ref, item_version_ref, \
                 payload_digest, server_sequence, observed_at, received_at\
             ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8) \
             ON CONFLICT (response_event_ref) DO NOTHING \
             RETURNING session_ref, client_event_ref, item_version_ref, payload_digest, \
                       server_sequence, TRUE AS inserted\
         ) \
         SELECT session_ref, client_event_ref, item_version_ref, payload_digest, \
                server_sequence, inserted \
         FROM inserted \
         UNION ALL \
         SELECT session_ref, client_event_ref, item_version_ref, payload_digest, \
                server_sequence, FALSE AS inserted \
         FROM response_event WHERE response_event_ref = $1 \
         LIMIT 1",
        &[
            &response_event_ref,
            &session_ref,
            &client_event_ref,
            &item_version_ref,
            &event.payload_digest(),
            &server_sequence,
            &observed_at,
            &received_at,
        ],
    ) {
        Ok(row) => row,
        Err(error) => return Err(classify_unique_violation(error)),
    };
    let inserted: bool = row.get(5);
    if inserted {
        Ok(true)
    } else {
        classify_existing_event(&row, session_ref, event, server_sequence)
    }
}

fn classify_existing_event(
    row: &postgres::Row,
    session_ref: &str,
    event: &ResponseEvent,
    server_sequence: i64,
) -> Result<bool, ResponseEventPersistenceError> {
    let stored_session: String = row.get(0);
    let stored_client: String = row.get(1);
    let stored_item: String = row.get(2);
    let stored_digest: String = row.get(3);
    let stored_sequence: i64 = row.get(4);
    if stored_session == session_ref
        && stored_client == event.client_event_ref()
        && stored_item == event.item_version_ref()
        && stored_digest == event.payload_digest()
        && stored_sequence == server_sequence
    {
        Ok(false)
    } else {
        Err(ResponseEventPersistenceError::ConflictingReplay)
    }
}

fn classify_unique_violation(error: postgres::Error) -> ResponseEventPersistenceError {
    match error
        .as_db_error()
        .and_then(postgres::error::DbError::constraint)
    {
        Some("response_event_session_client_unique") => {
            ResponseEventPersistenceError::ConflictingReplay
        }
        Some("response_event_session_sequence_unique") => {
            ResponseEventPersistenceError::SequenceConflict
        }
        _ => ResponseEventPersistenceError::Database(error),
    }
}

fn required_reference(reference: &str) -> Result<&str, ResponseEventPersistenceError> {
    normalized_reference(reference).ok_or(ResponseEventPersistenceError::InvalidReference)
}

fn postgres_sequence(value: usize) -> Result<i64, ResponseEventPersistenceError> {
    i64::try_from(value).map_err(|_| ResponseEventPersistenceError::InvalidSequence)
}

fn postgres_loaded_sequence(value: i64) -> Result<usize, ResponseEventPersistenceError> {
    usize::try_from(value).map_err(|_| ResponseEventPersistenceError::InvalidStoredIdentity)
}

fn postgres_timestamptz(unix_ms: u64) -> Result<SystemTime, ResponseEventPersistenceError> {
    if unix_ms == 0 {
        return Err(ResponseEventPersistenceError::InvalidTimestamp);
    }
    UNIX_EPOCH
        .checked_add(Duration::from_millis(unix_ms))
        .ok_or(ResponseEventPersistenceError::InvalidTimestamp)
}

fn require_read_committed(
    transaction: &mut Transaction<'_>,
) -> Result<(), ResponseEventPersistenceError> {
    let row = transaction.query_one("SHOW transaction_isolation", &[])?;
    let isolation: String = row.get(0);
    if isolation == "read committed" {
        Ok(())
    } else {
        Err(ResponseEventPersistenceError::UnsupportedIsolationLevel)
    }
}

#[cfg(test)]
mod reference_guard_tests {
    use super::{
        postgres_sequence, postgres_timestamptz, required_reference, ResponseEventPersistenceError,
    };

    #[test]
    fn blank_numeric_zero_time_and_overflow_fail_closed() {
        assert!(matches!(
            required_reference(" "),
            Err(ResponseEventPersistenceError::InvalidReference)
        ));
        assert!(matches!(
            required_reference("12"),
            Err(ResponseEventPersistenceError::InvalidReference)
        ));
        assert_eq!(
            required_reference("session_big_five_ko").unwrap(),
            "session_big_five_ko"
        );
        assert_eq!(postgres_sequence(1).unwrap(), 1);
        assert!(matches!(
            postgres_sequence(usize::MAX),
            Err(ResponseEventPersistenceError::InvalidSequence)
        ));
        assert!(matches!(
            postgres_timestamptz(0),
            Err(ResponseEventPersistenceError::InvalidTimestamp)
        ));
        assert!(postgres_timestamptz(1_700_000_000_000).is_ok());
    }
}
