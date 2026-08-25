//! Real `PostgreSQL` contract: a worker claims the next due scoring job.

use postgres::{error::SqlState, Client, IsolationLevel, NoTls};
use psychometrics_commons_runtime::postgres_scoring_job::{
    apply_scoring_job_migration, claim_next_scoring_job, claim_scoring_job, persist_scoring_job,
    record_retryable_scoring_failure, ScoringJobPersistenceError,
};
use psychometrics_commons_runtime::scoring_job::{ScoringJob, ScoringJobState};
use std::sync::{Arc, Barrier};
use std::thread;

const DATABASE_TEST_LOCK_KEY: i64 = 0x5343_4F52_434E_584C;

fn claim_next_test_guard() -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let mut client = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    client
        .query_one("SELECT set_config('lock_timeout', $1, false)", &[&"60s"])
        .expect("PostgreSQL lock timeout must be configurable for the scoring claim-next fixture");
    client
        .query_one("SELECT pg_advisory_lock($1)", &[&DATABASE_TEST_LOCK_KEY])
        .expect("shared PostgreSQL scoring claim-next lock should be acquired");
    client
}

fn test_client() -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let mut client = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    client
        .batch_execute(
            "CREATE SCHEMA IF NOT EXISTS scoring_job_claim_next_test;\
             SET search_path TO scoring_job_claim_next_test;",
        )
        .unwrap();
    client
}

fn reset_scoring_job_table(client: &mut Client) {
    client
        .batch_execute("DROP TABLE IF EXISTS scoring_job_claim_next_test.scoring_job_state;")
        .unwrap();
}

fn persist_queued(client: &mut Client, job_ref: &str, request_ref: &str) {
    let job = ScoringJob::new(job_ref, request_ref, 3).unwrap();
    let mut transaction = client.transaction().unwrap();
    persist_scoring_job(&mut transaction, &job).unwrap();
    transaction.commit().unwrap();
}

fn job_state(client: &mut Client, job_ref: &str) -> String {
    client
        .query_one(
            "SELECT scoring_state FROM scoring_job_state WHERE scoring_job_ref = $1",
            &[&job_ref],
        )
        .unwrap()
        .get(0)
}

#[test]
fn fixed_schema_serialization_must_be_visible_to_other_database_sessions() {
    let _guard = claim_next_test_guard();
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
            .expect("RED fixture lock should be released after probing");
    }
    assert!(
        !acquired,
        "a process-local mutex cannot serialize a fixed PostgreSQL schema across CI processes"
    );
}

#[test]
fn claim_next_fixture_lock_wait_is_bounded_by_live_postgresql_behavior() {
    let mut guard = claim_next_test_guard();
    let timeout_ms: i64 = guard
        .query_one(
            "SELECT setting::bigint FROM pg_settings WHERE name = 'lock_timeout'",
            &[],
        )
        .expect("claim-next fixture lock timeout should be queryable from PostgreSQL")
        .get(0);
    assert_eq!(timeout_ms, 60_000);

    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let mut contender = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    contender
        .query_one("SELECT set_config('lock_timeout', $1, false)", &[&"100ms"])
        .expect("claim-next contender lock timeout should be configurable");
    let error = contender
        .query_one("SELECT pg_advisory_lock($1)", &[&DATABASE_TEST_LOCK_KEY])
        .expect_err("contended claim-next fixture lock must stop at its configured timeout");
    assert_eq!(error.code(), Some(&SqlState::LOCK_NOT_AVAILABLE));
}

