//! Database-failure propagation coverage for the `PostgreSQL` scoring-job adapter.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::postgres_scoring_job::{
    claim_scoring_job, persist_scoring_job, record_retryable_scoring_failure,
    ScoringJobPersistenceError,
};
use psychometrics_commons_runtime::scoring_job::ScoringJob;

fn test_client() -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let mut client = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    client
        .batch_execute(
            "CREATE SCHEMA IF NOT EXISTS scoring_job_database_failure_test;\
             SET search_path TO scoring_job_database_failure_test;\
             DROP TABLE IF EXISTS scoring_job_database_failure_test.scoring_job_state;",
        )
        .unwrap();
    client
}

#[test]
fn persistence_operations_wrap_missing_table_failures() {
    let mut client = test_client();
    let job = ScoringJob::new("scoring_job_dberror", "scoring_request_dberror", 3).unwrap();

    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        persist_scoring_job(&mut transaction, &job),
        Err(ScoringJobPersistenceError::Database(_))
    ));
    transaction.rollback().unwrap();

    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        claim_scoring_job(
            &mut transaction,
            "scoring_job_dberror",
            "worker_dberror",
            "scoring_lease_dberror",
            10_000,
            11_000,
        ),
        Err(ScoringJobPersistenceError::Database(_))
    ));
    transaction.rollback().unwrap();

    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        record_retryable_scoring_failure(
            &mut transaction,
            "scoring_job_dberror",
            1,
            "provider_timeout",
            10_500,
            12_000,
        ),
        Err(ScoringJobPersistenceError::Database(_))
    ));
    transaction.rollback().unwrap();
}
