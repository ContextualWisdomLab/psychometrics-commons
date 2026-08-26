//! PostgreSQL deployment-retention migration for product audit evidence.
//!
//! Retention is intentionally split from the core append-only audit migration because the product
//! does not own one universal retention period. Deployments apply this capability after
//! [`crate::postgres_audit::apply_audit_evidence_migration`], then explicitly grant the bounded
//! expiry routine only to their approved maintenance authority.

use postgres::GenericClient;

const AUDIT_EVIDENCE_RETENTION_MIGRATION: &str =
    include_str!("../migrations/0041_audit_evidence_retention.sql");

/// Apply the idempotent deployment-retention capability after the core audit schema exists.
///
/// This creates no schedule and grants no deployment role. It only installs the bounded database
/// primitive whose execution grant and cutoff must come from deployment policy.
///
/// # Errors
///
/// Returns the PostgreSQL error when the audit table is absent or the hardened retention routine
/// and mutation guard cannot be installed.
pub fn apply_audit_evidence_retention_migration(
    client: &mut impl GenericClient,
) -> Result<(), postgres::Error> {
    client.batch_execute(AUDIT_EVIDENCE_RETENTION_MIGRATION)
}
