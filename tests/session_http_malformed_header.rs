//! Session HTTP framing rejects malformed field lines before application dispatch.

use psychometrics_commons_runtime::session_http::{
    accept_one_session_http, bind_session_http, MemorySessionHttpPort,
};
use std::io::{Read, Write};
use std::net::{Shutdown, SocketAddr, TcpStream};

#[test]
fn listener_rejects_header_field_without_a_colon() {
    let listener = bind_session_http(SocketAddr::from(([127, 0, 0, 1], 0))).unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let mut port = MemorySessionHttpPort::published();
        accept_one_session_http(&listener, &mut port, 20_000)
    });

    let mut stream = TcpStream::connect(address).unwrap();
    stream
        .write_all(
            b"POST /v1/sessions HTTP/1.1\r\nIdempotency-Key: ses_bad_field\r\nBroken Header\r\nContent-Length: 2\r\n\r\n{}",
        )
        .unwrap();
    stream.shutdown(Shutdown::Write).unwrap();
    let mut response = String::new();
    let _ = stream.read_to_string(&mut response);

    assert_eq!(
        server.join().unwrap().unwrap_err().kind(),
        std::io::ErrorKind::InvalidData
    );
}
