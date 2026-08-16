//! `PostgreSQL` 18 persistence for immutable response snapshots.
//!
//! This adapter stores the frozen accepted-response prefix after collection
//! completes. It does not store response bodies and does not recompute scores.
//! The caller owns the connection, credentials, and transaction boundary.
//! Replay requires `READ COMMITTED` so a concurrent insert that wins a unique-key
//! race is visible to the exact-replay classifier.

use crate::reference::normalized_reference;
use crate::response::ResponseSnapshot;
use postgres::Transaction;
use std::error::Error;
use std::fmt::{Display, Formatter};

const RESPONSE_SNAPSHOT_MIGRATION: &str = include_str!("../migrations/0010_response_snapshot.sql");

/// Outcome of persisting one immutable response snapshot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ResponseSnapshotPersistenceDisposition {
    /// A new snapshot header and its entries were inserted.
    Inserted,
    /// The same immutable snapshot identity and entries already existed.
    Duplicate,
}

/// Fail-closed error for durable response-snapshot persistence.
#[derive(Debug)]
#[non_exhaustive]
pub enum ResponseSnapshotPersistenceError {
    /// A snapshot or session identity was not an exact safe opaque durable reference.
    InvalidReference,
    /// Snapshot identity was replayed with different immutable evidence.
    ConflictingReplay,
    /// A sequence cannot be represented by the bounded database column.
    InvalidSequence,
    /// Response-snapshot persistence requires `PostgreSQL` `READ COMMITTED` isolation.
    UnsupportedIsolationLevel,
    /// `PostgreSQL` rejected or could not execute the persistence operation.
    Database(postgres::Error),
}

impl Display for ResponseSnapshotPersistenceError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidReference => {
                "response snapshot persistence references must be exact safe opaque durable values"
            }
            Self::ConflictingReplay => {
                "response snapshot identity was replayed with conflicting evidence"
            }
            Self::InvalidSequence => {
                "response snapshot sequence exceeds the PostgreSQL bigint range"
            }
            Self::UnsupportedIsolationLevel => {
                "response snapshot persistence requires read committed isolation"
            }
            Self::Database(_) => "PostgreSQL response-snapshot persistence failed",
        })
    }
}

impl Error for ResponseSnapshotPersistenceError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Database(error) => Some(error),
            _ => None,
        }
    }
}

impl From<postgres::Error> for ResponseSnapshotPersistenceError {
    fn from(error: postgres::Error) -> Self {
        Self::Database(error)
    }
}

/// Apply the idempotent response-snapshot migration to a `PostgreSQL` connection.
///
/// # Errors
///
/// Returns the `PostgreSQL` error if the migration cannot be applied.
pub fn apply_response_snapshot_migration(
    client: &mut impl postgres::GenericClient,
) -> Result<(), postgres::Error> {
    client.batch_execute(RESPONSE_SNAPSHOT_MIGRATION)
}

/// Persist one immutable response snapshot frozen after collection completes.
///
/// Exact replay of the same snapshot identity, session binding, and ordered
/// entries is idempotent. Rebinding `snapshot_ref` to a different session or
/// entry set fails closed. Historical snapshots are never updated.
///
/// # Errors
///
/// Returns [`ResponseSnapshotPersistenceError`] for an unbound snapshot,
/// unsupported isolation, conflicting replay, an invalid sequence, or a
/// database failure.
pub fn persist_response_snapshot(
    transaction: &mut Transaction<'_>,
    snapshot: &ResponseSnapshot,
) -> Result<ResponseSnapshotPersistenceDisposition, ResponseSnapshotPersistenceError> {
    require_read_committed(transaction)?;
    let snapshot_ref = snapshot
        .snapshot_ref()
        .ok_or(ResponseSnapshotPersistenceError::InvalidReference)?;
    let session_ref = required_reference(snapshot.session_ref())?;
    let event_count = postgres_sequence(snapshot.event_count())?;
    let last_sequence = snapshot
        .last_sequence()
        .map(postgres_sequence)
        .transpose()?;

    let inserted = transaction.execute(
        "INSERT INTO response_snapshot (\
             snapshot_ref, session_ref, event_count, last_sequence\
         ) VALUES ($1, $2, $3, $4) \
         ON CONFLICT (snapshot_ref) DO NOTHING",
        &[&snapshot_ref, &session_ref, &event_count, &last_sequence],
    )?;
    if inserted == 1 {
        insert_entries(transaction, snapshot_ref, snapshot)?;
        return Ok(ResponseSnapshotPersistenceDisposition::Inserted);
    }
    classify_existing_snapshot(transaction, snapshot, snapshot_ref, session_ref)
}

