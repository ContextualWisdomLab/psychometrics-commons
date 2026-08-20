//! Session HTTP framing rejects malformed field lines before application dispatch.

use psychometrics_commons_runtime::session_http::{
    accept_one_session_http, bind_session_http, MemorySessionHttpPort,
};
use std::io::{Read, Write};
use std::net::{Shutdown, SocketAddr, TcpStream};

fn rejected_request_kind(request: &[u8]) -> std::io::ErrorKind {
    let listener = bind_session_http(SocketAddr::from(([127, 0, 0, 1], 0))).unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let mut port = MemorySessionHttpPort::published();
        accept_one_session_http(&listener, &mut port, 20_000)
    });

    let mut stream = TcpStream::connect(address).unwrap();
    stream.write_all(request).unwrap();
    stream.shutdown(Shutdown::Write).unwrap();
    let mut response = String::new();
    let _ = stream.read_to_string(&mut response);

    server.join().unwrap().unwrap_err().kind()
}

#[test]
fn listener_rejects_header_field_without_a_colon() {
    assert_eq!(
        rejected_request_kind(
            b"POST /v1/sessions HTTP/1.1\r\nIdempotency-Key: ses_bad_field\r\nBroken Header\r\nContent-Length: 2\r\n\r\n{}",
        ),
        std::io::ErrorKind::InvalidData
    );
}

#[test]
fn listener_rejects_whitespace_before_a_framing_header_colon() {
    for request in [
        b"POST /v1/sessions HTTP/1.1\r\nIdempotency-Key: ses_bad_space\r\nContent-Length : 2\r\n\r\n{}".as_slice(),
        b"POST /v1/sessions HTTP/1.1\r\nIdempotency-Key: ses_bad_tab\r\nContent-Length\t: 2\r\n\r\n{}".as_slice(),
        b"POST /v1/sessions HTTP/1.1\r\nIdempotency-Key: ses_bad_te_space\r\nTransfer-Encoding : chunked\r\n\r\n0\r\n\r\n".as_slice(),
    ] {
        assert_eq!(
            rejected_request_kind(request),
            std::io::ErrorKind::InvalidData
        );
    }
}

#[test]
fn listener_rejects_obsolete_folded_framing_headers() {
    for request in [
        b"POST /v1/sessions HTTP/1.1\r\nIdempotency-Key: ses_bad_fold_cl\r\n Content-Length: 2\r\n\r\n{}".as_slice(),
        b"POST /v1/sessions HTTP/1.1\r\nIdempotency-Key: ses_bad_fold_te\r\n\tTransfer-Encoding: chunked\r\n\r\n0\r\n\r\n".as_slice(),
    ] {
        assert_eq!(
            rejected_request_kind(request),
            std::io::ErrorKind::InvalidData
        );
    }
}
