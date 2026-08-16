//! Public account-link HTTP contract for persist, recover, and unlink.
//!
//! A buyer who proved control of an anonymous session and a Keyverse account
//! posts both current proofs. The server clock is the link time. A later
//! recover with a still-valid account proof returns the same participant.
//! Unlink uses that same proof, not a previously recovered `participant_ref`.
//! This file covers fail-closed routing without a database. Durable persist,
//! recover, and unlink run in `postgres_account_link_http`.

use psychometrics_commons_runtime::account_link_http::{
    classify_account_link_http_request, AccountLinkHttpClassification,
    ACCOUNT_LINK_COLLECTION_PATH, ACCOUNT_LINK_RECOVER_PATH, ACCOUNT_LINK_UNLINK_PATH,
};

const IDEMPOTENCY_KEY: &str = "idem_account_link_ko_001";

fn persist_body() -> String {
    "{\"participant_ref\":\"participant_identity_http\",\"tenant_ref\":\"tenant_identity_http\",\"anonymous_session_ref\":\"session_identity_http\",\"anonymous_proof_ref\":\"anonymous_proof_http\",\"anonymous_valid_until_unix_ms\":11000,\"identity_issuer\":\"keyverse_issuer_http\",\"identity_subject_ref\":\"keyverse_subject_http\",\"authenticated_proof_ref\":\"authenticated_proof_http\",\"authenticated_valid_until_unix_ms\":11000,\"link_event_ref\":\"link_event_identity_http\"}".to_owned()
}

fn recover_body() -> String {
    "{\"tenant_ref\":\"tenant_identity_http\",\"identity_issuer\":\"keyverse_issuer_http\",\"identity_subject_ref\":\"keyverse_subject_http\",\"authenticated_proof_ref\":\"authenticated_proof_http\",\"authenticated_valid_until_unix_ms\":11000}".to_owned()
}

fn unlink_body() -> String {
    "{\"tenant_ref\":\"tenant_identity_http\",\"identity_issuer\":\"keyverse_issuer_http\",\"identity_subject_ref\":\"keyverse_subject_http\",\"authenticated_proof_ref\":\"authenticated_proof_http\",\"authenticated_valid_until_unix_ms\":11000,\"link_end_event_ref\":\"link_end_event_identity_http\"}".to_owned()
}

