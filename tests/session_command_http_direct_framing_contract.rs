//! Direct-handler framing regressions for session-command HTTP.
//!
//! Socket framing is owned by the hardened boundary, but the exported direct
//! handler must still fail closed rather than panic or read header identities
//! from body lines when it is exercised in-process.

use psychometrics_commons_runtime::session_command_http::{
    handle_session_command_http_request, SessionCommandHttpRuntime,
};

#[test]
fn direct_handler_rejects_content_length_that_splits_utf8() {
    let mut runtime = SessionCommandHttpRuntime::new(Vec::new());
    let request = concat!(
        "POST /v1/sessions/ses_direct_utf8/commands HTTP/1.1\r\n",
        "Host: localhost\r\n",
        "Idempotency-Key: idem_direct_utf8\r\n",
        "Content-Type: application/json\r\n",
        "Content-Length: 1\r\n",
        "\r\n",
        "é"
    );

    let response = handle_session_command_http_request(request, &mut runtime);

    assert_eq!(response.status(), 400);
    assert!(response
        .body()
        .contains("urn:psychometrics-commons:problem:bad-request"));
}

#[test]
fn body_lines_cannot_supply_the_idempotency_header() {
    let mut runtime = SessionCommandHttpRuntime::new(Vec::new());
    let body = "Idempotency-Key: idem_body_only";
    let request = format!(
        "POST /v1/sessions/ses_direct_header/commands HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );

    let response = handle_session_command_http_request(&request, &mut runtime);

    assert_eq!(response.status(), 400);
    assert!(response
        .body()
        .contains("urn:psychometrics-commons:problem:missing-idempotency-key"));
}
