//! Real `PostgreSQL` coverage for retry persistence rejection paths.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::postgres_scoring_job::{
    apply_scoring_job_migration, claim_scoring_job, persist_scoring_job,
    record_retryable_scoring_failure, ScoringJobPersistenceError,
};
use psychometrics_commons_runtime::scoring_job::ScoringJob;
use std::mem::discriminant;

fn test_client() -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let mut client = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    client
        .batch_execute(
            "CREATE SCHEMA IF NOT EXISTS scoring_job_retry_error_paths_test;\
             SET search_path TO scoring_job_retry_error_paths_test;\
             DROP TABLE IF EXISTS scoring_job_state;",
        )
        .unwrap();
    apply_scoring_job_migration(&mut client).unwrap();
    client
}

fn persist_and_claim(client: &mut Client, job_ref: &str, max_attempts: u32) {
    let request_ref = format!("request_{job_ref}");
    let job = ScoringJob::new(job_ref, request_ref, max_attempts).unwrap();
    let mut transaction = client.transaction().unwrap();
    persist_scoring_job(&mut transaction, &job).unwrap();
    claim_scoring_job(
        &mut transaction,
        job_ref,
        "worker_error_path",
        "scoring_lease_error_path",
        10_000,
        11_000,
    )
    .unwrap();
    transaction.commit().unwrap();
}

#[test]
fn missing_job_and_oversized_fence_fail_closed() {
    let mut client = test_client();

    {
        let mut transaction = client.transaction().unwrap();
        assert!(matches!(
            record_retryable_scoring_failure(
                &mut transaction,
                "missing_scoring_job",
                1,
                "provider_timeout",
                10_500,
                12_000,
            ),
            Err(ScoringJobPersistenceError::JobNotFound)
        ));
        transaction.rollback().unwrap();
    }

    let oversized_fence = u64::try_from(i64::MAX).unwrap() + 1;
    let mut transaction = client.transaction().unwrap();
    let error = record_retryable_scoring_failure(
        &mut transaction,
        "missing_scoring_job",
        oversized_fence,
        "provider_timeout",
        10_500,
        12_000,
    )
    .unwrap_err();
    assert_eq!(
        discriminant(&error),
        discriminant(&ScoringJobPersistenceError::ValueOutOfRange)
    );
    transaction.rollback().unwrap();
}

#[test]
fn already_transitioned_jobs_reject_stale_failure_and_new_claims() {
    let mut client = test_client();
    persist_and_claim(&mut client, "scoring_job_retry_not_leased", 3);

    {
        let mut transaction = client.transaction().unwrap();
        record_retryable_scoring_failure(
            &mut transaction,
            "scoring_job_retry_not_leased",
            1,
            "provider_timeout",
            10_500,
            12_000,
        )
        .unwrap();
        transaction.commit().unwrap();
    }

    {
        let mut transaction = client.transaction().unwrap();
        assert!(matches!(
            record_retryable_scoring_failure(
                &mut transaction,
                "scoring_job_retry_not_leased",
                1,
                "duplicate_failure",
                10_600,
                12_100,
            ),
            Err(ScoringJobPersistenceError::NotLeased)
        ));
        transaction.rollback().unwrap();
    }

    let mut quarantined_client = test_client();
    persist_and_claim(&mut quarantined_client, "scoring_job_quarantined", 1);
    {
        let mut transaction = quarantined_client.transaction().unwrap();
        record_retryable_scoring_failure(
            &mut transaction,
            "scoring_job_quarantined",
            1,
            "permanent_retry_exhaustion",
            10_500,
            12_000,
        )
        .unwrap();
        transaction.commit().unwrap();
    }

    {
        let mut transaction = quarantined_client.transaction().unwrap();
        assert!(matches!(
            record_retryable_scoring_failure(
                &mut transaction,
                "scoring_job_quarantined",
                1,
                "duplicate_failure",
                10_600,
                12_100,
            ),
            Err(ScoringJobPersistenceError::NotLeased)
        ));
        transaction.rollback().unwrap();
    }

    let mut transaction = quarantined_client.transaction().unwrap();
    assert!(matches!(
        claim_scoring_job(
            &mut transaction,
            "scoring_job_quarantined",
            "worker_after_quarantine",
            "scoring_lease_after_quarantine",
            12_000,
            13_000,
        ),
        Err(ScoringJobPersistenceError::NotLeaseable)
    ));
    transaction.rollback().unwrap();
}
