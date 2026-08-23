//! `PostgreSQL` 18 persistence for immutable result snapshots.
//!
//! This adapter stores product-owned result identity, copied scoring provenance,
//! and construct-level observations. It does not recompute psychometric values.
//! The caller owns the connection, credentials, and transaction boundary. Replay
//! requires `READ COMMITTED` so a concurrent insert that wins a unique-key race
//! is visible to the exact-replay classifier.

use crate::reference::normalized_reference;
use crate::result::ResultSnapshot;
use crate::scoring::ObservationDisposition;
use postgres::Transaction;
use std::error::Error;
use std::fmt::{Display, Formatter};

const RESULT_SNAPSHOT_MIGRATION: &str = include_str!("../migrations/0007_result_snapshot.sql");

/// Outcome of persisting one immutable result snapshot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ResultSnapshotPersistenceDisposition {
    /// A new result snapshot and its observations were inserted.
    Inserted,
    /// The same immutable result identity and copied observations already existed.
    Duplicate,
}

/// Fail-closed error for durable result-snapshot persistence.
#[derive(Debug)]
#[non_exhaustive]
pub enum ResultSnapshotPersistenceError {
    /// A result, participant, or related identity was blank or numeric-like.
    InvalidReference,
    /// Result identity was replayed with different immutable evidence.
    ConflictingReplay,
    /// A superseding snapshot names no different predecessor row visible to this transaction.
    ///
    /// A predecessor inserted earlier in the same transaction is visible and valid. A row
    /// still uncommitted in another transaction is not visible yet and is rejected.
    InvalidSupersession,
    /// A timestamp cannot be represented by the bounded database column.
    InvalidTimestamp,
    /// Result-snapshot persistence requires `PostgreSQL` `READ COMMITTED` isolation.
    UnsupportedIsolationLevel,
    /// `PostgreSQL` rejected or could not execute the persistence operation.
    Database(postgres::Error),
}

impl Display for ResultSnapshotPersistenceError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidReference => {
                "result snapshot persistence references must be opaque values"
            }
            Self::ConflictingReplay => {
                "result snapshot identity was replayed with conflicting evidence"
            }
            Self::InvalidSupersession => {
                "result snapshot supersession predecessor must already exist"
            }
            Self::InvalidTimestamp => {
                "result snapshot timestamp exceeds the PostgreSQL bigint range"
            }
            Self::UnsupportedIsolationLevel => {
                "result snapshot persistence requires read committed isolation"
            }
            Self::Database(_) => "PostgreSQL result-snapshot persistence failed",
        })
    }
}

impl Error for ResultSnapshotPersistenceError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Database(error) => Some(error),
            _ => None,
        }
    }
}

impl From<postgres::Error> for ResultSnapshotPersistenceError {
    fn from(error: postgres::Error) -> Self {
        Self::Database(error)
    }
}

/// Apply the idempotent result-snapshot migration to a `PostgreSQL` connection.
///
/// # Errors
///
/// Returns the `PostgreSQL` error if the migration cannot be applied.
pub fn apply_result_snapshot_migration(
    client: &mut impl postgres::GenericClient,
) -> Result<(), postgres::Error> {
    client.batch_execute(RESULT_SNAPSHOT_MIGRATION)
}

