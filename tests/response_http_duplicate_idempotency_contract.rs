//! Duplicate client-event identity headers fail closed before response dispatch.
//!
//! `Idempotency-Key` is a single opaque response-event identity. Accepting two
//! values would let intermediaries and the application disagree about replay
//! identity, so the socket boundary must reject the request before any product
//! state can change.

use psychometrics_commons_runtime::response_http::{
    accept_one_response_http, bind_response_http, ResponseHttpRuntime,
};
use std::io::{self, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpStream};
use std::time::Duration;

const SESSION_REF: &str = "ses_duplicate_idempotency_opaque";
const PAYLOAD_DIGEST: &str =
    "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

#[test]
fn duplicate_idempotency_key_is_rejected_before_dispatch() {
    let listener = bind_response_http(SocketAddr::from(([127, 0, 0, 1], 0))).unwrap();
    let server_listener = listener.try_clone().unwrap();
    let server = std::thread::spawn(move || {
        let mut runtime = ResponseHttpRuntime::new(Vec::new(), Vec::new(), "evt_duplicate_seed");
        accept_one_response_http(&server_listener, &mut runtime)
    });

    let body = format!(
        "{{\"item_version_ref\":\"item_version_opaque\",\"payload_digest\":\"{PAYLOAD_DIGEST}\"}}"
    );
    let request = format!(
        "POST /v1/sessions/{SESSION_REF}/responses HTTP/1.1\r\nHost: localhost\r\nIdempotency-Key: idem_first_opaque\r\nIdempotency-Key: idem_second_opaque\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );

    let mut client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    client.write_all(request.as_bytes()).unwrap();
    client.shutdown(Shutdown::Write).unwrap();
    let _ = client.read_to_end(&mut Vec::new());

    let error = server.join().unwrap().unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
}
