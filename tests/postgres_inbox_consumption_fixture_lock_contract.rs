//! Regression contract for the shared inbox-consumption PostgreSQL fixture lock.

const FIXTURE_SOURCE: &str = include_str!("postgres_inbox_consumption_persistence.rs");

#[test]
fn fixture_serialization_is_database_visible_not_process_local() {
    assert!(
        !FIXTURE_SOURCE.contains("Mutex<()>"),
        "a process-local Mutex cannot serialize a fixed PostgreSQL schema across integration-test processes"
    );
    assert!(
        FIXTURE_SOURCE.contains("INBOX_CONSUMPTION_TEST_LOCK_KEY"),
        "the fixture must declare a stable database-visible advisory-lock identity"
    );
    assert!(
        FIXTURE_SOURCE.contains("pg_advisory_lock"),
        "the fixture guard must acquire its serialization boundary in PostgreSQL"
    );
}
