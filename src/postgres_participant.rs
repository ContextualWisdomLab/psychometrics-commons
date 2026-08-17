//! `PostgreSQL` 18 persistence for the anonymous-first participant base record.
//!
//! Psychometrics Commons owns the stable participant reference and tenant binding. This adapter
//! deliberately persists only that base identity and its server-authoritative creation time.
//! Optional Keyverse account-link history is a separate append-only concern and must be composed
//! from its own durable evidence before account-scoped authorization. Raw credentials, Keyverse
//! subjects, assessment responses, and research linkage identifiers never belong in this table.
//!
//! Exact replay requires `READ COMMITTED`: when concurrent creators race on the same participant
//! reference, the loser must observe the winning row before deciding whether the replay is exact.

use crate::participant::ParticipantRecord;
use crate::reference::normalized_reference;
use postgres::Transaction;
use std::error::Error;
use std::fmt::{Display, Formatter};

const PARTICIPANT_BASE_MIGRATION: &str =
    include_str!("../migrations/0030_assessment_participant.sql");

/// Outcome of persisting one anonymous participant base record.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ParticipantBasePersistenceDisposition {
    /// A new base identity row was inserted.
    Inserted,
    /// The exact same participant, tenant, and creation time were already stored.
    Duplicate,
}

/// Fail-closed error for participant base persistence and reload.
#[derive(Debug)]
#[non_exhaustive]
pub enum ParticipantBasePersistenceError {
    /// A participant or tenant reference was blank, numeric-like, or not in exact issued spelling.
    InvalidReference,
    /// The participant creation timestamp was zero or outside the `PostgreSQL` `BIGINT` range.
    InvalidTimestamp,
    /// A linked or historically linked record was passed to this base-only persistence boundary.
    LinkedRecordRequiresIdentityHistory,
    /// The participant reference was replayed with a different tenant or creation time.
    ConflictingReplay,
    /// Stored participant identity evidence could not be reconstructed without normalization.
    CorruptStoredIdentity,
    /// Participant base persistence requires `PostgreSQL` `READ COMMITTED` isolation.
    UnsupportedIsolationLevel,
    /// `PostgreSQL` rejected or could not execute the persistence operation.
    Database(postgres::Error),
}

impl Display for ParticipantBasePersistenceError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidReference => {
                "participant persistence references must use exact opaque non-numeric spelling"
            }
            Self::InvalidTimestamp => {
                "participant creation time must be positive and fit PostgreSQL bigint"
            }
            Self::LinkedRecordRequiresIdentityHistory => {
                "linked participant state must be reconstructed with durable identity-link history"
            }
            Self::ConflictingReplay => {
                "participant identity was replayed with a different tenant or creation time"
            }
            Self::CorruptStoredIdentity => {
                "stored participant identity cannot be reconstructed without changing evidence"
            }
            Self::UnsupportedIsolationLevel => {
                "participant base persistence requires read committed isolation"
            }
            Self::Database(_) => "PostgreSQL participant base persistence failed",
        })
    }
}

impl Error for ParticipantBasePersistenceError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Database(error) => Some(error),
            _ => None,
        }
    }
}

impl From<postgres::Error> for ParticipantBasePersistenceError {
    fn from(error: postgres::Error) -> Self {
        Self::Database(error)
    }
}

/// Apply the idempotent participant-base migration to a `PostgreSQL` connection.
///
/// # Errors
///
/// Returns the `PostgreSQL` error if the schema cannot be created.
pub fn apply_participant_base_migration(
    client: &mut impl postgres::GenericClient,
) -> Result<(), postgres::Error> {
    client.batch_execute(PARTICIPANT_BASE_MIGRATION)
}

