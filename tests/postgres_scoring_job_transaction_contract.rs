//! Transaction-boundary contracts for `PostgreSQL` scoring-job persistence.

use postgres::{Client, IsolationLevel, NoTls};
use psychometrics_commons_runtime::postgres_scoring_job::{
    apply_scoring_job_migration, claim_scoring_job, persist_scoring_job,
    record_retryable_scoring_failure, ScoringJobPersistenceError,
};
use psychometrics_commons_runtime::scoring_job::ScoringJob;

fn isolated_client(schema_sql: &str) -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let mut client = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    client.batch_execute(schema_sql).unwrap();
    client
}

fn queued_job(job_ref: &str, request_ref: &str) -> ScoringJob {
    ScoringJob::new(job_ref, request_ref, 3).unwrap()
}

#[test]
fn claim_rejects_stronger_isolation_without_mutating_the_job() {
    let mut client = isolated_client(
        "DROP SCHEMA IF EXISTS scoring_job_claim_isolation_test CASCADE;\
         CREATE SCHEMA scoring_job_claim_isolation_test;\
         SET search_path TO scoring_job_claim_isolation_test;",
    );
    apply_scoring_job_migration(&mut client).unwrap();

    let job = queued_job(
        "scoring_job_claim_isolation",
        "scoring_request_claim_isolation",
    );
    {
        let mut transaction = client.transaction().unwrap();
        persist_scoring_job(&mut transaction, &job).unwrap();
        transaction.commit().unwrap();
    }

    let mut transaction = client
        .build_transaction()
        .isolation_level(IsolationLevel::Serializable)
        .start()
        .unwrap();
    assert!(matches!(
        claim_scoring_job(
            &mut transaction,
            "scoring_job_claim_isolation",
            "worker_claim_isolation",
            "scoring_lease_claim_isolation",
            10_000,
            11_000,
        ),
        Err(ScoringJobPersistenceError::UnsupportedIsolationLevel)
    ));
    transaction.rollback().unwrap();

    let row = client
        .query_one(
            "SELECT scoring_state, attempt_count FROM scoring_job_state \
             WHERE scoring_job_ref = 'scoring_job_claim_isolation'",
            &[],
        )
        .unwrap();
    assert_eq!(row.get::<_, String>(0), "queued");
    assert_eq!(row.get::<_, i32>(1), 0);

    client
        .batch_execute("DROP SCHEMA scoring_job_claim_isolation_test CASCADE;")
        .unwrap();
}

#[test]
fn persistence_operations_wrap_missing_table_failures() {
    let mut client = isolated_client(
        "DROP SCHEMA IF EXISTS scoring_job_database_error_test CASCADE;\
         CREATE SCHEMA scoring_job_database_error_test;\
         SET search_path TO scoring_job_database_error_test;",
    );
    let job = queued_job(
        "scoring_job_database_error",
        "scoring_request_database_error",
    );

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
            "scoring_job_database_error",
            "worker_database_error",
            "scoring_lease_database_error",
            10_000,
            11_000,
        ),
        Err(ScoringJobPersistenceError::Database(_))
    ));
    transaction.rollback().unwrap();

    client
        .batch_execute("DROP SCHEMA scoring_job_database_error_test CASCADE;")
        .unwrap();
}

#[test]
fn enqueue_classification_wraps_a_second_statement_database_failure() {
    let mut client = isolated_client(
        "DROP SCHEMA IF EXISTS scoring_job_enqueue_classification_test CASCADE;\
         DROP SCHEMA IF EXISTS scoring_job_enqueue_failure_sink CASCADE;\
         CREATE SCHEMA scoring_job_enqueue_classification_test;\
         CREATE SCHEMA scoring_job_enqueue_failure_sink;\
         SET search_path TO scoring_job_enqueue_classification_test;",
    );
    apply_scoring_job_migration(&mut client).unwrap();

    let job = queued_job(
        "scoring_job_enqueue_classification",
        "scoring_request_enqueue_classification",
    );
    {
        let mut transaction = client.transaction().unwrap();
        persist_scoring_job(&mut transaction, &job).unwrap();
        transaction.commit().unwrap();
    }

    client
        .batch_execute(
            r"CREATE FUNCTION redirect_after_insert() RETURNS trigger LANGUAGE plpgsql AS $$
            BEGIN
                PERFORM set_config('search_path', 'scoring_job_enqueue_failure_sink', true);
                RETURN NULL;
            END
            $$;
            CREATE TRIGGER redirect_after_insert
            AFTER INSERT ON scoring_job_state
            FOR EACH STATEMENT EXECUTE FUNCTION redirect_after_insert();",
        )
        .unwrap();

    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        persist_scoring_job(&mut transaction, &job),
        Err(ScoringJobPersistenceError::Database(_))
    ));
    transaction.rollback().unwrap();

    client
        .batch_execute(
            "DROP SCHEMA scoring_job_enqueue_classification_test CASCADE;\
             DROP SCHEMA scoring_job_enqueue_failure_sink CASCADE;",
        )
        .unwrap();
}

