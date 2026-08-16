//! `PostgreSQL` 18 persistence for purpose-specific consent evidence.
//!
//! This adapter stores and reloads product-owned consent events only. Identity
//! credentials remain in Keyverse. The caller owns the connection, credentials,
//! and transaction boundary. Ledger persist, exact-replay classification, and
//! restart reload require `READ COMMITTED` so a concurrent insert that wins a
//! unique-key race is visible to the classifier and so a later-inserted
//! same-millisecond revocation is not hidden behind a grant whose identity
//! sorts later.

use crate::consent::{
    ConsentDecision, ConsentEvent, ConsentEventInput, ConsentLedger, ConsentPurpose,
};
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
    /// A consent event timestamp cannot be represented by the bounded database column.
    InvalidTimestamp,
    /// Consent persistence requires `PostgreSQL` `READ COMMITTED` isolation.
    UnsupportedIsolationLevel,
    /// Stored events cannot reconstruct a valid append-only consent ledger.
    CorruptHistory,
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
            Self::InvalidTimestamp => "consent event timestamp exceeds the PostgreSQL bigint range",
            Self::UnsupportedIsolationLevel => {
                "consent persistence requires read committed isolation"
            }
            Self::CorruptHistory => "stored consent events cannot reconstruct a valid ledger",
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

/// Reload one participant-bound consent ledger after process restart.
///
/// Events are reconstructed in occurrence time, then physical insertion time,
/// then event identity. Equal server timestamps therefore keep a later-inserted
/// revocation after the grant whose opaque identity sorts later. A missing
/// ledger is absent rather than an empty grant. Stored events that violate
/// append-only domain rules fail closed instead of being reordered into a
/// newer grant.
///
/// The caller owns the `READ COMMITTED` transaction. The load takes `FOR SHARE`
/// on the ledger header so a concurrent writer that locks the same row cannot
/// hide an in-flight append from the reconstructed snapshot.
///
/// # Errors
///
/// Returns [`ConsentPersistenceError`] for an invalid participant reference,
/// unsupported isolation, stored events that cannot reconstruct a valid ledger,
/// a timestamp outside the `PostgreSQL` range, or a database failure.
pub fn load_consent_ledger(
    transaction: &mut Transaction<'_>,
    participant_ref: &str,
) -> Result<Option<ConsentLedger>, ConsentPersistenceError> {
    require_read_committed(transaction)?;
    let mut ledger = ConsentLedger::new(participant_ref)
        .map_err(|_| ConsentPersistenceError::InvalidReference)?;
    let participant_ref = ledger.participant_ref().to_owned();
    if transaction
        .query_opt(
            "SELECT participant_ref FROM consent_ledger WHERE participant_ref = $1 FOR SHARE",
            &[&participant_ref],
        )?
        .is_none()
    {
        return Ok(None);
    }
    let rows = transaction.query(
        "SELECT event_ref, consent_purpose, consent_decision, \
                consent_form_version_ref, research_scope_ref, occurred_at_unix_ms \
         FROM consent_event \
         WHERE participant_ref = $1 \
         ORDER BY occurred_at_unix_ms ASC, created_at ASC, event_ref ASC",
        &[&participant_ref],
    )?;
    for row in rows {
        let event_ref: String = row.get(0);
        let purpose = parse_purpose(row.get::<_, String>(1).as_str())?;
        let decision = parse_decision(row.get::<_, String>(2).as_str())?;
        let consent_form_version_ref: String = row.get(3);
        let research_scope_ref: Option<String> = row.get(4);
        let occurred_at_unix_ms = stored_timestamp(row.get(5))?;
        ledger
            .record(ConsentEventInput {
                event_ref: &event_ref,
                purpose,
                decision,
                consent_form_version_ref: &consent_form_version_ref,
                research_scope_ref: research_scope_ref.as_deref(),
                occurred_at_unix_ms,
            })
            .map_err(|_| ConsentPersistenceError::CorruptHistory)?;
    }
    Ok(Some(ledger))
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
/// replay, an invalid reference, a timestamp outside the `PostgreSQL` range, or
/// a database failure.
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
    let occurred_at = i64::try_from(event.occurred_at_unix_ms())
        .map_err(|_| ConsentPersistenceError::InvalidTimestamp)?;
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

fn parse_purpose(purpose: &str) -> Result<ConsentPurpose, ConsentPersistenceError> {
    match purpose {
        "service_operation" => Ok(ConsentPurpose::ServiceOperation),
        "account_persistence" => Ok(ConsentPurpose::AccountPersistence),
        "longitudinal_observation" => Ok(ConsentPurpose::LongitudinalObservation),
        "research_contribution" => Ok(ConsentPurpose::ResearchContribution),
        "communications" => Ok(ConsentPurpose::Communications),
        _ => Err(ConsentPersistenceError::CorruptHistory),
    }
}

fn decision_name(decision: ConsentDecision) -> &'static str {
    match decision {
        ConsentDecision::Granted => "granted",
        ConsentDecision::Revoked => "revoked",
    }
}

fn parse_decision(decision: &str) -> Result<ConsentDecision, ConsentPersistenceError> {
    match decision {
        "granted" => Ok(ConsentDecision::Granted),
        "revoked" => Ok(ConsentDecision::Revoked),
        _ => Err(ConsentPersistenceError::CorruptHistory),
    }
}

fn stored_timestamp(value: i64) -> Result<u64, ConsentPersistenceError> {
    u64::try_from(value).map_err(|_| ConsentPersistenceError::InvalidTimestamp)
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
    use super::{
        decision_name, parse_decision, parse_purpose, purpose_name, required_reference,
        stored_timestamp, ConsentPersistenceError,
    };
    use crate::consent::{ConsentDecision, ConsentPurpose};

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

    #[test]
    fn stored_purpose_and_decision_labels_round_trip_or_fail_closed() {
        for purpose in [
            ConsentPurpose::ServiceOperation,
            ConsentPurpose::AccountPersistence,
            ConsentPurpose::LongitudinalObservation,
            ConsentPurpose::ResearchContribution,
            ConsentPurpose::Communications,
        ] {
            assert_eq!(parse_purpose(purpose_name(purpose)).unwrap(), purpose);
        }
        assert!(matches!(
            parse_purpose("unknown_purpose"),
            Err(ConsentPersistenceError::CorruptHistory)
        ));
        assert_eq!(
            parse_decision(decision_name(ConsentDecision::Granted)).unwrap(),
            ConsentDecision::Granted
        );
        assert_eq!(
            parse_decision(decision_name(ConsentDecision::Revoked)).unwrap(),
            ConsentDecision::Revoked
        );
        assert!(matches!(
            parse_decision("unknown_decision"),
            Err(ConsentPersistenceError::CorruptHistory)
        ));
        assert_eq!(stored_timestamp(32_000).unwrap(), 32_000);
        assert!(matches!(
            stored_timestamp(-1),
            Err(ConsentPersistenceError::InvalidTimestamp)
        ));
    }
}
