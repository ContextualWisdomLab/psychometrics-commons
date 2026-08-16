//! Real `PostgreSQL` contract: claim the oldest due queued or retry-scheduled job.
//!
//! A buyer who finishes an assessment must not wait for an operator to name the
//! job. Two workers must not take the same row. A retry that is not yet due must
//! not starve an older queued job, and an empty queue must not invent work.

use postgres::{Client, IsolationLevel, NoTls};
use psychometrics_commons_runtime::postgres_scoring_job::{
    apply_scoring_job_migration, cancel_scoring_job, claim_next_due_scoring_job, claim_scoring_job,
    persist_scoring_job, record_retryable_scoring_failure, ScoringJobPersistenceError,
};
use psychometrics_commons_runtime::scoring_job::{ScoringJob, ScoringJobState};
use std::sync::{Arc, Barrier, Mutex, MutexGuard};
use std::thread;

static CLAIM_NEXT_TEST_LOCK: Mutex<()> = Mutex::new(());

fn claim_next_test_guard() -> MutexGuard<'static, ()> {
    CLAIM_NEXT_TEST_LOCK
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
        .batch_execute("DROP TABLE IF EXISTS scoring_job_claim_next_test.scoring_job_state;")
        .unwrap();
    apply_scoring_job_migration(client).unwrap();
}

fn persist_queued(client: &mut Client, job_ref: &str, request_ref: &str) {
    let job = ScoringJob::new(job_ref, request_ref, 3).unwrap();
    let mut transaction = client.transaction().unwrap();
    persist_scoring_job(&mut transaction, &job).unwrap();
    transaction.commit().unwrap();
}

fn persist_due_retry(client: &mut Client, job_ref: &str, request_ref: &str) {
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
            12_000,
        )
        .unwrap(),
        ScoringJobState::RetryScheduled
    );
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
fn empty_queue_returns_no_claimed_job() {
    let _guard = claim_next_test_guard();
    let mut client = test_client();
    reset_and_migrate(&mut client);

    let mut transaction = client.transaction().unwrap();
    assert_eq!(
        claim_next_due_scoring_job(
            &mut transaction,
            "worker_claim_next_empty",
            "lease_claim_next_empty",
            20_000,
            30_000,
        )
        .unwrap(),
        None
    );
    transaction.commit().unwrap();
}

#[test]
fn oldest_queued_job_is_claimed_with_its_stored_request() {
    let _guard = claim_next_test_guard();
    let mut client = test_client();
    reset_and_migrate(&mut client);
    persist_queued(
        &mut client,
        "scoring_job_claim_next_alpha",
        "scoring_request_claim_next_alpha",
    );
    persist_queued(
        &mut client,
        "scoring_job_claim_next_beta",
        "scoring_request_claim_next_beta",
    );

    let mut transaction = client.transaction().unwrap();
    let claimed = claim_next_due_scoring_job(
        &mut transaction,
        "worker_claim_next_oldest",
        "lease_claim_next_oldest",
        20_000,
        30_000,
    )
    .unwrap()
    .expect("oldest queued job must be claimed");
    transaction.commit().unwrap();

    assert_eq!(claimed.scoring_job_ref(), "scoring_job_claim_next_alpha");
    assert_eq!(
        claimed.scoring_request_ref(),
        "scoring_request_claim_next_alpha"
    );
    assert_eq!(claimed.lease().worker_ref(), "worker_claim_next_oldest");
    assert_eq!(claimed.lease().lease_ref(), "lease_claim_next_oldest");
    assert_eq!(claimed.fencing_token(), 1);
    assert_eq!(claimed.lease().expires_at_unix_ms(), 30_000);
    assert_eq!(
        job_state(&mut client, "scoring_job_claim_next_alpha"),
        "leased"
    );
    assert_eq!(
        job_state(&mut client, "scoring_job_claim_next_beta"),
        "queued"
    );
}

#[test]
fn not_due_retry_is_skipped_for_a_queued_job() {
    let _guard = claim_next_test_guard();
    let mut client = test_client();
    reset_and_migrate(&mut client);
    persist_due_retry(
        &mut client,
        "scoring_job_claim_next_retry",
        "scoring_request_claim_next_retry",
    );
    persist_queued(
        &mut client,
        "scoring_job_claim_next_ready",
        "scoring_request_claim_next_ready",
    );

    let mut transaction = client.transaction().unwrap();
    let claimed = claim_next_due_scoring_job(
        &mut transaction,
        "worker_claim_next_skip",
        "lease_claim_next_skip",
        11_000,
        21_000,
    )
    .unwrap()
    .expect("queued job must be claimed while the retry is not due");
    transaction.commit().unwrap();

    assert_eq!(claimed.scoring_job_ref(), "scoring_job_claim_next_ready");
    assert_eq!(
        job_state(&mut client, "scoring_job_claim_next_retry"),
        "retry_scheduled"
    );
}

