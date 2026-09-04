//! Regression for distinguishing a present-but-invalid response idempotency key.

use psychometrics_commons_runtime::response_http::{
    handle_response_http_request, ResponseHttpRuntime,
};

#[test]
fn numeric_idempotency_key_is_invalid_not_missing() {
    let body = concat!(
        "{\"item_version_ref\":\"item_version_001\",",
        "\"payload_digest\":\"sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"}"
    );
    let request = format!(
        "POST /v1/sessions/ses_invalid_key_contract/responses HTTP/1.1\r\nIdempotency-Key: 42\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );
    let mut runtime = ResponseHttpRuntime::new(Vec::new(), Vec::new(), "evt_invalid_key_contract");

    let response = handle_response_http_request(&request, &mut runtime);

    assert_eq!(response.status(), 400);
    assert_eq!(response.content_type(), "application/problem+json");
    assert!(response
        .body()
        .contains("urn:psychometrics-commons:problem:invalid-reference"));
    assert!(response.body().contains("Invalid Reference"));
    assert!(!response.body().contains("Missing Idempotency Key"));
}
