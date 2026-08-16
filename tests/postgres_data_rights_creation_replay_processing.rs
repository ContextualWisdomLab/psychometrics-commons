//! Creation replay remains idempotent after durable processing has started.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::data_rights::{DataRightsRequest, DataRightsRequestKind};
use psychometrics_commons_runtime::integration::IntegrationEvent;
use psychometrics_commons_runtime::postgres_data_rights::{
    apply_data_rights_migration, persist_data_rights_identity_verification,
    persist_requested_data_rights_with_propagation, DataRightsPersistenceDisposition,
    DataRightsPersistenceError, DataRightsPropagationTarget,
};
use psychometrics_commons_runtime::postgres_integration::apply_integration_migration;
use std::ops::{Deref, DerefMut};

const DIGEST: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

struct IsolatedClient {
    client: Client,
    schema: String,
}

impl Deref for IsolatedClient {
    type Target = Client;

    fn deref(&self) -> &Self::Target {
        &self.client
    }
}

impl DerefMut for IsolatedClient {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.client
    }
}

impl Drop for IsolatedClient {
    fn drop(&mut self) {
        let _ = self.client.batch_execute(&format!(
            "SET search_path TO public; DROP SCHEMA IF EXISTS {} CASCADE;",
            self.schema
        ));
    }
}

fn ready_client() -> IsolatedClient {
    let url = std::env::var("TEST_DATABASE_URL").expect("TEST_DATABASE_URL is required");
    let mut client = Client::connect(&url, NoTls).expect("CI PostgreSQL must be reachable");
    let backend_pid: i32 = client
        .query_one("SELECT pg_backend_pid()", &[])
        .expect("PostgreSQL backend identity must be available")
        .get(0);
    let schema = format!("data_rights_processing_replay_{backend_pid}");
    client
        .batch_execute(&format!(
            "DROP SCHEMA IF EXISTS {schema} CASCADE; CREATE SCHEMA {schema}; SET search_path TO {schema};"
        ))
        .unwrap();
    apply_integration_migration(&mut client).unwrap();
    apply_data_rights_migration(&mut client).unwrap();
    IsolatedClient { client, schema }
}

fn request() -> DataRightsRequest {
    DataRightsRequest::new(
        "data_rights_request_processing_replay",
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
        "data_rights_event_processing_replay",
        "data_rights.deletion.requested",
        "v1",
        "psychometrics_commons",
        "tenant_alpha",
        "data_rights_request_processing_replay",
        10_000,
        "data_rights_request_processing_replay",
        None,
        DIGEST,
    )
    .unwrap()
}

fn persist_and_verify(client: &mut Client) -> DataRightsRequest {
    let original = request();
    let original_event = event();
    let targets = [DataRightsPropagationTarget::new(
        "dependent_system_alpha",
        &original_event,
    )];
    assert_eq!(
        persist_requested_data_rights_with_propagation(client, &original, &targets, 3).unwrap(),
        DataRightsPersistenceDisposition::Inserted
    );

    let mut verified = request();
    verified
        .verify_identity("verification_evidence_processing_replay", 10_100)
        .unwrap();
    let mut transaction = client.transaction().unwrap();
    persist_data_rights_identity_verification(&mut transaction, &verified).unwrap();
    transaction.commit().unwrap();
    verified
}

#[test]
fn exact_creation_replay_remains_duplicate_after_processing_starts() {
    let mut client = ready_client();
    let verified = persist_and_verify(&mut client);

    client
        .execute(
            "UPDATE data_rights_request_state
             SET current_state = 'processing', latest_event_at_unix_ms = $2
             WHERE request_ref = $1",
            &[&verified.request_ref(), &10_200_i64],
        )
        .unwrap();

    let replay_event = event();
    let replay_targets = [DataRightsPropagationTarget::new(
        "dependent_system_alpha",
        &replay_event,
    )];
    assert_eq!(
        persist_requested_data_rights_with_propagation(&mut client, &request(), &replay_targets, 3)
            .unwrap(),
        DataRightsPersistenceDisposition::Duplicate,
        "the original creation command must stay replay-safe after processing begins"
    );
}

#[test]
fn creation_replay_rejects_incoherent_processing_history() {
    let mut client = ready_client();
    let verified = persist_and_verify(&mut client);

    client
        .execute(
            "UPDATE data_rights_request_state
             SET current_state = 'processing', latest_event_at_unix_ms = $2
             WHERE request_ref = $1",
            &[&verified.request_ref(), &10_050_i64],
        )
        .unwrap();

    let replay_event = event();
    let replay_targets = [DataRightsPropagationTarget::new(
        "dependent_system_alpha",
        &replay_event,
    )];
    assert!(matches!(
        persist_requested_data_rights_with_propagation(&mut client, &request(), &replay_targets, 3),
        Err(DataRightsPersistenceError::ConflictingReplay)
    ));
}
