//! Durable identity verification for an already requested data-rights export or deletion.

use postgres::{Client, IsolationLevel, NoTls};
use psychometrics_commons_runtime::data_rights::{
    DataRightsRequest, DataRightsRequestKind, DataRightsState,
};
use psychometrics_commons_runtime::integration::IntegrationEvent;
use psychometrics_commons_runtime::postgres_data_rights::{
    apply_data_rights_migration, persist_data_rights_identity_verification,
    persist_requested_data_rights_with_propagation, DataRightsPersistenceError,
    DataRightsPropagationTarget, DataRightsVerificationDisposition,
};
use psychometrics_commons_runtime::postgres_integration::apply_integration_migration;

const DIGEST: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn test_client(schema_prefix: &str) -> Client {
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
    let mut client = test_client(schema_prefix);
    apply_integration_migration(&mut client).unwrap();
    apply_data_rights_migration(&mut client).unwrap();
    apply_data_rights_migration(&mut client).unwrap();
    client
}

fn new_request() -> DataRightsRequest {
    DataRightsRequest::new(
        "data_rights_request_verify",
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
        "data_rights_event_verify",
        "data_rights.deletion.requested",
        "v1",
        "psychometrics_commons",
        "tenant_alpha",
        "data_rights_request_verify",
        10_000,
        "data_rights_request_verify",
        None,
        DIGEST,
    )
    .unwrap()
}

fn persist_requested(client: &mut Client) -> DataRightsRequest {
    let request = new_request();
    let event = event();
    let targets = [DataRightsPropagationTarget::new(
        "dependent_system_alpha",
        &event,
    )];
    persist_requested_data_rights_with_propagation(client, &request, &targets, 3).unwrap();
    request
}

#[test]
fn identity_verification_persists_and_replays_exactly() {
    let mut client = ready_client("data_rights_verify_replay");
    let mut request = persist_requested(&mut client);
    request
        .verify_identity("verification_evidence_alpha", 10_100)
        .unwrap();
    assert_eq!(request.state(), DataRightsState::IdentityVerified);

    let mut transaction = client.transaction().unwrap();
    assert_eq!(
        persist_data_rights_identity_verification(&mut transaction, &request).unwrap(),
        DataRightsVerificationDisposition::Verified
    );
    assert_eq!(
        persist_data_rights_identity_verification(&mut transaction, &request).unwrap(),
        DataRightsVerificationDisposition::Duplicate
    );
    transaction.commit().unwrap();

    let row = client
        .query_one(
            "SELECT current_state, verification_evidence_ref, verified_at_unix_ms,
                    latest_event_at_unix_ms
             FROM data_rights_request_state WHERE request_ref = $1",
            &[&"data_rights_request_verify"],
        )
        .unwrap();
    assert_eq!(row.get::<_, String>(0), "identity_verified");
    assert_eq!(
        row.get::<_, Option<String>>(1).as_deref(),
        Some("verification_evidence_alpha")
    );
    assert_eq!(row.get::<_, Option<i64>>(2), Some(10_100));
    assert_eq!(row.get::<_, i64>(3), 10_100);
}

#[test]
fn conflicting_verification_and_unverified_requests_fail_closed() {
    let mut client = ready_client("data_rights_verify_conflict");
    let mut request = persist_requested(&mut client);
    request
        .verify_identity("verification_evidence_alpha", 10_100)
        .unwrap();
    {
        let mut transaction = client.transaction().unwrap();
        persist_data_rights_identity_verification(&mut transaction, &request).unwrap();
        transaction.commit().unwrap();
    }

    let mut conflicting = new_request();
    conflicting
        .verify_identity("verification_evidence_beta", 10_200)
        .unwrap();
    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        persist_data_rights_identity_verification(&mut transaction, &conflicting),
        Err(DataRightsPersistenceError::ConflictingReplay)
    ));
    let mut same_evidence_later = new_request();
    same_evidence_later
        .verify_identity("verification_evidence_alpha", 10_200)
        .unwrap();
    assert!(matches!(
        persist_data_rights_identity_verification(&mut transaction, &same_evidence_later),
        Err(DataRightsPersistenceError::ConflictingReplay)
    ));
    let unverified = new_request();
    assert!(matches!(
        persist_data_rights_identity_verification(&mut transaction, &unverified),
        Err(DataRightsPersistenceError::InvalidRequestState)
    ));
    let mut missing = DataRightsRequest::new(
        "data_rights_request_missing",
        "tenant_alpha",
        "participant_alpha",
        DataRightsRequestKind::Deletion,
        "scope_alpha",
        10_000,
    )
    .unwrap();
    missing
        .verify_identity("verification_evidence_alpha", 10_100)
        .unwrap();
    assert!(matches!(
        persist_data_rights_identity_verification(&mut transaction, &missing),
        Err(DataRightsPersistenceError::RequestNotFound)
    ));
    transaction.rollback().unwrap();
}