fn persist_request(body: &str, idempotency_key: &str) -> String {
    format!(
        "POST {ACCOUNT_LINK_COLLECTION_PATH} HTTP/1.1\r\nHost: localhost\r\nIdempotency-Key: {idempotency_key}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    )
}

fn recover_request(body: &str) -> String {
    format!(
        "POST {ACCOUNT_LINK_RECOVER_PATH} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    )
}

fn unlink_request(body: &str, idempotency_key: &str) -> String {
    format!(
        "POST {ACCOUNT_LINK_UNLINK_PATH} HTTP/1.1\r\nHost: localhost\r\nIdempotency-Key: {idempotency_key}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    )
}

#[test]
fn post_account_links_classifies_a_complete_dual_proof_persist() {
    match classify_account_link_http_request(&persist_request(&persist_body(), IDEMPOTENCY_KEY)) {
        AccountLinkHttpClassification::Persist(persist) => {
            assert_eq!(persist.idempotency_key(), IDEMPOTENCY_KEY);
            assert_eq!(persist.participant_ref(), "participant_identity_http");
            assert_eq!(persist.tenant_ref(), "tenant_identity_http");
            assert_eq!(persist.identity_issuer(), "keyverse_issuer_http");
            assert_eq!(persist.identity_subject_ref(), "keyverse_subject_http");
            assert_eq!(persist.link_event_ref(), "link_event_identity_http");
            assert_eq!(persist.anonymous_valid_until_unix_ms(), 11_000);
            assert_eq!(persist.authenticated_valid_until_unix_ms(), 11_000);
        }
        other => panic!("expected persist classification, got {other:?}"),
    }
}

#[test]
fn post_account_links_recover_classifies_a_still_valid_account_proof() {
    match classify_account_link_http_request(&recover_request(&recover_body())) {
        AccountLinkHttpClassification::Recover(recover) => {
            assert_eq!(recover.tenant_ref(), "tenant_identity_http");
            assert_eq!(recover.identity_issuer(), "keyverse_issuer_http");
            assert_eq!(recover.identity_subject_ref(), "keyverse_subject_http");
            assert_eq!(
                recover.authenticated_proof_ref(),
                "authenticated_proof_http"
            );
            assert_eq!(recover.authenticated_valid_until_unix_ms(), 11_000);
        }
        other => panic!("expected recover classification, got {other:?}"),
    }
}

#[test]
fn post_account_links_unlink_classifies_current_proof_without_participant_ref() {
    match classify_account_link_http_request(&unlink_request(&unlink_body(), IDEMPOTENCY_KEY)) {
        AccountLinkHttpClassification::Unlink(unlink) => {
            assert_eq!(unlink.idempotency_key(), IDEMPOTENCY_KEY);
            assert_eq!(unlink.tenant_ref(), "tenant_identity_http");
            assert_eq!(unlink.identity_issuer(), "keyverse_issuer_http");
            assert_eq!(unlink.identity_subject_ref(), "keyverse_subject_http");
            assert_eq!(unlink.link_end_event_ref(), "link_end_event_identity_http");
            assert_eq!(unlink.authenticated_valid_until_unix_ms(), 11_000);
        }
        other => panic!("expected unlink classification, got {other:?}"),
    }
}

#[test]
fn unlink_rejects_a_client_supplied_participant_ref_as_a_capability_grant() {
    let body_with_grant = "{\"participant_ref\":\"participant_identity_stolen\",\"tenant_ref\":\"tenant_identity_http\",\"identity_issuer\":\"keyverse_issuer_http\",\"identity_subject_ref\":\"keyverse_subject_http\",\"authenticated_proof_ref\":\"authenticated_proof_http\",\"authenticated_valid_until_unix_ms\":11000,\"link_end_event_ref\":\"link_end_event_identity_http\"}";
    let classified =
        classify_account_link_http_request(&unlink_request(body_with_grant, IDEMPOTENCY_KEY));
    let AccountLinkHttpClassification::Ready(rejected) = classified else {
        panic!("unlink must not accept a client-supplied participant_ref");
    };
    assert_eq!(rejected.status(), 400);
    assert_eq!(rejected.content_type(), "application/problem+json");
    assert!(rejected
        .body()
        .contains("urn:psychometrics-commons:problem:bad-request"));
}

#[test]
fn missing_idempotency_unknown_json_and_wrong_methods_fail_closed() {
    let missing_key = classify_account_link_http_request(&format!(
        "POST {ACCOUNT_LINK_COLLECTION_PATH} HTTP/1.1\r\nContent-Length: 2\r\n\r\n{{}}"
    ));
    let AccountLinkHttpClassification::Ready(missing) = missing_key else {
        panic!("missing Idempotency-Key must fail before persist");
    };
    assert_eq!(missing.status(), 400);
    assert_eq!(missing.content_type(), "application/problem+json");
    assert!(missing
        .body()
        .contains("urn:psychometrics-commons:problem:missing-idempotency-key"));

    let missing_unlink_key = classify_account_link_http_request(&format!(
        "POST {ACCOUNT_LINK_UNLINK_PATH} HTTP/1.1\r\nContent-Length: 2\r\n\r\n{{}}"
    ));
    let AccountLinkHttpClassification::Ready(missing_unlink) = missing_unlink_key else {
        panic!("missing Idempotency-Key must fail before unlink");
    };
    assert_eq!(missing_unlink.status(), 400);
    assert!(missing_unlink
        .body()
        .contains("urn:psychometrics-commons:problem:missing-idempotency-key"));

    let bad_json = classify_account_link_http_request(&persist_request("{}", IDEMPOTENCY_KEY));
    let AccountLinkHttpClassification::Ready(invalid) = bad_json else {
        panic!("incomplete persist JSON must fail closed");
    };
    assert_eq!(invalid.status(), 400);
    assert!(invalid
        .body()
        .contains("urn:psychometrics-commons:problem:bad-request"));

    let get_collection =
        classify_account_link_http_request("GET /v1/account-links HTTP/1.1\r\n\r\n");
    let AccountLinkHttpClassification::Ready(method) = get_collection else {
        panic!("GET collection must be method-not-allowed");
    };
    assert_eq!(method.status(), 405);

    let unknown = classify_account_link_http_request("GET /v1/sessions/ses_x HTTP/1.1\r\n\r\n");
    let AccountLinkHttpClassification::Ready(not_found) = unknown else {
        panic!("other families must be not-found");
    };
    assert_eq!(not_found.status(), 404);
}
