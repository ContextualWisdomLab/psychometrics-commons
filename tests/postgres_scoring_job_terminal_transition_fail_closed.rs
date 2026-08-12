//! Regression contract for fail-closed scoring terminal persistence.
//!
//! `PostgreSQL` row-level triggers can suppress an `UPDATE` by returning `NULL`. A terminal
//! scoring API must not report success when its guarded update affects zero rows after the
//! worker lease was validated and locked.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::postgres_scoring_job::{
    apply_scoring_job_migration, claim_scoring_job, persist_scoring_job,
    record_permanent_scoring_failure, record_successful_scoring_completion,
    ScoringJobPersistenceError,
};
use psychometrics_commons_runtime::scoring_job::ScoringJob;

fn test_client(schema: &str, job_ref: &str, request_ref: &str) -> Client {
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

    let job = ScoringJob::new(job_ref, request_ref, 3).unwrap();
    let mut transaction = client.transaction().unwrap();
    persist_scoring_job(&mut transaction, &job).unwrap();
    claim_scoring_job(
        &mut transaction,
        job_ref,
        "worker_terminal_fail_closed",
        "scoring_lease_terminal_fail_closed",
        10_000,
        11_000,
    )
    .unwrap();
    transaction.commit().unwrap();
    client
}

fn suppress_terminal_updates(client: &mut Client) {
    client
        .batch_execute(
            r#"CREATE FUNCTION suppress_terminal_update() RETURNS trigger AS $$
BEGIN
    IF NEW.scoring_state IN ('completed', 'quarantined') THEN
        RETURN NULL;
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;
CREATE TRIGGER suppress_terminal_update
BEFORE UPDATE ON scoring_job_state
FOR EACH ROW EXECUTE FUNCTION suppress_terminal_update();"#,
        )
        .unwrap();
}

#[test]
fn permanent_failure_rejects_a_suppressed_terminal_update() {
    let mut client = test_client(
        "scoring_job_permanent_transition_fail_closed_test",
        "scoring_job_permanent_transition_fail_closed",
        "scoring_request_permanent_transition_fail_closed",
    );
    suppress_terminal_updates(&mut client);

    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        record_permanent_scoring_failure(
            &mut transaction,
            "scoring_job_permanent_transition_fail_closed",
            1,
            "scientific_failure_permanent",
            10_500,
        ),
        Err(ScoringJobPersistenceError::TransitionNotApplied)
    ));
    transaction.rollback().unwrap();
}

#[test]
fn successful_completion_rejects_a_suppressed_terminal_update() {
    let mut client = test_client(
        "scoring_job_completion_transition_fail_closed_test",
        "scoring_job_completion_transition_fail_closed",
        "scoring_request_completion_transition_fail_closed",
    );
    suppress_terminal_updates(&mut client);

    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        record_successful_scoring_completion(
            &mut transaction,
            "scoring_job_completion_transition_fail_closed",
            1,
            "scoring_result_transition_fail_closed",
            10_500,
        ),
        Err(ScoringJobPersistenceError::TransitionNotApplied)
    ));
    transaction.rollback().unwrap();
}
