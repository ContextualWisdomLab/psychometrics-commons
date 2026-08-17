//! Real `PostgreSQL` contract for indexes that keep readiness backlog probes bounded.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::postgres_data_rights::apply_data_rights_migration;
use psychometrics_commons_runtime::postgres_health::apply_backlog_health_index_migration;
use psychometrics_commons_runtime::postgres_inbox_consumption::apply_inbox_consumption_migration;
use psychometrics_commons_runtime::postgres_integration::apply_integration_migration;
use std::sync::atomic::{AtomicU64, Ordering};

static SCHEMA_NONCE: AtomicU64 = AtomicU64::new(1);

fn test_client() -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    Client::connect(&connection, NoTls).expect("isolated CI PostgreSQL database must be reachable")
}

fn isolated_client() -> (Client, String) {
    let mut client = test_client();
    let nonce = SCHEMA_NONCE.fetch_add(1, Ordering::Relaxed);
    let schema = format!("backlog_health_index_{}_{}", std::process::id(), nonce);
    client
        .batch_execute(&format!(
            "CREATE SCHEMA {schema}; SET search_path TO {schema};"
        ))
        .expect("isolated backlog-health index schema should be created");
    apply_integration_migration(&mut client).expect("integration migration should apply");
    apply_inbox_consumption_migration(&mut client)
        .expect("inbox-consumption migration should apply");
    apply_data_rights_migration(&mut client).expect("data-rights migration should apply");
    apply_backlog_health_index_migration(&mut client)
        .expect("product backlog-health index migration should apply");
    (client, schema)
}

fn cleanup(mut client: Client, schema: &str) {
    client
        .batch_execute(&format!(
            "SET search_path TO public; DROP SCHEMA {schema} CASCADE;"
        ))
        .expect("isolated backlog-health index schema should be removed");
}

fn index_definition(client: &mut Client, index_name: &str) -> String {
    client
        .query_one(
            "SELECT indexdef FROM pg_indexes \
             WHERE schemaname = current_schema() AND indexname = $1",
            &[&index_name],
        )
        .unwrap_or_else(|error| panic!("index {index_name} must exist: {error}"))
        .get(0)
}

fn assert_partial_health_index(
    client: &mut Client,
    index_name: &str,
    table_name: &str,
    timestamp_column: &str,
    state_column: &str,
    state_tokens: &[&str],
) {
    let definition = index_definition(client, index_name).to_ascii_lowercase();
    assert!(
        definition.contains(table_name),
        "{index_name} must index {table_name}: {definition}"
    );
    assert!(
        definition.contains(timestamp_column),
        "{index_name} must cover {timestamp_column}: {definition}"
    );
    assert!(
        definition.contains(" where "),
        "{index_name} must be partial so terminal history does not dominate the readiness index: {definition}"
    );
    assert!(
        definition.contains(state_column),
        "{index_name} predicate must constrain {state_column}: {definition}"
    );
    for state in state_tokens {
        assert!(
            definition.contains(state),
            "{index_name} predicate must include state {state}: {definition}"
        );
    }
}

#[test]
fn readiness_backlog_queries_have_state_selective_indexes() {
    let (mut client, schema) = isolated_client();

    assert_partial_health_index(
        &mut client,
        "integration_outbox_pending_health_idx",
        "integration_outbox",
        "latest_event_at_unix_ms",
        "current_state",
        &["pending"],
    );
    assert_partial_health_index(
        &mut client,
        "integration_outbox_quarantined_health_idx",
        "integration_outbox",
        "latest_event_at_unix_ms",
        "current_state",
        &["quarantined"],
    );
    assert_partial_health_index(
        &mut client,
        "integration_consumption_active_health_idx",
        "integration_consumption",
        "latest_event_at_unix_ms",
        "consumption_state",
        &["pending", "processing"],
    );
    assert_partial_health_index(
        &mut client,
        "integration_consumption_quarantined_health_idx",
        "integration_consumption",
        "latest_event_at_unix_ms",
        "consumption_state",
        &["quarantined"],
    );
    assert_partial_health_index(
        &mut client,
        "data_rights_request_active_health_idx",
        "data_rights_request_state",
        "requested_at_unix_ms",
        "current_state",
        &["requested", "identity_verified", "processing"],
    );

    let propagation_index =
        index_definition(&mut client, "data_rights_propagation_state_idx").to_ascii_lowercase();
    assert!(
        propagation_index.contains("current_state")
            && propagation_index.contains("latest_event_at_unix_ms"),
        "the existing propagation state/time index must continue covering propagation health probes: {propagation_index}"
    );

    apply_backlog_health_index_migration(&mut client)
        .expect("readiness index apply must be idempotent for existing installations");

    cleanup(client, &schema);
}

#[test]
fn product_apply_path_fails_closed_when_required_backlog_relations_are_missing() {
    let mut client = test_client();
    let nonce = SCHEMA_NONCE.fetch_add(1, Ordering::Relaxed);
    let schema = format!(
        "backlog_health_index_missing_{}_{}",
        std::process::id(),
        nonce
    );
    client
        .batch_execute(&format!(
            "CREATE SCHEMA {schema}; SET search_path TO {schema};"
        ))
        .expect("isolated missing-relation schema should be created");

    assert!(
        apply_backlog_health_index_migration(&mut client).is_err(),
        "readiness indexes must not be claimed when the owned backlog tables are absent"
    );

    cleanup(client, &schema);
}
