//! Bound TCP listener contract for public item-delivery record and reload.

use psychometrics_commons_runtime::instrument::InstrumentReleaseManifest;
use psychometrics_commons_runtime::item_delivery::ItemDeliveryLedger;
use psychometrics_commons_runtime::item_delivery_http::{
    accept_one_item_delivery_http, bind_item_delivery_http, ItemDeliveryHttpRuntime,
    ITEM_DELIVERY_COLLECTION_SUFFIX,
};
use psychometrics_commons_runtime::session::{AssessmentSession, SessionCommand};
use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, Shutdown, SocketAddr, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

const RELEASE_DIGEST: &str =
    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const SESSION_REF: &str = "ses_tcp_item_delivery";
const DELIVERY_REF: &str = "dlv_tcp_item_001";

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
        RELEASE_DIGEST,
    )
    .unwrap()
}

fn runtime() -> ItemDeliveryHttpRuntime {
    let manifest = manifest();
    let ledger = ItemDeliveryLedger::from_manifest(SESSION_REF, &manifest).unwrap();
    let mut session = AssessmentSession::from_currently_published_manifest(
        SESSION_REF,
        "participant_tcp_item_delivery",
        &manifest,
        "ko-KR",
        1,
    )
    .unwrap();
    session
        .apply_command(
            "cmd_activate_tcp_item_delivery",
            1,
            SessionCommand::Activate,
        )
        .unwrap();
    ItemDeliveryHttpRuntime::new(vec![(session, ledger)]).unwrap()
}

fn exchange(addr: SocketAddr, request: &str) -> String {
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(2)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    stream.write_all(request.as_bytes()).unwrap();
    stream.shutdown(Shutdown::Write).unwrap();
    let mut body = String::new();
    stream.read_to_string(&mut body).unwrap();
    body
}

fn exchange_in_chunks(addr: SocketAddr, headers: &str, body: &str) -> String {
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(2)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    stream.write_all(headers.as_bytes()).unwrap();
    stream.flush().unwrap();
    thread::sleep(Duration::from_millis(20));
    stream.write_all(body.as_bytes()).unwrap();
    stream.shutdown(Shutdown::Write).unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    response
}

fn rejected_request_kind(request: &[u8]) -> (std::io::ErrorKind, usize) {
    let listener =
        bind_item_delivery_http(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0)).unwrap();
    let addr = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let mut runtime = runtime();
        let error = accept_one_item_delivery_http(&listener, &mut runtime).unwrap_err();
        (error.kind(), runtime.event_count(SESSION_REF))
    });
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(2)).unwrap();
    stream.write_all(request).unwrap();
    stream.shutdown(Shutdown::Write).unwrap();
    drop(stream);
    server.join().unwrap()
}

#[test]
fn bound_listener_records_and_reloads_item_delivery_over_tcp() {
    let listener =
        bind_item_delivery_http(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0)).unwrap();
    let addr = listener.local_addr().unwrap();
    let runtime = Arc::new(Mutex::new(runtime()));
    let server_runtime = Arc::clone(&runtime);
    let server = thread::spawn(move || {
        let mut locked = server_runtime.lock().unwrap();
        accept_one_item_delivery_http(&listener, &mut locked).unwrap();
        accept_one_item_delivery_http(&listener, &mut locked).unwrap();
    });

    let body = format!(
        "{{\"delivery_ref\":\"{DELIVERY_REF}\",\"item_version_ref\":\"item_version_001\",\"presentation_context_ref\":\"presentation_web_self_report_v1\"}}"
    );
    let created = exchange_in_chunks(
        addr,
        &format!(
            "POST /v1/sessions/{SESSION_REF}{ITEM_DELIVERY_COLLECTION_SUFFIX} HTTP/1.1\r\nHost: localhost\r\nIdempotency-Key: {DELIVERY_REF}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
            body.len()
        ),
        &body,
    );
    assert!(created.starts_with("HTTP/1.1 201 Created\r\n"));
    assert!(created.contains("Content-Type: application/json\r\n"));
    assert!(created.contains("Connection: close\r\n"));
    assert!(created.contains(&format!("\"delivery_ref\":\"{DELIVERY_REF}\"")));
    assert!(created.contains("\"item_version_ref\":\"item_version_001\""));
    assert!(created.contains("\"sequence\":1"));

    let loaded = exchange(
        addr,
        &format!(
            "GET /v1/sessions/{SESSION_REF}{ITEM_DELIVERY_COLLECTION_SUFFIX} HTTP/1.1\r\nHost: localhost\r\n\r\n"
        ),
    );
    assert!(loaded.starts_with("HTTP/1.1 200 OK\r\n"));
    assert!(loaded.contains("\"item_version_001\""));
    assert!(loaded.contains("\"sequence\":1"));

    server.join().unwrap();
}

#[test]
fn listener_rejects_transfer_encoding_duplicate_lengths_non_utf8_headers_and_trailing_bytes() {
    let transfer = format!(
        "POST /v1/sessions/{SESSION_REF}{ITEM_DELIVERY_COLLECTION_SUFFIX} HTTP/1.1\r\nHost: localhost\r\nTransfer-Encoding: chunked\r\n\r\n0\r\n\r\n"
    );
    assert_eq!(
        rejected_request_kind(transfer.as_bytes()),
        (std::io::ErrorKind::InvalidData, 0)
    );

    let duplicate_length = format!(
        "POST /v1/sessions/{SESSION_REF}{ITEM_DELIVERY_COLLECTION_SUFFIX} HTTP/1.1\r\nHost: localhost\r\nContent-Length: 0\r\nContent-Length: 0\r\n\r\n"
    );
    assert_eq!(
        rejected_request_kind(duplicate_length.as_bytes()),
        (std::io::ErrorKind::InvalidData, 0)
    );

    let mut non_utf8 = format!(
        "GET /v1/sessions/{SESSION_REF}{ITEM_DELIVERY_COLLECTION_SUFFIX} HTTP/1.1\r\nX-Test: "
    )
    .into_bytes();
    non_utf8.push(0xff);
    non_utf8.extend_from_slice(b"\r\n\r\n");
    assert_eq!(
        rejected_request_kind(&non_utf8),
        (std::io::ErrorKind::InvalidData, 0)
    );

    let trailing = format!(
        "GET /v1/sessions/{SESSION_REF}{ITEM_DELIVERY_COLLECTION_SUFFIX} HTTP/1.1\r\nHost: localhost\r\n\r\nx"
    );
    assert_eq!(
        rejected_request_kind(trailing.as_bytes()),
        (std::io::ErrorKind::InvalidData, 0)
    );
}