#[test]
fn claim_classification_wraps_a_second_statement_database_failure() {
    let mut client = isolated_client(
        "DROP SCHEMA IF EXISTS scoring_job_claim_classification_test CASCADE;\
         DROP SCHEMA IF EXISTS scoring_job_claim_failure_sink CASCADE;\
         CREATE SCHEMA scoring_job_claim_classification_test;\
         CREATE SCHEMA scoring_job_claim_failure_sink;\
         SET search_path TO scoring_job_claim_classification_test;",
    );
    apply_scoring_job_migration(&mut client).unwrap();

    let job = queued_job(
        "scoring_job_claim_classification",
        "scoring_request_claim_classification",
    );
    {
        let mut transaction = client.transaction().unwrap();
        persist_scoring_job(&mut transaction, &job).unwrap();
        transaction.commit().unwrap();
    }
    {
        let mut transaction = client.transaction().unwrap();
        claim_scoring_job(
            &mut transaction,
            "scoring_job_claim_classification",
            "worker_claim_classification",
            "scoring_lease_claim_classification",
            10_000,
            11_000,
        )
        .unwrap();
        transaction.commit().unwrap();
    }

    client
        .batch_execute(
            r"CREATE FUNCTION redirect_after_update() RETURNS trigger LANGUAGE plpgsql AS $$
            BEGIN
                PERFORM set_config('search_path', 'scoring_job_claim_failure_sink', true);
                RETURN NULL;
            END
            $$;
            CREATE TRIGGER redirect_after_update
            AFTER UPDATE ON scoring_job_state
            FOR EACH STATEMENT EXECUTE FUNCTION redirect_after_update();",
        )
        .unwrap();

    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        claim_scoring_job(
            &mut transaction,
            "scoring_job_claim_classification",
            "worker_claim_classification_retry",
            "scoring_lease_claim_classification_retry",
            10_500,
            11_500,
        ),
        Err(ScoringJobPersistenceError::Database(_))
    ));
    transaction.rollback().unwrap();

    client
        .batch_execute(
            "DROP SCHEMA scoring_job_claim_classification_test CASCADE;\
             DROP SCHEMA scoring_job_claim_failure_sink CASCADE;",
        )
        .unwrap();
}

#[test]
fn retry_transition_wraps_update_database_failure_after_lease_validation() {
    let mut client = isolated_client(
        "DROP SCHEMA IF EXISTS scoring_job_retry_transition_failure_test CASCADE;\
         CREATE SCHEMA scoring_job_retry_transition_failure_test;\
         SET search_path TO scoring_job_retry_transition_failure_test;",
    );
    apply_scoring_job_migration(&mut client).unwrap();

    let job = queued_job(
        "scoring_job_retry_transition_failure",
        "scoring_request_retry_transition_failure",
    );
    {
        let mut transaction = client.transaction().unwrap();
        persist_scoring_job(&mut transaction, &job).unwrap();
        claim_scoring_job(
            &mut transaction,
            "scoring_job_retry_transition_failure",
            "worker_retry_transition_failure",
            "scoring_lease_retry_transition_failure",
            10_000,
            11_000,
        )
        .unwrap();
        transaction.commit().unwrap();
    }

    client
        .batch_execute(
            r"CREATE FUNCTION fail_retry_update() RETURNS trigger LANGUAGE plpgsql AS $$
            BEGIN
                RAISE EXCEPTION 'forced retry transition failure';
                RETURN NULL;
            END
            $$;
            CREATE TRIGGER fail_retry_update
            BEFORE UPDATE ON scoring_job_state
            FOR EACH STATEMENT EXECUTE FUNCTION fail_retry_update();",
        )
        .unwrap();

    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        record_retryable_scoring_failure(
            &mut transaction,
            "scoring_job_retry_transition_failure",
            1,
            "provider_timeout",
            10_500,
            12_000,
        ),
        Err(ScoringJobPersistenceError::Database(_))
    ));
    transaction.rollback().unwrap();

    let row = client
        .query_one(
            "SELECT scoring_state, attempt_count, last_failure_code \
             FROM scoring_job_state \
             WHERE scoring_job_ref = 'scoring_job_retry_transition_failure'",
            &[],
        )
        .unwrap();
    assert_eq!(row.get::<_, String>(0), "leased");
    assert_eq!(row.get::<_, i32>(1), 1);
    assert_eq!(row.get::<_, Option<String>>(2), None);

    client
        .batch_execute("DROP SCHEMA scoring_job_retry_transition_failure_test CASCADE;")
        .unwrap();
}
