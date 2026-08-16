//! Public item-delivery HTTP contract for record and exact-identity reload.
//!
//! A purchaser who already has an Active in-process session records that one
//! published item version was shown, then reloads the server-ordered ledger to
//! resume. Selection/calibration stay in `fast-mlsirm`. Persistence across
//! process restart remains a later slice.

use psychometrics_commons_runtime::instrument::InstrumentReleaseManifest;
use psychometrics_commons_runtime::item_delivery::ItemDeliveryLedger;
use psychometrics_commons_runtime::item_delivery_http::{
    handle_item_delivery_http_request, ItemDeliveryHttpRuntime, ItemDeliveryHttpRuntimeError,
    ITEM_DELIVERY_COLLECTION_SUFFIX,
};
use psychometrics_commons_runtime::session::SessionState;

const RELEASE_DIGEST: &str =
    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const SESSION_REF: &str = "ses_7c2f0a91d4b64e1f9a0c3e5d8b1a2468";
const DELIVERY_REF: &str = "dlv_item_001_ko_web";
const PRESENTATION: &str = "presentation_web_self_report_v1";
const SELECTION: &str = "selection_fixed_order_v1";

fn manifest() -> InstrumentReleaseManifest {
    InstrumentReleaseManifest::new(
        "release_big_five_ko_v1",
        "instrument_big_five",
        "instrument_version_big_five_ko_v1",
        "construct_big_five",
        &["item_version_001", "item_version_002"],
        "ko-KR",
        "assessment_spec_big_five_v1",
        "scoring_version_big_five_v1",
        "calibration_big_five_ko_v1",
        Some("norm_version_big_five_ko_v1"),
        "narrative_version_big_five_v1",
        &["consent_service_v1"],
        "intended_use_self_reflection_v1",
        "limitations_nonclinical_v1",
        RELEASE_DIGEST,
    )
    .unwrap()
}

fn active_runtime() -> ItemDeliveryHttpRuntime {
    let ledger = ItemDeliveryLedger::from_manifest(SESSION_REF, &manifest()).unwrap();
    ItemDeliveryHttpRuntime::new(vec![(SessionState::Active, ledger)]).unwrap()
}

fn collection_path(session_ref: &str) -> String {
    format!("/v1/sessions/{session_ref}{ITEM_DELIVERY_COLLECTION_SUFFIX}")
}

fn post_request(session_ref: &str, body: &str, idempotency_key: &str) -> String {
    format!(
        "POST {} HTTP/1.1\r\nHost: localhost\r\nIdempotency-Key: {idempotency_key}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        collection_path(session_ref),
        body.len()
    )
}

fn get_request(session_ref: &str) -> String {
    format!(
        "GET {} HTTP/1.1\r\nHost: localhost\r\n\r\n",
        collection_path(session_ref)
    )
}

fn valid_body() -> String {
    format!(
        "{{\"delivery_ref\":\"{DELIVERY_REF}\",\"item_version_ref\":\"item_version_001\",\"presentation_context_ref\":\"{PRESENTATION}\",\"selection_evidence_ref\":\"{SELECTION}\"}}"
    )
}

#[test]
fn post_records_the_first_published_item_for_an_active_korean_session() {
    let mut runtime = active_runtime();
    let response = handle_item_delivery_http_request(
        &post_request(SESSION_REF, &valid_body(), DELIVERY_REF),
        &mut runtime,
    );

    assert_eq!(response.status(), 201);
    assert_eq!(response.content_type(), "application/json");
    assert!(response
        .body()
        .contains(&format!("\"session_ref\":\"{SESSION_REF}\"")));
    assert!(response
        .body()
        .contains("\"instrument_release_ref\":\"release_big_five_ko_v1\""));
    assert!(response.body().contains("\"locale\":\"ko-KR\""));
    assert!(response
        .body()
        .contains(&format!("\"delivery_ref\":\"{DELIVERY_REF}\"")));
    assert!(response
        .body()
        .contains("\"item_version_ref\":\"item_version_001\""));
    assert!(response
        .body()
        .contains(&format!("\"presentation_context_ref\":\"{PRESENTATION}\"")));
    assert!(response
        .body()
        .contains(&format!("\"selection_evidence_ref\":\"{SELECTION}\"")));
    assert!(response.body().contains("\"sequence\":1"));
    assert!(response
        .body()
        .contains(&format!("\"release_content_digest\":\"{RELEASE_DIGEST}\"")));
    assert!(response.body().contains("\"session_state\":\"active\""));
    assert!(response.body().contains(
        "This response is delivery evidence, not item text and not the next selected item"
    ));
    assert_eq!(runtime.event_count(SESSION_REF), 1);
}

