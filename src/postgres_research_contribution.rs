//! `PostgreSQL` 18 persistence for explicit research-contribution evidence.
//!
//! The operational participant reference is retained only in the product-owned
//! restricted research boundary. Public research artifacts must use the separate
//! pseudonymous research participant reference and must never expose this table.
//! The authorizing [`ConsentSnapshot`] is supplied alongside the contribution so
//! the adapter can fail closed if participant, snapshot, or research-scope
//! evidence is rebound before persistence. The caller owns credentials and the
//! surrounding transaction boundary.

use crate::consent::{ConsentSnapshot, ResearchContribution, ResearchContributionState};
use crate::reference::normalized_reference;
use postgres::Transaction;
use std::error::Error;
use std::fmt::{Display, Formatter};

const RESEARCH_CONTRIBUTION_MIGRATION: &str =
    include_str!("../migrations/0017_research_contribution.sql");

/// Outcome of persisting research-contribution lifecycle evidence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ResearchContributionPersistenceDisposition {
    /// At least one new immutable contribution or withdrawal record was inserted.
    Inserted,
    /// The exact contribution and any withdrawal evidence already existed.
    Duplicate,
}

/// Fail-closed error for durable research-contribution persistence.
#[derive(Debug)]
#[non_exhaustive]
pub enum ResearchContributionPersistenceError {
    /// A contribution, participant, snapshot, scope, or withdrawal identity is invalid.
    InvalidReference,
    /// The supplied consent snapshot does not authorize this exact contribution evidence.
    ConsentSnapshotMismatch,
    /// Operational and research participant namespaces were incorrectly reused.
    OperationalIdentityReuse,
    /// Contribution or withdrawal time cannot be represented safely in `PostgreSQL`.
    InvalidTimestamp,
    /// Contribution lifecycle state and withdrawal evidence are internally inconsistent.
    InvalidLifecycleEvidence,
    /// An immutable contribution or withdrawal identity was rebound to different evidence.
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
                "research contribution persistence references must be opaque durable values"
            }
            Self::ConsentSnapshotMismatch => {
                "research contribution is not bound to the supplied active research-consent snapshot"
            }
            Self::OperationalIdentityReuse => {
                "research participant reference must differ from the operational participant"
            }
            Self::InvalidTimestamp => {
                "research contribution timestamp is outside the PostgreSQL bigint range"
            }
            Self::InvalidLifecycleEvidence => {
                "research contribution lifecycle state is inconsistent with withdrawal evidence"
            }
            Self::ConflictingReplay => {
                "research contribution identity was replayed with conflicting evidence"
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

/// Persist one consent-bound research contribution and optional withdrawal.
///
/// The supplied consent snapshot must be the exact immutable snapshot referenced
/// by `contribution`, must belong to a distinct operational participant identity,
/// and must still carry the same active research scope. The start record is
/// append-only. Withdrawal is stored as a separate append-only event so the
/// original opt-in evidence is never rewritten. Exact replay is idempotent;
/// rebinding any identity, scope, timestamp, or withdrawal evidence fails closed.
///
/// Replaying the original active contribution after a withdrawal remains an
/// idempotent replay of the start record and never removes the stored withdrawal.
///
/// # Errors
///
/// Returns [`ResearchContributionPersistenceError`] for invalid or rebound
/// references, unsupported isolation, inconsistent lifecycle evidence, timestamp
/// overflow, conflicting replay, or a database failure.
pub fn persist_research_contribution(
    transaction: &mut Transaction<'_>,
    consent_snapshot: &ConsentSnapshot,
    contribution: &ResearchContribution,
) -> Result<ResearchContributionPersistenceDisposition, ResearchContributionPersistenceError> {
    require_read_committed(transaction)?;
    let evidence = validated_evidence(consent_snapshot, contribution)?;
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

#[derive(Clone, Copy)]
struct ValidatedEvidence<'a> {
    contribution_ref: &'a str,
    participant_ref: &'a str,
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

fn validated_evidence<'a>(
    consent_snapshot: &'a ConsentSnapshot,
    contribution: &'a ResearchContribution,
) -> Result<ValidatedEvidence<'a>, ResearchContributionPersistenceError> {
    let contribution_ref = required_reference(contribution.contribution_ref())?;
    let participant_ref = required_reference(consent_snapshot.participant_ref())?;
    let research_participant_ref = required_reference(contribution.research_participant_ref())?;
    let consent_snapshot_ref = required_reference(contribution.consent_snapshot_ref())?;
    let research_scope_ref = required_reference(contribution.research_scope_ref())?;
    let supplied_snapshot_ref = required_reference(consent_snapshot.snapshot_ref())?;
    let active_scope = consent_snapshot
        .active_research_scope()
        .and_then(normalized_reference)
        .ok_or(ResearchContributionPersistenceError::ConsentSnapshotMismatch)?;

    if consent_snapshot_ref != supplied_snapshot_ref || research_scope_ref != active_scope {
        return Err(ResearchContributionPersistenceError::ConsentSnapshotMismatch);
    }
    if participant_ref == research_participant_ref {
        return Err(ResearchContributionPersistenceError::OperationalIdentityReuse);
    }

    let started_at_unix_ms = bounded_timestamp(contribution.started_at_unix_ms())?;
    let withdrawal = validated_withdrawal(contribution, started_at_unix_ms)?;
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

fn validated_withdrawal<'a>(
    contribution: &'a ResearchContribution,
    started_at_unix_ms: i64,
) -> Result<Option<ValidatedWithdrawal<'a>>, ResearchContributionPersistenceError> {
    match (
        contribution.state(),
        contribution.withdrawal_event_ref(),
        contribution.withdrawn_at_unix_ms(),
    ) {
        (ResearchContributionState::Active, None, None) => Ok(None),
        (ResearchContributionState::Withdrawn, Some(event_ref), Some(withdrawn_at)) => {
            let withdrawal_event_ref = required_reference(event_ref)?;
            let withdrawn_at_unix_ms = bounded_timestamp(withdrawn_at)?;
            if withdrawn_at_unix_ms <= started_at_unix_ms {
                return Err(ResearchContributionPersistenceError::InvalidLifecycleEvidence);
            }
            Ok(Some(ValidatedWithdrawal {
                withdrawal_event_ref,
                withdrawn_at_unix_ms,
            }))
        }
        _ => Err(ResearchContributionPersistenceError::InvalidLifecycleEvidence),
    }
}

