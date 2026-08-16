//! `PostgreSQL` 18 persistence for explicit research-contribution evidence.
//!
//! The operational participant reference is retained only in the product-owned
//! restricted research boundary. Public research artifacts must use the separate
//! pseudonymous research participant reference and must never expose these tables.
//!
//! A [`ResearchContribution`] intentionally does not carry operational participant
//! identity. To prevent write-time snapshot substitution from rebinding identity,
//! callers must first persist the exact active research-consent snapshot projection.
//! Contribution persistence then resolves participant and scope from that durable
//! binding rather than trusting a second in-memory snapshot. A new contribution
//! start re-checks the latest research-purpose `consent_event` for that
//! participant. That event must still be `granted` for the contribution's exact
//! scope, matching [`ConsentSnapshot::is_granted`] and
//! [`ConsentSnapshot::active_research_scope`]. Same-millisecond events use
//! `consent_event.created_at` append order, not `event_ref` sort order. A later
//! grant or revoke for another scope therefore replaces the prior scope as the
//! live write capability.
//! Exact replay and withdrawal of already stored evidence stay allowed. The
//! caller owns credentials and the surrounding transaction boundary.

use crate::consent::{ConsentPurpose, ConsentSnapshot, ResearchContribution};
use crate::reference::normalized_reference;
use postgres::Transaction;
use std::error::Error;
use std::fmt::{Display, Formatter};

const RESEARCH_CONTRIBUTION_MIGRATION: &str =
    include_str!("../migrations/0017_research_contribution.sql");

/// Outcome of persisting research-consent or contribution lifecycle evidence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ResearchContributionPersistenceDisposition {
    /// New immutable evidence was inserted.
    Inserted,
    /// The exact immutable evidence already existed.
    Duplicate,
}

/// Fail-closed error for durable research-consent and contribution persistence.
#[derive(Debug)]
#[non_exhaustive]
pub enum ResearchContributionPersistenceError {
    /// A contribution, participant, snapshot, scope, form, or withdrawal identity is invalid.
    InvalidReference,
    /// The supplied snapshot has no active explicit research-contribution grant.
    ResearchConsentRequired,
    /// Contribution persistence ran before its immutable consent snapshot projection existed.
    ConsentSnapshotMissing,
    /// Contribution scope does not match the durable authorizing snapshot binding.
    ConsentSnapshotMismatch,
    /// Operational and research participant namespaces were incorrectly reused.
    OperationalIdentityReuse,
    /// Contribution or withdrawal time cannot be represented safely in `PostgreSQL`.
    InvalidTimestamp,
    /// An immutable snapshot, contribution, or withdrawal identity was rebound to other evidence.
    ConflictingReplay,
    /// Research-contribution persistence requires `PostgreSQL` `READ COMMITTED` isolation.
    UnsupportedIsolationLevel,
    /// `PostgreSQL` rejected or could not execute the persistence operation.
    Database(postgres::Error),
}

impl Display for ResearchContributionPersistenceError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidReference => {
                "research persistence references must be opaque durable values"
            }
            Self::ResearchConsentRequired => {
                "research consent snapshot requires an active explicit research grant"
            }
            Self::ConsentSnapshotMissing => {
                "research contribution requires a durable authorizing consent snapshot"
            }
            Self::ConsentSnapshotMismatch => {
                "research contribution scope does not match its durable consent snapshot"
            }
            Self::OperationalIdentityReuse => {
                "research participant reference must differ from the operational participant"
            }
            Self::InvalidTimestamp => {
                "research contribution timestamp must be positive and fit PostgreSQL bigint"
            }
            Self::ConflictingReplay => {
                "research persistence identity was replayed with conflicting evidence"
            }
            Self::UnsupportedIsolationLevel => {
                "research contribution persistence requires read committed isolation"
            }
            Self::Database(_) => "PostgreSQL research-contribution persistence failed",
        })
    }
}

impl Error for ResearchContributionPersistenceError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Database(error) => Some(error),
            _ => None,
        }
    }
}

