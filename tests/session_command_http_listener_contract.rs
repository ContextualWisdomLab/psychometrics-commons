//! Listener contract for one-shot session-command HTTP.

use psychometrics_commons_runtime::authorization::{AuthorizationContext, ProductRole};
use psychometrics_commons_runtime::instrument::{
    InstrumentRelease, InstrumentReleaseManifest, PublicationCommand,
    PublicationEvidenceProvenance, PublicationEvidenceRecord, PublicationEvidenceStatus,
};
use psychometrics_commons_runtime::participant::ParticipantRecord;
use psychometrics_commons_runtime::session::AssessmentSession;
use psychometrics_commons_runtime::session_command_http::{
    accept_one_authorized_session_command_http, accept_one_session_command_http,
    bind_session_command_http, SessionCommandAuthority, SessionCommandHttpRuntime,
};
use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

const TENANT_REF: &str = "tenant_session_command_listener";
const RELEASE_DIGEST: &str =
    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const EVIDENCE_DIGEST: &str =
    "sha256:1111111111111111111111111111111111111111111111111111111111111111";

fn published_release() -> InstrumentRelease {
    let manifest = InstrumentReleaseManifest::new(
        "release_big_five_en_v1",
        "instrument_big_five",
        "instrument_version_big_five_en_v1",
        "construct_big_five",
        &["item_version_001"],
        "en-US",
        "assessment_spec_big_five_v1",
        "scoring_version_big_five_v1",
        "calibration_big_five_en_v1",
        Some("norm_version_big_five_en_v1"),
        "narrative_version_big_five_v1",
        &["consent_service_v1"],
        "intended_use_self_reflection_v1",
        "limitations_nonclinical_v1",
        RELEASE_DIGEST,
    )
    .unwrap();
    let evidence = PublicationEvidenceRecord::new(
        "publication_evidence_big_five_en_v1",
        "evidence_policy_self_reflection_v1",
        "release_big_five_en_v1",
        "instrument_version_big_five_en_v1",
        &["item_version_001"],
        RELEASE_DIGEST,
        "en-US",
        "intended_use_self_reflection_v1",
        "assessment_spec_big_five_v1",
        "scoring_version_big_five_v1",
        "calibration_big_five_en_v1",
        Some("norm_version_big_five_en_v1"),
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
        &["recovery_big_five_en_v1"],
        &["approval_psychometrics_big_five_en_v1"],
        PublicationEvidenceStatus::Approved,
    )
    .unwrap();
    let mut release = InstrumentRelease::new(manifest, 10_000).unwrap();
    release
        .apply_command(
            "publication_review_en_11d5",
            PublicationCommand::SubmitReview,
            10_100,
        )
        .unwrap();
    release.bind_publication_evidence(evidence).unwrap();
    release
        .apply_command(
            "publication_publish_en_20f6",
            PublicationCommand::Publish,
            10_200,
        )
        .unwrap();
    release
}

fn serve_authorized(
    listener: &TcpListener,
    participant_ref: &str,
    runtime: &mut SessionCommandHttpRuntime,
) -> io::Result<()> {
    let participant =
        ParticipantRecord::new_anonymous(participant_ref, TENANT_REF, 19_000).unwrap();
    let actor = AuthorizationContext::new(
        TENANT_REF,
        "subject_session_command_listener",
        Some(participant_ref),
        &[ProductRole::Participant],
    )
    .unwrap();
    let authority = SessionCommandAuthority::Authenticated(&actor);
    accept_one_authorized_session_command_http(listener, &authority, &participant, runtime)
}

fn assert_bad_request_response(response: &str) {
    assert!(response.starts_with("HTTP/1.1 400 Bad Request"));
    assert!(response.contains("Content-Type: application/problem+json"));
    assert!(response.contains("urn:psychometrics-commons:problem:bad-request"));
}

