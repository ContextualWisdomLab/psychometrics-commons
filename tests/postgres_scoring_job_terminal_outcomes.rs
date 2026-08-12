//! Verifies that a live fenced scoring worker can durably finish one attempt.
//! A successful attempt stores one immutable result and accepts only exact replay;
//! a permanent failure quarantines the job without inventing a result. Stale workers
//! and expired leases must not be able to rewrite either terminal outcome.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::postgres_scoring_job::{
    apply_scoring_job_migration, claim_scoring_job, persist_scoring_job,
    record_permanent_scoring_failure, record_successful_scoring_completion,
    ScoringJobCompletionDisposition, ScoringJobPersistenceError,
};
use psychometrics_commons_runtime::scoring_job::ScoringJob;

fn test_client(schema: &str) -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let mut client = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    client
        .batch_execute(&format!(
            "DROP SCHEMA IF EXISTS {schema} CASCADE;\
             CREATE SCHEMA {schema};\
             SET search_path TO {schema};",
        ))
        .unwrap();
    apply_scoring_job_migration(&mut client).unwrap();
    client
}

fn persist_job(client: &mut Client, job_ref: &str, request_ref: &str) {
    let job = ScoringJob::new(job_ref, request_ref, 3).unwrap();
    let mut transaction = client.transaction().unwrap();
    persist_scoring_job(&mut transaction, &job).unwrap();
    transaction.commit().unwrap();
}

fn persist_and_claim(client: &mut Client, job_ref: &str, request_ref: &str) {
    let job = ScoringJob::new(job_ref, request_ref, 3).unwrap();
    let mut transaction = client.transaction().unwrap();
    persist_scoring_job(&mut transaction, &job).unwrap();
    claim_scoring_job(
        &mut transaction,
        job_ref,
        "worker_terminal_outcome",
        "scoring_lease_terminal_outcome",
        10_000,
        11_000,
    )
    .unwrap();
    transaction.commit().unwrap();
}

#[test]
fn successful_completion_is_immutable_and_exact_replay_is_idempotent() {
    let mut client = test_client("scoring_job_successful_completion_test");
    persist_and_claim(
        &mut client,
        "scoring_job_successful_completion",
        "scoring_request_successful_completion",
    );

    {
        let mut transaction = client.transaction().unwrap();
        assert_eq!(
            record_successful_scoring_completion(
                &mut transaction,
                "scoring_job_successful_completion",
                1,
                "scoring_result_successful_completion",
                10_500,
            )
            .unwrap(),
            ScoringJobCompletionDisposition::Completed
        );
        transaction.commit().unwrap();
    }

    let row = client
        .query_one(
            "SELECT scoring_state, result_ref, completed_fencing_token,\
                    active_worker_ref, active_lease_ref, active_fencing_token,\
                    active_lease_expires_at_unix_ms, next_attempt_at_unix_ms,\
                    last_failure_code \
             FROM scoring_job_state \
             WHERE scoring_job_ref = 'scoring_job_successful_completion'",
            &[],
        )
        .unwrap();
    assert_eq!(row.get::<_, String>(0), "completed");
    assert_eq!(
        row.get::<_, Option<String>>(1).as_deref(),
        Some("scoring_result_successful_completion")
    );
    assert_eq!(row.get::<_, Option<i64>>(2), Some(1));
    assert_eq!(row.get::<_, Option<String>>(3), None);
    assert_eq!(row.get::<_, Option<String>>(4), None);
    assert_eq!(row.get::<_, Option<i64>>(5), None);
    assert_eq!(row.get::<_, Option<i64>>(6), None);
    assert_eq!(row.get::<_, Option<i64>>(7), None);
    assert_eq!(row.get::<_, Option<String>>(8), None);

    let mut transaction = client.transaction().unwrap();
    assert_eq!(
        record_successful_scoring_completion(
            &mut transaction,
            "scoring_job_successful_completion",
            1,
            "scoring_result_successful_completion",
            10_600,
        )
        .unwrap(),
        ScoringJobCompletionDisposition::Duplicate
    );
    assert!(matches!(
        record_successful_scoring_completion(
            &mut transaction,
            "scoring_job_successful_completion",
            1,
            "scoring_result_conflicting_completion",
            10_600,
        ),
        Err(ScoringJobPersistenceError::ConflictingCompletion)
    ));
    transaction.rollback().unwrap();
}

#[test]
fn permanent_failure_quarantines_without_a_result() {
    let mut client = test_client("scoring_job_permanent_failure_test");
    persist_and_claim(
        &mut client,
        "scoring_job_permanent_failure",
        "scoring_request_permanent_failure",
    );

    let mut transaction = client.transaction().unwrap();
    record_permanent_scoring_failure(
        &mut transaction,
        "scoring_job_permanent_failure",
        1,
        "invalid_scientific_evidence",
        10_500,
    )
    .unwrap();
    transaction.commit().unwrap();

    let row = client
        .query_one(
            "SELECT scoring_state, last_failure_code, result_ref, completed_fencing_token,\
                    active_worker_ref, active_fencing_token \
             FROM scoring_job_state \
             WHERE scoring_job_ref = 'scoring_job_permanent_failure'",
            &[],
        )
        .unwrap();
    assert_eq!(row.get::<_, String>(0), "quarantined");
    assert_eq!(
        row.get::<_, Option<String>>(1).as_deref(),
        Some("invalid_scientific_evidence")
    );
    assert_eq!(row.get::<_, Option<String>>(2), None);
    assert_eq!(row.get::<_, Option<i64>>(3), None);
    assert_eq!(row.get::<_, Option<String>>(4), None);
    assert_eq!(row.get::<_, Option<i64>>(5), None);
}

