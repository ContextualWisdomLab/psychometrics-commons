//! `PostgreSQL` persistence for terminal data-rights completion evidence.
//!
//! Completion is accepted only from the exact durable processing state for the same tenant,
//! participant, request kind, scope, request-specific verification evidence, and operation.
//! Deletion retention exceptions are stored as separate immutable scope evidence so a partial
//! completion cannot be represented as a full deletion. The caller owns the transaction.

use crate::data_rights::{DataRightsRequest, DataRightsRequestKind, DataRightsState};
use crate::postgres_data_rights::DataRightsPersistenceError;
use crate::reference::normalized_reference;
use postgres::{Client, Transaction};

const DATA_RIGHTS_COMPLETION_MIGRATION: &str =
    include_str!("../migrations/0019_data_rights_completion.sql");

/// Outcome of persisting a terminal data-rights completion transition.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DataRightsCompletionDisposition {
    /// Completion evidence and any retained-scope evidence were persisted for the first time.
    Completed,
    /// The exact completion evidence was already durable.
    Duplicate,
}

/// Apply the idempotent data-rights completion migration.
///
/// The base request, identity-verification, and processing-start migrations must already exist.
///
/// # Errors
///
/// Returns the `PostgreSQL` error if completion columns, constraints, or retained-scope storage
/// cannot be applied.
pub fn apply_data_rights_completion_migration(
    client: &mut Client,
) -> Result<(), postgres::Error> {
    client.batch_execute(DATA_RIGHTS_COMPLETION_MIGRATION)
}

/// Persist one completed or partially completed data-rights request.
///
/// Exact replay is idempotent. Rebinding any request identity, verification evidence, operation,
/// completion evidence, completion time, or retained deletion scope fails closed. A deletion with
/// retained scopes must remain `partially_completed`; export requests cannot carry retained scopes.
///
/// The caller owns the `READ COMMITTED` transaction and its commit/rollback decision. Successful
/// persistence and replay classification retain a row lock until the transaction ends.
///
/// # Errors
///
/// Returns [`DataRightsPersistenceError`] for incomplete domain evidence, unsupported isolation,
/// missing durable request identity, conflicting immutable evidence, an ineligible durable state,
/// out-of-range timestamps, or a `PostgreSQL` failure.
pub fn persist_data_rights_completion(
    transaction: &mut Transaction<'_>,
    request: &DataRightsRequest,
) -> Result<DataRightsCompletionDisposition, DataRightsPersistenceError> {
    let (target_state, completion_ref, completed_at_ms) = match (
        request.state(),
        request.completion_evidence_ref().and_then(normalized_reference),
        request.completed_at_unix_ms(),
        request.retained_scope_refs().is_empty(),
    ) {
        (DataRightsState::Completed, Some(completion_ref), Some(completed_at), true) => {
            ("completed", completion_ref, completed_at)
        }
        (
            DataRightsState::PartiallyCompleted,
            Some(completion_ref),
            Some(completed_at),
            false,
        ) if request.kind() == DataRightsRequestKind::Deletion => {
            ("partially_completed", completion_ref, completed_at)
        }
        _ => return Err(DataRightsPersistenceError::InvalidRequestState),
    };

    let verification_ref = request
        .verification_evidence_ref()
        .and_then(normalized_reference)
        .ok_or(DataRightsPersistenceError::InvalidRequestState)?;
    let operation_ref = request
        .operation_ref()
        .and_then(normalized_reference)
        .ok_or(DataRightsPersistenceError::InvalidRequestState)?;
    let verified_at_ms = request
        .verified_at_unix_ms()
        .ok_or(DataRightsPersistenceError::InvalidRequestState)?;
    let processing_started_at_ms = request
        .processing_started_at_unix_ms()
        .ok_or(DataRightsPersistenceError::InvalidRequestState)?;
    let completed_at = i64::try_from(completed_at_ms)
        .map_err(|_| DataRightsPersistenceError::ValueOutOfRange)?;
    if processing_started_at_ms > completed_at_ms || verified_at_ms > processing_started_at_ms {
        return Err(DataRightsPersistenceError::InvalidRequestState);
    }
    let verified_at = verified_at_ms as i64;
    let processing_started_at = processing_started_at_ms as i64;

    let retained_scopes = request
        .retained_scope_refs()
        .iter()
        .map(|reference| {
            normalized_reference(reference).ok_or(DataRightsPersistenceError::InvalidReference)
        })
        .collect::<Result<Vec<_>, _>>()?;
    if request.kind() == DataRightsRequestKind::Export && !retained_scopes.is_empty() {
        return Err(DataRightsPersistenceError::InvalidRequestState);
    }

    require_read_committed(transaction)?;
    let request_kind = request_kind_name(request.kind());
    let updated = transaction.query_opt(
        "UPDATE data_rights_request_state
         SET current_state = $3,
             completion_evidence_ref = $4,
             completed_at_unix_ms = $5,
             latest_event_at_unix_ms = $5,
             updated_at = clock_timestamp()
         WHERE request_ref = $1
           AND tenant_ref = $2
           AND participant_ref = $6
           AND request_kind = $7
           AND scope_ref = $8
           AND current_state = 'processing'
           AND verification_evidence_ref = $9
           AND verified_at_unix_ms = $10
           AND operation_ref = $11
           AND processing_started_at_unix_ms = $12
           AND latest_event_at_unix_ms = $12
         RETURNING request_ref",
        &[
            &request.request_ref(),
            &request.tenant_ref(),
            &target_state,
            &completion_ref,
            &completed_at,
            &request.participant_ref(),
            &request_kind,
            &request.scope_ref(),
            &verification_ref,
            &verified_at,
            &operation_ref,
            &processing_started_at,
        ],
    )?;

    if updated.is_some() {
        for retained_scope in &retained_scopes {
            transaction.execute(
                "INSERT INTO data_rights_retained_scope_evidence
                    (request_ref, tenant_ref, retained_scope_ref)
                 VALUES ($1, $2, $3)",
                &[&request.request_ref(), &request.tenant_ref(), retained_scope],
            )?;
        }
        return Ok(DataRightsCompletionDisposition::Completed);
    }

    classify_replay(
        transaction,
        request,
        request_kind,
        target_state,
        completion_ref,
        completed_at,
        verification_ref,
        verified_at,
        operation_ref,
        processing_started_at,
        &retained_scopes,
    )
}