/// Persist one immutable result snapshot and its copied score observations.
///
/// Exact replay of the same snapshot identity, provenance, and observations is
/// idempotent. Rebinding `result_snapshot_ref` to different provenance or
/// observation evidence fails closed. A superseding snapshot may reference only
/// a different predecessor row that is already visible to the current transaction
/// before the successor is inserted. A predecessor inserted earlier in this same
/// transaction is valid; an uncommitted predecessor in another transaction is not
/// yet visible and is rejected. This insertion ordering prevents forward-reference
/// cycles under the immutable snapshot model. Historical snapshots are never
/// updated.
///
/// # Errors
///
/// Returns [`ResultSnapshotPersistenceError`] for unsupported isolation,
/// conflicting replay, a missing or not-yet-visible supersession predecessor, an
/// invalid reference or timestamp, or a database failure.
pub fn persist_result_snapshot(
    transaction: &mut Transaction<'_>,
    snapshot: &ResultSnapshot,
) -> Result<ResultSnapshotPersistenceDisposition, ResultSnapshotPersistenceError> {
    require_read_committed(transaction)?;
    let snapshot_ref = required_reference(snapshot.result_snapshot_ref())?;
    validate_supersession_predecessor(transaction, snapshot.supersedes_ref())?;
    let created_at = postgres_timestamp(snapshot.created_at_unix_ms())?;
    let consent_snapshot_refs = snapshot.consent_snapshot_refs().to_vec();
    let schema_version = i32::from(snapshot.requested_output_schema_version());
    let inserted = transaction.execute(
        "INSERT INTO result_snapshot (\
             result_snapshot_ref, participant_ref, scoring_result_ref, session_ref, \
             response_snapshot_ref, assessment_spec_ref, instrument_version_ref, \
             scoring_version_ref, calibration_reference, norm_version_ref, \
             requested_output_schema_version, narrative_version_ref, \
             consent_snapshot_refs, engine_artifact_digest, created_at_unix_ms, \
             supersedes_ref\
         ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16) \
         ON CONFLICT (result_snapshot_ref) DO NOTHING",
        &[
            &snapshot_ref,
            &snapshot.participant_ref(),
            &snapshot.scoring_result_ref(),
            &snapshot.session_ref(),
            &snapshot.response_snapshot_ref(),
            &snapshot.assessment_spec_ref(),
            &snapshot.instrument_version_ref(),
            &snapshot.scoring_version_ref(),
            &snapshot.calibration_reference(),
            &snapshot.norm_version_ref(),
            &schema_version,
            &snapshot.narrative_version_ref(),
            &consent_snapshot_refs,
            &snapshot.engine_artifact_digest(),
            &created_at,
            &snapshot.supersedes_ref(),
        ],
    )?;
    if inserted == 1 {
        insert_observations(transaction, snapshot_ref, snapshot)?;
        return Ok(ResultSnapshotPersistenceDisposition::Inserted);
    }
    classify_existing_snapshot(transaction, snapshot, created_at, schema_version)
}

fn validate_supersession_predecessor(
    transaction: &mut Transaction<'_>,
    supersedes_ref: Option<&str>,
) -> Result<(), ResultSnapshotPersistenceError> {
    let Some(supersedes_ref) = supersedes_ref else {
        return Ok(());
    };
    let supersedes_ref = required_reference(supersedes_ref)?;
    if transaction
        .query_opt(
            "SELECT 1 FROM result_snapshot WHERE result_snapshot_ref = $1",
            &[&supersedes_ref],
        )?
        .is_some()
    {
        Ok(())
    } else {
        Err(ResultSnapshotPersistenceError::InvalidSupersession)
    }
}

fn insert_observations(
    transaction: &mut Transaction<'_>,
    snapshot_ref: &str,
    snapshot: &ResultSnapshot,
) -> Result<(), ResultSnapshotPersistenceError> {
    for (index, observation) in snapshot.score_observations().iter().enumerate() {
        let observation_order = observation_order(index)?;
        let disposition = observation_disposition_name(observation.disposition());
        transaction.execute(
            "INSERT INTO result_snapshot_observation (\
                 result_snapshot_ref, observation_order, construct_ref, \
                 observation_disposition, score, standard_error\
             ) VALUES ($1, $2, $3, $4, $5, $6)",
            &[
                &snapshot_ref,
                &observation_order,
                &observation.construct_ref(),
                &disposition,
                &observation.score(),
                &observation.standard_error(),
            ],
        )?;
    }
    Ok(())
}

