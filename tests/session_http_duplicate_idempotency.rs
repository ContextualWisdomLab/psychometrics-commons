//! Session create rejects ambiguous duplicate idempotency identity fields.

use psychometrics_commons_runtime::session_http::{
    handle_session_http_request, MemorySessionHttpPort, SESSION_COLLECTION_PATH,
};

const PARTICIPANT: &str = "ptc_eb1b318917d24ca0ac5153c37ff696c7";
const RELEASE: &str = "release_big_five_ko_v1";
const SESSION: &str = "ses_ipip_ko_duplicate_key";

fn duplicate_request(second_key: &str) -> String {
    format!(
        "POST {SESSION_COLLECTION_PATH} HTTP/1.1\r\n\
         Host: assessment.example\r\n\
         Idempotency-Key: {SESSION}\r\n\
         Idempotency-Key: {second_key}\r\n\
         Content-Type: application/json\r\n\
         \r\n\
         {{\"participant_ref\":\"{PARTICIPANT}\",\"instrument_release_ref\":\"{RELEASE}\",\"locale\":\"ko-KR\"}}"
    )
}

#[test]
fn duplicate_idempotency_key_fields_fail_before_session_creation() {
    for second_key in [SESSION, "ses_ipip_ko_conflicting_key"] {
        let mut port = MemorySessionHttpPort::published();
        let response =
            handle_session_http_request(&duplicate_request(second_key), &mut port, 20_000);

        assert_eq!(response.status(), 400);
        assert_eq!(response.content_type(), "application/problem+json");
        assert!(response.body().contains("invalid-idempotency-key"));
        assert!(response
            .body()
            .contains("exactly one Idempotency-Key header"));

        for session_ref in [SESSION, second_key] {
            let loaded = handle_session_http_request(
                &format!(
                    "GET /v1/sessions/{session_ref} HTTP/1.1\r\nHost: assessment.example\r\n\r\n"
                ),
                &mut port,
                20_000,
            );
            assert_eq!(loaded.status(), 404);
        }
    }
}
