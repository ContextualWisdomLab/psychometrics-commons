//! `PostgreSQL` 18 persistence for anonymous assessment participants.
//!
//! This adapter stores the product-owned participant identity that later
//! anonymous session commands must load. Tenant lives on this row, not on the
//! session aggregate. Identity-link history remains a later slice. The caller
//! owns the connection, credentials, and transaction boundary. Replay requires
//! `READ COMMITTED` so a concurrent insert that wins a unique-key race is
//! visible to the exact-replay classifier.

use crate::participant::ParticipantRecord;
use crate::reference::normalized_reference;
use postgres::{GenericClient, Transaction};
use std::error::Error;
use std::fmt::{Display, Formatter};

const ASSESSMENT_PARTICIPANT_MIGRATION: &str =
    include_str!("../migrations/0021_assessment_participant.sql");

/// Outcome of persisting one anonymous assessment participant.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ParticipantPersistenceDisposition {
    /// A new anonymous participant row was inserted.
    Inserted,
    /// The same immutable participant identity already existed.
    Duplicate,
}

/// Fail-closed error for durable assessment-participant persistence.
#[derive(Debug)]
#[non_exhaustive]
pub enum ParticipantPersistenceError {
    /// A participant or tenant identity was blank or numeric-like.
    InvalidReference,
    /// Participant identity was replayed with different immutable evidence.
    ConflictingReplay,
    /// A timestamp cannot be represented by the bounded database column.
    InvalidTimestamp,
    /// Assessment-participant persistence requires `PostgreSQL` `READ COMMITTED` isolation.
    UnsupportedIsolationLevel,
    /// The participant currently carries account-link evidence this slice does not store.
    IdentityLinkOutOfScope,
    /// No participant row exists for the requested tenant and participant reference.
    NotFound,
    /// `PostgreSQL` rejected or could not execute the persistence operation.
    Database(postgres::Error),
}

impl Display for ParticipantPersistenceError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidReference => {
                "assessment participant persistence references must be opaque values"
            }
            Self::ConflictingReplay => {
                "assessment participant identity was replayed with conflicting evidence"
            }
            Self::InvalidTimestamp => {
                "assessment participant timestamp exceeds the PostgreSQL bigint range"
            }
            Self::UnsupportedIsolationLevel => {
                "assessment participant persistence requires read committed isolation"
            }
            Self::IdentityLinkOutOfScope => {
                "assessment participant persistence stores anonymous identity only"
            }
            Self::NotFound => "assessment participant was not found for the requested tenant",
            Self::Database(_) => "PostgreSQL assessment-participant persistence failed",
        })
    }
}

impl Error for ParticipantPersistenceError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Database(error) => Some(error),
            Self::InvalidReference
            | Self::ConflictingReplay
            | Self::InvalidTimestamp
            | Self::UnsupportedIsolationLevel
            | Self::IdentityLinkOutOfScope
            | Self::NotFound => None,
        }
    }
}

impl From<postgres::Error> for ParticipantPersistenceError {
    fn from(error: postgres::Error) -> Self {
        Self::Database(error)
    }
}

/// Apply the idempotent assessment-participant migration to a `PostgreSQL` connection.
///
/// # Errors
///
/// Returns the `PostgreSQL` error if the migration cannot be applied.
pub fn apply_assessment_participant_migration(
    client: &mut impl GenericClient,
) -> Result<(), postgres::Error> {
    client.batch_execute(ASSESSMENT_PARTICIPANT_MIGRATION)
}

/// Persist one anonymous participant identity.
///
/// Exact replay of the same participant reference, tenant, anonymous status, and
/// creation time is idempotent. Rebinding that reference to another tenant or
/// creation time fails closed. Linked participants are rejected so this slice
/// cannot silently drop account-link evidence.
///
/// # Errors
///
/// Returns [`ParticipantPersistenceError`] for unsupported isolation, a linked
/// participant, conflicting replay, an invalid timestamp, or a database failure.
pub fn persist_assessment_participant(
    transaction: &mut Transaction<'_>,
    participant: &ParticipantRecord,
) -> Result<ParticipantPersistenceDisposition, ParticipantPersistenceError> {
    require_read_committed(transaction)?;
    if participant.linked_subject_ref().is_some() || !participant.link_history().is_empty() {
        return Err(ParticipantPersistenceError::IdentityLinkOutOfScope);
    }
    let participant_ref = required_reference(participant.participant_ref())?;
    let tenant_ref = required_reference(participant.tenant_ref())?;
    let created_at = postgres_timestamp(participant.created_at_unix_ms())?;
    let inserted = transaction.execute(
        "INSERT INTO assessment_participant (\
             participant_ref, tenant_ref, participant_status, created_at_unix_ms\
         ) VALUES ($1, $2, 'anonymous', $3) \
         ON CONFLICT (participant_ref) DO NOTHING",
        &[&participant_ref, &tenant_ref, &created_at],
    )?;
    if inserted == 1 {
        return Ok(ParticipantPersistenceDisposition::Inserted);
    }
    classify_existing_participant(transaction, participant_ref, tenant_ref, created_at)
}

