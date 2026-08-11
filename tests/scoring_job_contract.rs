//! Contract tests for asynchronous scoring-job ownership, retry, and fencing semantics.

use psychometrics_commons_runtime::scoring_job::{
    ScoringJob, ScoringJobError, ScoringJobState,
};

fn job(max_attempts: u32) -> ScoringJob {
    ScoringJob::new("scoring_job_alpha", "scoring_request_alpha", max_attempts).unwrap()
}

#[test]
fn scoring_job_rejects_ambiguous_identity_and_attempt_policy() {
    assert_eq!(
        ScoringJob::new("123", "scoring_request_alpha", 3).unwrap_err(),
        ScoringJobError::InvalidReference
    );
    assert_eq!(
        ScoringJob::new("scoring_job_alpha", "-3", 3).unwrap_err(),
        ScoringJobError::InvalidReference
    );
    assert_eq!(
        ScoringJob::new("scoring_job_alpha", "scoring_request_alpha", 0).unwrap_err(),
        ScoringJobError::InvalidAttemptLimit
    );
}

#[test]
fn lease_claims_are_due_bounded_and_fenced() {
    let mut scoring_job = job(3);
    assert_eq!(scoring_job.state(), ScoringJobState::Queued);
    assert_eq!(scoring_job.scoring_job_ref(), "scoring_job_alpha");
    assert_eq!(scoring_job.scoring_request_ref(), "scoring_request_alpha");
    assert_eq!(scoring_job.attempt_count(), 0);
    assert_eq!(scoring_job.max_attempts(), 3);
    assert!(scoring_job.active_lease().is_none());
    assert!(scoring_job.result_ref().is_none());
    assert!(scoring_job.last_failure_code().is_none());

    assert_eq!(
        scoring_job
            .claim("worker_alpha", "lease_alpha", 0, 20_000)
            .unwrap_err(),
        ScoringJobError::InvalidTimestamp
    );
    assert_eq!(
        scoring_job
            .claim("worker_alpha", "lease_alpha", 10_000, 10_000)
            .unwrap_err(),
        ScoringJobError::InvalidLeaseWindow
    );
    assert_eq!(
        scoring_job
            .claim("123", "lease_alpha", 10_000, 20_000)
            .unwrap_err(),
        ScoringJobError::InvalidReference
    );
    assert_eq!(
        scoring_job
            .claim("worker_alpha", "1.5", 10_000, 20_000)
            .unwrap_err(),
        ScoringJobError::InvalidReference
    );

    let lease = scoring_job
        .claim("worker_alpha", "lease_alpha", 10_000, 20_000)
        .unwrap();
    assert_eq!(lease.worker_ref(), "worker_alpha");
    assert_eq!(lease.lease_ref(), "lease_alpha");
    assert_eq!(lease.fencing_token(), 1);
    assert_eq!(lease.expires_at_unix_ms(), 20_000);
    assert_eq!(scoring_job.state(), ScoringJobState::Leased);
    assert_eq!(scoring_job.attempt_count(), 1);
    assert_eq!(scoring_job.active_lease(), Some(&lease));

    assert_eq!(
        scoring_job
            .claim("worker_beta", "lease_beta", 11_000, 21_000)
            .unwrap_err(),
        ScoringJobError::NotLeaseable
    );
}

