//! `PostgreSQL` 18 persistence for idempotent response-event evidence.
//!
//! This adapter stores product-owned response identity and payload digests
//! only. It does not store response bodies. The caller owns the connection,
//! credentials, and transaction boundary. Replay requires `READ COMMITTED` so a
//! concurrent insert that wins a unique-key race is visible to the exact-replay
//! classifier.

use crate::reference::normalized_reference;
use crate::response::{ResponseEvent, ResponseLedger};
use postgres::Transaction;
use std::error::Error;
use std::fmt::{Display, Formatter};

const RESPONSE_EVENT_MIGRATION: &str = include_str!("../migrations/0009_response_event.sql");
const CLIENT_EVENT_UNIQUE_CONSTRAINT: &str = "response_event_client_event_unique";
const SERVER_SEQUENCE_UNIQUE_CONSTRAINT: &str = "response_event_server_sequence_unique";

/// Outcome of persisting one response-event ledger snapshot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ResponsePersistenceDisposition {
    /// At least one new ledger or event row was inserted.
    Inserted,
    /// The same immutable ledger and event evidence already existed.
    Duplicate,
}

/// Fail-closed error for durable response-event persistence.
#[derive(Debug)]
#[non_exhaustive]
pub enum ResponsePersistenceError {
    /// A session, event, or item identity was blank or numeric-like.
    InvalidReference,
    /// Event identity was replayed with different immutable evidence.
    ConflictingReplay,
    /// A sequence cannot be represented by the bounded database column.
    InvalidSequence,
    /// Response-event persistence requires `PostgreSQL` `READ COMMITTED` isolation.
    UnsupportedIsolationLevel,
    /// `PostgreSQL` rejected or could not execute the persistence operation.
    Database(postgres::Error),
}

impl Display for ResponsePersistenceError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidReference => "response persistence references must be opaque values",
            Self::ConflictingReplay => {
                "response event identity was replayed with conflicting evidence"
            }
            Self::InvalidSequence => "response event sequence exceeds the PostgreSQL bigint range",
            Self::UnsupportedIsolationLevel => {
                "response persistence requires read committed isolation"
            }
            Self::Database(_) => "PostgreSQL response-event persistence failed",
        })
    }
}

impl Error for ResponsePersistenceError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Database(error) => Some(error),
            _ => None,
        }
    }
}

