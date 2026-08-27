//! Contract for cross-process serialization of the shared PostgreSQL family-catalog fixture.
//!
//! Cargo may execute integration-test binaries in separate operating-system processes. A Rust
//! process-local mutex therefore cannot protect the fixed `instrument_family_catalog_test` schema
//! from another test process using the same `TEST_DATABASE_URL`. The fixture lock must be owned by
//! PostgreSQL so every process observes the same serialization boundary.

const FAMILY_CATALOG_TEST: &str = include_str!("postgres_instrument_family_catalog.rs");

#[test]
fn shared_family_catalog_fixture_uses_a_postgres_session_lock() {
    assert!(
        FAMILY_CATALOG_TEST.contains("SELECT pg_advisory_lock($1)"),
        "the fixed PostgreSQL schema must be serialized by a database-session advisory lock"
    );
    assert!(
        !FAMILY_CATALOG_TEST.contains("std::sync::{Mutex, MutexGuard}"),
        "a process-local mutex cannot serialize the shared PostgreSQL fixture across test binaries"
    );
    assert!(
        !FAMILY_CATALOG_TEST.contains("static FAMILY_CATALOG_TEST_LOCK: Mutex"),
        "the family catalog fixture must not rely on a process-local static mutex"
    );
}