/// Persist the stable base identity of an anonymous participant.
///
/// This boundary intentionally accepts only a record with no current or historical account link.
/// Call it when the anonymous participant is first created, before any optional Keyverse link is
/// appended. Exact replay is idempotent. Reusing the same `participant_ref` with another tenant or
/// creation time fails closed and never rewrites the stored row.
///
/// # Errors
///
/// Returns [`ParticipantBasePersistenceError`] for a non-canonical reference, invalid timestamp,
/// linked record, conflicting replay, unsupported transaction isolation, or database failure.
pub fn persist_anonymous_participant_base(
    transaction: &mut Transaction<'_>,
    participant: &ParticipantRecord,
) -> Result<ParticipantBasePersistenceDisposition, ParticipantBasePersistenceError> {
    require_read_committed(transaction)?;
    if !participant.link_history().is_empty()
        || !participant.link_end_history().is_empty()
        || participant.linked_issuer_ref().is_some()
        || participant.linked_subject_ref().is_some()
        || participant.link_event_ref().is_some()
        || participant.anonymous_proof_ref().is_some()
        || participant.authenticated_proof_ref().is_some()
        || participant.linked_at_unix_ms().is_some()
    {
        return Err(ParticipantBasePersistenceError::LinkedRecordRequiresIdentityHistory);
    }

    let participant_ref = required_exact_reference(participant.participant_ref())?;
    let tenant_ref = required_exact_reference(participant.tenant_ref())?;
    let created_at_unix_ms = postgres_timestamp(participant.created_at_unix_ms())?;

    let inserted = transaction.execute(
        "INSERT INTO assessment_participant (participant_ref, tenant_ref, created_at_unix_ms) \
         VALUES ($1, $2, $3) ON CONFLICT (participant_ref) DO NOTHING",
        &[&participant_ref, &tenant_ref, &created_at_unix_ms],
    )?;
    if inserted == 1 {
        return Ok(ParticipantBasePersistenceDisposition::Inserted);
    }

    let existing = read_conflict_winner(transaction, participant_ref)?;
    classify_conflict_winner(
        existing
            .as_ref()
            .map(|(stored_tenant_ref, stored_created_at_unix_ms)| {
                (stored_tenant_ref.as_str(), *stored_created_at_unix_ms)
            }),
        tenant_ref,
        created_at_unix_ms,
    )
}

/// Load one tenant-bound anonymous participant base record.
///
/// The lookup is scoped by both `participant_ref` and `tenant_ref`; another tenant therefore sees
/// absence rather than the participant's existence. The returned record intentionally has no
/// account-link projection. A caller that needs authenticated-account authority must compose the
/// separate durable identity-link history before authorization.
///
/// # Errors
///
/// Returns [`ParticipantBasePersistenceError`] for invalid lookup references, corrupt stored
/// evidence, an out-of-range stored timestamp, or a database failure.
pub fn load_anonymous_participant_base(
    client: &mut impl postgres::GenericClient,
    participant_ref: &str,
    tenant_ref: &str,
) -> Result<Option<ParticipantRecord>, ParticipantBasePersistenceError> {
    let participant_ref = required_exact_reference(participant_ref)?;
    let tenant_ref = required_exact_reference(tenant_ref)?;
    let Some(row) = client.query_opt(
        "SELECT participant_ref, tenant_ref, created_at_unix_ms \
         FROM assessment_participant \
         WHERE participant_ref = $1 AND tenant_ref = $2",
        &[&participant_ref, &tenant_ref],
    )?
    else {
        return Ok(None);
    };

    let stored_participant_ref: String = row.get(0);
    let stored_tenant_ref: String = row.get(1);
    let stored_created_at_unix_ms: i64 = row.get(2);
    reconstruct_loaded_base(
        &stored_participant_ref,
        &stored_tenant_ref,
        stored_created_at_unix_ms,
        participant_ref,
        tenant_ref,
    )
    .map(Some)
}

fn read_conflict_winner(
    transaction: &mut Transaction<'_>,
    participant_ref: &str,
) -> Result<Option<(String, i64)>, ParticipantBasePersistenceError> {
    match transaction.query_opt(
        "SELECT tenant_ref, created_at_unix_ms FROM assessment_participant \
         WHERE participant_ref = $1",
        &[&participant_ref],
    ) {
        Ok(Some(row)) => Ok(Some((row.get(0), row.get(1)))),
        Ok(None) => Ok(None),
        Err(error) => Err(ParticipantBasePersistenceError::from(error)),
    }
}

fn reconstruct_loaded_base(
    stored_participant_ref: &str,
    stored_tenant_ref: &str,
    stored_created_at_unix_ms: i64,
    participant_ref: &str,
    tenant_ref: &str,
) -> Result<crate::participant::ParticipantRecord, ParticipantBasePersistenceError> {
    required_exact_reference(stored_participant_ref)
        .map_err(|_| ParticipantBasePersistenceError::CorruptStoredIdentity)?;
    required_exact_reference(stored_tenant_ref)
        .map_err(|_| ParticipantBasePersistenceError::CorruptStoredIdentity)?;
    require_stored_base_identity(
        stored_participant_ref,
        stored_tenant_ref,
        participant_ref,
        tenant_ref,
    )?;
    let created_at_unix_ms = stored_timestamp(stored_created_at_unix_ms)?;
    crate::participant::ParticipantRecord::new_anonymous(
        stored_participant_ref,
        stored_tenant_ref,
        created_at_unix_ms,
    )
    .map_err(|_| ParticipantBasePersistenceError::CorruptStoredIdentity)
}

