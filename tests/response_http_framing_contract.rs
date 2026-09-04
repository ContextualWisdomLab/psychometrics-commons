//! Fail-closed HTTP/1.1 framing contract for public response writes.
//!
//! The application handler is intentionally tested elsewhere. These tests pin
//! the socket boundary so a proxy and this process cannot disagree about where
//! one response-write request ends. No session is installed: a correctly framed
//! request therefore reaches the application and returns 404, while malformed
//! framing must be rejected before application dispatch.

use psychometrics_commons_runtime::response_http::{
    accept_one_response_http, bind_response_http, ResponseHttpRuntime,
};
use std::io::{self, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpStream};
use std::time::Duration;

const SESSION_REF: &str = "ses_framing_contract_opaque";
const IDEMPOTENCY_KEY: &str = "idem_framing_contract_opaque";
const PAYLOAD_DIGEST: &str =
    "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn body() -> String {
    format!(
        "{{\"item_version_ref\":\"item_version_opaque\",\"payload_digest\":\"{PAYLOAD_DIGEST}\"}}"
    )
}

fn empty_runtime() -> ResponseHttpRuntime {
    ResponseHttpRuntime::new(Vec::new(), Vec::new(), "evt_framing_seed")
}

fn connect(listener: &std::net::TcpListener) -> TcpStream {
    let client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    client
        .set_write_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    client
}

#[test]
fn listener_waits_for_the_declared_body_after_headers_arrive_first() {
    let listener = bind_response_http(SocketAddr::from(([127, 0, 0, 1], 0))).unwrap();
    let address_listener = listener.try_clone().unwrap();
    let server = std::thread::spawn(move || {
        let mut runtime = empty_runtime();
        accept_one_response_http(&address_listener, &mut runtime)
    });

    let request_body = body();
    let headers = format!(
        "POST /v1/sessions/{SESSION_REF}/responses HTTP/1.1\r\nHost: localhost\r\nIdempotency-Key: {IDEMPOTENCY_KEY}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
        request_body.len()
    );
    let mut client = connect(&listener);
    client.write_all(headers.as_bytes()).unwrap();
    client.flush().unwrap();

    // Force the server to observe a complete header block before body bytes are
    // available. A header-terminator-only reader will dispatch prematurely.
    std::thread::sleep(Duration::from_millis(50));
    client.write_all(request_body.as_bytes()).unwrap();
    client.shutdown(Shutdown::Write).unwrap();

    let mut response = String::new();
    client.read_to_string(&mut response).unwrap();
    server.join().unwrap().unwrap();

    // The empty runtime means correct framing reaches application dispatch and
    // fails only because this synthetic session does not exist.
    assert!(response.starts_with("HTTP/1.1 404 Not Found"));
    assert!(response.contains("urn:psychometrics-commons:problem:session-not-found"));
}

fn framing_result_and_response(request: &[u8]) -> (io::Result<()>, String) {
    let listener = bind_response_http(SocketAddr::from(([127, 0, 0, 1], 0))).unwrap();
    let address_listener = listener.try_clone().unwrap();
    let server = std::thread::spawn(move || {
        let mut runtime = empty_runtime();
        accept_one_response_http(&address_listener, &mut runtime)
    });

    let mut client = connect(&listener);
    client.write_all(request).unwrap();
    client.shutdown(Shutdown::Write).unwrap();
    let mut response = String::new();
    let _ = client.read_to_string(&mut response);
    (server.join().unwrap(), response)
}

fn framing_result(request: &[u8]) -> io::Result<()> {
    framing_result_and_response(request).0
}

fn request_with_extra_headers(extra_headers: &str, suffix: &str) -> Vec<u8> {
    let request_body = body();
    format!(
        "POST /v1/sessions/{SESSION_REF}/responses HTTP/1.1\r\nHost: localhost\r\nIdempotency-Key: {IDEMPOTENCY_KEY}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n{extra_headers}\r\n{request_body}{suffix}",
        request_body.len()
    )
    .into_bytes()
}

#[test]
fn duplicate_content_length_is_rejected_before_dispatch() {
    let request_body = body();
    let request =
        request_with_extra_headers(&format!("Content-Length: {}\r\n", request_body.len()), "");
    let error = framing_result(&request).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
}

#[test]
fn transfer_encoding_is_rejected_before_dispatch() {
    let request = request_with_extra_headers("Transfer-Encoding: chunked\r\n", "");
    let error = framing_result(&request).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
}

#[test]
fn bytes_after_the_declared_request_are_rejected_before_dispatch() {
    let request = request_with_extra_headers("", "GET /smuggled HTTP/1.1\r\n\r\n");
    let error = framing_result(&request).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
}

#[test]
fn malformed_framing_returns_a_generic_problem_before_connection_close() {
    let request = request_with_extra_headers("Idempotency-Key: idem_duplicate_opaque\r\n", "");
    let (result, response) = framing_result_and_response(&request);
    let error = result.expect_err("duplicate singleton framing must remain rejected");
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    assert!(response.starts_with("HTTP/1.1 400 Bad Request"));
    assert!(response.contains("Content-Type: application/problem+json"));
    assert!(response.contains("urn:psychometrics-commons:problem:bad-request"));
    assert!(!response.contains(IDEMPOTENCY_KEY));
    assert!(!response.contains("idem_duplicate_opaque"));
}
