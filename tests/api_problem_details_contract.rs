//! Contract tests for the transport-neutral RFC 9457 problem-details boundary.

use psychometrics_commons_runtime::api_problem::{
    ApiProblem, ApiProblemContractError, PROBLEM_JSON_MEDIA_TYPE,
};

const TYPE_URI: &str = "urn:psychometrics-commons:problem:cross-tenant-denied";
const TITLE: &str = "Cross-tenant access denied";
const DETAIL: &str = "The requested resource is outside the authorized tenant.";
const CODE: &str = "cross_tenant_denied";

#[test]
fn typed_problem_exposes_stable_rfc9457_fields_without_runtime_error_text() {
    let problem = ApiProblem::new(TYPE_URI, 403, TITLE, DETAIL, CODE).unwrap();

    assert_eq!(problem.type_uri(), TYPE_URI);
    assert_eq!(problem.status(), 403);
    assert_eq!(problem.title(), TITLE);
    assert_eq!(problem.detail(), DETAIL);
    assert_eq!(problem.code(), CODE);
    assert_eq!(ApiProblem::media_type(), PROBLEM_JSON_MEDIA_TYPE);
    assert_eq!(PROBLEM_JSON_MEDIA_TYPE, "application/problem+json");
}

#[test]
fn problem_status_must_be_an_http_client_or_server_error() {
    for invalid_status in [0, 199, 200, 399, 600, u16::MAX] {
        assert_eq!(
            ApiProblem::new(TYPE_URI, invalid_status, TITLE, DETAIL, CODE),
            Err(ApiProblemContractError::InvalidStatus)
        );
    }

    assert!(ApiProblem::new(TYPE_URI, 400, TITLE, DETAIL, CODE).is_ok());
    assert!(ApiProblem::new(TYPE_URI, 599, TITLE, DETAIL, CODE).is_ok());
}

#[test]
fn problem_type_requires_an_explicit_structurally_valid_product_identifier() {
    for invalid_type in [
        "",
        "about:blank",
        "/problems/denied",
        "http://example.test/problem",
        "https://",
        "https:///problem",
        "https://example.test/%zz",
        "urn:",
        "urn:x:value",
        "urn:example:",
        "urn:example:bad value",
        "urn:example:%zz",
    ] {
        assert_eq!(
            ApiProblem::new(invalid_type, 403, TITLE, DETAIL, CODE),
            Err(ApiProblemContractError::InvalidTypeUri),
            "type URI {invalid_type:?} must fail closed"
        );
    }

    for valid_type in [
        "https://example.test/problems/cross-tenant-denied",
        "https://example.test/problems/denied?version=2#details",
        "urn:example:problem/v1",
        TYPE_URI,
    ] {
        assert!(
            ApiProblem::new(valid_type, 403, TITLE, DETAIL, CODE).is_ok(),
            "type URI {valid_type:?} must be accepted"
        );
    }
}

#[test]
fn machine_code_is_lowercase_ascii_and_stable_for_clients() {
    for invalid_code in [
        "",
        "CrossTenantDenied",
        "cross-tenant-denied",
        "9cross_tenant",
    ] {
        assert_eq!(
            ApiProblem::new(TYPE_URI, 403, TITLE, DETAIL, invalid_code),
            Err(ApiProblemContractError::InvalidCode)
        );
    }

    assert!(ApiProblem::new(TYPE_URI, 403, TITLE, DETAIL, "denied_v2").is_ok());
}

#[test]
fn public_title_and_detail_must_be_deliberately_supplied_static_text() {
    assert_eq!(
        ApiProblem::new(TYPE_URI, 403, "   ", DETAIL, CODE),
        Err(ApiProblemContractError::EmptyTitle)
    );
    assert_eq!(
        ApiProblem::new(TYPE_URI, 403, TITLE, "\t\n", CODE),
        Err(ApiProblemContractError::EmptyDetail)
    );
}

#[test]
fn contract_errors_are_stable_and_do_not_echo_rejected_input() {
    let cases = [
        (
            ApiProblemContractError::InvalidTypeUri,
            "problem type must use a structurally valid explicit HTTPS or URN identifier",
        ),
        (
            ApiProblemContractError::InvalidStatus,
            "problem status must be an HTTP client or server error status",
        ),
        (
            ApiProblemContractError::EmptyTitle,
            "problem title must contain public-safe text",
        ),
        (
            ApiProblemContractError::EmptyDetail,
            "problem detail must contain public-safe text",
        ),
        (
            ApiProblemContractError::InvalidCode,
            "problem code must be lowercase ASCII and start with a letter",
        ),
    ];

    for (error, expected) in cases {
        assert_eq!(error.to_string(), expected);
    }
}
