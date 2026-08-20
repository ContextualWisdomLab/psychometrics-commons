//! Public session HTTP must read the declared request body before dispatch.

use psychometrics_commons_runtime::postgres_assessment_session::{
    AssessmentSessionPersistenceError, AssessmentSessionStartError,
};
use psychometrics_commons_runtime::session_http::{
    accept_one_session_http, bind_session_http, handle_session_http_request, MemorySessionHttpPort,
    SESSION_COLLECTION_PATH,
};
use std::io::{Read, Write};
use std::net::{Shutdown, SocketAddr, TcpStream};
use std::time::Duration;

const PARTICIPANT: &str = "ptc_eb1b318917d24ca0ac5153c37ff696c7";
const RELEASE: &str = "release_big_five_ko_v1";
const SESSION: &str = "ses_fragmented_body_3f6aa7882cf94ec6a1a561328d24bace";

#[test]
fn listener_waits_for_a_fragmented_content_length_body() {
    let listener = bind_session_http(SocketAddr::from(([127, 0, 0, 1], 0))).unwrap();
    let address = listener.local_addr().unwrap();
    let body = format!(
        "{{\"participant_ref\":\"{PARTICIPANT}\",\"instrument_release_ref\":\"{RELEASE}\",\"locale\":\"ko-KR\"}}"
    );
    let headers = format!(
        "POST {SESSION_COLLECTION_PATH} HTTP/1.1\r\n\
         Host: assessment.example\r\n\
         Idempotency-Key: {SESSION}\r\n\
         Content-Type: application/json\r\n\
         Content-Length: {}\r\n\
         \r\n",
        body.len()
    );

    let server = std::thread::spawn(move || {
        let mut port = MemorySessionHttpPort::published();
        accept_one_session_http(&listener, &mut port, 20_000).unwrap();
    });
    let mut stream = TcpStream::connect(address).unwrap();
    stream.write_all(headers.as_bytes()).unwrap();
    stream.flush().unwrap();
    std::thread::sleep(Duration::from_millis(150));
    stream.write_all(body.as_bytes()).unwrap();
    stream.shutdown(Shutdown::Write).unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    server.join().unwrap();
    assert!(
        response.starts_with("HTTP/1.1 201 Created\r\n"),
        "fragmented request body must be read before dispatch: {response}"
    );
}

fn framing_error(request: &[u8]) -> std::io::ErrorKind {
    let listener = bind_session_http(SocketAddr::from(([127, 0, 0, 1], 0))).unwrap();
    let address = listener.local_addr().unwrap();
    let payload = request.to_vec();
    let client = std::thread::spawn(move || {
        let mut stream = TcpStream::connect(address).unwrap();
        stream.write_all(&payload).unwrap();
        // The server may reject malformed framing before the client half-closes.
        let _ = stream.shutdown(Shutdown::Write);
        let mut response = String::new();
        let _ = stream.read_to_string(&mut response);
    });
    let mut port = MemorySessionHttpPort::published();
    let error = accept_one_session_http(&listener, &mut port, 20_000).unwrap_err();
    client.join().unwrap();
    error.kind()
}

fn response_for(request: &str, mut port: MemorySessionHttpPort) -> String {
    let listener = bind_session_http(SocketAddr::from(([127, 0, 0, 1], 0))).unwrap();
    let address = listener.local_addr().unwrap();
    let request = request.as_bytes().to_vec();
    let server = std::thread::spawn(move || accept_one_session_http(&listener, &mut port, 20_000));

    let mut stream = TcpStream::connect(address).unwrap();
    stream.write_all(&request).unwrap();
    stream.shutdown(Shutdown::Write).unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    server.join().unwrap().unwrap();
    response
}

#[test]
fn listener_rejects_invalid_and_oversized_content_length() {
    assert_eq!(
        framing_error(
            b"POST /v1/sessions HTTP/1.1\r\nIdempotency-Key: ses_badlen\r\nContent-Length: no\r\n\r\n{}"
        ),
        std::io::ErrorKind::InvalidData
    );
    let oversized = format!(
        "POST /v1/sessions HTTP/1.1\r\nIdempotency-Key: ses_huge\r\nContent-Length: 20000\r\n\r\n{}",
        "x".repeat(100)
    );
    assert_eq!(
        framing_error(oversized.as_bytes()),
        std::io::ErrorKind::InvalidData
    );
    let huge_header = format!(
        "POST /v1/sessions HTTP/1.1\r\nX-Pad: {}\r\n\r\n",
        "a".repeat(9000)
    );
    assert_eq!(
        framing_error(huge_header.as_bytes()),
        std::io::ErrorKind::InvalidData
    );
    assert_eq!(
        framing_error(
            b"POST /v1/sessions HTTP/1.1\r\nIdempotency-Key: ses_overflow\r\nContent-Length: 18446744073709551615\r\n\r\n"
        ),
        std::io::ErrorKind::InvalidData
    );
    assert_eq!(
        framing_error(
            b"POST /v1/sessions HTTP/1.1\r\nIdempotency-Key: ses_bad_utf8\r\nX-Bad: \xff\r\n\r\n"
        ),
        std::io::ErrorKind::InvalidData
    );
    assert_eq!(
        framing_error(
            b"POST /v1/sessions HTTP/1.1\r\nIdempotency-Key: ses_bad_body\r\nContent-Length: 2\r\n\r\n\xff\xfe"
        ),
        std::io::ErrorKind::InvalidData
    );
}