impl From<postgres::Error> for ResearchContributionPersistenceError {
    fn from(error: postgres::Error) -> Self {
        Self::Database(error)
    }
}

/// Apply the idempotent research-contribution migration to a `PostgreSQL` connection.
///
/// # Errors
///
/// Returns the `PostgreSQL` error when the migration cannot be applied.
pub fn apply_research_contribution_migration(
    client: &mut impl postgres::GenericClient,
) -> Result<(), postgres::Error> {
    client.batch_execute(RESEARCH_CONTRIBUTION_MIGRATION)
}

/// Persist the immutable active research-consent projection used for contribution authorization.
///
/// This is a purpose-specific durable projection of an in-memory [`ConsentSnapshot`],
/// not a replacement for the general consent ledger. It binds one snapshot reference
/// to its operational participant, active research scope, and exact consent-form
/// version before any contribution row may reference it. Exact replay is idempotent;
/// reusing a snapshot reference for another binding fails closed.
///
/// # Errors
///
/// Returns [`ResearchContributionPersistenceError`] when the snapshot lacks an
/// active research grant, contains an invalid reference, reuses a research
/// participant as an operational identity, conflicts with an existing immutable
/// binding, uses unsupported transaction isolation, or the database fails.
pub fn persist_research_consent_snapshot(
    transaction: &mut Transaction<'_>,
    consent_snapshot: &ConsentSnapshot,
) -> Result<ResearchContributionPersistenceDisposition, ResearchContributionPersistenceError> {
    require_read_committed(transaction)?;
    let snapshot_ref = required_reference(consent_snapshot.snapshot_ref())?;
    let participant_ref = required_reference(consent_snapshot.participant_ref())?;
    let research_scope_ref = consent_snapshot
        .active_research_scope()
        .and_then(normalized_reference)
        .ok_or(ResearchContributionPersistenceError::ResearchConsentRequired)?;
    let consent_form_version_ref = consent_snapshot
        .active_form_version(ConsentPurpose::ResearchContribution)
        .and_then(normalized_reference)
        .ok_or(ResearchContributionPersistenceError::ResearchConsentRequired)?;
    require_operational_ref_is_not_research_identity(transaction, participant_ref)?;

    let inserted = transaction.execute(
        "INSERT INTO research_consent_snapshot (\
             consent_snapshot_ref, participant_ref, research_scope_ref, consent_form_version_ref\
         ) VALUES ($1, $2, $3, $4) \
         ON CONFLICT (consent_snapshot_ref) DO NOTHING",
        &[
            &snapshot_ref,
            &participant_ref,
            &research_scope_ref,
            &consent_form_version_ref,
        ],
    )?;
    if inserted == 1 {
        return Ok(ResearchContributionPersistenceDisposition::Inserted);
    }

    let row = transaction.query_one(
        "SELECT participant_ref, research_scope_ref, consent_form_version_ref \
         FROM research_consent_snapshot WHERE consent_snapshot_ref = $1",
        &[&snapshot_ref],
    )?;
    let stored_participant_ref: String = row.get(0);
    let stored_research_scope_ref: String = row.get(1);
    let stored_form_version_ref: String = row.get(2);
    if stored_participant_ref == participant_ref
        && stored_research_scope_ref == research_scope_ref
        && stored_form_version_ref == consent_form_version_ref
    {
        Ok(ResearchContributionPersistenceDisposition::Duplicate)
    } else {
        Err(ResearchContributionPersistenceError::ConflictingReplay)
    }
}

