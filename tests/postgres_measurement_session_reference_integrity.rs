//! Database reference validation must match the product's canonical opaque-reference boundary.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::postgres_measurement_session::apply_measurement_session_migration;
use std::sync::{Mutex, MutexGuard};

static TEST_LOCK: Mutex<()> = Mutex::new(());

fn guard() -> MutexGuard<'static, ()> {
    TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn client() -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let mut client = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    client
        .batch_execute(
            "DROP SCHEMA IF EXISTS measurement_session_reference_integrity_test CASCADE; \
             CREATE SCHEMA measurement_session_reference_integrity_test; \
             SET search_path TO measurement_session_reference_integrity_test;",
        )
        .unwrap();
    apply_measurement_session_migration(&mut client).unwrap();
    client
}

#[test]
fn database_rejects_unicode_numeric_and_control_reference_aliases() {
    let _guard = guard();
    let mut client = client();

    for participant_ref in [
        "1．5",
        "1，000",
        "1٫5",
        "1٬000",
        "participant_\u{0001}_alpha",
        "participant_\u{001f}_alpha",
    ] {
        let error = client
            .execute(
                "INSERT INTO assessment_participant \
                 (participant_ref, tenant_ref, created_at_unix_ms) VALUES ($1, $2, $3)",
                &[&participant_ref, &"tenant_alpha", &1_700_000_000_000_i64],
            )
            .expect_err("unsafe opaque-reference aliases must be rejected by PostgreSQL");
        let database_error = error
            .as_db_error()
            .expect("reference rejection must be a PostgreSQL constraint error");
        assert_eq!(
            database_error.code(),
            &postgres::error::SqlState::CHECK_VIOLATION
        );
    }
}
