//! Contract for method-allow metadata on the in-process response HTTP surface.

use psychometrics_commons_runtime::response_http::{
    handle_response_http_request, ResponseHttpRuntime,
};

#[test]
fn method_rejection_exposes_allow_post_to_embedding_hosts() {
    let mut runtime = ResponseHttpRuntime::new(vec![], vec![], "evt_response_contract_001");
    let response = handle_response_http_request(
        "GET /v1/sessions/ses_allow_contract/responses HTTP/1.1\r\nHost: example.test\r\n\r\n",
        &mut runtime,
    );

    assert_eq!(response.status(), 405);
    assert_eq!(response.allow(), Some("POST"));
}

#[test]
fn ordinary_problem_response_does_not_advertise_allow() {
    let mut runtime = ResponseHttpRuntime::new(vec![], vec![], "evt_response_contract_002");
    let response = handle_response_http_request("GARBAGE\r\n\r\n", &mut runtime);

    assert_eq!(response.status(), 400);
    assert_eq!(response.allow(), None);
}