fn classify_existing_snapshot(
    transaction: &mut Transaction<'_>,
    snapshot: &ResultSnapshot,
    created_at: i64,
    schema_version: i32,
) -> Result<ResultSnapshotPersistenceDisposition, ResultSnapshotPersistenceError> {
    let row = transaction.query_one(
        "SELECT participant_ref, scoring_result_ref, session_ref, response_snapshot_ref, \
                assessment_spec_ref, instrument_version_ref, scoring_version_ref, \
                calibration_reference, norm_version_ref, requested_output_schema_version, \
                narrative_version_ref, consent_snapshot_refs, engine_artifact_digest, \
                created_at_unix_ms, supersedes_ref \
         FROM result_snapshot WHERE result_snapshot_ref = $1",
        &[&snapshot.result_snapshot_ref()],
    )?;
    let stored = StoredSnapshot {
        participant_ref: row.get(0),
        scoring_result_ref: row.get(1),
        session_ref: row.get(2),
        response_snapshot_ref: row.get(3),
        assessment_spec_ref: row.get(4),
        instrument_version_ref: row.get(5),
        scoring_version_ref: row.get(6),
        calibration_reference: row.get(7),
        norm_version_ref: row.get(8),
        requested_output_schema_version: row.get(9),
        narrative_version_ref: row.get(10),
        consent_snapshot_refs: row.get(11),
        engine_artifact_digest: row.get(12),
        created_at_unix_ms: row.get(13),
        supersedes_ref: row.get(14),
    };
    if stored != StoredSnapshot::from_domain(snapshot, created_at, schema_version) {
        return Err(ResultSnapshotPersistenceError::ConflictingReplay);
    }

    let rows = transaction.query(
        "SELECT construct_ref, observation_disposition, score, standard_error \
         FROM result_snapshot_observation \
         WHERE result_snapshot_ref = $1 \
         ORDER BY observation_order",
        &[&snapshot.result_snapshot_ref()],
    )?;
    let stored_observations: Vec<StoredObservation> = rows
        .into_iter()
        .map(|observation_row| StoredObservation {
            construct_ref: observation_row.get(0),
            observation_disposition: observation_row.get(1),
            score: observation_row.get(2),
            standard_error: observation_row.get(3),
        })
        .collect();
    let incoming_observations: Vec<StoredObservation> = snapshot
        .score_observations()
        .iter()
        .map(|observation| StoredObservation {
            construct_ref: observation.construct_ref().to_owned(),
            observation_disposition: observation_disposition_name(observation.disposition())
                .to_owned(),
            score: observation.score(),
            standard_error: observation.standard_error(),
        })
        .collect();
    if stored_observations == incoming_observations {
        Ok(ResultSnapshotPersistenceDisposition::Duplicate)
    } else {
        Err(ResultSnapshotPersistenceError::ConflictingReplay)
    }
}

#[derive(Debug, PartialEq)]
struct StoredSnapshot {
    participant_ref: String,
    scoring_result_ref: String,
    session_ref: String,
    response_snapshot_ref: String,
    assessment_spec_ref: String,
    instrument_version_ref: String,
    scoring_version_ref: String,
    calibration_reference: String,
    norm_version_ref: Option<String>,
    requested_output_schema_version: i32,
    narrative_version_ref: String,
    consent_snapshot_refs: Vec<String>,
    engine_artifact_digest: String,
    created_at_unix_ms: i64,
    supersedes_ref: Option<String>,
}

