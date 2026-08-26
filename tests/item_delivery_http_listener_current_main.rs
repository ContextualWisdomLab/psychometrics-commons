//! Bound listener contract for the current-main item-delivery HTTP surface.

use psychometrics_commons_runtime::instrument::InstrumentReleaseManifest;
use psychometrics_commons_runtime::item_delivery::ItemDeliveryLedger;
use psychometrics_commons_runtime::item_delivery_http::{
    accept_one_item_delivery_http, bind_item_delivery_http, ItemDeliveryHttpRuntime,
};
use psychometrics_commons_runtime::session::{AssessmentSession, SessionCommand};
use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, Shutdown, SocketAddr, TcpStream};
use std::thread;
use std::time::Duration;

const SESSION_REF: &str = "ses_item_delivery_listener";
const DIGEST: &str =
    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn runtime() -> ItemDeliveryHttpRuntime {
    let manifest = InstrumentReleaseManifest::new(
        "release_big_five_ko_listener",
        "instrument_big_five",
        "instrument_version_big_five_ko_listener",
        "construct_big_five",
        &["item_version_001"],
        "ko-KR",
        "assessment_spec_big_five_listener",
        "scoring_version_big_five_listener",
        "calibration_big_five_listener",
        None,
        "narrative_version_big_five_listener",
        &["consent_service_listener"],
        "intended_use_self_reflection_listener",
        "limitations_nonclinical_listener",
        DIGEST,
    )
    .unwrap();
    let mut session = AssessmentSession::from_currently_published_manifest(
        SESSION_REF,
        "participant_listener",
        &manifest,
        "ko-KR",
        1,
    )
    .unwrap();
    session
        .apply_command("command_activate_listener", 1, SessionCommand::Activate)
        .unwrap();
    let ledger = ItemDeliveryLedger::from_manifest(SESSION_REF, &manifest).unwrap();
    let mut runtime = ItemDeliveryHttpRuntime::new();
    runtime.insert_session(session, ledger).unwrap();
    runtime
}

fn exchange(addr: SocketAddr, request: &str) -> String {
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(2)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    stream.write_all(request.as_bytes()).unwrap();
    stream.shutdown(Shutdown::Write).unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    response
}

#[test]
fn bound_listener_records_one_delivery_and_returns_no_store_response() {
    let listener =
        bind_item_delivery_http(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0)).unwrap();
    let addr = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let mut runtime = runtime();
        accept_one_item_delivery_http(&listener, &mut runtime).unwrap();
        assert_eq!(runtime.event_count(SESSION_REF), 1);
    });

    let body = "{\"delivery_ref\":\"delivery_listener_001\",\"item_version_ref\":\"item_version_001\",\"presentation_context_ref\":\"presentation_web_listener\"}";
    let request = format!(
        "POST /v1/sessions/{SESSION_REF}/item-deliveries HTTP/1.1\r\nHost: localhost\r\nIdempotency-Key: delivery_listener_001\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );
    let response = exchange(addr, &request);
    assert!(response.starts_with("HTTP/1.1 201 Created\r\n"));
    assert!(response.contains("Cache-Control: no-store\r\n"));
    assert!(response.contains("Connection: close\r\n"));
    assert!(response.contains("\"sequence\":1"));
    server.join().unwrap();
}

#[test]
fn bound_listener_rejects_transfer_encoding_before_application_dispatch() {
    let listener =
        bind_item_delivery_http(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0)).unwrap();
    let addr = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let mut runtime = runtime();
        let error = accept_one_item_delivery_http(&listener, &mut runtime).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
        assert_eq!(runtime.event_count(SESSION_REF), 0);
    });

    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(2)).unwrap();
    stream
        .write_all(
            format!(
                "POST /v1/sessions/{SESSION_REF}/item-deliveries HTTP/1.1\r\nTransfer-Encoding: chunked\r\nIdempotency-Key: delivery_bad\r\nContent-Type: application/json\r\n\r\n0\r\n\r\n"
            )
            .as_bytes(),
        )
        .unwrap();
    stream.shutdown(Shutdown::Write).unwrap();
    server.join().unwrap();
}
