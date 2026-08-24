//! Regression contract for item-delivery `PostgreSQL` fixture serialization.

const FIXTURE_SOURCE: &str = include_str!("postgres_item_delivery_persistence.rs");

#[test]
fn fixture_serialization_is_database_visible_not_process_local() {
    assert!(
        !FIXTURE_SOURCE.contains("ITEM_DELIVERY_TEST_LOCK"),
        "the item-delivery persistence fixture must not retain the predecessor process-local lock"
    );
    assert!(
        FIXTURE_SOURCE.contains("ITEM_DELIVERY_PERSISTENCE_DATABASE_LOCK_KEY"),
        "the fixture must declare a dedicated database-visible advisory-lock identity"
    );
    assert!(
        FIXTURE_SOURCE.contains("pg_advisory_lock"),
        "the fixture guard must acquire its serialization lease in PostgreSQL"
    );
}
