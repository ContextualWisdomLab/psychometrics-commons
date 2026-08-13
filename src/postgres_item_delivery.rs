//! `PostgreSQL` 18 persistence for session-bound item-delivery evidence.
//!
//! This adapter stores product-runtime delivery evidence only. Item selection,
//! calibration, and scoring remain in `fast-mlsirm`. The caller owns the connection,
//! credentials, and transaction boundary. Ledger and event replay require
//! `READ COMMITTED` so a concurrent insert that wins a unique-key race is visible to
//! the exact-replay classifier.

use crate::item_delivery::{ItemDeliveryEvent, ItemDeliveryLedger};
use crate::reference::normalized_reference;
use postgres::{GenericClient, Transaction};
use std::error::Error;
use std::fmt::{Display, Formatter};

const ITEM_DELIVERY_MIGRATION: &str = include_str!("../migrations/0004_item_delivery_evidence.sql");

/// Outcome of persisting one item-delivery ledger snapshot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ItemDeliveryPersistenceDisposition {
    /// At least one new ledger or event row was inserted.
    Inserted,
    /// The same immutable ledger and event evidence already existed.
    Duplicate,
}

/// Fail-closed error for durable item-delivery persistence.
#[derive(Debug)]
#[non_exhaustive]
pub enum ItemDeliveryPersistenceError {
    /// A session, release, delivery, item, or presentation identity was blank or numeric-like.
    InvalidReference,
    /// Ledger or event identity was replayed with different immutable evidence.
    ConflictingReplay,
    /// The same immutable item version already exists under another delivery identity.
    DuplicateItemDelivery,
    /// A server sequence was reused by a different delivery identity in the session.
    SequenceConflict,
    /// Item-delivery persistence requires `PostgreSQL` `READ COMMITTED` isolation.
    UnsupportedIsolationLevel,
    /// `PostgreSQL` rejected or could not execute the persistence operation.
    Database(postgres::Error),
}

impl Display for ItemDeliveryPersistenceError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidReference => "item delivery persistence references must be opaque values",
            Self::ConflictingReplay => {
                "item delivery identity was replayed with conflicting evidence"
            }
            Self::DuplicateItemDelivery => {
                "item version was already delivered in this persisted session"
            }
            Self::SequenceConflict => {
                "item delivery sequence was reused by a different delivery identity"
            }
            Self::UnsupportedIsolationLevel => {
                "item delivery persistence requires read committed isolation"
            }
            Self::Database(_) => "PostgreSQL item-delivery persistence failed",
        })
    }
}

impl Error for ItemDeliveryPersistenceError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Database(error) => Some(error),
            _ => None,
        }
    }
}

impl From<postgres::Error> for ItemDeliveryPersistenceError {
    fn from(error: postgres::Error) -> Self {
        Self::Database(error)
    }
}

/// Apply the idempotent item-delivery migration to a `PostgreSQL` connection.
///
/// # Errors
///
/// Returns the `PostgreSQL` error if the migration cannot be applied.
pub fn apply_item_delivery_migration(
    client: &mut impl GenericClient,
) -> Result<(), postgres::Error> {
    client.batch_execute(ITEM_DELIVERY_MIGRATION)
}

/// Persist one session-bound item-delivery ledger and its accepted events.
///
/// Exact replay of the same release binding and event evidence is idempotent.
/// Rebinding the session to a different release, locale, digest, or allowed item
/// set fails closed. Reusing a delivery identity with different item, presentation,
/// selection, or sequence evidence fails closed. A different delivery identity
/// cannot persist an already delivered item version or reuse a server sequence.
///
/// The insert-then-inspect classifier requires `READ COMMITTED` so a concurrent
/// insert that wins the unique-key race is visible to the replay statement.
///
/// # Errors
///
/// Returns [`ItemDeliveryPersistenceError`] for unsupported isolation, conflicting
/// replay, a duplicate item or sequence, an invalid reference, or a database failure.
pub fn persist_item_delivery_ledger(
    transaction: &mut Transaction<'_>,
    ledger: &ItemDeliveryLedger,
) -> Result<ItemDeliveryPersistenceDisposition, ItemDeliveryPersistenceError> {
    require_read_committed(transaction)?;
    let session_ref = required_reference(ledger.session_ref())?;
    let mut inserted_any = persist_ledger_header(transaction, ledger, session_ref)?;
    for event in ledger.events() {
        if persist_one_event(transaction, session_ref, event)? {
            inserted_any = true;
        }
    }
    if inserted_any {
        Ok(ItemDeliveryPersistenceDisposition::Inserted)
    } else {
        Ok(ItemDeliveryPersistenceDisposition::Duplicate)
    }
}

