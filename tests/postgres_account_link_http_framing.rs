//! Account-link HTTP must read a declared request body before classification.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::account_link_http::{
    accept_one_account_link_http, bind_account_link_http, AccountLinkHttpRuntime,
    ACCOUNT_LINK_RECOVER_PATH,
};
use psychometrics_commons_runtime::postgres_participant_identity_link::apply_participant_identity_link_migration;
use std::io::{Read, Write};
use std::net::Shutdown;
use std::time::Duration;

fn test_client() -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let mut client = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    client
        .batch_execute(
            "DROP SCHEMA IF EXISTS account_link_http_framing_test CASCADE;\
             CREATE SCHEMA account_link_http_framing_test;\
             SET search_path TO account_link_http_framing_test;",
        )
        .unwrap();
    apply_participant_identity_link_migration(&mut client).unwrap();
    client
}

#[test]
fn listener_waits_for_a_fragmented_content_length_body() {
    let mut client = test_client();
    let listener = bind_account_link_http("127.0.0.1:0".parse().unwrap()).unwrap();
    let address = listener.local_addr().unwrap();
    let body = "{\"tenant_ref\":\"tenant_identity_http\",\"identity_issuer\":\"keyverse_issuer_http\",\"identity_subject_ref\":\"keyverse_subject_http\",\"authenticated_proof_ref\":\"authenticated_proof_http\",\"authenticated_valid_until_unix_ms\":11000}";
    let headers = format!(
        "POST {ACCOUNT_LINK_RECOVER_PATH} HTTP/1.1\r\n\
         Host: localhost\r\n\
         Content-Type: application/json\r\n\
         Content-Length: {}\r\n\
         \r\n",
        body.len()
    );

    let client_thread = std::thread::spawn(move || {
        let mut stream = std::net::TcpStream::connect(address).unwrap();
        stream.write_all(headers.as_bytes()).unwrap();
        stream.flush().unwrap();
        std::thread::sleep(Duration::from_millis(100));
        stream.write_all(body.as_bytes()).unwrap();
        stream.shutdown(Shutdown::Write).unwrap();
        let mut wire = String::new();
        stream.read_to_string(&mut wire).unwrap();
        wire
    });

    let mut runtime = AccountLinkHttpRuntime::new(10_400);
    let mut transaction = client.transaction().unwrap();
    let response = accept_one_account_link_http(&listener, &mut runtime, &mut transaction).unwrap();
    transaction.commit().unwrap();
    let wire = client_thread.join().unwrap();

    assert_eq!(
        response.status(),
        404,
        "complete recover body should classify before lookup"
    );
    assert!(
        wire.starts_with("HTTP/1.1 404 Not Found\r\n"),
        "fragmented request body must be read before classification: {wire}"
    );
}
