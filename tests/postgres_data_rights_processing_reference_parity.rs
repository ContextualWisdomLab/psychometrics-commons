//! Data-rights processing persistence must preserve the Rust opaque-reference boundary.

use postgres::{error::SqlState, Client, NoTls};
use psychometrics_commons_runtime::postgres_data_rights::apply_data_rights_migration;
use psychometrics_commons_runtime::postgres_data_rights_processing::apply_data_rights_processing_migration;
use psychometrics_commons_runtime::postgres_integration::apply_integration_migration;
use std::sync::{Mutex, MutexGuard};

static TEST_LOCK: Mutex<()> = Mutex::new(());

fn guard() -> MutexGuard<'static, ()> {
    TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn client(schema_name: &str) -> Client {
    let url = std::env::var("TEST_DATABASE_URL").expect("TEST_DATABASE_URL is required");
    let mut client = Client::connect(&url, NoTls).expect("CI PostgreSQL must be reachable");
    client
        .batch_execute(&format!(
            "DROP SCHEMA IF EXISTS {schema_name} CASCADE; \
             CREATE SCHEMA {schema_name}; \
             SET search_path TO {schema_name};"
        ))
        .unwrap();
    apply_integration_migration(&mut client).unwrap();
    apply_data_rights_migration(&mut client).unwrap();
    apply_data_rights_processing_migration(&mut client).unwrap();
    client
}

fn insert_request(client: &mut Client, request_ref: &str) {
    client
        .execute(
            "INSERT INTO data_rights_request_state (\
                 request_ref, tenant_ref, participant_ref, request_kind, scope_ref, current_state, \
                 requested_at_unix_ms, latest_event_at_unix_ms\
             ) VALUES ($1, 'tenant_alpha', 'participant_alpha', 'deletion', 'scope_alpha', \
                       'requested', 10000, 10000)",
            &[&request_ref],
        )
        .unwrap();
}

fn assert_check(error: &postgres::Error, constraint: &str) {
    let database_error = error
        .as_db_error()
        .expect("processing persistence rejection must come from a PostgreSQL CHECK");
    assert_eq!(database_error.code(), &SqlState::CHECK_VIOLATION);
    assert_eq!(database_error.constraint(), Some(constraint));
}

#[test]
fn operation_reference_rejects_unicode_numeric_whitespace_and_control_aliases() {
    let _guard = guard();
    let mut client = client("data_rights_processing_reference_parity_test");

    for (index, invalid_reference) in [
        "½",
        "²",
        "Ⅳ",
        "\u{00a0}operation_alpha",
        "operation_\u{0001}_alpha",
    ]
    .into_iter()
    .enumerate()
    {
        let request_ref = format!("data_rights_request_processing_reference_{index}");
        insert_request(&mut client, &request_ref);
        let error = client
            .execute(
                "UPDATE data_rights_request_state \
                 SET operation_ref = $2, processing_started_at_unix_ms = 10200 \
                 WHERE request_ref = $1",
                &[&request_ref, &invalid_reference],
            )
            .expect_err("direct SQL must not bypass the Rust operation-reference boundary");
        assert_check(&error, "data_rights_operation_ref_format_check");
    }
}

#[test]
fn migration_reapplication_repairs_all_owned_processing_constraints() {
    let _guard = guard();
    let mut client = client("data_rights_processing_constraint_repair_test");

    client
        .batch_execute(
            "ALTER TABLE data_rights_request_state \
                 DROP CONSTRAINT data_rights_operation_ref_format_check; \
             ALTER TABLE data_rights_request_state \
                 ADD CONSTRAINT data_rights_operation_ref_format_check CHECK (true); \
             ALTER TABLE data_rights_request_state \
                 DROP CONSTRAINT data_rights_processing_started_time_positive_check; \
             ALTER TABLE data_rights_request_state \
                 ADD CONSTRAINT data_rights_processing_started_time_positive_check CHECK (true); \
             ALTER TABLE data_rights_request_state \
                 DROP CONSTRAINT data_rights_processing_presence_check; \
             ALTER TABLE data_rights_request_state \
                 ADD CONSTRAINT data_rights_processing_presence_check CHECK (true);",
        )
        .unwrap();

    apply_data_rights_processing_migration(&mut client)
        .expect("migration reapplication must repair same-named owned constraints");

    insert_request(&mut client, "data_rights_request_repaired_reference");
    let reference_error = client
        .execute(
            "UPDATE data_rights_request_state \
             SET operation_ref = '½', processing_started_at_unix_ms = 10200 \
             WHERE request_ref = 'data_rights_request_repaired_reference'",
            &[],
        )
        .expect_err("repaired operation constraint must reject Rust-invalid references");
    assert_check(
        &reference_error,
        "data_rights_operation_ref_format_check",
    );

    insert_request(&mut client, "data_rights_request_repaired_time");
    let time_error = client
        .execute(
            "UPDATE data_rights_request_state \
             SET operation_ref = 'operation_alpha', processing_started_at_unix_ms = 0 \
             WHERE request_ref = 'data_rights_request_repaired_time'",
            &[],
        )
        .expect_err("repaired processing-start time constraint must reject zero");
    assert_check(
        &time_error,
        "data_rights_processing_started_time_positive_check",
    );

    insert_request(&mut client, "data_rights_request_repaired_presence");
    let presence_error = client
        .execute(
            "UPDATE data_rights_request_state \
             SET operation_ref = 'operation_alpha' \
             WHERE request_ref = 'data_rights_request_repaired_presence'",
            &[],
        )
        .expect_err("repaired presence constraint must require operation/time pairing");
    assert_check(&presence_error, "data_rights_processing_presence_check");
}

#[test]
fn migration_upgrade_fails_closed_on_historical_invalid_operation_identity() {
    let _guard = guard();
    let mut client = client("data_rights_processing_invalid_history_test");
    insert_request(&mut client, "data_rights_request_historical_invalid");

    client
        .batch_execute(
            "ALTER TABLE data_rights_request_state \
                 DROP CONSTRAINT data_rights_operation_ref_format_check; \
             ALTER TABLE data_rights_request_state \
                 ADD CONSTRAINT data_rights_operation_ref_format_check CHECK (true);",
        )
        .unwrap();
    client
        .execute(
            "UPDATE data_rights_request_state \
             SET operation_ref = '½', processing_started_at_unix_ms = 10200 \
             WHERE request_ref = 'data_rights_request_historical_invalid'",
            &[],
        )
        .expect("the simulated historical schema should admit the invalid operation identity");

    let error = apply_data_rights_processing_migration(&mut client)
        .expect_err("upgrade must fail closed while an invalid historical identity remains");
    assert_eq!(
        error.as_db_error().map(|database_error| database_error.code()),
        Some(&SqlState::CHECK_VIOLATION)
    );
}
