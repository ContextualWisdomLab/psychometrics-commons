//! Public HTTP contract for participant session lifecycle commands.

use psychometrics_commons_runtime::instrument::{
    InstrumentRelease, InstrumentReleaseManifest, PublicationCommand,
    PublicationEvidenceProvenance, PublicationEvidenceRecord, PublicationEvidenceStatus,
};
use psychometrics_commons_runtime::session::{AssessmentSession, SessionCommand, SessionState};
use psychometrics_commons_runtime::session_command_http::{
    handle_session_command_http_request, SessionCommandHttpRuntime,
};

const RELEASE_DIGEST: &str =
    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const EVIDENCE_DIGEST: &str =
    "sha256:1111111111111111111111111111111111111111111111111111111111111111";
const SESSION_REF: &str = "ses_3d657ef743a54698868e4b6ee6c49af4";
const PARTICIPANT_REF: &str = "ptc_471a8fd35e1747b7b25b66d219ce4ccd";

fn published_release() -> InstrumentRelease {
    let manifest = InstrumentReleaseManifest::new(
        "release_big_five_ko_v1",
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
        RELEASE_DIGEST,
    )
    .unwrap();
    let evidence = PublicationEvidenceRecord::new(
        "publication_evidence_big_five_ko_v1",
        "evidence_policy_self_reflection_v1",
        "release_big_five_ko_v1",
        "instrument_version_big_five_ko_v1",
        &["item_version_001"],
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
            "publication_review_11d5b1e7",
            PublicationCommand::SubmitReview,
            10_100,
        )
        .unwrap();
    release.bind_publication_evidence(evidence).unwrap();
    release
        .apply_command(
            "publication_publish_20f6c2a8",
            PublicationCommand::Publish,
            10_200,
        )
        .unwrap();
    release
}

fn created_session() -> AssessmentSession {
    AssessmentSession::new(
        SESSION_REF,
        PARTICIPANT_REF,
        &published_release(),
        "ko-KR",
        20_000,
    )
    .unwrap()
}

fn runtime_with(session: AssessmentSession) -> SessionCommandHttpRuntime {
    SessionCommandHttpRuntime::new(vec![session])
}

