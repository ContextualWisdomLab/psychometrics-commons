//! Creation replay stays idempotent after the durable request lifecycle advances.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::data_rights::{DataRightsRequest, DataRightsRequestKind};
use psychometrics_commons_runtime::integration::IntegrationEvent;
use psychometrics_commons_runtime::postgres_data_rights::{
    apply_data_rights_migration, persist_data_rights_identity_verification,
    persist_requested_data_rights_with_propagation, DataRightsPersistenceDisposition,
    DataRightsPropagationTarget,
};
use psychometrics_commons_runtime::postgres_integration::apply_integration_migration;

const DIGEST: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn ready_client() -> Client {
    let url = std::env::var("TEST_DATABASE_URL").expect("TEST_DATABASE_URL is required");
    let mut client = Client::connect(&url, NoTls).expect("CI PostgreSQL must be reachable");
    let schema = format!("data_rights_creation_replay_{}", std::process::id());
    client
        .batch_execute(&format!(
            "DROP SCHEMA IF EXISTS {schema} CASCADE; CREATE SCHEMA {schema}; SET search_path TO {schema};"
        ))
        .unwrap();
    apply_integration_migration(&mut client).unwrap();
    apply_data_rights_migration(&mut client).unwrap();
    client
}

fn request() -> DataRightsRequest {
    DataRightsRequest::new(
        "data_rights_request_creation_replay",
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
        "data_rights_event_creation_replay",
        "data_rights.deletion.requested",
        "v1",
        "psychometrics_commons",
        "tenant_alpha",
        "data_rights_request_creation_replay",
        10_000,
        "data_rights_request_creation_replay",
        None,
        DIGEST,
    )
    .unwrap()
}

#[test]
fn exact_creation_replay_remains_duplicate_after_identity_verification() {
    let mut client = ready_client();
    let event = event();
    let targets = [DataRightsPropagationTarget::new(
        "dependent_system_alpha",
        &event,
    )];

    assert_eq!(
        persist_requested_data_rights_with_propagation(&mut client, &request(), &targets, 3)
            .unwrap(),
        DataRightsPersistenceDisposition::Inserted
    );

    let mut advanced = request();
    advanced
        .verify_identity("verification_evidence_creation_replay", 10_100)
        .unwrap();
    {
        let mut transaction = client.transaction().unwrap();
        persist_data_rights_identity_verification(&mut transaction, &advanced).unwrap();
        transaction.commit().unwrap();
    }

    assert_eq!(
        persist_requested_data_rights_with_propagation(&mut client, &request(), &targets, 3)
            .unwrap(),
        DataRightsPersistenceDisposition::Duplicate,
        "retrying immutable creation evidence must not conflict merely because the stored lifecycle advanced"
    );
}