#[test]
fn due_retry_is_claimed_when_it_is_the_oldest_due_row() {
    let _guard = claim_next_test_guard();
    let mut client = test_client();
    reset_and_migrate(&mut client);
    persist_due_retry(
        &mut client,
        "scoring_job_claim_next_due",
        "scoring_request_claim_next_due",
    );

    let mut transaction = client.transaction().unwrap();
    let claimed = claim_next_due_scoring_job(
        &mut transaction,
        "worker_claim_next_due",
        "lease_claim_next_due",
        12_000,
        22_000,
    )
    .unwrap()
    .expect("due retry must be claimable");
    transaction.commit().unwrap();

    assert_eq!(claimed.scoring_job_ref(), "scoring_job_claim_next_due");
    assert_eq!(claimed.fencing_token(), 2);
    assert_eq!(
        job_state(&mut client, "scoring_job_claim_next_due"),
        "leased"
    );
}

#[test]
fn future_retry_alone_returns_no_claimed_job() {
    let _guard = claim_next_test_guard();
    let mut client = test_client();
    reset_and_migrate(&mut client);
    persist_due_retry(
        &mut client,
        "scoring_job_claim_next_future",
        "scoring_request_claim_next_future",
    );

    let mut transaction = client.transaction().unwrap();
    assert_eq!(
        claim_next_due_scoring_job(
            &mut transaction,
            "worker_claim_next_future",
            "lease_claim_next_future",
            11_000,
            21_000,
        )
        .unwrap(),
        None
    );
    transaction.commit().unwrap();
    assert_eq!(
        job_state(&mut client, "scoring_job_claim_next_future"),
        "retry_scheduled"
    );
}

#[test]
fn leased_cancelled_and_quarantined_rows_are_not_selected() {
    let _guard = claim_next_test_guard();
    let mut client = test_client();
    reset_and_migrate(&mut client);
    persist_queued(
        &mut client,
        "scoring_job_claim_next_leased",
        "scoring_request_claim_next_leased",
    );
    persist_queued(
        &mut client,
        "scoring_job_claim_next_cancelled",
        "scoring_request_claim_next_cancelled",
    );
    let quarantined = ScoringJob::new(
        "scoring_job_claim_next_quarantined",
        "scoring_request_claim_next_quarantined",
        1,
    )
    .unwrap();
    let mut seed = client.transaction().unwrap();
    persist_scoring_job(&mut seed, &quarantined).unwrap();
    seed.commit().unwrap();
    persist_queued(
        &mut client,
        "scoring_job_claim_next_open",
        "scoring_request_claim_next_open",
    );

    let mut transaction = client.transaction().unwrap();
    claim_scoring_job(
        &mut transaction,
        "scoring_job_claim_next_leased",
        "worker_claim_next_hold",
        "lease_claim_next_hold",
        10_000,
        30_000,
    )
    .unwrap();
    cancel_scoring_job(&mut transaction, "scoring_job_claim_next_cancelled").unwrap();
    claim_scoring_job(
        &mut transaction,
        "scoring_job_claim_next_quarantined",
        "worker_claim_next_budget",
        "lease_claim_next_budget",
        10_000,
        11_000,
    )
    .unwrap();
    assert_eq!(
        record_retryable_scoring_failure(
            &mut transaction,
            "scoring_job_claim_next_quarantined",
            1,
            "engine_unavailable",
            10_500,
            12_000,
        )
        .unwrap(),
        ScoringJobState::Quarantined
    );
    transaction.commit().unwrap();

    let mut transaction = client.transaction().unwrap();
    let claimed = claim_next_due_scoring_job(
        &mut transaction,
        "worker_claim_next_filter",
        "lease_claim_next_filter",
        20_000,
        30_000,
    )
    .unwrap()
    .expect("only the remaining queued job must be claimed");
    transaction.commit().unwrap();

    assert_eq!(claimed.scoring_job_ref(), "scoring_job_claim_next_open");
    assert_eq!(
        job_state(&mut client, "scoring_job_claim_next_leased"),
        "leased"
    );
    assert_eq!(
        job_state(&mut client, "scoring_job_claim_next_cancelled"),
        "cancelled"
    );
    assert_eq!(
        job_state(&mut client, "scoring_job_claim_next_quarantined"),
        "quarantined"
    );
}

