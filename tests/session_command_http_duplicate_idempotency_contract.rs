//! Duplicate session-command identity headers fail before application dispatch.
//!
//! `Idempotency-Key` is one opaque client-command identity. Accepting more than
//! one value would let an intermediary and this process select different replay
//! identities, so the public socket boundary must reject the frame itself.

use psychometrics_commons_runtime::session_command_http::{
    accept_one_session_command_http, bind_session_command_http, SessionCommandHttpRuntime,
};
use std::io::{self, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpStream};
use std::time::Duration;

#[test]
fn duplicate_idempotency_key_is_rejected_before_session_lookup() {
    let listener = bind_session_command_http(SocketAddr::from(([127, 0, 0, 1], 0))).unwrap();
    let server_listener = listener.try_clone().unwrap();
    let server = std::thread::spawn(move || {
        let mut runtime = SessionCommandHttpRuntime::new(Vec::new());
        accept_one_session_command_http(&server_listener, &mut runtime)
    });

    let body = "{\"command\":\"activate\"}";
    let request = format!(
        "POST /v1/sessions/ses_duplicate_command_identity/commands HTTP/1.1\r\nHost: localhost\r\nIdempotency-Key: cmd_first_opaque\r\nIdempotency-Key: cmd_second_opaque\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
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
