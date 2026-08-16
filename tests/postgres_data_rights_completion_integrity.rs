//! Database-boundary invariants for data-rights completion evidence.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::data_rights::{DataRightsRequest, DataRightsRequestKind};
use psychometrics_commons_runtime::integration::IntegrationEvent;
use psychometrics_commons_runtime::postgres_data_rights::{
    apply_data_rights_migration, persist_data_rights_identity_verification,
    persist_requested_data_rights_with_propagation, DataRightsPersistenceError,
    DataRightsPropagationTarget,
};
use psychometrics_commons_runtime::postgres_data_rights_completion::{
    apply_data_rights_completion_migration, persist_data_rights_completion,
};
use psychometrics_commons_runtime::postgres_data_rights_processing::{
    apply_data_rights_processing_migration, persist_data_rights_processing_start,
};
use psychometrics_commons_runtime::postgres_integration::apply_integration_migration;

const DIGEST: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn client(schema_prefix: &str) -> Client {
    let url = std::env::var("TEST_DATABASE_URL").expect("TEST_DATABASE_URL is required");
    let mut client = Client::connect(&url, NoTls).expect("CI PostgreSQL must be reachable");
    let schema = format!("{schema_prefix}_{}", std::process::id());
    client
        .batch_execute(&format!(
            "DROP SCHEMA IF EXISTS {schema} CASCADE; CREATE SCHEMA {schema}; SET search_path TO {schema};"
        ))
        .unwrap();
    apply_integration_migration(&mut client).unwrap();
    apply_data_rights_migration(&mut client).unwrap();
    apply_data_rights_processing_migration(&mut client).unwrap();
    apply_data_rights_completion_migration(&mut client).unwrap();
    client
}

fn persist_processing(
    client: &mut Client,
    request_ref: &str,
    kind: DataRightsRequestKind,
) -> DataRightsRequest {
    let mut request = DataRightsRequest::new(
        request_ref,
        "tenant_alpha",
        "participant_alpha",
        kind,
        "scope_alpha",
        10_000,
    )
    .unwrap();
    let event_type = match kind {
        DataRightsRequestKind::Export => "data_rights.export.requested",
        DataRightsRequestKind::Deletion => "data_rights.deletion.requested",
        _ => "data_rights.request.requested",
    };
    let event = IntegrationEvent::new(
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
    .unwrap();
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
fn retained_scope_row_cannot_be_attached_to_completed_export() {
    let mut client = client("data_rights_completion_export_scope");
    let mut request = persist_processing(
        &mut client,
        "data_rights_request_export",
        DataRightsRequestKind::Export,
    );
    request
        .complete("completion_evidence_export", &[], 10_300)
        .unwrap();
    {
        let mut transaction = client.transaction().unwrap();
        persist_data_rights_completion(&mut transaction, &request).unwrap();
        transaction.commit().unwrap();
    }

    assert!(client
        .execute(
            "INSERT INTO data_rights_retained_scope_evidence
                (request_ref, tenant_ref, retained_scope_ref)
             VALUES ($1, $2, 'retention_legal')",
            &[&request.request_ref(), &request.tenant_ref()],
        )
        .is_err());
}

#[test]
fn completion_timestamp_overflow_fails_closed() {
    let mut client = client("data_rights_completion_overflow");
    let mut request = persist_processing(
        &mut client,
        "data_rights_request_overflow",
        DataRightsRequestKind::Deletion,
    );
    request
        .complete("completion_evidence_overflow", &[], u64::MAX)
        .unwrap();
    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        persist_data_rights_completion(&mut transaction, &request),
        Err(DataRightsPersistenceError::ValueOutOfRange)
    ));
    transaction.rollback().unwrap();
}

#[test]
fn later_durable_state_is_not_reclassified_as_completion_replay() {
    let mut client = client("data_rights_completion_stale_state");
    let mut request = persist_processing(
        &mut client,
        "data_rights_request_stale_state",
        DataRightsRequestKind::Deletion,
    );
    request
        .complete("completion_evidence_alpha", &[], 10_300)
        .unwrap();
    client
        .execute(
            "UPDATE data_rights_request_state SET current_state = 'failed'
             WHERE request_ref = $1",
            &[&request.request_ref()],
        )
        .unwrap();

    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        persist_data_rights_completion(&mut transaction, &request),
        Err(DataRightsPersistenceError::InvalidRequestState)
    ));
    transaction.rollback().unwrap();
}

#[test]
fn terminal_states_cannot_exist_without_completion_evidence() {
    let mut client = client("data_rights_completion_terminal_evidence");
    let request = persist_processing(
        &mut client,
        "data_rights_request_terminal_evidence",
        DataRightsRequestKind::Deletion,
    );

    for terminal_state in ["completed", "partially_completed"] {
        assert!(
            client
                .execute(
                    "UPDATE data_rights_request_state SET current_state = $1 WHERE request_ref = $2",
                    &[&terminal_state, &request.request_ref()],
                )
                .is_err(),
            "terminal state {terminal_state} must require durable completion evidence"
        );
    }
}

#[test]
fn completion_evidence_cannot_exist_before_terminal_state() {
    let mut client = client("data_rights_completion_premature_evidence");
    let request = persist_processing(
        &mut client,
        "data_rights_request_premature_evidence",
        DataRightsRequestKind::Deletion,
    );

    assert!(
        client
            .execute(
                "UPDATE data_rights_request_state
                 SET completion_evidence_ref = 'completion_evidence_direct',
                     completed_at_unix_ms = 10300
                 WHERE request_ref = $1",
                &[&request.request_ref()],
            )
            .is_err(),
        "processing rows must not carry terminal completion evidence"
    );
}

#[test]
fn retained_scope_evidence_is_immutable_after_completion() {
    let mut client = client("data_rights_completion_retained_scope_immutable");
    let mut request = persist_processing(
        &mut client,
        "data_rights_request_retained_scope_immutable",
        DataRightsRequestKind::Deletion,
    );
    request
        .complete("completion_evidence_immutable", &["retention_legal"], 10_300)
        .unwrap();
    {
        let mut transaction = client.transaction().unwrap();
        persist_data_rights_completion(&mut transaction, &request).unwrap();
        transaction.commit().unwrap();
    }

    assert!(client
        .execute(
            "UPDATE data_rights_retained_scope_evidence
             SET retained_scope_ref = 'retention_rewritten'
             WHERE request_ref = $1 AND retained_scope_ref = 'retention_legal'",
            &[&request.request_ref()],
        )
        .is_err());
    assert!(client
        .execute(
            "DELETE FROM data_rights_retained_scope_evidence
             WHERE request_ref = $1 AND retained_scope_ref = 'retention_legal'",
            &[&request.request_ref()],
        )
        .is_err());
    assert!(client
        .batch_execute("TRUNCATE TABLE data_rights_retained_scope_evidence")
        .is_err());

    let retained: String = client
        .query_one(
            "SELECT retained_scope_ref FROM data_rights_retained_scope_evidence
             WHERE request_ref = $1",
            &[&request.request_ref()],
        )
        .unwrap()
        .get(0);
    assert_eq!(retained, "retention_legal");
}
