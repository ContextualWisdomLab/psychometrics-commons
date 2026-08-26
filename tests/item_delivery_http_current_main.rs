//! Current-main public item-delivery HTTP contract.
//!
//! RED contract: the public transport must use the server-authoritative
//! `AssessmentSession` aggregate rather than a detached lifecycle-state copy.

use psychometrics_commons_runtime::instrument::InstrumentReleaseManifest;
use psychometrics_commons_runtime::item_delivery::ItemDeliveryLedger;
use psychometrics_commons_runtime::item_delivery_http::{
    handle_item_delivery_http_request, ItemDeliveryHttpRuntime,
};
use psychometrics_commons_runtime::session::{AssessmentSession, SessionCommand};

const SESSION_REF: &str = "ses_item_delivery_http_current_main";
const RELEASE_DIGEST: &str =
    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

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

fn runtime() -> ItemDeliveryHttpRuntime {
    let manifest = manifest();
    let mut session = AssessmentSession::from_currently_published_manifest(
        SESSION_REF,
        "participant_anonymous_http",
        &manifest,
        "ko-KR",
        1,
    )
    .unwrap();
    session
        .apply_command("command_activate_http", 1, SessionCommand::Activate)
        .unwrap();
    let ledger = ItemDeliveryLedger::from_manifest(SESSION_REF, &manifest).unwrap();
    let mut runtime = ItemDeliveryHttpRuntime::new();
    runtime.insert_session(session, ledger).unwrap();
    runtime
}

fn post(body: &str, key: &str) -> String {
    format!(
        "POST /v1/sessions/{SESSION_REF}/item-deliveries HTTP/1.1\r\nIdempotency-Key: {key}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    )
}

#[test]
fn active_authoritative_session_records_and_replays_exact_delivery() {
    let mut runtime = runtime();
    let body = "{\"delivery_ref\":\"delivery_item_001\",\"item_version_ref\":\"item_version_001\",\"presentation_context_ref\":\"presentation_web_v1\"}";

    let first = handle_item_delivery_http_request(&post(body, "delivery_item_001"), &mut runtime);
    assert_eq!(first.status(), 201);
    assert!(first.body().contains("\"sequence\":1"));

    let replay = handle_item_delivery_http_request(&post(body, "delivery_item_001"), &mut runtime);
    assert_eq!(replay.status(), 200);
    assert_eq!(replay.body(), first.body());
    assert_eq!(runtime.event_count(SESSION_REF), 1);
}

#[test]
fn runtime_rejects_session_ledger_rebinding_and_path_aliases() {
    let manifest = manifest();
    let session = AssessmentSession::from_currently_published_manifest(
        "ses_other",
        "participant_other",
        &manifest,
        "ko-KR",
        1,
    )
    .unwrap();
    let ledger = ItemDeliveryLedger::from_manifest(SESSION_REF, &manifest).unwrap();
    let mut runtime = ItemDeliveryHttpRuntime::new();
    assert!(runtime.insert_session(session, ledger).is_err());

    let mut runtime = runtime();
    let response = handle_item_delivery_http_request(
        "GET /v1/sessions/%73es_item_delivery_http_current_main/item-deliveries HTTP/1.1\r\n\r\n",
        &mut runtime,
    );
    assert_eq!(response.status(), 404);
}

#[test]
fn get_lists_server_ordered_delivery_evidence_without_scientific_inference() {
    let mut runtime = runtime();
    let first = "{\"delivery_ref\":\"delivery_item_001\",\"item_version_ref\":\"item_version_001\",\"presentation_context_ref\":\"presentation_web_v1\"}";
    let second = "{\"delivery_ref\":\"delivery_item_002\",\"item_version_ref\":\"item_version_002\",\"presentation_context_ref\":\"presentation_web_v1\"}";
    assert_eq!(handle_item_delivery_http_request(&post(first, "delivery_item_001"), &mut runtime).status(), 201);
    assert_eq!(handle_item_delivery_http_request(&post(second, "delivery_item_002"), &mut runtime).status(), 201);

    let response = handle_item_delivery_http_request(
        &format!("GET /v1/sessions/{SESSION_REF}/item-deliveries HTTP/1.1\r\n\r\n"),
        &mut runtime,
    );
    assert_eq!(response.status(), 200);
    let first_pos = response.body().find("delivery_item_001").unwrap();
    let second_pos = response.body().find("delivery_item_002").unwrap();
    assert!(first_pos < second_pos);
    assert!(response.body().contains("\"session_state\":\"active\""));
    assert!(!response.body().contains("score"));
}