#[test]
fn stale_and_expired_workers_cannot_write_terminal_outcomes() {
    let mut stale_client = test_client("scoring_job_terminal_stale_worker_test");
    persist_and_claim(
        &mut stale_client,
        "scoring_job_terminal_stale_worker",
        "scoring_request_terminal_stale_worker",
    );
    {
        let mut transaction = stale_client.transaction().unwrap();
        assert!(matches!(
            record_successful_scoring_completion(
                &mut transaction,
                "scoring_job_terminal_stale_worker",
                2,
                "scoring_result_stale_worker",
                10_500,
            ),
            Err(ScoringJobPersistenceError::StaleLease)
        ));
        transaction.rollback().unwrap();
    }

    let mut expired_client = test_client("scoring_job_terminal_expired_worker_test");
    persist_and_claim(
        &mut expired_client,
        "scoring_job_terminal_expired_worker",
        "scoring_request_terminal_expired_worker",
    );
    let mut transaction = expired_client.transaction().unwrap();
    assert!(matches!(
        record_permanent_scoring_failure(
            &mut transaction,
            "scoring_job_terminal_expired_worker",
            1,
            "provider_rejected_payload",
            11_000,
        ),
        Err(ScoringJobPersistenceError::LeaseExpired)
    ));
    transaction.rollback().unwrap();
}

#[test]
fn successful_completion_rejects_missing_not_leased_and_expired_jobs() {
    let mut missing_client = test_client("scoring_job_terminal_missing_test");
    {
        let mut transaction = missing_client.transaction().unwrap();
        assert!(matches!(
            record_successful_scoring_completion(
                &mut transaction,
                "scoring_job_missing_terminal_outcome",
                1,
                "scoring_result_missing_terminal_outcome",
                10_500,
            ),
            Err(ScoringJobPersistenceError::JobNotFound)
        ));
        transaction.rollback().unwrap();
    }

    let mut queued_client = test_client("scoring_job_terminal_not_leased_test");
    persist_job(
        &mut queued_client,
        "scoring_job_terminal_not_leased",
        "scoring_request_terminal_not_leased",
    );
    {
        let mut transaction = queued_client.transaction().unwrap();
        assert!(matches!(
            record_successful_scoring_completion(
                &mut transaction,
                "scoring_job_terminal_not_leased",
                1,
                "scoring_result_terminal_not_leased",
                10_500,
            ),
            Err(ScoringJobPersistenceError::NotLeased)
        ));
        transaction.rollback().unwrap();
    }

    let mut expired_client = test_client("scoring_job_completion_expired_test");
    persist_and_claim(
        &mut expired_client,
        "scoring_job_completion_expired",
        "scoring_request_completion_expired",
    );
    let mut transaction = expired_client.transaction().unwrap();
    assert!(matches!(
        record_successful_scoring_completion(
            &mut transaction,
            "scoring_job_completion_expired",
            1,
            "scoring_result_completion_expired",
            11_000,
        ),
        Err(ScoringJobPersistenceError::LeaseExpired)
    ));
    transaction.rollback().unwrap();
}

#[test]
fn terminal_outcomes_propagate_database_failures() {
    let mut query_client = test_client("scoring_job_completion_query_failure_test");
    {
        let mut transaction = query_client.transaction().unwrap();
        transaction.batch_execute("DROP TABLE scoring_job_state").unwrap();
        assert!(matches!(
            record_successful_scoring_completion(
                &mut transaction,
                "scoring_job_query_failure",
                1,
                "scoring_result_query_failure",
                10_500,
            ),
            Err(ScoringJobPersistenceError::Database(_))
        ));
        transaction.rollback().unwrap();
    }

    let mut permanent_client = test_client("scoring_job_permanent_update_failure_test");
    persist_and_claim(
        &mut permanent_client,
        "scoring_job_permanent_update_failure",
        "scoring_request_permanent_update_failure",
    );
    permanent_client
        .batch_execute(
            "ALTER TABLE scoring_job_state \
             ADD CONSTRAINT reject_quarantined_state \
             CHECK (scoring_state <> 'quarantined')",
        )
        .unwrap();
    {
        let mut transaction = permanent_client.transaction().unwrap();
        assert!(matches!(
            record_permanent_scoring_failure(
                &mut transaction,
                "scoring_job_permanent_update_failure",
                1,
                "permanent_update_rejected",
                10_500,
            ),
            Err(ScoringJobPersistenceError::Database(_))
        ));
        transaction.rollback().unwrap();
    }

    let mut completion_client = test_client("scoring_job_completion_update_failure_test");
    persist_and_claim(
        &mut completion_client,
        "scoring_job_completion_update_failure",
        "scoring_request_completion_update_failure",
    );
    completion_client
        .batch_execute(
            "ALTER TABLE scoring_job_state \
             ADD CONSTRAINT reject_result_ref \
             CHECK (result_ref IS NULL)",
        )
        .unwrap();
    let mut transaction = completion_client.transaction().unwrap();
    assert!(matches!(
        record_successful_scoring_completion(
            &mut transaction,
            "scoring_job_completion_update_failure",
            1,
            "scoring_result_completion_update_failure",
            10_500,
        ),
        Err(ScoringJobPersistenceError::Database(_))
    ));
    transaction.rollback().unwrap();
}

#[test]
fn conflicting_completion_error_is_operator_readable() {
    assert_eq!(
        ScoringJobPersistenceError::ConflictingCompletion.to_string(),
        "scoring completion was replayed with conflicting immutable evidence"
    );
}
