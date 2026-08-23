//! Data-rights persist rejects `PostgreSQL` isolation other than read committed.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::data_rights::{DataRightsRequest, DataRightsRequestKind};
use psychometrics_commons_runtime::integration::IntegrationEvent;
use psychometrics_commons_runtime::postgres_data_rights::{
    apply_data_rights_migration, persist_requested_data_rights_with_propagation,
    DataRightsPersistenceError, DataRightsPropagationTarget,
};
use psychometrics_commons_runtime::postgres_integration::apply_integration_migration;

fn allocate_schema_name(client: &mut Client) -> String {
    let transaction_id: String = client
        .query_one("SELECT pg_current_xact_id()::text", &[])
        .expect("PostgreSQL must allocate a durable transaction identity for the test schema")
        .get(0);
    format!("data_rights_iso_{transaction_id}")
}

struct TestDatabase {
    client: Client,
    schema_name: String,
}

impl TestDatabase {
    fn new() -> Self {
        let url = std::env::var("TEST_DATABASE_URL").unwrap();
        let mut client = Client::connect(&url, NoTls).unwrap();
        let schema_name = allocate_schema_name(&mut client);
        client
            .batch_execute(&format!(
                "CREATE SCHEMA {schema_name}; SET search_path TO {schema_name};"
            ))
            .unwrap();
        apply_integration_migration(&mut client).unwrap();
        apply_data_rights_migration(&mut client).unwrap();
        Self {
            client,
            schema_name,
        }
    }
}

impl Drop for TestDatabase {
    fn drop(&mut self) {
        let _ = self.client.batch_execute(&format!(
            "SET search_path TO public; DROP SCHEMA IF EXISTS {} CASCADE;",
            self.schema_name
        ));
    }
}

#[test]
fn fixture_schema_identity_is_restart_safe() {
    let url = std::env::var("TEST_DATABASE_URL").unwrap();
    let mut client = Client::connect(&url, NoTls).unwrap();
    let first = allocate_schema_name(&mut client);
    let second = allocate_schema_name(&mut client);
    assert_ne!(
        first, second,
        "independent fixture allocations must use database-issued identities"
    );
}

#[test]
fn serializable_session_default_is_rejected() {
    let mut database = TestDatabase::new();
    database
        .client
        .batch_execute("SET default_transaction_isolation TO 'serializable'")
        .unwrap();

    let request = DataRightsRequest::new(
        "data_rights_request_iso",
        "tenant_alpha",
        "participant_alpha",
        DataRightsRequestKind::Deletion,
        "scope_alpha",
        10_000,
    )
    .unwrap();
    let event = IntegrationEvent::new(
        "data_rights_event_iso",
        "data_rights.deletion.requested",
        "v1",
        "psychometrics_commons",
        "tenant_alpha",
        "data_rights_request_iso",
        10_000,
        "data_rights_request_iso",
        None,
        "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    )
    .unwrap();
    let targets = [DataRightsPropagationTarget::new(
        "dependent_system_alpha",
        &event,
    )];

    let error = persist_requested_data_rights_with_propagation(
        &mut database.client,
        &request,
        &targets,
        3,
    )
    .unwrap_err();
    assert!(matches!(
        &error,
        DataRightsPersistenceError::UnsupportedIsolationLevel
    ));
    assert_eq!(
        error.to_string(),
        "data-rights persistence requires read committed isolation"
    );
}