#[test]
fn invalid_worker_identity_fails_closed_without_a_lease() {
    let _guard = claim_next_test_guard();
    let mut client = test_client();
    reset_and_migrate(&mut client);
    persist_queued(
        &mut client,
        "scoring_job_claim_next_invalid",
        "scoring_request_claim_next_invalid",
    );

    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        claim_next_due_scoring_job(
            &mut transaction,
            "123",
            "lease_claim_next_invalid",
            20_000,
            30_000,
        ),
        Err(ScoringJobPersistenceError::InvalidReference)
    ));
    transaction.rollback().unwrap();
    assert_eq!(
        job_state(&mut client, "scoring_job_claim_next_invalid"),
        "queued"
    );
}

#[test]
fn zero_claim_time_fails_closed_without_a_lease() {
    let _guard = claim_next_test_guard();
    let mut client = test_client();
    reset_and_migrate(&mut client);
    persist_queued(
        &mut client,
        "scoring_job_claim_next_clock",
        "scoring_request_claim_next_clock",
    );

    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        claim_next_due_scoring_job(
            &mut transaction,
            "worker_claim_next_clock",
            "lease_claim_next_clock",
            0,
            30_000,
        ),
        Err(ScoringJobPersistenceError::InvalidTimestamp)
    ));
    transaction.rollback().unwrap();
    assert_eq!(
        job_state(&mut client, "scoring_job_claim_next_clock"),
        "queued"
    );
}

#[test]
fn empty_lease_window_fails_closed_without_a_lease() {
    let _guard = claim_next_test_guard();
    let mut client = test_client();
    reset_and_migrate(&mut client);
    persist_queued(
        &mut client,
        "scoring_job_claim_next_window",
        "scoring_request_claim_next_window",
    );

    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        claim_next_due_scoring_job(
            &mut transaction,
            "worker_claim_next_window",
            "lease_claim_next_window",
            20_000,
            20_000,
        ),
        Err(ScoringJobPersistenceError::InvalidLeaseWindow)
    ));
    transaction.rollback().unwrap();
    assert_eq!(
        job_state(&mut client, "scoring_job_claim_next_window"),
        "queued"
    );
}

#[test]
fn stronger_isolation_is_rejected_without_mutating_due_jobs() {
    let _guard = claim_next_test_guard();
    let mut client = test_client();
    reset_and_migrate(&mut client);
    persist_queued(
        &mut client,
        "scoring_job_claim_next_isolation",
        "scoring_request_claim_next_isolation",
    );

    let mut transaction = client
        .build_transaction()
        .isolation_level(IsolationLevel::Serializable)
        .start()
        .unwrap();
    assert!(matches!(
        claim_next_due_scoring_job(
            &mut transaction,
            "worker_claim_next_isolation",
            "lease_claim_next_isolation",
            20_000,
            30_000,
        ),
        Err(ScoringJobPersistenceError::UnsupportedIsolationLevel)
    ));
    transaction.rollback().unwrap();
    assert_eq!(
        job_state(&mut client, "scoring_job_claim_next_isolation"),
        "queued"
    );
}

#[test]
fn two_workers_claim_different_due_jobs() {
    let _guard = claim_next_test_guard();
    let mut setup = test_client();
    reset_and_migrate(&mut setup);
    persist_queued(
        &mut setup,
        "scoring_job_claim_next_left",
        "scoring_request_claim_next_left",
    );
    persist_queued(
        &mut setup,
        "scoring_job_claim_next_right",
        "scoring_request_claim_next_right",
    );

    let barrier = Arc::new(Barrier::new(2));
    let first_barrier = Arc::clone(&barrier);
    let first = thread::spawn(move || {
        let mut client = test_client();
        first_barrier.wait();
        let mut transaction = client.transaction().unwrap();
        let claimed = claim_next_due_scoring_job(
            &mut transaction,
            "worker_claim_next_left",
            "lease_claim_next_left",
            20_000,
            30_000,
        )
        .unwrap();
        transaction.commit().unwrap();
        claimed.map(|row| row.scoring_job_ref().to_owned())
    });
    let second_barrier = Arc::clone(&barrier);
    let second = thread::spawn(move || {
        let mut client = test_client();
        second_barrier.wait();
        let mut transaction = client.transaction().unwrap();
        let claimed = claim_next_due_scoring_job(
            &mut transaction,
            "worker_claim_next_right",
            "lease_claim_next_right",
            20_000,
            30_000,
        )
        .unwrap();
        transaction.commit().unwrap();
        claimed.map(|row| row.scoring_job_ref().to_owned())
    });

    let mut claimed: Vec<String> = [first.join().unwrap(), second.join().unwrap()]
        .into_iter()
        .flatten()
        .collect();
    claimed.sort();
    assert_eq!(
        claimed,
        vec![
            "scoring_job_claim_next_left".to_owned(),
            "scoring_job_claim_next_right".to_owned()
        ]
    );
}
