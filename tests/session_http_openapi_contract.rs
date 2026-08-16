//! As-built `OpenAPI` 3.2.0 coverage for the implemented `POST /v1/sessions` operation.

use std::fs;
use std::path::PathBuf;

fn openapi_document() -> String {
    fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("contracts/openapi-public-v1.yaml"),
    )
    .expect("as-built OpenAPI document must be readable")
}

#[test]
fn as_built_openapi_lists_only_post_v1_sessions() {
    let document = openapi_document();

    assert!(document.starts_with("openapi: \"3.2.0\"") || document.contains("openapi: \"3.2.0\""));
    assert!(document.contains("/v1/sessions"));
    assert!(document.contains("operationId: createSession"));
    assert!(document.contains("application/problem+json"));
    assert!(document.contains("\"201\""));
    assert!(document.contains("session_ref"));
    assert!(document.contains("As-built operations only"));

    for unimplemented in [
        "/v1/instruments",
        "/v1/results",
        "/v1/consents",
        "/v1/research-contributions",
        "/v1/data-rights",
        "GET    /v1/sessions",
    ] {
        assert!(
            !document.contains(unimplemented),
            "as-built OpenAPI must omit unimplemented family {unimplemented}"
        );
    }
}
