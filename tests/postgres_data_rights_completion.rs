//! Durable completion evidence for processed data-rights requests.

use postgres::{Client, IsolationLevel, NoTls};
use psychometrics_commons_runtime::data_rights::{DataRightsRequest, DataRightsRequestKind};
use psychometrics_commons_runtime::integration::IntegrationEvent;
use psychometrics_commons_runtime::postgres_data_rights::{
    apply_data_rights_migration, persist_data_rights_identity_verification,
    persist_requested_data_rights_with_propagation, DataRightsPersistenceError,
    DataRightsPropagationTarget,
};
use psychometrics_commons_runtime::postgres_data_rights_completion::{
    apply_data_rights_completion_migration, persist_data_rights_completion,
    DataRightsCompletionDisposition,
};
use psychometrics_commons_runtime::postgres_data_rights_processing::{
    apply_data_rights_processing_migration, persist_data_rights_processing_start,
};
use psychometrics_commons_runtime::postgres_integration::apply_integration_migration;

const DIGEST: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn test_client(schema_prefix: &str) -> Client {
    let url = std::env::var("TEST_DATABASE_URL").expect("TEST_DATABASE_URL is required");
    let mut client = Client::connect(&url, NoTls).expect("CI PostgreSQL must be reachable");
    let schema = format!("{schema_prefix}_{}", std::process::id());
    client
        .batch_execute(&format!(
            "DROP SCHEMA IF EXISTS {schema} CASCADE; CREATE SCHEMA {schema}; SET search_path TO {schema};"
        ))
        .unwrap();
    client
}

fn ready_client(schema_prefix: &str) -> Client {
    let mut client = test_client(schema_prefix);
    apply_integration_migration(&mut client).unwrap();
    apply_data_rights_migration(&mut client).unwrap();
    apply_data_rights_processing_migration(&mut client).unwrap();
    apply_data_rights_completion_migration(&mut client).unwrap();
    apply_data_rights_completion_migration(&mut client).unwrap();
    client
}

fn new_request(request_ref: &str, kind: DataRightsRequestKind) -> DataRightsRequest {
    DataRightsRequest::new(
        request_ref,
        "tenant_alpha",
        "participant_alpha",
        kind,
        "scope_alpha",
        10_000,
    )
    .unwrap()
}

