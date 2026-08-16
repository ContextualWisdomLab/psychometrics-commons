//! Public session-command HTTP contract for Activate, Pause, Resume, Complete, and Cancel.
//!
//! A purchaser who already has a Created Korean Big Five session Activates it,
//! records answers on the response family, then Completes. Exact
//! `Idempotency-Key` replay returns the original command outcome. Illegal
//! transitions, unknown sessions, and conflicting replays fail closed with
//! state-specific next actions. Session create, response writes, and persistence
//! stay other families.

use psychometrics_commons_runtime::instrument::{
    InstrumentRelease, InstrumentReleaseManifest, PublicationCommand,
    PublicationEvidenceProvenance, PublicationEvidenceRecord, PublicationEvidenceStatus,
};
use psychometrics_commons_runtime::session::{AssessmentSession, SessionCommand, SessionState};
use psychometrics_commons_runtime::session_command_http::{
    accept_one_session_command_http, bind_session_command_http,
    handle_session_command_http_request, SessionCommandHttpRuntime,
};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::thread;
use std::time::Duration;

const VALID_DIGEST: &str =
    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const EVIDENCE_DIGEST: &str =
    "sha256:1111111111111111111111111111111111111111111111111111111111111111";
const PARTICIPANT_REF: &str = "ptc_eb1b318917d24ca0ac5153c37ff696c7";
const SESSION_REF: &str = "ses_7c2f0a91d4b64e1f9a0c3e5d8b1a2468";
const ACTIVATE_KEY: &str = "idem_activate_session";
const PAUSE_KEY: &str = "idem_pause_session";
const RESUME_KEY: &str = "idem_resume_session";
const COMPLETE_KEY: &str = "idem_complete_session";
const CANCEL_KEY: &str = "idem_cancel_session";

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

fn created_session(release: &InstrumentRelease) -> AssessmentSession {
    AssessmentSession::new(
        SESSION_REF,
        PARTICIPANT_REF,
        release,
        "ko-KR",
        1_725_000_000_000,
    )
    .unwrap()
}

fn runtime_with(session: AssessmentSession) -> SessionCommandHttpRuntime {
    SessionCommandHttpRuntime::new(vec![session])
}

