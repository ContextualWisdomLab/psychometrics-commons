//! Public session HTTP must read the declared request body before dispatch.

use psychometrics_commons_runtime::session_http::{
    accept_one_session_http, bind_session_http, MemorySessionHttpPort, SESSION_COLLECTION_PATH,
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

    let client = std::thread::spawn(move || {
        let mut stream = TcpStream::connect(address).unwrap();
        stream.write_all(headers.as_bytes()).unwrap();
        stream.flush().unwrap();
        std::thread::sleep(Duration::from_millis(100));
        stream.write_all(body.as_bytes()).unwrap();
        stream.shutdown(Shutdown::Write).unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        response
    });

    let mut port = MemorySessionHttpPort::published();
    accept_one_session_http(&listener, &mut port, 20_000).unwrap();
    let response = client.join().unwrap();
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
        stream.shutdown(Shutdown::Write).unwrap();
        let mut response = String::new();
        let _ = stream.read_to_string(&mut response);
    });
    let mut port = MemorySessionHttpPort::published();
    let error = accept_one_session_http(&listener, &mut port, 20_000).unwrap_err();
    client.join().unwrap();
    error.kind()
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