#[test]
fn retry_schedule_blocks_early_claim_and_invalidates_stale_workers() {
    let mut scoring_job = job(3);
    let first = scoring_job
        .claim("worker_alpha", "lease_alpha", 10_000, 20_000)
        .unwrap();

    assert_eq!(
        scoring_job
            .record_retryable_failure(first.fencing_token() + 1, "timeout_error", 11_000, 15_000)
            .unwrap_err(),
        ScoringJobError::StaleLease
    );
    assert_eq!(
        scoring_job
            .record_retryable_failure(first.fencing_token(), "123", 11_000, 15_000)
            .unwrap_err(),
        ScoringJobError::InvalidReference
    );
    assert_eq!(
        scoring_job
            .record_retryable_failure(first.fencing_token(), "timeout_error", 0, 15_000)
            .unwrap_err(),
        ScoringJobError::InvalidTimestamp
    );
    assert_eq!(
        scoring_job
            .record_retryable_failure(first.fencing_token(), "timeout_error", 11_000, 10_999)
            .unwrap_err(),
        ScoringJobError::InvalidRetryWindow
    );

    assert_eq!(
        scoring_job
            .record_retryable_failure(first.fencing_token(), "timeout_error", 11_000, 15_000)
            .unwrap(),
        ScoringJobState::RetryScheduled
    );
    assert_eq!(scoring_job.last_failure_code(), Some("timeout_error"));
    assert_eq!(scoring_job.next_attempt_at_unix_ms(), Some(15_000));
    assert!(scoring_job.active_lease().is_none());

    assert_eq!(
        scoring_job
            .claim("worker_beta", "lease_beta", 14_999, 25_000)
            .unwrap_err(),
        ScoringJobError::LeaseNotDue
    );
    let second = scoring_job
        .claim("worker_beta", "lease_beta", 15_000, 25_000)
        .unwrap();
    assert_eq!(second.fencing_token(), 2);
    assert_eq!(scoring_job.attempt_count(), 2);
    assert_eq!(scoring_job.next_attempt_at_unix_ms(), None);

    assert_eq!(
        scoring_job
            .record_success(first.fencing_token(), "scoring_result_alpha")
            .unwrap_err(),
        ScoringJobError::StaleLease
    );
}

#[test]
fn successful_completion_is_idempotent_only_for_the_same_result_and_fence() {
    let mut scoring_job = job(2);
    let lease = scoring_job
        .claim("worker_alpha", "lease_alpha", 10_000, 20_000)
        .unwrap();

    assert_eq!(
        scoring_job
            .record_success(lease.fencing_token(), "123")
            .unwrap_err(),
        ScoringJobError::InvalidReference
    );
    scoring_job
        .record_success(lease.fencing_token(), "scoring_result_alpha")
        .unwrap();
    assert_eq!(scoring_job.state(), ScoringJobState::Completed);
    assert_eq!(scoring_job.result_ref(), Some("scoring_result_alpha"));
    assert!(scoring_job.active_lease().is_none());

    scoring_job
        .record_success(lease.fencing_token(), "scoring_result_alpha")
        .unwrap();
    assert_eq!(
        scoring_job
            .record_success(lease.fencing_token() + 1, "scoring_result_alpha")
            .unwrap_err(),
        ScoringJobError::ConflictingCompletion
    );
    assert_eq!(
        scoring_job
            .record_success(lease.fencing_token(), "scoring_result_beta")
            .unwrap_err(),
        ScoringJobError::ConflictingCompletion
    );
    assert_eq!(
        scoring_job
            .record_retryable_failure(lease.fencing_token(), "late_failure", 21_000, 22_000)
            .unwrap_err(),
        ScoringJobError::NotLeased
    );
    assert_eq!(scoring_job.cancel().unwrap_err(), ScoringJobError::TerminalState);
}

#[test]
fn lease_expiry_retries_then_quarantines_when_attempts_are_exhausted() {
    let mut scoring_job = job(2);
    let first = scoring_job
        .claim("worker_alpha", "lease_alpha", 10_000, 20_000)
        .unwrap();
    assert_eq!(
        scoring_job.expire_lease(0).unwrap_err(),
        ScoringJobError::InvalidTimestamp
    );
    assert_eq!(
        scoring_job.expire_lease(19_999).unwrap_err(),
        ScoringJobError::LeaseStillActive
    );
    assert_eq!(
        scoring_job.expire_lease(20_000).unwrap(),
        ScoringJobState::RetryScheduled
    );
    assert_eq!(scoring_job.next_attempt_at_unix_ms(), Some(20_000));
    assert_eq!(scoring_job.last_failure_code(), Some("lease_expired"));
    assert_eq!(
        scoring_job
            .record_success(first.fencing_token(), "scoring_result_stale")
            .unwrap_err(),
        ScoringJobError::NotLeased
    );

    let second = scoring_job
        .claim("worker_beta", "lease_beta", 20_000, 30_000)
        .unwrap();
    assert_eq!(second.fencing_token(), 2);
    assert_eq!(
        scoring_job.expire_lease(30_000).unwrap(),
        ScoringJobState::Quarantined
    );
    assert_eq!(scoring_job.state(), ScoringJobState::Quarantined);
    assert_eq!(scoring_job.last_failure_code(), Some("lease_expired"));
    assert_eq!(
        scoring_job
            .claim("worker_gamma", "lease_gamma", 31_000, 40_000)
            .unwrap_err(),
        ScoringJobError::NotLeaseable
    );
}

