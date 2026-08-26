//! Exact-spelling contracts for public scoring-job references.
//!
//! Scoring-job identities are opaque issued values. Leading or trailing
//! whitespace must be rejected rather than trimmed into an alias, while valid
//! multilingual references are preserved exactly.

use psychometrics_commons_runtime::scoring_job::{ScoringJob, ScoringJobError};

#[test]
fn scoring_job_creation_rejects_padded_identity_aliases() {
    assert_eq!(
        ScoringJob::new(" scoring_job_alpha", "scoring_request_alpha", 3).unwrap_err(),
        ScoringJobError::InvalidReference
    );
    assert_eq!(
        ScoringJob::new("scoring_job_alpha", "scoring_request_alpha ", 3).unwrap_err(),
        ScoringJobError::InvalidReference
    );

    let job = ScoringJob::new("채점_job_α", "요청_request_β", 3).unwrap();
    assert_eq!(job.scoring_job_ref(), "채점_job_α");
    assert_eq!(job.scoring_request_ref(), "요청_request_β");
}

#[test]
fn scoring_job_commands_reject_padded_evidence_aliases() {
    let mut claim_job = ScoringJob::new("scoring_job_claim", "scoring_request_claim", 3).unwrap();
    assert_eq!(
        claim_job
            .claim(" worker_alpha", "lease_alpha", 10_000, 20_000)
            .unwrap_err(),
        ScoringJobError::InvalidReference
    );
    assert_eq!(
        claim_job
            .claim("worker_alpha", "lease_alpha ", 10_000, 20_000)
            .unwrap_err(),
        ScoringJobError::InvalidReference
    );

    let lease = claim_job
        .claim("작업자_worker_α", "임대_lease_β", 10_000, 20_000)
        .unwrap();
    assert_eq!(lease.worker_ref(), "작업자_worker_α");
    assert_eq!(lease.lease_ref(), "임대_lease_β");
    assert_eq!(
        claim_job
            .record_retryable_failure(lease.fencing_token(), " timeout_error", 11_000, 15_000)
            .unwrap_err(),
        ScoringJobError::InvalidReference
    );

    let mut result_job =
        ScoringJob::new("scoring_job_result", "scoring_request_result", 2).unwrap();
    let result_lease = result_job
        .claim("worker_result", "lease_result", 10_000, 20_000)
        .unwrap();
    assert_eq!(
        result_job
            .record_success(
                result_lease.fencing_token(),
                "scoring_result_alpha ",
                11_000,
            )
            .unwrap_err(),
        ScoringJobError::InvalidReference
    );
    result_job
        .record_success(result_lease.fencing_token(), "결과_result_γ", 11_000)
        .unwrap();
    assert_eq!(result_job.result_ref(), Some("결과_result_γ"));
}