fn event(request_ref: &str, kind: DataRightsRequestKind) -> IntegrationEvent {
    let event_type = match kind {
        DataRightsRequestKind::Export => "data_rights.export.requested",
        DataRightsRequestKind::Deletion => "data_rights.deletion.requested",
        _ => "data_rights.request.requested",
    };
    IntegrationEvent::new(
        &format!("event_{request_ref}"),
        event_type,
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

fn persist_processing(
    client: &mut Client,
    request_ref: &str,
    kind: DataRightsRequestKind,
) -> DataRightsRequest {
    let mut request = new_request(request_ref, kind);
    let event = event(request_ref, kind);
    let targets = [DataRightsPropagationTarget::new(
        "dependent_system_alpha",
        &event,
    )];
    persist_requested_data_rights_with_propagation(client, &request, &targets, 3).unwrap();
    request
        .verify_identity("verification_evidence_alpha", 10_100)
        .unwrap();
    {
        let mut transaction = client.transaction().unwrap();
        persist_data_rights_identity_verification(&mut transaction, &request).unwrap();
        transaction.commit().unwrap();
    }
    request.start_processing("operation_alpha", 10_200).unwrap();
    {
        let mut transaction = client.transaction().unwrap();
        persist_data_rights_processing_start(&mut transaction, &request).unwrap();
        transaction.commit().unwrap();
    }
    request
}

#[test]
fn deletion_completion_persists_retention_evidence_and_replays_exactly() {
    let mut client = ready_client("data_rights_completion_deletion");
    let mut request = persist_processing(
        &mut client,
        "data_rights_request_completion",
        DataRightsRequestKind::Deletion,
    );
    request
        .complete(
            "completion_evidence_alpha",
            &["retention_legal", "retention_audit"],
            10_300,
        )
        .unwrap();

    let mut transaction = client.transaction().unwrap();
    assert_eq!(
        persist_data_rights_completion(&mut transaction, &request).unwrap(),
        DataRightsCompletionDisposition::Completed
    );
    assert_eq!(
        persist_data_rights_completion(&mut transaction, &request).unwrap(),
        DataRightsCompletionDisposition::Duplicate
    );
    transaction.commit().unwrap();

    let row = client
        .query_one(
            "SELECT current_state, completion_evidence_ref, completed_at_unix_ms,
                    latest_event_at_unix_ms
             FROM data_rights_request_state WHERE request_ref = $1",
            &[&"data_rights_request_completion"],
        )
        .unwrap();
    assert_eq!(row.get::<_, String>(0), "partially_completed");
    assert_eq!(
        row.get::<_, Option<String>>(1).as_deref(),
        Some("completion_evidence_alpha")
    );
    assert_eq!(row.get::<_, Option<i64>>(2), Some(10_300));
    assert_eq!(row.get::<_, i64>(3), 10_300);

    let retained = client
        .query(
            "SELECT retained_scope_ref FROM data_rights_retained_scope_evidence
             WHERE request_ref = $1 ORDER BY retained_scope_ref",
            &[&"data_rights_request_completion"],
        )
        .unwrap()
        .into_iter()
        .map(|row| row.get::<_, String>(0))
        .collect::<Vec<_>>();
    assert_eq!(retained, vec!["retention_audit", "retention_legal"]);
}

#[test]
fn export_completion_persists_without_retention_rows() {
    let mut client = ready_client("data_rights_completion_export");
    let mut request = persist_processing(
        &mut client,
        "data_rights_request_export",
        DataRightsRequestKind::Export,
    );
    request
        .complete("completion_evidence_export", &[], 10_300)
        .unwrap();

    let mut transaction = client.transaction().unwrap();
    assert_eq!(
        persist_data_rights_completion(&mut transaction, &request).unwrap(),
        DataRightsCompletionDisposition::Completed
    );
    transaction.commit().unwrap();

    let state: String = client
        .query_one(
            "SELECT current_state FROM data_rights_request_state WHERE request_ref = $1",
            &[&"data_rights_request_export"],
        )
        .unwrap()
        .get(0);
    assert_eq!(state, "completed");
    let retained_count: i64 = client
        .query_one(
            "SELECT COUNT(*) FROM data_rights_retained_scope_evidence WHERE request_ref = $1",
            &[&"data_rights_request_export"],
        )
        .unwrap()
        .get(0);
    assert_eq!(retained_count, 0);
}

#[test]
fn completion_rejects_identity_operation_and_retention_rebinding() {
    let mut client = ready_client("data_rights_completion_conflict");
    let mut request = persist_processing(
        &mut client,
        "data_rights_request_completion",
        DataRightsRequestKind::Deletion,
    );
    request
        .complete("completion_evidence_alpha", &["retention_legal"], 10_300)
        .unwrap();
    {
        let mut transaction = client.transaction().unwrap();
        persist_data_rights_completion(&mut transaction, &request).unwrap();
        transaction.commit().unwrap();
    }

    let mut conflicting = new_request(
        "data_rights_request_completion",
        DataRightsRequestKind::Deletion,
    );
    conflicting
        .verify_identity("verification_evidence_alpha", 10_100)
        .unwrap();
    conflicting
        .start_processing("operation_alpha", 10_200)
        .unwrap();
    conflicting
        .complete("completion_evidence_alpha", &["retention_audit"], 10_300)
        .unwrap();
    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        persist_data_rights_completion(&mut transaction, &conflicting),
        Err(DataRightsPersistenceError::ConflictingReplay)
    ));
    transaction.rollback().unwrap();

    let mut conflicting_operation = new_request(
        "data_rights_request_completion",
        DataRightsRequestKind::Deletion,
    );
    conflicting_operation
        .verify_identity("verification_evidence_alpha", 10_100)
        .unwrap();
    conflicting_operation
        .start_processing("operation_beta", 10_200)
        .unwrap();
    conflicting_operation
        .complete("completion_evidence_alpha", &["retention_legal"], 10_300)
        .unwrap();
    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        persist_data_rights_completion(&mut transaction, &conflicting_operation),
        Err(DataRightsPersistenceError::ConflictingReplay)
    ));
    transaction.rollback().unwrap();
}

#[test]
fn completion_requires_processing_state_existing_request_and_read_committed() {
    let mut client = ready_client("data_rights_completion_invalid");
    let mut processing = persist_processing(
        &mut client,
        "data_rights_request_completion",
        DataRightsRequestKind::Deletion,
    );
    processing
        .complete("completion_evidence_alpha", &[], 10_300)
        .unwrap();

    let mut transaction = client
        .build_transaction()
        .isolation_level(IsolationLevel::Serializable)
        .start()
        .unwrap();
    assert!(matches!(
        persist_data_rights_completion(&mut transaction, &processing),
        Err(DataRightsPersistenceError::UnsupportedIsolationLevel)
    ));
    transaction.rollback().unwrap();

    let mut missing = new_request(
        "data_rights_request_missing",
        DataRightsRequestKind::Deletion,
    );
    missing
        .verify_identity("verification_evidence_alpha", 10_100)
        .unwrap();
    missing.start_processing("operation_alpha", 10_200).unwrap();
    missing
        .complete("completion_evidence_alpha", &[], 10_300)
        .unwrap();
    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        persist_data_rights_completion(&mut transaction, &missing),
        Err(DataRightsPersistenceError::RequestNotFound)
    ));
    transaction.rollback().unwrap();

    let mut incomplete = new_request(
        "data_rights_request_incomplete",
        DataRightsRequestKind::Deletion,
    );
    incomplete
        .verify_identity("verification_evidence_alpha", 10_100)
        .unwrap();
    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        persist_data_rights_completion(&mut transaction, &incomplete),
        Err(DataRightsPersistenceError::InvalidRequestState)
    ));
    transaction.rollback().unwrap();
}