#[test]
fn permanent_failure_and_cancellation_are_terminal_and_fence_active_work() {
    let mut failed_job = job(3);
    let lease = failed_job
        .claim("worker_alpha", "lease_alpha", 10_000, 20_000)
        .unwrap();
    assert_eq!(
        failed_job
            .record_permanent_failure(lease.fencing_token() + 1, "invalid_contract")
            .unwrap_err(),
        ScoringJobError::StaleLease
    );
    assert_eq!(
        failed_job
            .record_permanent_failure(lease.fencing_token(), "1e5")
            .unwrap_err(),
        ScoringJobError::InvalidReference
    );
    failed_job
        .record_permanent_failure(lease.fencing_token(), "invalid_contract")
        .unwrap();
    assert_eq!(failed_job.state(), ScoringJobState::Quarantined);
    assert_eq!(failed_job.last_failure_code(), Some("invalid_contract"));
    assert!(failed_job.active_lease().is_none());
    assert_eq!(
        failed_job
            .record_permanent_failure(lease.fencing_token(), "invalid_contract")
            .unwrap_err(),
        ScoringJobError::NotLeased
    );

    let mut cancelled_job = job(3);
    let active = cancelled_job
        .claim("worker_alpha", "lease_cancel", 10_000, 20_000)
        .unwrap();
    cancelled_job.cancel().unwrap();
    assert_eq!(cancelled_job.state(), ScoringJobState::Cancelled);
    assert!(cancelled_job.active_lease().is_none());
    assert_eq!(
        cancelled_job
            .record_success(active.fencing_token(), "scoring_result_late")
            .unwrap_err(),
        ScoringJobError::NotLeased
    );
    cancelled_job.cancel().unwrap();
    assert_eq!(
        failed_job.cancel().unwrap_err(),
        ScoringJobError::TerminalState
    );

    let mut queued_job = job(1);
    queued_job.cancel().unwrap();
    assert_eq!(queued_job.state(), ScoringJobState::Cancelled);
}

#[test]
fn error_messages_are_stable_for_operator_classification() {
    let expectations = [
        (ScoringJobError::InvalidReference, "scoring job references must be opaque non-numeric values"),
        (ScoringJobError::InvalidAttemptLimit, "scoring job maximum attempts must be greater than zero"),
        (ScoringJobError::InvalidTimestamp, "scoring job timestamps must be greater than zero"),
        (ScoringJobError::InvalidLeaseWindow, "scoring lease expiry must be later than claim time"),
        (ScoringJobError::InvalidRetryWindow, "scoring retry time must not precede failure time"),
        (ScoringJobError::LeaseNotDue, "scoring job retry is not due yet"),
        (ScoringJobError::NotLeaseable, "scoring job is not available for a new lease"),
        (ScoringJobError::NotLeased, "scoring job has no active lease"),
        (ScoringJobError::StaleLease, "scoring job lease fencing token is stale"),
        (ScoringJobError::LeaseStillActive, "scoring job lease has not expired"),
        (ScoringJobError::ConflictingCompletion, "scoring job already completed with different result evidence"),
        (ScoringJobError::TerminalState, "scoring job terminal state cannot accept this command"),
    ];
    for (error, message) in expectations {
        assert_eq!(error.to_string(), message);
        assert!(std::error::Error::source(&error).is_none());
    }
}
