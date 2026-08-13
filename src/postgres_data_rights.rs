//! `PostgreSQL` persistence for participant data-rights requests and dependent-system propagation.
//!
//! The adapter stores product-owned request identity and, in the same local transaction,
//! enqueues immutable integration events for each declared dependent system. It never opens
//! another service database. A request replay is accepted only when the request evidence,
//! propagation target set, event identities, and outbox evidence are all unchanged.

use crate::data_rights::{DataRightsRequest, DataRightsRequestKind, DataRightsState};
use crate::integration::IntegrationEvent;
use crate::postgres_integration::{enqueue_outbox_event, PersistenceError};
use crate::reference::normalized_reference;
use postgres::{Client, Transaction};
use std::error::Error;
use std::fmt::{Display, Formatter};

const DATA_RIGHTS_MIGRATION: &str = include_str!("../migrations/0003_data_rights_propagation.sql");
const SOURCE_REF: &str = "psychometrics_commons";
const SCHEMA_VERSION: &str = "v1";

/// Whether a durable request was inserted or an exact prior request was replayed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DataRightsPersistenceDisposition {
    /// New durable request, propagation rows, and outbox evidence were committed.
    Inserted,
    /// Exact durable request and propagation evidence already existed.
    Duplicate,
}

/// One dependent system that must receive the immutable request event.
#[derive(Clone, Copy, Debug)]
pub struct DataRightsPropagationTarget<'a> {
    dependent_system_ref: &'a str,
    event: &'a IntegrationEvent,
}

impl<'a> DataRightsPropagationTarget<'a> {
    /// Bind one opaque dependent-system reference to its immutable outbox event.
    #[must_use]
    pub const fn new(dependent_system_ref: &'a str, event: &'a IntegrationEvent) -> Self {
        Self {
            dependent_system_ref,
            event,
        }
    }
}

/// Fail-closed error for durable data-rights propagation persistence.
#[derive(Debug)]
#[non_exhaustive]
pub enum DataRightsPersistenceError {
    /// A dependent-system reference was invalid.
    InvalidReference,
    /// Only newly requested domain resources may enter this first durable slice.
    InvalidRequestState,
    /// At least one dependent-system target is required by this propagation operation.
    EmptyTargetSet,
    /// The same dependent system was supplied more than once.
    DuplicateTarget,
    /// The same immutable outbox event identity was bound to more than one system.
    DuplicateEventIdentity,
    /// A target event did not identify the exact request, tenant, source, kind, or time.
    InvalidPropagationEnvelope,
    /// The request identity was replayed with different immutable evidence or target set.
    ConflictingReplay,
    /// The configured transaction isolation cannot preserve the replay classifier.
    UnsupportedIsolationLevel,
    /// A timestamp cannot be represented by the `PostgreSQL` bigint contract.
    ValueOutOfRange,
    /// Existing integration-outbox persistence rejected the event evidence.
    Integration(PersistenceError),
    /// `PostgreSQL` rejected or could not execute the local transaction.
    Database(postgres::Error),
}

impl Display for DataRightsPersistenceError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidReference => {
                "data-rights propagation references must be opaque non-numeric values"
            }
            Self::InvalidRequestState => "data-rights durable propagation requires Requested state",
            Self::EmptyTargetSet => {
                "data-rights propagation requires at least one dependent system"
            }
            Self::DuplicateTarget => {
                "data-rights propagation target set contains a duplicate system"
            }
            Self::DuplicateEventIdentity => {
                "data-rights propagation target set reuses an event identity"
            }
            Self::InvalidPropagationEnvelope => {
                "data-rights propagation event does not match the durable request"
            }
            Self::ConflictingReplay => {
                "data-rights request was replayed with conflicting durable evidence"
            }
            Self::UnsupportedIsolationLevel => {
                "data-rights persistence requires read committed isolation"
            }
            Self::ValueOutOfRange => {
                "data-rights persistence value exceeds the PostgreSQL bigint range"
            }
            Self::Integration(_) => "data-rights outbox evidence failed persistence validation",
            Self::Database(_) => "PostgreSQL data-rights persistence operation failed",
        })
    }
}

impl Error for DataRightsPersistenceError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Integration(error) => Some(error),
            Self::Database(error) => Some(error),
            _ => None,
        }
    }
}

