//! `PostgreSQL` 18 persistence for purpose-specific consent evidence.
//!
//! This adapter stores and reloads product-owned consent events only. Identity
//! credentials remain in Keyverse. The caller owns the connection, credentials,
//! and transaction boundary. Ledger persistence and restart reload require
//! `READ COMMITTED`. Writers take the participant ledger header `FOR UPDATE` and
//! readers take it `FOR SHARE`, so one durable per-ledger sequence is the order
//! authority even when wall-clock timestamps tie or move backwards.

use crate::consent::{
    ConsentDecision, ConsentEvent, ConsentEventInput, ConsentLedger, ConsentPurpose,
};
use crate::reference::normalized_reference;
use postgres::Transaction;
use std::error::Error;
use std::fmt::{Display, Formatter};

const CONSENT_MIGRATION: &str = include_str!("../migrations/0005_consent_lifecycle.sql");
const CONSENT_EVENT_ORDER_MIGRATION: &str =
    include_str!("../migrations/0021_consent_event_order.sql");

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
    /// Stored history and the supplied immutable ledger snapshot disagree.
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

/// Apply the idempotent consent-lifecycle migrations to a `PostgreSQL` connection.
///
/// Existing rows from the original consent migration remain deliberately
/// unsequenced. A single legacy event is order-unambiguous and may be followed by
/// sequenced events. If one participant already has two or more unsequenced legacy
/// events, the ordering migration fails before changing that schema because their
/// relative order cannot be proven without fabricating history. After upgrade, a
/// database constraint permits at most one unsequenced legacy row per participant.
///
/// # Errors
///
/// Returns the `PostgreSQL` error if either migration cannot be applied, including
/// when legacy history lacks deterministic order evidence required for the upgrade.
pub fn apply_consent_migration(
    client: &mut impl postgres::GenericClient,
) -> Result<(), postgres::Error> {
    client.batch_execute(CONSENT_MIGRATION)?;
    client.batch_execute(CONSENT_EVENT_ORDER_MIGRATION)
}

/// Reload one participant-bound consent ledger after process restart.
///
/// New events are reconstructed by the immutable per-ledger `event_sequence`.
/// Database wall-clock `created_at` is intentionally not an ordering authority.
/// At most one legacy unsequenced event is accepted because a single row has no
/// relative-order ambiguity; multiple legacy rows fail closed. Missing ledgers
/// are absent rather than an invented empty grant. Stored events that violate
/// sequence continuity or append-only domain rules also fail closed.
///
/// The caller owns the `READ COMMITTED` transaction. This load takes `FOR SHARE`
/// on the ledger header, which conflicts with the persist-side `FOR UPDATE` lock
/// and keeps the reconstructed event set stable for the transaction boundary.
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
    let participant_ref = required_reference(participant_ref)?;
    let mut ledger = ConsentLedger::new(participant_ref)
        .map_err(|_| ConsentPersistenceError::InvalidReference)?;
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
                consent_form_version_ref, research_scope_ref, occurred_at_unix_ms, \
                event_sequence \
         FROM consent_event \
         WHERE participant_ref = $1 \
         ORDER BY event_sequence ASC NULLS FIRST",
        &[&participant_ref],
    )?;
    let mut loaded_events = Vec::with_capacity(rows.len());
    for row in rows {
        loaded_events.push(LoadedConsentEvent {
            event_ref: row.get(0),
            purpose: parse_purpose(row.get::<_, String>(1).as_str())?,
            decision: parse_decision(row.get::<_, String>(2).as_str())?,
            consent_form_version_ref: row.get(3),
            research_scope_ref: row.get(4),
            occurred_at_unix_ms: stored_timestamp(row.get(5))?,
            event_sequence: row.get(6),
        });
    }
    reconstruct_loaded_events(&mut ledger, loaded_events)?;
    Ok(Some(ledger))
}

struct LoadedConsentEvent {
    event_ref: String,
    purpose: ConsentPurpose,
    decision: ConsentDecision,
    consent_form_version_ref: String,
    research_scope_ref: Option<String>,
    occurred_at_unix_ms: u64,
    event_sequence: Option<i64>,
}

fn reconstruct_loaded_events(
    ledger: &mut ConsentLedger,
    rows: Vec<LoadedConsentEvent>,
) -> Result<(), ConsentPersistenceError> {
    let mut saw_legacy_event = false;
    let mut saw_sequenced_event = false;
    let mut expected_sequence = 1_i64;

    for row in rows {
        match row.event_sequence {
            None if !saw_legacy_event && !saw_sequenced_event => saw_legacy_event = true,
            None => return Err(ConsentPersistenceError::CorruptHistory),
            Some(sequence) if sequence == expected_sequence => {
                saw_sequenced_event = true;
                expected_sequence = expected_sequence
                    .checked_add(1)
                    .ok_or(ConsentPersistenceError::CorruptHistory)?;
            }
            Some(_) => return Err(ConsentPersistenceError::CorruptHistory),
        }

        ledger
            .record(ConsentEventInput {
                event_ref: &row.event_ref,
                purpose: row.purpose,
                decision: row.decision,
                consent_form_version_ref: &row.consent_form_version_ref,
                research_scope_ref: row.research_scope_ref.as_deref(),
                occurred_at_unix_ms: row.occurred_at_unix_ms,
            })
            .map_err(|_| ConsentPersistenceError::CorruptHistory)?;
    }
    Ok(())
}

