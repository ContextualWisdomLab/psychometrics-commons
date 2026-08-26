//! Guard the migration repair marker against drifting away from validator semantics.

const BASE_MIGRATION: &str = include_str!("../migrations/0001_integration_delivery.sql");
const LEASE_MIGRATION: &str = include_str!("../migrations/0013_outbox_delivery_lease.sql");

fn uses_validator_fingerprint(sql: &str, marker_prefix: &str) -> bool {
    sql.contains(marker_prefix)
        && sql.contains("pg_catalog.md5(")
        && sql.contains("pg_catalog.pg_get_functiondef(")
        && sql.contains("integration_reference_is_valid(text)")
}

#[test]
fn reference_constraint_markers_follow_the_installed_validator_definition() {
    assert!(
        uses_validator_fingerprint(
            BASE_MIGRATION,
            "psychometrics-commons:integration-reference:"
        ),
        "base reference constraints must derive their repair marker from the installed validator definition"
    );
    assert!(
        uses_validator_fingerprint(
            LEASE_MIGRATION,
            "psychometrics-commons:integration-lease-reference:"
        ),
        "lease reference constraints must derive their repair marker from the same installed validator definition"
    );

    for migration in [BASE_MIGRATION, LEASE_MIGRATION] {
        assert!(
            !migration.contains("integration-reference:v1")
                && !migration.contains("integration-lease-reference:v1"),
            "a fixed v1 marker can silently skip revalidation after validator semantics change"
        );
    }
}
