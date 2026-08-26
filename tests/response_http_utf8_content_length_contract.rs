//! Direct-handler regressions for strict byte framing.
//!
//! The socket boundary rejects malformed frames before dispatch, but the public
//! pure handler is also used by contract tests and in-process adapters. It must
//! reject byte Content-Length values that split UTF-8 scalars and must not ignore
//! trailing bytes beyond the one declared request body.

use psychometrics_commons_runtime::response_http::{
    handle_response_http_request, ResponseHttpRuntime,
};

#[test]
fn direct_handler_rejects_content_length_that_splits_a_utf8_scalar() {
    let body = concat!(
        "{\"item_version_ref\":\"item_한\",",
        "\"payload_digest\":\"sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"}"
    );
    let split_at = body.find('한').unwrap() + 1;
    assert!(!body.is_char_boundary(split_at));

    let request = format!(
        "POST /v1/sessions/ses_utf8_contract/responses HTTP/1.1\r\nIdempotency-Key: idem_utf8_contract\r\nContent-Length: {split_at}\r\n\r\n{body}"
    );
    let mut runtime = ResponseHttpRuntime::new(Vec::new(), Vec::new(), "evt_utf8_contract");

    let response = handle_response_http_request(&request, &mut runtime);

    assert_eq!(response.status(), 400);
    assert_eq!(response.content_type(), "application/problem+json");
    assert!(response
        .body()
        .contains("urn:psychometrics-commons:problem:bad-request"));
}

#[test]
fn direct_handler_rejects_bytes_beyond_declared_content_length() {
    let body = concat!(
        "{\"item_version_ref\":\"item_one\",",
        "\"payload_digest\":\"sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"}"
    );
    let request = format!(
        "POST /v1/sessions/ses_trailing_contract/responses HTTP/1.1\r\nIdempotency-Key: idem_trailing_contract\r\nContent-Length: {}\r\n\r\n{body}EXTRA",
        body.len()
    );
    let mut runtime = ResponseHttpRuntime::new(Vec::new(), Vec::new(), "evt_trailing_contract");

    let response = handle_response_http_request(&request, &mut runtime);

    assert_eq!(response.status(), 400);
    assert_eq!(response.content_type(), "application/problem+json");
    assert!(response
        .body()
        .contains("urn:psychometrics-commons:problem:bad-request"));
}