/// Persist one contribution start record and optional immutable withdrawal evidence.
///
/// The contribution's `consent_snapshot_ref` must already exist in
/// `research_consent_snapshot`. Operational participant identity is read only from
/// that durable binding, never from a write-time snapshot argument. The stored
/// scope must equal the contribution's immutable scope and the bound operational
/// participant must differ from the pseudonymous research participant. A new
/// contribution start also requires the latest research-purpose `consent_event`
/// for that participant to still be `granted` for the contribution's exact scope.
/// Latest means last-appended (`occurred_at_unix_ms`, then `created_at`).
///
/// The contribution start record is append-only. Withdrawal is stored as a separate
/// event, so replaying original active evidence after withdrawal cannot reactivate
/// or erase the withdrawal. Exact replay is idempotent; rebinding immutable identity,
/// scope, time, or withdrawal evidence fails closed.
///
/// # Errors
///
/// Returns [`ResearchContributionPersistenceError`] for a missing/mismatched
/// consent binding, namespace reuse, invalid references/timestamps, conflicting
/// replay, unsupported isolation, or a database failure.
pub fn persist_research_contribution(
    transaction: &mut Transaction<'_>,
    contribution: &ResearchContribution,
) -> Result<ResearchContributionPersistenceDisposition, ResearchContributionPersistenceError> {
    require_read_committed(transaction)?;
    let evidence = validated_contribution_evidence(transaction, contribution)?;
    let inserted_contribution = persist_contribution_start(transaction, &evidence)?;
    let inserted_withdrawal = match evidence.withdrawal {
        Some(withdrawal) => persist_withdrawal(transaction, evidence.contribution_ref, withdrawal)?,
        None => false,
    };

    if inserted_contribution || inserted_withdrawal {
        Ok(ResearchContributionPersistenceDisposition::Inserted)
    } else {
        Ok(ResearchContributionPersistenceDisposition::Duplicate)
    }
}

#[derive(Clone)]
struct ValidatedEvidence<'a> {
    contribution_ref: &'a str,
    participant_ref: String,
    research_participant_ref: &'a str,
    consent_snapshot_ref: &'a str,
    research_scope_ref: &'a str,
    started_at_unix_ms: i64,
    withdrawal: Option<ValidatedWithdrawal<'a>>,
}

#[derive(Clone, Copy)]
struct ValidatedWithdrawal<'a> {
    withdrawal_event_ref: &'a str,
    withdrawn_at_unix_ms: i64,
}

fn validated_contribution_evidence<'a>(
    transaction: &mut Transaction<'_>,
    contribution: &'a ResearchContribution,
) -> Result<ValidatedEvidence<'a>, ResearchContributionPersistenceError> {
    let contribution_ref = required_reference(contribution.contribution_ref())?;
    let research_participant_ref = required_reference(contribution.research_participant_ref())?;
    let consent_snapshot_ref = required_reference(contribution.consent_snapshot_ref())?;
    let research_scope_ref = required_reference(contribution.research_scope_ref())?;
    let started_at_unix_ms = bounded_timestamp(contribution.started_at_unix_ms())?;

    let binding = transaction.query_opt(
        "SELECT participant_ref, research_scope_ref \
         FROM research_consent_snapshot WHERE consent_snapshot_ref = $1",
        &[&consent_snapshot_ref],
    )?;
    let Some(binding) = binding else {
        return Err(ResearchContributionPersistenceError::ConsentSnapshotMissing);
    };
    let participant_ref: String = binding.get(0);
    let bound_scope_ref: String = binding.get(1);
    if bound_scope_ref != research_scope_ref {
        return Err(ResearchContributionPersistenceError::ConsentSnapshotMismatch);
    }
    if participant_ref == research_participant_ref {
        return Err(ResearchContributionPersistenceError::OperationalIdentityReuse);
    }

    let withdrawal = match contribution.withdrawal_evidence() {
        Some((event_ref, withdrawn_at_unix_ms)) => Some(ValidatedWithdrawal {
            withdrawal_event_ref: required_reference(event_ref)?,
            withdrawn_at_unix_ms: bounded_timestamp(withdrawn_at_unix_ms)?,
        }),
        None => None,
    };

    Ok(ValidatedEvidence {
        contribution_ref,
        participant_ref,
        research_participant_ref,
        consent_snapshot_ref,
        research_scope_ref,
        started_at_unix_ms,
        withdrawal,
    })
}

