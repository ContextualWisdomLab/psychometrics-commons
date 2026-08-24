//! Real `PostgreSQL` locking evidence for scoring-job fallback classification.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::postgres_scoring_job::{
    apply_scoring_job_migration, cancel_scoring_job, claim_scoring_job, expire_scoring_lease,
    persist_scoring_job, record_successful_scoring_completion, ScoringJobPersistenceError,
};
use psychometrics_commons_runtime::scoring_job::ScoringJob;
use std::sync::{Mutex, MutexGuard};

const SCHEMA: &str = "scoring_job_classification_locking_test";
const DATABASE_TEST_LOCK_KEY: i64 = 0x5343_4C41_5353_4C4B;
static DATABASE_TEST_LOCK: Mutex<()> = Mutex::new(());

fn test_clients() -> (MutexGuard<'static, ()>, Client, Client) {
    let guard = DATABASE_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let mut owner = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    owner
        .batch_execute(&format!(
            "CREATE SCHEMA IF NOT EXISTS {SCHEMA}; SET search_path TO {SCHEMA};"
        ))
        .unwrap();
    let mut contender = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    contender
        .batch_execute(&format!("SET search_path TO {SCHEMA};"))
        .unwrap();
    (guard, owner, contender)
}

fn reset_scoring_job_table(client: &mut Client) {
    client
        .batch_execute(&format!("DROP TABLE IF EXISTS {SCHEMA}.scoring_job_state;"))
        .unwrap();
}

fn enqueue(client: &mut Client, job_ref: &str, request_ref: &str) {
    let job = ScoringJob::new(job_ref, request_ref, 3).unwrap();
    let mut transaction = client.transaction().unwrap();
    persist_scoring_job(&mut transaction, &job).unwrap();
    transaction.commit().unwrap();
}

fn complete(client: &mut Client, job_ref: &str) {
    let fence = {
        let mut transaction = client.transaction().unwrap();
        let lease = claim_scoring_job(
            &mut transaction,
            job_ref,
            "worker_classification_lock",
            "lease_classification_lock",
            10_000,
            20_000,
        )
        .unwrap();
        transaction.commit().unwrap();
        lease.fencing_token()
    };
    let mut transaction = client.transaction().unwrap();
    record_successful_scoring_completion(
        &mut transaction,
        job_ref,
        fence,
        "result_classification_lock",
        11_000,
    )
    .unwrap();
    transaction.commit().unwrap();
}

fn assert_row_update_waits_for_lock(contender: &mut Client, job_ref: &str) {
    contender
        .batch_execute("SET lock_timeout = '100ms';")
        .unwrap();
    let error = contender
        .execute(
            "UPDATE scoring_job_state
             SET updated_at = clock_timestamp()
             WHERE scoring_job_ref = $1",
            &[&job_ref],
        )
        .unwrap_err();
    assert_eq!(
        error.code().map(postgres::error::SqlState::code),
        Some("55P03")
    );
}

#[test]
fn fixture_lock_is_visible_across_database_sessions() {
    let (_guard, _owner, mut contender) = test_clients();
    let acquired: bool = contender
        .query_one(
            "SELECT pg_try_advisory_lock($1)",
            &[&DATABASE_TEST_LOCK_KEY],
        )
        .unwrap()
        .get(0);

    if acquired {
        contender
            .query_one(
                "SELECT pg_advisory_unlock($1)",
                &[&DATABASE_TEST_LOCK_KEY],
            )
            .unwrap();
    }

    assert!(
        !acquired,
        "fixture serialization must be enforced by PostgreSQL, not only by a process-local mutex"
    );
}

#[test]
fn cancellation_terminal_classification_locks_the_row_until_transaction_end() {
    let (_guard, mut owner, mut contender) = test_clients();
    reset_scoring_job_table(&mut owner);
    apply_scoring_job_migration(&mut owner).unwrap();
    enqueue(
        &mut owner,
        "scoring_job_cancel_classification_lock",
        "scoring_request_cancel_classification_lock",
    );
    complete(&mut owner, "scoring_job_cancel_classification_lock");

    let mut transaction = owner.transaction().unwrap();
    assert!(matches!(
        cancel_scoring_job(&mut transaction, "scoring_job_cancel_classification_lock"),
        Err(ScoringJobPersistenceError::TerminalState)
    ));
    assert_row_update_waits_for_lock(&mut contender, "scoring_job_cancel_classification_lock");
    transaction.rollback().unwrap();
}

#[test]
fn expiry_nonleased_classification_locks_the_row_until_transaction_end() {
    let (_guard, mut owner, mut contender) = test_clients();
    reset_scoring_job_table(&mut owner);
    apply_scoring_job_migration(&mut owner).unwrap();
    enqueue(
        &mut owner,
        "scoring_job_expiry_classification_lock",
        "scoring_request_expiry_classification_lock",
    );

    let mut transaction = owner.transaction().unwrap();
    assert!(matches!(
        expire_scoring_lease(
            &mut transaction,
            "scoring_job_expiry_classification_lock",
            10_000,
        ),
        Err(ScoringJobPersistenceError::NotLeased)
    ));
    assert_row_update_waits_for_lock(&mut contender, "scoring_job_expiry_classification_lock");
    transaction.rollback().unwrap();
}