fn require_stored_base_identity(
    stored_participant_ref: &str,
    stored_tenant_ref: &str,
    participant_ref: &str,
    tenant_ref: &str,
) -> Result<(), ParticipantBasePersistenceError> {
    if stored_base_identity_matches(
        stored_participant_ref,
        stored_tenant_ref,
        participant_ref,
        tenant_ref,
    ) {
        Ok(())
    } else {
        Err(ParticipantBasePersistenceError::CorruptStoredIdentity)
    }
}

fn classify_conflict_winner(
    existing: Option<(&str, i64)>,
    tenant_ref: &str,
    created_at_unix_ms: i64,
) -> Result<ParticipantBasePersistenceDisposition, ParticipantBasePersistenceError> {
    match existing {
        Some((stored_tenant_ref, stored_created_at_unix_ms))
            if stored_tenant_ref == tenant_ref
                && stored_created_at_unix_ms == created_at_unix_ms =>
        {
            Ok(ParticipantBasePersistenceDisposition::Duplicate)
        }
        Some(_) => Err(ParticipantBasePersistenceError::ConflictingReplay),
        None => Err(ParticipantBasePersistenceError::CorruptStoredIdentity),
    }
}

fn stored_base_identity_matches(
    stored_participant_ref: &str,
    stored_tenant_ref: &str,
    participant_ref: &str,
    tenant_ref: &str,
) -> bool {
    stored_participant_ref == participant_ref && stored_tenant_ref == tenant_ref
}

fn required_exact_reference(reference: &str) -> Result<&str, ParticipantBasePersistenceError> {
    let normalized =
        normalized_reference(reference).ok_or(ParticipantBasePersistenceError::InvalidReference)?;
    if normalized == reference {
        Ok(reference)
    } else {
        Err(ParticipantBasePersistenceError::InvalidReference)
    }
}

fn postgres_timestamp(timestamp: u64) -> Result<i64, ParticipantBasePersistenceError> {
    if timestamp == 0 {
        return Err(ParticipantBasePersistenceError::InvalidTimestamp);
    }
    i64::try_from(timestamp).map_err(|_| ParticipantBasePersistenceError::InvalidTimestamp)
}

fn stored_timestamp(timestamp: i64) -> Result<u64, ParticipantBasePersistenceError> {
    let timestamp = u64::try_from(timestamp)
        .map_err(|_| ParticipantBasePersistenceError::CorruptStoredIdentity)?;
    if timestamp == 0 {
        Err(ParticipantBasePersistenceError::CorruptStoredIdentity)
    } else {
        Ok(timestamp)
    }
}

