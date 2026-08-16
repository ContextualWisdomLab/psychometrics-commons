//! Durable processing-start evidence for an identity-verified data-rights request.

use postgres::{Client, IsolationLevel, NoTls};
use psychometrics_commons_runtime::data_rights::{
    DataRightsRequest, DataRightsRequestKind, DataRightsState,
};
use psychometrics_commons_runtime::integration::IntegrationEvent;
use psychometrics_commons_runtime::postgres_data_rights::{
    apply_data_rights_migration, persist_data_rights_identity_verification,
    persist_requested_data_rights_with_propagation, DataRightsPersistenceError,
    DataRightsPropagationTarget,
};
use psychometrics_commons_runtime::postgres_data_rights_processing::{
    apply_data_rights_processing_migration, persist_data_rights_processing_start,
    DataRightsProcessingDisposition,
};
use psychometrics_commons_runtime::postgres_integration::apply_integration_migration;

const DIGEST: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn test_client(schema_prefix: &str) -> Client {
    let url = std::env::var("TEST_DATABASE_URL").expect("TEST_DATABASE_URL is required");
    let mut client = Client::connect(&url, NoTls).expect("CI PostgreSQL must be reachable");
    let schema = format!("{schema_prefix}_{}", std::process::id());
    client
        .batch_execute(&format!(
            "CREATE SCHEMA {schema}; SET search_path TO {schema};"
        ))
        .unwrap();
    client
}

fn ready_client(schema_prefix: &str) -> Client {
    let mut client = test_client(schema_prefix);
    apply_integration_migration(&mut client).unwrap();
    apply_data_rights_migration(&mut client).unwrap();
    apply_data_rights_migration(&mut client).unwrap();
    apply_data_rights_processing_migration(&mut client).unwrap();
    apply_data_rights_processing_migration(&mut client).unwrap();
    client
}

fn new_request(request_ref: &str) -> DataRightsRequest {
    DataRightsRequest::new(
        request_ref,
        "tenant_alpha",
        "participant_alpha",
        DataRightsRequestKind::Deletion,
        "scope_alpha",
        10_000,
    )
    .unwrap()
}

fn event(request_ref: &str) -> IntegrationEvent {
    IntegrationEvent::new(
        &format!("event_{request_ref}"),
        "data_rights.deletion.requested",
        "v1",
        "psychometrics_commons",
        "tenant_alpha",
        request_ref,
        10_000,
        request_ref,
        None,
        DIGEST,
    )
    .unwrap()
}

fn persist_requested(client: &mut Client, request_ref: &str) -> DataRightsRequest {
    let request = new_request(request_ref);
    let event = event(request_ref);
    let targets = [DataRightsPropagationTarget::new(
        "dependent_system_alpha",
        &event,
    )];
    persist_requested_data_rights_with_propagation(client, &request, &targets, 3).unwrap();
    request
}

fn persist_verified(client: &mut Client, request_ref: &str) -> DataRightsRequest {
    let mut request = persist_requested(client, request_ref);
    request
        .verify_identity("verification_evidence_alpha", 10_100)
        .unwrap();
    let mut transaction = client.transaction().unwrap();
    persist_data_rights_identity_verification(&mut transaction, &request).unwrap();
    transaction.commit().unwrap();
    request
}

#[test]
fn processing_start_persists_and_replays_exactly() {
    let mut client = ready_client("data_rights_process_replay");
    let mut request = persist_verified(&mut client, "data_rights_request_process");
    request.start_processing("operation_alpha", 10_200).unwrap();
    assert_eq!(request.state(), DataRightsState::Processing);

    let mut transaction = client.transaction().unwrap();
    assert_eq!(
        persist_data_rights_processing_start(&mut transaction, &request).unwrap(),
        DataRightsProcessingDisposition::Started
    );
    assert_eq!(
        persist_data_rights_processing_start(&mut transaction, &request).unwrap(),
        DataRightsProcessingDisposition::Duplicate
    );
    transaction.commit().unwrap();

    let row = client
        .query_one(
            "SELECT current_state, operation_ref, processing_started_at_unix_ms,
                    latest_event_at_unix_ms
             FROM data_rights_request_state WHERE request_ref = $1",
            &[&"data_rights_request_process"],
        )
        .unwrap();
    assert_eq!(row.get::<_, String>(0), "processing");
    assert_eq!(
        row.get::<_, Option<String>>(1).as_deref(),
        Some("operation_alpha")
    );
    assert_eq!(row.get::<_, Option<i64>>(2), Some(10_200));
    assert_eq!(row.get::<_, i64>(3), 10_200);
}