#[test]
fn exact_idempotent_replay_returns_the_original_delivery_without_a_second_row() {
    let mut runtime = active_runtime();
    let first = handle_item_delivery_http_request(
        &post_request(SESSION_REF, &valid_body(), DELIVERY_REF),
        &mut runtime,
    );
    let replay = handle_item_delivery_http_request(
        &post_request(SESSION_REF, &valid_body(), DELIVERY_REF),
        &mut runtime,
    );

    assert_eq!(first.status(), 201);
    assert_eq!(replay.status(), 200);
    assert!(replay
        .body()
        .contains("Reuse this original event; do not insert another row"));
    assert_eq!(runtime.event_count(SESSION_REF), 1);
}

#[test]
fn get_returns_server_ordered_deliveries_so_the_client_can_resume() {
    let mut runtime = active_runtime();
    assert_eq!(
        handle_item_delivery_http_request(
            &post_request(SESSION_REF, &valid_body(), DELIVERY_REF),
            &mut runtime,
        )
        .status(),
        201
    );
    let second_body = format!(
        "{{\"delivery_ref\":\"dlv_item_002_ko_web\",\"item_version_ref\":\"item_version_002\",\"presentation_context_ref\":\"{PRESENTATION}\"}}"
    );
    assert_eq!(
        handle_item_delivery_http_request(
            &post_request(SESSION_REF, &second_body, "dlv_item_002_ko_web"),
            &mut runtime,
        )
        .status(),
        201
    );

    let loaded = handle_item_delivery_http_request(&get_request(SESSION_REF), &mut runtime);
    assert_eq!(loaded.status(), 200);
    assert_eq!(loaded.content_type(), "application/json");
    assert!(loaded.body().contains("\"sequence\":1"));
    assert!(loaded.body().contains("\"sequence\":2"));
    assert!(loaded.body().contains("\"item_version_001\""));
    assert!(loaded.body().contains("\"item_version_002\""));
    assert!(loaded
        .body()
        .contains("\"allowed_item_version_refs\":[\"item_version_001\",\"item_version_002\"]"));
    assert!(loaded
        .body()
        .contains(&format!("\"release_content_digest\":\"{RELEASE_DIGEST}\"")));
    assert!(loaded.body().contains("\"session_state\":\"active\""));
    assert!(loaded
        .body()
        .contains("POST the next undelivered allowed item_version_ref"));
    let first_pos = loaded
        .body()
        .find("\"item_version_ref\":\"item_version_001\"")
        .unwrap();
    let second_pos = loaded
        .body()
        .find("\"item_version_ref\":\"item_version_002\"")
        .unwrap();
    assert!(first_pos < second_pos);
}

#[test]
fn unpublished_item_paused_session_and_conflicting_replay_fail_closed() {
    let mut runtime = active_runtime();
    let unknown_item = format!(
        "{{\"delivery_ref\":\"dlv_unknown_item\",\"item_version_ref\":\"item_version_999\",\"presentation_context_ref\":\"{PRESENTATION}\"}}"
    );
    let unknown = handle_item_delivery_http_request(
        &post_request(SESSION_REF, &unknown_item, "dlv_unknown_item"),
        &mut runtime,
    );
    assert_eq!(unknown.status(), 409);
    assert_eq!(unknown.content_type(), "application/problem+json");
    assert!(unknown
        .body()
        .contains("urn:psychometrics-commons:problem:item-not-in-release"));
    assert!(unknown
        .body()
        .contains("POST an item_version_ref from the bound release allowed set"));
    assert_eq!(runtime.event_count(SESSION_REF), 0);

    assert_eq!(
        handle_item_delivery_http_request(
            &post_request(SESSION_REF, &valid_body(), DELIVERY_REF),
            &mut runtime,
        )
        .status(),
        201
    );
    let conflict_body = format!(
        "{{\"delivery_ref\":\"{DELIVERY_REF}\",\"item_version_ref\":\"item_version_002\",\"presentation_context_ref\":\"{PRESENTATION}\"}}"
    );
    let conflict = handle_item_delivery_http_request(
        &post_request(SESSION_REF, &conflict_body, DELIVERY_REF),
        &mut runtime,
    );
    assert_eq!(conflict.status(), 409);
    assert!(conflict
        .body()
        .contains("urn:psychometrics-commons:problem:idempotency-conflict"));
    assert!(conflict
        .body()
        .contains("Replay the original evidence or mint a new delivery_ref"));
    assert_eq!(runtime.event_count(SESSION_REF), 1);

    runtime.set_session_state(SESSION_REF, SessionState::Paused);
    let paused_body = format!(
        "{{\"delivery_ref\":\"dlv_after_pause\",\"item_version_ref\":\"item_version_002\",\"presentation_context_ref\":\"{PRESENTATION}\"}}"
    );
    let paused = handle_item_delivery_http_request(
        &post_request(SESSION_REF, &paused_body, "dlv_after_pause"),
        &mut runtime,
    );
    assert_eq!(paused.status(), 409);
    assert!(paused
        .body()
        .contains("urn:psychometrics-commons:problem:session-not-active"));
    assert!(paused
        .body()
        .contains("Return the session to Active, then POST the new delivery"));
    let paused_ledger = handle_item_delivery_http_request(&get_request(SESSION_REF), &mut runtime);
    assert_eq!(paused_ledger.status(), 200);
    assert!(paused_ledger
        .body()
        .contains("\"session_state\":\"paused\""));
    assert!(paused_ledger
        .body()
        .contains("return the session to Active before a new POST"));
    let replay = handle_item_delivery_http_request(
        &post_request(SESSION_REF, &valid_body(), DELIVERY_REF),
        &mut runtime,
    );
    assert_eq!(replay.status(), 200);
    assert_eq!(runtime.event_count(SESSION_REF), 1);
}

