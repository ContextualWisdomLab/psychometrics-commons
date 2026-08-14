//! `PostgreSQL` 18 persistence for immutable scoring-request identity.
//!
//! This adapter stores the version-pinned dispatch identity that names one
//! completed response snapshot and the exact `AssessmentSpec`, instrument,
//! scoring, calibration, and optional norm references. It does not call
//! `fast-mlsirm` and does not store numeric scores. Replay requires
//! `READ COMMITTED`.

use crate::reference::normalized_reference;
use crate::scoring::ScoringRequest;
use postgres::Transaction;
use std::error::Error;
use std::fmt::{Display, Formatter};

const SCORING_REQUEST_MIGRATION: &str = include_str!("../migrations/0011_scoring_request.sql");

/// Outcome of persisting one immutable scoring request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ScoringRequestPersistenceDisposition {
    /// A new scoring-request row was inserted.
    Inserted,
    /// The same immutable scoring-request identity already existed.
    Duplicate,
}

/// Fail-closed error for durable scoring-request persistence.
#[derive(Debug)]
#[non_exhaustive]
pub enum ScoringRequestPersistenceError {
    /// A scoring, session, snapshot, or version identity was blank or numeric-like.
    InvalidReference,
    /// Request identity was replayed with different immutable evidence.
    ConflictingReplay,
    /// A schema version cannot be represented by the bounded database column.
    InvalidSchemaVersion,
    /// Scoring-request persistence requires `PostgreSQL` `READ COMMITTED` isolation.
    UnsupportedIsolationLevel,
    /// `PostgreSQL` rejected or could not execute the persistence operation.
    Database(postgres::Error),
}

impl Display for ScoringRequestPersistenceError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidReference => {
                "scoring request persistence references must be opaque values"
            }
            Self::ConflictingReplay => {
                "scoring request identity was replayed with conflicting evidence"
            }
            Self::InvalidSchemaVersion => {
                "scoring request schema version exceeds the PostgreSQL integer range"
            }
            Self::UnsupportedIsolationLevel => {
                "scoring request persistence requires read committed isolation"
            }
            Self::Database(_) => "PostgreSQL scoring-request persistence failed",
        })
    }
}

impl Error for ScoringRequestPersistenceError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Database(error) => Some(error),
            _ => None,
        }
    }
}

impl From<postgres::Error> for ScoringRequestPersistenceError {
    fn from(error: postgres::Error) -> Self {
        Self::Database(error)
    }
}

/// Apply the idempotent scoring-request migration to a `PostgreSQL` connection.
///
/// # Errors
///
/// Returns the `PostgreSQL` error if the migration cannot be applied.
pub fn apply_scoring_request_migration(
    client: &mut impl postgres::GenericClient,
) -> Result<(), postgres::Error> {
    client.batch_execute(SCORING_REQUEST_MIGRATION)
}

/// Persist one immutable scoring-request identity.
///
/// Exact replay of the same request identity and version bundle is idempotent.
/// Rebinding any stored field fails closed. Historical requests are never
/// updated.
///
/// # Errors
///
/// Returns [`ScoringRequestPersistenceError`] for unsupported isolation,
/// conflicting replay, an invalid reference or schema version, or a database
/// failure.
pub fn persist_scoring_request(
    transaction: &mut Transaction<'_>,
    request: &ScoringRequest,
) -> Result<ScoringRequestPersistenceDisposition, ScoringRequestPersistenceError> {
    require_read_committed(transaction)?;
    let request_ref = required_reference(request.scoring_request_ref())?;
    let schema_version = i32::from(request.requested_output_schema_version());
    let inserted = transaction.execute(
        "INSERT INTO scoring_request (\
             scoring_request_ref, session_ref, response_snapshot_ref, \
             assessment_spec_ref, instrument_version_ref, scoring_version_ref, \
             calibration_reference, norm_version_ref, requested_output_schema_version\
         ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9) \
         ON CONFLICT (scoring_request_ref) DO NOTHING",
        &[
            &request_ref,
            &request.session_ref(),
            &request.response_snapshot_ref(),
            &request.assessment_spec_ref(),
            &request.instrument_version_ref(),
            &request.scoring_version_ref(),
            &request.calibration_reference(),
            &request.norm_version_ref(),
            &schema_version,
        ],
    )?;
    if inserted == 1 {
        return Ok(ScoringRequestPersistenceDisposition::Inserted);
    }
    classify_existing_request(transaction, request, request_ref, schema_version)
}

fn classify_existing_request(
    transaction: &mut Transaction<'_>,
    request: &ScoringRequest,
    request_ref: &str,
    schema_version: i32,
) -> Result<ScoringRequestPersistenceDisposition, ScoringRequestPersistenceError> {
    let row = transaction.query_one(
        "SELECT session_ref, response_snapshot_ref, assessment_spec_ref, \
                instrument_version_ref, scoring_version_ref, calibration_reference, \
                norm_version_ref, requested_output_schema_version \
         FROM scoring_request WHERE scoring_request_ref = $1",
        &[&request_ref],
    )?;
    let stored_session: String = row.get(0);
    let stored_snapshot: String = row.get(1);
    let stored_spec: String = row.get(2);
    let stored_instrument: String = row.get(3);
    let stored_scoring: String = row.get(4);
    let stored_calibration: String = row.get(5);
    let stored_norm: Option<String> = row.get(6);
    let stored_schema: i32 = row.get(7);
    if stored_session == request.session_ref()
        && stored_snapshot == request.response_snapshot_ref()
        && stored_spec == request.assessment_spec_ref()
        && stored_instrument == request.instrument_version_ref()
        && stored_scoring == request.scoring_version_ref()
        && stored_calibration == request.calibration_reference()
        && stored_norm.as_deref() == request.norm_version_ref()
        && stored_schema == schema_version
    {
        Ok(ScoringRequestPersistenceDisposition::Duplicate)
    } else {
        Err(ScoringRequestPersistenceError::ConflictingReplay)
    }
}

fn required_reference(reference: &str) -> Result<&str, ScoringRequestPersistenceError> {
    normalized_reference(reference).ok_or(ScoringRequestPersistenceError::InvalidReference)
}

fn require_read_committed(
    transaction: &mut Transaction<'_>,
) -> Result<(), ScoringRequestPersistenceError> {
    let row = transaction.query_one("SHOW transaction_isolation", &[])?;
    let isolation: String = row.get(0);
    if isolation == "read committed" {
        Ok(())
    } else {
        Err(ScoringRequestPersistenceError::UnsupportedIsolationLevel)
    }
}

#[cfg(test)]
mod reference_guard_tests {
    use super::{required_reference, ScoringRequestPersistenceError};

    #[test]
    fn blank_and_numeric_references_fail_closed() {
        assert!(matches!(
            required_reference(" "),
            Err(ScoringRequestPersistenceError::InvalidReference)
        ));
        assert!(matches!(
            required_reference("12"),
            Err(ScoringRequestPersistenceError::InvalidReference)
        ));
        assert_eq!(
            required_reference("scoring_request_ko_v1").unwrap(),
            "scoring_request_ko_v1"
        );
        assert_eq!(
            ScoringRequestPersistenceError::InvalidSchemaVersion.to_string(),
            "scoring request schema version exceeds the PostgreSQL integer range"
        );
    }
}
