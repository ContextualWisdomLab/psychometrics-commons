//! Stored data-rights request identity remains opaque and non-numeric.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::postgres_data_rights::apply_data_rights_migration;
use psychometrics_commons_runtime::postgres_integration::apply_integration_migration;
use std::ops::{Deref, DerefMut};

struct SchemaClient {
    client: Client,
    schema_name: String,
}

impl SchemaClient {
    fn schema_name(&self) -> &str {
        &self.schema_name
    }
}

impl Deref for SchemaClient {
    type Target = Client;

    fn deref(&self) -> &Self::Target {
        &self.client
    }
}

impl DerefMut for SchemaClient {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.client
    }
}

impl Drop for SchemaClient {
    fn drop(&mut self) {
        let _ = self.client.batch_execute(&format!(
            "RESET search_path; DROP SCHEMA IF EXISTS {} CASCADE;",
            self.schema_name
        ));
    }
}

fn schema_client() -> SchemaClient {
    let url = std::env::var("TEST_DATABASE_URL").expect("TEST_DATABASE_URL is required");
    let mut client = Client::connect(&url, NoTls).expect("CI PostgreSQL must be reachable");
    let database_nonce: String = client
        .query_one("SELECT pg_current_xact_id()::text", &[])
        .expect("PostgreSQL must allocate a durable transaction identity for test isolation")
        .get(0);
    let schema_name = format!("data_rights_identity_{database_nonce}");
    client
        .batch_execute(&format!(
            "CREATE SCHEMA {schema_name}; SET search_path TO {schema_name};"
        ))
        .expect("isolated data-rights identity schema should be created");
    let mut client = SchemaClient {
        client,
        schema_name,
    };
    apply_integration_migration(&mut *client)
        .expect("integration migration should apply in the isolated schema");
    apply_data_rights_migration(&mut *client)
        .expect("data-rights migration should apply in the isolated schema");
    client
}

#[test]
fn database_transaction_identity_prevents_schema_name_reuse() {
    let first = schema_client();
    let second = schema_client();

    assert_ne!(
        first.schema_name(),
        second.schema_name(),
        "schema isolation must not depend on PID lifetime"
    );
}

#[test]
fn request_reference_must_remain_opaque_in_storage() {
    let mut db = schema_client();

    let error = db
        .execute(
            "INSERT INTO data_rights_request_state (request_ref, tenant_ref, participant_ref, request_kind, scope_ref, current_state, requested_at_unix_ms, latest_event_at_unix_ms) VALUES ('123', 'tenant_alpha', 'participant_alpha', 'export', 'scope_alpha', 'requested', 10000, 10000)",
            &[],
        )
        .unwrap_err();
    assert_eq!(
        error
            .as_db_error()
            .and_then(postgres::error::DbError::constraint),
        Some("data_rights_request_ref_format_check")
    );
}
