//! Public response HTTP contract for recording answers on an active session.
//!
//! A purchaser who already has an Active Korean Big Five session posts one
//! answer for an item that belongs to that published release. Exact
//! `Idempotency-Key` replay returns the original event. Created, paused, or
//! unknown sessions, items outside the release, and conflicting replays fail
//! closed. Session create, catalog list, and persistence stay other families.

use psychometrics_commons_runtime::instrument::{
    InstrumentRelease, InstrumentReleaseManifest, PublicationCommand,
    PublicationEvidenceProvenance, PublicationEvidenceRecord, PublicationEvidenceStatus,
};
use psychometrics_commons_runtime::response_http::{
    accept_one_response_http, bind_response_http, handle_response_http_request, ResponseHttpRuntime,
};
use psychometrics_commons_runtime::session::{AssessmentSession, SessionCommand};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

const VALID_DIGEST: &str =
    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const EVIDENCE_DIGEST: &str =
    "sha256:1111111111111111111111111111111111111111111111111111111111111111";
const PAYLOAD_DIGEST: &str =
    "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const OTHER_PAYLOAD_DIGEST: &str =
    "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const PARTICIPANT_REF: &str = "ptc_eb1b318917d24ca0ac5153c37ff696c7";
const SESSION_REF: &str = "ses_7c2f0a91d4b64e1f9a0c3e5d8b1a2468";
const SERVER_EVENT_REF: &str = "evt_response_item_001";
const IDEMPOTENCY_KEY: &str = "idem_response_item_001";
const ITEM_VERSION_REF: &str = "item_version_001";

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

fn active_session(release: &InstrumentRelease) -> AssessmentSession {
    let mut session = created_session(release);
    session
        .apply_command("cmd_activate_session", 1, SessionCommand::Activate)
        .unwrap();
    session
}

fn paused_session(release: &InstrumentRelease) -> AssessmentSession {
    let mut session = active_session(release);
    session
        .apply_command("cmd_pause_session", 2, SessionCommand::Pause)
        .unwrap();
    session
}

fn runtime_with(session: AssessmentSession, release: InstrumentRelease) -> ResponseHttpRuntime {
    ResponseHttpRuntime::new(vec![session], vec![release], SERVER_EVENT_REF)
}

