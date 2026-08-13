//! `PostgreSQL` 18 persistence for purpose-specific consent evidence.
//!
//! This adapter stores product-owned consent events only. Identity credentials
//! remain in Keyverse. The caller owns the connection, credentials, and
//! transaction boundary. Ledger and event replay require `READ COMMITTED` so a
//! concurrent insert that wins a unique-key race is visible to the exact-replay
//! classifier.

use crate::consent::{ConsentDecision, ConsentEvent, ConsentLedger, ConsentPurpose};
use crate::reference::normalized_reference;
use postgres::Transaction;
use std::error::Error;
use std::fmt::{Display, Formatter};

const CONSENT_MIGRATION: &str = include_str!("../migrations/0005_consent_lifecycle.sql");

/// Outcome of persisting one consent ledger snapshot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ConsentPersistenceDisposition {
    /// At least one new ledger or event row was inserted.
    Inserted,
    /// The same immutable ledger and event evidence already existed.
    Duplicate,
}

/// Fail-closed error for durable consent persistence.
#[derive(Debug)]
#[non_exhaustive]
pub enum ConsentPersistenceError {
    /// A participant, event, form, or research-scope identity was blank or numeric-like.
    InvalidReference,
    /// Event identity was replayed with different immutable evidence.
    ConflictingReplay,
    /// Consent persistence requires `PostgreSQL` `READ COMMITTED` isolation.
    UnsupportedIsolationLevel,
    /// `PostgreSQL` rejected or could not execute the persistence operation.
    Database(postgres::Error),
}

impl Display for ConsentPersistenceError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidReference => "consent persistence references must be opaque values",
            Self::ConflictingReplay => {
                "consent event identity was replayed with conflicting evidence"
            }
            Self::UnsupportedIsolationLevel => {
                "consent persistence requires read committed isolation"
            }
            Self::Database(_) => "PostgreSQL consent persistence failed",
        })
    }
}

impl Error for ConsentPersistenceError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Database(error) => Some(error),
            _ => None,
        }
    }
}

impl From<postgres::Error> for ConsentPersistenceError {
    fn from(error: postgres::Error) -> Self {
        Self::Database(error)
    }
}

/// Apply the idempotent consent-lifecycle migration to a `PostgreSQL` connection.
///
/// # Errors
///
/// Returns the `PostgreSQL` error if the migration cannot be applied.
pub fn apply_consent_migration(
    client: &mut impl postgres::GenericClient,
) -> Result<(), postgres::Error> {
    client.batch_execute(CONSENT_MIGRATION)
}

/// Persist one participant-bound consent ledger and its accepted events.
///
/// Exact replay of the same participant and event evidence is idempotent.
/// Reusing an event identity with different purpose, decision, form, scope, or
/// time fails closed. New events append without rewriting earlier evidence.
///
/// # Errors
///
/// Returns [`ConsentPersistenceError`] for unsupported isolation, conflicting
/// replay, an invalid reference, or a database failure.
pub fn persist_consent_ledger(
    transaction: &mut Transaction<'_>,
    ledger: &ConsentLedger,
) -> Result<ConsentPersistenceDisposition, ConsentPersistenceError> {
    require_read_committed(transaction)?;
    let participant_ref = required_reference(ledger.participant_ref())?;
    let mut inserted_any = persist_ledger_header(transaction, participant_ref)?;
    for event in ledger.events() {
        if persist_one_event(transaction, participant_ref, event)? {
            inserted_any = true;
        }
    }
    if inserted_any {
        Ok(ConsentPersistenceDisposition::Inserted)
    } else {
        Ok(ConsentPersistenceDisposition::Duplicate)
    }
}

fn persist_ledger_header(
    transaction: &mut Transaction<'_>,
    participant_ref: &str,
) -> Result<bool, ConsentPersistenceError> {
    let inserted = transaction.execute(
        "INSERT INTO consent_ledger (participant_ref) VALUES ($1) \
         ON CONFLICT (participant_ref) DO NOTHING",
        &[&participant_ref],
    )?;
    Ok(inserted == 1)
}

