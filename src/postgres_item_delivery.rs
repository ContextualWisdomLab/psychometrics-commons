//! `PostgreSQL` 18 persistence for tenant- and session-bound item-delivery evidence.
//!
//! Item selection, calibration, and scoring remain in `fast-mlsirm`. Callers own the
//! connection, transaction, credentials, and explicit tenant authorization context.

use crate::item_delivery::{ItemDeliveryError, ItemDeliveryEvent, ItemDeliveryLedger};
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
    /// `PostgreSQL` rejected or could not execute the operation.
    Database(postgres::Error),
    /// Durable rows cannot reconstruct the domain item-delivery ledger.
    InconsistentEvidence,
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
            Self::InconsistentEvidence => {
                "durable item-delivery evidence cannot reconstruct the session ledger"
            }
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
/// allowed-item, delivery, or event-evidence rebinding fails closed.
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

/// Load one tenant-bound item-delivery ledger from durable evidence.
///
/// Returns `Ok(None)` when no ledger header exists. An empty header
/// reconstructs as an empty [`ItemDeliveryLedger`] still bound to the stored
/// release item set. Events are ordered by `delivery_sequence` and must form
/// the same monotonic prefix the domain assigned. After load,
/// [`ItemDeliveryLedger::deliver`] continues that prefix.
///
/// # Errors
///
/// Returns [`ItemDeliveryPersistenceError`] for unsupported isolation, an
/// invalid tenant or session reference, a tenant mismatch, inconsistent
/// durable evidence, or a database failure.
pub fn load_item_delivery_ledger(
    transaction: &mut Transaction<'_>,
    tenant_ref: &str,
    session_ref: &str,
) -> Result<Option<ItemDeliveryLedger>, ItemDeliveryPersistenceError> {
    require_read_committed(transaction)?;
    let tenant_ref = required_reference(tenant_ref)?;
    let session_ref = required_reference(session_ref)?;
    let header = transaction.query_opt(
        "SELECT tenant_ref, instrument_release_ref, release_content_digest, locale, \
         allowed_item_version_refs FROM item_delivery_ledger WHERE session_ref = $1",
        &[&session_ref],
    )?;
    let Some(header) = header else {
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
    let rows = transaction.query(
        "SELECT delivery_event_ref, item_version_ref, presentation_context_ref, \
         selection_evidence_ref, delivery_sequence FROM item_delivery_event \
         WHERE session_ref = $1 ORDER BY delivery_sequence ASC",
        &[&session_ref],
    )?;
    let mut events = Vec::with_capacity(rows.len());
    for row in rows {
        let sequence = stored_sequence(row.get(4))?;
        let selection_evidence_ref: Option<String> = row.get(3);
        events.push(
            ItemDeliveryEvent::from_durable_evidence(
                row.get::<_, String>(0),
                row.get::<_, String>(1),
                row.get::<_, String>(2),
                selection_evidence_ref.as_deref(),
                sequence,
            )
            .map_err(durable_evidence_error)?,
        );
    }
    ItemDeliveryLedger::from_durable_events(
        session_ref,
        instrument_release_ref,
        release_content_digest,
        locale,
        &allowed_item_version_refs,
        events,
    )
    .map(Some)
    .map_err(durable_evidence_error)
}

fn stored_sequence(sequence: i64) -> Result<usize, ItemDeliveryPersistenceError> {
    usize::try_from(sequence).map_err(|_| ItemDeliveryPersistenceError::InconsistentEvidence)
}

fn durable_evidence_error(error: ItemDeliveryError) -> ItemDeliveryPersistenceError {
    match error {
        ItemDeliveryError::InvalidReference => ItemDeliveryPersistenceError::InvalidReference,
        ItemDeliveryError::InconsistentSequence
        | ItemDeliveryError::IdempotencyConflict
        | ItemDeliveryError::DuplicateItemDelivery
        | ItemDeliveryError::ItemNotInRelease
        | ItemDeliveryError::SessionNotActive(_) => {
            ItemDeliveryPersistenceError::InconsistentEvidence
        }
    }
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
                 tenant_ref, session_ref, instrument_release_ref, release_content_digest, locale, \
                 allowed_item_version_refs\
             ) VALUES ($1, $2, $3, $4, $5, $6) \
             ON CONFLICT (session_ref) DO NOTHING \
             RETURNING tenant_ref, instrument_release_ref, release_content_digest, locale, \
                       allowed_item_version_refs, TRUE AS inserted\
         ) \
         SELECT tenant_ref, instrument_release_ref, release_content_digest, locale, \
                allowed_item_version_refs, inserted \
         FROM inserted \
         UNION ALL \
         SELECT tenant_ref, instrument_release_ref, release_content_digest, locale, \
                allowed_item_version_refs, FALSE AS inserted \
         FROM item_delivery_ledger WHERE session_ref = $2 \
         LIMIT 1",
        &[
            &tenant_ref,
            &session_ref,
            &ledger.instrument_release_ref(),
            &ledger.release_content_digest(),
            &ledger.locale(),
            &allowed_item_version_refs,
        ],
    )?;
    let stored_tenant_ref: String = row.get(0);
    let stored_release_ref: String = row.get(1);
    let stored_digest: String = row.get(2);
    let stored_locale: String = row.get(3);
    let stored_allowed: Vec<String> = row.get(4);
    let inserted: bool = row.get(5);
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
    use super::{
        durable_evidence_error, required_reference, stored_sequence, ItemDeliveryPersistenceError,
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
        assert_eq!(stored_sequence(1).unwrap(), 1);
        assert!(matches!(
            stored_sequence(-1),
            Err(ItemDeliveryPersistenceError::InconsistentEvidence)
        ));
    }

    #[test]
    fn durable_reconstruction_errors_map_to_persistence_failures() {
        assert!(matches!(
            durable_evidence_error(ItemDeliveryError::InvalidReference),
            ItemDeliveryPersistenceError::InvalidReference
        ));
        assert!(matches!(
            durable_evidence_error(ItemDeliveryError::InconsistentSequence),
            ItemDeliveryPersistenceError::InconsistentEvidence
        ));
        assert!(matches!(
            durable_evidence_error(ItemDeliveryError::IdempotencyConflict),
            ItemDeliveryPersistenceError::InconsistentEvidence
        ));
        assert!(matches!(
            durable_evidence_error(ItemDeliveryError::DuplicateItemDelivery),
            ItemDeliveryPersistenceError::InconsistentEvidence
        ));
        assert!(matches!(
            durable_evidence_error(ItemDeliveryError::ItemNotInRelease),
            ItemDeliveryPersistenceError::InconsistentEvidence
        ));
        assert!(matches!(
            durable_evidence_error(ItemDeliveryError::SessionNotActive(SessionState::Paused)),
            ItemDeliveryPersistenceError::InconsistentEvidence
        ));
    }
}
