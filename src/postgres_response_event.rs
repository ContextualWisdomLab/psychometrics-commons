//! `PostgreSQL` 18 persistence for accepted response events.
//!
//! This adapter stores the mid-session ledger so a process restart can rebuild
//! the same accepted prefix before snapshot freeze. It does not store response
//! bodies and does not compute scores. The caller owns the connection,
//! credentials, and transaction boundary. Replay requires `READ COMMITTED` so a
//! concurrent insert that wins a unique-key race is visible to the exact-replay
//! classifier.

use crate::reference::normalized_reference;
use crate::response::{ResponseEvent, ResponseLedger, WriteError};
use postgres::Transaction;
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const RESPONSE_EVENT_MIGRATION: &str = include_str!("../migrations/0020_response_event.sql");

/// One accepted event plus the distinct observed and received clocks.
///
/// `observed_at_unix_ms` is source-valid time. `received_at_unix_ms` is
/// platform receipt time. Reload keeps both so a Korean IPIP Quick restart
/// can hand the same temporal prefix to later TEPP or scoring composition
/// without inventing an answer or a score.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResponseEventReceipt {
    event: ResponseEvent,
    observed_at_unix_ms: u64,
    received_at_unix_ms: u64,
}

impl ResponseEventReceipt {
    /// Return the accepted response-event identity and evidence.
    #[must_use]
    pub const fn event(&self) -> &ResponseEvent {
        &self.event
    }

    /// Return source-valid observed time in Unix milliseconds.
    #[must_use]
    pub const fn observed_at_unix_ms(&self) -> u64 {
        self.observed_at_unix_ms
    }

    /// Return platform receipt time in Unix milliseconds.
    #[must_use]
    pub const fn received_at_unix_ms(&self) -> u64 {
        self.received_at_unix_ms
    }
}

