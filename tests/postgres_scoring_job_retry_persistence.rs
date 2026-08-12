//! Real `PostgreSQL` contract for durable retryable scoring failures and retry claims.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::postgres_scoring_job::{
    apply_scoring_job_migration, claim_scoring_job, persist_scoring_job,
    record_retryable_scoring_failure, ScoringJobPersistenceError,
};
use psychometrics_commons_runtime::scoring_job::{ScoringJob, ScoringJobState};
use std::mem::discriminant;
use std::sync::{Mutex, MutexGuard};

static SCORING_JOB_RETRY_TEST_LOCK: Mutex<()> = Mutex::new(());

fn scoring_job_retry_test_guard() -> MutexGuard<'static, ()> {
    SCORING_JOB_RETRY_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn test_client() -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let mut client = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    client
        .batch_execute(
            "CREATE SCHEMA IF NOT EXISTS scoring_job_retry_persistence_test;\
             SET search_path TO scoring_job_retry_persistence_test;",
        )
        .unwrap();
    client
}

fn reset_scoring_job_table(client: &mut Client) {
    client
        .batch_execute("DROP TABLE IF EXISTS scoring_job_retry_persistence_test.scoring_job_state;")
        .unwrap();
}

fn queued_job(job_ref: &str, request_ref: &str, max_attempts: u32) -> ScoringJob {
    ScoringJob::new(job_ref, request_ref, max_attempts).unwrap()
}

fn persist_and_claim(
    client: &mut Client,
    job_ref: &str,
    request_ref: &str,
    max_attempts: u32,
    expires_at_unix_ms: u64,
) {
    let job = queued_job(job_ref, request_ref, max_attempts);
    let mut transaction = client.transaction().unwrap();
    persist_scoring_job(&mut transaction, &job).unwrap();
    claim_scoring_job(
        &mut transaction,
        job_ref,
        "worker_initial",
        "scoring_lease_initial",
        10_000,
        expires_at_unix_ms,
    )
    .unwrap();
    transaction.commit().unwrap();
}

#[test]
fn retryable_failure_persists_due_retry_and_reclaim_issues_next_fence() {
    let _guard = scoring_job_retry_test_guard();
    let mut client = test_client();
    reset_scoring_job_table(&mut client);
    apply_scoring_job_migration(&mut client).unwrap();
    persist_and_claim(
        &mut client,
        "scoring_job_retry",
        "scoring_request_retry",
        3,
        11_000,
    );

    {
        let mut transaction = client.transaction().unwrap();
        assert_eq!(
            record_retryable_scoring_failure(
                &mut transaction,
                "scoring_job_retry",
                1,
                "provider_timeout",
                10_500,
                12_000,
            )
            .unwrap(),
            ScoringJobState::RetryScheduled
        );
        transaction.commit().unwrap();
    }

    let row = client
        .query_one(
            "SELECT scoring_state, attempt_count, next_attempt_at_unix_ms, last_failure_code,\
                    active_worker_ref, active_lease_ref, active_fencing_token,\
                    active_lease_expires_at_unix_ms \
             FROM scoring_job_state WHERE scoring_job_ref = 'scoring_job_retry'",
            &[],
        )
        .unwrap();
    assert_eq!(row.get::<_, String>(0), "retry_scheduled");
    assert_eq!(row.get::<_, i32>(1), 1);
    assert_eq!(row.get::<_, Option<i64>>(2), Some(12_000));
    assert_eq!(row.get::<_, Option<String>>(3).as_deref(), Some("provider_timeout"));
    assert_eq!(row.get::<_, Option<String>>(4), None);
    assert_eq!(row.get::<_, Option<String>>(5), None);
    assert_eq!(row.get::<_, Option<i64>>(6), None);
    assert_eq!(row.get::<_, Option<i64>>(7), None);

    {
        let mut transaction = client.transaction().unwrap();
        assert!(matches!(
            claim_scoring_job(
                &mut transaction,
                "scoring_job_retry",
                "worker_early",
                "scoring_lease_early",
                11_999,
                12_999,
            ),
            Err(ScoringJobPersistenceError::LeaseNotDue)
        ));
        transaction.rollback().unwrap();
    }

    let lease = {
        let mut transaction = client.transaction().unwrap();
        let lease = claim_scoring_job(
            &mut transaction,
            "scoring_job_retry",
            "worker_retry",
            "scoring_lease_retry",
            12_000,
            13_000,
        )
        .unwrap();
        transaction.commit().unwrap();
        lease
    };
    assert_eq!(lease.fencing_token(), 2);
    assert_eq!(lease.worker_ref(), "worker_retry");
    assert_eq!(lease.lease_ref(), "scoring_lease_retry");
}

