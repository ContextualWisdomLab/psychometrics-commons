//! Public API contract for the durable audit persistence boundary.
//!
//! External crates must enter audit persistence through the validated public operations. Internal
//! SQL helpers intentionally remain crate-private so callers cannot bypass READ COMMITTED or
//! conflicting-replay classification by invoking the insert/read primitives directly.

const POSTGRES_AUDIT_SOURCE: &str = include_str!("../src/postgres_audit.rs");

#[test]
fn audit_sql_helpers_are_not_public_crate_api() {
    for helper in [
        "insert_audit_row",
        "classify_persisted_audit",
        "query_required_audit_row",
        "query_optional_audit_row",
    ] {
        assert!(
            POSTGRES_AUDIT_SOURCE.contains(&format!("pub(crate) fn {helper}")),
            "{helper} must remain crate-private"
        );
        assert!(
            !POSTGRES_AUDIT_SOURCE.contains(&format!("pub fn {helper}")),
            "{helper} must not be callable by external crates"
        );
    }
}
