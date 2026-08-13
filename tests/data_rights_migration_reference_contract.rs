//! Schema-source contract for opaque public references in data-rights persistence.

const MIGRATION: &str = include_str!("../migrations/0003_data_rights_propagation.sql");

#[test]
fn data_rights_migration_declares_opaque_reference_guards() {
    for constraint_name in [
        "data_rights_request_ref_opaque",
        "data_rights_tenant_ref_opaque",
        "data_rights_participant_ref_opaque",
        "data_rights_scope_ref_opaque",
        "data_rights_dependent_system_ref_opaque",
        "data_rights_propagation_source_ref_opaque",
        "data_rights_propagation_event_ref_opaque",
    ] {
        assert!(
            MIGRATION.contains(constraint_name),
            "missing opaque-reference constraint: {constraint_name}"
        );
    }

    assert!(MIGRATION.contains("[[:digit:]]"));
    assert!(MIGRATION.contains("^[[:digit:]+,.eE-]+$"));
}