/// Outcome of persisting one accepted response event.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ResponseEventPersistenceDisposition {
    /// A new accepted event row was inserted.
    Inserted,
    /// The same immutable event identity and evidence already existed.
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
    /// A server sequence was reused by another event identity in the session.
    SequenceConflict,
    /// A sequence cannot be represented by the bounded database column.
    InvalidSequence,
    /// Observed time was zero, inverted after received time, or out of range.
    InvalidTimestamp,
    /// Response-event persistence requires `PostgreSQL` `READ COMMITTED` isolation.
    UnsupportedIsolationLevel,
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
            Self::InvalidSequence => {
                "response event sequence is missing, gapped, or outside the PostgreSQL bigint range"
            }
            Self::InvalidTimestamp => {
                "response event observed time must be positive and not after received time"
            }
            Self::UnsupportedIsolationLevel => {
                "response event persistence requires read committed isolation"
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

/// Persist one accepted response event for a session.
///
/// Exact replay of the same event identity, session binding, item version,
/// payload digest, server sequence, and observed/received times is idempotent.
/// Rebinding any of those values fails closed. Historical accepted answers are
/// never updated. `observed_at_unix_ms` is source-valid time; `received_at_unix_ms`
/// is platform receipt time.
///
/// # Errors
///
/// Returns [`ResponseEventPersistenceError`] for an unbound identity,
/// unsupported isolation, conflicting replay, a sequence conflict, an invalid
/// sequence or timestamp, or a database failure.
pub fn persist_response_event(
    transaction: &mut Transaction<'_>,
    session_ref: &str,
    event: &ResponseEvent,
    observed_at_unix_ms: u64,
    received_at_unix_ms: u64,
) -> Result<ResponseEventPersistenceDisposition, ResponseEventPersistenceError> {
    require_read_committed(transaction)?;
    let session_ref = required_reference(session_ref)?;
    let server_event_ref = required_reference(event.server_event_ref())?;
    let client_event_ref = required_reference(event.client_event_ref())?;
    let item_version_ref = required_reference(event.item_version_ref())?;
    let server_sequence = postgres_sequence(event.sequence())?;
    let observed_at = postgres_timestamptz(observed_at_unix_ms)?;
    let received_at = postgres_timestamptz(received_at_unix_ms)?;
    if observed_at_unix_ms > received_at_unix_ms {
        return Err(ResponseEventPersistenceError::InvalidTimestamp);
    }

    let inserted = match transaction.execute(
        "INSERT INTO response_event (\
             response_event_ref, session_ref, client_event_ref, item_version_ref, \
             payload_digest, server_sequence, observed_at, received_at\
         ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8) \
         ON CONFLICT (response_event_ref) DO NOTHING",
        &[
            &server_event_ref,
            &session_ref,
            &client_event_ref,
            &item_version_ref,
            &event.payload_digest(),
            &server_sequence,
            &observed_at,
            &received_at,
        ],
    ) {
        Ok(count) => count,
        Err(error) => return Err(classify_unique_violation(error)),
    };
    if inserted == 1 {
        return Ok(ResponseEventPersistenceDisposition::Inserted);
    }
    classify_existing_event(
        transaction,
        session_ref,
        event,
        server_event_ref,
        server_sequence,
        observed_at,
        received_at,
    )
}

/// Rebuild accepted events and their observed/received clocks after restart.
///
/// Rows are read in `server_sequence` order. A missing session returns an empty
/// list. Gapped, reordered, conflicting, inverted, or zero stored times fail
/// closed. Observed time stays distinct from platform receipt time.
///
/// # Errors
///
/// Returns [`ResponseEventPersistenceError`] for an unbound session, unsupported
/// isolation, corrupt stored history, invalid stored time, or a database failure.
pub fn load_response_event_receipts(
    transaction: &mut Transaction<'_>,
    session_ref: &str,
) -> Result<Vec<ResponseEventReceipt>, ResponseEventPersistenceError> {
    require_read_committed(transaction)?;
    let session_ref = required_reference(session_ref)?;
    let rows = transaction.query(
        "SELECT response_event_ref, client_event_ref, item_version_ref, \
                payload_digest, server_sequence, observed_at, received_at \
         FROM response_event \
         WHERE session_ref = $1 \
         ORDER BY server_sequence",
        &[&session_ref],
    )?;
    let mut receipts = Vec::with_capacity(rows.len());
    for (index, row) in rows.into_iter().enumerate() {
        let server_event_ref: String = row.get(0);
        let client_event_ref: String = row.get(1);
        let item_version_ref: String = row.get(2);
        let payload_digest: String = row.get(3);
        let server_sequence: i64 = row.get(4);
        let observed_at: SystemTime = row.get(5);
        let received_at: SystemTime = row.get(6);
        let sequence = usize::try_from(server_sequence)
            .map_err(|_| ResponseEventPersistenceError::InvalidSequence)?;
        require_contiguous_server_sequence(index, sequence)?;
        let observed_at_unix_ms = unix_ms_from_system_time(observed_at)?;
        let received_at_unix_ms = unix_ms_from_system_time(received_at)?;
        if observed_at_unix_ms > received_at_unix_ms {
            return Err(ResponseEventPersistenceError::InvalidTimestamp);
        }
        let event = ResponseEvent::from_persisted(
            server_event_ref,
            client_event_ref,
            item_version_ref,
            payload_digest,
            sequence,
        )
        .map_err(map_rebuild_error)?;
        receipts.push(ResponseEventReceipt {
            event,
            observed_at_unix_ms,
            received_at_unix_ms,
        });
    }
    Ok(receipts)
}

/// Rebuild the accepted response ledger for one session after restart.
///
/// Rows are read in `server_sequence` order. A missing session returns an empty
/// ledger. Gapped, reordered, conflicting identities, or invalid stored times
/// fail closed.
///
/// # Errors
///
/// Returns [`ResponseEventPersistenceError`] for an unbound session, unsupported
/// isolation, corrupt stored history, or a database failure.
pub fn load_response_ledger(
    transaction: &mut Transaction<'_>,
    session_ref: &str,
) -> Result<ResponseLedger, ResponseEventPersistenceError> {
    let receipts = load_response_event_receipts(transaction, session_ref)?;
    let events = receipts.into_iter().map(|receipt| receipt.event).collect();
    ResponseLedger::from_persisted(session_ref, events).map_err(map_rebuild_error)
}

fn classify_existing_event(
    transaction: &mut Transaction<'_>,
    session_ref: &str,
    event: &ResponseEvent,
    server_event_ref: &str,
    server_sequence: i64,
    observed_at: SystemTime,
    received_at: SystemTime,
) -> Result<ResponseEventPersistenceDisposition, ResponseEventPersistenceError> {
    let row = transaction.query_one(
        "SELECT session_ref, client_event_ref, item_version_ref, payload_digest, \
                server_sequence, observed_at, received_at \
         FROM response_event WHERE response_event_ref = $1",
        &[&server_event_ref],
    )?;
    let stored_session: String = row.get(0);
    let stored_client: String = row.get(1);
    let stored_item: String = row.get(2);
    let stored_digest: String = row.get(3);
    let stored_sequence: i64 = row.get(4);
    let stored_observed: SystemTime = row.get(5);
    let stored_received: SystemTime = row.get(6);
    if stored_session == session_ref
        && stored_client == event.client_event_ref()
        && stored_item == event.item_version_ref()
        && stored_digest == event.payload_digest()
        && stored_sequence == server_sequence
        && stored_observed == observed_at
        && stored_received == received_at
    {
        Ok(ResponseEventPersistenceDisposition::Duplicate)
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

fn map_rebuild_error(error: WriteError) -> ResponseEventPersistenceError {
    match error {
        WriteError::InvalidReference => ResponseEventPersistenceError::InvalidReference,
        WriteError::InvalidSequence => ResponseEventPersistenceError::InvalidSequence,
        WriteError::EmptyReference
        | WriteError::InvalidPayloadDigest
        | WriteError::IdempotencyConflict
        | WriteError::ServerReferenceConflict
        | WriteError::SessionNotActive(_)
        | WriteError::SnapshotRequiresCompleted(_) => {
            ResponseEventPersistenceError::ConflictingReplay
        }
    }
}

fn required_reference(reference: &str) -> Result<&str, ResponseEventPersistenceError> {
    normalized_reference(reference).ok_or(ResponseEventPersistenceError::InvalidReference)
}

fn postgres_sequence(value: usize) -> Result<i64, ResponseEventPersistenceError> {
    i64::try_from(value).map_err(|_| ResponseEventPersistenceError::InvalidSequence)
}

fn postgres_timestamptz(unix_ms: u64) -> Result<SystemTime, ResponseEventPersistenceError> {
    if unix_ms == 0 {
        return Err(ResponseEventPersistenceError::InvalidTimestamp);
    }
    // Every `u64` millisecond offset from the Unix epoch is representable as
    // `SystemTime` on the supported 64-bit hosts. An overflow `checked_add`
    // arm would be untestable and would fail the exact branch-coverage gate.
    Ok(UNIX_EPOCH + Duration::from_millis(unix_ms))
}

fn unix_ms_from_system_time(time: SystemTime) -> Result<u64, ResponseEventPersistenceError> {
    let duration = time
        .duration_since(UNIX_EPOCH)
        .map_err(|_| ResponseEventPersistenceError::InvalidTimestamp)?;
    let unix_ms = duration
        .as_secs()
        .checked_mul(1_000)
        .and_then(|value| value.checked_add(u64::from(duration.subsec_millis())))
        .ok_or(ResponseEventPersistenceError::InvalidTimestamp)?;
    if unix_ms == 0 {
        return Err(ResponseEventPersistenceError::InvalidTimestamp);
    }
    Ok(unix_ms)
}

fn require_contiguous_server_sequence(
    index: usize,
    sequence: usize,
) -> Result<(), ResponseEventPersistenceError> {
    if sequence == index + 1 {
        Ok(())
    } else {
        Err(ResponseEventPersistenceError::InvalidSequence)
    }
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
        map_rebuild_error, postgres_sequence, postgres_timestamptz,
        require_contiguous_server_sequence, required_reference, unix_ms_from_system_time,
        ResponseEventPersistenceError, ResponseEventReceipt,
    };
    use crate::response::ResponseEvent;
    use crate::response::WriteError;
    use crate::session::SessionState;
    use std::time::{Duration, UNIX_EPOCH};

    #[test]
    fn blank_numeric_and_overflow_sequences_fail_closed() {
        assert!(matches!(
            required_reference(" "),
            Err(ResponseEventPersistenceError::InvalidReference)
        ));
        assert!(matches!(
            required_reference("12"),
            Err(ResponseEventPersistenceError::InvalidReference)
        ));
        assert_eq!(
            required_reference("session_ipip_ko_quick").unwrap(),
            "session_ipip_ko_quick"
        );
        assert_eq!(postgres_sequence(1).unwrap(), 1);
        assert!(require_contiguous_server_sequence(0, 1).is_ok());
        assert!(require_contiguous_server_sequence(1, 2).is_ok());
        assert!(matches!(
            require_contiguous_server_sequence(1, 3),
            Err(ResponseEventPersistenceError::InvalidSequence)
        ));
        assert!(matches!(
            require_contiguous_server_sequence(0, 2),
            Err(ResponseEventPersistenceError::InvalidSequence)
        ));
        assert!(matches!(
            postgres_sequence(usize::MAX),
            Err(ResponseEventPersistenceError::InvalidSequence)
        ));
        assert!(matches!(
            postgres_timestamptz(0),
            Err(ResponseEventPersistenceError::InvalidTimestamp)
        ));
        assert!(postgres_timestamptz(1_700_000_000_000).is_ok());
        assert_eq!(
            postgres_timestamptz(u64::MAX).unwrap(),
            UNIX_EPOCH + Duration::from_millis(u64::MAX)
        );
        assert!(matches!(
            unix_ms_from_system_time(UNIX_EPOCH),
            Err(ResponseEventPersistenceError::InvalidTimestamp)
        ));
        assert!(matches!(
            unix_ms_from_system_time(UNIX_EPOCH - Duration::from_millis(1)),
            Err(ResponseEventPersistenceError::InvalidTimestamp)
        ));
        assert_eq!(
            unix_ms_from_system_time(UNIX_EPOCH + Duration::from_secs(1_700_000_000)).unwrap(),
            1_700_000_000_000
        );
        let overflow_secs = u64::MAX / 1_000 + 1;
        if let Some(far_future) = UNIX_EPOCH.checked_add(Duration::from_secs(overflow_secs)) {
            assert!(matches!(
                unix_ms_from_system_time(far_future),
                Err(ResponseEventPersistenceError::InvalidTimestamp)
            ));
        }
        let add_overflow_secs = u64::MAX / 1_000;
        if let Some(near_max) = UNIX_EPOCH
            .checked_add(Duration::from_secs(add_overflow_secs))
            .and_then(|time| time.checked_add(Duration::from_millis(616)))
        {
            assert!(matches!(
                unix_ms_from_system_time(near_max),
                Err(ResponseEventPersistenceError::InvalidTimestamp)
            ));
        }
    }

    #[test]
    fn receipt_keeps_distinct_observed_and_received_times() {
        let event = ResponseEvent::from_persisted(
            "server_event_item_01",
            "client_event_item_01",
            "item_version_n1_ko",
            "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            1,
        )
        .unwrap();
        let receipt = ResponseEventReceipt {
            event: event.clone(),
            observed_at_unix_ms: 1_700_000_000_000,
            received_at_unix_ms: 1_700_000_000_250,
        };
        assert_eq!(receipt.event(), &event);
        assert_eq!(receipt.observed_at_unix_ms(), 1_700_000_000_000);
        assert_eq!(receipt.received_at_unix_ms(), 1_700_000_000_250);
    }

    #[test]
    fn rebuild_errors_map_to_typed_persistence_failures() {
        assert!(matches!(
            map_rebuild_error(WriteError::InvalidReference),
            ResponseEventPersistenceError::InvalidReference
        ));
        assert!(matches!(
            map_rebuild_error(WriteError::InvalidSequence),
            ResponseEventPersistenceError::InvalidSequence
        ));
        for error in [
            WriteError::EmptyReference,
            WriteError::InvalidPayloadDigest,
            WriteError::IdempotencyConflict,
            WriteError::ServerReferenceConflict,
            WriteError::SessionNotActive(SessionState::Paused),
            WriteError::SnapshotRequiresCompleted(SessionState::Active),
        ] {
            assert!(matches!(
                map_rebuild_error(error),
                ResponseEventPersistenceError::ConflictingReplay
            ));
        }
    }
}