#[test]
fn export_processing_start_persists_request_kind_branch() {
    let mut client = ready_client("data_rights_process_export");
    let request_ref = "data_rights_request_export";
    let mut request = DataRightsRequest::new(
        request_ref,
        "tenant_alpha",
        "participant_alpha",
        DataRightsRequestKind::Export,
        "scope_export",
        11_000,
    )
    .unwrap();
    let event = IntegrationEvent::new(
        "event_data_rights_request_export",
        "data_rights.export.requested",
        "v1",
        "psychometrics_commons",
        "tenant_alpha",
        request_ref,
        11_000,
        request_ref,
        None,
        DIGEST,
    )
    .unwrap();
    let targets = [DataRightsPropagationTarget::new(
        "dependent_system_alpha",
        &event,
    )];
    persist_requested_data_rights_with_propagation(&mut client, &request, &targets, 3).unwrap();

    request
        .verify_identity("verification_evidence_export", 11_100)
        .unwrap();
    let mut transaction = client.transaction().unwrap();
    persist_data_rights_identity_verification(&mut transaction, &request).unwrap();
    transaction.commit().unwrap();

    request
        .start_processing("operation_export", 11_200)
        .unwrap();
    let mut transaction = client.transaction().unwrap();
    assert_eq!(
        persist_data_rights_processing_start(&mut transaction, &request).unwrap(),
        DataRightsProcessingDisposition::Started
    );
    transaction.commit().unwrap();

    let row = client
        .query_one(
            "SELECT request_kind, current_state FROM data_rights_request_state WHERE request_ref = $1",
            &[&request_ref],
        )
        .unwrap();
    assert_eq!(row.get::<_, String>(0), "export");
    assert_eq!(row.get::<_, String>(1), "processing");
}

#[test]
fn processing_start_rejects_each_identity_field_mismatch_independently() {
    let mut client = ready_client("data_rights_process_identity_fields");
    persist_verified(&mut client, "data_rights_request_process");

    let mut mismatched_participant = DataRightsRequest::new(
        "data_rights_request_process",
        "tenant_alpha",
        "participant_beta",
        DataRightsRequestKind::Deletion,
        "scope_alpha",
        10_000,
    )
    .unwrap();
    mismatched_participant
        .verify_identity("verification_evidence_alpha", 10_100)
        .unwrap();
    mismatched_participant
        .start_processing("operation_alpha", 10_200)
        .unwrap();
    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        persist_data_rights_processing_start(&mut transaction, &mismatched_participant),
        Err(DataRightsPersistenceError::ConflictingReplay)
    ));
    transaction.rollback().unwrap();

    let mut mismatched_kind = DataRightsRequest::new(
        "data_rights_request_process",
        "tenant_alpha",
        "participant_alpha",
        DataRightsRequestKind::Export,
        "scope_alpha",
        10_000,
    )
    .unwrap();
    mismatched_kind
        .verify_identity("verification_evidence_alpha", 10_100)
        .unwrap();
    mismatched_kind
        .start_processing("operation_alpha", 10_200)
        .unwrap();
    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        persist_data_rights_processing_start(&mut transaction, &mismatched_kind),
        Err(DataRightsPersistenceError::ConflictingReplay)
    ));
    transaction.rollback().unwrap();

    let mut mismatched_scope = DataRightsRequest::new(
        "data_rights_request_process",
        "tenant_alpha",
        "participant_alpha",
        DataRightsRequestKind::Deletion,
        "scope_beta",
        10_000,
    )
    .unwrap();
    mismatched_scope
        .verify_identity("verification_evidence_alpha", 10_100)
        .unwrap();
    mismatched_scope
        .start_processing("operation_alpha", 10_200)
        .unwrap();
    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        persist_data_rights_processing_start(&mut transaction, &mismatched_scope),
        Err(DataRightsPersistenceError::ConflictingReplay)
    ));
    transaction.rollback().unwrap();
}

