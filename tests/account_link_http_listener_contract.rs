//! Listener contract for public account-link HTTP.
//!
//! Operators bind an ephemeral local port in tests and `0.0.0.0:$PORT` in
//! hosted processes. This slice proves the listener binds and that a fail-closed
//! request never requires a product-store transaction.

use psychometrics_commons_runtime::account_link_http::{
    bind_account_link_http, classify_account_link_http_request, AccountLinkHttpClassification,
};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::thread;
use std::time::Duration;

#[test]
fn bind_listens_and_a_bad_request_fails_closed_before_the_store() {
    let listener = bind_account_link_http("127.0.0.1:0".parse::<SocketAddr>().unwrap())
        .expect("account-link HTTP must bind an ephemeral local port");
    let address = listener.local_addr().unwrap();
    assert_ne!(address.port(), 0);

    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut buffer = [0_u8; 512];
        let read = stream.read(&mut buffer).unwrap();
        let request = String::from_utf8_lossy(&buffer[..read]).into_owned();
        let AccountLinkHttpClassification::Ready(response) =
            classify_account_link_http_request(&request)
        else {
            panic!("a request without an HTTP target must not reach persist");
        };
        assert_eq!(response.status(), 400);
        let body = response.body().as_bytes();
        let header = format!(
            "HTTP/1.1 400 Bad Request\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            response.content_type(),
            body.len()
        );
        stream.write_all(header.as_bytes()).unwrap();
        stream.write_all(body).unwrap();
    });

    let mut client = TcpStream::connect(address).unwrap();
    client.write_all(b"NOT-A-REQUEST").unwrap();
    let mut received = String::new();
    client.read_to_string(&mut received).unwrap();
    assert!(received.starts_with("HTTP/1.1 400 "));
    assert!(received.contains("application/problem+json"));
    server.join().unwrap();
}