fn insert_entries(
    transaction: &mut Transaction<'_>,
    snapshot_ref: &str,
    snapshot: &ResponseSnapshot,
) -> Result<(), ResponseSnapshotPersistenceError> {
    for (index, ((event_ref, item_version_ref), payload_digest)) in snapshot
        .event_refs()
        .iter()
        .zip(snapshot.item_version_refs())
        .zip(snapshot.payload_digests())
        .enumerate()
    {
        let snapshot_sequence = postgres_sequence(index + 1)?;
        transaction.execute(
            "INSERT INTO response_snapshot_entry (\
                 snapshot_ref, snapshot_sequence, event_ref, item_version_ref, payload_digest\
             ) VALUES ($1, $2, $3, $4, $5)",
            &[
                &snapshot_ref,
                &snapshot_sequence,
                &event_ref.as_str(),
                &item_version_ref.as_str(),
                &payload_digest.as_str(),
            ],
        )?;
    }
    Ok(())
}

fn classify_existing_snapshot(
    transaction: &mut Transaction<'_>,
    snapshot: &ResponseSnapshot,
    snapshot_ref: &str,
    session_ref: &str,
) -> Result<ResponseSnapshotPersistenceDisposition, ResponseSnapshotPersistenceError> {
    let row = transaction.query_one(
        "SELECT session_ref, event_count, last_sequence \
         FROM response_snapshot WHERE snapshot_ref = $1",
        &[&snapshot_ref],
    )?;
    let stored_session: String = row.get(0);
    let stored_count: i64 = row.get(1);
    let stored_last: Option<i64> = row.get(2);
    let expected_count = postgres_sequence(snapshot.event_count())?;
    let expected_last = snapshot
        .last_sequence()
        .map(postgres_sequence)
        .transpose()?;
    if stored_session != session_ref
        || stored_count != expected_count
        || stored_last != expected_last
    {
        return Err(ResponseSnapshotPersistenceError::ConflictingReplay);
    }

    let rows = transaction.query(
        "SELECT event_ref, item_version_ref, payload_digest \
         FROM response_snapshot_entry \
         WHERE snapshot_ref = $1 \
         ORDER BY snapshot_sequence",
        &[&snapshot_ref],
    )?;
    let stored: Vec<(String, String, String)> = rows
        .into_iter()
        .map(|entry_row| (entry_row.get(0), entry_row.get(1), entry_row.get(2)))
        .collect();
    let incoming: Vec<(String, String, String)> = snapshot
        .event_refs()
        .iter()
        .zip(snapshot.item_version_refs())
        .zip(snapshot.payload_digests())
        .map(|((event_ref, item_version_ref), payload_digest)| {
            (
                event_ref.clone(),
                item_version_ref.clone(),
                payload_digest.clone(),
            )
        })
        .collect();
    if stored == incoming {
        Ok(ResponseSnapshotPersistenceDisposition::Duplicate)
    } else {
        Err(ResponseSnapshotPersistenceError::ConflictingReplay)
    }
}

fn required_reference(reference: &str) -> Result<&str, ResponseSnapshotPersistenceError> {
    normalized_reference(reference).ok_or(ResponseSnapshotPersistenceError::InvalidReference)
}

fn postgres_sequence(value: usize) -> Result<i64, ResponseSnapshotPersistenceError> {
    i64::try_from(value).map_err(|_| ResponseSnapshotPersistenceError::InvalidSequence)
}

fn require_read_committed(
    transaction: &mut Transaction<'_>,
) -> Result<(), ResponseSnapshotPersistenceError> {
    let row = transaction.query_one("SHOW transaction_isolation", &[])?;
    let isolation: String = row.get(0);
    if isolation == "read committed" {
        Ok(())
    } else {
        Err(ResponseSnapshotPersistenceError::UnsupportedIsolationLevel)
    }
}

#[cfg(test)]
mod reference_guard_tests {
    use super::{postgres_sequence, required_reference, ResponseSnapshotPersistenceError};

    #[test]
    fn noncanonical_references_and_overflow_sequences_fail_closed() {
        for invalid_reference in [
            " ",
            "12",
            " response_snapshot_ko_v1",
            "response_snapshot_ko_v1 ",
            "response\n_snapshot_ko_v1",
            "response\u{200b}snapshot_ko_v1",
            "response\u{202e}snapshot_ko_v1",
        ] {
            assert!(matches!(
                required_reference(invalid_reference),
                Err(ResponseSnapshotPersistenceError::InvalidReference)
            ));
        }
        assert_eq!(
            required_reference("response_snapshot_ko_v1").unwrap(),
            "response_snapshot_ko_v1"
        );
        assert_eq!(postgres_sequence(0).unwrap(), 0);
        assert_eq!(postgres_sequence(3).unwrap(), 3);
        assert!(matches!(
            postgres_sequence(usize::MAX),
            Err(ResponseSnapshotPersistenceError::InvalidSequence)
        ));
    }
}