//! Direct response-handler header hygiene must match the hardened socket boundary.
//!
//! In-process embedding adapters can call the pure handler without traversing the
//! TCP listener. They must not therefore gain a looser header grammar: bare-LF
//! framing, invalid HTTP field names, and colon-less header fields all fail closed
//! before authorization or product-state lookup.

use psychometrics_commons_runtime::response_http::{
    handle_response_http_request, ResponseHttpRuntime,
};

const SESSION_REF: &str = "ses_direct_header_hygiene";
const BODY: &str = "{\"item_version_ref\":\"item_version_header_hygiene\",\"payload_digest\":\"sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"}";

fn runtime() -> ResponseHttpRuntime {
    ResponseHttpRuntime::new(Vec::new(), Vec::new(), "evt_direct_header_hygiene")
}

fn strict_request() -> String {
    format!(
        "POST /v1/sessions/{SESSION_REF}/responses HTTP/1.1\r\nHost: localhost\r\nIdempotency-Key: idem_direct_header_hygiene\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{BODY}",
        BODY.len()
    )
}

#[test]
fn strict_header_shape_reaches_the_authority_and_session_gate() {
    let response = handle_response_http_request(&strict_request(), &mut runtime());

    assert_eq!(response.status(), 404);
    assert!(response
        .body()
        .contains("urn:psychometrics-commons:problem:session-not-found"));
}

#[test]
fn bare_lf_header_framing_is_rejected_before_product_lookup() {
    let request = format!(
        "POST /v1/sessions/{SESSION_REF}/responses HTTP/1.1\nHost: localhost\nIdempotency-Key: idem_direct_header_hygiene\nContent-Type: application/json\nContent-Length: {}\n\n{BODY}",
        BODY.len()
    );
    let response = handle_response_http_request(&request, &mut runtime());

    assert_eq!(response.status(), 400);
    assert!(response
        .body()
        .contains("urn:psychometrics-commons:problem:bad-request"));
}

#[test]
fn invalid_or_colonless_header_fields_are_rejected_before_product_lookup() {
    for extra_header in ["Bad Header: value", "HeaderWithoutColon"] {
        let request = format!(
            "POST /v1/sessions/{SESSION_REF}/responses HTTP/1.1\r\nHost: localhost\r\n{extra_header}\r\nIdempotency-Key: idem_direct_header_hygiene\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{BODY}",
            BODY.len()
        );
        let response = handle_response_http_request(&request, &mut runtime());

        assert_eq!(response.status(), 400, "{extra_header}");
        assert!(response
            .body()
            .contains("urn:psychometrics-commons:problem:bad-request"));
    }
}
