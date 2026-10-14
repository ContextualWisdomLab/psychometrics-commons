//! Architectural contract for one session-command HTTP framing owner.
//!
//! The public module is wired through `session_command_http_boundary.rs`.
//! Application semantics therefore must not retain a second socket
//! accept/read/write loop in `session_command_http.rs`, where framing can
//! silently diverge from the hardened boundary.

#[test]
fn session_command_application_does_not_own_a_second_socket_framing_loop() {
    let application = include_str!("../src/session_command_http.rs");
    let boundary = include_str!("../src/session_command_http_boundary.rs");

    for forbidden in [
        "pub fn accept_one_session_command_http(",
        "fn read_http_request(",
        "fn apply_request_read(",
        "fn write_http_response(",
        "enum RequestReadProgress",
    ] {
        assert!(
            !application.contains(forbidden),
            "session_command_http.rs must not own socket framing helper `{forbidden}`; keep framing in session_command_http_boundary.rs"
        );
    }

    assert!(
        boundary.contains("pub fn accept_one_session_command_http("),
        "session_command_http_boundary.rs must remain the public socket framing owner"
    );
}