fn persist_one_event(
    transaction: &mut Transaction<'_>,
    participant_ref: &str,
    event: &ConsentEvent,
) -> Result<bool, ConsentPersistenceError> {
    let event_ref = required_reference(event.event_ref())?;
    let occurred_at = i64::try_from(event.occurred_at_unix_ms()).unwrap_or(i64::MAX);
    let purpose = purpose_name(event.purpose());
    let decision = decision_name(event.decision());
    let research_scope_ref = event.research_scope_ref();
    let inserted = transaction.execute(
        "INSERT INTO consent_event (\
             participant_ref, event_ref, consent_purpose, consent_decision, \
             consent_form_version_ref, research_scope_ref, occurred_at_unix_ms\
         ) VALUES ($1, $2, $3, $4, $5, $6, $7) \
         ON CONFLICT (participant_ref, event_ref) DO NOTHING",
        &[
            &participant_ref,
            &event_ref,
            &purpose,
            &decision,
            &event.consent_form_version_ref(),
            &research_scope_ref,
            &occurred_at,
        ],
    )?;
    if inserted == 1 {
        return Ok(true);
    }

    let row = transaction
        .query_one(
            "SELECT consent_purpose, consent_decision, consent_form_version_ref, \
                    research_scope_ref, occurred_at_unix_ms \
             FROM consent_event WHERE participant_ref = $1 AND event_ref = $2",
            &[&participant_ref, &event_ref],
        )
        .map_err(ConsentPersistenceError::from)?;
    let stored_purpose: String = row.get(0);
    let stored_decision: String = row.get(1);
    let stored_form: String = row.get(2);
    let stored_scope: Option<String> = row.get(3);
    let stored_occurred: i64 = row.get(4);
    if stored_purpose == purpose
        && stored_decision == decision
        && stored_form == event.consent_form_version_ref()
        && stored_scope.as_deref() == event.research_scope_ref()
        && stored_occurred == occurred_at
    {
        Ok(false)
    } else {
        Err(ConsentPersistenceError::ConflictingReplay)
    }
}

fn purpose_name(purpose: ConsentPurpose) -> &'static str {
    match purpose {
        ConsentPurpose::ServiceOperation => "service_operation",
        ConsentPurpose::AccountPersistence => "account_persistence",
        ConsentPurpose::LongitudinalObservation => "longitudinal_observation",
        ConsentPurpose::ResearchContribution => "research_contribution",
        ConsentPurpose::Communications => "communications",
    }
}

fn decision_name(decision: ConsentDecision) -> &'static str {
    match decision {
        ConsentDecision::Granted => "granted",
        ConsentDecision::Revoked => "revoked",
    }
}

fn required_reference(reference: &str) -> Result<&str, ConsentPersistenceError> {
    normalized_reference(reference).ok_or(ConsentPersistenceError::InvalidReference)
}

fn require_read_committed(
    transaction: &mut Transaction<'_>,
) -> Result<(), ConsentPersistenceError> {
    let row = transaction.query_one("SHOW transaction_isolation", &[])?;
    let isolation: String = row.get(0);
    if isolation == "read committed" {
        Ok(())
    } else {
        Err(ConsentPersistenceError::UnsupportedIsolationLevel)
    }
}

#[cfg(test)]
mod reference_guard_tests {
    use super::{required_reference, ConsentPersistenceError};

    #[test]
    fn blank_and_numeric_references_fail_closed() {
        assert!(matches!(
            required_reference(" "),
            Err(ConsentPersistenceError::InvalidReference)
        ));
        assert!(matches!(
            required_reference("12"),
            Err(ConsentPersistenceError::InvalidReference)
        ));
        assert_eq!(
            required_reference("participant_consent_alpha").unwrap(),
            "participant_consent_alpha"
        );
    }
}