#[test]
fn listener_activates_a_created_session_over_tcp() {
    let session = AssessmentSession::new(
        "ses_listener_command_one",
        "ptc_listener_command_one",
        &published_release(),
        "en-US",
        20_000,
    )
    .unwrap();
    let mut runtime = SessionCommandHttpRuntime::new(vec![session]);
    let listener = bind_session_command_http(SocketAddr::from(([127, 0, 0, 1], 0))).unwrap();
    let addr = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        serve_authorized(&listener, "ptc_listener_command_one", &mut runtime)
    });

    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(2)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let body = "{\"command\":\"activate\"}";
    let request = format!(
        "POST /v1/sessions/ses_listener_command_one/commands HTTP/1.1\r\nIdempotency-Key: cmd_listener_activate\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(request.as_bytes()).unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    server.join().unwrap().unwrap();
    assert!(response.starts_with("HTTP/1.1 200 OK"));
    assert!(response.contains("\"state\":\"active\""));
    assert!(response.contains("Cache-Control: no-store"));
}

#[test]
fn listener_rejects_get_with_allow_post() {
    let session = AssessmentSession::new(
        "ses_listener_command_get",
        "ptc_listener_command_get",
        &published_release(),
        "en-US",
        20_000,
    )
    .unwrap();
    let mut runtime = SessionCommandHttpRuntime::new(vec![session]);
    let listener = bind_session_command_http(SocketAddr::from(([127, 0, 0, 1], 0))).unwrap();
    let addr = listener.local_addr().unwrap();
    let server = thread::spawn(move || accept_one_session_command_http(&listener, &mut runtime));

    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(2)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    stream
        .write_all(b"GET /v1/sessions/ses_listener_command_get/commands HTTP/1.1\r\n\r\n")
        .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    server.join().unwrap().unwrap();
    assert!(response.starts_with("HTTP/1.1 405 Method Not Allowed"));
    assert!(response.contains("Allow: POST"));
}

#[test]
fn listener_activates_across_padded_multi_chunk_headers() {
    let session = AssessmentSession::new(
        "ses_listener_command_pad",
        "ptc_listener_command_pad",
        &published_release(),
        "en-US",
        20_000,
    )
    .unwrap();
    let mut runtime = SessionCommandHttpRuntime::new(vec![session]);
    let listener = bind_session_command_http(SocketAddr::from(([127, 0, 0, 1], 0))).unwrap();
    let addr = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        serve_authorized(&listener, "ptc_listener_command_pad", &mut runtime)
    });

    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(2)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    // Padding pushes the header block past the 512-byte read chunk so the
    // listener must keep reading before it can dispatch the activate command.
    let padding = "a".repeat(700);
    let body = "{\"command\":\"activate\"}";
    let request = format!(
        "POST /v1/sessions/ses_listener_command_pad/commands HTTP/1.1\r\nIdempotency-Key: cmd_listener_padded_activate\r\nX-Pad: {padding}\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(request.as_bytes()).unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    server.join().unwrap().unwrap();
    assert!(response.starts_with("HTTP/1.1 200 OK"));
    assert!(response.contains("\"state\":\"active\""));
}

#[test]
fn listener_waits_for_a_declared_body_delivered_after_the_headers() {
    let session = AssessmentSession::new(
        "ses_listener_command_delayed_body",
        "ptc_listener_command_delayed_body",
        &published_release(),
        "en-US",
        20_000,
    )
    .unwrap();
    let mut runtime = SessionCommandHttpRuntime::new(vec![session]);
    let listener = bind_session_command_http(SocketAddr::from(([127, 0, 0, 1], 0))).unwrap();
    let addr = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        serve_authorized(&listener, "ptc_listener_command_delayed_body", &mut runtime)
    });

    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(2)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let body = "{\"command\":\"activate\"}";
    let headers = format!(
        "POST /v1/sessions/ses_listener_command_delayed_body/commands HTTP/1.1\r\nIdempotency-Key: cmd_listener_delayed_body\r\nContent-Length: {}\r\n\r\n",
        body.len()
    );
    stream.write_all(headers.as_bytes()).unwrap();
    stream.flush().unwrap();
    thread::sleep(Duration::from_millis(50));
    stream.write_all(body.as_bytes()).unwrap();

    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    server.join().unwrap().unwrap();
    assert!(response.starts_with("HTTP/1.1 200 OK"));
    assert!(response.contains("\"state\":\"active\""));
}