impl StoredSnapshot {
    fn from_domain(snapshot: &ResultSnapshot, created_at: i64, schema_version: i32) -> Self {
        Self {
            participant_ref: snapshot.participant_ref().to_owned(),
            scoring_result_ref: snapshot.scoring_result_ref().to_owned(),
            session_ref: snapshot.session_ref().to_owned(),
            response_snapshot_ref: snapshot.response_snapshot_ref().to_owned(),
            assessment_spec_ref: snapshot.assessment_spec_ref().to_owned(),
            instrument_version_ref: snapshot.instrument_version_ref().to_owned(),
            scoring_version_ref: snapshot.scoring_version_ref().to_owned(),
            calibration_reference: snapshot.calibration_reference().to_owned(),
            norm_version_ref: snapshot.norm_version_ref().map(str::to_owned),
            requested_output_schema_version: schema_version,
            narrative_version_ref: snapshot.narrative_version_ref().to_owned(),
            consent_snapshot_refs: snapshot.consent_snapshot_refs().to_vec(),
            engine_artifact_digest: snapshot.engine_artifact_digest().to_owned(),
            created_at_unix_ms: created_at,
            supersedes_ref: snapshot.supersedes_ref().map(str::to_owned),
        }
    }
}

#[derive(Debug, PartialEq)]
struct StoredObservation {
    construct_ref: String,
    observation_disposition: String,
    score: Option<f64>,
    standard_error: Option<f64>,
}

fn observation_disposition_name(disposition: ObservationDisposition) -> &'static str {
    match disposition {
        ObservationDisposition::Scored => "scored",
        ObservationDisposition::Abstained => "abstained",
        ObservationDisposition::Failed => "failed",
        ObservationDisposition::Excluded => "excluded",
    }
}

fn required_reference(reference: &str) -> Result<&str, ResultSnapshotPersistenceError> {
    normalized_reference(reference).ok_or(ResultSnapshotPersistenceError::InvalidReference)
}

fn postgres_timestamp(timestamp: u64) -> Result<i64, ResultSnapshotPersistenceError> {
    i64::try_from(timestamp).map_err(|_| ResultSnapshotPersistenceError::InvalidTimestamp)
}

fn observation_order(index: usize) -> Result<i32, ResultSnapshotPersistenceError> {
    i32::try_from(index).map_err(|_| ResultSnapshotPersistenceError::InvalidTimestamp)
}

fn require_read_committed(
    transaction: &mut Transaction<'_>,
) -> Result<(), ResultSnapshotPersistenceError> {
    let row = transaction.query_one("SHOW transaction_isolation", &[])?;
    let isolation: String = row.get(0);
    if isolation == "read committed" {
        Ok(())
    } else {
        Err(ResultSnapshotPersistenceError::UnsupportedIsolationLevel)
    }
}

#[cfg(test)]
mod reference_guard_tests {
    use super::{
        observation_disposition_name, observation_order, postgres_timestamp, required_reference,
        ResultSnapshotPersistenceError,
    };
    use crate::scoring::ObservationDisposition;

    #[test]
    fn blank_numeric_overflow_and_disposition_names_are_classified() {
        assert!(matches!(
            required_reference(" "),
            Err(ResultSnapshotPersistenceError::InvalidReference)
        ));
        assert!(matches!(
            required_reference("12"),
            Err(ResultSnapshotPersistenceError::InvalidReference)
        ));
        assert_eq!(
            required_reference("result_snapshot_ko_v1").unwrap(),
            "result_snapshot_ko_v1"
        );
        assert!(matches!(
            postgres_timestamp(u64::MAX),
            Err(ResultSnapshotPersistenceError::InvalidTimestamp)
        ));
        assert_eq!(postgres_timestamp(70_000).unwrap(), 70_000);
        assert_eq!(observation_order(0).unwrap(), 0);
        assert!(matches!(
            observation_order(usize::MAX),
            Err(ResultSnapshotPersistenceError::InvalidTimestamp)
        ));
        assert_eq!(
            observation_disposition_name(ObservationDisposition::Scored),
            "scored"
        );
        assert_eq!(
            observation_disposition_name(ObservationDisposition::Abstained),
            "abstained"
        );
        assert_eq!(
            observation_disposition_name(ObservationDisposition::Failed),
            "failed"
        );
        assert_eq!(
            observation_disposition_name(ObservationDisposition::Excluded),
            "excluded"
        );
    }
}
