//! RFC 9110 method-rejection contract for the public instrument catalog.
//!
//! A 405 response must expose the allowed method to both in-process embedding
//! hosts and the HTTP/1.1 wire serializer. Ordinary responses must not invent an
//! `Allow` field.

use psychometrics_commons_runtime::instrument_http::{
    accept_one_instrument_http, bind_instrument_http, handle_instrument_http_request,
    InstrumentHttpRuntime,
};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

#[test]
fn method_rejection_exposes_get_as_the_allowed_method() {
    let runtime = InstrumentHttpRuntime::new(Vec::new());

    let rejected = handle_instrument_http_request(
        "POST /v1/instruments HTTP/1.1\r\nHost: localhost\r\n\r\n",
        &runtime,
    );
    assert_eq!(rejected.status(), 405);
    assert_eq!(rejected.allow(), Some("GET"));

    let listed = handle_instrument_http_request(
        "GET /v1/instruments HTTP/1.1\r\nHost: localhost\r\n\r\n",
        &runtime,
    );
    assert_eq!(listed.status(), 200);
    assert_eq!(listed.allow(), None);
}

#[test]
fn socket_serialization_emits_allow_only_for_405() {
    let listener = bind_instrument_http(SocketAddr::from(([127, 0, 0, 1], 0))).unwrap();
    let address = listener.local_addr().unwrap();
    let runtime = InstrumentHttpRuntime::new(Vec::new());
    let server = std::thread::spawn(move || accept_one_instrument_http(&listener, &runtime));

    let mut client = TcpStream::connect(address).unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    client
        .set_write_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    client
        .write_all(b"POST /v1/instruments HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .unwrap();
    client.shutdown(std::net::Shutdown::Write).unwrap();

    let mut response = String::new();
    client.read_to_string(&mut response).unwrap();
    server.join().unwrap().unwrap();

    assert!(response.starts_with("HTTP/1.1 405 Method Not Allowed"));
    assert!(response.contains("\r\nAllow: GET\r\n"), "{response}");
}
