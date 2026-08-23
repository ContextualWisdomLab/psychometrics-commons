//! Concurrent data-rights persist replay is exactly one insert plus duplicates.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::data_rights::{DataRightsRequest, DataRightsRequestKind};
use psychometrics_commons_runtime::integration::IntegrationEvent;
use psychometrics_commons_runtime::postgres_data_rights::{
    apply_data_rights_migration, persist_requested_data_rights_with_propagation,
    DataRightsPersistenceDisposition, DataRightsPropagationTarget,
};
use psychometrics_commons_runtime::postgres_integration::apply_integration_migration;
use std::sync::{Arc, Barrier};
use std::thread;

const DIGEST: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn allocate_schema_name(client: &mut Client) -> String {
    let transaction_id: String = client
        .query_one("SELECT pg_current_xact_id()::text", &[])
        .expect("PostgreSQL must allocate a durable transaction identity for the test schema")
        .get(0);
    format!("data_rights_concurrent_{transaction_id}")
}

struct TestDatabase {
    client: Client,
    schema_name: String,
}

impl TestDatabase {
    fn new(url: &str) -> Self {
        let mut client = Client::connect(url, NoTls).unwrap();
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

fn run_worker(url: &str, schema: &str, barrier: &Arc<Barrier>) -> DataRightsPersistenceDisposition {
    let mut db = Client::connect(url, NoTls).unwrap();
    db.batch_execute(&format!("SET search_path TO {schema}"))
        .unwrap();
    let request = DataRightsRequest::new(
        "data_rights_request_concurrent",
        "tenant_alpha",
        "participant_alpha",
        DataRightsRequestKind::Deletion,
        "scope_alpha",
        10_000,
    )
    .unwrap();
    let event = IntegrationEvent::new(
        "data_rights_event_concurrent",
        "data_rights.deletion.requested",
        "v1",
        "psychometrics_commons",
        "tenant_alpha",
        "data_rights_request_concurrent",
        10_000,
        "data_rights_request_concurrent",
        None,
        DIGEST,
    )
    .unwrap();
    let targets = [DataRightsPropagationTarget::new(
        "dependent_system_alpha",
        &event,
    )];
    barrier.wait();
    persist_requested_data_rights_with_propagation(&mut db, &request, &targets, 3).unwrap()
}

#[test]
fn fixture_schema_identity_is_restart_safe() {
    let url = std::env::var("TEST_DATABASE_URL").unwrap();
    let mut client = Client::connect(&url, NoTls).unwrap();
    let first = allocate_schema_name(&mut client);
    let second = allocate_schema_name(&mut client);
    assert_ne!(
        first, second,
        "independent concurrent fixtures must use database-issued identities"
    );
}

#[test]
fn concurrent_exact_first_write_is_idempotent() {
    let url = std::env::var("TEST_DATABASE_URL").unwrap();
    let setup = TestDatabase::new(&url);
    let schema = setup.schema_name.clone();

    let barrier = Arc::new(Barrier::new(3));
    let first = {
        let barrier = Arc::clone(&barrier);
        let url = url.clone();
        let schema = schema.clone();
        thread::spawn(move || run_worker(&url, &schema, &barrier))
    };
    let second = {
        let barrier = Arc::clone(&barrier);
        let url = url.clone();
        let schema = schema.clone();
        thread::spawn(move || run_worker(&url, &schema, &barrier))
    };
    barrier.wait();

    let mut outcomes = vec![first.join().unwrap(), second.join().unwrap()];
    outcomes.sort_by_key(|value| matches!(value, DataRightsPersistenceDisposition::Duplicate));
    assert_eq!(
        outcomes,
        vec![
            DataRightsPersistenceDisposition::Inserted,
            DataRightsPersistenceDisposition::Duplicate,
        ]
    );
}
