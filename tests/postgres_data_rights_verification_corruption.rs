//! Fail closed when durable identity-verification evidence is internally inconsistent.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::data_rights::{DataRightsRequest, DataRightsRequestKind};
use psychometrics_commons_runtime::integration::IntegrationEvent;
use psychometrics_commons_runtime::postgres_data_rights::{
    apply_data_rights_migration, persist_data_rights_identity_verification,
    persist_requested_data_rights_with_propagation, DataRightsPersistenceError,
    DataRightsPropagationTarget,
};
use psychometrics_commons_runtime::postgres_integration::apply_integration_migration;

const DIGEST: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn ready_client() -> Client {
    let url = std::env::var("TEST_DATABASE_URL").expect("TEST_DATABASE_URL is required");
    let mut client = Client::connect(&url, NoTls).expect("CI PostgreSQL must be reachable");
    let schema = format!("data_rights_verification_corruption_{}", std::process::id());
    client
        .batch_execute(&format!(
            "CREATE SCHEMA {schema}; SET search_path TO {schema};"
        ))
        .unwrap();
    apply_integration_migration(&mut client).unwrap();
    apply_data_rights_migration(&mut client).unwrap();
    client
}

fn requested() -> DataRightsRequest {
    DataRightsRequest::new(
        "data_rights_request_corruption",
        "tenant_alpha",
        "participant_alpha",
        DataRightsRequestKind::Deletion,
        "scope_alpha",
        10_000,
    )
    .unwrap()
}

fn event() -> IntegrationEvent {
    IntegrationEvent::new(
        "data_rights_event_corruption",
        "data_rights.deletion.requested",
        "v1",
        "psychometrics_commons",
        "tenant_alpha",
        "data_rights_request_corruption",
        10_000,
        "data_rights_request_corruption",
        None,
        DIGEST,
    )
    .unwrap()
}

#[test]
fn verification_replay_rejects_one_sided_persisted_verification_evidence() {
    let mut client = ready_client();
    let request = requested();
    let event = event();
    let targets = [DataRightsPropagationTarget::new(
        "dependent_system_alpha",
        &event,
    )];
    persist_requested_data_rights_with_propagation(&mut client, &request, &targets, 3).unwrap();

    // The migration normally prevents one-sided verification evidence. Drop only that isolated
    // test-schema guard to prove the Rust replay boundary still fails closed if stored data is
    // corrupted or imported from a damaged backup.
    client
        .batch_execute(
            "ALTER TABLE data_rights_request_state
             DROP CONSTRAINT data_rights_verification_presence_check;
             UPDATE data_rights_request_state
             SET current_state = 'processing',
                 verification_evidence_ref = NULL,
                 verified_at_unix_ms = 10100,
                 latest_event_at_unix_ms = 10200
             WHERE request_ref = 'data_rights_request_corruption';",
        )
        .unwrap();

    let mut verified = requested();
    verified
        .verify_identity("verification_evidence_corruption", 10_100)
        .unwrap();
    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        persist_data_rights_identity_verification(&mut transaction, &verified),
        Err(DataRightsPersistenceError::ConflictingReplay)
    ));
    transaction.rollback().unwrap();
}
