//! Security regressions for the single-request session HTTP/1.1 framing boundary.

use psychometrics_commons_runtime::session_http::{
    accept_one_session_http, bind_session_http, MemorySessionHttpPort,
};
use std::io::{Read, Write};
use std::net::{Shutdown, SocketAddr, TcpStream};
use std::time::{Duration, Instant};

fn framing_error(request: &[u8]) -> std::io::ErrorKind {
    let listener = bind_session_http(SocketAddr::from(([127, 0, 0, 1], 0))).unwrap();
    let address = listener.local_addr().unwrap();
    let payload = request.to_vec();
    let client = std::thread::spawn(move || {
        let mut stream = TcpStream::connect(address).unwrap();
        stream.write_all(&payload).unwrap();
        let _ = stream.shutdown(Shutdown::Write);
        let mut response = String::new();
        let _ = stream.read_to_string(&mut response);
    });
    let mut port = MemorySessionHttpPort::published();
    let error = accept_one_session_http(&listener, &mut port, 20_000).unwrap_err();
    client.join().unwrap();
    error.kind()
}

#[test]
fn listener_rejects_content_length_outside_exact_digit_grammar() {
    assert_eq!(
        framing_error(
            b"POST /v1/sessions HTTP/1.1\r\nIdempotency-Key: ses_signed_length\r\nContent-Length: +2\r\n\r\n{}"
        ),
        std::io::ErrorKind::InvalidData
    );

    let unicode_ows = "POST /v1/sessions HTTP/1.1\r\nIdempotency-Key: ses_unicode_length\r\nContent-Length: \u{2003}2\r\n\r\n{}";
    assert_eq!(
        framing_error(unicode_ows.as_bytes()),
        std::io::ErrorKind::InvalidData
    );
}

#[test]
fn listener_rejects_bytes_beyond_one_framed_request() {
    assert_eq!(
        framing_error(b"GET /v1/sessions/ses_trailing HTTP/1.1\r\n\r\nX"),
        std::io::ErrorKind::InvalidData
    );
    assert_eq!(
        framing_error(
            b"POST /v1/sessions HTTP/1.1\r\nIdempotency-Key: ses_pipeline\r\nContent-Length: 2\r\n\r\n{}GET /v1/sessions/ses_next HTTP/1.1\r\n\r\n"
        ),
        std::io::ErrorKind::InvalidData
    );
}

#[test]
fn listener_rejects_peer_close_before_declared_body_is_complete() {
    let listener = bind_session_http(SocketAddr::from(([127, 0, 0, 1], 0))).unwrap();
    let address = listener.local_addr().unwrap();
    let body = b"{\"participant_ref\":\"ptc_short_body\",\"instrument_release_ref\":\"release_big_five_ko_v1\",\"locale\":\"ko-KR\"}";
    let request = format!(
        "POST /v1/sessions HTTP/1.1\r\nIdempotency-Key: ses_short_body\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
        body.len() + 1,
        std::str::from_utf8(body).unwrap()
    );
    let client = std::thread::spawn(move || {
        let mut stream = TcpStream::connect(address).unwrap();
        stream.write_all(request.as_bytes()).unwrap();
        stream.shutdown(Shutdown::Write).unwrap();
        let mut response = String::new();
        let _ = stream.read_to_string(&mut response);
        response
    });

    let mut port = MemorySessionHttpPort::published();
    let error = accept_one_session_http(&listener, &mut port, 20_000).unwrap_err();
    let response = client.join().unwrap();

    assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
    assert!(response.is_empty());
    assert!(
        port.last_start_locale.is_none(),
        "an incomplete HTTP frame must not reach session creation"
    );
}

#[test]
fn listener_enforces_one_deadline_across_slow_fragmented_reads() {
    let listener = bind_session_http(SocketAddr::from(([127, 0, 0, 1], 0))).unwrap();
    let address = listener.local_addr().unwrap();
    let client = std::thread::spawn(move || {
        let mut stream = TcpStream::connect(address).unwrap();
        for byte in b"GET /v1/" {
            if stream.write_all(&[*byte]).is_err() {
                break;
            }
            let _ = stream.flush();
            std::thread::sleep(Duration::from_millis(300));
        }
        let _ = stream.shutdown(Shutdown::Write);
    });

    let started = Instant::now();
    let mut port = MemorySessionHttpPort::published();
    let error = accept_one_session_http(&listener, &mut port, 20_000).unwrap_err();
    let elapsed = started.elapsed();
    client.join().unwrap();

    assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
    assert!(
        elapsed < Duration::from_secs(4),
        "one connection must not refresh the full read timeout after every byte: {elapsed:?}"
    );
}
