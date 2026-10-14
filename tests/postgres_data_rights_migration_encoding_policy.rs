//! Fail-closed encoding preflight for durable data-rights reference parity.
//!
//! The migration classifies Unicode scalars with `ascii(substr(...))`. That expression is a
//! code-point oracle only for UTF8 databases, so the migration must reject unsupported server
//! encodings before installing or replacing the immutable reference validator.

#[test]
fn migration_checks_utf8_before_installing_unicode_reference_validator() {
    let migration = include_str!("../migrations/0003_data_rights_propagation.sql");
    let encoding_guard = migration
        .find("current_setting('server_encoding') <> 'UTF8'")
        .expect("migration must fail closed when PostgreSQL server encoding is not UTF8");
    let validator = migration
        .find("CREATE OR REPLACE FUNCTION data_rights_reference_is_valid")
        .expect("migration must install the data-rights reference validator");

    assert!(
        encoding_guard < validator,
        "UTF8 must be verified before ascii(substr(...)) is used as a Unicode code-point oracle"
    );
    assert!(migration.contains(
        "data_rights reference parity requires PostgreSQL server_encoding UTF8"
    ));
}
