//! `PostgreSQL` 18 persistence for append-only, tenant-scoped product audit evidence.
//!
//! Callers own the database connection, transaction, credentials, and authorization decision that
//! precedes persistence. This module stores only the already-minimized audit contract from
//! [`crate::audit`]; raw assessment payloads, bearer credentials, and provider prompts are outside
//! this boundary.

use crate::audit::{AuditEvidence, AuditEvidenceInput, AuditOutcome};
use crate::reference::normalized_reference;
use postgres::{GenericClient, Transaction};
use std::error::Error;
use std::fmt::{Display, Formatter};

const AUDIT_EVIDENCE_MIGRATION: &str = include_str!("../migrations/0040_audit_evidence_record.sql");

/// Outcome of persisting one immutable audit record.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum AuditPersistenceDisposition {
    /// The exact audit identity was newly inserted.
    Inserted,
    /// The same immutable evidence already existed under that identity.
    Duplicate,
}

/// Fail-closed error for durable audit evidence.
#[derive(Debug)]
#[non_exhaustive]
pub enum AuditPersistenceError {
    /// A caller-supplied tenant or audit reference was not exact canonical opaque identity.
    InvalidReference,
    /// The same audit-event identity already exists with different immutable evidence.
    ConflictingReplay,
    /// Persisted evidence cannot be reconstructed under the current domain contract.
    CorruptHistory,
    /// The Unix-millisecond event time cannot be represented by the database schema.
    TimestampOutOfRange,
    /// Audit persistence requires `PostgreSQL` `READ COMMITTED` isolation.
    UnsupportedIsolationLevel,
    /// `PostgreSQL` rejected or could not execute the operation.
    Database(postgres::Error),
}

impl Display for AuditPersistenceError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidReference => {
                "audit persistence references must be exact canonical opaque values"
            }
            Self::ConflictingReplay => {
                "audit event identity was replayed with conflicting immutable evidence"
            }
            Self::CorruptHistory => "persisted audit evidence is inconsistent or unsupported",
            Self::TimestampOutOfRange => {
                "audit event timestamp exceeds the supported PostgreSQL integer range"
            }
            Self::UnsupportedIsolationLevel => {
                "audit persistence requires read committed isolation"
            }
            Self::Database(_) => "PostgreSQL audit persistence failed",
        })
    }
}

impl Error for AuditPersistenceError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Database(error) => Some(error),
            _ => None,
        }
    }
}

impl From<postgres::Error> for AuditPersistenceError {
    fn from(error: postgres::Error) -> Self {
        Self::Database(error)
    }
}

/// Apply the idempotent append-only audit migration.
///
/// # Errors
///
/// Returns the database error when the schema, constraints, index, or immutability triggers cannot
/// be created.
pub fn apply_audit_evidence_migration(
    client: &mut impl GenericClient,
) -> Result<(), postgres::Error> {
    client.batch_execute(AUDIT_EVIDENCE_MIGRATION)
}

/// Persist one exact audit record under `READ COMMITTED` isolation.
///
/// Exact replay is idempotent. Reusing `audit_event_ref` with a different tenant, actor, purpose,
/// action, resource, outcome, digest, or event time fails closed rather than rewriting history.
/// The insert and verification read intentionally use separate commands: under `PostgreSQL`
/// `READ COMMITTED`, `ON CONFLICT DO NOTHING` can observe a concurrent unique-key winner that is
/// absent from the insert command's snapshot, while the following command receives a fresh
/// snapshot and can verify that committed winner exactly.
///
/// # Errors
///
/// Returns a typed error for unsupported isolation, timestamp overflow, conflicting replay, or a
/// database failure.
pub fn persist_audit_evidence(
    transaction: &mut Transaction<'_>,
    evidence: &AuditEvidence,
) -> Result<AuditPersistenceDisposition, AuditPersistenceError> {
    require_read_committed(transaction)?;
    let occurred_at_unix_ms = i64::try_from(evidence.occurred_at_unix_ms())
        .map_err(|_| AuditPersistenceError::TimestampOutOfRange)?;
    let inserted_rows = transaction.execute(
        r"INSERT INTO audit_evidence_record (
              audit_event_ref, tenant_ref, actor_ref, purpose_code, action_code, resource_ref,
              outcome_code, evidence_digest, occurred_at_unix_ms
          ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)
          ON CONFLICT (audit_event_ref) DO NOTHING",
        &[
            &evidence.audit_event_ref(),
            &evidence.tenant_ref(),
            &evidence.actor_ref(),
            &evidence.purpose_code(),
            &evidence.action_code(),
            &evidence.resource_ref(),
            &evidence.outcome().as_code(),
            &evidence.evidence_digest(),
            &occurred_at_unix_ms,
        ],
    )?;
    let row = transaction.query_one(
        r"SELECT tenant_ref, actor_ref, purpose_code, action_code, resource_ref, outcome_code,
                 evidence_digest, occurred_at_unix_ms
          FROM audit_evidence_record
          WHERE audit_event_ref = $1",
        &[&evidence.audit_event_ref()],
    )?;

    let stored_tenant_ref: String = row.get(0);
    let stored_actor_ref: String = row.get(1);
    let stored_purpose_code: String = row.get(2);
    let stored_action_code: String = row.get(3);
    let stored_resource_ref: String = row.get(4);
    let stored_outcome_code: String = row.get(5);
    let stored_evidence_digest: String = row.get(6);
    let stored_occurred_at_unix_ms: i64 = row.get(7);

    if stored_tenant_ref == evidence.tenant_ref()
        && stored_actor_ref == evidence.actor_ref()
        && stored_purpose_code == evidence.purpose_code()
        && stored_action_code == evidence.action_code()
        && stored_resource_ref == evidence.resource_ref()
        && stored_outcome_code == evidence.outcome().as_code()
        && stored_evidence_digest == evidence.evidence_digest()
        && stored_occurred_at_unix_ms == occurred_at_unix_ms
    {
        Ok(if inserted_rows == 1 {
            AuditPersistenceDisposition::Inserted
        } else {
            AuditPersistenceDisposition::Duplicate
        })
    } else {
        Err(AuditPersistenceError::ConflictingReplay)
    }
}

