//! Regression contract for item-delivery PostgreSQL fixture serialization.

const FIXTURE_SOURCE: &str = include_str!("postgres_item_delivery_persistence.rs");

#[test]
fn fixture_serialization_is_database_visible_not_process_local() {
    assert!(
        !FIXTURE_SOURCE.contains("std::sync::{Mutex, MutexGuard}"),
        "the fixed PostgreSQL fixture must not rely on a process-local Rust mutex"
    );
    assert!(
        !FIXTURE_SOURCE.contains("static ITEM_DELIVERY_TEST_LOCK: Mutex"),
        "the item-delivery persistence fixture must not retain the predecessor mutex"
    );
    assert!(
        FIXTURE_SOURCE.contains("ITEM_DELIVERY_PERSISTENCE_DATABASE_LOCK_KEY"),
        "the fixture must declare a dedicated database-visible advisory-lock identity"
    );
    assert!(
        FIXTURE_SOURCE.contains("SELECT pg_advisory_lock($1)"),
        "the fixture guard must acquire its serialization lease in PostgreSQL"
    );
}