#[test]
fn claim_next_selects_the_oldest_due_queued_job_and_leaves_the_newer_queued() {
    let _guard = claim_next_test_guard();
    let mut client = test_client();
    reset_scoring_job_table(&mut client);
    apply_scoring_job_migration(&mut client).unwrap();
    persist_queued(
        &mut client,
        "scoring_job_claim_next_older",
        "scoring_request_claim_next_older",
    );
    persist_queued(
        &mut client,
        "scoring_job_claim_next_newer",
        "scoring_request_claim_next_newer",
    );

    let mut transaction = client.transaction().unwrap();
    let claimed = claim_next_scoring_job(
        &mut transaction,
        "worker_claim_next_alpha",
        "lease_claim_next_alpha",
        10_000,
        30_000,
    )
    .unwrap()
    .expect("a due queued job must be claimable");
    transaction.commit().unwrap();

    assert_eq!(claimed.scoring_job_ref(), "scoring_job_claim_next_older");
    assert_eq!(
        claimed.scoring_request_ref(),
        "scoring_request_claim_next_older"
    );
    assert_eq!(claimed.lease().worker_ref(), "worker_claim_next_alpha");
    assert_eq!(claimed.lease().lease_ref(), "lease_claim_next_alpha");
    assert_eq!(claimed.lease().fencing_token(), 1);
    assert_eq!(claimed.lease().expires_at_unix_ms(), 30_000);
    assert_eq!(
        job_state(&mut client, "scoring_job_claim_next_older"),
        "leased"
    );
    assert_eq!(
        job_state(&mut client, "scoring_job_claim_next_newer"),
        "queued"
    );
}

#[test]
fn claim_next_returns_none_when_no_job_is_due() {
    let _guard = claim_next_test_guard();
    let mut client = test_client();
    reset_scoring_job_table(&mut client);
    apply_scoring_job_migration(&mut client).unwrap();

    let mut transaction = client.transaction().unwrap();
    assert!(claim_next_scoring_job(
        &mut transaction,
        "worker_claim_next_empty",
        "lease_claim_next_empty",
        10_000,
        30_000,
    )
    .unwrap()
    .is_none());
    transaction.commit().unwrap();
}

#[test]
fn claim_next_skips_a_retry_that_is_not_due_and_claims_the_queued_job() {
    let _guard = claim_next_test_guard();
    let mut client = test_client();
    reset_scoring_job_table(&mut client);
    apply_scoring_job_migration(&mut client).unwrap();
    persist_queued(
        &mut client,
        "scoring_job_claim_next_retry",
        "scoring_request_claim_next_retry",
    );
    {
        let mut transaction = client.transaction().unwrap();
        let lease = claim_scoring_job(
            &mut transaction,
            "scoring_job_claim_next_retry",
            "worker_claim_next_setup",
            "lease_claim_next_setup",
            10_000,
            30_000,
        )
        .unwrap();
        assert_eq!(
            record_retryable_scoring_failure(
                &mut transaction,
                "scoring_job_claim_next_retry",
                lease.fencing_token(),
                "provider_timeout",
                20_000,
                50_000,
            )
            .unwrap(),
            ScoringJobState::RetryScheduled
        );
        transaction.commit().unwrap();
    }
    persist_queued(
        &mut client,
        "scoring_job_claim_next_ready",
        "scoring_request_claim_next_ready",
    );

    let mut transaction = client.transaction().unwrap();
    let claimed = claim_next_scoring_job(
        &mut transaction,
        "worker_claim_next_beta",
        "lease_claim_next_beta",
        20_000,
        40_000,
    )
    .unwrap()
    .expect("the queued job must be claimed while the retry is not due");
    transaction.commit().unwrap();

    assert_eq!(claimed.scoring_job_ref(), "scoring_job_claim_next_ready");
    assert_eq!(
        claimed.scoring_request_ref(),
        "scoring_request_claim_next_ready"
    );
    assert_eq!(
        job_state(&mut client, "scoring_job_claim_next_retry"),
        "retry_scheduled"
    );
    assert_eq!(
        job_state(&mut client, "scoring_job_claim_next_ready"),
        "leased"
    );
}

