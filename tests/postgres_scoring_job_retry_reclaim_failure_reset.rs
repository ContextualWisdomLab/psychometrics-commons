//! Verifies that a job scheduled for retry can be claimed again after its due time.
//! The first attempt records a failure code; the second claim must start a new attempt
//! and clear that old attempt-local failure code while advancing the fencing token.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::postgres_scoring_job::{
    apply_scoring_job_migration, claim_scoring_job, persist_scoring_job,
    record_retryable_scoring_failure,
};
use psychometrics_commons_runtime::scoring_job::ScoringJob;

const SCORING_RETRY_RECLAIM_TEST_LOCK_KEY: i64 = 8_256_710_451_992_402;

fn test_client() -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let mut client = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    client
        .query_one(
            "SELECT pg_advisory_lock($1)",
            &[&SCORING_RETRY_RECLAIM_TEST_LOCK_KEY],
        )
        .expect("scoring retry-reclaim fixture advisory lock should be acquired");
    client
        .batch_execute(
            "CREATE SCHEMA IF NOT EXISTS scoring_job_retry_reclaim_failure_reset_test;\
             SET search_path TO scoring_job_retry_reclaim_failure_reset_test;\
             DROP TABLE IF EXISTS scoring_job_state;",
        )
        .unwrap();
    client
}

#[test]
fn fixture_serialization_is_visible_to_other_postgresql_sessions() {
    let _fixture = test_client();
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let mut contender = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    let acquired: bool = contender
        .query_one(
            "SELECT pg_try_advisory_lock($1)",
            &[&SCORING_RETRY_RECLAIM_TEST_LOCK_KEY],
        )
        .unwrap()
        .get(0);
    if acquired {
        contender
            .execute(
                "SELECT pg_advisory_unlock($1)",
                &[&SCORING_RETRY_RECLAIM_TEST_LOCK_KEY],
            )
            .unwrap();
    }
    assert!(
        !acquired,
        "the fixed-schema fixture must serialize through a PostgreSQL-visible advisory lock"
    );
}

#[test]
fn fixture_lock_wait_has_finite_postgresql_budget() {
    let mut fixture = test_client();
    let timeout_ms: i64 = fixture
        .query_one(
            "SELECT setting::bigint FROM pg_settings WHERE name = 'lock_timeout'",
            &[],
        )
        .expect("scoring retry-reclaim fixture lock timeout should be queryable from PostgreSQL")
        .get(0);

    assert_eq!(
        timeout_ms, 60_000,
        "scoring retry-reclaim fixture must not wait indefinitely for its PostgreSQL advisory lock"
    );
}

#[test]
fn retry_reclaim_clears_previous_attempt_failure_code() {
    let mut client = test_client();
    apply_scoring_job_migration(&mut client).unwrap();
    let job = ScoringJob::new(
        "scoring_job_failure_reset",
        "scoring_request_failure_reset",
        3,
    )
    .unwrap();

    {
        let mut transaction = client.transaction().unwrap();
        persist_scoring_job(&mut transaction, &job).unwrap();
        claim_scoring_job(
            &mut transaction,
            "scoring_job_failure_reset",
            "worker_first_attempt",
            "scoring_lease_first_attempt",
            10_000,
            11_000,
        )
        .unwrap();
        record_retryable_scoring_failure(
            &mut transaction,
            "scoring_job_failure_reset",
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
        claim_scoring_job(
            &mut transaction,
            "scoring_job_failure_reset",
            "worker_second_attempt",
            "scoring_lease_second_attempt",
            12_000,
            13_000,
        )
        .unwrap();
        transaction.commit().unwrap();
    }

    let row = client
        .query_one(
            "SELECT scoring_state, attempt_count, last_failure_code, active_fencing_token \
             FROM scoring_job_state WHERE scoring_job_ref = 'scoring_job_failure_reset'",
            &[],
        )
        .unwrap();
    assert_eq!(row.get::<_, String>(0), "leased");
    assert_eq!(row.get::<_, i32>(1), 2);
    assert_eq!(row.get::<_, Option<String>>(2), None);
    assert_eq!(row.get::<_, Option<i64>>(3), Some(2));
}
