//! Concurrency contract for data-rights identity-verification replay classification.
//!
//! Once replay state is classified inside a caller-owned transaction, the matched
//! request row must remain locked until that transaction ends. Otherwise a second
//! writer can advance the lifecycle after classification and make the returned
//! disposition stale before the caller can compose further work atomically.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::data_rights::{DataRightsRequest, DataRightsRequestKind};
use psychometrics_commons_runtime::integration::IntegrationEvent;
use psychometrics_commons_runtime::postgres_data_rights::{
    apply_data_rights_migration, persist_data_rights_identity_verification,
    persist_requested_data_rights_with_propagation, DataRightsPropagationTarget,
    DataRightsVerificationDisposition,
};
use psychometrics_commons_runtime::postgres_integration::apply_integration_migration;

const DIGEST: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn next_schema_name(client: &mut Client, prefix: &str) -> String {
    let transaction_identity: String = client
        .query_one("SELECT pg_current_xact_id()::text", &[])
        .expect("PostgreSQL must issue a transaction identity for the fixture schema")
        .get(0);
    format!("{prefix}_{transaction_identity}")
}

fn connect() -> Client {
    let url = std::env::var("TEST_DATABASE_URL").expect("TEST_DATABASE_URL is required");
    Client::connect(&url, NoTls).expect("CI PostgreSQL must be reachable")
}

struct TestDatabase {
    client: Client,
    schema_name: String,
}

impl TestDatabase {
    fn new(prefix: &str) -> Self {
        let mut client = connect();
        let schema_name = next_schema_name(&mut client, prefix);
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
        self.client
            .batch_execute(&format!(
                "SET search_path TO public; DROP SCHEMA IF EXISTS {} CASCADE;",
                self.schema_name
            ))
            .expect("isolated data-rights verification fixture schema should be removable");
    }
}

fn requested_request(client: &mut Client) -> DataRightsRequest {
    let request = DataRightsRequest::new(
        "data_rights_request_verify_lock",
        "tenant_alpha",
        "participant_alpha",
        DataRightsRequestKind::Deletion,
        "scope_alpha",
        10_000,
    )
    .unwrap();
    let event = IntegrationEvent::new(
        "data_rights_event_verify_lock",
        "data_rights.deletion.requested",
        "v1",
        "psychometrics_commons",
        "tenant_alpha",
        "data_rights_request_verify_lock",
        10_000,
        "data_rights_request_verify_lock",
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
}

#[test]
fn fixture_schema_identity_is_database_issued_and_restart_safe() {
    let mut client = connect();
    let first = next_schema_name(&mut client, "data_rights_verify_classification_lock");
    let second = next_schema_name(&mut client, "data_rights_verify_classification_lock");

    for schema_name in [&first, &second] {
        let identity = schema_name
            .strip_prefix("data_rights_verify_classification_lock_")
            .expect("fixture schema must keep its descriptive prefix");
        identity
            .parse::<u64>()
            .expect("fixture schema suffix must be a database-issued transaction identity");
    }
    assert_ne!(
        first, second,
        "separate fixture allocations must never reuse a schema identity"
    );
}

#[test]
fn duplicate_verification_classification_holds_row_lock_until_transaction_end() {
    let mut database = TestDatabase::new("data_rights_verify_classification_lock");
    let schema_name = database.schema_name.clone();
    let mut request = requested_request(&mut database.client);
    request
        .verify_identity("verification_evidence_alpha", 10_100)
        .unwrap();

    {
        let mut transaction = database.client.transaction().unwrap();
        assert_eq!(
            persist_data_rights_identity_verification(&mut transaction, &request).unwrap(),
            DataRightsVerificationDisposition::Verified
        );
        transaction.commit().unwrap();
    }

    let mut classifier = database.client.transaction().unwrap();
    assert_eq!(
        persist_data_rights_identity_verification(&mut classifier, &request).unwrap(),
        DataRightsVerificationDisposition::Duplicate
    );

    let mut contender = connect();
    contender
        .batch_execute(&format!(
            "SET search_path TO {schema_name}; SET lock_timeout TO '100ms';"
        ))
        .unwrap();
    let error = contender
        .execute(
            "UPDATE data_rights_request_state
             SET current_state = 'processing', updated_at = clock_timestamp()
             WHERE request_ref = $1 AND tenant_ref = $2",
            &[&"data_rights_request_verify_lock", &"tenant_alpha"],
        )
        .expect_err("classification must keep the matched request row locked");
    assert_eq!(
        error.code(),
        Some(&postgres::error::SqlState::LOCK_NOT_AVAILABLE)
    );

    classifier.rollback().unwrap();
}
