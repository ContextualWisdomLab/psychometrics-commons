//! Real `PostgreSQL` contract for hosted account-link HTTP persist and recover.
//!
//! A buyer posts both current proofs over HTTP. The server clock is the link
//! time. After that write commits, recover with the same valid account proof
//! returns the same `participant_ref`. An expired proof does not persist, and
//! a second participant cannot take a still-bound subject.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::account_link_http::{
    handle_account_link_http_request, AccountLinkHttpRuntime, ACCOUNT_LINK_COLLECTION_PATH,
    ACCOUNT_LINK_RECOVER_PATH,
};
use psychometrics_commons_runtime::postgres_participant_identity_link::apply_participant_identity_link_migration;
use std::sync::{Mutex, MutexGuard};

static ACCOUNT_LINK_HTTP_TEST_LOCK: Mutex<()> = Mutex::new(());

fn write_test_guard() -> MutexGuard<'static, ()> {
    ACCOUNT_LINK_HTTP_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn test_client() -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let mut client = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    client
        .batch_execute(
            "CREATE SCHEMA IF NOT EXISTS account_link_http_test;\
             SET search_path TO account_link_http_test;",
        )
        .unwrap();
    client
}

fn reset_tables(client: &mut Client) {
    client
        .batch_execute(
            "DROP TABLE IF EXISTS account_link_http_test.current_participant_identity_link;\
             DROP TABLE IF EXISTS account_link_http_test.participant_identity_link_end;\
             DROP TABLE IF EXISTS account_link_http_test.participant_identity_link;\
             DROP TABLE IF EXISTS account_link_http_test.assessment_participant;",
        )
        .unwrap();
}

fn persist_body() -> String {
    "{\"participant_ref\":\"participant_identity_http\",\"tenant_ref\":\"tenant_identity_http\",\"anonymous_session_ref\":\"session_identity_http\",\"anonymous_proof_ref\":\"anonymous_proof_http\",\"anonymous_valid_until_unix_ms\":11000,\"identity_issuer\":\"keyverse_issuer_http\",\"identity_subject_ref\":\"keyverse_subject_http\",\"authenticated_proof_ref\":\"authenticated_proof_http\",\"authenticated_valid_until_unix_ms\":11000,\"link_event_ref\":\"link_event_identity_http\"}".to_owned()
}

