//! Restart-safety contract for the data-rights processing-start `PostgreSQL` fixture.

#[test]
fn processing_fixture_uses_database_issued_schema_identity_and_cleanup() {
    let source = include_str!("postgres_data_rights_processing_start.rs");

    assert!(
        source.contains("pg_current_xact_id()::text"),
        "fixture schema identity must come from PostgreSQL rather than process-local state"
    );
    assert!(
        source.contains("DROP SCHEMA IF EXISTS"),
        "fixture schema ownership must include best-effort teardown"
    );
    assert!(
        !source.contains("std::process::id()"),
        "fixture and failure-injection schemas must not depend on recyclable process IDs"
    );
}