/// Load one tenant-scoped audit record by opaque event identity.
///
/// Another tenant receives `None` for the same event reference so this read path does not reveal
/// cross-tenant existence. Persisted values are reconstructed through the current domain
/// constructor and corrupt history therefore fails closed.
///
/// # Errors
///
/// Returns [`AuditPersistenceError::InvalidReference`] for malformed caller aliases,
/// [`AuditPersistenceError::UnsupportedIsolationLevel`] outside `READ COMMITTED`,
/// [`AuditPersistenceError::CorruptHistory`] for unsupported stored evidence, or a database error.
pub fn load_audit_evidence(
    transaction: &mut Transaction<'_>,
    tenant_ref: &str,
    audit_event_ref: &str,
) -> Result<Option<AuditEvidence>, AuditPersistenceError> {
    require_read_committed(transaction)?;
    let tenant_ref = required_reference(tenant_ref)?;
    let audit_event_ref = required_reference(audit_event_ref)?;
    let row = transaction.query_opt(
        r"SELECT actor_ref, purpose_code, action_code, resource_ref, outcome_code,
                  evidence_digest, occurred_at_unix_ms
           FROM audit_evidence_record
           WHERE tenant_ref = $1 AND audit_event_ref = $2",
        &[&tenant_ref, &audit_event_ref],
    )?;
    let Some(row) = row else {
        return Ok(None);
    };

    let actor_ref: String = row.get(0);
    let purpose_code: String = row.get(1);
    let action_code: String = row.get(2);
    let resource_ref: String = row.get(3);
    let outcome_code: String = row.get(4);
    let evidence_digest: String = row.get(5);
    let occurred_at_unix_ms: i64 = row.get(6);
    let occurred_at_unix_ms =
        u64::try_from(occurred_at_unix_ms).map_err(|_| AuditPersistenceError::CorruptHistory)?;
    let outcome = AuditOutcome::from_code(&outcome_code)
        .map_err(|_| AuditPersistenceError::CorruptHistory)?;

    AuditEvidence::new(AuditEvidenceInput {
        audit_event_ref,
        tenant_ref,
        actor_ref: &actor_ref,
        purpose_code: &purpose_code,
        action_code: &action_code,
        resource_ref: &resource_ref,
        outcome,
        evidence_digest: &evidence_digest,
        occurred_at_unix_ms,
    })
    .map(Some)
    .map_err(|_| AuditPersistenceError::CorruptHistory)
}

fn required_reference(reference: &str) -> Result<&str, AuditPersistenceError> {
    match normalized_reference(reference) {
        Some(normalized) if normalized == reference => Ok(reference),
        _ => Err(AuditPersistenceError::InvalidReference),
    }
}

fn require_read_committed(transaction: &mut Transaction<'_>) -> Result<(), AuditPersistenceError> {
    let row = transaction.query_one("SHOW transaction_isolation", &[])?;
    let isolation: String = row.get(0);
    if isolation == "read committed" {
        Ok(())
    } else {
        Err(AuditPersistenceError::UnsupportedIsolationLevel)
    }
}

#[cfg(test)]
mod tests {
    use super::{required_reference, AuditPersistenceError};

    #[test]
    fn caller_aliases_fail_closed_before_database_access() {
        for invalid in ["", " ", " audit_event_alias ", "123"] {
            assert!(matches!(
                required_reference(invalid),
                Err(AuditPersistenceError::InvalidReference)
            ));
        }
        assert_eq!(
            required_reference("audit_event_alpha").unwrap(),
            "audit_event_alpha"
        );
    }
}