#[allow(clippy::too_many_arguments)]
fn classify_replay(
    transaction: &mut Transaction<'_>,
    request: &DataRightsRequest,
    request_kind: &str,
    target_state: &str,
    completion_ref: &str,
    completed_at: i64,
    verification_ref: &str,
    verified_at: i64,
    operation_ref: &str,
    processing_started_at: i64,
    retained_scopes: &[&str],
) -> Result<DataRightsCompletionDisposition, DataRightsPersistenceError> {
    let row = transaction.query_opt(
        "SELECT participant_ref, request_kind, scope_ref, current_state,
                verification_evidence_ref, verified_at_unix_ms,
                operation_ref, processing_started_at_unix_ms,
                completion_evidence_ref, completed_at_unix_ms
         FROM data_rights_request_state
         WHERE request_ref = $1 AND tenant_ref = $2
         FOR UPDATE",
        &[&request.request_ref(), &request.tenant_ref()],
    )?;
    let Some(row) = row else {
        return Err(DataRightsPersistenceError::RequestNotFound);
    };

    let identity_matches = row.get::<_, String>(0) == request.participant_ref()
        && row.get::<_, String>(1) == request_kind
        && row.get::<_, String>(2) == request.scope_ref()
        && row.get::<_, Option<String>>(4).as_deref() == Some(verification_ref)
        && row.get::<_, Option<i64>>(5) == Some(verified_at)
        && row.get::<_, Option<String>>(6).as_deref() == Some(operation_ref)
        && row.get::<_, Option<i64>>(7) == Some(processing_started_at);
    if !identity_matches {
        return Err(DataRightsPersistenceError::ConflictingReplay);
    }

    let stored_state: String = row.get(3);
    if stored_state != target_state {
        return if matches!(stored_state.as_str(), "completed" | "partially_completed") {
            Err(DataRightsPersistenceError::ConflictingReplay)
        } else {
            Err(DataRightsPersistenceError::InvalidRequestState)
        };
    }
    if row.get::<_, Option<String>>(8).as_deref() != Some(completion_ref)
        || row.get::<_, Option<i64>>(9) != Some(completed_at)
    {
        return Err(DataRightsPersistenceError::ConflictingReplay);
    }

    let stored_retained = transaction
        .query(
            "SELECT retained_scope_ref
             FROM data_rights_retained_scope_evidence
             WHERE request_ref = $1 AND tenant_ref = $2
             ORDER BY retained_scope_ref",
            &[&request.request_ref(), &request.tenant_ref()],
        )?
        .into_iter()
        .map(|row| row.get::<_, String>(0))
        .collect::<Vec<_>>();
    let expected_retained = retained_scopes
        .iter()
        .map(|reference| (*reference).to_owned())
        .collect::<Vec<_>>();
    if stored_retained != expected_retained {
        return Err(DataRightsPersistenceError::ConflictingReplay);
    }

    Ok(DataRightsCompletionDisposition::Duplicate)
}

fn request_kind_name(kind: DataRightsRequestKind) -> &'static str {
    match kind {
        DataRightsRequestKind::Export => "export",
        DataRightsRequestKind::Deletion => "deletion",
    }
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
