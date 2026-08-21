//! Contract tests for operator liveness and readiness HTTP probes.
//!
//! These probes are the first implemented HTTP surface. They expose the existing
//! domain health snapshot without inventing SLO values or leaking raw store errors.

use psychometrics_commons_runtime::health::{
    BacklogHealth, CapabilityHealth, CapabilityState, DataIntegrityHealth, RuntimeHealthSnapshot,
};
use psychometrics_commons_runtime::health_http::{
    handle_health_http_request, HEALTH_LIVE_PATH, HEALTH_READY_PATH,
};
use std::fs;
use std::path::PathBuf;

fn healthy_snapshot() -> RuntimeHealthSnapshot {
    RuntimeHealthSnapshot::new(
        true,
        BacklogHealth::WithinBounds,
        DataIntegrityHealth::Verified,
        vec![
            CapabilityHealth::new("scoring", CapabilityState::Available, true).unwrap(),
            CapabilityHealth::new("authenticated_linking", CapabilityState::Unavailable, false)
                .unwrap(),
        ],
    )
    .unwrap()
}

fn request(method: &str, target: &str) -> String {
    format!("{method} {target} HTTP/1.1\r\nHost: localhost\r\n\r\n")
}

#[test]
fn liveness_probe_is_independent_from_operation_readiness() {
    let live = handle_health_http_request(&request("GET", HEALTH_LIVE_PATH), &healthy_snapshot());
    assert_eq!(live.status(), 200);
    assert_eq!(live.content_type(), "application/json");
    assert!(live.body().contains("\"live\":true"));
    assert!(live.body().contains("\"ready\":true"));

    let not_live = RuntimeHealthSnapshot::new(
        false,
        BacklogHealth::WithinBounds,
        DataIntegrityHealth::Verified,
        vec![],
    )
    .unwrap();
    let response = handle_health_http_request(&request("GET", HEALTH_LIVE_PATH), &not_live);
    assert_eq!(response.status(), 503);
    assert!(response.body().contains("\"live\":false"));
    assert!(response.body().contains("\"ready\":false"));
}

#[test]
fn readiness_probe_fails_closed_for_named_or_unknown_required_capabilities() {
    let snapshot = healthy_snapshot();
    let ready = handle_health_http_request(&request("GET", HEALTH_READY_PATH), &snapshot);
    assert_eq!(ready.status(), 200);
    assert!(ready.body().contains("\"ready\":true"));

    let scoring =
        handle_health_http_request(&request("GET", "/ready?capability=scoring"), &snapshot);
    assert_eq!(scoring.status(), 200);

    let linking = handle_health_http_request(
        &request("GET", "/ready?capability=authenticated_linking"),
        &snapshot,
    );
    assert_eq!(linking.status(), 503);
    assert!(linking.body().contains("\"ready\":false"));
    assert!(linking
        .body()
        .contains("\"capability_ref\":\"authenticated_linking\""));

    let unknown = handle_health_http_request(
        &request("GET", "/ready?capability=unregistered_capability"),
        &snapshot,
    );
    assert_eq!(unknown.status(), 503);
}

#[test]
fn stalled_backlog_or_unknown_integrity_makes_readiness_unavailable() {
    let stalled = RuntimeHealthSnapshot::new(
        true,
        BacklogHealth::Stalled,
        DataIntegrityHealth::Verified,
        vec![],
    )
    .unwrap();
    let stalled_response = handle_health_http_request(&request("GET", HEALTH_READY_PATH), &stalled);
    assert_eq!(stalled_response.status(), 503);
    assert!(stalled_response
        .body()
        .contains("\"backlog_health\":\"stalled\""));

    let unknown_integrity = RuntimeHealthSnapshot::new(
        true,
        BacklogHealth::WithinBounds,
        DataIntegrityHealth::Unknown,
        vec![],
    )
    .unwrap();
    let integrity_response =
        handle_health_http_request(&request("GET", HEALTH_READY_PATH), &unknown_integrity);
    assert_eq!(integrity_response.status(), 503);
    assert!(integrity_response
        .body()
        .contains("\"data_integrity_health\":\"unknown\""));
}

#[test]
fn unsupported_method_or_path_returns_safe_problem_details() {
    let snapshot = healthy_snapshot();
    let not_allowed = handle_health_http_request(&request("POST", HEALTH_LIVE_PATH), &snapshot);
    assert_eq!(not_allowed.status(), 405);
    assert_eq!(not_allowed.content_type(), "application/problem+json");
    assert!(not_allowed
        .body()
        .contains("\"title\":\"Method Not Allowed\""));
    assert!(not_allowed
        .body()
        .contains("\"type\":\"urn:psychometrics-commons:problem:method-not-allowed\""));
    assert!(!not_allowed.body().contains("about:blank"));
    assert!(!not_allowed.body().contains("postgres"));
    assert!(!not_allowed.body().contains("sql"));

    let missing = handle_health_http_request(&request("GET", "/v1/sessions"), &snapshot);
    assert_eq!(missing.status(), 404);
    assert_eq!(missing.content_type(), "application/problem+json");
    assert!(missing.body().contains("\"title\":\"Not Found\""));
    assert!(missing
        .body()
        .contains("\"type\":\"urn:psychometrics-commons:problem:not-found\""));
    assert!(!missing.body().contains("/v1/instruments"));
}

#[test]
fn malformed_request_fails_closed_without_echoing_raw_input() {
    let response = handle_health_http_request("NOT-A-REQUEST", &healthy_snapshot());
    assert_eq!(response.status(), 400);
    assert_eq!(response.content_type(), "application/problem+json");
    assert!(response.body().contains("\"title\":\"Bad Request\""));
    assert!(response
        .body()
        .contains("\"type\":\"urn:psychometrics-commons:problem:bad-request\""));
    assert!(!response.body().contains("NOT-A-REQUEST"));
    assert!(!response.body().contains("about:blank"));
}

#[test]
fn as_built_openapi_lists_only_implemented_health_probe_operations() {
    let openapi_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("openapi/health-probes.yaml");
    let openapi = fs::read_to_string(openapi_path).expect("as-built health OpenAPI must exist");
    assert!(openapi.contains("openapi: 3.2.0"));
    assert!(openapi.contains(HEALTH_LIVE_PATH));
    assert!(openapi.contains(HEALTH_READY_PATH));
    assert!(openapi.contains("urn:psychometrics-commons:problem:bad-request"));
    assert!(!openapi.contains("about:blank"));
    assert!(!openapi.contains("/v1/sessions"));
    assert!(!openapi.contains("/v1/instruments"));
    let live_section = openapi
        .split("/ready:")
        .next()
        .expect("as-built OpenAPI must describe /live before /ready");
    assert!(
        live_section.contains("\"503\""),
        "GET /live must document HTTP 503 when the process is not live"
    );
}