#[test]
fn retryable_failure_exhausting_budget_quarantines_without_another_due_time() {
    let _guard = scoring_job_retry_test_guard();
    let mut client = test_client();
    reset_scoring_job_table(&mut client);
    apply_scoring_job_migration(&mut client).unwrap();
    persist_and_claim(
        &mut client,
        "scoring_job_exhausted",
        "scoring_request_exhausted",
        1,
        11_000,
    );

    let mut transaction = client.transaction().unwrap();
    assert_eq!(
        record_retryable_scoring_failure(
            &mut transaction,
            "scoring_job_exhausted",
            1,
            "provider_unavailable",
            10_500,
            12_000,
        )
        .unwrap(),
        ScoringJobState::Quarantined
    );
    transaction.commit().unwrap();

    let row = client
        .query_one(
            "SELECT scoring_state, attempt_count, next_attempt_at_unix_ms, last_failure_code,\
                    active_fencing_token \
             FROM scoring_job_state WHERE scoring_job_ref = 'scoring_job_exhausted'",
            &[],
        )
        .unwrap();
    assert_eq!(row.get::<_, String>(0), "quarantined");
    assert_eq!(row.get::<_, i32>(1), 1);
    assert_eq!(row.get::<_, Option<i64>>(2), None);
    assert_eq!(
        row.get::<_, Option<String>>(3).as_deref(),
        Some("provider_unavailable")
    );
    assert_eq!(row.get::<_, Option<i64>>(4), None);
}

#[test]
fn stale_or_expired_retry_failure_evidence_cannot_mutate_current_lease() {
    let _guard = scoring_job_retry_test_guard();
    let mut client = test_client();
    reset_scoring_job_table(&mut client);
    apply_scoring_job_migration(&mut client).unwrap();
    persist_and_claim(
        &mut client,
        "scoring_job_fenced",
        "scoring_request_fenced",
        3,
        11_000,
    );

    {
        let mut transaction = client.transaction().unwrap();
        assert!(matches!(
            record_retryable_scoring_failure(
                &mut transaction,
                "scoring_job_fenced",
                2,
                "stale_worker_failure",
                10_500,
                12_000,
            ),
            Err(ScoringJobPersistenceError::StaleLease)
        ));
        transaction.rollback().unwrap();
    }

    {
        let mut transaction = client.transaction().unwrap();
        assert!(matches!(
            record_retryable_scoring_failure(
                &mut transaction,
                "scoring_job_fenced",
                1,
                "late_worker_failure",
                11_000,
                12_000,
            ),
            Err(ScoringJobPersistenceError::LeaseExpired)
        ));
        transaction.rollback().unwrap();
    }

    let row = client
        .query_one(
            "SELECT scoring_state, attempt_count, active_worker_ref, active_lease_ref,\
                    active_fencing_token, active_lease_expires_at_unix_ms, last_failure_code \
             FROM scoring_job_state WHERE scoring_job_ref = 'scoring_job_fenced'",
            &[],
        )
        .unwrap();
    assert_eq!(row.get::<_, String>(0), "leased");
    assert_eq!(row.get::<_, i32>(1), 1);
    assert_eq!(
        row.get::<_, Option<String>>(2).as_deref(),
        Some("worker_initial")
    );
    assert_eq!(
        row.get::<_, Option<String>>(3).as_deref(),
        Some("scoring_lease_initial")
    );
    assert_eq!(row.get::<_, Option<i64>>(4), Some(1));
    assert_eq!(row.get::<_, Option<i64>>(5), Some(11_000));
    assert_eq!(row.get::<_, Option<String>>(6), None);
}

#[test]
fn invalid_retry_failure_evidence_fails_before_database_mutation() {
    let _guard = scoring_job_retry_test_guard();
    let mut client = test_client();
    reset_scoring_job_table(&mut client);
    apply_scoring_job_migration(&mut client).unwrap();
    persist_and_claim(
        &mut client,
        "scoring_job_invalid_retry",
        "scoring_request_invalid_retry",
        3,
        11_000,
    );

    for (token, cause_code, failed_at, retry_at, expected) in [
        (
            0,
            "provider_timeout",
            10_500,
            12_000,
            ScoringJobPersistenceError::InvalidFencingToken,
        ),
        (
            1,
            "123",
            10_500,
            12_000,
            ScoringJobPersistenceError::InvalidReference,
        ),
        (
            1,
            "provider_timeout",
            0,
            12_000,
            ScoringJobPersistenceError::InvalidTimestamp,
        ),
        (
            1,
            "provider_timeout",
            10_500,
            10_499,
            ScoringJobPersistenceError::InvalidRetryWindow,
        ),
    ] {
        let mut transaction = client.transaction().unwrap();
        let error = record_retryable_scoring_failure(
            &mut transaction,
            "scoring_job_invalid_retry",
            token,
            cause_code,
            failed_at,
            retry_at,
        )
        .unwrap_err();
        assert_eq!(discriminant(&error), discriminant(&expected));
        transaction.rollback().unwrap();
    }

    let row = client
        .query_one(
            "SELECT scoring_state, attempt_count, active_fencing_token, last_failure_code \
             FROM scoring_job_state WHERE scoring_job_ref = 'scoring_job_invalid_retry'",
            &[],
        )
        .unwrap();
    assert_eq!(row.get::<_, String>(0), "leased");
    assert_eq!(row.get::<_, i32>(1), 1);
    assert_eq!(row.get::<_, Option<i64>>(2), Some(1));
    assert_eq!(row.get::<_, Option<String>>(3), None);
}