fn persist_contribution_start(
    transaction: &mut Transaction<'_>,
    evidence: &ValidatedEvidence<'_>,
) -> Result<bool, ResearchContributionPersistenceError> {
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
    let inserted = transaction.execute(
        "INSERT INTO research_withdrawal_event (\
             contribution_ref, withdrawal_event_ref, withdrawn_at_unix_ms\
         ) VALUES ($1, $2, $3) \
         ON CONFLICT DO NOTHING",
        &[
            &contribution_ref,
            &withdrawal.withdrawal_event_ref,
            &withdrawal.withdrawn_at_unix_ms,
        ],
    )?;
    if inserted == 1 {
        return Ok(true);
    }

    let row = transaction.query_opt(
        "SELECT withdrawal_event_ref, withdrawn_at_unix_ms \
         FROM research_withdrawal_event WHERE contribution_ref = $1",
        &[&contribution_ref],
    )?;
    let Some(row) = row else {
        return Err(ResearchContributionPersistenceError::ConflictingReplay);
    };
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

fn required_reference(
    reference: &str,
) -> Result<&str, ResearchContributionPersistenceError> {
    normalized_reference(reference).ok_or(ResearchContributionPersistenceError::InvalidReference)
}

fn bounded_timestamp(value: u64) -> Result<i64, ResearchContributionPersistenceError> {
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
    use super::{
        bounded_timestamp, required_reference, ResearchContributionPersistenceError,
    };

    #[test]
    fn invalid_references_and_timestamp_overflow_fail_closed() {
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
        assert_eq!(bounded_timestamp(1).unwrap(), 1);
        assert!(matches!(
            bounded_timestamp(u64::MAX),
            Err(ResearchContributionPersistenceError::InvalidTimestamp)
        ));
    }
}
