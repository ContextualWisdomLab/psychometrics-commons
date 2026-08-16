//! Bound TCP listener contract for public session create and reload.

use psychometrics_commons_runtime::instrument::{
    InstrumentRelease, InstrumentReleaseManifest, PublicationCommand,
    PublicationEvidenceProvenance, PublicationEvidenceRecord, PublicationEvidenceStatus,
};
use psychometrics_commons_runtime::session_http::{
    accept_one_session_http, bind_session_http, SessionHttpRuntime, SESSION_COLLECTION_PATH,
};
use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

const VALID_DIGEST: &str =
    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const EVIDENCE_DIGEST: &str =
    "sha256:1111111111111111111111111111111111111111111111111111111111111111";

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
        VALID_DIGEST,
    )
    .unwrap();
    let mut release = InstrumentRelease::new(manifest, 10_000).unwrap();
    release
        .apply_command(
            "publication_review_f9f86084",
            PublicationCommand::SubmitReview,
            10_100,
        )
        .unwrap();
    release
        .bind_publication_evidence(
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
            .unwrap(),
        )
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

fn exchange(addr: SocketAddr, request: &str) -> String {
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(2)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    stream.write_all(request.as_bytes()).unwrap();
    stream.shutdown(std::net::Shutdown::Write).unwrap();
    let mut body = String::new();
    stream.read_to_string(&mut body).unwrap();
    body
}

#[test]
fn bound_listener_creates_and_reloads_a_session_over_tcp() {
    let listener = bind_session_http(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0)).unwrap();
    let addr = listener.local_addr().unwrap();
    let runtime = Arc::new(Mutex::new(SessionHttpRuntime::new(
        vec![published_release()],
        "ses_tcp_created",
        1_725_000_000_000,
    )));
    let server_runtime = Arc::clone(&runtime);
    let server = thread::spawn(move || {
        let mut locked = server_runtime.lock().unwrap();
        accept_one_session_http(&listener, &mut locked).unwrap();
        accept_one_session_http(&listener, &mut locked).unwrap();
    });

    let body = "{\"participant_ref\":\"ptc_eb1b318917d24ca0ac5153c37ff696c7\",\"instrument_release_ref\":\"release_big_five_ko_v1\",\"locale\":\"ko-KR\"}";
    let created = exchange(
        addr,
        &format!(
            "POST {SESSION_COLLECTION_PATH} HTTP/1.1\r\nHost: localhost\r\nIdempotency-Key: idem_tcp\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        ),
    );
    assert!(created.starts_with("HTTP/1.1 201 Created\r\n"));
    assert!(created.contains("Content-Type: application/json\r\n"));
    assert!(created.contains("Connection: close\r\n"));
    assert!(created.contains("\"session_ref\":\"ses_tcp_created\""));
    assert!(created.contains("\"state\":\"created\""));

    let loaded = exchange(
        addr,
        "GET /v1/sessions/ses_tcp_created HTTP/1.1\r\nHost: localhost\r\n\r\n",
    );
    assert!(loaded.starts_with("HTTP/1.1 200 OK\r\n"));
    assert!(loaded.contains("\"session_ref\":\"ses_tcp_created\""));

    server.join().unwrap();
}
