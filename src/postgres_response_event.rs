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
/// unsupported isolation, conflicting replay, a sequence conflict, a gapped
/// or out-of-range sequence, an invalid timestamp, or a database failure.
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
    let next_sequence = next_contiguous_sequence(transaction, session_ref)?;
    if server_sequence > next_sequence {
        return Err(ResponseEventPersistenceError::InvalidSequence);
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
/// list. Gapped, reordered, conflicting identities, inverted, or zero stored
/// times fail closed before any receipt is returned. Observed time stays
/// distinct from platform receipt time.
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
    for row in rows {
        let server_event_ref: String = row.get(0);
        let client_event_ref: String = row.get(1);
        let item_version_ref: String = row.get(2);
        let payload_digest: String = row.get(3);
        let server_sequence: i64 = row.get(4);
        let observed_at: SystemTime = row.get(5);
        let received_at: SystemTime = row.get(6);
        let sequence = usize::try_from(server_sequence)
            .map_err(|_| ResponseEventPersistenceError::InvalidSequence)?;
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
    require_contiguous_receipt_history(session_ref, &receipts)?;
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
    let row = query_existing_event_row(transaction, server_event_ref)?;
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

fn query_existing_event_row(
    transaction: &mut Transaction<'_>,
    server_event_ref: &str,
) -> Result<postgres::Row, ResponseEventPersistenceError> {
    match transaction.query_one(
        "SELECT session_ref, client_event_ref, item_version_ref, payload_digest, \
                server_sequence, observed_at, received_at \
         FROM response_event WHERE response_event_ref = $1",
        &[&server_event_ref],
    ) {
        Ok(row) => Ok(row),
        Err(error) => Err(ResponseEventPersistenceError::from(error)),
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

fn require_contiguous_receipt_history(
    session_ref: &str,
    receipts: &[ResponseEventReceipt],
) -> Result<(), ResponseEventPersistenceError> {
    let events = receipts
        .iter()
        .map(|receipt| receipt.event.clone())
        .collect();
    ResponseLedger::from_persisted(session_ref, events).map_err(map_rebuild_error)?;
    Ok(())
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

fn next_contiguous_sequence(
    transaction: &mut Transaction<'_>,
    session_ref: &str,
) -> Result<i64, ResponseEventPersistenceError> {
    let highest: Option<i64> = transaction
        .query_one(
            "SELECT MAX(server_sequence) FROM response_event WHERE session_ref = $1",
            &[&session_ref],
        )?
        .get(0);
    Ok(highest.map_or(1, |value| value.saturating_add(1)))
}

fn postgres_timestamptz(unix_ms: u64) -> Result<SystemTime, ResponseEventPersistenceError> {
    if unix_ms == 0 {
        return Err(ResponseEventPersistenceError::InvalidTimestamp);
    }
    Ok(UNIX_EPOCH + Duration::from_millis(unix_ms))
}

fn unix_ms_from_system_time(time: SystemTime) -> Result<u64, ResponseEventPersistenceError> {
    let duration = time
        .duration_since(UNIX_EPOCH)
        .map_err(|_| ResponseEventPersistenceError::InvalidTimestamp)?;
    millis_from_duration(duration)
}

fn millis_from_duration(duration: Duration) -> Result<u64, ResponseEventPersistenceError> {
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
        apply_response_event_migration, classify_existing_event, load_response_event_receipts,
        load_response_ledger, map_rebuild_error, millis_from_duration, next_contiguous_sequence,
        persist_response_event, postgres_sequence, postgres_timestamptz, query_existing_event_row,
        require_contiguous_receipt_history, required_reference, unix_ms_from_system_time,
        ResponseEventPersistenceError, ResponseEventReceipt,
    };
    use crate::response::ResponseEvent;
    use crate::response::WriteError;
    use crate::session::SessionState;
    use postgres::{Client, IsolationLevel, NoTls};
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
        assert!(matches!(
            postgres_sequence(usize::MAX),
            Err(ResponseEventPersistenceError::InvalidSequence)
        ));
        assert!(matches!(
            postgres_timestamptz(0),
            Err(ResponseEventPersistenceError::InvalidTimestamp)
        ));
        assert!(postgres_timestamptz(1_700_000_000_000).is_ok());
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
        assert!(matches!(
            millis_from_duration(Duration::from_secs(u64::MAX / 1_000 + 1)),
            Err(ResponseEventPersistenceError::InvalidTimestamp)
        ));
        assert!(matches!(
            millis_from_duration(Duration::from_millis(0)),
            Err(ResponseEventPersistenceError::InvalidTimestamp)
        ));
    }

    #[test]
    fn gapped_or_duplicate_receipt_history_fails_closed() {
        let first = ResponseEvent::from_persisted(
            "server_event_item_01",
            "client_event_item_01",
            "item_version_n1_ko",
            "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            1,
        )
        .unwrap();
        let gapped = ResponseEvent::from_persisted(
            "server_event_item_03",
            "client_event_item_03",
            "item_version_n3_ko",
            "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            3,
        )
        .unwrap();
        let duplicate_server = ResponseEvent::from_persisted(
            "server_event_item_01",
            "client_event_item_02",
            "item_version_n2_ko",
            "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            2,
        )
        .unwrap();
        let first_receipt = ResponseEventReceipt {
            event: first.clone(),
            observed_at_unix_ms: 1_700_000_000_000,
            received_at_unix_ms: 1_700_000_000_250,
        };
        assert!(matches!(
            require_contiguous_receipt_history(
                "session_ipip_ko_quick",
                &[
                    first_receipt.clone(),
                    ResponseEventReceipt {
                        event: gapped,
                        observed_at_unix_ms: 1_700_000_000_500,
                        received_at_unix_ms: 1_700_000_000_750,
                    },
                ]
            ),
            Err(ResponseEventPersistenceError::InvalidSequence)
        ));
        assert!(matches!(
            require_contiguous_receipt_history(
                "session_ipip_ko_quick",
                &[
                    first_receipt,
                    ResponseEventReceipt {
                        event: duplicate_server,
                        observed_at_unix_ms: 1_700_000_000_500,
                        received_at_unix_ms: 1_700_000_000_750,
                    },
                ]
            ),
            Err(ResponseEventPersistenceError::ConflictingReplay)
        ));
        assert!(require_contiguous_receipt_history("session_ipip_ko_quick", &[]).is_ok());
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
    fn existing_event_lookup_maps_missing_relation_to_database_error() {
        let url = std::env::var("TEST_DATABASE_URL").expect("TEST_DATABASE_URL is required");
        let mut client = Client::connect(&url, NoTls).expect("CI PostgreSQL must be reachable");
        client
            .batch_execute("SET search_path TO response_event_query_helper_missing;")
            .unwrap();
        let event = ResponseEvent::from_persisted(
            "server_event_item_01",
            "client_event_item_01",
            "item_version_n1_ko",
            "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            1,
        )
        .unwrap();
        let mut transaction = client.transaction().unwrap();
        assert!(matches!(
            query_existing_event_row(&mut transaction, "server_event_item_01"),
            Err(ResponseEventPersistenceError::Database(_))
        ));
        let observed_at = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        assert!(matches!(
            classify_existing_event(
                &mut transaction,
                "session_ipip_ko_quick",
                &event,
                "server_event_item_01",
                1,
                observed_at,
                observed_at + Duration::from_millis(250),
            ),
            Err(ResponseEventPersistenceError::Database(_))
        ));
        transaction.rollback().unwrap();
    }

    #[test]
    fn next_contiguous_sequence_instantiates_empty_prefix_and_missing_relation() {
        let url = std::env::var("TEST_DATABASE_URL").expect("TEST_DATABASE_URL is required");
        let mut client = Client::connect(&url, NoTls).expect("CI PostgreSQL must be reachable");
        client
            .batch_execute(
                "CREATE SCHEMA IF NOT EXISTS response_event_next_sequence_test;\
                 SET search_path TO response_event_next_sequence_test;\
                 DROP TABLE IF EXISTS response_event;",
            )
            .unwrap();
        let mut missing = client.transaction().unwrap();
        let missing_error = next_contiguous_sequence(&mut missing, "session_ipip_ko_quick")
            .expect_err("missing relation must fail closed");
        assert!(matches!(
            missing_error,
            ResponseEventPersistenceError::Database(_)
        ));
        assert_eq!(
            missing_error.to_string(),
            "PostgreSQL response-event persistence failed"
        );
        missing.rollback().unwrap();

        apply_response_event_migration(&mut client).unwrap();
        let mut empty = client.transaction().unwrap();
        assert_eq!(
            next_contiguous_sequence(&mut empty, "session_ipip_ko_quick").unwrap(),
            1
        );
        empty
            .execute(
                "INSERT INTO response_event (\
                     response_event_ref, session_ref, client_event_ref, item_version_ref, \
                     payload_digest, server_sequence, observed_at, received_at\
                 ) VALUES (\
                     'server_event_item_01', 'session_ipip_ko_quick', 'client_event_item_01', \
                     'item_version_n1_ko', \
                     'sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', \
                     1, TIMESTAMPTZ '2023-11-14 22:13:20+00', TIMESTAMPTZ '2023-11-14 22:13:20.250+00'\
                 )",
                &[],
            )
            .unwrap();
        assert_eq!(
            next_contiguous_sequence(&mut empty, "session_ipip_ko_quick").unwrap(),
            2
        );
        empty.rollback().unwrap();
    }

    #[test]
    fn persist_and_load_instantiate_library_copies_on_missing_relation() {
        let url = std::env::var("TEST_DATABASE_URL").expect("TEST_DATABASE_URL is required");
        let mut client = Client::connect(&url, NoTls).expect("CI PostgreSQL must be reachable");
        client
            .batch_execute("SET search_path TO response_event_persist_load_missing;")
            .unwrap();
        let event = ResponseEvent::from_persisted(
            "server_event_item_01",
            "client_event_item_01",
            "item_version_n1_ko",
            "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            1,
        )
        .unwrap();
        let mut transaction = client.transaction().unwrap();
        assert!(matches!(
            persist_response_event(
                &mut transaction,
                "session_ipip_ko_quick",
                &event,
                1_700_000_000_000,
                1_700_000_000_250,
            ),
            Err(ResponseEventPersistenceError::Database(_))
        ));
        assert!(matches!(
            load_response_event_receipts(&mut transaction, "session_ipip_ko_quick"),
            Err(ResponseEventPersistenceError::Database(_))
        ));
        assert!(matches!(
            load_response_ledger(&mut transaction, "session_ipip_ko_quick"),
            Err(ResponseEventPersistenceError::Database(_))
        ));
        transaction.rollback().unwrap();
    }

    #[test]
    fn persist_and_load_instantiate_unsupported_isolation_in_the_library() {
        let url = std::env::var("TEST_DATABASE_URL").expect("TEST_DATABASE_URL is required");
        let mut client = Client::connect(&url, NoTls).expect("CI PostgreSQL must be reachable");
        client
            .batch_execute(
                "CREATE SCHEMA IF NOT EXISTS response_event_isolation_lib_test;\
                 SET search_path TO response_event_isolation_lib_test;",
            )
            .unwrap();
        apply_response_event_migration(&mut client).unwrap();
        let event = ResponseEvent::from_persisted(
            "server_event_item_iso",
            "client_event_item_iso",
            "item_version_n1_ko",
            "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            1,
        )
        .unwrap();
        let mut serializable = client
            .build_transaction()
            .isolation_level(IsolationLevel::RepeatableRead)
            .start()
            .unwrap();
        let persist_error = persist_response_event(
            &mut serializable,
            "session_ipip_ko_iso",
            &event,
            1_700_000_000_000,
            1_700_000_000_250,
        )
        .expect_err("lib persist must reject stronger isolation");
        assert!(matches!(
            persist_error,
            ResponseEventPersistenceError::UnsupportedIsolationLevel
        ));
        assert_eq!(
            persist_error.to_string(),
            "response event persistence requires read committed isolation"
        );
        let load_error = load_response_event_receipts(&mut serializable, "session_ipip_ko_iso")
            .expect_err("lib load must reject stronger isolation");
        assert!(matches!(
            load_error,
            ResponseEventPersistenceError::UnsupportedIsolationLevel
        ));
        serializable.rollback().unwrap();
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