#[test]
fn processing_start_rejects_same_operation_with_a_later_start_time() {
    let mut client = ready_client("data_rights_process_start_time");
    let mut request = persist_verified(&mut client, "data_rights_request_process");
    request.start_processing("operation_alpha", 10_200).unwrap();
    {
        let mut transaction = client.transaction().unwrap();
        persist_data_rights_processing_start(&mut transaction, &request).unwrap();
        transaction.commit().unwrap();
    }

    let mut later_start = new_request("data_rights_request_process");
    later_start
        .verify_identity("verification_evidence_alpha", 10_100)
        .unwrap();
    later_start
        .start_processing("operation_alpha", 10_300)
        .unwrap();
    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        persist_data_rights_processing_start(&mut transaction, &later_start),
        Err(DataRightsPersistenceError::ConflictingReplay)
    ));
    transaction.rollback().unwrap();
}

#[test]
fn processing_start_classify_select_failure_is_a_database_failure() {
    let mut client = ready_client("data_rights_process_classify_select");
    let mut request = persist_requested(&mut client, "data_rights_request_process");
    request
        .verify_identity("verification_evidence_alpha", 10_100)
        .unwrap();
    request.start_processing("operation_alpha", 10_200).unwrap();
    let sink = format!(
        "data_rights_processing_classify_sink_{}",
        std::process::id()
    );
    client
        .batch_execute(&format!(
            "DROP SCHEMA IF EXISTS {sink} CASCADE;
             CREATE SCHEMA {sink};
             CREATE OR REPLACE FUNCTION data_rights_processing_redirect_after_update()
             RETURNS trigger LANGUAGE plpgsql AS $$
             BEGIN
                 PERFORM set_config('search_path', '{sink}', false);
                 RETURN NULL;
             END $$;
             CREATE TRIGGER data_rights_processing_redirect_after_update
             AFTER UPDATE ON data_rights_request_state
             FOR EACH STATEMENT
             EXECUTE FUNCTION data_rights_processing_redirect_after_update();"
        ))
        .unwrap();

    let mut transaction = client.transaction().unwrap();
    let error = persist_data_rights_processing_start(&mut transaction, &request)
        .expect_err("classify-select must return the database error");
    transaction.rollback().unwrap();
    assert!(matches!(error, DataRightsPersistenceError::Database(_)));
    assert_eq!(
        error.to_string(),
        "PostgreSQL data-rights persistence operation failed"
    );
    assert!(std::error::Error::source(&error).is_some());
}

#[test]
fn processing_start_rejects_conflicting_and_later_lifecycle_evidence() {
    let mut client = ready_client("data_rights_process_conflict");
    let mut request = persist_verified(&mut client, "data_rights_request_process");
    request.start_processing("operation_alpha", 10_200).unwrap();
    {
        let mut transaction = client.transaction().unwrap();
        persist_data_rights_processing_start(&mut transaction, &request).unwrap();
        transaction.commit().unwrap();
    }

    let mut conflicting_operation = new_request("data_rights_request_process");
    conflicting_operation
        .verify_identity("verification_evidence_alpha", 10_100)
        .unwrap();
    conflicting_operation
        .start_processing("operation_beta", 10_200)
        .unwrap();
    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        persist_data_rights_processing_start(&mut transaction, &conflicting_operation),
        Err(DataRightsPersistenceError::ConflictingReplay)
    ));
    transaction.rollback().unwrap();

    let mut conflicting_verification = new_request("data_rights_request_process");
    conflicting_verification
        .verify_identity("verification_evidence_beta", 10_100)
        .unwrap();
    conflicting_verification
        .start_processing("operation_alpha", 10_200)
        .unwrap();
    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        persist_data_rights_processing_start(&mut transaction, &conflicting_verification),
        Err(DataRightsPersistenceError::ConflictingReplay)
    ));
    transaction.rollback().unwrap();

    client
        .execute(
            "UPDATE data_rights_request_state SET current_state = 'completed'
             WHERE request_ref = $1",
            &[&"data_rights_request_process"],
        )
        .unwrap();
    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        persist_data_rights_processing_start(&mut transaction, &request),
        Err(DataRightsPersistenceError::InvalidRequestState)
    ));
    transaction.rollback().unwrap();
}

#[test]
fn processing_start_rejects_invalid_state_missing_request_and_overflow() {
    let mut client = ready_client("data_rights_process_invalid");
    let verified = persist_verified(&mut client, "data_rights_request_process");
    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        persist_data_rights_processing_start(&mut transaction, &verified),
        Err(DataRightsPersistenceError::InvalidRequestState)
    ));
    transaction.rollback().unwrap();

    let mut missing = new_request("data_rights_request_missing");
    missing
        .verify_identity("verification_evidence_alpha", 10_100)
        .unwrap();
    missing.start_processing("operation_alpha", 10_200).unwrap();
    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        persist_data_rights_processing_start(&mut transaction, &missing),
        Err(DataRightsPersistenceError::RequestNotFound)
    ));
    transaction.rollback().unwrap();

    let mut overflow = verified;
    overflow
        .start_processing("operation_overflow", u64::MAX)
        .unwrap();
    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        persist_data_rights_processing_start(&mut transaction, &overflow),
        Err(DataRightsPersistenceError::ValueOutOfRange)
    ));
    transaction.rollback().unwrap();
}