impl From<postgres::Error> for ResponsePersistenceError {
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

/// Persist one session-bound response ledger and its accepted events.
///
/// Exact replay of the same session and event evidence is idempotent. Reusing a
/// server or client event identity with a different item, digest, or sequence
/// fails closed. New events append without rewriting earlier evidence.
///
/// # Errors
///
/// Returns [`ResponsePersistenceError`] for unsupported isolation, conflicting
/// replay, an invalid reference or sequence, or a database failure.
pub fn persist_response_ledger(
    transaction: &mut Transaction<'_>,
    ledger: &ResponseLedger,
) -> Result<ResponsePersistenceDisposition, ResponsePersistenceError> {
    require_read_committed(transaction)?;
    let session_ref = required_reference(ledger.session_ref())?;
    let mut inserted_any = persist_ledger_header(transaction, session_ref)?;
    for event in ledger.events() {
        if persist_one_event(transaction, session_ref, event)? {
            inserted_any = true;
        }
    }
    if inserted_any {
        Ok(ResponsePersistenceDisposition::Inserted)
    } else {
        Ok(ResponsePersistenceDisposition::Duplicate)
    }
}

fn persist_ledger_header(
    transaction: &mut Transaction<'_>,
    session_ref: &str,
) -> Result<bool, ResponsePersistenceError> {
    let inserted = transaction.execute(
        "INSERT INTO response_event_ledger (session_ref) VALUES ($1) \
         ON CONFLICT (session_ref) DO NOTHING",
        &[&session_ref],
    )?;
    Ok(inserted == 1)
}

fn persist_one_event(
    transaction: &mut Transaction<'_>,
    session_ref: &str,
    event: &ResponseEvent,
) -> Result<bool, ResponsePersistenceError> {
    let sequence = postgres_sequence(event.sequence())?;
    let inserted = match transaction.execute(
        "INSERT INTO response_event (\
             session_ref, server_event_ref, client_event_ref, item_version_ref, \
             payload_digest, server_sequence\
         ) VALUES ($1, $2, $3, $4, $5, $6) \
         ON CONFLICT (session_ref, server_event_ref) DO NOTHING",
        &[
            &session_ref,
            &event.server_event_ref(),
            &event.client_event_ref(),
            &event.item_version_ref(),
            &event.payload_digest(),
            &sequence,
        ],
    ) {
        Ok(inserted) => inserted,
        Err(error) if is_response_uniqueness_conflict(&error) => {
            return Err(ResponsePersistenceError::ConflictingReplay);
        }
        Err(error) => return Err(ResponsePersistenceError::Database(error)),
    };
    if inserted == 1 {
        return Ok(true);
    }

    let row = transaction.query_one(
        "SELECT client_event_ref, item_version_ref, payload_digest, server_sequence \
         FROM response_event WHERE session_ref = $1 AND server_event_ref = $2",
        &[&session_ref, &event.server_event_ref()],
    )?;
    let stored_client: String = row.get(0);
    let stored_item: String = row.get(1);
    let stored_digest: String = row.get(2);
    let stored_sequence: i64 = row.get(3);
    if stored_client == event.client_event_ref()
        && stored_item == event.item_version_ref()
        && stored_digest == event.payload_digest()
        && stored_sequence == sequence
    {
        Ok(false)
    } else {
        Err(ResponsePersistenceError::ConflictingReplay)
    }
}

fn is_response_uniqueness_conflict(error: &postgres::Error) -> bool {
    error.code() == Some(&postgres::error::SqlState::UNIQUE_VIOLATION)
        && matches!(
            error
                .as_db_error()
                .and_then(postgres::error::DbError::constraint),
            Some(CLIENT_EVENT_UNIQUE_CONSTRAINT | SERVER_SEQUENCE_UNIQUE_CONSTRAINT)
        )
}

fn postgres_sequence(sequence: usize) -> Result<i64, ResponsePersistenceError> {
    i64::try_from(sequence).map_err(|_| ResponsePersistenceError::InvalidSequence)
}

fn required_reference(reference: &str) -> Result<&str, ResponsePersistenceError> {
    normalized_reference(reference).ok_or(ResponsePersistenceError::InvalidReference)
}

fn require_read_committed(
    transaction: &mut Transaction<'_>,
) -> Result<(), ResponsePersistenceError> {
    let row = transaction.query_one("SHOW transaction_isolation", &[])?;
    let isolation: String = row.get(0);
    if isolation == "read committed" {
        Ok(())
    } else {
        Err(ResponsePersistenceError::UnsupportedIsolationLevel)
    }
}

#[cfg(test)]
mod reference_guard_tests {
    use super::{
        is_response_uniqueness_conflict, postgres_sequence, required_reference,
        ResponsePersistenceError,
    };

    #[test]
    fn blank_and_numeric_references_fail_closed() {
        assert!(matches!(
            required_reference(" "),
            Err(ResponsePersistenceError::InvalidReference)
        ));
        assert!(matches!(
            required_reference("12"),
            Err(ResponsePersistenceError::InvalidReference)
        ));
        assert_eq!(
            required_reference("session_response_alpha").unwrap(),
            "session_response_alpha"
        );
        assert!(matches!(
            postgres_sequence(usize::MAX),
            Err(ResponsePersistenceError::InvalidSequence)
        ));
        assert_eq!(postgres_sequence(1).unwrap(), 1);
    }

    #[test]
    fn non_database_postgres_error_is_not_a_uniqueness_conflict() {
        let error = postgres::Client::connect(
            "host=127.0.0.1 port=1 user=x dbname=x connect_timeout=1",
            postgres::NoTls,
        )
        .err()
        .expect("a closed loopback port must fail without a PostgreSQL database error");
        assert!(!is_response_uniqueness_conflict(&error));
    }
}
