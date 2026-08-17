//! Operator recovery contract for persisted scoring job/request mismatches.

use psychometrics_commons_runtime::postgres_scoring_worker::ScoringWorkerCommitError;
use psychometrics_commons_runtime::scoring_worker::ScoringWorkerError;

#[test]
fn stored_request_mismatch_is_integrity_evidence_not_an_engine_retry() {
    let error = ScoringWorkerCommitError::StoredRequestMismatch;

    assert!(error.is_stored_request_integrity_failure());
    let message = error.to_string();
    assert!(message.contains("integrity"));
    assert!(message.contains("investigate"));
    assert!(!message.contains("retry after a typed engine outcome"));

    let planner_error =
        ScoringWorkerCommitError::Planning(ScoringWorkerError::MismatchedScoringResult);
    assert!(!planner_error.is_stored_request_integrity_failure());
}
