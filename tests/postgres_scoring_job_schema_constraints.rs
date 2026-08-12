//! Real `PostgreSQL` state-shape constraints for durable scoring-job rows.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::postgres_scoring_job::apply_scoring_job_migration;

fn test_client() -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    Client::connect(&connection, NoTls).expect("isolated CI PostgreSQL database must be reachable")
}

#[test]
fn scoring_job_migration_rejects_incompatible_preexisting_schema() {
    let mut client = test_client();
    client
        .batch_execute(
            "DROP SCHEMA IF EXISTS scoring_job_drift_test CASCADE;\
             CREATE SCHEMA scoring_job_drift_test;\
             SET search_path TO scoring_job_drift_test, public;\
             CREATE TABLE scoring_job_state (unexpected_column TEXT);",
        )
        .unwrap();

    assert!(apply_scoring_job_migration(&mut client).is_err());

    client
        .batch_execute("DROP SCHEMA scoring_job_drift_test CASCADE;")
        .unwrap();
}

#[test]
fn scoring_job_migration_rejects_weakened_same_name_constraint() {
    let mut client = test_client();
    client
        .batch_execute(
            "DROP SCHEMA IF EXISTS scoring_job_constraint_drift_test CASCADE;\
             CREATE SCHEMA scoring_job_constraint_drift_test;\
             SET search_path TO scoring_job_constraint_drift_test, public;",
        )
        .unwrap();
    apply_scoring_job_migration(&mut client).unwrap();
    client
        .batch_execute(
            "ALTER TABLE scoring_job_state DROP CONSTRAINT scoring_state_shape_check;\
             ALTER TABLE scoring_job_state ADD CONSTRAINT scoring_state_shape_check CHECK (true);",
        )
        .unwrap();

    assert!(apply_scoring_job_migration(&mut client).is_err());

    client
        .batch_execute("DROP SCHEMA scoring_job_constraint_drift_test CASCADE;")
        .unwrap();
}

#[test]
fn impossible_scoring_job_state_shapes_are_rejected_by_postgres() {
    let mut client = test_client();
    client
        .batch_execute(
            "DROP SCHEMA IF EXISTS scoring_job_shape_test CASCADE;\
             CREATE SCHEMA scoring_job_shape_test;\
             SET search_path TO scoring_job_shape_test, public;",
        )
        .unwrap();
    apply_scoring_job_migration(&mut client).unwrap();
    apply_scoring_job_migration(&mut client).unwrap();

    for statement in [
        "INSERT INTO scoring_job_state (scoring_job_ref, scoring_request_ref, scoring_state, attempt_count, max_attempts) VALUES ('scoring_job_bad_queued_attempt', 'scoring_request_bad_queued_attempt', 'queued', 1, 3)",
        "INSERT INTO scoring_job_state (scoring_job_ref, scoring_request_ref, scoring_state, attempt_count, max_attempts, last_failure_code) VALUES ('scoring_job_bad_queued_failure', 'scoring_request_bad_queued_failure', 'queued', 0, 3, 'unexpected_failure')",
        "INSERT INTO scoring_job_state (scoring_job_ref, scoring_request_ref, scoring_state, attempt_count, max_attempts) VALUES ('scoring_job_bad_retry_schedule', 'scoring_request_bad_retry_schedule', 'retry_scheduled', 1, 3)",
        "INSERT INTO scoring_job_state (scoring_job_ref, scoring_request_ref, scoring_state, attempt_count, max_attempts, next_attempt_at_unix_ms) VALUES ('scoring_job_bad_retry_cause', 'scoring_request_bad_retry_cause', 'retry_scheduled', 1, 3, 20000)",
        "INSERT INTO scoring_job_state (scoring_job_ref, scoring_request_ref, scoring_state, attempt_count, max_attempts, next_attempt_at_unix_ms, last_failure_code) VALUES ('scoring_job_bad_retry_budget', 'scoring_request_bad_retry_budget', 'retry_scheduled', 3, 3, 20000, 'retryable_failure')",
        "INSERT INTO scoring_job_state (scoring_job_ref, scoring_request_ref, scoring_state, attempt_count, max_attempts) VALUES ('scoring_job_bad_quarantine_cause', 'scoring_request_bad_quarantine_cause', 'quarantined', 1, 3)",
        "INSERT INTO scoring_job_state (scoring_job_ref, scoring_request_ref, scoring_state, attempt_count, max_attempts) VALUES ('scoring_job_bad_completed_missing', 'scoring_request_bad_completed_missing', 'completed', 1, 3)",
        "INSERT INTO scoring_job_state (scoring_job_ref, scoring_request_ref, scoring_state, attempt_count, max_attempts, result_ref, completed_fencing_token) VALUES ('scoring_job_bad_completed_fence', 'scoring_request_bad_completed_fence', 'completed', 1, 3, 'scoring_result_bad_completed_fence', 2)",
        "INSERT INTO scoring_job_state (scoring_job_ref, scoring_request_ref, scoring_state, attempt_count, max_attempts, result_ref) VALUES ('scoring_job_bad_cancelled_result', 'scoring_request_bad_cancelled_result', 'cancelled', 0, 3, 'scoring_result_bad_cancelled_result')",
    ] {
        assert!(
            client.execute(statement, &[]).is_err(),
            "database accepted an impossible scoring-job state shape: {statement}"
        );
    }

    client
        .batch_execute("DROP SCHEMA scoring_job_shape_test CASCADE;")
        .unwrap();
}