#[test]
fn listener_rejects_ambiguous_http_message_framing() {
    assert_eq!(
        framing_error(
            b"POST /v1/sessions HTTP/1.1\r\nIdempotency-Key: ses_chunked\r\nTransfer-Encoding: chunked\r\n\r\n2\r\n{}\r\n0\r\n\r\n"
        ),
        std::io::ErrorKind::InvalidData
    );
    assert_eq!(
        framing_error(
            b"POST /v1/sessions HTTP/1.1\r\nIdempotency-Key: ses_both\r\nTransfer-Encoding: chunked\r\nContent-Length: 2\r\n\r\n{}"
        ),
        std::io::ErrorKind::InvalidData
    );
    assert_eq!(
        framing_error(
            b"POST /v1/sessions HTTP/1.1\r\nIdempotency-Key: ses_duplicate\r\nContent-Length: 2\r\nContent-Length: 3\r\n\r\n{}x"
        ),
        std::io::ErrorKind::InvalidData
    );
    assert_eq!(
        framing_error(
            b"POST /v1/sessions HTTP/1.1\r\nIdempotency-Key: ses_duplicate_same\r\nContent-Length: 2\r\nContent-Length: 2\r\n\r\n{}"
        ),
        std::io::ErrorKind::InvalidData
    );
    assert_eq!(
        framing_error(
            b"POST /v1/sessions HTTP/1.1\r\nIdempotency-Key: ses_duplicate_a\r\nIdempotency-Key: ses_duplicate_b\r\n\r\n"
        ),
        std::io::ErrorKind::InvalidData
    );
}

#[test]
fn listener_writes_problem_reason_phrases_for_public_error_classes() {
    let not_found = response_for(
        "GET /v1/unknown HTTP/1.1\r\n\r\n",
        MemorySessionHttpPort::published(),
    );
    assert!(not_found.starts_with("HTTP/1.1 404 Not Found\r\n"));

    let method_not_allowed = response_for(
        "DELETE /v1/sessions HTTP/1.1\r\n\r\n",
        MemorySessionHttpPort::published(),
    );
    assert!(method_not_allowed.starts_with("HTTP/1.1 405 Method Not Allowed\r\n"));

    let body = format!(
        "{{\"participant_ref\":\"{PARTICIPANT}\",\"instrument_release_ref\":\"{RELEASE}\",\"locale\":\"ko-KR\"}}"
    );
    let conflict_request = format!(
        "POST {SESSION_COLLECTION_PATH} HTTP/1.1\r\nIdempotency-Key: ses_unpublished_reason\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );
    let mut unpublished = MemorySessionHttpPort::published();
    unpublished.published = false;
    let conflict = response_for(&conflict_request, unpublished);
    assert!(conflict.starts_with("HTTP/1.1 409 Conflict\r\n"));

    let mut unavailable = MemorySessionHttpPort::published();
    unavailable.next_load_error = Some(AssessmentSessionPersistenceError::InvalidStoredIdentity);
    let server_error = response_for(
        "GET /v1/sessions/ses_reason_failure HTTP/1.1\r\n\r\n",
        unavailable,
    );
    assert!(server_error.starts_with("HTTP/1.1 500 Internal Server Error\r\n"));
}

#[test]
fn listener_reloads_a_created_session_over_get() {
    let mut port = MemorySessionHttpPort::published();
    let body = format!(
        "{{\"participant_ref\":\"{PARTICIPANT}\",\"instrument_release_ref\":\"{RELEASE}\",\"locale\":\"ko-KR\"}}"
    );
    let created = handle_session_http_request(
        &format!(
            "POST {SESSION_COLLECTION_PATH} HTTP/1.1\r\n\
             Idempotency-Key: {SESSION}\r\n\
             Content-Length: {}\r\n\
             \r\n{body}",
            body.len()
        ),
        &mut port,
        20_000,
    );
    assert_eq!(created.status(), 201);

    let listener = bind_session_http(SocketAddr::from(([127, 0, 0, 1], 0))).unwrap();
    let address = listener.local_addr().unwrap();
    let reload = format!("GET {SESSION_COLLECTION_PATH}/{SESSION} HTTP/1.1\r\n\r\n");
    let server = std::thread::spawn(move || {
        accept_one_session_http(&listener, &mut port, 20_000).unwrap();
        port
    });
    let mut stream = TcpStream::connect(address).unwrap();
    stream.write_all(reload.as_bytes()).unwrap();
    stream.shutdown(Shutdown::Write).unwrap();
    let mut reload_response = String::new();
    stream.read_to_string(&mut reload_response).unwrap();
    server.join().unwrap();
    assert!(
        reload_response.starts_with("HTTP/1.1 200 OK\r\n"),
        "GET must reload the session created on the same port: {reload_response}"
    );
    assert_eq!(
        AssessmentSessionStartError::from(AssessmentSessionPersistenceError::ConflictingReplay)
            .to_string(),
        "session start could not persist the created session; retry the exact start or repair the store"
    );
}

#[test]
fn listener_finishes_a_header_stream_closed_before_the_terminator() {
    let listener = bind_session_http(SocketAddr::from(([127, 0, 0, 1], 0))).unwrap();
    let address = listener.local_addr().unwrap();
    let client = std::thread::spawn(move || {
        let mut stream = TcpStream::connect(address).unwrap();
        stream
            .write_all(b"POST /v1/sessions HTTP/1.1\r\nIdempotency-Key: ses_eof")
            .unwrap();
        stream.shutdown(Shutdown::Write).unwrap();
        let mut response = String::new();
        let _ = stream.read_to_string(&mut response);
    });
    let mut port = MemorySessionHttpPort::published();
    let _ = accept_one_session_http(&listener, &mut port, 20_000);
    client.join().unwrap();
}
