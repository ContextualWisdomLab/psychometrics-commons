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
const DATA_RIGHTS_VERIFICATION_MIGRATION: &str =
    include_str!("../migrations/0015_data_rights_identity_verification.sql");
const SOURCE_REF: &str = "psychometrics_commons";
const SCHEMA_VERSION: &str = "v1";

/// Outcome of persisting requester identity verification for one data-rights request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DataRightsVerificationDisposition {
    /// The requested identity was verified for the first time.
    Verified,
    /// The same verification evidence was replayed exactly.
    Duplicate,
}

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
    /// The domain resource is not in a state this persistence operation accepts.
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
    /// The requested data-rights identity does not exist.
    RequestNotFound,
    /// `PostgreSQL` rejected or could not execute the local transaction.
    Database(postgres::Error),
}

impl Display for DataRightsPersistenceError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidReference => {
                "data-rights propagation references must be opaque non-numeric values"
            }
            Self::InvalidRequestState => {
                "data-rights persistence received a request in a state this operation does not accept"
            }
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
            Self::RequestNotFound => "data-rights request does not exist",
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

/// Apply the idempotent data-rights propagation and identity-verification migrations.
///
/// Integration migration `0001` must already be present because propagation rows reference
/// durable outbox identities. Verification columns are added only after the request table
/// exists.
///
/// # Errors
///
/// Returns the `PostgreSQL` error if the migration cannot be applied.
pub fn apply_data_rights_migration(client: &mut Client) -> Result<(), postgres::Error> {
    match client.batch_execute(DATA_RIGHTS_MIGRATION) {
        Ok(()) => client.batch_execute(DATA_RIGHTS_VERIFICATION_MIGRATION),
        Err(error) => Err(error),
    }
}

/// Persist one requested data-rights resource and all declared dependent-system outbox events.
///
/// The function owns one short local `PostgreSQL` transaction so a database or outbox error rolls
/// back the request row, target rows, and any earlier event inserts together. It does not deliver
/// events itself; the existing outbox worker owns retry, quarantine, and reconciliation behavior.
/// Exact creation replay remains idempotent after the stored lifecycle advances when the immutable
/// request evidence, target set, event identities, and outbox evidence are unchanged. The
/// insert-then-inspect first-write classifier requires `READ COMMITTED`, matching the existing
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

/// Persist requester identity verification for one already requested data-rights identity.
///
/// Exact replay of the same evidence and verification time is idempotent. A later
/// conflicting verification fails closed. Replay classification locks the matched
/// request row until the caller-owned transaction ends so the classified lifecycle
/// cannot change before the caller composes subsequent atomic work.
/// This adapter does not start processing or complete the request.
///
/// # Errors
///
/// Returns [`DataRightsPersistenceError`] when the domain state is not identity-verified,
/// isolation is unsupported, the request is missing, stored evidence conflicts, a
/// timestamp is out of range, or the database operation fails.
pub fn persist_data_rights_identity_verification(
    transaction: &mut Transaction<'_>,
    request: &DataRightsRequest,
) -> Result<DataRightsVerificationDisposition, DataRightsPersistenceError> {
    let (evidence_ref, verified_at) = match (
        request.state(),
        request
            .verification_evidence_ref()
            .and_then(normalized_reference),
        request.verified_at_unix_ms(),
    ) {
        (DataRightsState::IdentityVerified, Some(evidence_ref), Some(verified_at_ms)) => {
            match i64::try_from(verified_at_ms) {
                Ok(verified_at) => (evidence_ref, verified_at),
                Err(_) => return Err(DataRightsPersistenceError::ValueOutOfRange),
            }
        }
        _ => return Err(DataRightsPersistenceError::InvalidRequestState),
    };
    require_read_committed(transaction)?;
    let request_kind = request_kind_name(request.kind());

    let updated = query_optional_row(
        transaction,
        "UPDATE data_rights_request_state
         SET current_state = 'identity_verified',
             verification_evidence_ref = $3,
             verified_at_unix_ms = $4,
             latest_event_at_unix_ms = $4,
             updated_at = clock_timestamp()
         WHERE request_ref = $1
           AND tenant_ref = $2
           AND participant_ref = $5
           AND request_kind = $6
           AND scope_ref = $7
           AND current_state = 'requested'
         RETURNING request_ref",
        &[
            &request.request_ref(),
            &request.tenant_ref(),
            &evidence_ref,
            &verified_at,
            &request.participant_ref(),
            &request_kind,
            &request.scope_ref(),
        ],
    )?;
    if updated.is_some() {
        return Ok(DataRightsVerificationDisposition::Verified);
    }

    let row = query_optional_row(
        transaction,
        "SELECT participant_ref, request_kind, scope_ref,
                current_state, verification_evidence_ref, verified_at_unix_ms
         FROM data_rights_request_state
         WHERE request_ref = $1 AND tenant_ref = $2
         FOR UPDATE",
        &[&request.request_ref(), &request.tenant_ref()],
    )?;
    let Some(row) = row else {
        return Err(DataRightsPersistenceError::RequestNotFound);
    };
    let stored_participant: String = row.get(0);
    let stored_kind: String = row.get(1);
    let stored_scope: String = row.get(2);
    let stored_state: String = row.get(3);
    let stored_evidence: Option<String> = row.get(4);
    let stored_verified_at: Option<i64> = row.get(5);
    let identity_matches = stored_participant == request.participant_ref()
        && stored_kind == request_kind
        && stored_scope == request.scope_ref();
    if !identity_matches {
        Err(DataRightsPersistenceError::ConflictingReplay)
    } else if stored_state == "identity_verified"
        && stored_evidence.as_deref() == Some(evidence_ref)
        && stored_verified_at == Some(verified_at)
    {
        Ok(DataRightsVerificationDisposition::Duplicate)
    } else if stored_state == "identity_verified" {
        Err(DataRightsPersistenceError::ConflictingReplay)
    } else {
        Err(DataRightsPersistenceError::InvalidRequestState)
    }
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
         ) VALUES ($1,$2,$3,$4,$5,'requested',$6,$6)",
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
        "SELECT tenant_ref, participant_ref, request_kind, scope_ref, requested_at_unix_ms \
         FROM data_rights_request_state WHERE request_ref = $1",
        &[&request.request_ref()],
    )?;
    let exact = row.get::<_, String>(0) == request.tenant_ref()
        && row.get::<_, String>(1) == request.participant_ref()
        && row.get::<_, String>(2) == request_kind_name(request.kind())
        && row.get::<_, String>(3) == request.scope_ref()
        && row.get::<_, i64>(4) == requested_at;
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
        rows.iter().any(|row| {
            row.get::<_, String>(0) == target.dependent_system_ref
                && row.get::<_, String>(1) == target.event.source()
                && row.get::<_, String>(2) == target.event.event_ref()
        })
    }))
}

fn query_optional_row(
    transaction: &mut Transaction<'_>,
    statement: &str,
    params: &[&(dyn postgres::types::ToSql + Sync)],
) -> Result<Option<postgres::Row>, DataRightsPersistenceError> {
    match transaction.query_opt(statement, params) {
        Ok(row) => Ok(row),
        Err(error) => Err(DataRightsPersistenceError::from(error)),
    }
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