#[test]
fn completion_classify_select_failure_is_a_database_failure() {
    let mut client = ready_client("data_rights_completion_classify_select");
    let mut request = new_request(
        "data_rights_request_completion",
        DataRightsRequestKind::Deletion,
    );
    let event = event(
        "data_rights_request_completion",
        DataRightsRequestKind::Deletion,
    );
    let targets = [DataRightsPropagationTarget::new(
        "dependent_system_alpha",
        &event,
    )];
    persist_requested_data_rights_with_propagation(&mut client, &request, &targets, 3).unwrap();
    request
        .verify_identity("verification_evidence_alpha", 10_100)
        .unwrap();
    request.start_processing("operation_alpha", 10_200).unwrap();
    request
        .complete("completion_evidence_alpha", &[], 10_300)
        .unwrap();
    let sink = format!(
        "data_rights_completion_classify_sink_{}",
        std::process::id()
    );
    client
        .batch_execute(&format!(
            "DROP SCHEMA IF EXISTS {sink} CASCADE;
             CREATE SCHEMA {sink};
             CREATE OR REPLACE FUNCTION data_rights_completion_redirect_after_update()
             RETURNS trigger LANGUAGE plpgsql AS $$
             BEGIN
                 PERFORM set_config('search_path', '{sink}', false);
                 RETURN NULL;
             END $$;
             CREATE TRIGGER data_rights_completion_redirect_after_update
             AFTER UPDATE ON data_rights_request_state
             FOR EACH STATEMENT
             EXECUTE FUNCTION data_rights_completion_redirect_after_update();"
        ))
        .unwrap();

    let mut transaction = client.transaction().unwrap();
    let error = persist_data_rights_completion(&mut transaction, &request)
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
fn completion_update_failure_is_a_database_failure() {
    let mut client = ready_client("data_rights_completion_update");
    let mut request = persist_processing(
        &mut client,
        "data_rights_request_completion",
        DataRightsRequestKind::Deletion,
    );
    request
        .complete("completion_evidence_alpha", &["retention_legal"], 10_300)
        .unwrap();
    let sink = format!("data_rights_completion_update_sink_{}", std::process::id());
    client
        .batch_execute(&format!(
            "DROP SCHEMA IF EXISTS {sink} CASCADE; CREATE SCHEMA {sink};"
        ))
        .unwrap();

    let mut transaction = client.transaction().unwrap();
    transaction
        .batch_execute(&format!("SET LOCAL search_path TO {sink}"))
        .unwrap();
    let error = persist_data_rights_completion(&mut transaction, &request)
        .expect_err("completion update must return the database error");
    transaction.rollback().unwrap();
    assert!(matches!(error, DataRightsPersistenceError::Database(_)));
    assert_eq!(
        error.to_string(),
        "PostgreSQL data-rights persistence operation failed"
    );
    assert!(std::error::Error::source(&error).is_some());
}

#[test]
fn completion_retained_scope_insert_failure_is_a_database_failure() {
    let mut client = ready_client("data_rights_completion_retain_insert");
    let mut request = persist_processing(
        &mut client,
        "data_rights_request_completion",
        DataRightsRequestKind::Deletion,
    );
    request
        .complete("completion_evidence_alpha", &["retention_legal"], 10_300)
        .unwrap();
    let sink = format!(
        "data_rights_completion_retain_insert_sink_{}",
        std::process::id()
    );
    client
        .batch_execute(&format!(
            "DROP SCHEMA IF EXISTS {sink} CASCADE;
             CREATE SCHEMA {sink};
             CREATE OR REPLACE FUNCTION data_rights_completion_redirect_after_update()
             RETURNS trigger LANGUAGE plpgsql AS $$
             BEGIN
                 PERFORM set_config('search_path', '{sink}', false);
                 RETURN NULL;
             END $$;
             CREATE TRIGGER data_rights_completion_redirect_after_update
             AFTER UPDATE ON data_rights_request_state
             FOR EACH STATEMENT
             EXECUTE FUNCTION data_rights_completion_redirect_after_update();"
        ))
        .unwrap();

    let mut transaction = client.transaction().unwrap();
    let error = persist_data_rights_completion(&mut transaction, &request)
        .expect_err("retained-scope insert must return the database error");
    transaction.rollback().unwrap();
    assert!(matches!(error, DataRightsPersistenceError::Database(_)));
    assert_eq!(
        error.to_string(),
        "PostgreSQL data-rights persistence operation failed"
    );
    assert!(std::error::Error::source(&error).is_some());
}

#[test]
fn completion_replay_rejects_terminal_state_and_completion_evidence_rebinding() {
    let mut client = ready_client("data_rights_completion_terminal_replay");
    let mut request = persist_processing(
        &mut client,
        "data_rights_request_completion",
        DataRightsRequestKind::Deletion,
    );
    request
        .complete("completion_evidence_alpha", &["retention_legal"], 10_300)
        .unwrap();
    {
        let mut transaction = client.transaction().unwrap();
        persist_data_rights_completion(&mut transaction, &request).unwrap();
        transaction.commit().unwrap();
    }

    let mut completed_without_retention = new_request(
        "data_rights_request_completion",
        DataRightsRequestKind::Deletion,
    );
    completed_without_retention
        .verify_identity("verification_evidence_alpha", 10_100)
        .unwrap();
    completed_without_retention
        .start_processing("operation_alpha", 10_200)
        .unwrap();
    completed_without_retention
        .complete("completion_evidence_alpha", &[], 10_300)
        .unwrap();
    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        persist_data_rights_completion(&mut transaction, &completed_without_retention),
        Err(DataRightsPersistenceError::ConflictingReplay)
    ));
    transaction.rollback().unwrap();

    let mut rebound_completion = new_request(
        "data_rights_request_completion",
        DataRightsRequestKind::Deletion,
    );
    rebound_completion
        .verify_identity("verification_evidence_alpha", 10_100)
        .unwrap();
    rebound_completion
        .start_processing("operation_alpha", 10_200)
        .unwrap();
    rebound_completion
        .complete("completion_evidence_beta", &["retention_legal"], 10_300)
        .unwrap();
    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        persist_data_rights_completion(&mut transaction, &rebound_completion),
        Err(DataRightsPersistenceError::ConflictingReplay)
    ));
    transaction.rollback().unwrap();
}