#[test]
fn concurrent_claim_next_workers_receive_distinct_due_jobs() {
    let _guard = claim_next_test_guard();
    let mut setup_client = test_client();
    reset_scoring_job_table(&mut setup_client);
    apply_scoring_job_migration(&mut setup_client).unwrap();
    persist_queued(
        &mut setup_client,
        "scoring_job_claim_next_left",
        "scoring_request_claim_next_left",
    );
    persist_queued(
        &mut setup_client,
        "scoring_job_claim_next_right",
        "scoring_request_claim_next_right",
    );

    let barrier = Arc::new(Barrier::new(2));
    let handles = [
        ("worker_claim_next_left", "lease_claim_next_left"),
        ("worker_claim_next_right", "lease_claim_next_right"),
    ]
    .into_iter()
    .map(|(worker_ref, lease_ref)| {
        let barrier = Arc::clone(&barrier);
        thread::spawn(move || {
            let mut client = test_client();
            barrier.wait();
            let mut transaction = client.transaction().unwrap();
            match claim_next_scoring_job(&mut transaction, worker_ref, lease_ref, 20_000, 40_000) {
                Ok(Some(claimed)) => {
                    let job_ref = claimed.scoring_job_ref().to_owned();
                    transaction.commit().unwrap();
                    Some(job_ref)
                }
                Ok(None) => {
                    transaction.rollback().unwrap();
                    None
                }
                Err(error) => panic!("unexpected concurrent claim-next error: {error:?}"),
            }
        })
    })
    .collect::<Vec<_>>();

    let mut claimed_jobs = handles
        .into_iter()
        .filter_map(|handle| handle.join().unwrap())
        .collect::<Vec<_>>();
    claimed_jobs.sort();
    assert_eq!(
        claimed_jobs,
        vec![
            "scoring_job_claim_next_left".to_owned(),
            "scoring_job_claim_next_right".to_owned()
        ]
    );
}

#[test]
fn invalid_claim_next_evidence_fails_before_selecting_a_job() {
    let _guard = claim_next_test_guard();
    let mut client = test_client();
    reset_scoring_job_table(&mut client);
    apply_scoring_job_migration(&mut client).unwrap();
    persist_queued(
        &mut client,
        "scoring_job_claim_next_invalid",
        "scoring_request_claim_next_invalid",
    );

    for (worker_ref, lease_ref, claimed_at, expires_at, expected) in [
        (
            "123",
            "lease_claim_next_invalid",
            10_000,
            30_000,
            ScoringJobPersistenceError::InvalidReference,
        ),
        (
            "worker_claim_next_invalid",
            "123",
            10_000,
            30_000,
            ScoringJobPersistenceError::InvalidReference,
        ),
        (
            "worker_claim_next_invalid",
            "lease_claim_next_invalid",
            0,
            30_000,
            ScoringJobPersistenceError::InvalidTimestamp,
        ),
        (
            "worker_claim_next_invalid",
            "lease_claim_next_invalid",
            10_000,
            10_000,
            ScoringJobPersistenceError::InvalidLeaseWindow,
        ),
    ] {
        let mut transaction = client.transaction().unwrap();
        assert!(matches!(
            claim_next_scoring_job(
                &mut transaction,
                worker_ref,
                lease_ref,
                claimed_at,
                expires_at,
            ),
            Err(error) if std::mem::discriminant(&error) == std::mem::discriminant(&expected)
        ));
        transaction.rollback().unwrap();
    }
    assert_eq!(
        job_state(&mut client, "scoring_job_claim_next_invalid"),
        "queued"
    );

    let mut transaction = client
        .build_transaction()
        .isolation_level(IsolationLevel::Serializable)
        .start()
        .unwrap();
    assert!(matches!(
        claim_next_scoring_job(
            &mut transaction,
            "worker_claim_next_serializable",
            "lease_claim_next_serializable",
            10_000,
            30_000,
        ),
        Err(ScoringJobPersistenceError::UnsupportedIsolationLevel)
    ));
    transaction.rollback().unwrap();
    assert_eq!(
        job_state(&mut client, "scoring_job_claim_next_invalid"),
        "queued"
    );
}
