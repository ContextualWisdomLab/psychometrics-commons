//! `PostgreSQL` persistence for starting an identity-verified data-rights operation.
//!
//! Processing-start evidence is stored only after the exact request-specific identity
//! verification already persisted for the same tenant, participant, request kind, and scope.
//! Replay classification holds a row lock until the caller-owned transaction ends so later
//! lifecycle composition cannot race the state that was classified.

use crate::data_rights::{DataRightsRequest, DataRightsRequestKind, DataRightsState};
use crate::postgres_data_rights::DataRightsPersistenceError;
use crate::reference::normalized_reference;
use postgres::{Client, Transaction};

const DATA_RIGHTS_PROCESSING_MIGRATION: &str =
    include_str!("../migrations/0018_data_rights_processing_start.sql");

/// Outcome of persisting a data-rights processing-start transition.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DataRightsProcessingDisposition {
    /// Processing-start evidence was persisted for the first time.
    Started,
    /// The exact processing-start evidence was already durable.
    Duplicate,
}

/// Apply the idempotent data-rights processing-start migration.
///
/// The base data-rights request table and identity-verification migration must already exist.
///
/// # Errors
///
/// Returns the `PostgreSQL` error if the processing-start columns or constraints cannot be
/// applied.
pub fn apply_data_rights_processing_migration(client: &mut Client) -> Result<(), postgres::Error> {
    client.batch_execute(DATA_RIGHTS_PROCESSING_MIGRATION)
}

/// Persist the start of durable processing for one already identity-verified request.
///
/// The supplied domain request must be in [`DataRightsState::Processing`] and retain the exact
/// request-specific verification evidence that was previously persisted. The operation identity
/// and processing-start time are immutable. Exact replay is idempotent; identity rebinding,
/// verification rebinding, operation rebinding, stale/later lifecycle state, unsupported
/// transaction isolation, and missing request identity all fail closed.
///
/// The caller owns the `READ COMMITTED` transaction and its final commit/rollback decision.
/// Successful first-write and replay classification keep the request row locked until that
/// transaction ends, allowing later processing orchestration to compose against the same state.
///
/// # Errors
///
/// Returns [`DataRightsPersistenceError`] when the domain evidence is incomplete or out of range,
/// transaction isolation is unsupported, the request is missing, immutable evidence conflicts,
/// the durable lifecycle is not eligible for processing start, or `PostgreSQL` fails.
pub fn persist_data_rights_processing_start(
    transaction: &mut Transaction<'_>,
    request: &DataRightsRequest,
) -> Result<DataRightsProcessingDisposition, DataRightsPersistenceError> {
    let (operation_ref, started_at, verification_evidence_ref, verified_at) =
        processing_evidence(request)?;

    require_read_committed(transaction)?;
    let request_kind = request_kind_name(request.kind());
    let updated = query_optional_row(
        transaction,
        "UPDATE data_rights_request_state
         SET current_state = 'processing',
             operation_ref = $3,
             processing_started_at_unix_ms = $4,
             latest_event_at_unix_ms = $4,
             updated_at = clock_timestamp()
         WHERE request_ref = $1
           AND tenant_ref = $2
           AND participant_ref = $5
           AND request_kind = $6
           AND scope_ref = $7
           AND current_state = 'identity_verified'
           AND verification_evidence_ref = $8
           AND verified_at_unix_ms = $9
           AND latest_event_at_unix_ms = $9
         RETURNING request_ref",
        &[
            &request.request_ref(),
            &request.tenant_ref(),
            &operation_ref,
            &started_at,
            &request.participant_ref(),
            &request_kind,
            &request.scope_ref(),
            &verification_evidence_ref,
            &verified_at,
        ],
    )?;
    if updated.is_some() {
        return Ok(DataRightsProcessingDisposition::Started);
    }

    let row = match query_optional_row(
        transaction,
        "SELECT participant_ref, request_kind, scope_ref, current_state,
                verification_evidence_ref, verified_at_unix_ms,
                operation_ref, processing_started_at_unix_ms
         FROM data_rights_request_state
         WHERE request_ref = $1 AND tenant_ref = $2
         FOR UPDATE",
        &[&request.request_ref(), &request.tenant_ref()],
    ) {
        Ok(row) => row,
        Err(error) => return Err(error),
    };
    let Some(row) = row else {
        return Err(DataRightsPersistenceError::RequestNotFound);
    };

    let stored_participant: String = row.get(0);
    let stored_kind: String = row.get(1);
    let stored_scope: String = row.get(2);
    let stored_state: String = row.get(3);
    let stored_verification: Option<String> = row.get(4);
    let stored_verified_at: Option<i64> = row.get(5);
    let stored_operation: Option<String> = row.get(6);
    let stored_started_at: Option<i64> = row.get(7);
    let identity_matches = stored_participant == request.participant_ref()
        && stored_kind == request_kind
        && stored_scope == request.scope_ref()
        && stored_verification.as_deref() == Some(verification_evidence_ref)
        && stored_verified_at == Some(verified_at);

    if !identity_matches {
        Err(DataRightsPersistenceError::ConflictingReplay)
    } else if stored_state == "processing"
        && stored_operation.as_deref() == Some(operation_ref)
        && stored_started_at == Some(started_at)
    {
        Ok(DataRightsProcessingDisposition::Duplicate)
    } else if stored_state == "processing" {
        Err(DataRightsPersistenceError::ConflictingReplay)
    } else {
        Err(DataRightsPersistenceError::InvalidRequestState)
    }
}

fn processing_evidence(
    request: &DataRightsRequest,
) -> Result<(&str, i64, &str, i64), DataRightsPersistenceError> {
    match (
        request.state(),
        request.operation_ref().and_then(normalized_reference),
        request.processing_started_at_unix_ms(),
        request
            .verification_evidence_ref()
            .and_then(normalized_reference),
        request.verified_at_unix_ms(),
    ) {
        (
            DataRightsState::Processing,
            Some(operation_ref),
            Some(started_at_ms),
            Some(verification_evidence_ref),
            Some(verified_at_ms),
        ) => {
            let started_at = i64::try_from(started_at_ms)
                .map_err(|_| DataRightsPersistenceError::ValueOutOfRange)?;
            let verified_at = i64::try_from(verified_at_ms)
                .map_err(|_| DataRightsPersistenceError::ValueOutOfRange)?;
            Ok((
                operation_ref,
                started_at,
                verification_evidence_ref,
                verified_at,
            ))
        }
        _ => Err(DataRightsPersistenceError::InvalidRequestState),
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn optional_row_query_maps_database_errors() {
        let url = std::env::var("TEST_DATABASE_URL").expect("TEST_DATABASE_URL is required");
        let mut client =
            Client::connect(&url, postgres::NoTls).expect("CI PostgreSQL must be reachable");
        let mut transaction = client.transaction().unwrap();

        assert!(matches!(
            query_optional_row(
                &mut transaction,
                "SELECT * FROM data_rights_processing_missing_relation",
                &[],
            ),
            Err(DataRightsPersistenceError::Database(_))
        ));
    }
}
