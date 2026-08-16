//! Real `PostgreSQL` contract: claim-next leases the oldest due job and returns
//! the stored request pin so a worker cannot name a different scoring request.

use postgres::{Client, IsolationLevel, NoTls};
use psychometrics_commons_runtime::postgres_scoring_job::{
    apply_scoring_job_migration, claim_next_scoring_job, claim_scoring_job, persist_scoring_job,
    record_retryable_scoring_failure, ScoringJobPersistenceError,
};
use psychometrics_commons_runtime::scoring_job::{ScoringJob, ScoringJobState};
use std::mem::discriminant;
use std::sync::{Arc, Barrier, Mutex, MutexGuard};
use std::thread;

static CLAIM_NEXT_LOCK: Mutex<()> = Mutex::new(());

fn claim_next_guard() -> MutexGuard<'static, ()> {
    CLAIM_NEXT_LOCK
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
            "CREATE SCHEMA IF NOT EXISTS scoring_job_claim_next_test;\
             SET search_path TO scoring_job_claim_next_test;",
        )
        .unwrap();
    client
}

fn reset_and_migrate(client: &mut Client) {
    client
        .batch_execute("DROP TABLE IF EXISTS scoring_job_state;")
        .unwrap();
    apply_scoring_job_migration(client).unwrap();
}

fn persist_queued(client: &mut Client, job_ref: &str, request_ref: &str) {
    let job = ScoringJob::new(job_ref, request_ref, 3).unwrap();
    let mut transaction = client.transaction().unwrap();
    persist_scoring_job(&mut transaction, &job).unwrap();
    transaction.commit().unwrap();
}

fn persist_due_retry(client: &mut Client, job_ref: &str, request_ref: &str, retry_at_unix_ms: u64) {
    persist_queued(client, job_ref, request_ref);
    let mut transaction = client.transaction().unwrap();
    claim_scoring_job(
        &mut transaction,
        job_ref,
        "worker_claim_next_seed",
        "lease_claim_next_seed",
        10_000,
        11_000,
    )
    .unwrap();
    assert_eq!(
        record_retryable_scoring_failure(
            &mut transaction,
            job_ref,
            1,
            "engine_unavailable",
            10_500,
            retry_at_unix_ms,
        )
        .unwrap(),
        ScoringJobState::RetryScheduled
    );
    transaction.commit().unwrap();
}

#[test]
fn claim_next_returns_the_oldest_due_job_and_its_stored_request_pin() {
    let _guard = claim_next_guard();
    let mut client = test_client();
    reset_and_migrate(&mut client);
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
        20_000,
        30_000,
    )
    .unwrap();
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

    let state: String = client
        .query_one(
            "SELECT scoring_state FROM scoring_job_state WHERE scoring_job_ref = $1",
            &[&"scoring_job_claim_next_older"],
        )
        .unwrap()
        .get(0);
    assert_eq!(state, "leased");
}

#[test]
fn claim_next_skips_a_retry_that_is_not_due_yet() {
    let _guard = claim_next_guard();
    let mut client = test_client();
    reset_and_migrate(&mut client);
    persist_due_retry(
        &mut client,
        "scoring_job_claim_next_early",
        "scoring_request_claim_next_early",
        50_000,
    );
    persist_queued(
        &mut client,
        "scoring_job_claim_next_ready",
        "scoring_request_claim_next_ready",
    );

    let mut transaction = client.transaction().unwrap();
    let claimed = claim_next_scoring_job(
        &mut transaction,
        "worker_claim_next_ready",
        "lease_claim_next_ready",
        20_000,
        30_000,
    )
    .unwrap();
    transaction.commit().unwrap();

    assert_eq!(claimed.scoring_job_ref(), "scoring_job_claim_next_ready");
    assert_eq!(
        claimed.scoring_request_ref(),
        "scoring_request_claim_next_ready"
    );
}

#[test]
fn claim_next_leases_a_due_retry_using_the_stored_request_pin() {
    let _guard = claim_next_guard();
    let mut client = test_client();
    reset_and_migrate(&mut client);
    persist_due_retry(
        &mut client,
        "scoring_job_claim_next_retry",
        "scoring_request_claim_next_retry",
        20_000,
    );

    let mut transaction = client.transaction().unwrap();
    let claimed = claim_next_scoring_job(
        &mut transaction,
        "worker_claim_next_retry",
        "lease_claim_next_retry",
        20_000,
        30_000,
    )
    .unwrap();
    transaction.commit().unwrap();

    assert_eq!(claimed.scoring_job_ref(), "scoring_job_claim_next_retry");
    assert_eq!(
        claimed.scoring_request_ref(),
        "scoring_request_claim_next_retry"
    );
    assert_eq!(claimed.lease().fencing_token(), 2);
}

