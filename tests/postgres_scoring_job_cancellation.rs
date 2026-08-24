//! Real `PostgreSQL` contract for durable scoring-job cancellation.

use postgres::{Client, IsolationLevel, NoTls};
use psychometrics_commons_runtime::postgres_scoring_job::{
    apply_scoring_job_migration, cancel_scoring_job, claim_scoring_job, persist_scoring_job,
    record_permanent_scoring_failure, record_retryable_scoring_failure,
    record_successful_scoring_completion, ScoringJobCancellationDisposition,
    ScoringJobPersistenceError,
};
use psychometrics_commons_runtime::scoring_job::ScoringJob;

const SCHEMA: &str = "scoring_job_cancellation_test";
const DATABASE_TEST_LOCK_KEY: i64 = 0x5343_4F52_4341_4E43;

fn test_client() -> (Client, Client) {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let mut guard = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    guard
        .query_one("SELECT pg_advisory_lock($1)", &[&DATABASE_TEST_LOCK_KEY])
        .expect("shared PostgreSQL scoring cancellation lock should be acquired");
    let mut client = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    client
        .batch_execute(&format!(
            "CREATE SCHEMA IF NOT EXISTS {SCHEMA}; SET search_path TO {SCHEMA};"
        ))
        .unwrap();
    (guard, client)
}

fn reset_scoring_job_table(client: &mut Client) {
    client
        .batch_execute(&format!("DROP TABLE IF EXISTS {SCHEMA}.scoring_job_state;"))
        .unwrap();
}

fn enqueue(client: &mut Client, job_ref: &str, request_ref: &str, max_attempts: u32) {
    let job = ScoringJob::new(job_ref, request_ref, max_attempts).unwrap();
    let mut transaction = client.transaction().unwrap();
    persist_scoring_job(&mut transaction, &job).unwrap();
    transaction.commit().unwrap();
}

fn claim(client: &mut Client, job_ref: &str, worker_ref: &str, lease_ref: &str) -> u64 {
    let mut transaction = client.transaction().unwrap();
    let lease = claim_scoring_job(
        &mut transaction,
        job_ref,
        worker_ref,
        lease_ref,
        10_000,
        20_000,
    )
    .unwrap();
    transaction.commit().unwrap();
    lease.fencing_token()
}

#[test]
fn fixed_schema_serialization_must_be_visible_to_other_database_sessions() {
    let (_guard, _client) = test_client();
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
fn queued_cancellation_is_durable_and_exact_replay_is_idempotent() {
    let (_guard, mut client) = test_client();
    reset_scoring_job_table(&mut client);
    apply_scoring_job_migration(&mut client).unwrap();
    enqueue(
        &mut client,
        "scoring_job_cancel_queued",
        "scoring_request_cancel_queued",
        3,
    );

    let mut transaction = client.transaction().unwrap();
    assert_eq!(
        cancel_scoring_job(&mut transaction, "scoring_job_cancel_queued").unwrap(),
        ScoringJobCancellationDisposition::Cancelled
    );
    assert_eq!(
        cancel_scoring_job(&mut transaction, "scoring_job_cancel_queued").unwrap(),
        ScoringJobCancellationDisposition::Duplicate
    );
    transaction.commit().unwrap();

    let row = client
        .query_one(
            "SELECT scoring_state, attempt_count, next_attempt_at_unix_ms, active_worker_ref, \
                    active_lease_ref, active_fencing_token, active_lease_expires_at_unix_ms, \
                    result_ref, completed_fencing_token \
             FROM scoring_job_state WHERE scoring_job_ref = 'scoring_job_cancel_queued'",
            &[],
        )
        .unwrap();
    assert_eq!(row.get::<_, String>(0), "cancelled");
    assert_eq!(row.get::<_, i32>(1), 0);
    assert_eq!(row.get::<_, Option<i64>>(2), None);
    assert_eq!(row.get::<_, Option<String>>(3), None);
    assert_eq!(row.get::<_, Option<String>>(4), None);
    assert_eq!(row.get::<_, Option<i64>>(5), None);
    assert_eq!(row.get::<_, Option<i64>>(6), None);
    assert_eq!(row.get::<_, Option<String>>(7), None);
    assert_eq!(row.get::<_, Option<i64>>(8), None);
}

#[test]
fn leased_cancellation_invalidates_the_worker_fence() {
    let (_guard, mut client) = test_client();
    reset_scoring_job_table(&mut client);
    apply_scoring_job_migration(&mut client).unwrap();
    enqueue(
        &mut client,
        "scoring_job_cancel_leased",
        "scoring_request_cancel_leased",
        3,
    );
    let fence = claim(
        &mut client,
        "scoring_job_cancel_leased",
        "worker_cancel_leased",
        "scoring_lease_cancel_leased",
    );

    let mut transaction = client.transaction().unwrap();
    assert_eq!(
        cancel_scoring_job(&mut transaction, "scoring_job_cancel_leased").unwrap(),
        ScoringJobCancellationDisposition::Cancelled
    );
    transaction.commit().unwrap();

    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        record_successful_scoring_completion(
            &mut transaction,
            "scoring_job_cancel_leased",
            fence,
            "scoring_result_cancelled",
            11_000,
        ),
        Err(ScoringJobPersistenceError::NotLeased)
    ));
    transaction.rollback().unwrap();
}

