//! Validation and lease-authority regressions for scoring terminal outcomes.
//!
//! These tests keep invalid identity, fencing, timestamp, isolation, and stale-worker
//! evidence fail-closed before a terminal outcome can be persisted.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::postgres_scoring_job::{
    apply_scoring_job_migration, claim_scoring_job, persist_scoring_job,
    record_permanent_scoring_failure, record_successful_scoring_completion,
    ScoringJobPersistenceError,
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

fn persist_and_claim(client: &mut Client, job_ref: &str, request_ref: &str) {
    let job = ScoringJob::new(job_ref, request_ref, 3).unwrap();
    let mut transaction = client.transaction().unwrap();
    persist_scoring_job(&mut transaction, &job).unwrap();
    claim_scoring_job(
        &mut transaction,
        job_ref,
        "worker_terminal_validation",
        "scoring_lease_terminal_validation",
        10_000,
        11_000,
    )
    .unwrap();
    transaction.commit().unwrap();
}

#[test]
fn terminal_outcome_inputs_fail_closed_before_database_mutation() {
    let mut client = test_client("scoring_job_terminal_input_validation_test");
    let mut transaction = client.transaction().unwrap();

    assert!(matches!(
        record_successful_scoring_completion(
            &mut transaction,
            "",
            1,
            "scoring_result_terminal_validation",
            10_500,
        ),
        Err(ScoringJobPersistenceError::InvalidReference)
    ));
    assert!(matches!(
        record_successful_scoring_completion(
            &mut transaction,
            "scoring_job_terminal_validation",
            1,
            "",
            10_500,
        ),
        Err(ScoringJobPersistenceError::InvalidReference)
    ));
    assert!(matches!(
        record_successful_scoring_completion(
            &mut transaction,
            "scoring_job_terminal_validation",
            0,
            "scoring_result_terminal_validation",
            10_500,
        ),
        Err(ScoringJobPersistenceError::InvalidFencingToken)
    ));
    assert!(matches!(
        record_successful_scoring_completion(
            &mut transaction,
            "scoring_job_terminal_validation",
            u64::MAX,
            "scoring_result_terminal_validation",
            10_500,
        ),
        Err(ScoringJobPersistenceError::ValueOutOfRange)
    ));
    assert!(matches!(
        record_successful_scoring_completion(
            &mut transaction,
            "scoring_job_terminal_validation",
            1,
            "scoring_result_terminal_validation",
            0,
        ),
        Err(ScoringJobPersistenceError::InvalidTimestamp)
    ));

    assert!(matches!(
        record_permanent_scoring_failure(
            &mut transaction,
            "",
            1,
            "terminal_validation_failure",
            10_500,
        ),
        Err(ScoringJobPersistenceError::InvalidReference)
    ));
    assert!(matches!(
        record_permanent_scoring_failure(
            &mut transaction,
            "scoring_job_terminal_validation",
            1,
            "",
            10_500,
        ),
        Err(ScoringJobPersistenceError::InvalidReference)
    ));
    assert!(matches!(
        record_permanent_scoring_failure(
            &mut transaction,
            "scoring_job_terminal_validation",
            0,
            "terminal_validation_failure",
            10_500,
        ),
        Err(ScoringJobPersistenceError::InvalidFencingToken)
    ));
    assert!(matches!(
        record_permanent_scoring_failure(
            &mut transaction,
            "scoring_job_terminal_validation",
            u64::MAX,
            "terminal_validation_failure",
            10_500,
        ),
        Err(ScoringJobPersistenceError::ValueOutOfRange)
    ));
    assert!(matches!(
        record_permanent_scoring_failure(
            &mut transaction,
            "scoring_job_terminal_validation",
            1,
            "terminal_validation_failure",
            0,
        ),
        Err(ScoringJobPersistenceError::InvalidTimestamp)
    ));

    transaction.rollback().unwrap();
}

#[test]
fn terminal_outcomes_reject_unsupported_transaction_isolation() {
    let mut client = test_client("scoring_job_terminal_isolation_validation_test");

    {
        let mut transaction = client.transaction().unwrap();
        transaction
            .batch_execute("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
            .unwrap();
        assert!(matches!(
            record_successful_scoring_completion(
                &mut transaction,
                "scoring_job_terminal_isolation_validation",
                1,
                "scoring_result_terminal_isolation_validation",
                10_500,
            ),
            Err(ScoringJobPersistenceError::UnsupportedIsolationLevel)
        ));
        transaction.rollback().unwrap();
    }

    let mut transaction = client.transaction().unwrap();
    transaction
        .batch_execute("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE")
        .unwrap();
    assert!(matches!(
        record_permanent_scoring_failure(
            &mut transaction,
            "scoring_job_terminal_isolation_validation",
            1,
            "terminal_isolation_validation_failure",
            10_500,
        ),
        Err(ScoringJobPersistenceError::UnsupportedIsolationLevel)
    ));
    transaction.rollback().unwrap();
}

#[test]
fn permanent_failure_rejects_stale_fencing_token() {
    let mut client = test_client("scoring_job_permanent_failure_stale_fence_test");
    persist_and_claim(
        &mut client,
        "scoring_job_permanent_failure_stale_fence",
        "scoring_request_permanent_failure_stale_fence",
    );

    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        record_permanent_scoring_failure(
            &mut transaction,
            "scoring_job_permanent_failure_stale_fence",
            2,
            "stale_worker_terminal_failure",
            10_500,
        ),
        Err(ScoringJobPersistenceError::StaleLease)
    ));
    transaction.rollback().unwrap();
}