fn persist_request(body: &str, idempotency_key: &str) -> String {
    format!(
        "POST {ACCOUNT_LINK_COLLECTION_PATH} HTTP/1.1\r\nHost: localhost\r\nIdempotency-Key: {idempotency_key}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    )
}

fn recover_request() -> String {
    let body = "{\"tenant_ref\":\"tenant_identity_http\",\"identity_issuer\":\"keyverse_issuer_http\",\"identity_subject_ref\":\"keyverse_subject_http\",\"authenticated_proof_ref\":\"authenticated_proof_http\",\"authenticated_valid_until_unix_ms\":11000}";
    format!(
        "POST {ACCOUNT_LINK_RECOVER_PATH} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    )
}

#[test]
fn http_persist_survives_commit_and_recover_returns_the_same_participant() {
    let _guard = write_test_guard();
    let mut client = test_client();
    reset_tables(&mut client);
    apply_participant_identity_link_migration(&mut client).unwrap();

    let mut runtime = AccountLinkHttpRuntime::new(10_400);
    let mut transaction = client.transaction().unwrap();
    let created = handle_account_link_http_request(
        &persist_request(&persist_body(), "idem_account_link_http"),
        &mut runtime,
        &mut transaction,
    );
    transaction.commit().unwrap();
    assert_eq!(created.status(), 201);
    assert_eq!(created.content_type(), "application/json");
    assert!(created
        .body()
        .contains("\"participant_ref\":\"participant_identity_http\""));
    assert!(created
        .body()
        .contains("\"identity_subject_ref\":\"keyverse_subject_http\""));
    assert!(created.body().contains("\"disposition\":\"inserted\""));

    let mut replay_runtime = AccountLinkHttpRuntime::new(10_400);
    let mut transaction = client.transaction().unwrap();
    let replayed = handle_account_link_http_request(
        &persist_request(&persist_body(), "idem_account_link_http_replay"),
        &mut replay_runtime,
        &mut transaction,
    );
    transaction.commit().unwrap();
    assert_eq!(replayed.status(), 200);
    assert!(replayed.body().contains("\"disposition\":\"duplicate\""));

    let mut recover_runtime = AccountLinkHttpRuntime::new(10_600);
    let mut transaction = client.transaction().unwrap();
    let recovered = handle_account_link_http_request(
        &recover_request(),
        &mut recover_runtime,
        &mut transaction,
    );
    transaction.commit().unwrap();
    assert_eq!(recovered.status(), 200);
    assert!(recovered
        .body()
        .contains("\"participant_ref\":\"participant_identity_http\""));
    assert!(recovered.body().contains("\"disposition\":\"current\""));
}

#[test]
fn expired_proof_does_not_persist_and_unused_recover_does_not_invent_a_participant() {
    let _guard = write_test_guard();
    let mut client = test_client();
    reset_tables(&mut client);
    apply_participant_identity_link_migration(&mut client).unwrap();

    let expired_body = "{\"participant_ref\":\"participant_identity_http\",\"tenant_ref\":\"tenant_identity_http\",\"anonymous_session_ref\":\"session_identity_http\",\"anonymous_proof_ref\":\"anonymous_proof_http\",\"anonymous_valid_until_unix_ms\":10300,\"identity_issuer\":\"keyverse_issuer_http\",\"identity_subject_ref\":\"keyverse_subject_http\",\"authenticated_proof_ref\":\"authenticated_proof_http\",\"authenticated_valid_until_unix_ms\":10300,\"link_event_ref\":\"link_event_identity_http\"}";
    let mut runtime = AccountLinkHttpRuntime::new(10_400);
    let mut transaction = client.transaction().unwrap();
    let expired = handle_account_link_http_request(
        &persist_request(expired_body, "idem_expired_http"),
        &mut runtime,
        &mut transaction,
    );
    transaction.rollback().unwrap();
    assert_eq!(expired.status(), 401);
    assert_eq!(expired.content_type(), "application/problem+json");
    assert!(expired
        .body()
        .contains("urn:psychometrics-commons:problem:proof-expired"));

    let mut recover_runtime = AccountLinkHttpRuntime::new(10_600);
    let mut transaction = client.transaction().unwrap();
    let missing = handle_account_link_http_request(
        &recover_request(),
        &mut recover_runtime,
        &mut transaction,
    );
    transaction.commit().unwrap();
    assert_eq!(missing.status(), 404);
    assert!(missing
        .body()
        .contains("urn:psychometrics-commons:problem:account-link-not-found"));
}

#[test]
fn bound_subject_stays_on_the_first_http_participant() {
    let _guard = write_test_guard();
    let mut client = test_client();
    reset_tables(&mut client);
    apply_participant_identity_link_migration(&mut client).unwrap();

    let mut runtime = AccountLinkHttpRuntime::new(10_400);
    let mut transaction = client.transaction().unwrap();
    let first = handle_account_link_http_request(
        &persist_request(&persist_body(), "idem_first_http"),
        &mut runtime,
        &mut transaction,
    );
    transaction.commit().unwrap();
    assert_eq!(first.status(), 201);

    let second_body = "{\"participant_ref\":\"participant_identity_http_beta\",\"tenant_ref\":\"tenant_identity_http\",\"anonymous_session_ref\":\"session_identity_http_beta\",\"anonymous_proof_ref\":\"anonymous_proof_http_beta\",\"anonymous_valid_until_unix_ms\":11000,\"identity_issuer\":\"keyverse_issuer_http\",\"identity_subject_ref\":\"keyverse_subject_http\",\"authenticated_proof_ref\":\"authenticated_proof_http_beta\",\"authenticated_valid_until_unix_ms\":11000,\"link_event_ref\":\"link_event_identity_http_beta\"}";
    let mut second_runtime = AccountLinkHttpRuntime::new(10_450);
    let mut transaction = client.transaction().unwrap();
    let second = handle_account_link_http_request(
        &persist_request(second_body, "idem_second_http"),
        &mut second_runtime,
        &mut transaction,
    );
    transaction.rollback().unwrap();
    assert_eq!(second.status(), 409);
    assert!(second
        .body()
        .contains("urn:psychometrics-commons:problem:subject-already-bound"));
}

#[test]
fn exact_http_idempotency_replay_returns_the_original_body() {
    let _guard = write_test_guard();
    let mut client = test_client();
    reset_tables(&mut client);
    apply_participant_identity_link_migration(&mut client).unwrap();

    let mut runtime = AccountLinkHttpRuntime::new(10_400);
    let mut transaction = client.transaction().unwrap();
    let first = handle_account_link_http_request(
        &persist_request(&persist_body(), "idem_shared_http"),
        &mut runtime,
        &mut transaction,
    );
    let replay = handle_account_link_http_request(
        &persist_request(&persist_body(), "idem_shared_http"),
        &mut runtime,
        &mut transaction,
    );
    let other_body = "{\"participant_ref\":\"participant_identity_http_gamma\",\"tenant_ref\":\"tenant_identity_http\",\"anonymous_session_ref\":\"session_identity_http_gamma\",\"anonymous_proof_ref\":\"anonymous_proof_http_gamma\",\"anonymous_valid_until_unix_ms\":11000,\"identity_issuer\":\"keyverse_issuer_http\",\"identity_subject_ref\":\"keyverse_subject_http_gamma\",\"authenticated_proof_ref\":\"authenticated_proof_http_gamma\",\"authenticated_valid_until_unix_ms\":11000,\"link_event_ref\":\"link_event_identity_http_gamma\"}";
    let conflict = handle_account_link_http_request(
        &persist_request(other_body, "idem_shared_http"),
        &mut runtime,
        &mut transaction,
    );
    transaction.commit().unwrap();
    assert_eq!(first.status(), 201);
    assert_eq!(replay.status(), 200);
    assert_eq!(replay.body(), first.body());
    assert_eq!(conflict.status(), 409);
    assert!(conflict
        .body()
        .contains("urn:psychometrics-commons:problem:idempotency-conflict"));
}