fn post_request(session_ref: &str, command: &str, idempotency_key: &str) -> String {
    let body = format!("{{\"command\":\"{command}\"}}");
    format!(
        "POST /v1/sessions/{session_ref}/commands HTTP/1.1\r\nHost: localhost\r\nIdempotency-Key: {idempotency_key}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    )
}

#[test]
fn activate_starts_created_korean_big_five_session() {
    let release = published_release();
    let mut runtime = runtime_with(created_session(&release));
    let response = handle_session_command_http_request(
        &post_request(SESSION_REF, "Activate", ACTIVATE_KEY),
        &mut runtime,
    );

    assert_eq!(response.status(), 201);
    assert_eq!(response.content_type(), "application/json");
    assert!(response
        .body()
        .contains(&format!("\"session_ref\":\"{SESSION_REF}\"")));
    assert!(response
        .body()
        .contains(&format!("\"command_ref\":\"{ACTIVATE_KEY}\"")));
    assert!(response.body().contains("\"command\":\"Activate\""));
    assert!(response.body().contains("\"sequence\":1"));
    assert!(response.body().contains("\"state\":\"Active\""));
    assert!(response
        .body()
        .contains("POST /v1/sessions/{session_ref}/responses"));
    assert_eq!(
        runtime.session_state(SESSION_REF).unwrap().as_str(),
        "Active"
    );
}

#[test]
fn exact_idempotent_replay_returns_the_original_command() {
    let release = published_release();
    let mut runtime = runtime_with(created_session(&release));
    let first = handle_session_command_http_request(
        &post_request(SESSION_REF, "Activate", ACTIVATE_KEY),
        &mut runtime,
    );
    let replay = handle_session_command_http_request(
        &post_request(SESSION_REF, "Activate", ACTIVATE_KEY),
        &mut runtime,
    );

    assert_eq!(first.status(), 201);
    assert_eq!(replay.status(), 200);
    assert_eq!(replay.body(), first.body());
    assert_eq!(
        runtime.session_state(SESSION_REF).unwrap().as_str(),
        "Active"
    );
}

#[test]
fn pause_resume_complete_and_cancel_follow_the_buyer_journey() {
    let release = published_release();
    let mut runtime = runtime_with(created_session(&release));
    assert_eq!(
        handle_session_command_http_request(
            &post_request(SESSION_REF, "Activate", ACTIVATE_KEY),
            &mut runtime,
        )
        .status(),
        201
    );

    let paused = handle_session_command_http_request(
        &post_request(SESSION_REF, "Pause", PAUSE_KEY),
        &mut runtime,
    );
    assert_eq!(paused.status(), 201);
    assert!(paused.body().contains("\"state\":\"Paused\""));
    assert!(paused
        .body()
        .contains("Resume the session before posting responses"));

    let resumed = handle_session_command_http_request(
        &post_request(SESSION_REF, "Resume", RESUME_KEY),
        &mut runtime,
    );
    assert_eq!(resumed.status(), 201);
    assert!(resumed.body().contains("\"state\":\"Active\""));
    assert!(resumed.body().contains("\"sequence\":3"));

    let completed = handle_session_command_http_request(
        &post_request(SESSION_REF, "Complete", COMPLETE_KEY),
        &mut runtime,
    );
    assert_eq!(completed.status(), 201);
    assert!(completed.body().contains("\"state\":\"Completed\""));
    assert!(completed.body().contains("do not reopen this session"));

    let replay_after_complete = handle_session_command_http_request(
        &post_request(SESSION_REF, "Activate", ACTIVATE_KEY),
        &mut runtime,
    );
    assert_eq!(replay_after_complete.status(), 200);
    assert!(replay_after_complete
        .body()
        .contains("\"state\":\"Active\""));
    assert_eq!(
        runtime.session_state(SESSION_REF).unwrap().as_str(),
        "Completed"
    );

    let mut cancellable = runtime_with(created_session(&release));
    let cancelled = handle_session_command_http_request(
        &post_request(SESSION_REF, "Cancel", CANCEL_KEY),
        &mut cancellable,
    );
    assert_eq!(cancelled.status(), 201);
    assert!(cancelled.body().contains("\"state\":\"Cancelled\""));
    assert!(cancelled
        .body()
        .contains("start a new session if another attempt is needed"));
}

#[test]
fn illegal_transitions_unknown_inputs_and_conflicts_fail_closed() {
    let release = published_release();
    let mut created = runtime_with(created_session(&release));
    let complete_created = handle_session_command_http_request(
        &post_request(SESSION_REF, "Complete", COMPLETE_KEY),
        &mut created,
    );
    assert_eq!(complete_created.status(), 409);
    assert_eq!(complete_created.content_type(), "application/problem+json");
    assert!(complete_created
        .body()
        .contains("urn:psychometrics-commons:problem:illegal-session-command"));
    assert!(complete_created
        .body()
        .contains("Activate the session before Completing"));
    assert_eq!(
        created.session_state(SESSION_REF).unwrap().as_str(),
        "Created"
    );

    let mut active = runtime_with(created_session(&release));
    assert_eq!(
        handle_session_command_http_request(
            &post_request(SESSION_REF, "Activate", ACTIVATE_KEY),
            &mut active,
        )
        .status(),
        201
    );
    assert_eq!(
        handle_session_command_http_request(
            &post_request(SESSION_REF, "Complete", COMPLETE_KEY),
            &mut active,
        )
        .status(),
        201
    );
    let reopen = handle_session_command_http_request(
        &post_request(SESSION_REF, "Pause", PAUSE_KEY),
        &mut active,
    );
    assert_eq!(reopen.status(), 409);
    assert!(reopen.body().contains("do not reopen a Completed session"));

    let mut conflict = runtime_with(created_session(&release));
    assert_eq!(
        handle_session_command_http_request(
            &post_request(SESSION_REF, "Activate", ACTIVATE_KEY),
            &mut conflict,
        )
        .status(),
        201
    );
    let reused = handle_session_command_http_request(
        &post_request(SESSION_REF, "Pause", ACTIVATE_KEY),
        &mut conflict,
    );
    assert_eq!(reused.status(), 409);
    assert!(reused
        .body()
        .contains("urn:psychometrics-commons:problem:idempotency-conflict"));
    assert!(!reused.body().contains("Pause"));
    assert_eq!(
        conflict.session_state(SESSION_REF).unwrap().as_str(),
        "Active"
    );
}

#[test]
fn unknown_sessions_and_malformed_requests_fail_closed() {
    let release = published_release();
    let mut conflict = runtime_with(created_session(&release));
    assert_eq!(
        handle_session_command_http_request(
            &post_request(SESSION_REF, "Activate", ACTIVATE_KEY),
            &mut conflict,
        )
        .status(),
        201
    );

    let missing = handle_session_command_http_request(
        &post_request("ses_missing_session", "Activate", "idem_missing"),
        &mut conflict,
    );
    assert_eq!(missing.status(), 404);
    assert!(missing
        .body()
        .contains("urn:psychometrics-commons:problem:session-not-found"));
    assert!(missing
        .body()
        .contains("Use GET /v1/sessions/{session_ref} to confirm the session exists"));

    let missing_key = handle_session_command_http_request(
        "POST /v1/sessions/ses_7c2f0a91d4b64e1f9a0c3e5d8b1a2468/commands HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: 22\r\n\r\n{\"command\":\"Activate\"}",
        &mut conflict,
    );
    assert_eq!(missing_key.status(), 400);
    assert!(missing_key
        .body()
        .contains("urn:psychometrics-commons:problem:missing-idempotency-key"));

    let unknown_command = handle_session_command_http_request(
        &post_request(SESSION_REF, "BeginScoring", "idem_begin_scoring"),
        &mut conflict,
    );
    assert_eq!(unknown_command.status(), 400);
    assert!(unknown_command
        .body()
        .contains("Activate, Pause, Resume, Complete, or Cancel"));

    let numeric = handle_session_command_http_request(
        &post_request("42", "Activate", "idem_numeric_session"),
        &mut conflict,
    );
    assert_eq!(numeric.status(), 400);

    let encoded = handle_session_command_http_request(
        &post_request("%20ses_padded", "Activate", "idem_encoded"),
        &mut conflict,
    );
    assert_eq!(encoded.status(), 400);

    let scientific_key = handle_session_command_http_request(
        &post_request(SESSION_REF, "Pause", "1e2"),
        &mut conflict,
    );
    assert_eq!(scientific_key.status(), 400);
    assert!(scientific_key
        .body()
        .contains("urn:psychometrics-commons:problem:invalid-reference"));

    let empty = handle_session_command_http_request("", &mut conflict);
    assert_eq!(empty.status(), 400);

    let short_body = handle_session_command_http_request(
        "POST /v1/sessions/ses_7c2f0a91d4b64e1f9a0c3e5d8b1a2468/commands HTTP/1.1\r\nHost: localhost\r\nIdempotency-Key: idem_short\r\nContent-Length: 22\r\n\r\n{\"command\":\"Acti",
        &mut conflict,
    );
    assert_eq!(short_body.status(), 400);

    let get = handle_session_command_http_request(
        &format!("GET /v1/sessions/{SESSION_REF}/commands HTTP/1.1\r\nHost: localhost\r\n\r\n"),
        &mut conflict,
    );
    assert_eq!(get.status(), 405);

    let other_family = handle_session_command_http_request(
        "POST /v1/sessions HTTP/1.1\r\nHost: localhost\r\n\r\n",
        &mut conflict,
    );
    assert_eq!(other_family.status(), 404);
}

#[test]
fn listener_reads_split_packet_body_before_activating() {
    let release = published_release();
    let mut runtime = runtime_with(created_session(&release));
    let listener = bind_session_command_http(SocketAddr::from(([127, 0, 0, 1], 0))).unwrap();
    let address = listener.local_addr().unwrap();
    let body = "{\"command\":\"Activate\"}";
    let headers = format!(
        "POST /v1/sessions/{SESSION_REF}/commands HTTP/1.1\r\nHost: localhost\r\nIdempotency-Key: {ACTIVATE_KEY}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
        body.len()
    );
    let server = thread::spawn(move || accept_one_session_command_http(&listener, &mut runtime));

    let mut client = TcpStream::connect(address).unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    client
        .set_write_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    client.write_all(headers.as_bytes()).unwrap();
    client.flush().unwrap();
    thread::sleep(Duration::from_millis(20));
    client.write_all(body.as_bytes()).unwrap();
    let mut received = String::new();
    client.read_to_string(&mut received).unwrap();
    server.join().unwrap().unwrap();

    assert!(received.starts_with("HTTP/1.1 201 Created"));
    assert!(received.contains("\"state\":\"Active\""));
    assert!(received.contains("application/json"));
}

#[test]
fn domain_command_sequence_helpers_support_http_replay() {
    let release = published_release();
    let mut session = created_session(&release);
    assert_eq!(session.next_command_sequence(), 1);
    assert_eq!(session.accepted_command_sequence(ACTIVATE_KEY), None);
    session
        .apply_command(ACTIVATE_KEY, 1, SessionCommand::Activate)
        .unwrap();
    assert_eq!(session.next_command_sequence(), 2);
    assert_eq!(session.accepted_command_sequence(ACTIVATE_KEY), Some(1));
    assert_eq!(session.state().as_str(), "Active");
    assert_eq!(SessionState::Created.as_str(), "Created");
    assert_eq!(SessionState::Paused.as_str(), "Paused");
    assert_eq!(SessionState::Completed.as_str(), "Completed");
    assert_eq!(SessionState::Scoring.as_str(), "Scoring");
    assert_eq!(SessionState::Scored.as_str(), "Scored");
    assert_eq!(SessionState::Released.as_str(), "Released");
    assert_eq!(SessionState::Expired.as_str(), "Expired");
    assert_eq!(SessionState::Cancelled.as_str(), "Cancelled");
    assert_eq!(SessionState::Invalidated.as_str(), "Invalidated");
}

#[test]
fn listener_rejects_get_with_allow_post() {
    let release = published_release();
    let mut runtime = runtime_with(created_session(&release));
    let listener = bind_session_command_http(SocketAddr::from(([127, 0, 0, 1], 0))).unwrap();
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || accept_one_session_command_http(&listener, &mut runtime));

    let mut client = TcpStream::connect(address).unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    client
        .set_write_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    client
        .write_all(
            format!("GET /v1/sessions/{SESSION_REF}/commands HTTP/1.1\r\nHost: localhost\r\n\r\n")
                .as_bytes(),
        )
        .unwrap();
    let mut received = String::new();
    client.read_to_string(&mut received).unwrap();
    server.join().unwrap().unwrap();

    assert!(received.starts_with("HTTP/1.1 405 Method Not Allowed"));
    assert!(received.contains("Allow: POST"));
    assert!(received.contains("application/problem+json"));
}