#[test]
fn retry_scheduled_cancellation_clears_due_time_but_retains_failure_evidence() {
    let (_guard, mut client) = test_client();
    reset_scoring_job_table(&mut client);
    apply_scoring_job_migration(&mut client).unwrap();
    enqueue(
        &mut client,
        "scoring_job_cancel_retry",
        "scoring_request_cancel_retry",
        3,
    );
    let fence = claim(
        &mut client,
        "scoring_job_cancel_retry",
        "worker_cancel_retry",
        "scoring_lease_cancel_retry",
    );
    {
        let mut transaction = client.transaction().unwrap();
        record_retryable_scoring_failure(
            &mut transaction,
            "scoring_job_cancel_retry",
            fence,
            "provider_unavailable",
            11_000,
            12_000,
        )
        .unwrap();
        transaction.commit().unwrap();
    }

    let mut transaction = client.transaction().unwrap();
    assert_eq!(
        cancel_scoring_job(&mut transaction, "scoring_job_cancel_retry").unwrap(),
        ScoringJobCancellationDisposition::Cancelled
    );
    transaction.commit().unwrap();

    let row = client
        .query_one(
            "SELECT scoring_state, next_attempt_at_unix_ms, last_failure_code \
             FROM scoring_job_state WHERE scoring_job_ref = 'scoring_job_cancel_retry'",
            &[],
        )
        .unwrap();
    assert_eq!(row.get::<_, String>(0), "cancelled");
    assert_eq!(row.get::<_, Option<i64>>(1), None);
    assert_eq!(
        row.get::<_, Option<String>>(2).as_deref(),
        Some("provider_unavailable")
    );
}

#[test]
fn completed_and_quarantined_jobs_cannot_be_rewritten_as_cancelled() {
    let (_guard, mut client) = test_client();
    reset_scoring_job_table(&mut client);
    apply_scoring_job_migration(&mut client).unwrap();

    enqueue(
        &mut client,
        "scoring_job_cancel_completed",
        "scoring_request_cancel_completed",
        2,
    );
    let completed_fence = claim(
        &mut client,
        "scoring_job_cancel_completed",
        "worker_cancel_completed",
        "scoring_lease_cancel_completed",
    );
    {
        let mut transaction = client.transaction().unwrap();
        record_successful_scoring_completion(
            &mut transaction,
            "scoring_job_cancel_completed",
            completed_fence,
            "scoring_result_completed",
            11_000,
        )
        .unwrap();
        transaction.commit().unwrap();
    }

    enqueue(
        &mut client,
        "scoring_job_cancel_quarantined",
        "scoring_request_cancel_quarantined",
        2,
    );
    let quarantined_fence = claim(
        &mut client,
        "scoring_job_cancel_quarantined",
        "worker_cancel_quarantined",
        "scoring_lease_cancel_quarantined",
    );
    {
        let mut transaction = client.transaction().unwrap();
        record_permanent_scoring_failure(
            &mut transaction,
            "scoring_job_cancel_quarantined",
            quarantined_fence,
            "scientific_failure",
            11_000,
        )
        .unwrap();
        transaction.commit().unwrap();
    }

    for job_ref in [
        "scoring_job_cancel_completed",
        "scoring_job_cancel_quarantined",
    ] {
        let mut transaction = client.transaction().unwrap();
        assert!(matches!(
            cancel_scoring_job(&mut transaction, job_ref),
            Err(ScoringJobPersistenceError::TerminalState)
        ));
        transaction.rollback().unwrap();
    }
}

