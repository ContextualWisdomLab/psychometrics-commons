//! Exact-reference contract tests for asynchronous scoring-job evidence.

use psychometrics_commons_runtime::scoring_job::{ScoringJob, ScoringJobError};

#[test]
fn scoring_job_identity_entry_points_reject_padded_aliases() {
    assert_eq!(
        ScoringJob::new(" scoring_job_alpha", "scoring_request_alpha", 3).unwrap_err(),
        ScoringJobError::InvalidReference
    );
    assert_eq!(
        ScoringJob::new("scoring_job_alpha", "scoring_request_alpha\u{00a0}", 3).unwrap_err(),
        ScoringJobError::InvalidReference
    );

    let mut job = ScoringJob::new("scoring_job_alpha", "scoring_request_alpha", 3).unwrap();
    assert_eq!(
        job.claim("\u{2003}worker_alpha", "lease_alpha", 10_000, 20_000)
            .unwrap_err(),
        ScoringJobError::InvalidReference
    );
    assert_eq!(
        job.claim("worker_alpha", "lease_alpha\u{202f}", 10_000, 20_000)
            .unwrap_err(),
        ScoringJobError::InvalidReference
    );

    let lease = job
        .claim("worker_alpha", "lease_alpha", 10_000, 20_000)
        .unwrap();
    assert_eq!(
        job.record_retryable_failure(
            lease.fencing_token(),
            "\u{3000}timeout_error",
            11_000,
            12_000,
        )
        .unwrap_err(),
        ScoringJobError::InvalidReference
    );
    assert_eq!(
        job.record_permanent_failure(lease.fencing_token(), "fatal_error ", 11_000)
            .unwrap_err(),
        ScoringJobError::InvalidReference
    );
    assert_eq!(
        job.record_success(
            lease.fencing_token(),
            " scoring_result_alpha",
            11_000,
        )
        .unwrap_err(),
        ScoringJobError::InvalidReference
    );
}