#[test]
fn get_on_an_active_session_with_no_deliveries_still_returns_the_allowed_set() {
    let mut runtime = active_runtime();
    let loaded = handle_item_delivery_http_request(&get_request(SESSION_REF), &mut runtime);
    assert_eq!(loaded.status(), 200);
    assert!(loaded.body().contains("\"events\":[]"));
    assert!(loaded
        .body()
        .contains("\"allowed_item_version_refs\":[\"item_version_001\",\"item_version_002\"]"));
    assert!(loaded.body().contains("\"session_state\":\"active\""));
    assert!(loaded
        .body()
        .contains("POST the next undelivered allowed item_version_ref"));
}

#[test]
fn colliding_seeded_session_refs_fail_closed() {
    let first = ItemDeliveryLedger::from_manifest(SESSION_REF, &manifest()).unwrap();
    let second = ItemDeliveryLedger::from_manifest(SESSION_REF, &manifest()).unwrap();
    assert_eq!(
        ItemDeliveryHttpRuntime::new(vec![
            (SessionState::Active, first),
            (SessionState::Paused, second),
        ])
        .expect_err("duplicate session_ref must not silently keep the last ledger"),
        ItemDeliveryHttpRuntimeError::DuplicateSessionRef
    );
}

#[test]
fn missing_json_content_type_and_scientific_identity_fail_closed_with_next_action() {
    let mut runtime = active_runtime();
    let body = valid_body();
    let missing_type = format!(
        "POST {} HTTP/1.1\r\nIdempotency-Key: {DELIVERY_REF}\r\nContent-Length: {}\r\n\r\n{body}",
        collection_path(SESSION_REF),
        body.len()
    );
    let typed = handle_item_delivery_http_request(&missing_type, &mut runtime);
    assert_eq!(typed.status(), 400);
    assert!(typed
        .body()
        .contains("urn:psychometrics-commons:problem:unsupported-media-type"));
    assert!(typed
        .body()
        .contains("Set Content-Type to application/json, then retry"));
    assert_eq!(runtime.event_count(SESSION_REF), 0);

    let scientific = format!(
        "POST {} HTTP/1.1\r\nIdempotency-Key: 1e10\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        collection_path(SESSION_REF),
        body.len()
    );
    let rejected = handle_item_delivery_http_request(&scientific, &mut runtime);
    assert_eq!(rejected.status(), 400);
    assert!(rejected
        .body()
        .contains("urn:psychometrics-commons:problem:missing-idempotency-key"));
    assert!(rejected
        .body()
        .contains("Set Idempotency-Key to the exact delivery_ref value"));
}

#[test]
fn unknown_session_and_mismatched_key_name_the_next_call() {
    let mut runtime = active_runtime();
    let missing =
        handle_item_delivery_http_request(&get_request("ses_missing_item_delivery"), &mut runtime);
    assert_eq!(missing.status(), 404);
    assert!(missing
        .body()
        .contains("Create or start this session_ref in this process, then retry"));

    let mismatch = format!(
        "{{\"delivery_ref\":\"dlv_a\",\"item_version_ref\":\"item_version_001\",\"presentation_context_ref\":\"{PRESENTATION}\"}}"
    );
    let conflicted = handle_item_delivery_http_request(
        &post_request(SESSION_REF, &mismatch, "dlv_b"),
        &mut runtime,
    );
    assert_eq!(conflicted.status(), 400);
    assert!(conflicted
        .body()
        .contains("Set Idempotency-Key to the exact delivery_ref value"));
}
