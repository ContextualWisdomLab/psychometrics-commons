//! `PostgreSQL` 18 persistence for immutable response snapshots.
//!
//! This adapter stores and reloads the frozen accepted-response prefix after
//! collection completes. It does not store response bodies and does not
//! recompute scores. The caller owns the connection, credentials, and
//! transaction boundary. Persist replay and restart reload require
//! `READ COMMITTED` so a concurrent insert that wins a unique-key race is
//! visible to the classifier and so a later load sees the committed prefix.

use crate::reference::normalized_reference;
use crate::response::{ResponseSnapshot, ResponseSnapshotEntryInput, WriteError};
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
    /// A snapshot or session identity was blank, numeric-like, or unbound.
    InvalidReference,
    /// Snapshot identity was replayed with different immutable evidence.
    ConflictingReplay,
    /// A sequence cannot be represented by the bounded database column.
    InvalidSequence,
    /// Response-snapshot persistence requires `PostgreSQL` `READ COMMITTED` isolation.
    UnsupportedIsolationLevel,
    /// Stored header and entry rows cannot reconstruct a valid frozen snapshot.
    CorruptHistory,
    /// `PostgreSQL` rejected or could not execute the persistence operation.
    Database(postgres::Error),
}

impl Display for ResponseSnapshotPersistenceError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidReference => {
                "response snapshot persistence references must be opaque durable values"
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
            Self::CorruptHistory => {
                "stored response snapshot rows cannot reconstruct a valid frozen prefix"
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

/// Reload one immutable response snapshot after process restart.
///
/// The caller owns the `READ COMMITTED` transaction. The load takes `FOR SHARE`
/// on the snapshot header so a concurrent writer holding that row waits until
/// reconstruction finishes. Entries are reconstructed in `snapshot_sequence`
/// order, not by opaque event identity. A missing snapshot is absent. Header
/// counts, gapped sequences, or stored labels that cannot rebuild the freeze
/// fail closed. Historical snapshots are not rewritten.
///
/// # Errors
///
/// Returns [`ResponseSnapshotPersistenceError`] for an invalid reference,
/// unsupported isolation, stored evidence that cannot reconstruct a valid
/// snapshot, a sequence outside the `PostgreSQL` range, or a database failure.
pub fn load_response_snapshot(
    transaction: &mut Transaction<'_>,
    snapshot_ref: &str,
) -> Result<Option<ResponseSnapshot>, ResponseSnapshotPersistenceError> {
    require_read_committed(transaction)?;
    let snapshot_ref = required_reference(snapshot_ref)?;
    load_snapshot_from_header(
        transaction,
        "SELECT snapshot_ref, session_ref, event_count, last_sequence \
         FROM response_snapshot WHERE snapshot_ref = $1 FOR SHARE",
        snapshot_ref,
    )
}

/// Reload the unique frozen snapshot bound to one completed session.
///
/// `response_snapshot.session_ref` is unique. Scoring after restart can
/// therefore recover the completed prefix from the session identity without
/// inventing a second snapshot.
///
/// # Errors
///
/// Returns the same fail-closed errors as [`load_response_snapshot`].
pub fn load_response_snapshot_for_session(
    transaction: &mut Transaction<'_>,
    session_ref: &str,
) -> Result<Option<ResponseSnapshot>, ResponseSnapshotPersistenceError> {
    require_read_committed(transaction)?;
    let session_ref = required_reference(session_ref)?;
    load_snapshot_from_header(
        transaction,
        "SELECT snapshot_ref, session_ref, event_count, last_sequence \
         FROM response_snapshot WHERE session_ref = $1 FOR SHARE",
        session_ref,
    )
}

fn load_snapshot_from_header(
    transaction: &mut Transaction<'_>,
    header_sql: &str,
    identity_ref: &str,
) -> Result<Option<ResponseSnapshot>, ResponseSnapshotPersistenceError> {
    let Some(header) = transaction.query_opt(header_sql, &[&identity_ref])? else {
        return Ok(None);
    };
    let snapshot_ref: String = header.get(0);
    let session_ref: String = header.get(1);
    let event_count = stored_sequence(header.get(2))?;
    let last_sequence = header
        .get::<_, Option<i64>>(3)
        .map(stored_sequence)
        .transpose()?;
    let rows = transaction.query(
        "SELECT snapshot_sequence, event_ref, item_version_ref, payload_digest \
         FROM response_snapshot_entry \
         WHERE snapshot_ref = $1 \
         ORDER BY snapshot_sequence ASC",
        &[&snapshot_ref],
    )?;
    if rows.len() != event_count {
        return Err(ResponseSnapshotPersistenceError::CorruptHistory);
    }
    let mut owned_entries = Vec::with_capacity(rows.len());
    for (index, row) in rows.iter().enumerate() {
        let snapshot_sequence = stored_sequence(row.get(0))?;
        if snapshot_sequence != index + 1 {
            return Err(ResponseSnapshotPersistenceError::CorruptHistory);
        }
        let event_ref: String = row.get(1);
        let item_version_ref: String = row.get(2);
        let payload_digest: String = row.get(3);
        owned_entries.push((event_ref, item_version_ref, payload_digest));
    }
    let entries: Vec<ResponseSnapshotEntryInput<'_>> = owned_entries
        .iter()
        .map(
            |(event_ref, item_version_ref, payload_digest)| ResponseSnapshotEntryInput {
                event_ref,
                item_version_ref,
                payload_digest,
            },
        )
        .collect();
    ResponseSnapshot::from_persisted(&snapshot_ref, &session_ref, &entries, last_sequence)
        .map(Some)
        .map_err(map_reconstruct_error)
}

fn stored_sequence(value: i64) -> Result<usize, ResponseSnapshotPersistenceError> {
    usize::try_from(value).map_err(|_| ResponseSnapshotPersistenceError::InvalidSequence)
}

fn map_reconstruct_error(error: WriteError) -> ResponseSnapshotPersistenceError {
    match error {
        WriteError::InvalidReference => ResponseSnapshotPersistenceError::InvalidReference,
        WriteError::EmptyReference
        | WriteError::InvalidPayloadDigest
        | WriteError::CorruptSnapshotEvidence => ResponseSnapshotPersistenceError::CorruptHistory,
        WriteError::SessionNotActive(_)
        | WriteError::IdempotencyConflict
        | WriteError::ServerReferenceConflict
        | WriteError::SnapshotRequiresCompleted(_) => {
            ResponseSnapshotPersistenceError::CorruptHistory
        }
    }
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
    use super::{
        map_reconstruct_error, postgres_sequence, required_reference, stored_sequence,
        ResponseSnapshotPersistenceError,
    };
    use crate::response::WriteError;
    use crate::session::SessionState;

    #[test]
    fn blank_numeric_and_overflow_sequences_fail_closed() {
        assert!(matches!(
            required_reference(" "),
            Err(ResponseSnapshotPersistenceError::InvalidReference)
        ));
        assert!(matches!(
            required_reference("12"),
            Err(ResponseSnapshotPersistenceError::InvalidReference)
        ));
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
        assert_eq!(stored_sequence(0).unwrap(), 0);
        assert_eq!(stored_sequence(3).unwrap(), 3);
        assert!(matches!(
            stored_sequence(-1),
            Err(ResponseSnapshotPersistenceError::InvalidSequence)
        ));
    }

    #[test]
    fn reconstruct_errors_map_to_fail_closed_persistence_errors() {
        assert!(matches!(
            map_reconstruct_error(WriteError::InvalidReference),
            ResponseSnapshotPersistenceError::InvalidReference
        ));
        for error in [
            WriteError::EmptyReference,
            WriteError::InvalidPayloadDigest,
            WriteError::CorruptSnapshotEvidence,
            WriteError::SessionNotActive(SessionState::Paused),
            WriteError::IdempotencyConflict,
            WriteError::ServerReferenceConflict,
            WriteError::SnapshotRequiresCompleted(SessionState::Active),
        ] {
            assert!(
                matches!(
                    map_reconstruct_error(error),
                    ResponseSnapshotPersistenceError::CorruptHistory
                ),
                "{error} must fail closed as corrupt stored history"
            );
        }
    }
}