fn persist_contribution_start(
    transaction: &mut Transaction<'_>,
    evidence: &ValidatedEvidence<'_>,
) -> Result<bool, ResearchContributionPersistenceError> {
    let existing = transaction.query_opt(
        "SELECT participant_ref, research_participant_ref, consent_snapshot_ref, \
                research_scope_ref, started_at_unix_ms \
         FROM research_contribution WHERE contribution_ref = $1",
        &[&evidence.contribution_ref],
    )?;
    if existing.is_none() {
        require_live_research_grant(
            transaction,
            &evidence.participant_ref,
            evidence.research_scope_ref,
        )?;
        require_namespace_separation(transaction, evidence.research_participant_ref)?;
        require_operational_ref_is_not_research_identity(transaction, &evidence.participant_ref)?;
    }

    let inserted = transaction.execute(
        "INSERT INTO research_contribution (\
             contribution_ref, participant_ref, research_participant_ref, \
             consent_snapshot_ref, research_scope_ref, started_at_unix_ms\
         ) VALUES ($1, $2, $3, $4, $5, $6) \
         ON CONFLICT (contribution_ref) DO NOTHING",
        &[
            &evidence.contribution_ref,
            &evidence.participant_ref,
            &evidence.research_participant_ref,
            &evidence.consent_snapshot_ref,
            &evidence.research_scope_ref,
            &evidence.started_at_unix_ms,
        ],
    )?;
    if inserted == 1 {
        return Ok(true);
    }

    let row = transaction.query_one(
        "SELECT participant_ref, research_participant_ref, consent_snapshot_ref, \
                research_scope_ref, started_at_unix_ms \
         FROM research_contribution WHERE contribution_ref = $1",
        &[&evidence.contribution_ref],
    )?;
    let stored_participant_ref: String = row.get(0);
    let stored_research_participant_ref: String = row.get(1);
    let stored_consent_snapshot_ref: String = row.get(2);
    let stored_research_scope_ref: String = row.get(3);
    let stored_started_at_unix_ms: i64 = row.get(4);

    if stored_participant_ref == evidence.participant_ref
        && stored_research_participant_ref == evidence.research_participant_ref
        && stored_consent_snapshot_ref == evidence.consent_snapshot_ref
        && stored_research_scope_ref == evidence.research_scope_ref
        && stored_started_at_unix_ms == evidence.started_at_unix_ms
    {
        Ok(false)
    } else {
        Err(ResearchContributionPersistenceError::ConflictingReplay)
    }
}

fn persist_withdrawal(
    transaction: &mut Transaction<'_>,
    contribution_ref: &str,
    withdrawal: ValidatedWithdrawal<'_>,
) -> Result<bool, ResearchContributionPersistenceError> {
    let reused_event = transaction.query_opt(
        "SELECT 1 FROM research_withdrawal_event \
         WHERE withdrawal_event_ref = $1 AND contribution_ref <> $2",
        &[&withdrawal.withdrawal_event_ref, &contribution_ref],
    )?;
    if reused_event.is_some() {
        return Err(ResearchContributionPersistenceError::ConflictingReplay);
    }

    let inserted = transaction.execute(
        "INSERT INTO research_withdrawal_event (\
             contribution_ref, withdrawal_event_ref, withdrawn_at_unix_ms\
         ) VALUES ($1, $2, $3) \
         ON CONFLICT (contribution_ref) DO NOTHING",
        &[
            &contribution_ref,
            &withdrawal.withdrawal_event_ref,
            &withdrawal.withdrawn_at_unix_ms,
        ],
    )?;
    if inserted == 1 {
        return Ok(true);
    }

    let row = transaction.query_one(
        "SELECT withdrawal_event_ref, withdrawn_at_unix_ms \
         FROM research_withdrawal_event WHERE contribution_ref = $1",
        &[&contribution_ref],
    )?;
    let stored_event_ref: String = row.get(0);
    let stored_withdrawn_at_unix_ms: i64 = row.get(1);
    if stored_event_ref == withdrawal.withdrawal_event_ref
        && stored_withdrawn_at_unix_ms == withdrawal.withdrawn_at_unix_ms
    {
        Ok(false)
    } else {
        Err(ResearchContributionPersistenceError::ConflictingReplay)
    }
}