#[test]
fn completion_retained_scope_classify_select_failure_is_a_database_failure() {
    let mut client = ready_client("data_rights_completion_retain_select");
    let mut request = persist_processing(
        &mut client,
        "data_rights_request_completion",
        DataRightsRequestKind::Deletion,
    );
    request
        .complete("completion_evidence_alpha", &["retention_legal"], 10_300)
        .unwrap();
    {
        let mut transaction = client.transaction().unwrap();
        persist_data_rights_completion(&mut transaction, &request).unwrap();
        transaction.commit().unwrap();
    }
    client
        .batch_execute(
            "CREATE OR REPLACE FUNCTION data_rights_completion_drop_retained_after_update()
             RETURNS trigger LANGUAGE plpgsql AS $$
             BEGIN
                 DROP TABLE data_rights_retained_scope_evidence;
                 RETURN NULL;
             END $$;
             CREATE TRIGGER data_rights_completion_drop_retained_after_update
             AFTER UPDATE ON data_rights_request_state
             FOR EACH STATEMENT
             EXECUTE FUNCTION data_rights_completion_drop_retained_after_update();",
        )
        .unwrap();

    let mut transaction = client.transaction().unwrap();
    let error = persist_data_rights_completion(&mut transaction, &request)
        .expect_err("retained-scope classify-select must return the database error");
    transaction.rollback().unwrap();
    assert!(matches!(error, DataRightsPersistenceError::Database(_)));
    assert_eq!(
        error.to_string(),
        "PostgreSQL data-rights persistence operation failed"
    );
    assert!(std::error::Error::source(&error).is_some());
}

#[test]
fn completion_schema_rejects_invalid_evidence_and_retention_scope() {
    let mut client = ready_client("data_rights_completion_constraints");
    let request = persist_processing(
        &mut client,
        "data_rights_request_completion",
        DataRightsRequestKind::Deletion,
    );
    assert!(client
        .execute(
            "UPDATE data_rights_request_state
             SET completion_evidence_ref = '123', completed_at_unix_ms = 10300
             WHERE request_ref = $1",
            &[&request.request_ref()],
        )
        .is_err());
    assert!(client
        .execute(
            "UPDATE data_rights_request_state
             SET completion_evidence_ref = 'completion_alpha', completed_at_unix_ms = 0
             WHERE request_ref = $1",
            &[&request.request_ref()],
        )
        .is_err());
    assert!(client
        .execute(
            "INSERT INTO data_rights_retained_scope_evidence
                (request_ref, tenant_ref, retained_scope_ref)
             VALUES ($1, $2, '123')",
            &[&request.request_ref(), &request.tenant_ref()],
        )
        .is_err());
}
