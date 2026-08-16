//! `PostgreSQL` 18 persistence for tenant- and session-bound item-delivery evidence.
//!
//! Item selection, calibration, and scoring remain in `fast-mlsirm`. Callers own the
//! connection, transaction, credentials, and explicit tenant authorization context.

use crate::item_delivery::{ItemDeliveryEvent, ItemDeliveryLedger};
use crate::reference::normalized_reference;
use postgres::{GenericClient, Transaction};
use std::error::Error;
use std::fmt::{Display, Formatter};

const ITEM_DELIVERY_MIGRATION: &str = include_str!("../migrations/0004_item_delivery_evidence.sql");
const ITEM_DELIVERY_VERSION_MIGRATION: &str =
    include_str!("../migrations/0020_item_delivery_instrument_version.sql");

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
    client.batch_execute(ITEM_DELIVERY_MIGRATION)?;
    client.batch_execute(ITEM_DELIVERY_VERSION_MIGRATION)
}

/// Persist one tenant-bound item-delivery ledger and its accepted events.
///
/// Exact replay under the same tenant is idempotent. Tenant, release, instrument
/// version, locale, digest, allowed-item, delivery, or event-evidence rebinding
/// fails closed.
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

fn persist_ledger_header(
    transaction: &mut Transaction<'_>,
    tenant_ref: &str,
    ledger: &ItemDeliveryLedger,
    session_ref: &str,
) -> Result<bool, ItemDeliveryPersistenceError> {
    let allowed_item_version_refs = ledger.allowed_item_version_refs().to_vec();
    let row = transaction.query_one(
        "WITH inserted AS (\
             INSERT INTO item_delivery_ledger (\
                 tenant_ref, session_ref, instrument_release_ref, instrument_version_ref, \
                 release_content_digest, locale, allowed_item_version_refs\
             ) VALUES ($1, $2, $3, $4, $5, $6, $7) \
             ON CONFLICT (session_ref) DO NOTHING \
             RETURNING tenant_ref, instrument_release_ref, instrument_version_ref, \
                       release_content_digest, locale, allowed_item_version_refs, TRUE AS inserted\
         ) \
         SELECT tenant_ref, instrument_release_ref, instrument_version_ref, \
                release_content_digest, locale, allowed_item_version_refs, inserted \
         FROM inserted \
         UNION ALL \
         SELECT tenant_ref, instrument_release_ref, instrument_version_ref, \
                release_content_digest, locale, allowed_item_version_refs, FALSE AS inserted \
         FROM item_delivery_ledger WHERE session_ref = $2 \
         LIMIT 1",
        &[
            &tenant_ref,
            &session_ref,
            &ledger.instrument_release_ref(),
            &ledger.instrument_version_ref(),
            &ledger.release_content_digest(),
            &ledger.locale(),
            &allowed_item_version_refs,
        ],
    )?;
    let stored_tenant_ref: String = row.get(0);
    let stored_release_ref: String = row.get(1);
    let stored_version_ref: Option<String> = row.get(2);
    let stored_digest: String = row.get(3);
    let stored_locale: String = row.get(4);
    let stored_allowed: Vec<String> = row.get(5);
    let inserted: bool = row.get(6);
    if stored_tenant_ref == tenant_ref
        && stored_release_ref == ledger.instrument_release_ref()
        && stored_version_ref.as_deref() == Some(ledger.instrument_version_ref())
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
