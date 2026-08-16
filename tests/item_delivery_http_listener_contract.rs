//! Bound TCP listener contract for public item-delivery record and reload.

use psychometrics_commons_runtime::instrument::InstrumentReleaseManifest;
use psychometrics_commons_runtime::item_delivery::ItemDeliveryLedger;
use psychometrics_commons_runtime::item_delivery_http::{
    accept_one_item_delivery_http, bind_item_delivery_http, ItemDeliveryHttpRuntime,
    ITEM_DELIVERY_COLLECTION_SUFFIX,
};
use psychometrics_commons_runtime::session::SessionState;
use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

const RELEASE_DIGEST: &str =
    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const SESSION_REF: &str = "ses_tcp_item_delivery";
const DELIVERY_REF: &str = "dlv_tcp_item_001";

fn ledger() -> ItemDeliveryLedger {
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
    ItemDeliveryLedger::from_manifest(SESSION_REF, &manifest).unwrap()
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
fn bound_listener_records_and_reloads_item_delivery_over_tcp() {
    let listener =
        bind_item_delivery_http(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0)).unwrap();
    let addr = listener.local_addr().unwrap();
    let runtime = Arc::new(Mutex::new(
        ItemDeliveryHttpRuntime::new(vec![(SessionState::Active, ledger())]).unwrap(),
    ));
    let server_runtime = Arc::clone(&runtime);
    let server = thread::spawn(move || {
        let mut locked = server_runtime.lock().unwrap();
        accept_one_item_delivery_http(&listener, &mut locked).unwrap();
        accept_one_item_delivery_http(&listener, &mut locked).unwrap();
    });

    let body = format!(
        "{{\"delivery_ref\":\"{DELIVERY_REF}\",\"item_version_ref\":\"item_version_001\",\"presentation_context_ref\":\"presentation_web_self_report_v1\"}}"
    );
    let created = exchange(
        addr,
        &format!(
            "POST /v1/sessions/{SESSION_REF}{ITEM_DELIVERY_COLLECTION_SUFFIX} HTTP/1.1\r\nHost: localhost\r\nIdempotency-Key: {DELIVERY_REF}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        ),
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
fn bound_listener_reads_content_length_after_split_headers_and_body() {
    let listener =
        bind_item_delivery_http(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0)).unwrap();
    let addr = listener.local_addr().unwrap();
    let runtime = Arc::new(Mutex::new(
        ItemDeliveryHttpRuntime::new(vec![(SessionState::Active, ledger())]).unwrap(),
    ));
    let server_runtime = Arc::clone(&runtime);
    let server = thread::spawn(move || {
        let mut locked = server_runtime.lock().unwrap();
        accept_one_item_delivery_http(&listener, &mut locked).unwrap();
    });

    let body = format!(
        "{{\"delivery_ref\":\"{DELIVERY_REF}\",\"item_version_ref\":\"item_version_001\",\"presentation_context_ref\":\"presentation_web_self_report_v1\"}}"
    );
    let headers = format!(
        "POST /v1/sessions/{SESSION_REF}{ITEM_DELIVERY_COLLECTION_SUFFIX} HTTP/1.1\r\nHost: localhost\r\nIdempotency-Key: {DELIVERY_REF}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
        body.len()
    );
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(2)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    stream.write_all(headers.as_bytes()).unwrap();
    stream.flush().unwrap();
    thread::sleep(Duration::from_millis(50));
    stream.write_all(body.as_bytes()).unwrap();
    stream.shutdown(std::net::Shutdown::Write).unwrap();
    let mut created = String::new();
    stream.read_to_string(&mut created).unwrap();

    assert!(
        created.starts_with("HTTP/1.1 201 Created\r\n"),
        "split POST must wait for Content-Length bytes, got {created}"
    );
    assert!(created.contains(&format!("\"delivery_ref\":\"{DELIVERY_REF}\"")));
    assert_eq!(runtime.lock().unwrap().event_count(SESSION_REF), 1);
    server.join().unwrap();
}
