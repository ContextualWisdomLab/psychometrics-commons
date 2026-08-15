//! Real `PostgreSQL` coverage for backlog probes across supported generic client types.
//!
//! The production probes accept any `GenericClient`, so both a direct `Client` and a
//! caller-owned `Transaction` must exercise successful and failed query boundaries.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::postgres_data_rights::apply_data_rights_migration;
use psychometrics_commons_runtime::postgres_health::{
    probe_postgres_data_rights_backlog, probe_postgres_integration_backlog,
    PostgresBacklogProbeError,
};
use psychometrics_commons_runtime::postgres_inbox_consumption::apply_inbox_consumption_migration;
use psychometrics_commons_runtime::postgres_integration::apply_integration_migration;
use std::sync::atomic::{AtomicU64, Ordering};

static SCHEMA_NONCE: AtomicU64 = AtomicU64::new(1);

fn test_client() -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    Client::connect(&connection, NoTls).expect("isolated CI PostgreSQL database must be reachable")
}

fn create_isolated_schema(client: &mut Client, prefix: &str) -> String {
    let nonce = SCHEMA_NONCE.fetch_add(1, Ordering::Relaxed);
    let schema = format!("{prefix}_{}_{}", std::process::id(), nonce);
    client
        .batch_execute(&format!(
            "CREATE SCHEMA {schema}; SET search_path TO {schema};"
        ))
        .expect("isolated backlog-probe schema should be created");
    schema
}

fn cleanup_schema(mut client: Client, schema: &str) {
    client
        .batch_execute(&format!(
            "SET search_path TO public; DROP SCHEMA {schema} CASCADE;"
        ))
        .expect("isolated backlog-probe schema should be removed");
}

#[test]
fn integration_probe_supports_transaction_success_and_direct_client_failure() {
    let mut client = test_client();
    let schema = create_isolated_schema(&mut client, "integration_probe_generic_client");
    apply_integration_migration(&mut client).expect("integration migration should apply");
    apply_inbox_consumption_migration(&mut client)
        .expect("inbox-consumption migration should apply");

    let mut transaction = client.transaction().unwrap();
    let evidence = probe_postgres_integration_backlog(&mut transaction).unwrap();
    assert_eq!(evidence.pending_outbox_count(), 0);
    assert_eq!(evidence.active_consumption_count(), 0);
    transaction.rollback().unwrap();

    client
        .batch_execute("DROP TABLE integration_outbox CASCADE;")
        .unwrap();
    assert!(matches!(
        probe_postgres_integration_backlog(&mut client),
        Err(PostgresBacklogProbeError::Database(_))
    ));

    cleanup_schema(client, &schema);
}

#[test]
fn data_rights_probe_supports_transaction_success_and_direct_client_failure() {
    let mut client = test_client();
    let schema = create_isolated_schema(&mut client, "data_rights_probe_generic_client");
    apply_integration_migration(&mut client).expect("integration migration should apply");
    apply_data_rights_migration(&mut client).expect("data-rights migration should apply");

    let mut transaction = client.transaction().unwrap();
    let evidence = probe_postgres_data_rights_backlog(&mut transaction).unwrap();
    assert_eq!(evidence.active_request_count(), 0);
    assert_eq!(evidence.pending_propagation_count(), 0);
    transaction.rollback().unwrap();

    client
        .batch_execute("DROP TABLE data_rights_request_state CASCADE;")
        .unwrap();
    assert!(matches!(
        probe_postgres_data_rights_backlog(&mut client),
        Err(PostgresBacklogProbeError::Database(_))
    ));

    cleanup_schema(client, &schema);
}
