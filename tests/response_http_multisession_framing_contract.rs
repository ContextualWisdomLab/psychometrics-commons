//! Regression contracts for response HTTP server-event allocation and header framing.

use psychometrics_commons_runtime::authorization::{AuthorizationContext, ProductRole};
use psychometrics_commons_runtime::instrument::{
    InstrumentRelease, InstrumentReleaseManifest, PublicationCommand,
    PublicationEvidenceProvenance, PublicationEvidenceRecord, PublicationEvidenceStatus,
};
use psychometrics_commons_runtime::participant::ParticipantRecord;
use psychometrics_commons_runtime::response_http::{
    handle_authorized_response_http_request, handle_response_http_request, ResponseHttpResponse,
    ResponseHttpRuntime, ResponseWriteAuthority,
};
use psychometrics_commons_runtime::session::{AssessmentSession, SessionCommand};

const TENANT_REF: &str = "tenant_response_multi";
const RELEASE_DIGEST: &str =
    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const EVIDENCE_DIGEST: &str =
    "sha256:1111111111111111111111111111111111111111111111111111111111111111";
const PAYLOAD_ONE: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const PAYLOAD_TWO: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn published_release() -> InstrumentRelease {
    let manifest = InstrumentReleaseManifest::new(
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
    .unwrap();
    let evidence = PublicationEvidenceRecord::new(
        "publication_evidence_big_five_ko_v1",
        "evidence_policy_self_reflection_v1",
        "release_big_five_ko_v1",
        "instrument_version_big_five_ko_v1",
        &["item_version_001", "item_version_002"],
        RELEASE_DIGEST,
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
    .unwrap();
    let mut release = InstrumentRelease::new(manifest, 10_000).unwrap();
    release
        .apply_command(
            "publication_review_response_http_multi",
            PublicationCommand::SubmitReview,
            10_100,
        )
        .unwrap();
    release.bind_publication_evidence(evidence).unwrap();
    release
        .apply_command(
            "publication_publish_response_http_multi",
            PublicationCommand::Publish,
            10_200,
        )
        .unwrap();
    release
}

fn active_session(
    session_ref: &str,
    participant_ref: &str,
    release: &InstrumentRelease,
) -> AssessmentSession {
    let mut session = AssessmentSession::new(
        session_ref,
        participant_ref,
        release,
        "ko-KR",
        1_725_000_000_000,
    )
    .unwrap();
    session
        .apply_command(
            &format!("cmd_activate_{session_ref}"),
            1,
            SessionCommand::Activate,
        )
        .unwrap();
    session
}

fn post_request(
    session_ref: &str,
    idempotency_key: &str,
    item_version_ref: &str,
    payload_digest: &str,
) -> String {
    let body = format!(
        "{{\"item_version_ref\":\"{item_version_ref}\",\"payload_digest\":\"{payload_digest}\"}}"
    );
    format!(
        "POST /v1/sessions/{session_ref}/responses HTTP/1.1\r\nHost: localhost\r\nIdempotency-Key: {idempotency_key}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    )
}

fn authorized_response(
    runtime: &mut ResponseHttpRuntime,
    participant_ref: &str,
    request: &str,
) -> ResponseHttpResponse {
    let participant =
        ParticipantRecord::new_anonymous(participant_ref, TENANT_REF, 10_300).unwrap();
    let actor = AuthorizationContext::new(
        TENANT_REF,
        "subject_response_multi_owner",
        Some(participant_ref),
        &[ProductRole::Participant],
    )
    .unwrap();
    let authority = ResponseWriteAuthority::Authenticated(&actor);
    handle_authorized_response_http_request(request, &authority, &participant, runtime)
}

#[test]
fn interleaved_sessions_do_not_reuse_server_event_reference() {
    let release = published_release();
    let session_a = active_session("ses_response_multi_a", "ptc_response_multi_a", &release);
    let session_b = active_session("ses_response_multi_b", "ptc_response_multi_b", &release);
    let mut runtime =
        ResponseHttpRuntime::new(vec![session_a, session_b], vec![release], "evt_response_1");

    let session_alpha_request = post_request(
        "ses_response_multi_a",
        "idem_response_multi_a_1",
        "item_version_001",
        PAYLOAD_ONE,
    );
    let first_a = authorized_response(&mut runtime, "ptc_response_multi_a", &session_alpha_request);

    let session_beta_initial = post_request(
        "ses_response_multi_b",
        "idem_response_multi_b_1",
        "item_version_001",
        PAYLOAD_ONE,
    );
    let first_b = authorized_response(&mut runtime, "ptc_response_multi_b", &session_beta_initial);

    let session_beta_followup = post_request(
        "ses_response_multi_b",
        "idem_response_multi_b_2",
        "item_version_002",
        PAYLOAD_TWO,
    );
    let second_b = authorized_response(&mut runtime, "ptc_response_multi_b", &session_beta_followup);

    assert_eq!(first_a.status(), 201);
    assert_eq!(first_b.status(), 201);
    assert_eq!(second_b.status(), 201);
    assert_eq!(runtime.event_count("ses_response_multi_a"), 1);
    assert_eq!(runtime.event_count("ses_response_multi_b"), 2);
}

#[test]
fn generated_server_event_cursor_skips_a_valid_seed_that_occupies_its_namespace() {
    let release = published_release();
    let session = active_session(
        "ses_response_seed_collision",
        "ptc_response_seed_collision",
        &release,
    );
    let mut runtime = ResponseHttpRuntime::new(vec![session], vec![release], "evt_response_2");

    let first_request = post_request(
        "ses_response_seed_collision",
        "idem_response_seed_collision_1",
        "item_version_001",
        PAYLOAD_ONE,
    );
    let first = authorized_response(&mut runtime, "ptc_response_seed_collision", &first_request);

    let second_request = post_request(
        "ses_response_seed_collision",
        "idem_response_seed_collision_2",
        "item_version_002",
        PAYLOAD_TWO,
    );
    let second = authorized_response(&mut runtime, "ptc_response_seed_collision", &second_request);

    assert_eq!(first.status(), 201);
    assert_eq!(second.status(), 201);
    assert_eq!(runtime.event_count("ses_response_seed_collision"), 2);
    assert!(first
        .body()
        .contains("\"server_event_ref\":\"evt_response_2\""));
    assert!(!second
        .body()
        .contains("\"server_event_ref\":\"evt_response_2\""));
}

#[test]
fn stale_server_event_cursor_cannot_reuse_identity_across_sessions() {
    let release = published_release();
    let session_a = active_session(
        "ses_response_global_ref_a",
        "ptc_response_global_ref_a",
        &release,
    );
    let session_b = active_session(
        "ses_response_global_ref_b",
        "ptc_response_global_ref_b",
        &release,
    );
    let mut runtime = ResponseHttpRuntime::new(
        vec![session_a, session_b],
        vec![release],
        "evt_response_global_shared",
    );

    let first_request = post_request(
        "ses_response_global_ref_a",
        "idem_response_global_ref_a",
        "item_version_001",
        PAYLOAD_ONE,
    );
    let first = authorized_response(&mut runtime, "ptc_response_global_ref_a", &first_request);
    assert_eq!(first.status(), 201);

    runtime.replace_next_server_event_ref("evt_response_global_shared");
    let conflicting_request = post_request(
        "ses_response_global_ref_b",
        "idem_response_global_ref_b",
        "item_version_002",
        PAYLOAD_TWO,
    );
    let conflict = authorized_response(
        &mut runtime,
        "ptc_response_global_ref_b",
        &conflicting_request,
    );

    assert_eq!(conflict.status(), 409);
    assert!(conflict
        .body()
        .contains("urn:psychometrics-commons:problem:server-reference-conflict"));
    assert_eq!(runtime.event_count("ses_response_global_ref_a"), 1);
    assert_eq!(runtime.event_count("ses_response_global_ref_b"), 0);
}

#[test]
fn exact_replay_survives_a_stale_cursor_that_names_its_original_server_event() {
    let release = published_release();
    let session = active_session(
        "ses_response_stale_replay",
        "ptc_response_stale_replay",
        &release,
    );
    let mut runtime =
        ResponseHttpRuntime::new(vec![session], vec![release], "evt_response_stale_replay");
    let request = post_request(
        "ses_response_stale_replay",
        "idem_response_stale_replay",
        "item_version_001",
        PAYLOAD_ONE,
    );

    let created = authorized_response(&mut runtime, "ptc_response_stale_replay", &request);
    assert_eq!(created.status(), 201);
    runtime.replace_next_server_event_ref("evt_response_stale_replay");
    let replay = authorized_response(&mut runtime, "ptc_response_stale_replay", &request);

    assert_eq!(replay.status(), 200);
    assert_eq!(replay.body(), created.body());
    assert_eq!(runtime.event_count("ses_response_stale_replay"), 1);
}

#[test]
fn body_lines_cannot_supply_the_idempotency_header() {
    let release = published_release();
    let session = active_session(
        "ses_response_header_boundary",
        "ptc_response_header_boundary",
        &release,
    );
    let mut runtime =
        ResponseHttpRuntime::new(vec![session], vec![release], "evt_response_header_boundary");
    let body = "Idempotency-Key: idem_body_only";
    let request = format!(
        "POST /v1/sessions/ses_response_header_boundary/responses HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );

    let response = handle_response_http_request(&request, &mut runtime);

    assert_eq!(response.status(), 400);
    assert!(response
        .body()
        .contains("urn:psychometrics-commons:problem:missing-idempotency-key"));
    assert_eq!(runtime.event_count("ses_response_header_boundary"), 0);
}
