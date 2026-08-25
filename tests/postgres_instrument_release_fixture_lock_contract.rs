//! Regression contract for the fixed-schema instrument-release persistence fixture.

#[test]
fn instrument_release_persistence_fixture_serialization_is_database_visible() {
    let source = include_str!("postgres_instrument_release_persistence.rs");

    assert!(
        source.contains("pg_advisory_lock"),
        "a fixed PostgreSQL schema must be serialized by a database-visible advisory lock"
    );
    assert!(
        !source.contains("static INSTRUMENT_RELEASE_TEST_LOCK: Mutex"),
        "a process-local mutex cannot serialize separate test processes sharing TEST_DATABASE_URL"
    );
}
