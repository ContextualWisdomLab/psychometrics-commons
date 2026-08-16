//! Buyer-facing `POST /v1/sessions` mapping: start a created session or get a safe problem.

use psychometrics_commons_runtime::api_problem::ApiProblem;
use psychometrics_commons_runtime::instrument::{
    InstrumentRelease, InstrumentReleaseManifest, PublicationCommand,
    PublicationEvidenceProvenance, PublicationEvidenceRecord, PublicationEvidenceStatus,
};
use psychometrics_commons_runtime::session::SessionState;
use psychometrics_commons_runtime::session_http::{
    create_assessment_session, CreateSessionHttpRequest, CREATE_SESSION_HTTP_METHOD,
    CREATE_SESSION_HTTP_PATH, CREATE_SESSION_JSON_MEDIA_TYPE, CREATE_SESSION_SUCCESS_STATUS,
};

const VALID_DIGEST: &str =
    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const EVIDENCE_DIGEST: &str =
    "sha256:1111111111111111111111111111111111111111111111111111111111111111";
const PARTICIPANT_REF: &str = "ptc_eb1b318917d24ca0ac5153c37ff696c7";
const SESSION_REF: &str = "ses_02fe09e373504b7986ae78491116edbd";

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

fn create_request<'a>(
    session_ref: &'a str,
    participant_ref: &'a str,
    locale: &'a str,
    created_at_unix_ms: u64,
) -> CreateSessionHttpRequest<'a> {
    CreateSessionHttpRequest::new(session_ref, participant_ref, locale, created_at_unix_ms)
}

#[test]
fn published_release_starts_created_session_for_post_v1_sessions() {
    let accepted = create_assessment_session(
        create_request(SESSION_REF, PARTICIPANT_REF, "ko-KR", 20_000),
        &published_release(),
    )
    .expect("a published Korean release must start a created session");

    assert_eq!(CREATE_SESSION_HTTP_METHOD, "POST");
    assert_eq!(CREATE_SESSION_HTTP_PATH, "/v1/sessions");
    assert_eq!(CREATE_SESSION_SUCCESS_STATUS, 201);
    assert_eq!(CREATE_SESSION_JSON_MEDIA_TYPE, "application/json");
    assert_eq!(accepted.session_ref(), SESSION_REF);
    assert_eq!(accepted.participant_ref(), PARTICIPANT_REF);
    assert_eq!(accepted.instrument_release_ref(), "release_big_five_ko_v1");
    assert_eq!(
        accepted.instrument_version_ref(),
        "instrument_version_big_five_ko_v1"
    );
    assert_eq!(accepted.instrument_release_content_digest(), VALID_DIGEST);
    assert_eq!(accepted.locale(), "ko-KR");
    assert_eq!(accepted.created_at_unix_ms(), 20_000);
    assert_eq!(accepted.session_state(), SessionState::Created);
    assert_eq!(accepted.session_state().persist_name(), "created");
    assert_eq!(accepted.session().session_ref(), SESSION_REF);
}

#[test]
fn unpublished_release_tells_the_caller_to_publish_before_starting() {
    let draft = InstrumentRelease::new(manifest(), 10_000).unwrap();
    let problem = create_assessment_session(
        create_request(SESSION_REF, PARTICIPANT_REF, "ko-KR", 20_000),
        &draft,
    )
    .expect_err("a draft release must not start a session");

    assert_eq!(problem.status(), 409);
    assert_eq!(problem.code(), "instrument_release_unavailable");
    assert_eq!(
        problem.type_uri(),
        "urn:psychometrics-commons:problem:instrument-release-unavailable"
    );
    assert_eq!(
        problem.detail(),
        "Publish this instrument release before starting a new session."
    );
    assert_eq!(ApiProblem::media_type(), "application/problem+json");
}

#[test]
fn locale_mismatch_tells_the_caller_to_use_the_published_locale() {
    let problem = create_assessment_session(
        create_request(SESSION_REF, PARTICIPANT_REF, "en-US", 20_000),
        &published_release(),
    )
    .expect_err("an English request must not start a Korean published release");

    assert_eq!(problem.status(), 409);
    assert_eq!(problem.code(), "locale_mismatch");
    assert_eq!(
        problem.detail(),
        "Request the exact locale published on this instrument release."
    );
}

#[test]
fn numeric_session_reference_tells_the_caller_to_use_an_opaque_id() {
    let problem = create_assessment_session(
        create_request("12345", PARTICIPANT_REF, "ko-KR", 20_000),
        &published_release(),
    )
    .expect_err("a numeric session reference must fail closed");

    assert_eq!(problem.status(), 400);
    assert_eq!(problem.code(), "invalid_session_reference");
    assert_eq!(
        problem.detail(),
        "Use an opaque non-numeric session and participant reference."
    );
}

#[test]
fn zero_timestamp_tells_the_caller_to_send_a_server_clock() {
    let problem = create_assessment_session(
        create_request(SESSION_REF, PARTICIPANT_REF, "ko-KR", 0),
        &published_release(),
    )
    .expect_err("a zero creation time must fail closed");

    assert_eq!(problem.status(), 400);
    assert_eq!(problem.code(), "invalid_session_timestamp");
    assert_eq!(
        problem.detail(),
        "Send a server-issued creation time greater than zero."
    );
}