fn require_read_committed(
    transaction: &mut Transaction<'_>,
) -> Result<(), ParticipantBasePersistenceError> {
    let row = transaction.query_one("SHOW transaction_isolation", &[])?;
    let isolation: String = row.get(0);
    if isolation == "read committed" {
        Ok(())
    } else {
        Err(ParticipantBasePersistenceError::UnsupportedIsolationLevel)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        classify_conflict_winner, postgres_timestamp, read_conflict_winner,
        reconstruct_loaded_base, require_stored_base_identity, required_exact_reference,
        stored_base_identity_matches, stored_timestamp, ParticipantBasePersistenceDisposition,
        ParticipantBasePersistenceError,
    };
    use postgres::{Client, NoTls};

    #[test]
    fn exact_reference_guard_rejects_aliases_and_numeric_values() {
        assert_eq!(
            required_exact_reference("participant_public_demo").unwrap(),
            "participant_public_demo"
        );
        for reference in [
            "",
            " ",
            " participant_public_demo",
            "participant_public_demo ",
            "12",
        ] {
            assert!(matches!(
                required_exact_reference(reference),
                Err(ParticipantBasePersistenceError::InvalidReference)
            ));
        }
    }

    #[test]
    fn timestamp_guards_reject_zero_overflow_and_corrupt_stored_values() {
        assert_eq!(postgres_timestamp(40_000).unwrap(), 40_000);
        assert!(matches!(
            postgres_timestamp(0),
            Err(ParticipantBasePersistenceError::InvalidTimestamp)
        ));
        assert!(matches!(
            postgres_timestamp(u64::MAX),
            Err(ParticipantBasePersistenceError::InvalidTimestamp)
        ));
        assert_eq!(stored_timestamp(40_000).unwrap(), 40_000);
        for timestamp in [0, -1] {
            assert!(matches!(
                stored_timestamp(timestamp),
                Err(ParticipantBasePersistenceError::CorruptStoredIdentity)
            ));
        }
    }

    #[test]
    fn safe_error_copy_does_not_expose_database_details() {
        for (error, expected) in [
            (
                ParticipantBasePersistenceError::InvalidReference,
                "participant persistence references must use exact opaque non-numeric spelling",
            ),
            (
                ParticipantBasePersistenceError::InvalidTimestamp,
                "participant creation time must be positive and fit PostgreSQL bigint",
            ),
            (
                ParticipantBasePersistenceError::LinkedRecordRequiresIdentityHistory,
                "linked participant state must be reconstructed with durable identity-link history",
            ),
            (
                ParticipantBasePersistenceError::ConflictingReplay,
                "participant identity was replayed with a different tenant or creation time",
            ),
            (
                ParticipantBasePersistenceError::CorruptStoredIdentity,
                "stored participant identity cannot be reconstructed without changing evidence",
            ),
            (
                ParticipantBasePersistenceError::UnsupportedIsolationLevel,
                "participant base persistence requires read committed isolation",
            ),
        ] {
            assert_eq!(error.to_string(), expected);
            assert!(std::error::Error::source(&error).is_none());
        }
    }

    #[test]
    fn conflict_winner_and_reload_guards_fail_closed_on_missing_or_rebound_identity() {
        assert_eq!(
            classify_conflict_winner(
                Some(("tenant_public_demo", 40_000)),
                "tenant_public_demo",
                40_000
            )
            .unwrap(),
            ParticipantBasePersistenceDisposition::Duplicate
        );
        assert!(matches!(
            classify_conflict_winner(
                Some(("tenant_other_demo", 40_000)),
                "tenant_public_demo",
                40_000
            ),
            Err(ParticipantBasePersistenceError::ConflictingReplay)
        ));
        assert!(matches!(
            classify_conflict_winner(None, "tenant_public_demo", 40_000),
            Err(ParticipantBasePersistenceError::CorruptStoredIdentity)
        ));
        assert!(stored_base_identity_matches(
            "participant_public_demo",
            "tenant_public_demo",
            "participant_public_demo",
            "tenant_public_demo"
        ));
        assert!(!stored_base_identity_matches(
            "participant_public_demo",
            "tenant_other_demo",
            "participant_public_demo",
            "tenant_public_demo"
        ));
        assert!(matches!(
            require_stored_base_identity(
                "participant_public_demo",
                "tenant_other_demo",
                "participant_public_demo",
                "tenant_public_demo"
            ),
            Err(ParticipantBasePersistenceError::CorruptStoredIdentity)
        ));
        assert!(require_stored_base_identity(
            "participant_public_demo",
            "tenant_public_demo",
            "participant_public_demo",
            "tenant_public_demo"
        )
        .is_ok());
        assert!(reconstruct_loaded_base(
            "participant_public_demo",
            "tenant_public_demo",
            40_000,
            "participant_public_demo",
            "tenant_public_demo",
        )
        .is_ok());
        assert!(matches!(
            reconstruct_loaded_base(
                "participant_public_demo",
                "tenant_other_demo",
                40_000,
                "participant_public_demo",
                "tenant_public_demo",
            ),
            Err(ParticipantBasePersistenceError::CorruptStoredIdentity)
        ));
    }

    #[test]
    fn conflict_winner_lookup_maps_missing_and_absent_rows() {
        let url = std::env::var("TEST_DATABASE_URL").expect("TEST_DATABASE_URL is required");
        let mut client = Client::connect(&url, NoTls).expect("CI PostgreSQL must be reachable");
        client
            .batch_execute(
                "CREATE SCHEMA IF NOT EXISTS participant_conflict_winner_test;\
                 SET search_path TO participant_conflict_winner_test;\
                 DROP TABLE IF EXISTS assessment_participant;",
            )
            .unwrap();
        let mut missing = client.transaction().unwrap();
        assert!(matches!(
            read_conflict_winner(&mut missing, "participant_public_demo"),
            Err(ParticipantBasePersistenceError::Database(_))
        ));
        missing.rollback().unwrap();

        client
            .batch_execute(
                "CREATE TABLE assessment_participant (\
                     participant_ref TEXT PRIMARY KEY,\
                     tenant_ref TEXT NOT NULL,\
                     created_at_unix_ms BIGINT NOT NULL\
                 );",
            )
            .unwrap();
        let mut empty = client.transaction().unwrap();
        assert!(matches!(
            read_conflict_winner(&mut empty, "participant_public_demo"),
            Ok(None)
        ));
        empty.rollback().unwrap();
    }

    #[test]
    fn database_error_wrap_is_instantiated_in_the_library() {
        let source = postgres::Config::new()
            .host("/no/such/psychometrics-commons.socket")
            .port(1)
            .user("postgres")
            .dbname("psychometrics_commons_test")
            .connect_timeout(std::time::Duration::from_millis(50))
            .connect(postgres::NoTls)
            .err()
            .expect("missing local socket must fail closed");
        let error = ParticipantBasePersistenceError::from(source);
        assert_eq!(
            error.to_string(),
            "PostgreSQL participant base persistence failed"
        );
        assert!(std::error::Error::source(&error).is_some());
    }
}
