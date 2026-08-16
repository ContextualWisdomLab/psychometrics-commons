//! Real `PostgreSQL` contract for hosted account-link HTTP persist, recover, and unlink.
//!
//! A buyer posts both current proofs over HTTP. The server clock is the link
//! time. After that write commits, recover with the same valid account proof
//! returns the same `participant_ref`. Unlink recovers from that proof, not a
//! client `participant_ref`. After unlink+relink, recover with the ended
//! subject returns 404.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::account_link_http::{
    accept_one_account_link_http, bind_account_link_http, handle_account_link_http_request,
    AccountLinkHttpRuntime, ACCOUNT_LINK_COLLECTION_PATH, ACCOUNT_LINK_RECOVER_PATH,
    ACCOUNT_LINK_UNLINK_PATH,
};
use psychometrics_commons_runtime::postgres_participant_identity_link::apply_participant_identity_link_migration;
use std::io::{Read, Write};
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

fn recover_request(subject_ref: &str, proof_ref: &str) -> String {
    let body = format!(
        "{{\"tenant_ref\":\"tenant_identity_http\",\"identity_issuer\":\"keyverse_issuer_http\",\"identity_subject_ref\":\"{subject_ref}\",\"authenticated_proof_ref\":\"{proof_ref}\",\"authenticated_valid_until_unix_ms\":11000}}"
    );
    format!(
        "POST {ACCOUNT_LINK_RECOVER_PATH} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    )
}

fn unlink_request(
    subject_ref: &str,
    proof_ref: &str,
    end_event: &str,
    idempotency_key: &str,
) -> String {
    let body = format!(
        "{{\"tenant_ref\":\"tenant_identity_http\",\"identity_issuer\":\"keyverse_issuer_http\",\"identity_subject_ref\":\"{subject_ref}\",\"authenticated_proof_ref\":\"{proof_ref}\",\"authenticated_valid_until_unix_ms\":11000,\"link_end_event_ref\":\"{end_event}\"}}"
    );
    format!(
        "POST {ACCOUNT_LINK_UNLINK_PATH} HTTP/1.1\r\nHost: localhost\r\nIdempotency-Key: {idempotency_key}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
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
        &recover_request("keyverse_subject_http", "authenticated_proof_http"),
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
fn http_unlink_uses_current_proof_then_ended_subject_cannot_recover() {
    let _guard = write_test_guard();
    let mut client = test_client();
    reset_tables(&mut client);
    apply_participant_identity_link_migration(&mut client).unwrap();

    let mut runtime = AccountLinkHttpRuntime::new(10_400);
    let mut transaction = client.transaction().unwrap();
    let created = handle_account_link_http_request(
        &persist_request(&persist_body(), "idem_link_before_unlink"),
        &mut runtime,
        &mut transaction,
    );
    assert_eq!(created.status(), 201);

    let unlinked = handle_account_link_http_request(
        &unlink_request(
            "keyverse_subject_http",
            "authenticated_proof_http",
            "link_end_event_identity_http",
            "idem_unlink_http",
        ),
        &mut runtime,
        &mut transaction,
    );
    let replay = handle_account_link_http_request(
        &unlink_request(
            "keyverse_subject_http",
            "authenticated_proof_http",
            "link_end_event_identity_http",
            "idem_unlink_http",
        ),
        &mut runtime,
        &mut transaction,
    );
    transaction.commit().unwrap();
    assert_eq!(unlinked.status(), 200);
    assert!(unlinked.body().contains("\"disposition\":\"ended\""));
    assert_eq!(replay.status(), 200);
    assert_eq!(replay.body(), unlinked.body());

    let mut recover_runtime = AccountLinkHttpRuntime::new(10_600);
    let mut transaction = client.transaction().unwrap();
    let missing = handle_account_link_http_request(
        &recover_request("keyverse_subject_http", "authenticated_proof_http"),
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
fn http_recover_rejects_an_ended_subject_after_relink_to_another_account() {
    let _guard = write_test_guard();
    let mut client = test_client();
    reset_tables(&mut client);
    apply_participant_identity_link_migration(&mut client).unwrap();

    let mut runtime = AccountLinkHttpRuntime::new(10_400);
    let mut transaction = client.transaction().unwrap();
    assert_eq!(
        handle_account_link_http_request(
            &persist_request(&persist_body(), "idem_first_subject"),
            &mut runtime,
            &mut transaction,
        )
        .status(),
        201
    );
    assert_eq!(
        handle_account_link_http_request(
            &unlink_request(
                "keyverse_subject_http",
                "authenticated_proof_http",
                "link_end_event_identity_http",
                "idem_end_first",
            ),
            &mut runtime,
            &mut transaction,
        )
        .status(),
        200
    );
    transaction.commit().unwrap();

    let relink_body = "{\"participant_ref\":\"participant_identity_http\",\"tenant_ref\":\"tenant_identity_http\",\"anonymous_session_ref\":\"session_identity_http_gamma\",\"anonymous_proof_ref\":\"anonymous_proof_http_gamma\",\"anonymous_valid_until_unix_ms\":11000,\"identity_issuer\":\"keyverse_issuer_http\",\"identity_subject_ref\":\"keyverse_subject_http_gamma\",\"authenticated_proof_ref\":\"authenticated_proof_http_gamma\",\"authenticated_valid_until_unix_ms\":11000,\"link_event_ref\":\"link_event_identity_http_gamma\"}";
    let mut relink_runtime = AccountLinkHttpRuntime::new(10_500);
    let mut transaction = client.transaction().unwrap();
    let relinked = handle_account_link_http_request(
        &persist_request(relink_body, "idem_second_subject"),
        &mut relink_runtime,
        &mut transaction,
    );
    transaction.commit().unwrap();
    assert_eq!(relinked.status(), 201);
    assert!(relinked
        .body()
        .contains("\"identity_subject_ref\":\"keyverse_subject_http_gamma\""));

    let mut recover_runtime = AccountLinkHttpRuntime::new(10_600);
    let mut transaction = client.transaction().unwrap();
    let ended_subject = handle_account_link_http_request(
        &recover_request("keyverse_subject_http", "authenticated_proof_http"),
        &mut recover_runtime,
        &mut transaction,
    );
    let current_subject = handle_account_link_http_request(
        &recover_request(
            "keyverse_subject_http_gamma",
            "authenticated_proof_http_gamma",
        ),
        &mut recover_runtime,
        &mut transaction,
    );
    let stale_unlink = handle_account_link_http_request(
        &unlink_request(
            "keyverse_subject_http",
            "authenticated_proof_http",
            "link_end_event_identity_http_stale",
            "idem_stale_unlink",
        ),
        &mut recover_runtime,
        &mut transaction,
    );
    transaction.commit().unwrap();
    assert_eq!(ended_subject.status(), 404);
    assert_eq!(current_subject.status(), 200);
    assert!(current_subject
        .body()
        .contains("\"participant_ref\":\"participant_identity_http\""));
    assert_eq!(stale_unlink.status(), 404);
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
        &recover_request("keyverse_subject_http", "authenticated_proof_http"),
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

#[test]
fn invalid_proofs_zero_clock_and_unlink_conflict_fail_closed() {
    let _guard = write_test_guard();
    let mut client = test_client();
    reset_tables(&mut client);
    apply_participant_identity_link_migration(&mut client).unwrap();

    let numeric_participant = "{\"participant_ref\":\"123\",\"tenant_ref\":\"tenant_identity_http\",\"anonymous_session_ref\":\"session_identity_http\",\"anonymous_proof_ref\":\"anonymous_proof_http\",\"anonymous_valid_until_unix_ms\":11000,\"identity_issuer\":\"keyverse_issuer_http\",\"identity_subject_ref\":\"keyverse_subject_http\",\"authenticated_proof_ref\":\"authenticated_proof_http\",\"authenticated_valid_until_unix_ms\":11000,\"link_event_ref\":\"link_event_identity_http\"}";
    let numeric_issuer = "{\"participant_ref\":\"participant_identity_http\",\"tenant_ref\":\"tenant_identity_http\",\"anonymous_session_ref\":\"session_identity_http\",\"anonymous_proof_ref\":\"anonymous_proof_http\",\"anonymous_valid_until_unix_ms\":11000,\"identity_issuer\":\"1\",\"identity_subject_ref\":\"keyverse_subject_http\",\"authenticated_proof_ref\":\"authenticated_proof_http\",\"authenticated_valid_until_unix_ms\":11000,\"link_event_ref\":\"link_event_identity_http\"}";
    let mut runtime = AccountLinkHttpRuntime::new(10_400);
    assert_eq!(runtime.now_unix_ms(), 10_400);
    let mut transaction = client.transaction().unwrap();
    let invalid_participant = handle_account_link_http_request(
        &persist_request(numeric_participant, "idem_numeric_participant"),
        &mut runtime,
        &mut transaction,
    );
    let invalid_issuer = handle_account_link_http_request(
        &persist_request(numeric_issuer, "idem_numeric_issuer"),
        &mut runtime,
        &mut transaction,
    );
    let invalid_recover = handle_account_link_http_request(
        &recover_request("1", "authenticated_proof_http"),
        &mut runtime,
        &mut transaction,
    );
    let invalid_unlink = handle_account_link_http_request(
        &unlink_request(
            "1",
            "authenticated_proof_http",
            "link_end_event_bad",
            "idem_bad_unlink",
        ),
        &mut runtime,
        &mut transaction,
    );
    transaction.rollback().unwrap();
    assert_eq!(invalid_participant.status(), 400);
    assert_eq!(invalid_issuer.status(), 400);
    assert_eq!(invalid_recover.status(), 400);
    assert_eq!(invalid_unlink.status(), 400);

    let mut zero_clock = AccountLinkHttpRuntime::new(0);
    let mut transaction = client.transaction().unwrap();
    let zero_recover = handle_account_link_http_request(
        &recover_request("keyverse_subject_http", "authenticated_proof_http"),
        &mut zero_clock,
        &mut transaction,
    );
    transaction.rollback().unwrap();
    assert_eq!(zero_recover.status(), 500);
    assert!(zero_recover
        .body()
        .contains("urn:psychometrics-commons:problem:server-clock"));

    let mut unlink_runtime = AccountLinkHttpRuntime::new(10_400);
    let mut transaction = client.transaction().unwrap();
    assert_eq!(
        handle_account_link_http_request(
            &persist_request(&persist_body(), "idem_for_unlink_conflict"),
            &mut unlink_runtime,
            &mut transaction,
        )
        .status(),
        201
    );
    assert_eq!(
        handle_account_link_http_request(
            &unlink_request(
                "keyverse_subject_http",
                "authenticated_proof_http",
                "link_end_event_identity_http",
                "idem_unlink_conflict",
            ),
            &mut unlink_runtime,
            &mut transaction,
        )
        .status(),
        200
    );
    let conflict = handle_account_link_http_request(
        &unlink_request(
            "keyverse_subject_http",
            "authenticated_proof_http",
            "link_end_event_identity_http_other",
            "idem_unlink_conflict",
        ),
        &mut unlink_runtime,
        &mut transaction,
    );
    transaction.commit().unwrap();
    assert_eq!(conflict.status(), 409);
    assert!(conflict
        .body()
        .contains("urn:psychometrics-commons:problem:idempotency-conflict"));
}

#[test]
fn tcp_listener_serves_one_recover_request() {
    let _guard = write_test_guard();
    let mut client = test_client();
    reset_tables(&mut client);
    apply_participant_identity_link_migration(&mut client).unwrap();

    let listener = bind_account_link_http("127.0.0.1:0".parse().unwrap()).unwrap();
    let addr = listener.local_addr().unwrap();
    let request = recover_request("keyverse_subject_http", "authenticated_proof_http");
    let client_thread = std::thread::spawn(move || {
        let mut stream = std::net::TcpStream::connect(addr).unwrap();
        stream.write_all(request.as_bytes()).unwrap();
        let mut body = String::new();
        stream.read_to_string(&mut body).unwrap();
        body
    });
    let mut runtime = AccountLinkHttpRuntime::new(10_400);
    let mut transaction = client.transaction().unwrap();
    let response = accept_one_account_link_http(&listener, &mut runtime, &mut transaction).unwrap();
    transaction.commit().unwrap();
    let wire = client_thread.join().unwrap();
    assert_eq!(response.status(), 404);
    assert!(wire.contains("HTTP/1.1 404"));
    assert!(wire.contains("application/problem+json"));
}

#[test]
fn missing_tables_map_store_failures_to_safe_problem_details() {
    let _guard = write_test_guard();
    let mut client = test_client();
    reset_tables(&mut client);

    let mut runtime = AccountLinkHttpRuntime::new(10_400);
    let mut transaction = client.transaction().unwrap();
    let missing_store = handle_account_link_http_request(
        &persist_request(&persist_body(), "idem_missing_store"),
        &mut runtime,
        &mut transaction,
    );
    transaction.rollback().unwrap();
    assert_eq!(missing_store.status(), 500);
    assert!(missing_store
        .body()
        .contains("urn:psychometrics-commons:problem:account-link-store"));
}
