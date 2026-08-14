//! `PostgreSQL` 18 persistence for immutable scoring-request identity.
//!
//! This adapter stores the version-pinned dispatch identity that names one
//! completed response snapshot and the exact `AssessmentSpec`, instrument,
//! scoring, calibration, and optional norm references. It does not call
//! `fast-mlsirm` and does not store numeric scores. Replay requires
//! `READ COMMITTED`.

use crate::integration::IntegrationEvent;
use crate::postgres_integration::{enqueue_outbox_event, PersistenceDisposition, PersistenceError};
use crate::postgres_scoring_job::{
    persist_scoring_job, ScoringJobPersistenceDisposition, ScoringJobPersistenceError,
};
use crate::reference::normalized_reference;
use crate::scoring::ScoringRequest;
use crate::scoring_job::ScoringJob;
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

/// Durable dispositions produced by one atomic scoring-dispatch persistence call.
///
/// Mixed dispositions are valid when this transaction safely reconciles pre-existing
/// exact evidence from an older write path. Every newly inserted row is still committed
/// or rolled back together by the caller-owned transaction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScoringDispatchPersistence {
    scoring_request: ScoringRequestPersistenceDisposition,
    scoring_job: ScoringJobPersistenceDisposition,
    outbox: PersistenceDisposition,
}

impl ScoringDispatchPersistence {
    /// Return whether immutable scoring-request evidence was inserted or replayed.
    #[must_use]
    pub const fn scoring_request(self) -> ScoringRequestPersistenceDisposition {
        self.scoring_request
    }

    /// Return whether immutable scoring-job evidence was inserted or replayed.
    #[must_use]
    pub const fn scoring_job(self) -> ScoringJobPersistenceDisposition {
        self.scoring_job
    }

    /// Return whether immutable outbox evidence was inserted or replayed.
    #[must_use]
    pub const fn outbox(self) -> PersistenceDisposition {
        self.outbox
    }
}

/// Fail-closed error for one atomic scoring-dispatch persistence operation.
#[derive(Debug)]
#[non_exhaustive]
pub enum ScoringDispatchPersistenceError {
    /// The scoring job names a different immutable scoring request.
    MismatchedScoringRequest,
    /// Immutable scoring-request persistence failed.
    Request(ScoringRequestPersistenceError),
    /// Durable scoring-job persistence failed.
    Job(ScoringJobPersistenceError),
    /// Transactional outbox persistence failed.
    Outbox(PersistenceError),
}

impl Display for ScoringDispatchPersistenceError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::MismatchedScoringRequest => {
                "scoring job must reference the immutable scoring request in the same dispatch"
            }
            Self::Request(_) => "scoring dispatch request persistence failed",
            Self::Job(_) => "scoring dispatch job persistence failed",
            Self::Outbox(_) => "scoring dispatch outbox persistence failed",
        })
    }
}

impl Error for ScoringDispatchPersistenceError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::MismatchedScoringRequest => None,
            Self::Request(error) => Some(error),
            Self::Job(error) => Some(error),
            Self::Outbox(error) => Some(error),
        }
    }
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

/// Persist a scoring request, its fresh asynchronous job, and one outbox event atomically.
///
/// The caller owns the transaction and therefore the final commit/rollback decision. This
/// function composes the existing immutable request, job, and transactional-outbox adapters
/// without introducing a second transaction boundary. The job must name exactly the supplied
/// scoring request. Event schema, subject, tenant, correlation, and payload semantics remain
/// the caller's versioned integration-contract responsibility; this function only guarantees
/// that the validated outbox envelope cannot commit independently from newly written local
/// dispatch state.
///
/// Exact replay is idempotent. Pre-existing exact evidence may produce mixed dispositions,
/// allowing a caller to reconcile a legacy partial state without rewriting immutable rows.
/// If any stage returns an error, callers must roll back the transaction rather than commit
/// earlier successful stages.
///
/// # Errors
///
/// Returns [`ScoringDispatchPersistenceError::MismatchedScoringRequest`] before writing when
/// the job is bound to another request. Request, job, or outbox persistence failures are
/// preserved in the corresponding typed error variant.
pub fn persist_scoring_dispatch(
    transaction: &mut Transaction<'_>,
    request: &ScoringRequest,
    job: &ScoringJob,
    dispatch_event: &IntegrationEvent,
    outbox_max_attempts: usize,
) -> Result<ScoringDispatchPersistence, ScoringDispatchPersistenceError> {
    if job.scoring_request_ref() != request.scoring_request_ref() {
        return Err(ScoringDispatchPersistenceError::MismatchedScoringRequest);
    }

    let scoring_request = persist_scoring_request(transaction, request)
        .map_err(ScoringDispatchPersistenceError::Request)?;
    let scoring_job = persist_scoring_job(transaction, job)
        .map_err(ScoringDispatchPersistenceError::Job)?;
    let outbox = enqueue_outbox_event(transaction, dispatch_event, outbox_max_attempts)
        .map_err(ScoringDispatchPersistenceError::Outbox)?;

    Ok(ScoringDispatchPersistence {
        scoring_request,
        scoring_job,
        outbox,
    })
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
