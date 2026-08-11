//! Regression tests for time-bounded scoring-worker lease authority.

use psychometrics_commons_runtime::scoring_job::{
    ScoringJob, ScoringJobError, ScoringJobState,
};

#[test]
fn expired_lease_rejects_worker_terminal_evidence_until_recovered() {
    let mut scoring_job =
        ScoringJob::new("scoring_job_expiry", "scoring_request_expiry", 2).unwrap();
    let lease = scoring_job
        .claim("worker_expiry", "lease_expiry", 10_000, 20_000)
        .unwrap();

    assert_eq!(
        scoring_job
            .record_success(lease.fencing_token(), "scoring_result_expired", 20_000)
            .unwrap_err(),
        ScoringJobError::LeaseExpired
    );
    assert_eq!(
        scoring_job
            .record_permanent_failure(lease.fencing_token(), "invalid_contract", 20_000)
            .unwrap_err(),
        ScoringJobError::LeaseExpired
    );
    assert_eq!(
        scoring_job
            .record_retryable_failure(lease.fencing_token(), "timeout_error", 20_000, 21_000)
            .unwrap_err(),
        ScoringJobError::LeaseExpired
    );

    assert_eq!(scoring_job.state(), ScoringJobState::Leased);
    assert_eq!(scoring_job.active_lease(), Some(&lease));
    assert_eq!(
        scoring_job.expire_lease(20_000).unwrap(),
        ScoringJobState::RetryScheduled
    );
}
