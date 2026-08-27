//! Contract for cross-process serialization of shared PostgreSQL family-catalog fixtures.
//!
//! Cargo may execute integration-test binaries in separate operating-system processes. A Rust
//! process-local mutex therefore cannot protect fixed test schemas from another test process using
//! the same `TEST_DATABASE_URL`. Fixture locks must be owned by PostgreSQL so every process observes
//! the same serialization boundary.

const FAMILY_CATALOG_TEST: &str = include_str!("postgres_instrument_family_catalog.rs");
const FAMILY_PAGINATION_TEST: &str = include_str!("postgres_instrument_family_pagination.rs");

fn assert_database_session_lock(test_source: &str, fixture_name: &str) {
    assert!(
        test_source.contains("SELECT pg_advisory_lock($1)"),
        "{fixture_name} must be serialized by a database-session advisory lock"
    );
    assert!(
        !test_source.contains("std::sync::{Mutex, MutexGuard}"),
        "{fixture_name} must not rely on a process-local mutex"
    );
    assert!(
        !test_source.contains("static FAMILY_CATALOG_TEST_LOCK: Mutex")
            && !test_source.contains("static FAMILY_PAGINATION_TEST_LOCK: Mutex"),
        "{fixture_name} must not rely on a process-local static mutex"
    );
}

#[test]
fn shared_family_catalog_fixture_uses_a_postgres_session_lock() {
    assert_database_session_lock(FAMILY_CATALOG_TEST, "family catalog fixture");
}

#[test]
fn shared_family_pagination_fixture_uses_a_postgres_session_lock() {
    assert_database_session_lock(FAMILY_PAGINATION_TEST, "family pagination fixture");
}
