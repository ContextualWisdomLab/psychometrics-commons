//! Verifies that invalid or stale retry operations fail closed in real `PostgreSQL`.
//! The tests submit missing jobs, stale or non-leased jobs, out-of-range fencing tokens,
//! and non-leaseable claims and assert that the adapter returns the matching safe error.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::postgres_scoring_job::{
    apply_scoring_job_migration, claim_scoring_job, persist_scoring_job,
    record_retryable_scoring_failure, ScoringJobPersistenceError,
};
use psychometrics_commons_runtime::scoring_job::ScoringJob;
use std::mem::discriminant;

const DATABASE_TEST_LOCK_KEY: i64 = 0x5343_5254_5259_4552;

fn test_client(schema: &str) -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let mut client = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    client
        .batch_execute(&format!(
            "CREATE SCHEMA IF NOT EXISTS {schema};\
             SET search_path TO {schema};\
             DROP TABLE IF EXISTS scoring_job_state;",
        ))
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
fn fixed_schema_serialization_is_database_visible_and_bounded() {
    let mut guard = test_client("scoring_job_retry_fixture_lock_contract_test");
    let timeout_ms: i64 = guard
        .query_one(
            "SELECT setting::bigint FROM pg_settings WHERE name = 'lock_timeout'",
            &[],
        )
        .expect("fixture lock wait budget should be queryable")
        .get(0);

    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let mut contender = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    let acquired: bool = contender
        .query_one(
            "SELECT pg_try_advisory_lock($1)",
            &[&DATABASE_TEST_LOCK_KEY],
        )
        .expect("cross-process fixture lock should be observable from PostgreSQL")
        .get(0);
    if acquired {
        contender
            .query_one("SELECT pg_advisory_unlock($1)", &[&DATABASE_TEST_LOCK_KEY])
            .expect("RED probe lock should be released after observation");
    }

    assert_eq!(
        timeout_ms, 60_000,
        "fixture lock acquisition must have a finite sixty-second PostgreSQL lock timeout"
    );
    assert!(
        !acquired,
        "fixed retry-error schemas must be serialized by a PostgreSQL-visible lease"
    );
}

#[test]
fn missing_job_and_oversized_fence_fail_closed() {
    let mut client = test_client("scoring_job_retry_missing_paths_test");

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
    let mut retry_client = test_client("scoring_job_retry_not_leased_paths_test");
    persist_and_claim(&mut retry_client, "scoring_job_retry_not_leased", 3);

    {
        let mut transaction = retry_client.transaction().unwrap();
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
        let mut transaction = retry_client.transaction().unwrap();
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

    let mut quarantined_client = test_client("scoring_job_retry_quarantine_paths_test");
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