#[test]
fn exact_verification_replay_rejects_request_identity_rebinding() {
    let mut client = ready_client("data_rights_verify_identity_rebind");
    let mut request = persist_requested(&mut client);
    request
        .verify_identity("verification_evidence_alpha", 10_100)
        .unwrap();
    {
        let mut transaction = client.transaction().unwrap();
        persist_data_rights_identity_verification(&mut transaction, &request).unwrap();
        transaction.commit().unwrap();
    }

    for (participant_ref, kind, scope_ref) in [
        (
            "participant_beta",
            DataRightsRequestKind::Deletion,
            "scope_alpha",
        ),
        (
            "participant_alpha",
            DataRightsRequestKind::Export,
            "scope_alpha",
        ),
        (
            "participant_alpha",
            DataRightsRequestKind::Deletion,
            "scope_beta",
        ),
    ] {
        let mut rebound = DataRightsRequest::new(
            "data_rights_request_verify",
            "tenant_alpha",
            participant_ref,
            kind,
            scope_ref,
            10_000,
        )
        .unwrap();
        rebound
            .verify_identity("verification_evidence_alpha", 10_100)
            .unwrap();
        let mut transaction = client.transaction().unwrap();
        assert!(matches!(
            persist_data_rights_identity_verification(&mut transaction, &rebound),
            Err(DataRightsPersistenceError::ConflictingReplay)
        ));
        transaction.rollback().unwrap();
    }
}

#[test]
fn stored_non_requested_state_and_overflowing_time_fail_closed() {
    let mut client = ready_client("data_rights_verify_invalid");
    let mut request = persist_requested(&mut client);
    client
        .execute(
            "UPDATE data_rights_request_state
             SET current_state = 'processing'
             WHERE request_ref = $1",
            &[&"data_rights_request_verify"],
        )
        .unwrap();
    request
        .verify_identity("verification_evidence_alpha", 10_100)
        .unwrap();
    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        persist_data_rights_identity_verification(&mut transaction, &request),
        Err(DataRightsPersistenceError::InvalidRequestState)
    ));
    transaction.rollback().unwrap();

    let mut overflow_client = ready_client("data_rights_verify_overflow");
    let mut overflow = persist_requested(&mut overflow_client);
    overflow
        .verify_identity("verification_evidence_overflow", u64::MAX)
        .unwrap();
    let mut transaction = overflow_client.transaction().unwrap();
    assert!(matches!(
        persist_data_rights_identity_verification(&mut transaction, &overflow),
        Err(DataRightsPersistenceError::ValueOutOfRange)
    ));
    transaction.rollback().unwrap();
}

#[test]
fn identity_verification_requires_read_committed_and_surfaces_database_failure() {
    let mut client = ready_client("data_rights_verify_isolation");
    let mut request = persist_requested(&mut client);
    request
        .verify_identity("verification_evidence_alpha", 10_100)
        .unwrap();

    let mut transaction = client
        .build_transaction()
        .isolation_level(IsolationLevel::Serializable)
        .start()
        .unwrap();
    assert!(matches!(
        persist_data_rights_identity_verification(&mut transaction, &request),
        Err(DataRightsPersistenceError::UnsupportedIsolationLevel)
    ));
    transaction.rollback().unwrap();

    let mut missing_table = test_client("data_rights_verify_missing");
    let mut transaction = missing_table.transaction().unwrap();
    assert!(matches!(
        persist_data_rights_identity_verification(&mut transaction, &request),
        Err(DataRightsPersistenceError::Database(_))
    ));
    transaction.rollback().unwrap();
}

#[test]
fn applying_data_rights_migration_without_outbox_tables_fails() {
    let mut client = test_client("data_rights_verify_no_outbox");
    assert!(apply_data_rights_migration(&mut client).is_err());
}

#[test]
fn unmatched_verification_select_failure_is_a_database_failure() {
    let mut client = ready_client("data_rights_verify_select_hidden");
    let mut request = persist_requested(&mut client);
    request
        .verify_identity("verification_evidence_alpha", 10_100)
        .unwrap();
    client
        .execute(
            "UPDATE data_rights_request_state
             SET current_state = 'processing'
             WHERE request_ref = $1",
            &[&"data_rights_request_verify"],
        )
        .unwrap();
    let sink = format!("data_rights_verify_select_sink_{}", std::process::id());
    client
        .batch_execute(&format!(
            "CREATE SCHEMA {sink};
             CREATE OR REPLACE FUNCTION data_rights_verify_redirect_after_update()
             RETURNS trigger LANGUAGE plpgsql AS $$
             BEGIN
                 PERFORM set_config('search_path', '{sink}', false);
                 RETURN NULL;
             END $$;
             CREATE TRIGGER data_rights_verify_redirect_after_update
             AFTER UPDATE ON data_rights_request_state
             FOR EACH STATEMENT EXECUTE FUNCTION data_rights_verify_redirect_after_update();"
        ))
        .unwrap();

    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        persist_data_rights_identity_verification(&mut transaction, &request),
        Err(DataRightsPersistenceError::Database(_))
    ));
    transaction.rollback().unwrap();
}