/// Persist one participant-bound consent ledger and its accepted events.
///
/// Persistence serializes writers on the participant ledger header, verifies
/// that existing durable events are an exact prefix of the supplied immutable
/// ledger snapshot, then appends only the new tail with contiguous positive
/// `event_sequence` values. Exact replay of the whole stored ledger is
/// idempotent. A stale prefix or reordered/conflicting history fails closed.
///
/// # Errors
///
/// Returns [`ConsentPersistenceError`] for unsupported isolation, conflicting
/// replay, corrupt stored ordering, an invalid reference, a timestamp outside
/// the `PostgreSQL` range, or a database failure.
pub fn persist_consent_ledger(
    transaction: &mut Transaction<'_>,
    ledger: &ConsentLedger,
) -> Result<ConsentPersistenceDisposition, ConsentPersistenceError> {
    require_read_committed(transaction)?;
    let participant_ref = required_reference(ledger.participant_ref())?;
    let inserted_header = persist_ledger_header(transaction, participant_ref)?;
    lock_ledger_header(transaction, participant_ref)?;

    let stored = load_consent_ledger(transaction, participant_ref)?
        .ok_or(ConsentPersistenceError::CorruptHistory)?;
    let stored_len = validate_history_prefix(&stored, ledger)?;
    let mut next_sequence = next_event_sequence(transaction, participant_ref)?;
    let mut inserted_any = inserted_header;

    for event in ledger.events().iter().skip(stored_len) {
        persist_one_event(transaction, participant_ref, event, next_sequence)?;
        next_sequence = next_sequence
            .checked_add(1)
            .ok_or(ConsentPersistenceError::CorruptHistory)?;
        inserted_any = true;
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

fn lock_ledger_header(
    transaction: &mut Transaction<'_>,
    participant_ref: &str,
) -> Result<(), ConsentPersistenceError> {
    transaction.query_one(
        "SELECT participant_ref FROM consent_ledger WHERE participant_ref = $1 FOR UPDATE",
        &[&participant_ref],
    )?;
    Ok(())
}

fn validate_history_prefix(
    stored: &ConsentLedger,
    supplied: &ConsentLedger,
) -> Result<usize, ConsentPersistenceError> {
    if stored.events().len() > supplied.events().len() {
        return Err(ConsentPersistenceError::ConflictingReplay);
    }
    for (stored_event, supplied_event) in stored.events().iter().zip(supplied.events()) {
        if !same_event(stored_event, supplied_event) {
            return Err(ConsentPersistenceError::ConflictingReplay);
        }
    }
    Ok(stored.events().len())
}

fn same_event(left: &ConsentEvent, right: &ConsentEvent) -> bool {
    left.event_ref() == right.event_ref()
        && left.purpose() == right.purpose()
        && left.decision() == right.decision()
        && left.consent_form_version_ref() == right.consent_form_version_ref()
        && left.research_scope_ref() == right.research_scope_ref()
        && left.occurred_at_unix_ms() == right.occurred_at_unix_ms()
}

fn next_event_sequence(
    transaction: &mut Transaction<'_>,
    participant_ref: &str,
) -> Result<i64, ConsentPersistenceError> {
    let row = transaction.query_one(
        "SELECT \
             COUNT(*) FILTER (WHERE event_sequence IS NULL), \
             COUNT(event_sequence), \
             COALESCE(MIN(event_sequence), 0), \
             COALESCE(MAX(event_sequence), 0) \
         FROM consent_event \
         WHERE participant_ref = $1",
        &[&participant_ref],
    )?;
    let legacy_count: i64 = row.get(0);
    let sequenced_count: i64 = row.get(1);
    let minimum_sequence: i64 = row.get(2);
    let maximum_sequence: i64 = row.get(3);

    if legacy_count > 1
        || (sequenced_count > 0 && (minimum_sequence != 1 || maximum_sequence != sequenced_count))
    {
        return Err(ConsentPersistenceError::CorruptHistory);
    }

    maximum_sequence
        .checked_add(1)
        .ok_or(ConsentPersistenceError::CorruptHistory)
}

fn persist_one_event(
    transaction: &mut Transaction<'_>,
    participant_ref: &str,
    event: &ConsentEvent,
    event_sequence: i64,
) -> Result<(), ConsentPersistenceError> {
    let event_ref = required_reference(event.event_ref())?;
    let occurred_at = i64::try_from(event.occurred_at_unix_ms())
        .map_err(|_| ConsentPersistenceError::InvalidTimestamp)?;
    let purpose = purpose_name(event.purpose());
    let decision = decision_name(event.decision());
    let research_scope_ref = event.research_scope_ref();
    let inserted = transaction.execute(
        "INSERT INTO consent_event (\
             participant_ref, event_ref, consent_purpose, consent_decision, \
             consent_form_version_ref, research_scope_ref, occurred_at_unix_ms, event_sequence\
         ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8) \
         ON CONFLICT (participant_ref, event_ref) DO NOTHING",
        &[
            &participant_ref,
            &event_ref,
            &purpose,
            &decision,
            &event.consent_form_version_ref(),
            &research_scope_ref,
            &occurred_at,
            &event_sequence,
        ],
    )?;
    if inserted == 1 {
        return Ok(());
    }

    let row = transaction.query_one(
        "SELECT consent_purpose, consent_decision, consent_form_version_ref, \
                research_scope_ref, occurred_at_unix_ms, event_sequence \
         FROM consent_event WHERE participant_ref = $1 AND event_ref = $2",
        &[&participant_ref, &event_ref],
    )?;
    let stored_purpose: String = row.get(0);
    let stored_decision: String = row.get(1);
    let stored_form: String = row.get(2);
    let stored_scope: Option<String> = row.get(3);
    let stored_occurred: i64 = row.get(4);
    let stored_sequence: Option<i64> = row.get(5);
    if stored_purpose == purpose
        && stored_decision == decision
        && stored_form == event.consent_form_version_ref()
        && stored_scope.as_deref() == event.research_scope_ref()
        && stored_occurred == occurred_at
        && stored_sequence == Some(event_sequence)
    {
        Ok(())
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
    match normalized_reference(reference) {
        Some(normalized) if normalized == reference => Ok(normalized),
        _ => Err(ConsentPersistenceError::InvalidReference),
    }
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
        decision_name, next_event_sequence, parse_decision, parse_purpose, purpose_name,
        reconstruct_loaded_events, required_reference, same_event, stored_timestamp,
        validate_history_prefix, ConsentPersistenceError, LoadedConsentEvent,
    };
    use crate::consent::{ConsentDecision, ConsentEventInput, ConsentLedger, ConsentPurpose};

    #[test]
    fn blank_numeric_and_noncanonical_references_fail_closed() {
        for invalid in [" ", "12", " participant_consent_alpha"] {
            assert!(matches!(
                required_reference(invalid),
                Err(ConsentPersistenceError::InvalidReference)
            ));
        }
        assert_eq!(
            required_reference("participant_consent_alpha").unwrap(),
            "participant_consent_alpha"
        );
    }

    #[test]
    fn stored_labels_round_trip_or_fail_closed() {
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

    fn loaded_event(
        event_ref: &str,
        purpose: ConsentPurpose,
        decision: ConsentDecision,
        research_scope_ref: Option<&str>,
        occurred_at_unix_ms: u64,
        event_sequence: Option<i64>,
    ) -> LoadedConsentEvent {
        LoadedConsentEvent {
            event_ref: event_ref.to_owned(),
            purpose,
            decision,
            consent_form_version_ref: "consent_form_reconstruct".to_owned(),
            research_scope_ref: research_scope_ref.map(str::to_owned),
            occurred_at_unix_ms,
            event_sequence,
        }
    }

    #[test]
    fn sequence_order_keeps_same_millisecond_revoke_latest() {
        let mut ledger = ConsentLedger::new("participant_consent_reconstruct").unwrap();
        reconstruct_loaded_events(
            &mut ledger,
            vec![
                loaded_event(
                    "consent_event_zzz_reload_grant",
                    ConsentPurpose::ResearchContribution,
                    ConsentDecision::Granted,
                    Some("research_scope_reconstruct"),
                    32_000,
                    Some(1),
                ),
                loaded_event(
                    "consent_event_aaa_reload_revoke",
                    ConsentPurpose::ResearchContribution,
                    ConsentDecision::Revoked,
                    Some("research_scope_reconstruct"),
                    32_000,
                    Some(2),
                ),
            ],
        )
        .unwrap();
        let snapshot = ledger.snapshot_as("consent_snapshot_reconstruct").unwrap();
        assert!(!snapshot.is_granted(ConsentPurpose::ResearchContribution));
    }

    #[test]
    fn one_legacy_event_can_precede_a_sequenced_tail() {
        let mut ledger = ConsentLedger::new("participant_consent_legacy_tail").unwrap();
        reconstruct_loaded_events(
            &mut ledger,
            vec![
                loaded_event(
                    "consent_event_legacy_grant",
                    ConsentPurpose::ResearchContribution,
                    ConsentDecision::Granted,
                    Some("research_scope_reconstruct"),
                    33_000,
                    None,
                ),
                loaded_event(
                    "consent_event_sequenced_revoke",
                    ConsentPurpose::ResearchContribution,
                    ConsentDecision::Revoked,
                    Some("research_scope_reconstruct"),
                    33_000,
                    Some(1),
                ),
            ],
        )
        .unwrap();
        let snapshot = ledger.snapshot_as("consent_snapshot_legacy_tail").unwrap();
        assert!(!snapshot.is_granted(ConsentPurpose::ResearchContribution));
    }

    #[test]
    fn corrupt_sequence_or_domain_reconstruction_fails_closed() {
        for rows in [
            vec![
                loaded_event(
                    "consent_event_legacy_one",
                    ConsentPurpose::ServiceOperation,
                    ConsentDecision::Granted,
                    None,
                    20_000,
                    None,
                ),
                loaded_event(
                    "consent_event_legacy_two",
                    ConsentPurpose::ServiceOperation,
                    ConsentDecision::Revoked,
                    None,
                    20_000,
                    None,
                ),
            ],
            vec![loaded_event(
                "consent_event_gap",
                ConsentPurpose::ServiceOperation,
                ConsentDecision::Granted,
                None,
                20_000,
                Some(2),
            )],
        ] {
            let mut ledger = ConsentLedger::new("participant_consent_corrupt_order").unwrap();
            assert!(matches!(
                reconstruct_loaded_events(&mut ledger, rows),
                Err(ConsentPersistenceError::CorruptHistory)
            ));
        }

        let mut non_monotonic = ConsentLedger::new("participant_consent_reconstruct").unwrap();
        assert!(matches!(
            reconstruct_loaded_events(
                &mut non_monotonic,
                vec![
                    loaded_event(
                        "consent_event_later",
                        ConsentPurpose::ServiceOperation,
                        ConsentDecision::Granted,
                        None,
                        20_000,
                        Some(1),
                    ),
                    loaded_event(
                        "consent_event_earlier",
                        ConsentPurpose::ServiceOperation,
                        ConsentDecision::Revoked,
                        None,
                        19_000,
                        Some(2),
                    ),
                ],
            ),
            Err(ConsentPersistenceError::CorruptHistory)
        ));

        let mut invalid_event = ConsentLedger::new("participant_consent_invalid_event").unwrap();
        assert!(matches!(
            reconstruct_loaded_events(
                &mut invalid_event,
                vec![loaded_event(
                    " ",
                    ConsentPurpose::ServiceOperation,
                    ConsentDecision::Granted,
                    None,
                    21_000,
                    Some(1),
                )],
            ),
            Err(ConsentPersistenceError::CorruptHistory)
        ));
    }

    fn service_ledger(
        participant_ref: &str,
        event_ref: &str,
        decision: ConsentDecision,
    ) -> ConsentLedger {
        let mut ledger = ConsentLedger::new(participant_ref).unwrap();
        ledger
            .record(ConsentEventInput {
                event_ref,
                purpose: ConsentPurpose::ServiceOperation,
                decision,
                consent_form_version_ref: "consent_form_service_prefix",
                research_scope_ref: None,
                occurred_at_unix_ms: 44_000,
            })
            .unwrap();
        ledger
    }

    #[test]
    fn stored_history_must_be_an_exact_prefix() {
        let stored = service_ledger(
            "participant_consent_prefix",
            "consent_event_prefix_grant",
            ConsentDecision::Granted,
        );
        let mut extension = stored.clone();
        extension
            .record(ConsentEventInput {
                event_ref: "consent_event_prefix_revoke",
                purpose: ConsentPurpose::ServiceOperation,
                decision: ConsentDecision::Revoked,
                consent_form_version_ref: "consent_form_service_prefix",
                research_scope_ref: None,
                occurred_at_unix_ms: 45_000,
            })
            .unwrap();
        assert_eq!(validate_history_prefix(&stored, &extension).unwrap(), 1);
        assert!(same_event(&stored.events()[0], &extension.events()[0]));

        let shorter = ConsentLedger::new("participant_consent_prefix").unwrap();
        assert!(matches!(
            validate_history_prefix(&stored, &shorter),
            Err(ConsentPersistenceError::ConflictingReplay)
        ));

        let conflict = service_ledger(
            "participant_consent_prefix",
            "consent_event_different",
            ConsentDecision::Granted,
        );
        assert!(matches!(
            validate_history_prefix(&stored, &conflict),
            Err(ConsentPersistenceError::ConflictingReplay)
        ));
    }

    #[test]
    fn private_database_sequence_helper_is_not_callable_without_postgres() {
        let _ = next_event_sequence;
    }
}