fn post_request(session_ref: &str, body: &str, idempotency_key: &str) -> String {
    format!(
        "POST /v1/sessions/{session_ref}/responses HTTP/1.1\r\nHost: localhost\r\nIdempotency-Key: {idempotency_key}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    )
}

fn valid_body() -> String {
    format!(
        "{{\"item_version_ref\":\"{ITEM_VERSION_REF}\",\"payload_digest\":\"{PAYLOAD_DIGEST}\"}}"
    )
}

#[test]
fn post_records_answer_for_active_korean_big_five_item() {
    let release = published_release();
    let mut runtime = runtime_with(active_session(&release), release);
    let response = handle_response_http_request(
        &post_request(SESSION_REF, &valid_body(), IDEMPOTENCY_KEY),
        &mut runtime,
    );

    assert_eq!(response.status(), 201);
    assert_eq!(response.content_type(), "application/json");
    assert!(response
        .body()
        .contains(&format!("\"session_ref\":\"{SESSION_REF}\"")));
    assert!(response
        .body()
        .contains(&format!("\"server_event_ref\":\"{SERVER_EVENT_REF}\"")));
    assert!(response
        .body()
        .contains(&format!("\"client_event_ref\":\"{IDEMPOTENCY_KEY}\"")));
    assert!(response
        .body()
        .contains(&format!("\"item_version_ref\":\"{ITEM_VERSION_REF}\"")));
    assert!(response
        .body()
        .contains(&format!("\"payload_digest\":\"{PAYLOAD_DIGEST}\"")));
    assert!(response.body().contains("\"sequence\":1"));
    assert_eq!(runtime.event_count(SESSION_REF), 1);
}

#[test]
fn exact_idempotent_replay_returns_the_original_event() {
    let release = published_release();
    let mut runtime = runtime_with(active_session(&release), release);
    let first = handle_response_http_request(
        &post_request(SESSION_REF, &valid_body(), IDEMPOTENCY_KEY),
        &mut runtime,
    );
    runtime.replace_next_server_event_ref("evt_should_not_be_minted");
    let replay = handle_response_http_request(
        &post_request(SESSION_REF, &valid_body(), IDEMPOTENCY_KEY),
        &mut runtime,
    );

    assert_eq!(first.status(), 201);
    assert_eq!(replay.status(), 200);
    assert_eq!(replay.body(), first.body());
    assert_eq!(runtime.event_count(SESSION_REF), 1);
}

#[test]
fn created_paused_unknown_and_foreign_item_fail_closed() {
    let release = published_release();
    let mut created = runtime_with(created_session(&release), release.clone());
    let created_response = handle_response_http_request(
        &post_request(SESSION_REF, &valid_body(), IDEMPOTENCY_KEY),
        &mut created,
    );
    assert_eq!(created_response.status(), 409);
    assert_eq!(created_response.content_type(), "application/problem+json");
    assert!(created_response
        .body()
        .contains("urn:psychometrics-commons:problem:session-not-active"));
    assert!(created_response
        .body()
        .contains("Activate the session before posting responses"));
    assert_eq!(created.event_count(SESSION_REF), 0);

    let mut paused = runtime_with(paused_session(&release), release.clone());
    let paused_response = handle_response_http_request(
        &post_request(SESSION_REF, &valid_body(), "idem_paused"),
        &mut paused,
    );
    assert_eq!(paused_response.status(), 409);
    assert!(paused_response
        .body()
        .contains("urn:psychometrics-commons:problem:session-not-active"));
    assert_eq!(paused.event_count(SESSION_REF), 0);

    let mut missing = runtime_with(active_session(&release), release.clone());
    let missing_response = handle_response_http_request(
        &post_request("ses_missing_session", &valid_body(), "idem_missing"),
        &mut missing,
    );
    assert_eq!(missing_response.status(), 404);
    assert!(missing_response
        .body()
        .contains("urn:psychometrics-commons:problem:session-not-found"));
    assert!(missing_response
        .body()
        .contains("Use GET /v1/sessions/{session_ref} to confirm the session exists"));

    let mut foreign = runtime_with(active_session(&release), release);
    let foreign_body =
        "{\"item_version_ref\":\"item_version_999\",\"payload_digest\":\"sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc\"}";
    let foreign_response = handle_response_http_request(
        &post_request(SESSION_REF, foreign_body, "idem_foreign"),
        &mut foreign,
    );
    assert_eq!(foreign_response.status(), 409);
    assert!(foreign_response
        .body()
        .contains("urn:psychometrics-commons:problem:item-not-in-release"));
    assert_eq!(foreign.event_count(SESSION_REF), 0);
}

#[test]
fn conflicting_replay_and_invalid_inputs_fail_closed_without_leaking_payloads() {
    let release = published_release();
    let mut runtime = runtime_with(active_session(&release), release);
    let first = handle_response_http_request(
        &post_request(SESSION_REF, &valid_body(), IDEMPOTENCY_KEY),
        &mut runtime,
    );
    assert_eq!(first.status(), 201);

    let conflict_body = format!(
        "{{\"item_version_ref\":\"{ITEM_VERSION_REF}\",\"payload_digest\":\"{OTHER_PAYLOAD_DIGEST}\"}}"
    );
    let conflict = handle_response_http_request(
        &post_request(SESSION_REF, &conflict_body, IDEMPOTENCY_KEY),
        &mut runtime,
    );
    assert_eq!(conflict.status(), 409);
    assert!(conflict
        .body()
        .contains("urn:psychometrics-commons:problem:idempotency-conflict"));
    assert!(!conflict.body().contains(OTHER_PAYLOAD_DIGEST));
    assert_eq!(runtime.event_count(SESSION_REF), 1);

    let missing_key = handle_response_http_request(
        &format!(
            "POST /v1/sessions/{SESSION_REF}/responses HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
            valid_body().len(),
            valid_body()
        ),
        &mut runtime,
    );
    assert_eq!(missing_key.status(), 400);
    assert!(missing_key
        .body()
        .contains("urn:psychometrics-commons:problem:missing-idempotency-key"));

    let numeric = handle_response_http_request(
        &post_request("42", &valid_body(), "idem_numeric_session"),
        &mut runtime,
    );
    assert_eq!(numeric.status(), 400);
    assert!(numeric
        .body()
        .contains("urn:psychometrics-commons:problem:bad-request"));

    let encoded = handle_response_http_request(
        &post_request("%20ses_padded", &valid_body(), "idem_encoded"),
        &mut runtime,
    );
    assert_eq!(encoded.status(), 400);

    let bad_digest = handle_response_http_request(
        &post_request(
            SESSION_REF,
            "{\"item_version_ref\":\"item_version_001\",\"payload_digest\":\"not-a-digest\"}",
            "idem_bad_digest",
        ),
        &mut runtime,
    );
    assert_eq!(bad_digest.status(), 400);
    assert!(bad_digest
        .body()
        .contains("urn:psychometrics-commons:problem:invalid-payload-digest"));

    let unknown_field = handle_response_http_request(
        &post_request(
            SESSION_REF,
            "{\"item_version_ref\":\"item_version_001\",\"payload_digest\":\"sha256:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd\",\"score\":1}",
            "idem_unknown_field",
        ),
        &mut runtime,
    );
    assert_eq!(unknown_field.status(), 400);

    let get = handle_response_http_request(
        &format!("GET /v1/sessions/{SESSION_REF}/responses HTTP/1.1\r\nHost: localhost\r\n\r\n"),
        &mut runtime,
    );
    assert_eq!(get.status(), 405);
    assert!(get
        .body()
        .contains("urn:psychometrics-commons:problem:method-not-allowed"));

    let other_family = handle_response_http_request(
        "POST /v1/sessions HTTP/1.1\r\nHost: localhost\r\n\r\n",
        &mut runtime,
    );
    assert_eq!(other_family.status(), 404);
}

#[test]
fn listener_serves_one_active_session_response() {
    let release = published_release();
    let mut runtime = runtime_with(active_session(&release), release);
    let listener = bind_response_http(SocketAddr::from(([127, 0, 0, 1], 0))).unwrap();
    let address = listener.local_addr().unwrap();
    let request = post_request(SESSION_REF, &valid_body(), IDEMPOTENCY_KEY);
    let server = std::thread::spawn(move || accept_one_response_http(&listener, &mut runtime));

    let mut client = TcpStream::connect(address).unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    client
        .set_write_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    client.write_all(request.as_bytes()).unwrap();
    let mut body = String::new();
    client.read_to_string(&mut body).unwrap();
    server.join().unwrap().unwrap();

    assert!(body.starts_with("HTTP/1.1 201 Created"));
    assert!(body.contains(SERVER_EVENT_REF));
    assert!(body.contains("application/json"));
}
