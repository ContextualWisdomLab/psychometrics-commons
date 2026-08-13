//! Expired leased scoring jobs recover to a due retry or quarantine.

use postgres::{Client, IsolationLevel, NoTls};
use psychometrics_commons_runtime::postgres_scoring_job::{
    apply_scoring_job_migration, claim_scoring_job, expire_scoring_lease, persist_scoring_job,
    ScoringJobPersistenceError,
};
use psychometrics_commons_runtime::scoring_job::{ScoringJob, ScoringJobState};
use std::sync::{Mutex, MutexGuard};

static SCORING_JOB_EXPIRY_TEST_LOCK: Mutex<()> = Mutex::new(());

fn scoring_job_expiry_test_guard() -> MutexGuard<'static, ()> {
    SCORING_JOB_EXPIRY_TEST_LOCK
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
            "CREATE SCHEMA IF NOT EXISTS scoring_job_expiry_recovery_test;\
             SET search_path TO scoring_job_expiry_recovery_test;",
        )
        .unwrap();
    client
}

fn reset_scoring_job_table(client: &mut Client) {
    client
        .batch_execute("DROP TABLE IF EXISTS scoring_job_expiry_recovery_test.scoring_job_state;")
        .unwrap();
}

fn persist_and_claim(client: &mut Client, job_ref: &str, max_attempts: u32, expires_at: u64) {
    let job = ScoringJob::new(job_ref, "scoring_request_expiry", max_attempts).unwrap();
    let mut transaction = client.transaction().unwrap();
    persist_scoring_job(&mut transaction, &job).unwrap();
    claim_scoring_job(
        &mut transaction,
        job_ref,
        "worker_expired",
        "scoring_lease_expired",
        10_000,
        expires_at,
    )
    .unwrap();
    transaction.commit().unwrap();
}

#[test]
fn expired_lease_recovers_to_due_retry_and_reclaim_issues_next_fence() {
    let _guard = scoring_job_expiry_test_guard();
    let mut client = test_client();
    reset_scoring_job_table(&mut client);
    apply_scoring_job_migration(&mut client).unwrap();
    persist_and_claim(&mut client, "scoring_job_expired_retry", 3, 11_000);

    {
        let mut transaction = client.transaction().unwrap();
        assert_eq!(
            expire_scoring_lease(&mut transaction, "scoring_job_expired_retry", 11_000).unwrap(),
            ScoringJobState::RetryScheduled
        );
        transaction.commit().unwrap();
    }

    let row = client
        .query_one(
            "SELECT scoring_state, last_failure_code, next_attempt_at_unix_ms, \
                    active_lease_ref, active_fencing_token \
             FROM scoring_job_state WHERE scoring_job_ref = $1",
            &[&"scoring_job_expired_retry"],
        )
        .unwrap();
    let state: String = row.get(0);
    let cause: String = row.get(1);
    let due: i64 = row.get(2);
    let lease_ref: Option<String> = row.get(3);
    let fence: Option<i64> = row.get(4);
    assert_eq!(
        (state.as_str(), cause.as_str(), due, lease_ref, fence),
        ("retry_scheduled", "lease_expired", 11_000, None, None)
    );

    let mut transaction = client.transaction().unwrap();
    let recovered = claim_scoring_job(
        &mut transaction,
        "scoring_job_expired_retry",
        "worker_recovered",
        "scoring_lease_recovered",
        11_000,
        12_000,
    )
    .unwrap();
    transaction.commit().unwrap();
    assert_eq!(recovered.fencing_token(), 2);
    assert_eq!(recovered.worker_ref(), "worker_recovered");
}

#[test]
fn expired_lease_with_exhausted_budget_quarantines() {
    let _guard = scoring_job_expiry_test_guard();
    let mut client = test_client();
    reset_scoring_job_table(&mut client);
    apply_scoring_job_migration(&mut client).unwrap();
    persist_and_claim(&mut client, "scoring_job_expired_quarantine", 1, 11_000);

    let mut transaction = client.transaction().unwrap();
    assert_eq!(
        expire_scoring_lease(&mut transaction, "scoring_job_expired_quarantine", 11_000).unwrap(),
        ScoringJobState::Quarantined
    );
    transaction.commit().unwrap();

    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        claim_scoring_job(
            &mut transaction,
            "scoring_job_expired_quarantine",
            "worker_late",
            "scoring_lease_late",
            12_000,
            13_000,
        ),
        Err(ScoringJobPersistenceError::NotLeaseable)
    ));
    transaction.rollback().unwrap();
}

