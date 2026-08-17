//! `PostgreSQL` 18 persistence for immutable result snapshots.
//!
//! This adapter stores product-owned result identity, copied scoring provenance,
//! and construct-level observations. It does not recompute psychometric values.
//! The caller owns the connection, credentials, and transaction boundary. Replay
//! requires `READ COMMITTED` so a concurrent insert that wins a unique-key race
//! is visible to the exact-replay classifier.

use crate::reference::normalized_reference;
use crate::result::{ResultSnapshot, ResultSnapshotError, ResultSnapshotEvidence};
use crate::scoring::{ObservationDisposition, ScoreObservation};
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
    /// A timestamp cannot be represented by the bounded database column.
    InvalidTimestamp,
    /// Result-snapshot persistence requires `PostgreSQL` `READ COMMITTED` isolation.
    UnsupportedIsolationLevel,
    /// `PostgreSQL` rejected or could not execute the persistence operation.
    Database(postgres::Error),
    /// Durable rows cannot reconstruct the published result snapshot.
    InconsistentEvidence,
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
            Self::InvalidTimestamp => {
                "result snapshot timestamp exceeds the PostgreSQL bigint range"
            }
            Self::UnsupportedIsolationLevel => {
                "result snapshot persistence requires read committed isolation"
            }
            Self::Database(_) => "PostgreSQL result-snapshot persistence failed",
            Self::InconsistentEvidence => {
                "durable result evidence cannot reconstruct the published snapshot"
            }
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
/// observation evidence fails closed. Historical snapshots are never updated.
///
/// # Errors
///
/// Returns [`ResultSnapshotPersistenceError`] for unsupported isolation,
/// conflicting replay, an invalid reference or timestamp, or a database failure.
pub fn persist_result_snapshot(
    transaction: &mut Transaction<'_>,
    snapshot: &ResultSnapshot,
) -> Result<ResultSnapshotPersistenceDisposition, ResultSnapshotPersistenceError> {
    require_read_committed(transaction)?;
    let snapshot_ref = required_reference(snapshot.result_snapshot_ref())?;
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

/// Load the current published result for one assessment session.
///
/// After restart, call this with the session the participant is viewing. It
/// returns the unique non-superseded snapshot so a worker can serve the stored
/// score without calling the scoring engine. No header returns `None`. Two
/// current tips, or stored rows whose supersession graph leaves no tip (a
/// cycle), fail closed so a worker cannot treat corruption as "score now".
///
/// # Errors
///
/// Returns [`ResultSnapshotPersistenceError`] for unsupported isolation, an
/// invalid session reference, inconsistent durable evidence, or a database
/// failure.
pub fn load_current_result_snapshot_for_session(
    transaction: &mut Transaction<'_>,
    session_ref: &str,
) -> Result<Option<ResultSnapshot>, ResultSnapshotPersistenceError> {
    require_read_committed(transaction)?;
    let session_ref = required_reference(session_ref)?;
    let counts = transaction.query_one(
        "SELECT \
            (SELECT COUNT(*) FROM result_snapshot \
             WHERE session_ref = $1 \
               AND result_snapshot_ref NOT IN ( \
                   SELECT supersedes_ref FROM result_snapshot \
                   WHERE session_ref = $1 AND supersedes_ref IS NOT NULL \
               ))::bigint, \
            (SELECT COUNT(*) FROM result_snapshot WHERE session_ref = $1)::bigint",
        &[&session_ref],
    )?;
    let tip_count = stored_nonnegative_count(counts.get(0))?;
    let session_has_snapshots = counts.get::<_, i64>(1) > 0;
    match classify_current_session_tips(tip_count, session_has_snapshots)? {
        CurrentSessionTipPlan::Absent => Ok(None),
        CurrentSessionTipPlan::LoadUniqueTip => {
            let snapshot_ref: String = transaction
                .query_one(
                    "SELECT result_snapshot_ref \
                     FROM result_snapshot \
                     WHERE session_ref = $1 \
                       AND result_snapshot_ref NOT IN ( \
                           SELECT supersedes_ref FROM result_snapshot \
                           WHERE session_ref = $1 AND supersedes_ref IS NOT NULL \
                       )",
                    &[&session_ref],
                )?
                .get(0);
            load_result_snapshot(transaction, &snapshot_ref)
        }
    }
}

/// Load one immutable result snapshot from durable evidence.
///
/// Returns `Ok(None)` when no snapshot header exists. Copied observations are
/// reconstructed in stored `observation_order`, which must be contiguous
/// `0..n-1`. After load, exact persist replay stays
/// [`ResultSnapshotPersistenceDisposition::Duplicate`].
///
/// # Errors
///
/// Returns [`ResultSnapshotPersistenceError`] for unsupported isolation, an
/// invalid snapshot reference, inconsistent durable evidence, or a database
/// failure.
pub fn load_result_snapshot(
    transaction: &mut Transaction<'_>,
    result_snapshot_ref: &str,
) -> Result<Option<ResultSnapshot>, ResultSnapshotPersistenceError> {
    require_read_committed(transaction)?;
    let result_snapshot_ref = required_reference(result_snapshot_ref)?;
    let header = transaction.query_opt(
        "SELECT participant_ref, scoring_result_ref, session_ref, response_snapshot_ref, \
                assessment_spec_ref, instrument_version_ref, scoring_version_ref, \
                calibration_reference, norm_version_ref, requested_output_schema_version, \
                narrative_version_ref, consent_snapshot_refs, engine_artifact_digest, \
                created_at_unix_ms, supersedes_ref \
         FROM result_snapshot WHERE result_snapshot_ref = $1",
        &[&result_snapshot_ref],
    )?;
    let Some(header) = header else {
        return Ok(None);
    };
    let participant_ref: String = header.get(0);
    let scoring_result_ref: String = header.get(1);
    let session_ref: String = header.get(2);
    let response_snapshot_ref: String = header.get(3);
    let assessment_spec_ref: String = header.get(4);
    let instrument_version_ref: String = header.get(5);
    let scoring_version_ref: String = header.get(6);
    let calibration_reference: String = header.get(7);
    let norm_version_ref: Option<String> = header.get(8);
    let requested_output_schema_version = stored_schema_version(header.get(9))?;
    let narrative_version_ref: String = header.get(10);
    let consent_snapshot_refs: Vec<String> = header.get(11);
    let engine_artifact_digest: String = header.get(12);
    let created_at_unix_ms = stored_timestamp(header.get(13))?;
    let supersedes_ref: Option<String> = header.get(14);
    let rows = transaction.query(
        "SELECT observation_order, construct_ref, observation_disposition, score, standard_error \
         FROM result_snapshot_observation \
         WHERE result_snapshot_ref = $1 \
         ORDER BY observation_order",
        &[&result_snapshot_ref],
    )?;
    let mut score_observations = Vec::with_capacity(rows.len());
    for (expected_index, row) in rows.iter().enumerate() {
        require_contiguous_observation_order(expected_index, row.get(0))?;
        score_observations.push(observation_from_stored(
            row.get(1),
            &row.get::<_, String>(2),
            row.get(3),
            row.get(4),
        )?);
    }
    ResultSnapshot::from_durable_evidence(ResultSnapshotEvidence {
        result_snapshot_ref,
        participant_ref: &participant_ref,
        scoring_result_ref: &scoring_result_ref,
        session_ref: &session_ref,
        response_snapshot_ref: &response_snapshot_ref,
        assessment_spec_ref: &assessment_spec_ref,
        instrument_version_ref: &instrument_version_ref,
        scoring_version_ref: &scoring_version_ref,
        calibration_reference: &calibration_reference,
        norm_version_ref: norm_version_ref.as_deref(),
        requested_output_schema_version,
        narrative_version_ref: &narrative_version_ref,
        consent_snapshot_refs: &consent_snapshot_refs,
        engine_artifact_digest: &engine_artifact_digest,
        score_observations,
        created_at_unix_ms,
        supersedes_ref: supersedes_ref.as_deref(),
    })
    .map(Some)
    .map_err(durable_evidence_error)
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

fn observation_from_stored(
    construct_ref: String,
    disposition: &str,
    score: Option<f64>,
    standard_error: Option<f64>,
) -> Result<ScoreObservation, ResultSnapshotPersistenceError> {
    let disposition = match disposition {
        "scored" => ObservationDisposition::Scored,
        "abstained" => ObservationDisposition::Abstained,
        "failed" => ObservationDisposition::Failed,
        "excluded" => ObservationDisposition::Excluded,
        _ => return Err(ResultSnapshotPersistenceError::InconsistentEvidence),
    };
    match disposition {
        ObservationDisposition::Scored => {
            let score = score.ok_or(ResultSnapshotPersistenceError::InconsistentEvidence)?;
            ScoreObservation::scored(construct_ref, score, standard_error)
                .map_err(|_| ResultSnapshotPersistenceError::InconsistentEvidence)
        }
        ObservationDisposition::Abstained
        | ObservationDisposition::Failed
        | ObservationDisposition::Excluded => {
            if score.is_some() || standard_error.is_some() {
                return Err(ResultSnapshotPersistenceError::InconsistentEvidence);
            }
            ScoreObservation::without_score(construct_ref, disposition)
                .map_err(|_| ResultSnapshotPersistenceError::InconsistentEvidence)
        }
    }
}

fn stored_timestamp(timestamp: i64) -> Result<u64, ResultSnapshotPersistenceError> {
    u64::try_from(timestamp).map_err(|_| ResultSnapshotPersistenceError::InconsistentEvidence)
}

fn stored_schema_version(schema_version: i32) -> Result<u16, ResultSnapshotPersistenceError> {
    u16::try_from(schema_version).map_err(|_| ResultSnapshotPersistenceError::InconsistentEvidence)
}

fn durable_evidence_error(error: ResultSnapshotError) -> ResultSnapshotPersistenceError {
    match error {
        ResultSnapshotError::EmptyReference => ResultSnapshotPersistenceError::InvalidReference,
        ResultSnapshotError::InconsistentEvidence
        | ResultSnapshotError::MissingConsentSnapshot
        | ResultSnapshotError::DuplicateConsentSnapshot
        | ResultSnapshotError::InvalidCreationTime
        | ResultSnapshotError::SelfSupersession
        | ResultSnapshotError::ScoringRequestMismatch => {
            ResultSnapshotPersistenceError::InconsistentEvidence
        }
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CurrentSessionTipPlan {
    Absent,
    LoadUniqueTip,
}

fn stored_nonnegative_count(value: i64) -> Result<usize, ResultSnapshotPersistenceError> {
    usize::try_from(value).map_err(|_| ResultSnapshotPersistenceError::InconsistentEvidence)
}

fn classify_current_session_tips(
    tip_count: usize,
    session_has_snapshots: bool,
) -> Result<CurrentSessionTipPlan, ResultSnapshotPersistenceError> {
    match (tip_count, session_has_snapshots) {
        (0, false) => Ok(CurrentSessionTipPlan::Absent),
        (1, true) => Ok(CurrentSessionTipPlan::LoadUniqueTip),
        _ => Err(ResultSnapshotPersistenceError::InconsistentEvidence),
    }
}

fn require_contiguous_observation_order(
    expected_index: usize,
    stored_order: i32,
) -> Result<(), ResultSnapshotPersistenceError> {
    let expected_order = i32::try_from(expected_index)
        .map_err(|_| ResultSnapshotPersistenceError::InconsistentEvidence)?;
    if stored_order == expected_order {
        Ok(())
    } else {
        Err(ResultSnapshotPersistenceError::InconsistentEvidence)
    }
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
        classify_current_session_tips, durable_evidence_error,
        load_current_result_snapshot_for_session, observation_disposition_name,
        observation_from_stored, observation_order, postgres_timestamp,
        require_contiguous_observation_order, required_reference, stored_nonnegative_count,
        stored_schema_version, stored_timestamp, CurrentSessionTipPlan,
        ResultSnapshotPersistenceError,
    };
    use crate::result::ResultSnapshotError;
    use crate::scoring::ObservationDisposition;
    use postgres::{Client, NoTls};

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
        assert!(require_contiguous_observation_order(0, 0).is_ok());
        assert!(matches!(
            require_contiguous_observation_order(1, 2),
            Err(ResultSnapshotPersistenceError::InconsistentEvidence)
        ));
        assert!(matches!(
            require_contiguous_observation_order(usize::MAX, 0),
            Err(ResultSnapshotPersistenceError::InconsistentEvidence)
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

    #[test]
    fn stored_observations_rebuild_or_fail_closed() {
        assert_eq!(stored_timestamp(70_000).unwrap(), 70_000);
        assert!(matches!(
            stored_timestamp(-1),
            Err(ResultSnapshotPersistenceError::InconsistentEvidence)
        ));
        assert_eq!(stored_schema_version(1).unwrap(), 1);
        assert!(matches!(
            stored_schema_version(-1),
            Err(ResultSnapshotPersistenceError::InconsistentEvidence)
        ));
        assert_eq!(
            observation_from_stored(
                "construct_extraversion".to_owned(),
                "scored",
                Some(0.5),
                Some(0.04)
            )
            .unwrap()
            .score(),
            Some(0.5)
        );
        assert_eq!(
            observation_from_stored("construct_openness".to_owned(), "abstained", None, None)
                .unwrap()
                .disposition(),
            ObservationDisposition::Abstained
        );
        assert_eq!(
            observation_from_stored("construct_agreeableness".to_owned(), "failed", None, None)
                .unwrap()
                .disposition(),
            ObservationDisposition::Failed
        );
        assert_eq!(
            observation_from_stored(
                "construct_conscientiousness".to_owned(),
                "excluded",
                None,
                None
            )
            .unwrap()
            .disposition(),
            ObservationDisposition::Excluded
        );
        assert!(matches!(
            observation_from_stored("construct_unknown".to_owned(), "invented", None, None),
            Err(ResultSnapshotPersistenceError::InconsistentEvidence)
        ));
        assert!(matches!(
            observation_from_stored("construct_extraversion".to_owned(), "scored", None, None),
            Err(ResultSnapshotPersistenceError::InconsistentEvidence)
        ));
        assert!(matches!(
            observation_from_stored(
                "construct_openness".to_owned(),
                "abstained",
                Some(0.1),
                None
            ),
            Err(ResultSnapshotPersistenceError::InconsistentEvidence)
        ));
        assert!(matches!(
            observation_from_stored("construct_openness".to_owned(), "failed", None, Some(0.1)),
            Err(ResultSnapshotPersistenceError::InconsistentEvidence)
        ));
        assert!(matches!(
            observation_from_stored(" ".to_owned(), "excluded", None, None),
            Err(ResultSnapshotPersistenceError::InconsistentEvidence)
        ));
        assert!(matches!(
            observation_from_stored(
                "construct_extraversion".to_owned(),
                "scored",
                Some(f64::NAN),
                None
            ),
            Err(ResultSnapshotPersistenceError::InconsistentEvidence)
        ));
    }

    #[test]
    fn durable_reconstruction_errors_map_to_persistence_failures() {
        assert!(matches!(
            durable_evidence_error(ResultSnapshotError::EmptyReference),
            ResultSnapshotPersistenceError::InvalidReference
        ));
        assert!(matches!(
            durable_evidence_error(ResultSnapshotError::InconsistentEvidence),
            ResultSnapshotPersistenceError::InconsistentEvidence
        ));
        assert!(matches!(
            durable_evidence_error(ResultSnapshotError::MissingConsentSnapshot),
            ResultSnapshotPersistenceError::InconsistentEvidence
        ));
        assert!(matches!(
            durable_evidence_error(ResultSnapshotError::DuplicateConsentSnapshot),
            ResultSnapshotPersistenceError::InconsistentEvidence
        ));
        assert!(matches!(
            durable_evidence_error(ResultSnapshotError::InvalidCreationTime),
            ResultSnapshotPersistenceError::InconsistentEvidence
        ));
        assert!(matches!(
            durable_evidence_error(ResultSnapshotError::SelfSupersession),
            ResultSnapshotPersistenceError::InconsistentEvidence
        ));
        assert!(matches!(
            durable_evidence_error(ResultSnapshotError::ScoringRequestMismatch),
            ResultSnapshotPersistenceError::InconsistentEvidence
        ));
    }

    #[test]
    fn session_tip_query_fails_closed_when_every_snapshot_is_superseded() {
        assert_eq!(
            classify_current_session_tips(0, false).unwrap(),
            CurrentSessionTipPlan::Absent
        );
        assert!(matches!(
            classify_current_session_tips(0, true),
            Err(ResultSnapshotPersistenceError::InconsistentEvidence)
        ));
        assert_eq!(
            classify_current_session_tips(1, true).unwrap(),
            CurrentSessionTipPlan::LoadUniqueTip
        );
        assert!(matches!(
            classify_current_session_tips(1, false),
            Err(ResultSnapshotPersistenceError::InconsistentEvidence)
        ));
        assert!(matches!(
            classify_current_session_tips(2, true),
            Err(ResultSnapshotPersistenceError::InconsistentEvidence)
        ));
        assert_eq!(stored_nonnegative_count(0).unwrap(), 0);
        assert_eq!(stored_nonnegative_count(1).unwrap(), 1);
        assert!(matches!(
            stored_nonnegative_count(-1),
            Err(ResultSnapshotPersistenceError::InconsistentEvidence)
        ));
    }

    #[test]
    fn current_session_tip_lookup_maps_missing_relation_to_database_error() {
        let url = std::env::var("TEST_DATABASE_URL").expect("TEST_DATABASE_URL is required");
        let mut client = Client::connect(&url, NoTls).expect("CI PostgreSQL must be reachable");
        client
            .batch_execute("SET search_path TO result_snapshot_current_tip_missing;")
            .unwrap();
        let mut transaction = client.transaction().unwrap();
        assert!(matches!(
            load_current_result_snapshot_for_session(&mut transaction, "session_ipip_ko_quick"),
            Err(ResultSnapshotPersistenceError::Database(_))
        ));
        transaction.rollback().unwrap();
    }
}
