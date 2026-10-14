//! Response HTTP framing parity for direct in-process callers.
//!
//! The pure handler is a public embedding boundary, so it must reject transfer
//! framing that the hardened socket boundary rejects rather than interpreting a
//! conflicting `Content-Length` itself.

use psychometrics_commons_runtime::response_http::{
    handle_response_http_request, ResponseHttpRuntime,
};

#[test]
fn direct_handler_rejects_transfer_encoding_even_with_content_length() {
    let body = "{\"item_version_ref\":\"item_version_001\",\"payload_digest\":\"sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"}";
    let request = format!(
        "POST /v1/sessions/ses_transfer_encoding/responses HTTP/1.1\r\nHost: localhost\r\nIdempotency-Key: idem_transfer_encoding\r\nTransfer-Encoding: chunked\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );
    let mut runtime = ResponseHttpRuntime::new(Vec::new(), Vec::new(), "evt_seed");

    let response = handle_response_http_request(&request, &mut runtime);

    assert_eq!(response.status(), 400);
    assert_eq!(response.content_type(), "application/problem+json");
    assert!(response
        .body()
        .contains("urn:psychometrics-commons:problem:bad-request"));
    assert!(!response
        .body()
        .contains("urn:psychometrics-commons:problem:session-not-found"));
}