/// Load one anonymous participant by exact tenant and participant reference.
///
/// The tenant comes from the stored row. A proof that names another tenant
/// cannot reconstruct this participant.
///
/// # Errors
///
/// Returns [`ParticipantPersistenceError::InvalidReference`] for a blank or
/// numeric-like identity, [`ParticipantPersistenceError::NotFound`] when no row
/// matches both references, or a database failure.
pub fn load_assessment_participant(
    client: &mut impl GenericClient,
    tenant_ref: &str,
    participant_ref: &str,
) -> Result<ParticipantRecord, ParticipantPersistenceError> {
    let tenant_ref = required_reference(tenant_ref)?;
    let participant_ref = required_reference(participant_ref)?;
    let row = client.query_opt(
        "SELECT participant_ref, tenant_ref, created_at_unix_ms \
         FROM assessment_participant \
         WHERE tenant_ref = $1 AND participant_ref = $2 \
           AND participant_status = 'anonymous'",
        &[&tenant_ref, &participant_ref],
    )?;
    let Some(row) = row else {
        return Err(ParticipantPersistenceError::NotFound);
    };
    let stored_participant_ref: String = row.get(0);
    let stored_tenant_ref: String = row.get(1);
    let created_at: i64 = row.get(2);
    let created_at =
        u64::try_from(created_at).map_err(|_| ParticipantPersistenceError::InvalidTimestamp)?;
    ParticipantRecord::new_anonymous(&stored_participant_ref, &stored_tenant_ref, created_at)
        .map_err(|_| ParticipantPersistenceError::InvalidTimestamp)
}

fn classify_existing_participant(
    transaction: &mut Transaction<'_>,
    participant_ref: &str,
    tenant_ref: &str,
    created_at: i64,
) -> Result<ParticipantPersistenceDisposition, ParticipantPersistenceError> {
    let row = transaction.query_one(
        "SELECT tenant_ref, created_at_unix_ms, participant_status \
         FROM assessment_participant \
         WHERE participant_ref = $1",
        &[&participant_ref],
    )?;
    let stored_tenant: String = row.get(0);
    let stored_created_at: i64 = row.get(1);
    let stored_status: String = row.get(2);
    if stored_tenant == tenant_ref
        && stored_created_at == created_at
        && stored_status == "anonymous"
    {
        Ok(ParticipantPersistenceDisposition::Duplicate)
    } else {
        Err(ParticipantPersistenceError::ConflictingReplay)
    }
}

fn required_reference(reference: &str) -> Result<&str, ParticipantPersistenceError> {
    normalized_reference(reference).ok_or(ParticipantPersistenceError::InvalidReference)
}

fn postgres_timestamp(timestamp: u64) -> Result<i64, ParticipantPersistenceError> {
    i64::try_from(timestamp).map_err(|_| ParticipantPersistenceError::InvalidTimestamp)
}

fn require_read_committed(
    transaction: &mut Transaction<'_>,
) -> Result<(), ParticipantPersistenceError> {
    let row = transaction.query_one("SHOW transaction_isolation", &[])?;
    let isolation: String = row.get(0);
    if isolation == "read committed" {
        Ok(())
    } else {
        Err(ParticipantPersistenceError::UnsupportedIsolationLevel)
    }
}

#[cfg(test)]
mod reference_guard_tests {
    use super::{postgres_timestamp, required_reference, ParticipantPersistenceError};

    #[test]
    fn blank_numeric_and_overflow_values_are_classified() {
        assert!(matches!(
            required_reference(" "),
            Err(ParticipantPersistenceError::InvalidReference)
        ));
        assert!(matches!(
            required_reference("12"),
            Err(ParticipantPersistenceError::InvalidReference)
        ));
        assert_eq!(
            required_reference("participant_persist_unit").unwrap(),
            "participant_persist_unit"
        );
        assert!(matches!(
            postgres_timestamp(u64::MAX),
            Err(ParticipantPersistenceError::InvalidTimestamp)
        ));
        assert_eq!(postgres_timestamp(70_000).unwrap(), 70_000);
    }
}
