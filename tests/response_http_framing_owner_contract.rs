//! Architectural contract for one response-HTTP framing owner.
//!
//! The public module is wired through `response_http_boundary.rs`. Application
//! behavior therefore must not retain a second socket accept/read/write loop in
//! `response_http.rs`, where it can silently diverge from the hardened boundary.

#[test]
fn response_http_application_does_not_own_a_second_socket_framing_loop() {
    let application = include_str!("../src/response_http.rs");
    let boundary = include_str!("../src/response_http_boundary.rs");

    for forbidden in [
        "pub fn accept_one_response_http(",
        "fn read_http_request(",
        "fn apply_request_read(",
        "fn write_http_response(",
        "enum RequestReadProgress",
    ] {
        assert!(
            !application.contains(forbidden),
            "response_http.rs must not own socket framing helper `{forbidden}`; keep framing in response_http_boundary.rs"
        );
    }

    assert!(
        boundary.contains("pub fn accept_one_response_http("),
        "response_http_boundary.rs must remain the public socket framing owner"
    );
}