#[test]
fn claim_next_returns_no_due_job_when_the_queue_is_empty_or_not_due() {
    let _guard = claim_next_guard();
    let mut client = test_client();
    reset_and_migrate(&mut client);

    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        claim_next_scoring_job(
            &mut transaction,
            "worker_claim_next_empty",
            "lease_claim_next_empty",
            20_000,
            30_000,
        ),
        Err(ScoringJobPersistenceError::NoDueJob)
    ));
    transaction.rollback().unwrap();

    persist_due_retry(
        &mut client,
        "scoring_job_claim_next_waiting",
        "scoring_request_claim_next_waiting",
        50_000,
    );
    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        claim_next_scoring_job(
            &mut transaction,
            "worker_claim_next_waiting",
            "lease_claim_next_waiting",
            20_000,
            30_000,
        ),
        Err(ScoringJobPersistenceError::NoDueJob)
    ));
    transaction.rollback().unwrap();
}

#[test]
fn invalid_claim_next_evidence_fails_before_a_lease_is_issued() {
    let _guard = claim_next_guard();
    let mut client = test_client();
    reset_and_migrate(&mut client);
    persist_queued(
        &mut client,
        "scoring_job_claim_next_invalid",
        "scoring_request_claim_next_invalid",
    );

    for (worker_ref, lease_ref, claimed_at, expires_at, expected) in [
        (
            "123",
            "lease_claim_next_invalid",
            20_000,
            30_000,
            ScoringJobPersistenceError::InvalidReference,
        ),
        (
            "worker_claim_next_invalid",
            "123",
            20_000,
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
            u64::MAX,
            u64::MAX,
            ScoringJobPersistenceError::ValueOutOfRange,
        ),
        (
            "worker_claim_next_invalid",
            "lease_claim_next_invalid",
            20_000,
            20_000,
            ScoringJobPersistenceError::InvalidLeaseWindow,
        ),
    ] {
        let mut transaction = client.transaction().unwrap();
        let error = claim_next_scoring_job(
            &mut transaction,
            worker_ref,
            lease_ref,
            claimed_at,
            expires_at,
        )
        .unwrap_err();
        assert_eq!(discriminant(&error), discriminant(&expected));
        transaction.rollback().unwrap();
    }

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
            20_000,
            30_000,
        ),
        Err(ScoringJobPersistenceError::UnsupportedIsolationLevel)
    ));
    transaction.rollback().unwrap();
}

#[test]
fn concurrent_claim_next_workers_receive_exactly_one_lease() {
    let _guard = claim_next_guard();
    let mut setup_client = test_client();
    reset_and_migrate(&mut setup_client);
    persist_queued(
        &mut setup_client,
        "scoring_job_claim_next_race",
        "scoring_request_claim_next_race",
    );

    let barrier = Arc::new(Barrier::new(2));
    let handles = [
        (
            "worker_claim_next_race_alpha",
            "lease_claim_next_race_alpha",
        ),
        ("worker_claim_next_race_beta", "lease_claim_next_race_beta"),
    ]
    .into_iter()
    .map(|(worker_ref, lease_ref)| {
        let barrier = Arc::clone(&barrier);
        thread::spawn(move || {
            let mut client = test_client();
            barrier.wait();
            let mut transaction = client.transaction().unwrap();
            match claim_next_scoring_job(&mut transaction, worker_ref, lease_ref, 20_000, 30_000) {
                Ok(claimed) => {
                    let evidence = (
                        claimed.scoring_job_ref().to_owned(),
                        claimed.scoring_request_ref().to_owned(),
                        claimed.lease().fencing_token(),
                    );
                    transaction.commit().unwrap();
                    Some(evidence)
                }
                Err(ScoringJobPersistenceError::NoDueJob) => {
                    transaction.rollback().unwrap();
                    None
                }
                Err(error) => panic!("unexpected concurrent claim-next error: {error:?}"),
            }
        })
    })
    .collect::<Vec<_>>();

    let outcomes = handles
        .into_iter()
        .map(|handle| handle.join().expect("claim-next worker thread"))
        .collect::<Vec<_>>();
    let winners = outcomes.into_iter().flatten().collect::<Vec<_>>();
    assert_eq!(winners.len(), 1);
    assert_eq!(winners[0].0, "scoring_job_claim_next_race");
    assert_eq!(winners[0].1, "scoring_request_claim_next_race");
    assert_eq!(winners[0].2, 1);
}