fn post(session_ref: &str, command: &str, idempotency_key: &str) -> String {
    let body = format!("{{\"command\":\"{command}\"}}");
    format!(
        "POST /v1/sessions/{session_ref}/commands HTTP/1.1\r\nIdempotency-Key: {idempotency_key}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    )
}

#[test]
fn activate_pause_resume_complete_and_exact_replay_keep_server_state() {
    let mut runtime = runtime_with(created_session());

    let activated = handle_session_command_http_request(
        &post(SESSION_REF, "activate", "cmd_activate_primary"),
        &mut runtime,
    );
    assert_eq!(activated.status(), 200);
    assert_eq!(activated.content_type(), "application/json");
    assert!(activated.body().contains("\"state\":\"active\""));
    assert!(activated.body().contains("\"sequence\":1"));
    assert!(activated
        .body()
        .contains("POST /v1/sessions/{session_ref}/responses"));
    assert_eq!(
        runtime.session(SESSION_REF).unwrap().state(),
        SessionState::Active
    );

    let replay = handle_session_command_http_request(
        &post(SESSION_REF, "activate", "cmd_activate_primary"),
        &mut runtime,
    );
    assert_eq!(replay.status(), 200);
    assert!(replay.body().contains("\"sequence\":1"));
    assert_eq!(
        runtime.session(SESSION_REF).unwrap().state(),
        SessionState::Active
    );

    let paused = handle_session_command_http_request(
        &post(SESSION_REF, "pause", "cmd_pause_primary"),
        &mut runtime,
    );
    assert_eq!(paused.status(), 200);
    assert!(paused.body().contains("\"state\":\"paused\""));
    assert!(paused.body().contains("POST resume"));

    let resumed = handle_session_command_http_request(
        &post(SESSION_REF, "resume", "cmd_resume_primary"),
        &mut runtime,
    );
    assert_eq!(resumed.status(), 200);
    assert!(resumed.body().contains("\"state\":\"active\""));

    let completed = handle_session_command_http_request(
        &post(SESSION_REF, "complete", "cmd_complete_primary"),
        &mut runtime,
    );
    assert_eq!(completed.status(), 200);
    assert!(completed.body().contains("\"state\":\"completed\""));
    assert!(completed.body().contains("GET /v1/results/{result_ref}"));
    assert_eq!(
        runtime.session(SESSION_REF).unwrap().state(),
        SessionState::Completed
    );
}

#[test]
fn cancel_stops_a_created_session_and_conflicting_replay_fails_closed() {
    let mut runtime = runtime_with(created_session());
    let cancelled = handle_session_command_http_request(
        &post(SESSION_REF, "cancel", "cmd_cancel_primary"),
        &mut runtime,
    );
    assert_eq!(cancelled.status(), 200);
    assert!(cancelled.body().contains("\"state\":\"cancelled\""));
    assert_eq!(
        runtime.session(SESSION_REF).unwrap().state(),
        SessionState::Cancelled
    );

    let conflict = handle_session_command_http_request(
        &post(SESSION_REF, "activate", "cmd_cancel_primary"),
        &mut runtime,
    );
    assert_eq!(conflict.status(), 409);
    assert_eq!(conflict.content_type(), "application/problem+json");
    assert!(conflict.body().contains("Idempotency-Key was reused"));
}

#[test]
fn illegal_and_non_public_commands_tell_the_purchaser_the_next_action() {
    let mut runtime = runtime_with(created_session());
    let pause_first = handle_session_command_http_request(
        &post(SESSION_REF, "pause", "cmd_pause_too_early"),
        &mut runtime,
    );
    assert_eq!(pause_first.status(), 409);
    assert!(pause_first.body().contains("POST activate before pause"));

    let scoring = handle_session_command_http_request(
        &post(SESSION_REF, "begin_scoring", "cmd_score_too_early"),
        &mut runtime,
    );
    assert_eq!(scoring.status(), 409);
    assert!(scoring.body().contains("Command Not Public"));

    let mut paused = created_session();
    paused
        .apply_client_command("cmd_activate_for_pause", SessionCommand::Activate)
        .unwrap();
    paused
        .apply_client_command("cmd_pause_for_complete", SessionCommand::Pause)
        .unwrap();
    let mut paused_runtime = runtime_with(paused);
    let complete_while_paused = handle_session_command_http_request(
        &post(SESSION_REF, "complete", "cmd_complete_while_paused"),
        &mut paused_runtime,
    );
    assert_eq!(complete_while_paused.status(), 409);
    assert!(complete_while_paused.body().contains("POST resume"));
}

#[test]
fn malformed_identity_method_and_missing_session_fail_closed() {
    let mut runtime = runtime_with(created_session());

    let missing_key = handle_session_command_http_request(
        "POST /v1/sessions/ses_3d657ef743a54698868e4b6ee6c49af4/commands HTTP/1.1\r\nContent-Length: 22\r\n\r\n{\"command\":\"activate\"}",
        &mut runtime,
    );
    assert_eq!(missing_key.status(), 400);
    assert!(missing_key.body().contains("Idempotency-Key"));

    let numeric = handle_session_command_http_request(
        &post("12", "activate", "cmd_numeric_session"),
        &mut runtime,
    );
    assert_eq!(numeric.status(), 400);
    assert!(numeric.body().contains("opaque non-numeric"));

    let encoded = handle_session_command_http_request(
        &post("ses_one%2Ftwo", "activate", "cmd_encoded_session"),
        &mut runtime,
    );
    assert_eq!(encoded.status(), 400);

    let unknown = handle_session_command_http_request(
        &post("ses_missing_session_row", "activate", "cmd_missing_session"),
        &mut runtime,
    );
    assert_eq!(unknown.status(), 404);
    assert!(unknown.body().contains("GET /v1/sessions/{session_ref}"));

    let get = handle_session_command_http_request(
        "GET /v1/sessions/ses_3d657ef743a54698868e4b6ee6c49af4/commands HTTP/1.1\r\n\r\n",
        &mut runtime,
    );
    assert_eq!(get.status(), 405);

    let other = handle_session_command_http_request(
        "GET /v1/results/res_one HTTP/1.1\r\n\r\n",
        &mut runtime,
    );
    assert_eq!(other.status(), 404);

    let with_query = handle_session_command_http_request(
        "POST /v1/sessions/ses_3d657ef743a54698868e4b6ee6c49af4/commands?x=1 HTTP/1.1\r\nIdempotency-Key: cmd_query_activate\r\nContent-Length: 22\r\n\r\n{\"command\":\"activate\"}",
        &mut runtime,
    );
    assert_eq!(with_query.status(), 200);

    let bad_line = handle_session_command_http_request("NOT-A-REQUEST", &mut runtime);
    assert_eq!(bad_line.status(), 400);

    let bad_body = handle_session_command_http_request(
        "POST /v1/sessions/ses_3d657ef743a54698868e4b6ee6c49af4/commands HTTP/1.1\r\nIdempotency-Key: cmd_bad_body\r\nContent-Length: 2\r\n\r\n[]",
        &mut runtime,
    );
    assert_eq!(bad_body.status(), 400);

    let whitespace_ref = handle_session_command_http_request(
        &post("ses_one two", "activate", "cmd_whitespace_session"),
        &mut runtime,
    );
    assert_eq!(whitespace_ref.status(), 400);
}

#[test]
fn apply_client_command_rejects_a_blank_command_identity() {
    let mut session = created_session();
    assert!(session
        .apply_client_command(" ", SessionCommand::Activate)
        .is_err());
}
