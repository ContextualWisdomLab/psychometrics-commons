//! `PostgreSQL` deployment-retention migration for product audit evidence.
//!
//! Retention is intentionally split from the core append-only audit migration because the product
//! does not own one universal retention period. Deployments apply this capability after
//! [`crate::postgres_audit::apply_audit_evidence_migration`], then explicitly grant the bounded
//! expiry routine only to their approved maintenance authority.

use postgres::GenericClient;

const AUDIT_EVIDENCE_RETENTION_MIGRATION: &str =
    include_str!("../migrations/0041_audit_evidence_retention.sql");
const AUDIT_EVIDENCE_OWNER_HARDENING_MIGRATION: &str =
    include_str!("../migrations/0042_audit_evidence_owner_hardening.sql");

/// Apply the idempotent deployment-retention capability after the core audit schema exists.
///
/// This creates no schedule and grants no deployment role. It installs the bounded database
/// primitive and, in the same migration batch, rebinds the SECURITY DEFINER routine and mutation
/// guard to the dedicated non-login audit owner. The execution grant and cutoff still come only
/// from deployment policy.
///
/// # Errors
///
/// Returns the `PostgreSQL` error when the audit table is absent, the dedicated owner boundary is
/// unsafe, or the hardened retention routine and mutation guard cannot be installed.
pub fn apply_audit_evidence_retention_migration(
    client: &mut impl GenericClient,
) -> Result<(), postgres::Error> {
    let migration = [
        AUDIT_EVIDENCE_RETENTION_MIGRATION,
        "\n",
        AUDIT_EVIDENCE_OWNER_HARDENING_MIGRATION,
    ]
    .concat();
    client.batch_execute(&migration)
}