#[test]
fn cancellation_validates_identity_isolation_missing_rows_and_suppressed_transitions() {
    let (_guard, mut client) = test_client();
    reset_scoring_job_table(&mut client);
    apply_scoring_job_migration(&mut client).unwrap();
    enqueue(
        &mut client,
        "scoring_job_cancel_suppressed",
        "scoring_request_cancel_suppressed",
        2,
    );

    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        cancel_scoring_job(&mut transaction, "123"),
        Err(ScoringJobPersistenceError::InvalidReference)
    ));
    assert!(matches!(
        cancel_scoring_job(&mut transaction, "scoring_job_cancel_missing"),
        Err(ScoringJobPersistenceError::JobNotFound)
    ));
    transaction.rollback().unwrap();

    let mut transaction = client
        .build_transaction()
        .isolation_level(IsolationLevel::Serializable)
        .start()
        .unwrap();
    assert!(matches!(
        cancel_scoring_job(&mut transaction, "scoring_job_cancel_suppressed"),
        Err(ScoringJobPersistenceError::UnsupportedIsolationLevel)
    ));
    transaction.rollback().unwrap();

    client
        .batch_execute(
            "CREATE OR REPLACE FUNCTION suppress_scoring_job_cancel() \
             RETURNS trigger LANGUAGE plpgsql AS $$ \
             BEGIN \
                 RETURN NULL; \
             END $$; \
             CREATE TRIGGER suppress_scoring_job_cancel \
             BEFORE UPDATE ON scoring_job_state \
             FOR EACH ROW EXECUTE FUNCTION suppress_scoring_job_cancel();",
        )
        .unwrap();
    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        cancel_scoring_job(&mut transaction, "scoring_job_cancel_suppressed"),
        Err(ScoringJobPersistenceError::TransitionNotApplied)
    ));
    transaction.rollback().unwrap();
}

#[test]
fn cancellation_surfaces_database_failure_and_terminal_error_message_is_stable() {
    let (_guard, mut client) = test_client();
    reset_scoring_job_table(&mut client);
    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        cancel_scoring_job(&mut transaction, "scoring_job_cancel_no_table"),
        Err(ScoringJobPersistenceError::Database(_))
    ));
    transaction.rollback().unwrap();

    assert_eq!(
        ScoringJobPersistenceError::TerminalState.to_string(),
        "completed or quarantined scoring jobs cannot be cancelled"
    );
}

#[test]
fn cancellation_surfaces_database_failure_from_post_transition_state_lookup() {
    let (_guard, mut client) = test_client();
    reset_scoring_job_table(&mut client);
    apply_scoring_job_migration(&mut client).unwrap();
    enqueue(
        &mut client,
        "scoring_job_cancel_lookup_failure",
        "scoring_request_cancel_lookup_failure",
        2,
    );

    client
        .batch_execute(
            "CREATE OR REPLACE FUNCTION suppress_cancel_then_hide_relation() \
             RETURNS trigger LANGUAGE plpgsql AS $$ \
             BEGIN \
                 PERFORM set_config('search_path', 'missing_cancel_lookup_schema', true); \
                 RETURN NULL; \
             END $$; \
             CREATE TRIGGER suppress_cancel_then_hide_relation \
             BEFORE UPDATE ON scoring_job_state \
             FOR EACH ROW EXECUTE FUNCTION suppress_cancel_then_hide_relation();",
        )
        .unwrap();

    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        cancel_scoring_job(&mut transaction, "scoring_job_cancel_lookup_failure"),
        Err(ScoringJobPersistenceError::Database(_))
    ));
    transaction.rollback().unwrap();

    client
        .batch_execute("DROP TRIGGER suppress_cancel_then_hide_relation ON scoring_job_state;")
        .unwrap();
}
