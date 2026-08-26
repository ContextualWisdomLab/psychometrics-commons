//! Exact-spelling persistence contract for data-rights propagation targets.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::data_rights::{DataRightsRequest, DataRightsRequestKind};
use psychometrics_commons_runtime::integration::IntegrationEvent;
use psychometrics_commons_runtime::postgres_data_rights::{
    apply_data_rights_migration, persist_requested_data_rights_with_propagation,
    DataRightsPersistenceError, DataRightsPropagationTarget,
};
use psychometrics_commons_runtime::postgres_integration::apply_integration_migration;

const DIGEST: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn ready_client() -> Client {
    let url = std::env::var("TEST_DATABASE_URL").expect("TEST_DATABASE_URL is required");
    let mut client = Client::connect(&url, NoTls).expect("CI PostgreSQL must be reachable");
    let schema = format!("data_rights_exact_target_{}", std::process::id());
    client
        .batch_execute(&format!(
            "CREATE SCHEMA {schema}; SET search_path TO {schema};"
        ))
        .unwrap();
    apply_integration_migration(&mut client).unwrap();
    apply_data_rights_migration(&mut client).unwrap();
    client
}

#[test]
fn padded_dependent_system_reference_is_rejected_before_persistence() {
    let mut client = ready_client();
    let request = DataRightsRequest::new(
        "data_rights_request_exact_target",
        "tenant_alpha",
        "participant_alpha",
        DataRightsRequestKind::Deletion,
        "scope_alpha",
        10_000,
    )
    .unwrap();
    let event = IntegrationEvent::new(
        "data_rights_event_exact_target",
        "data_rights.deletion.requested",
        "v1",
        "psychometrics_commons",
        "tenant_alpha",
        "data_rights_request_exact_target",
        10_000,
        "data_rights_request_exact_target",
        None,
        DIGEST,
    )
    .unwrap();
    let targets = [DataRightsPropagationTarget::new(
        " dependent_system_alpha ",
        &event,
    )];

    assert!(matches!(
        persist_requested_data_rights_with_propagation(&mut client, &request, &targets, 3),
        Err(DataRightsPersistenceError::InvalidReference)
    ));

    let request_rows: i64 = client
        .query_one("SELECT COUNT(*) FROM data_rights_request_state", &[])
        .unwrap()
        .get(0);
    let propagation_rows: i64 = client
        .query_one("SELECT COUNT(*) FROM data_rights_propagation_state", &[])
        .unwrap()
        .get(0);
    assert_eq!(request_rows, 0);
    assert_eq!(propagation_rows, 0);
}
