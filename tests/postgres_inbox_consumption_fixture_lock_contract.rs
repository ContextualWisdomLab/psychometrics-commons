//! Regression contract for the shared inbox-consumption PostgreSQL fixture lock.

const FIXTURE_SOURCE: &str = include_str!("postgres_inbox_consumption_persistence.rs");

#[test]
fn fixture_serialization_is_database_visible_not_process_local() {
    assert!(
        !FIXTURE_SOURCE.contains("Mutex<()>"),
        "a process-local Mutex cannot serialize a fixed PostgreSQL schema across integration-test processes"
    );

    let guard_start = FIXTURE_SOURCE
        .find("fn test_guard() -> Client")
        .expect("the persistence fixture must expose its database guard");
    let client_start = FIXTURE_SOURCE
        .find("fn test_client() -> Client")
        .expect("the persistence fixture must expose its schema-scoped client");
    let guard_body = &FIXTURE_SOURCE[guard_start..client_start];

    assert!(
        !guard_body.contains("test_client()"),
        "the guard must not touch shared-schema setup before acquiring its database lock"
    );
    assert!(
        guard_body.contains("INBOX_CONSUMPTION_TEST_LOCK_KEY"),
        "the fixture guard must bind the stable database-visible advisory-lock identity"
    );
    let lock_index = guard_body
        .find("pg_advisory_lock")
        .expect("the fixture guard must acquire its serialization boundary in PostgreSQL");
    let schema_index = guard_body
        .find("CREATE SCHEMA IF NOT EXISTS inbox_consumption_persistence_test")
        .expect("shared-schema initialization must remain inside the guarded setup boundary");
    assert!(
        lock_index < schema_index,
        "the PostgreSQL advisory lock must be acquired before shared-schema initialization"
    );

    let first_test_start = FIXTURE_SOURCE[client_start..]
        .find("#[test]")
        .map(|offset| client_start + offset)
        .expect("the fixture should retain its integration tests");
    let client_body = &FIXTURE_SOURCE[client_start..first_test_start];
    assert!(
        !client_body.contains("CREATE SCHEMA"),
        "ordinary test clients must not mutate the fixed schema outside the guard"
    );
}
