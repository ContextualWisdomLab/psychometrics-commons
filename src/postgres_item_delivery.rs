//! `PostgreSQL` 18 persistence for tenant- and session-bound item-delivery evidence.
//!
//! Item selection, calibration, and scoring remain in `fast-mlsirm`. Callers own the
//! connection, transaction, credentials, and explicit tenant authorization context.

use crate::item_delivery::{
    ItemDeliveryError, ItemDeliveryEvent, ItemDeliveryLedger, ItemDeliveryRequest,
};
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
    /// A required identity was blank or numeric-like.
    InvalidReference,
    /// An identity was replayed with different immutable evidence.
    ConflictingReplay,
    /// An item version already exists under another delivery identity.
    DuplicateItemDelivery,
    /// A server sequence was reused by another delivery identity.
    SequenceConflict,
    /// Persistence requires `PostgreSQL` `READ COMMITTED` isolation.
    UnsupportedIsolationLevel,
    /// Stored rows cannot reconstruct a valid item-delivery ledger.
    CorruptHistory,
    /// `PostgreSQL` rejected or could not execute the operation.
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
            Self::CorruptHistory => {
                "stored item-delivery history cannot reconstruct a valid ledger"
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

/// Apply the idempotent item-delivery migration.
///
/// # Errors
///
/// Returns the database error when the migration cannot be applied.
pub fn apply_item_delivery_migration(
    client: &mut impl GenericClient,
) -> Result<(), postgres::Error> {
    client.batch_execute(ITEM_DELIVERY_MIGRATION)
}

/// Persist one tenant-bound item-delivery ledger and its accepted events.
///
/// Exact replay under the same tenant is idempotent. Tenant, release, locale, digest,
/// allowed-item, delivery, or event-evidence rebinding fails closed. A whitespace-padded
/// tenant or session alias fails closed before write instead of storing the trimmed
/// identity.
///
/// # Errors
///
/// Returns a typed fail-closed persistence error for invalid evidence, conflicts,
/// unsupported isolation, duplicate item/sequence evidence, or database failures.
pub fn persist_item_delivery_ledger(
    transaction: &mut Transaction<'_>,
    tenant_ref: &str,
    ledger: &ItemDeliveryLedger,
) -> Result<ItemDeliveryPersistenceDisposition, ItemDeliveryPersistenceError> {
    require_read_committed(transaction)?;
    let tenant_ref = required_reference(tenant_ref)?;
    let session_ref = required_reference(ledger.session_ref())?;
    let mut inserted_any = persist_ledger_header(transaction, tenant_ref, ledger, session_ref)?;
    for event in ledger.events() {
        if persist_one_event(transaction, tenant_ref, session_ref, event)? {
            inserted_any = true;
        }
    }
    Ok(if inserted_any {
        ItemDeliveryPersistenceDisposition::Inserted
    } else {
        ItemDeliveryPersistenceDisposition::Duplicate
    })
}

/// Reload one tenant-bound item-delivery ledger after process restart.
///
/// Events are reconstructed in stored `delivery_sequence` order. A missing
/// session is absent rather than an empty delivery list. A header that exists
/// for a different tenant fails closed instead of looking like a new session.
/// A sequence gap or an item that is not in the stored allowed set fails closed
/// so a restarted runtime cannot skip or re-present items.
///
/// The caller owns the `READ COMMITTED` transaction. The load takes `FOR SHARE`
/// on the ledger header. [`persist_item_delivery_ledger`] inserts the header
/// without `FOR UPDATE`, so the share lock does not by itself hide a concurrent
/// persist append.
///
/// # Errors
///
/// Returns [`ItemDeliveryPersistenceError`] for an invalid tenant or session
/// reference, [`ItemDeliveryPersistenceError::ConflictingReplay`] when the
/// stored header belongs to another tenant, unsupported isolation, stored rows
/// that cannot reconstruct a valid ledger, or a database failure.
pub fn load_item_delivery_ledger(
    transaction: &mut Transaction<'_>,
    tenant_ref: &str,
    session_ref: &str,
) -> Result<Option<ItemDeliveryLedger>, ItemDeliveryPersistenceError> {
    require_read_committed(transaction)?;
    let tenant_ref = exact_reference(tenant_ref)?;
    let session_ref = exact_reference(session_ref)?;
    let Some(header) = transaction.query_opt(
        "SELECT tenant_ref, instrument_release_ref, release_content_digest, locale, \
                allowed_item_version_refs \
         FROM item_delivery_ledger WHERE session_ref = $1 FOR SHARE",
        &[&session_ref],
    )?
    else {
        return Ok(None);
    };
    let stored_tenant_ref: String = header.get(0);
    if stored_tenant_ref != tenant_ref {
        return Err(ItemDeliveryPersistenceError::ConflictingReplay);
    }
    let instrument_release_ref: String = header.get(1);
    let release_content_digest: String = header.get(2);
    let locale: String = header.get(3);
    let allowed_item_version_refs: Vec<String> = header.get(4);
    let allowed: Vec<&str> = allowed_item_version_refs
        .iter()
        .map(String::as_str)
        .collect();
    let mut ledger = ItemDeliveryLedger::from_persisted(
        session_ref,
        instrument_release_ref.as_str(),
        release_content_digest.as_str(),
        locale.as_str(),
        &allowed,
    )
    .map_err(reconstruct_error)?;
    let rows = transaction.query(
        "SELECT delivery_event_ref, item_version_ref, presentation_context_ref, \
                selection_evidence_ref, delivery_sequence \
         FROM item_delivery_event \
         WHERE session_ref = $1 AND tenant_ref = $2 \
         ORDER BY delivery_sequence ASC",
        &[&session_ref, &tenant_ref],
    )?;
    for row in rows {
        let delivery_ref: String = row.get(0);
        let item_version_ref: String = row.get(1);
        let presentation_context_ref: String = row.get(2);
        let selection_evidence_ref: Option<String> = row.get(3);
        let sequence = stored_sequence(row.get(4))?;
        ledger
            .restore_persisted_event(
                ItemDeliveryRequest {
                    delivery_ref: delivery_ref.as_str(),
                    item_version_ref: item_version_ref.as_str(),
                    presentation_context_ref: presentation_context_ref.as_str(),
                    selection_evidence_ref: selection_evidence_ref.as_deref(),
                },
                sequence,
            )
            .map_err(reconstruct_error)?;
    }
    Ok(Some(ledger))
}

fn persist_ledger_header(
    transaction: &mut Transaction<'_>,
    tenant_ref: &str,
    ledger: &ItemDeliveryLedger,
    session_ref: &str,
) -> Result<bool, ItemDeliveryPersistenceError> {
    let allowed_item_version_refs = ledger.allowed_item_version_refs().to_vec();
    let inserted = transaction.execute(
        "INSERT INTO item_delivery_ledger (\
             tenant_ref, session_ref, instrument_release_ref, release_content_digest, locale, \
             allowed_item_version_refs\
         ) VALUES ($1, $2, $3, $4, $5, $6) \
         ON CONFLICT (session_ref) DO NOTHING",
        &[
            &tenant_ref,
            &session_ref,
            &ledger.instrument_release_ref(),
            &ledger.release_content_digest(),
            &ledger.locale(),
            &allowed_item_version_refs,
        ],
    )? == 1;

    // Under READ COMMITTED, each SQL statement gets a fresh snapshot. Keep the
    // conflict insert and replay classification separate so a transaction that
    // waited for a concurrent winning insert can see that just-committed row.
    let row = transaction.query_one(
        "SELECT tenant_ref, instrument_release_ref, release_content_digest, locale, \
                allowed_item_version_refs \
         FROM item_delivery_ledger WHERE session_ref = $1",
        &[&session_ref],
    )?;
    let stored_tenant_ref: String = row.get(0);
    let stored_release_ref: String = row.get(1);
    let stored_digest: String = row.get(2);
    let stored_locale: String = row.get(3);
    let stored_allowed: Vec<String> = row.get(4);
    if stored_tenant_ref == tenant_ref
        && stored_release_ref == ledger.instrument_release_ref()
        && stored_digest == ledger.release_content_digest()
        && stored_locale == ledger.locale()
        && stored_allowed == allowed_item_version_refs
    {
        Ok(inserted)
    } else {
        Err(ItemDeliveryPersistenceError::ConflictingReplay)
    }
}

fn persist_one_event(
    transaction: &mut Transaction<'_>,
    tenant_ref: &str,
    session_ref: &str,
    event: &ItemDeliveryEvent,
) -> Result<bool, ItemDeliveryPersistenceError> {
    let delivery_event_ref = required_reference(event.delivery_ref())?;
    #[allow(clippy::cast_possible_wrap)]
    let sequence = event.sequence() as i64;
    let selection_evidence_ref = event.selection_evidence_ref();
    let row = match transaction.query_one(
        "WITH inserted AS (\
             INSERT INTO item_delivery_event (\
                 tenant_ref, session_ref, delivery_event_ref, item_version_ref, \
                 presentation_context_ref, selection_evidence_ref, delivery_sequence\
             ) VALUES ($1, $2, $3, $4, $5, $6, $7) \
             ON CONFLICT (session_ref, delivery_event_ref) DO NOTHING \
             RETURNING tenant_ref, item_version_ref, presentation_context_ref, \
                       selection_evidence_ref, delivery_sequence, TRUE AS inserted\
         ) \
         SELECT tenant_ref, item_version_ref, presentation_context_ref, \
                selection_evidence_ref, delivery_sequence, inserted \
         FROM inserted \
         UNION ALL \
         SELECT tenant_ref, item_version_ref, presentation_context_ref, \
                selection_evidence_ref, delivery_sequence, FALSE AS inserted \
         FROM item_delivery_event \
         WHERE session_ref = $2 AND delivery_event_ref = $3 \
         LIMIT 1",
        &[
            &tenant_ref,
            &session_ref,
            &delivery_event_ref,
            &event.item_version_ref(),
            &event.presentation_context_ref(),
            &selection_evidence_ref,
            &sequence,
        ],
    ) {
        Ok(row) => row,
        Err(error) => return Err(classify_unique_violation(error)),
    };
    let inserted: bool = row.get(5);
    if inserted {
        Ok(true)
    } else {
        classify_existing_event(&row, tenant_ref, event, sequence)
    }
}

fn classify_existing_event(
    row: &postgres::Row,
    tenant_ref: &str,
    event: &ItemDeliveryEvent,
    sequence: i64,
) -> Result<bool, ItemDeliveryPersistenceError> {
    let stored_tenant_ref: String = row.get(0);
    let stored_item_version_ref: String = row.get(1);
    let stored_presentation_context_ref: String = row.get(2);
    let stored_selection_evidence_ref: Option<String> = row.get(3);
    let stored_sequence: i64 = row.get(4);
    if stored_tenant_ref == tenant_ref
        && stored_item_version_ref == event.item_version_ref()
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
    match error
        .as_db_error()
        .and_then(postgres::error::DbError::constraint)
    {
        Some("item_delivery_event_delivery_ref_unique") => {
            ItemDeliveryPersistenceError::ConflictingReplay
        }
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
    exact_reference(reference)
}

fn exact_reference(reference: &str) -> Result<&str, ItemDeliveryPersistenceError> {
    match normalized_reference(reference) {
        Some(normalized) if normalized == reference => Ok(normalized),
        _ => Err(ItemDeliveryPersistenceError::InvalidReference),
    }
}

fn stored_sequence(value: i64) -> Result<usize, ItemDeliveryPersistenceError> {
    usize::try_from(value)
        .ok()
        .filter(|sequence| *sequence > 0)
        .ok_or(ItemDeliveryPersistenceError::CorruptHistory)
}

fn reconstruct_error(error: ItemDeliveryError) -> ItemDeliveryPersistenceError {
    match error {
        ItemDeliveryError::InvalidReference => ItemDeliveryPersistenceError::InvalidReference,
        ItemDeliveryError::CorruptHistory
        | ItemDeliveryError::ItemNotInRelease
        | ItemDeliveryError::DuplicateItemDelivery
        | ItemDeliveryError::IdempotencyConflict
        | ItemDeliveryError::SessionNotActive(_) => ItemDeliveryPersistenceError::CorruptHistory,
    }
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
    use super::{
        exact_reference, reconstruct_error, required_reference, stored_sequence,
        ItemDeliveryPersistenceError,
    };
    use crate::item_delivery::ItemDeliveryError;
    use crate::session::SessionState;

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

    #[test]
    fn persist_rejects_padded_tenant_aliases_instead_of_trimming() {
        assert!(matches!(
            required_reference(" tenant_item_delivery_alpha"),
            Err(ItemDeliveryPersistenceError::InvalidReference)
        ));
        assert!(matches!(
            required_reference("tenant_item_delivery_alpha "),
            Err(ItemDeliveryPersistenceError::InvalidReference)
        ));
        assert_eq!(
            required_reference("tenant_item_delivery_alpha").unwrap(),
            "tenant_item_delivery_alpha"
        );
    }

    #[test]
    fn reload_rejects_padded_aliases_and_non_positive_sequences() {
        assert!(matches!(
            exact_reference(" session_item_delivery_alpha"),
            Err(ItemDeliveryPersistenceError::InvalidReference)
        ));
        assert_eq!(
            exact_reference("session_item_delivery_alpha").unwrap(),
            "session_item_delivery_alpha"
        );
        assert!(matches!(
            stored_sequence(0),
            Err(ItemDeliveryPersistenceError::CorruptHistory)
        ));
        assert!(matches!(
            stored_sequence(-1),
            Err(ItemDeliveryPersistenceError::CorruptHistory)
        ));
        assert_eq!(stored_sequence(2).unwrap(), 2);
    }

    #[test]
    fn reconstruct_maps_domain_failures_to_typed_persistence_errors() {
        assert!(matches!(
            reconstruct_error(ItemDeliveryError::InvalidReference),
            ItemDeliveryPersistenceError::InvalidReference
        ));
        for error in [
            ItemDeliveryError::CorruptHistory,
            ItemDeliveryError::ItemNotInRelease,
            ItemDeliveryError::DuplicateItemDelivery,
            ItemDeliveryError::IdempotencyConflict,
            ItemDeliveryError::SessionNotActive(SessionState::Completed),
        ] {
            assert!(matches!(
                reconstruct_error(error),
                ItemDeliveryPersistenceError::CorruptHistory
            ));
        }
    }
}
