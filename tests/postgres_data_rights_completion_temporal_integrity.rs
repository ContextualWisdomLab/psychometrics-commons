//! `PostgreSQL` must preserve data-rights lifecycle time ordering even against direct writes.

use postgres::{error::SqlState, Client, NoTls};
use psychometrics_commons_runtime::postgres_data_rights::apply_data_rights_migration;
use psychometrics_commons_runtime::postgres_data_rights_completion::apply_data_rights_completion_migration;
use psychometrics_commons_runtime::postgres_data_rights_processing::apply_data_rights_processing_migration;
use psychometrics_commons_runtime::postgres_integration::apply_integration_migration;

const SCHEMA: &str = "data_rights_completion_temporal_integrity_test";
const DATABASE_TEST_LOCK_KEY: i64 = 0x4452_434F_4D50_544D;

fn ready_client() -> Client {
    let url = std::env::var("TEST_DATABASE_URL").expect("TEST_DATABASE_URL is required");
    let mut client = Client::connect(&url, NoTls).expect("CI PostgreSQL must be reachable");
    client
        .batch_execute("SET lock_timeout TO '60s'")
        .expect("database-lock waits should have a finite CI bound");
    client
        .query_one("SELECT pg_advisory_lock($1)", &[&DATABASE_TEST_LOCK_KEY])
        .expect("shared PostgreSQL temporal-integrity test lock should be acquired");
    client
        .batch_execute(&format!(
            "DROP SCHEMA IF EXISTS {SCHEMA} CASCADE;
             CREATE SCHEMA {SCHEMA};
             SET search_path TO {SCHEMA};"
        ))
        .expect("isolated temporal-integrity schema should be reset");
    apply_integration_migration(&mut client).expect("integration migration should apply");
    apply_data_rights_migration(&mut client).expect("data-rights migration should apply");
    apply_data_rights_processing_migration(&mut client).expect("processing migration should apply");
    apply_data_rights_completion_migration(&mut client).expect("completion migration should apply");
    client
}

#[test]
fn completion_time_cannot_precede_durable_processing_start() {
    let mut client = ready_client();
    client
        .execute(
            "INSERT INTO data_rights_request_state (
                 request_ref, tenant_ref, participant_ref, request_kind, scope_ref,
                 current_state, requested_at_unix_ms, latest_event_at_unix_ms,
                 verification_evidence_ref, verified_at_unix_ms,
                 operation_ref, processing_started_at_unix_ms
             ) VALUES (
                 'data_rights_request_temporal', 'tenant_alpha', 'participant_alpha',
                 'deletion', 'scope_alpha', 'processing', 10000, 10200,
                 'verification_evidence_alpha', 10100, 'operation_alpha', 10200
             )",
            &[],
        )
        .expect("valid durable processing state should be insertable");

    let error = client
        .execute(
            "UPDATE data_rights_request_state
             SET completion_evidence_ref = 'completion_evidence_alpha',
                 completed_at_unix_ms = 10199,
                 latest_event_at_unix_ms = 10199
             WHERE request_ref = 'data_rights_request_temporal'",
            &[],
        )
        .expect_err("completion earlier than processing start must fail at the database boundary");
    let database_error = error
        .as_db_error()
        .expect("temporal lifecycle violation should be a PostgreSQL constraint error");
    assert_eq!(database_error.code(), &SqlState::CHECK_VIOLATION);
    assert_eq!(
        database_error.constraint(),
        Some("data_rights_completion_after_processing_check")
    );

    let row = client
        .query_one(
            "SELECT current_state, completion_evidence_ref, completed_at_unix_ms,
                    latest_event_at_unix_ms
             FROM data_rights_request_state
             WHERE request_ref = 'data_rights_request_temporal'",
            &[],
        )
        .expect("failed direct completion write must leave durable state readable");
    assert_eq!(row.get::<_, String>(0), "processing");
    assert_eq!(row.get::<_, Option<String>>(1), None);
    assert_eq!(row.get::<_, Option<i64>>(2), None);
    assert_eq!(row.get::<_, i64>(3), 10200);

    client
        .batch_execute(&format!(
            "SET search_path TO public;
             DROP SCHEMA IF EXISTS {SCHEMA} CASCADE;"
        ))
        .expect("isolated temporal-integrity schema should be removed");
}