#[test]
fn listener_rejects_a_content_length_that_splits_a_utf8_code_point_without_panicking() {
    let session = AssessmentSession::new(
        "ses_listener_command_utf8_boundary",
        "ptc_listener_command_utf8_boundary",
        &published_release(),
        "en-US",
        20_000,
    )
    .unwrap();
    let mut runtime = SessionCommandHttpRuntime::new(vec![session]);
    let listener = bind_session_command_http(SocketAddr::from(([127, 0, 0, 1], 0))).unwrap();
    let addr = listener.local_addr().unwrap();
    let server = thread::spawn(move || accept_one_session_command_http(&listener, &mut runtime));

    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(2)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let body = "{\"command\":\"activaté\"}";
    let split_inside_e_acute = body.find('é').unwrap() + 1;
    let headers = format!(
        "POST /v1/sessions/ses_listener_command_utf8_boundary/commands HTTP/1.1\r\nIdempotency-Key: cmd_listener_utf8_boundary\r\nContent-Length: {split_inside_e_acute}\r\n\r\n"
    );
    stream.write_all(headers.as_bytes()).unwrap();
    stream
        .write_all(&body.as_bytes()[..split_inside_e_acute])
        .unwrap();

    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    let error = server.join().unwrap().unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    assert_bad_request_response(&response);
}

#[test]
fn authorized_listener_returns_a_problem_for_invalid_framing() {
    let session = AssessmentSession::new(
        "ses_listener_command_authorized_utf8_boundary",
        "ptc_listener_command_authorized_utf8_boundary",
        &published_release(),
        "en-US",
        20_000,
    )
    .unwrap();
    let mut runtime = SessionCommandHttpRuntime::new(vec![session]);
    let listener = bind_session_command_http(SocketAddr::from(([127, 0, 0, 1], 0))).unwrap();
    let addr = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        serve_authorized(
            &listener,
            "ptc_listener_command_authorized_utf8_boundary",
            &mut runtime,
        )
    });

    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(2)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let body = "{\"command\":\"activaté\"}";
    let split_inside_e_acute = body.find('é').unwrap() + 1;
    let headers = format!(
        "POST /v1/sessions/ses_listener_command_authorized_utf8_boundary/commands HTTP/1.1\r\nIdempotency-Key: cmd_listener_authorized_utf8_boundary\r\nContent-Length: {split_inside_e_acute}\r\n\r\n"
    );
    stream.write_all(headers.as_bytes()).unwrap();
    stream
        .write_all(&body.as_bytes()[..split_inside_e_acute])
        .unwrap();

    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    let error = server.join().unwrap().unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    assert_bad_request_response(&response);
}

#[test]
fn listener_rejects_non_crlf_header_lines_before_dispatch() {
    for request in [
        b"GET / HTTP/1.1\nHost: localhost\r\n\r\n".as_slice(),
        b"GET / HTTP/1.1\rHost: localhost\r\n\r\n".as_slice(),
    ] {
        let listener = bind_session_command_http(SocketAddr::from(([127, 0, 0, 1], 0))).unwrap();
        let server_listener = listener.try_clone().unwrap();
        let server = thread::spawn(move || {
            let mut runtime = SessionCommandHttpRuntime::new(Vec::new());
            accept_one_session_command_http(&server_listener, &mut runtime)
        });

        let mut stream = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        stream.write_all(request).unwrap();
        let error = server.join().unwrap().unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }
}
