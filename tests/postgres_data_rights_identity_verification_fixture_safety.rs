//! Restart-safety contract for the data-rights identity-verification `PostgreSQL` fixture.

#[test]
fn identity_verification_fixture_uses_database_issued_schema_identity_and_cleanup() {
    let source = include_str!("postgres_data_rights_identity_verification.rs");

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
