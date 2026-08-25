//! Verifies that two workers cannot both reclaim the same due retry in real `PostgreSQL`.
//! Two concurrent claim transactions race on one retry-scheduled job; exactly one receives
//! the next lease and fencing token while the other is rejected without duplicate ownership.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::postgres_scoring_job::{
    apply_scoring_job_migration, claim_scoring_job, persist_scoring_job,
    record_retryable_scoring_failure, ScoringJobPersistenceError,
};
use psychometrics_commons_runtime::scoring_job::{ScoringJob, ScoringJobState};
use std::sync::{Arc, Barrier};
use std::thread;

const DATABASE_TEST_LOCK_KEY: i64 = 0x5343_4F52_5254_5259;

type ClaimEvidence = (String, String, u64);

fn retry_concurrency_test_guard() -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let mut client = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    client
        .batch_execute("SET lock_timeout = '60s'")
        .expect("shared PostgreSQL scoring retry lock wait must be bounded");
    client
        .query_one("SELECT pg_advisory_lock($1)", &[&DATABASE_TEST_LOCK_KEY])
        .expect("shared PostgreSQL scoring retry lock should be acquired");
    client
}

fn test_client() -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let mut client = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    client
        .batch_execute(
            "CREATE SCHEMA IF NOT EXISTS scoring_job_retry_concurrency_test;\
             SET search_path TO scoring_job_retry_concurrency_test;",
        )
        .unwrap();
    client
}

fn reset_scoring_job_table(client: &mut Client) {
    client
        .batch_execute("DROP TABLE IF EXISTS scoring_job_retry_concurrency_test.scoring_job_state;")
        .unwrap();
}

fn persist_due_retry(client: &mut Client) {
    let job = ScoringJob::new("scoring_job_retry_race", "scoring_request_retry_race", 3).unwrap();
    let mut transaction = client.transaction().unwrap();
    persist_scoring_job(&mut transaction, &job).unwrap();
    claim_scoring_job(
        &mut transaction,
        "scoring_job_retry_race",
        "worker_initial_race",
        "scoring_lease_initial_race",
        10_000,
        11_000,
    )
    .unwrap();
    assert_eq!(
        record_retryable_scoring_failure(
            &mut transaction,
            "scoring_job_retry_race",
            1,
            "provider_timeout_race",
            10_500,
            12_000,
        )
        .unwrap(),
        ScoringJobState::RetryScheduled
    );
    transaction.commit().unwrap();
}

fn claim_due_retry(
    barrier: &Arc<Barrier>,
    worker_ref: &'static str,
    lease_ref: &'static str,
) -> Option<ClaimEvidence> {
    let mut client = test_client();
    barrier.wait();
    let mut transaction = client.transaction().unwrap();
    match claim_scoring_job(
        &mut transaction,
        "scoring_job_retry_race",
        worker_ref,
        lease_ref,
        12_000,
        13_000,
    ) {
        Ok(lease) => {
            let evidence = (
                lease.worker_ref().to_owned(),
                lease.lease_ref().to_owned(),
                lease.fencing_token(),
            );
            transaction.commit().unwrap();
            Some(evidence)
        }
        Err(ScoringJobPersistenceError::NotLeaseable) => {
            transaction.rollback().unwrap();
            None
        }
        Err(error) => panic!("unexpected concurrent retry claim error: {error:?}"),
    }
}

#[test]
fn fixed_schema_serialization_must_be_visible_to_other_database_sessions() {
    let _guard = retry_concurrency_test_guard();
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
fn concurrent_due_retry_claimers_receive_exactly_one_second_fence() {
    let _guard = retry_concurrency_test_guard();
    let mut setup_client = test_client();
    reset_scoring_job_table(&mut setup_client);
    apply_scoring_job_migration(&mut setup_client).unwrap();
    persist_due_retry(&mut setup_client);

    let barrier = Arc::new(Barrier::new(2));
    let handles = [
        ("worker_retry_alpha", "scoring_lease_retry_alpha"),
        ("worker_retry_beta", "scoring_lease_retry_beta"),
    ]
    .into_iter()
    .map(|(worker_ref, lease_ref)| {
        let barrier = Arc::clone(&barrier);
        thread::spawn(move || claim_due_retry(&barrier, worker_ref, lease_ref))
    })
    .collect::<Vec<_>>();

    let successful_claims = handles
        .into_iter()
        .filter_map(|handle| handle.join().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(successful_claims.len(), 1);
    assert_eq!(successful_claims[0].2, 2);
    assert!(matches!(
        successful_claims[0].0.as_str(),
        "worker_retry_alpha" | "worker_retry_beta"
    ));
    assert!(matches!(
        successful_claims[0].1.as_str(),
        "scoring_lease_retry_alpha" | "scoring_lease_retry_beta"
    ));

    let row = setup_client
        .query_one(
            "SELECT scoring_state, attempt_count, active_fencing_token,\
                    active_worker_ref, active_lease_ref, next_attempt_at_unix_ms \
             FROM scoring_job_state WHERE scoring_job_ref = 'scoring_job_retry_race'",
            &[],
        )
        .unwrap();
    assert_eq!(row.get::<_, String>(0), "leased");
    assert_eq!(row.get::<_, i32>(1), 2);
    assert_eq!(row.get::<_, Option<i64>>(2), Some(2));
    assert_eq!(
        row.get::<_, Option<String>>(3).as_deref(),
        Some(successful_claims[0].0.as_str())
    );
    assert_eq!(
        row.get::<_, Option<String>>(4).as_deref(),
        Some(successful_claims[0].1.as_str())
    );
    assert_eq!(row.get::<_, Option<i64>>(5), None);
}