#[test]
fn processing_start_requires_read_committed_and_surfaces_database_failure() {
    let mut client = ready_client("data_rights_process_isolation");
    let mut request = persist_verified(&mut client, "data_rights_request_process");
    request.start_processing("operation_alpha", 10_200).unwrap();

    let mut transaction = client
        .build_transaction()
        .isolation_level(IsolationLevel::Serializable)
        .start()
        .unwrap();
    assert!(matches!(
        persist_data_rights_processing_start(&mut transaction, &request),
        Err(DataRightsPersistenceError::UnsupportedIsolationLevel)
    ));
    transaction.rollback().unwrap();

    let mut missing_table = test_client("data_rights_process_missing_table");
    let mut transaction = missing_table.transaction().unwrap();
    assert!(matches!(
        persist_data_rights_processing_start(&mut transaction, &request),
        Err(DataRightsPersistenceError::Database(_))
    ));
    transaction.rollback().unwrap();
}

#[test]
fn processing_columns_enforce_pair_presence_reference_format_and_positive_time() {
    let mut client = ready_client("data_rights_process_constraints");
    persist_requested(&mut client, "data_rights_request_process");

    assert!(client
        .execute(
            "UPDATE data_rights_request_state SET operation_ref = 'operation_alpha'
             WHERE request_ref = $1",
            &[&"data_rights_request_process"],
        )
        .is_err());
    assert!(client
        .execute(
            "UPDATE data_rights_request_state
             SET operation_ref = '123', processing_started_at_unix_ms = 10200
             WHERE request_ref = $1",
            &[&"data_rights_request_process"],
        )
        .is_err());
    assert!(client
        .execute(
            "UPDATE data_rights_request_state
             SET operation_ref = 'operation_alpha', processing_started_at_unix_ms = 0
             WHERE request_ref = $1",
            &[&"data_rights_request_process"],
        )
        .is_err());
}

#[test]
fn processing_migration_repairs_constraints_when_columns_preexist() {
    let mut client = test_client("data_rights_process_repair_constraints");
    apply_integration_migration(&mut client).unwrap();
    apply_data_rights_migration(&mut client).unwrap();
    client
        .batch_execute(
            "ALTER TABLE data_rights_request_state ADD COLUMN operation_ref TEXT;
             ALTER TABLE data_rights_request_state
                 ADD COLUMN processing_started_at_unix_ms BIGINT;",
        )
        .unwrap();

    apply_data_rights_processing_migration(&mut client).unwrap();
    apply_data_rights_processing_migration(&mut client).unwrap();

    for constraint_name in [
        "data_rights_operation_ref_format_check",
        "data_rights_processing_started_time_positive_check",
        "data_rights_processing_presence_check",
    ] {
        let exists: bool = client
            .query_one(
                "SELECT EXISTS (
                    SELECT 1
                    FROM pg_constraint AS constraint_record
                    JOIN pg_class AS table_record
                      ON table_record.oid = constraint_record.conrelid
                    JOIN pg_namespace AS schema_record
                      ON schema_record.oid = table_record.relnamespace
                    WHERE constraint_record.conname = $1
                      AND table_record.relname = 'data_rights_request_state'
                      AND schema_record.nspname = current_schema()
                )",
                &[&constraint_name],
            )
            .unwrap()
            .get(0);
        assert!(exists, "missing repaired constraint {constraint_name}");
    }

    persist_requested(&mut client, "data_rights_request_repair");
    assert!(client
        .execute(
            "UPDATE data_rights_request_state
             SET operation_ref = '123', processing_started_at_unix_ms = 10200
             WHERE request_ref = $1",
            &[&"data_rights_request_repair"],
        )
        .is_err());
    assert!(client
        .execute(
            "UPDATE data_rights_request_state
             SET operation_ref = 'operation_alpha', processing_started_at_unix_ms = 0
             WHERE request_ref = $1",
            &[&"data_rights_request_repair"],
        )
        .is_err());
}
