//! Namespace-resolution contracts for the audit migration.
//!
//! Migration 0040 must inspect retention capabilities in the schema it is actively migrating. An
//! unqualified `PostgreSQL` routine lookup follows `search_path`, so a same-named routine in another
//! schema must never decide whether the audit mutation guard is installed or preserved.

const AUDIT_SCHEMA_MIGRATION: &str = include_str!("../migrations/0040_audit_evidence_record.sql");

#[test]
fn core_migration_scopes_retention_capability_lookup_to_current_schema() {
    assert!(
        AUDIT_SCHEMA_MIGRATION.contains(
            "to_regprocedure(\n        format('%I.expire_audit_evidence_before(text,bigint)', current_schema())\n    )"
        ),
        "migration 0040 must schema-qualify the retention-routine lookup with current_schema()"
    );
    assert!(
        !AUDIT_SCHEMA_MIGRATION
            .contains("to_regprocedure('expire_audit_evidence_before(text,bigint)')"),
        "an unqualified retention-routine lookup is search_path-dependent and must not reappear"
    );
}
