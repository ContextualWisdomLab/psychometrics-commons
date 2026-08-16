//! Public session HTTP contract for create and exact-identity reload.
//!
//! A purchaser starts one assessment over HTTP against a published locale-specific
//! release. The server mints the session reference, pins provenance, and returns
//! the same Created session for an exact idempotent replay. Persistence across
//! process restart remains a later slice.

use psychometrics_commons_runtime::instrument::{
    InstrumentRelease, InstrumentReleaseManifest, PublicationCommand,
    PublicationEvidenceProvenance, PublicationEvidenceRecord, PublicationEvidenceStatus,
};
use psychometrics_commons_runtime::session::SessionState;
use psychometrics_commons_runtime::session_http::{
    handle_session_http_request, SessionHttpRuntime, SESSION_COLLECTION_PATH,
};

const VALID_DIGEST: &str =
    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const EVIDENCE_DIGEST: &str =
    "sha256:1111111111111111111111111111111111111111111111111111111111111111";
const PARTICIPANT_REF: &str = "ptc_eb1b318917d24ca0ac5153c37ff696c7";
const SESSION_REF: &str = "ses_7c2f0a91d4b64e1f9a0c3e5d8b1a2468";
const IDEMPOTENCY_KEY: &str = "idem_create_session_ko_001";

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
        VALID_DIGEST,
    )
    .unwrap()
}

fn approved_evidence() -> PublicationEvidenceRecord {
    PublicationEvidenceRecord::new(
        "publication_evidence_big_five_ko_v1",
        "evidence_policy_self_reflection_v1",
        "release_big_five_ko_v1",
        "instrument_version_big_five_ko_v1",
        &["item_version_001", "item_version_002"],
        VALID_DIGEST,
        "ko-KR",
        "intended_use_self_reflection_v1",
        "assessment_spec_big_five_v1",
        "scoring_version_big_five_v1",
        "calibration_big_five_ko_v1",
        Some("norm_version_big_five_ko_v1"),
        "limitations_nonclinical_v1",
        PublicationEvidenceProvenance::new(
            EVIDENCE_DIGEST,
            "population_general_adult_v1",
            "administration_web_self_report_v1",
            "measurement_model_big_five_v1",
            10_050,
            None,
        )
        .unwrap(),
        &["rights_ipip_big_five_v1"],
        &["recovery_big_five_ko_v1"],
        &["approval_psychometrics_big_five_ko_v1"],
        PublicationEvidenceStatus::Approved,
    )
    .unwrap()
}

fn published_release() -> InstrumentRelease {
    let mut release = InstrumentRelease::new(manifest(), 10_000).unwrap();
    release
        .apply_command(
            "publication_review_f9f86084",
            PublicationCommand::SubmitReview,
            10_100,
        )
        .unwrap();
    release
        .bind_publication_evidence(approved_evidence())
        .unwrap();
    release
        .apply_command(
            "publication_publish_635a7491",
            PublicationCommand::Publish,
            10_200,
        )
        .unwrap();
    release
}

fn draft_release() -> InstrumentRelease {
    InstrumentRelease::new(manifest(), 10_000).unwrap()
}

fn runtime_with(release: InstrumentRelease) -> SessionHttpRuntime {
    SessionHttpRuntime::new(vec![release], SESSION_REF, 1_725_000_000_000)
}

fn create_request(body: &str, idempotency_key: &str) -> String {
    format!(
        "POST {SESSION_COLLECTION_PATH} HTTP/1.1\r\nHost: localhost\r\nIdempotency-Key: {idempotency_key}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    )
}

fn valid_create_body() -> String {
    format!(
        "{{\"participant_ref\":\"{PARTICIPANT_REF}\",\"instrument_release_ref\":\"release_big_five_ko_v1\",\"locale\":\"ko-KR\"}}"
    )
}

#[test]
fn post_creates_session_bound_to_published_release_and_exact_locale() {
    let mut runtime = runtime_with(published_release());
    let response = handle_session_http_request(
        &create_request(&valid_create_body(), IDEMPOTENCY_KEY),
        &mut runtime,
    );

    assert_eq!(response.status(), 201);
    assert_eq!(response.content_type(), "application/json");
    assert!(response
        .body()
        .contains(&format!("\"session_ref\":\"{SESSION_REF}\"")));
    assert!(response
        .body()
        .contains(&format!("\"participant_ref\":\"{PARTICIPANT_REF}\"")));
    assert!(response
        .body()
        .contains("\"instrument_release_ref\":\"release_big_five_ko_v1\""));
    assert!(response.body().contains(&format!(
        "\"instrument_release_content_digest\":\"{VALID_DIGEST}\""
    )));
    assert!(response.body().contains("\"locale\":\"ko-KR\""));
    assert!(response.body().contains("\"state\":\"created\""));
    assert_eq!(
        runtime
            .session(SESSION_REF)
            .map(psychometrics_commons_runtime::session::AssessmentSession::state),
        Some(SessionState::Created)
    );
}

#[test]
fn exact_idempotent_replay_returns_the_original_created_session() {
    let mut runtime = runtime_with(published_release());
    let first = handle_session_http_request(
        &create_request(&valid_create_body(), IDEMPOTENCY_KEY),
        &mut runtime,
    );
    runtime.replace_next_session_ref("ses_should_not_be_minted");
    let replay = handle_session_http_request(
        &create_request(&valid_create_body(), IDEMPOTENCY_KEY),
        &mut runtime,
    );

    assert_eq!(first.status(), 201);
    assert_eq!(replay.status(), 200);
    assert_eq!(replay.body(), first.body());
    assert_eq!(runtime.session_count(), 1);
}

#[test]
fn unpublished_release_and_locale_mismatch_fail_closed_without_creating_a_session() {
    let mut unpublished = runtime_with(draft_release());
    let unpublished_response = handle_session_http_request(
        &create_request(&valid_create_body(), IDEMPOTENCY_KEY),
        &mut unpublished,
    );
    assert_eq!(unpublished_response.status(), 409);
    assert_eq!(
        unpublished_response.content_type(),
        "application/problem+json"
    );
    assert!(unpublished_response
        .body()
        .contains("urn:psychometrics-commons:problem:instrument-release-unavailable"));
    assert_eq!(unpublished.session_count(), 0);

    let mut mismatched = runtime_with(published_release());
    let mismatch_body = format!(
        "{{\"participant_ref\":\"{PARTICIPANT_REF}\",\"instrument_release_ref\":\"release_big_five_ko_v1\",\"locale\":\"en-US\"}}"
    );
    let mismatch = handle_session_http_request(
        &create_request(&mismatch_body, "idem_locale_mismatch"),
        &mut mismatched,
    );
    assert_eq!(mismatch.status(), 409);
    assert!(mismatch
        .body()
        .contains("urn:psychometrics-commons:problem:locale-mismatch"));
    assert_eq!(mismatched.session_count(), 0);
}
