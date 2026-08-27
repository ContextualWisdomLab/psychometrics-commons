//! Fail-closed authority and request-boundary contracts for item-delivery HTTP.

use psychometrics_commons_runtime::instrument::InstrumentReleaseManifest;
use psychometrics_commons_runtime::item_delivery::ItemDeliveryLedger;
use psychometrics_commons_runtime::item_delivery_http::{
    handle_item_delivery_http_request, ItemDeliveryHttpRuntime,
};
use psychometrics_commons_runtime::session::{AssessmentSession, SessionCommand};

const DIGEST_A: &str =
    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const DIGEST_B: &str =
    "sha256:abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789";
const SESSION_REF: &str = "ses_item_delivery_authority";

fn manifest(release_ref: &str, digest: &str) -> InstrumentReleaseManifest {
    InstrumentReleaseManifest::new(
        release_ref,
        "instrument_big_five",
        "instrument_version_big_five_ko_v1",
        "construct_big_five",
        &["item_version_001"],
        "ko-KR",
        "assessment_spec_big_five_v1",
        "scoring_version_big_five_v1",
        "calibration_big_five_ko_v1",
        Some("norm_version_big_five_ko_v1"),
        "narrative_version_big_five_v1",
        &["consent_service_v1"],
        "intended_use_self_reflection_v1",
        "limitations_nonclinical_v1",
        digest,
    )
    .unwrap()
}

fn session(manifest: &InstrumentReleaseManifest) -> AssessmentSession {
    AssessmentSession::from_currently_published_manifest(
        SESSION_REF,
        "participant_item_delivery_authority",
        manifest,
        "ko-KR",
        1,
    )
    .unwrap()
}

fn record_request(body: &str, content_length: usize) -> String {
    format!(
        "POST /v1/sessions/{SESSION_REF}/item-deliveries HTTP/1.1\r\nHost: localhost\r\nIdempotency-Key: dlv_authority_001\r\nContent-Type: application/json\r\nContent-Length: {content_length}\r\n\r\n{body}"
    )
}

#[test]
fn delivery_state_is_derived_from_the_authoritative_assessment_session() {
    let manifest = manifest("release_big_five_ko_v1", DIGEST_A);
    let ledger = ItemDeliveryLedger::from_manifest(SESSION_REF, &manifest).unwrap();
    let mut runtime = ItemDeliveryHttpRuntime::new(vec![(session(&manifest), ledger)]).unwrap();
    let body = "{\"delivery_ref\":\"dlv_authority_001\",\"item_version_ref\":\"item_version_001\",\"presentation_context_ref\":\"presentation_web_v1\"}";

    let before_activate = handle_item_delivery_http_request(
        &record_request(body, body.len()),
        &mut runtime,
    );
    assert_eq!(before_activate.status(), 409);
    assert_eq!(runtime.event_count(SESSION_REF), 0);

    runtime
        .session_mut(SESSION_REF)
        .unwrap()
        .apply_command("cmd_activate_item_delivery", 1, SessionCommand::Activate)
        .unwrap();

    let after_activate = handle_item_delivery_http_request(
        &record_request(body, body.len()),
        &mut runtime,
    );
    assert_eq!(after_activate.status(), 201);
    assert_eq!(runtime.event_count(SESSION_REF), 1);
}

#[test]
fn runtime_seed_rejects_session_ledger_provenance_rebinding_and_duplicate_sessions() {
    let manifest_a = manifest("release_big_five_ko_v1", DIGEST_A);
    let manifest_b = manifest("release_big_five_ko_v2", DIGEST_B);
    let authoritative_session = session(&manifest_a);
    let rebound_ledger = ItemDeliveryLedger::from_manifest(SESSION_REF, &manifest_b).unwrap();
    assert!(ItemDeliveryHttpRuntime::new(vec![(authoritative_session, rebound_ledger)]).is_err());

    let first_session = session(&manifest_a);
    let second_session = session(&manifest_a);
    let first_ledger = ItemDeliveryLedger::from_manifest(SESSION_REF, &manifest_a).unwrap();
    let second_ledger = ItemDeliveryLedger::from_manifest(SESSION_REF, &manifest_a).unwrap();
    assert!(ItemDeliveryHttpRuntime::new(vec![
        (first_session, first_ledger),
        (second_session, second_ledger),
    ])
    .is_err());
}

#[test]
fn direct_handler_rejects_a_content_length_inside_a_utf8_scalar_without_panicking() {
    let manifest = manifest("release_big_five_ko_v1", DIGEST_A);
    let ledger = ItemDeliveryLedger::from_manifest(SESSION_REF, &manifest).unwrap();
    let mut authoritative_session = session(&manifest);
    authoritative_session
        .apply_command("cmd_activate_item_delivery", 1, SessionCommand::Activate)
        .unwrap();
    let mut runtime =
        ItemDeliveryHttpRuntime::new(vec![(authoritative_session, ledger)]).unwrap();

    let response = handle_item_delivery_http_request(&record_request("é", 1), &mut runtime);
    assert_eq!(response.status(), 400);
    assert_eq!(runtime.event_count(SESSION_REF), 0);
}