#[test]
fn claim_does_not_silently_steal_an_expired_lease() {
    let _guard = scoring_job_expiry_test_guard();
    let mut client = test_client();
    reset_scoring_job_table(&mut client);
    apply_scoring_job_migration(&mut client).unwrap();
    persist_and_claim(&mut client, "scoring_job_unrecovered", 3, 11_000);

    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        claim_scoring_job(
            &mut transaction,
            "scoring_job_unrecovered",
            "worker_thief",
            "scoring_lease_thief",
            12_000,
            13_000,
        ),
        Err(ScoringJobPersistenceError::NotLeaseable)
    ));
    transaction.rollback().unwrap();
}

#[test]
fn unexpired_lease_and_missing_or_unleased_jobs_fail_closed() {
    let _guard = scoring_job_expiry_test_guard();
    let mut client = test_client();
    reset_scoring_job_table(&mut client);
    apply_scoring_job_migration(&mut client).unwrap();
    persist_and_claim(&mut client, "scoring_job_still_live", 3, 20_000);

    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        expire_scoring_lease(&mut transaction, "scoring_job_still_live", 19_999),
        Err(ScoringJobPersistenceError::LeaseStillActive)
    ));
    assert!(matches!(
        expire_scoring_lease(&mut transaction, "scoring_job_missing", 20_000),
        Err(ScoringJobPersistenceError::JobNotFound)
    ));
    transaction.rollback().unwrap();

    let queued = ScoringJob::new(
        "scoring_job_never_claimed",
        "scoring_request_never_claimed",
        3,
    )
    .unwrap();
    let mut transaction = client.transaction().unwrap();
    persist_scoring_job(&mut transaction, &queued).unwrap();
    assert!(matches!(
        expire_scoring_lease(&mut transaction, "scoring_job_never_claimed", 20_000),
        Err(ScoringJobPersistenceError::NotLeased)
    ));
    assert!(matches!(
        expire_scoring_lease(&mut transaction, " ", 20_000),
        Err(ScoringJobPersistenceError::InvalidReference)
    ));
    assert!(matches!(
        expire_scoring_lease(&mut transaction, "scoring_job_still_live", 0),
        Err(ScoringJobPersistenceError::InvalidTimestamp)
    ));
    transaction.rollback().unwrap();
}

#[test]
fn lease_expiry_recovery_requires_read_committed() {
    let _guard = scoring_job_expiry_test_guard();
    let mut client = test_client();
    reset_scoring_job_table(&mut client);
    apply_scoring_job_migration(&mut client).unwrap();
    persist_and_claim(&mut client, "scoring_job_serializable", 3, 11_000);

    let mut transaction = client
        .build_transaction()
        .isolation_level(IsolationLevel::Serializable)
        .start()
        .unwrap();
    assert!(matches!(
        expire_scoring_lease(&mut transaction, "scoring_job_serializable", 11_000),
        Err(ScoringJobPersistenceError::UnsupportedIsolationLevel)
    ));
    transaction.rollback().unwrap();
}

#[test]
fn expiry_classify_select_failure_is_a_database_failure() {
    let _guard = scoring_job_expiry_test_guard();
    let mut client = test_client();
    reset_scoring_job_table(&mut client);
    apply_scoring_job_migration(&mut client).unwrap();
    persist_and_claim(&mut client, "scoring_job_classify_hidden", 3, 11_000);
    let sink = format!("scoring_job_expiry_classify_sink_{}", std::process::id());
    client
        .batch_execute(&format!(
            "CREATE SCHEMA {sink};\
             CREATE OR REPLACE FUNCTION scoring_job_redirect_after_update() \
             RETURNS trigger LANGUAGE plpgsql AS $$ \
             BEGIN \
                 PERFORM set_config('search_path', '{sink}', false); \
                 RETURN NULL; \
             END $$; \
             CREATE TRIGGER scoring_job_redirect_after_update \
             AFTER UPDATE ON scoring_job_state \
             FOR EACH STATEMENT EXECUTE FUNCTION scoring_job_redirect_after_update();"
        ))
        .unwrap();

    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        expire_scoring_lease(&mut transaction, "scoring_job_missing_after_update", 11_000),
        Err(ScoringJobPersistenceError::Database(_))
    ));
    transaction.rollback().unwrap();
}

#[test]
fn missing_scoring_job_relation_is_a_database_failure() {
    let _guard = scoring_job_expiry_test_guard();
    let mut client = test_client();
    reset_scoring_job_table(&mut client);

    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        expire_scoring_lease(&mut transaction, "scoring_job_missing_table", 11_000),
        Err(ScoringJobPersistenceError::Database(_))
    ));
    transaction.rollback().unwrap();
}