fn require_live_research_grant(
    transaction: &mut Transaction<'_>,
    participant_ref: &str,
    research_scope_ref: &str,
) -> Result<(), ResearchContributionPersistenceError> {
    let row = transaction.query_opt(
        "SELECT consent_decision, research_scope_ref \
         FROM consent_event \
         WHERE participant_ref = $1 \
           AND consent_purpose = 'research_contribution' \
         ORDER BY occurred_at_unix_ms DESC, created_at DESC \
         LIMIT 1",
        &[&participant_ref],
    )?;
    let Some(row) = row else {
        return Err(ResearchContributionPersistenceError::ResearchConsentRequired);
    };
    let decision: String = row.get(0);
    let live_scope_ref: String = row.get(1);
    if decision == "granted" && live_scope_ref == research_scope_ref {
        Ok(())
    } else {
        Err(ResearchContributionPersistenceError::ResearchConsentRequired)
    }
}

fn require_namespace_separation(
    transaction: &mut Transaction<'_>,
    research_participant_ref: &str,
) -> Result<(), ResearchContributionPersistenceError> {
    let collision = transaction.query_opt(
        "SELECT 1 FROM research_consent_snapshot WHERE participant_ref = $1 \
         UNION ALL \
         SELECT 1 FROM research_contribution WHERE participant_ref = $1 \
         UNION ALL \
         SELECT 1 FROM research_contribution WHERE research_participant_ref = $1 \
         LIMIT 1",
        &[&research_participant_ref],
    )?;
    if collision.is_some() {
        return Err(ResearchContributionPersistenceError::OperationalIdentityReuse);
    }
    Ok(())
}

fn require_operational_ref_is_not_research_identity(
    transaction: &mut Transaction<'_>,
    participant_ref: &str,
) -> Result<(), ResearchContributionPersistenceError> {
    let collision = transaction.query_opt(
        "SELECT 1 FROM research_contribution WHERE research_participant_ref = $1 LIMIT 1",
        &[&participant_ref],
    )?;
    if collision.is_some() {
        return Err(ResearchContributionPersistenceError::OperationalIdentityReuse);
    }
    Ok(())
}

fn required_reference(reference: &str) -> Result<&str, ResearchContributionPersistenceError> {
    normalized_reference(reference).ok_or(ResearchContributionPersistenceError::InvalidReference)
}

fn bounded_timestamp(value: u64) -> Result<i64, ResearchContributionPersistenceError> {
    if value == 0 {
        return Err(ResearchContributionPersistenceError::InvalidTimestamp);
    }
    i64::try_from(value).map_err(|_| ResearchContributionPersistenceError::InvalidTimestamp)
}

fn require_read_committed(
    transaction: &mut Transaction<'_>,
) -> Result<(), ResearchContributionPersistenceError> {
    let row = transaction.query_one("SHOW transaction_isolation", &[])?;
    let isolation: String = row.get(0);
    if isolation == "read committed" {
        Ok(())
    } else {
        Err(ResearchContributionPersistenceError::UnsupportedIsolationLevel)
    }
}

#[cfg(test)]
mod validation_tests {
    use super::{bounded_timestamp, required_reference, ResearchContributionPersistenceError};

    #[test]
    fn invalid_references_and_timestamps_fail_closed() {
        assert!(matches!(
            required_reference(" "),
            Err(ResearchContributionPersistenceError::InvalidReference)
        ));
        assert!(matches!(
            required_reference("42"),
            Err(ResearchContributionPersistenceError::InvalidReference)
        ));
        assert_eq!(
            required_reference("research_contribution_alpha").unwrap(),
            "research_contribution_alpha"
        );
        assert!(matches!(
            bounded_timestamp(0),
            Err(ResearchContributionPersistenceError::InvalidTimestamp)
        ));
        assert_eq!(bounded_timestamp(1).unwrap(), 1);
        assert!(matches!(
            bounded_timestamp(u64::MAX),
            Err(ResearchContributionPersistenceError::InvalidTimestamp)
        ));
    }
}
