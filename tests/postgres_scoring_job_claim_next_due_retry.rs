//! Real `PostgreSQL` regression: claim-next resumes a due retry after provider timeout.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::postgres_scoring_job::{
    apply_scoring_job_migration, claim_next_scoring_job, claim_scoring_job, persist_scoring_job,
    record_retryable_scoring_failure,
};
use psychometrics_commons_runtime::scoring_job::{ScoringJob, ScoringJobState};

const SCORING_CLAIM_NEXT_DUE_RETRY_TEST_LOCK_KEY: i64 = 8_256_710_451_992_403;

fn test_client() -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let mut client = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    client
        .query_one(
            "SELECT pg_advisory_lock($1)",
            &[&SCORING_CLAIM_NEXT_DUE_RETRY_TEST_LOCK_KEY],
        )
        .expect("scoring due-retry fixture advisory lock should be acquired");
    client
        .batch_execute(
            "DROP SCHEMA IF EXISTS scoring_job_claim_next_due_retry_test CASCADE;\
             CREATE SCHEMA scoring_job_claim_next_due_retry_test;\
             SET search_path TO scoring_job_claim_next_due_retry_test;",
        )
        .unwrap();
    apply_scoring_job_migration(&mut client).unwrap();
    client
}

fn persist_queued(client: &mut Client, job_ref: &str, request_ref: &str) {
    let job = ScoringJob::new(job_ref, request_ref, 3).unwrap();
    let mut transaction = client.transaction().unwrap();
    persist_scoring_job(&mut transaction, &job).unwrap();
    transaction.commit().unwrap();
}

fn state_and_fence(client: &mut Client, job_ref: &str) -> (String, i64) {
    let row = client
        .query_one(
            "SELECT scoring_state, COALESCE(active_fencing_token, 0) \
             FROM scoring_job_state WHERE scoring_job_ref = $1",
            &[&job_ref],
        )
        .unwrap();
    (row.get(0), row.get(1))
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
            &[&SCORING_CLAIM_NEXT_DUE_RETRY_TEST_LOCK_KEY],
        )
        .unwrap()
        .get(0);
    if acquired {
        contender
            .execute(
                "SELECT pg_advisory_unlock($1)",
                &[&SCORING_CLAIM_NEXT_DUE_RETRY_TEST_LOCK_KEY],
            )
            .unwrap();
    }
    assert!(
        !acquired,
        "the fixed-schema fixture must serialize through a PostgreSQL-visible advisory lock"
    );
}

#[test]
fn due_retry_is_claimed_with_next_fence_before_newer_queued_work() {
    let mut client = test_client();
    persist_queued(
        &mut client,
        "scoring_job_due_retry",
        "scoring_request_due_retry",
    );

    {
        let mut transaction = client.transaction().unwrap();
        let first_lease = claim_scoring_job(
            &mut transaction,
            "scoring_job_due_retry",
            "worker_before_provider_timeout",
            "lease_before_provider_timeout",
            10_000,
            20_000,
        )
        .unwrap();
        assert_eq!(first_lease.fencing_token(), 1);
        assert_eq!(
            record_retryable_scoring_failure(
                &mut transaction,
                "scoring_job_due_retry",
                first_lease.fencing_token(),
                "provider_timeout",
                15_000,
                25_000,
            )
            .unwrap(),
            ScoringJobState::RetryScheduled
        );
        transaction.commit().unwrap();
    }

    persist_queued(
        &mut client,
        "scoring_job_newer_queued",
        "scoring_request_newer_queued",
    );

    let mut transaction = client.transaction().unwrap();
    let claimed = claim_next_scoring_job(
        &mut transaction,
        "worker_after_restart",
        "lease_after_restart",
        25_000,
        35_000,
    )
    .unwrap()
    .expect("the retry is due and must be resumed before the newer queued job");
    transaction.commit().unwrap();

    assert_eq!(claimed.scoring_job_ref(), "scoring_job_due_retry");
    assert_eq!(claimed.scoring_request_ref(), "scoring_request_due_retry");
    assert_eq!(claimed.lease().fencing_token(), 2);
    assert_eq!(claimed.lease().worker_ref(), "worker_after_restart");
    assert_eq!(claimed.lease().lease_ref(), "lease_after_restart");
    assert_eq!(claimed.lease().expires_at_unix_ms(), 35_000);
    assert_eq!(
        state_and_fence(&mut client, "scoring_job_due_retry"),
        ("leased".to_owned(), 2)
    );
    assert_eq!(
        state_and_fence(&mut client, "scoring_job_newer_queued"),
        ("queued".to_owned(), 0)
    );
}