impl From<postgres::Error> for DataRightsPersistenceError {
    fn from(error: postgres::Error) -> Self {
        Self::Database(error)
    }
}

impl From<PersistenceError> for DataRightsPersistenceError {
    fn from(error: PersistenceError) -> Self {
        Self::Integration(error)
    }
}

/// Apply the idempotent data-rights propagation migration.
///
/// Integration migration `0001` must already be present because propagation rows reference
/// durable outbox identities.
///
/// # Errors
///
/// Returns the `PostgreSQL` error if the migration cannot be applied.
pub fn apply_data_rights_migration(client: &mut Client) -> Result<(), postgres::Error> {
    client.batch_execute(DATA_RIGHTS_MIGRATION)
}

/// Persist one requested data-rights resource and all declared dependent-system outbox events.
///
/// The function owns one short local `PostgreSQL` transaction so a database or outbox error rolls
/// back the request row, target rows, and any earlier event inserts together. It does not deliver
/// events itself; the existing outbox worker owns retry, quarantine, and reconciliation behavior.
/// Exact request replay is idempotent only when the target set and outbox evidence are unchanged.
/// The insert-then-inspect first-write classifier requires `READ COMMITTED`, matching the existing
/// integration outbox replay contract, and fails closed when the session default uses stronger
/// isolation.
///
/// # Errors
///
/// Returns [`DataRightsPersistenceError`] for invalid request state, target identities, event
/// envelope mismatch, unsupported isolation, conflicting replay, out-of-range time, integration
/// persistence failure, or database failure.
pub fn persist_requested_data_rights_with_propagation(
    client: &mut Client,
    request: &DataRightsRequest,
    targets: &[DataRightsPropagationTarget<'_>],
    max_attempts: usize,
) -> Result<DataRightsPersistenceDisposition, DataRightsPersistenceError> {
    if request.state() != DataRightsState::Requested {
        return Err(DataRightsPersistenceError::InvalidRequestState);
    }
    if targets.is_empty() {
        return Err(DataRightsPersistenceError::EmptyTargetSet);
    }
    let requested_at = i64::try_from(request.requested_at_unix_ms())
        .map_err(|_| DataRightsPersistenceError::ValueOutOfRange)?;
    validate_targets(request, targets)?;

    let mut transaction = client.transaction()?;
    require_read_committed(&mut transaction)?;
    let disposition = persist_request_header(&mut transaction, request, targets, requested_at)?;

    for target in targets {
        enqueue_outbox_event(&mut transaction, target.event, max_attempts)?;
        if disposition == DataRightsPersistenceDisposition::Inserted {
            transaction.execute(
                "INSERT INTO data_rights_propagation_state (\
                     request_ref, tenant_ref, dependent_system_ref, source_ref, event_ref,\
                     current_state, latest_event_at_unix_ms\
                 ) VALUES ($1,$2,$3,$4,$5,'pending',$6)",
                &[
                    &request.request_ref(),
                    &request.tenant_ref(),
                    &normalized_reference(target.dependent_system_ref)
                        .ok_or(DataRightsPersistenceError::InvalidReference)?,
                    &target.event.source(),
                    &target.event.event_ref(),
                    &requested_at,
                ],
            )?;
        }
    }

    transaction.commit()?;
    Ok(disposition)
}

fn persist_request_header(
    transaction: &mut Transaction<'_>,
    request: &DataRightsRequest,
    targets: &[DataRightsPropagationTarget<'_>],
    requested_at: i64,
) -> Result<DataRightsPersistenceDisposition, DataRightsPersistenceError> {
    transaction.batch_execute("SAVEPOINT data_rights_request_insert")?;
    let inserted = match transaction.execute(
        "INSERT INTO data_rights_request_state (\
             request_ref, tenant_ref, participant_ref, request_kind, scope_ref, current_state,\
             requested_at_unix_ms, latest_event_at_unix_ms\
         ) VALUES ($1,$2,$3,$4,$5,'requested',$6,$6) \
         ON CONFLICT (request_ref) DO NOTHING",
        &[
            &request.request_ref(),
            &request.tenant_ref(),
            &request.participant_ref(),
            &request_kind_name(request.kind()),
            &request.scope_ref(),
            &requested_at,
        ],
    ) {
        Ok(inserted) => {
            transaction.batch_execute("RELEASE SAVEPOINT data_rights_request_insert")?;
            inserted
        }
        Err(error) if is_unique_violation(&error) => {
            transaction.batch_execute("ROLLBACK TO SAVEPOINT data_rights_request_insert")?;
            0
        }
        Err(error) => return Err(error.into()),
    };
    if inserted == 1 {
        return Ok(DataRightsPersistenceDisposition::Inserted);
    }

    let row = transaction.query_one(
        "SELECT tenant_ref, participant_ref, request_kind, scope_ref, current_state, \
                requested_at_unix_ms, latest_event_at_unix_ms \
         FROM data_rights_request_state WHERE request_ref = $1",
        &[&request.request_ref()],
    )?;
    let exact = row.get::<_, String>(0) == request.tenant_ref()
        && row.get::<_, String>(1) == request.participant_ref()
        && row.get::<_, String>(2) == request_kind_name(request.kind())
        && row.get::<_, String>(3) == request.scope_ref()
        && row.get::<_, String>(4) == "requested"
        && row.get::<_, i64>(5) == requested_at
        && row.get::<_, i64>(6) == requested_at;
    if exact && stored_targets_match(transaction, request, targets)? {
        Ok(DataRightsPersistenceDisposition::Duplicate)
    } else {
        Err(DataRightsPersistenceError::ConflictingReplay)
    }
}

fn validate_targets(
    request: &DataRightsRequest,
    targets: &[DataRightsPropagationTarget<'_>],
) -> Result<(), DataRightsPersistenceError> {
    let expected_type = match request.kind() {
        DataRightsRequestKind::Export => "data_rights.export.requested",
        DataRightsRequestKind::Deletion => "data_rights.deletion.requested",
    };
    let mut systems = Vec::with_capacity(targets.len());
    let mut event_refs = Vec::with_capacity(targets.len());
    for target in targets {
        let system = normalized_reference(target.dependent_system_ref)
            .ok_or(DataRightsPersistenceError::InvalidReference)?;
        if systems.contains(&system) {
            return Err(DataRightsPersistenceError::DuplicateTarget);
        }
        systems.push(system);
        let event_ref = target.event.event_ref();
        if event_refs.contains(&event_ref) {
            return Err(DataRightsPersistenceError::DuplicateEventIdentity);
        }
        event_refs.push(event_ref);
        let event = target.event;
        if event.source() != SOURCE_REF
            || event.tenant_ref() != request.tenant_ref()
            || event.subject_ref() != request.request_ref()
            || event.event_type() != expected_type
            || event.schema_version() != SCHEMA_VERSION
            || event.occurred_at_unix_ms() != request.requested_at_unix_ms()
            || event.correlation_ref() != request.request_ref()
            || event.causation_ref().is_some()
        {
            return Err(DataRightsPersistenceError::InvalidPropagationEnvelope);
        }
    }
    Ok(())
}

fn stored_targets_match(
    transaction: &mut Transaction<'_>,
    request: &DataRightsRequest,
    targets: &[DataRightsPropagationTarget<'_>],
) -> Result<bool, postgres::Error> {
    let rows = transaction.query(
        "SELECT dependent_system_ref, source_ref, event_ref \
         FROM data_rights_propagation_state WHERE request_ref = $1",
        &[&request.request_ref()],
    )?;
    if rows.len() != targets.len() {
        return Ok(false);
    }
    Ok(targets.iter().all(|target| {
        let Some(system) = normalized_reference(target.dependent_system_ref) else {
            return false;
        };
        rows.iter().any(|row| {
            row.get::<_, String>(0) == system
                && row.get::<_, String>(1) == target.event.source()
                && row.get::<_, String>(2) == target.event.event_ref()
        })
    }))
}

fn is_unique_violation(error: &postgres::Error) -> bool {
    error.code() == Some(&postgres::error::SqlState::UNIQUE_VIOLATION)
}

fn require_read_committed(
    transaction: &mut Transaction<'_>,
) -> Result<(), DataRightsPersistenceError> {
    let row = transaction.query_one("SHOW transaction_isolation", &[])?;
    let isolation: String = row.get(0);
    if isolation == "read committed" {
        Ok(())
    } else {
        Err(DataRightsPersistenceError::UnsupportedIsolationLevel)
    }
}

const fn request_kind_name(kind: DataRightsRequestKind) -> &'static str {
    match kind {
        DataRightsRequestKind::Export => "export",
        DataRightsRequestKind::Deletion => "deletion",
    }
}
