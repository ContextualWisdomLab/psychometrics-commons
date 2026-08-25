//! Durable routing from scoring-engine outcomes into asynchronous job state.
//!
//! This module composes the product scoring adapter with the existing PostgreSQL
//! scoring-job state machine. It does not calculate psychometric quantities.
//! Deterministic scientific failures and provenance mismatches are terminal for
//! the current request and are quarantined immediately; unclassified engine or
//! provider failures remain eligible for the existing bounded retry policy.

use crate::postgres_scoring_job::{
    record_permanent_scoring_failure, record_retryable_scoring_failure,
    ScoringJobPersistenceError,
};
use crate::scoring_engine::ScoringEngineExecutionError;
use crate::scoring_job::ScoringJobState;
use postgres::Transaction;

const ENGINE_FAILURE_CODE: &str = "scoring_engine_failure";
const REQUEST_MISMATCH_CODE: &str = "scoring_request_mismatch";

/// Persist the retry/quarantine decision for one failed scoring-engine execution.
///
/// Scientific failures carry the stable scientific code supplied by the product
/// scoring boundary and are quarantined without consuming another automatic
/// attempt. A result/request provenance mismatch is likewise quarantined because
/// retrying the same invalid adapter outcome is not a safe recovery strategy.
/// Unclassified engine failures are recorded through the existing bounded retry
/// transition and therefore quarantine automatically when the attempt budget is
/// exhausted.
///
/// The caller must use the transaction that owns the currently leased scoring
/// job and present that lease's fencing token. The persistence layer remains the
/// authority for stale/expired lease rejection and atomic state mutation.
///
/// # Errors
///
/// Returns [`ScoringJobPersistenceError`] when job identity, timestamps, retry
/// timing, transaction isolation, fencing evidence, lease authority, or the
/// database transition is invalid.
pub fn record_scoring_execution_failure<E>(
    transaction: &mut Transaction<'_>,
    scoring_job_ref: &str,
    fencing_token: u64,
    error: &ScoringEngineExecutionError<E>,
    failed_at_unix_ms: u64,
    retry_at_unix_ms: u64,
) -> Result<ScoringJobState, ScoringJobPersistenceError> {
    match error {
        ScoringEngineExecutionError::Scientific { failure, .. } => {
            record_permanent_scoring_failure(
                transaction,
                scoring_job_ref,
                fencing_token,
                failure.code(),
                failed_at_unix_ms,
            )?;
            Ok(ScoringJobState::Quarantined)
        }
        ScoringEngineExecutionError::Engine(_) => record_retryable_scoring_failure(
            transaction,
            scoring_job_ref,
            fencing_token,
            ENGINE_FAILURE_CODE,
            failed_at_unix_ms,
            retry_at_unix_ms,
        ),
        ScoringEngineExecutionError::RequestMismatch => {
            record_permanent_scoring_failure(
                transaction,
                scoring_job_ref,
                fencing_token,
                REQUEST_MISMATCH_CODE,
                failed_at_unix_ms,
            )?;
            Ok(ScoringJobState::Quarantined)
        }
    }
}
