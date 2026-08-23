//! These tests verify that direct SQL rejects invalid worker and lease reference values.

use postgres::{error::SqlState, Client, NoTls};
use psychometrics_commons_runtime::postgres_integration::apply_integration_migration;
use std::sync::{Mutex, MutexGuard};

static TEST_LOCK: Mutex<()> = Mutex::new(());

fn guard() -> MutexGuard<'static, ()> {
    TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn test_client() -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    Client::connect(&connection, NoTls).expect("isolated CI PostgreSQL database must be reachable")
}

fn setup_outbox(client: &mut Client) {
    client
        .batch_execute(
            "DROP SCHEMA IF EXISTS integration_lease_reference_parity_test CASCADE; \
             CREATE SCHEMA integration_lease_reference_parity_test; \
             SET search_path TO integration_lease_reference_parity_test;",
        )
        .unwrap();
    apply_integration_migration(client).unwrap();
    client
        .execute(
            "INSERT INTO integration_outbox (\
                 event_ref, event_type, schema_version, source_ref, tenant_ref, subject_ref, \
                 occurred_at_unix_ms, correlation_ref, payload_digest, max_attempts, \
                 current_state, latest_event_at_unix_ms\
             ) VALUES ('event_alpha','result.released','v1','psychometrics_commons',\
                       'tenant_alpha','result_alpha',1000,'correlation_alpha',\
                       'sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',\
                       3,'pending',1000)",
            &[],
        )
        .unwrap();
}

fn assert_check(error: &postgres::Error, expected_constraint: &str) {
    let database_error = error
        .as_db_error()
        .expect("lease reference rejection must be a PostgreSQL CHECK violation");
    assert_eq!(database_error.code(), &SqlState::CHECK_VIOLATION);
    assert_eq!(database_error.constraint(), Some(expected_constraint));
}

#[test]
fn lease_worker_reference_rejects_unicode_numeric_aliases() {
    let _guard = guard();
    let mut client = test_client();
    setup_outbox(&mut client);

    let error = client
        .execute(
            "UPDATE integration_outbox SET \
                 lease_worker_ref = $1, lease_ref = 'lease_alpha', \
                 lease_fencing_token = 1, lease_expires_at_unix_ms = 2000, \
                 delivery_lease_generation = 1 \
             WHERE event_ref = 'event_alpha'",
            &[&"½"],
        )
        .expect_err("Unicode numeric-only worker references must fail closed");
    assert_check(&error, "integration_outbox_lease_worker_ref_format_check");
}

#[test]
fn lease_reference_rejects_unicode_whitespace_aliases() {
    let _guard = guard();
    let mut client = test_client();
    setup_outbox(&mut client);

    let error = client
        .execute(
            "UPDATE integration_outbox SET \
                 lease_worker_ref = 'worker_alpha', lease_ref = $1, \
                 lease_fencing_token = 1, lease_expires_at_unix_ms = 2000, \
                 delivery_lease_generation = 1 \
             WHERE event_ref = 'event_alpha'",
            &[&"\u{00a0}lease_alpha"],
        )
        .expect_err("Unicode-padded lease references must fail closed");
    assert_check(&error, "integration_outbox_lease_ref_format_check");
}
