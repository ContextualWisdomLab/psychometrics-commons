//! PostgreSQL contract for durable participant data-rights propagation.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::data_rights::{DataRightsRequest, DataRightsRequestKind};
use psychometrics_commons_runtime::integration::IntegrationEvent;
use psychometrics_commons_runtime::postgres_data_rights::{
    apply_data_rights_migration, persist_requested_data_rights_with_propagation,
    DataRightsPersistenceDisposition, DataRightsPersistenceError, DataRightsPropagationTarget,
};
use psychometrics_commons_runtime::postgres_integration::apply_integration_migration;

const DIGEST: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn client(schema_prefix: &str) -> Client {
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
    let mut client = client(schema_prefix);
    apply_integration_migration(&mut client).unwrap();
    apply_data_rights_migration(&mut client).unwrap();
    client
}

fn request(scope: &str) -> DataRightsRequest {
    DataRightsRequest::new(
        "data_rights_request_alpha",
        "tenant_alpha",
        "participant_alpha",
        DataRightsRequestKind::Deletion,
        scope,
        10_000,
    )
    .unwrap()
}

fn event() -> IntegrationEvent {
    IntegrationEvent::new(
        "data_rights_event_alpha",
        "data_rights.deletion.requested",
        "v1",
        "psychometrics_commons",
        "tenant_alpha",
        "data_rights_request_alpha",
        10_000,
        "data_rights_request_alpha",
        None,
        DIGEST,
    )
    .unwrap()
}

#[test]
fn request_target_and_outbox_commit_together_and_replay_exactly() {
    let mut db = ready_client("data_rights_exact_replay");
    let event = event();
    let targets = [DataRightsPropagationTarget::new(
        "dependent_system_alpha",
        &event,
    )];

    assert_eq!(
        persist_requested_data_rights_with_propagation(
            &mut db,
            &request("scope_alpha"),
            &targets,
            3,
        )
        .unwrap(),
        DataRightsPersistenceDisposition::Inserted
    );
    assert_eq!(
        persist_requested_data_rights_with_propagation(
            &mut db,
            &request("scope_alpha"),
            &targets,
            3,
        )
        .unwrap(),
        DataRightsPersistenceDisposition::Duplicate
    );

    let counts: (i64, i64, i64) = (
        db.query_one("SELECT count(*) FROM data_rights_request_state", &[])
            .unwrap()
            .get(0),
        db.query_one("SELECT count(*) FROM data_rights_propagation_state", &[])
            .unwrap()
            .get(0),
        db.query_one("SELECT count(*) FROM integration_outbox", &[])
            .unwrap()
            .get(0),
    );
    assert_eq!(counts, (1, 1, 1));
}

#[test]
fn changed_request_evidence_fails_closed() {
    let mut db = ready_client("data_rights_conflicting_replay");
    let event = event();
    let targets = [DataRightsPropagationTarget::new(
        "dependent_system_alpha",
        &event,
    )];

    persist_requested_data_rights_with_propagation(
        &mut db,
        &request("scope_alpha"),
        &targets,
        3,
    )
    .unwrap();

    assert!(matches!(
        persist_requested_data_rights_with_propagation(
            &mut db,
            &request("scope_beta"),
            &targets,
            3,
        ),
        Err(DataRightsPersistenceError::ConflictingReplay)
    ));
}