fn persist_ledger_header(
    transaction: &mut Transaction<'_>,
    ledger: &ItemDeliveryLedger,
    session_ref: &str,
) -> Result<bool, ItemDeliveryPersistenceError> {
    let allowed_item_version_refs = ledger.allowed_item_version_refs().to_vec();
    let inserted = transaction.execute(
        "INSERT INTO item_delivery_ledger (\
             session_ref, instrument_release_ref, release_content_digest, locale, \
             allowed_item_version_refs\
         ) VALUES ($1, $2, $3, $4, $5) \
         ON CONFLICT (session_ref) DO NOTHING",
        &[
            &session_ref,
            &ledger.instrument_release_ref(),
            &ledger.release_content_digest(),
            &ledger.locale(),
            &allowed_item_version_refs,
        ],
    )?;
    if inserted == 1 {
        return Ok(true);
    }

    let row = transaction.query_one(
        "SELECT instrument_release_ref, release_content_digest, locale, allowed_item_version_refs \
         FROM item_delivery_ledger WHERE session_ref = $1",
        &[&session_ref],
    )?;
    let stored_release_ref: String = row.get(0);
    let stored_digest: String = row.get(1);
    let stored_locale: String = row.get(2);
    let stored_allowed: Vec<String> = row.get(3);
    if stored_release_ref == ledger.instrument_release_ref()
        && stored_digest == ledger.release_content_digest()
        && stored_locale == ledger.locale()
        && stored_allowed == allowed_item_version_refs
    {
        Ok(false)
    } else {
        Err(ItemDeliveryPersistenceError::ConflictingReplay)
    }
}

fn persist_one_event(
    transaction: &mut Transaction<'_>,
    session_ref: &str,
    event: &ItemDeliveryEvent,
) -> Result<bool, ItemDeliveryPersistenceError> {
    let delivery_event_ref = required_reference(event.delivery_ref())?;
    #[allow(clippy::cast_possible_wrap)]
    let sequence = event.sequence() as i64;
    let selection_evidence_ref = event.selection_evidence_ref();
    let inserted = match transaction.execute(
        "INSERT INTO item_delivery_event (\
             session_ref, delivery_event_ref, item_version_ref, presentation_context_ref, \
             selection_evidence_ref, delivery_sequence\
         ) VALUES ($1, $2, $3, $4, $5, $6) \
         ON CONFLICT (session_ref, delivery_event_ref) DO NOTHING",
        &[
            &session_ref,
            &delivery_event_ref,
            &event.item_version_ref(),
            &event.presentation_context_ref(),
            &selection_evidence_ref,
            &sequence,
        ],
    ) {
        Ok(count) => count,
        Err(error) => return Err(classify_unique_violation(error)),
    };
    if inserted == 1 {
        return Ok(true);
    }

    let row = transaction.query_one(
        "SELECT item_version_ref, presentation_context_ref, selection_evidence_ref, \
                delivery_sequence \
         FROM item_delivery_event \
         WHERE session_ref = $1 AND delivery_event_ref = $2",
        &[&session_ref, &delivery_event_ref],
    )?;
    classify_existing_event(&row, event, sequence)
}

fn classify_existing_event(
    row: &postgres::Row,
    event: &ItemDeliveryEvent,
    sequence: i64,
) -> Result<bool, ItemDeliveryPersistenceError> {
    let stored_item_version_ref: String = row.get(0);
    let stored_presentation_context_ref: String = row.get(1);
    let stored_selection_evidence_ref: Option<String> = row.get(2);
    let stored_sequence: i64 = row.get(3);
    if stored_item_version_ref == event.item_version_ref()
        && stored_presentation_context_ref == event.presentation_context_ref()
        && stored_selection_evidence_ref.as_deref() == event.selection_evidence_ref()
        && stored_sequence == sequence
    {
        Ok(false)
    } else {
        Err(ItemDeliveryPersistenceError::ConflictingReplay)
    }
}

fn classify_unique_violation(error: postgres::Error) -> ItemDeliveryPersistenceError {
    let Some(database_error) = error.as_db_error() else {
        return ItemDeliveryPersistenceError::Database(error);
    };
    match database_error.constraint() {
        Some("item_delivery_event_item_version_unique") => {
            ItemDeliveryPersistenceError::DuplicateItemDelivery
        }
        Some("item_delivery_event_sequence_unique") => {
            ItemDeliveryPersistenceError::SequenceConflict
        }
        _ => ItemDeliveryPersistenceError::Database(error),
    }
}

fn required_reference(reference: &str) -> Result<&str, ItemDeliveryPersistenceError> {
    normalized_reference(reference).ok_or(ItemDeliveryPersistenceError::InvalidReference)
}

fn require_read_committed(
    transaction: &mut Transaction<'_>,
) -> Result<(), ItemDeliveryPersistenceError> {
    let row = transaction.query_one("SHOW transaction_isolation", &[])?;
    let isolation: String = row.get(0);
    if isolation == "read committed" {
        Ok(())
    } else {
        Err(ItemDeliveryPersistenceError::UnsupportedIsolationLevel)
    }
}

#[cfg(test)]
mod reference_guard_tests {
    use super::{required_reference, ItemDeliveryPersistenceError};

    #[test]
    fn blank_and_numeric_references_fail_closed() {
        assert!(matches!(
            required_reference(" "),
            Err(ItemDeliveryPersistenceError::InvalidReference)
        ));
        assert!(matches!(
            required_reference("12"),
            Err(ItemDeliveryPersistenceError::InvalidReference)
        ));
        assert_eq!(
            required_reference("session_item_delivery_alpha").unwrap(),
            "session_item_delivery_alpha"
        );
    }
}
