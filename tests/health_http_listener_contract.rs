//! Bound TCP listener contract for operator health probes.
//!
//! The listener is a transport around the existing request translator. It does
//! not add public/admin product routes or invent availability SLOs.

use psychometrics_commons_runtime::health::{
    BacklogHealth, CapabilityHealth, CapabilityState, DataIntegrityHealth, RuntimeHealthSnapshot,
};
use psychometrics_commons_runtime::health_http::{
    accept_one_health_http, bind_health_http, serve_health_http, HEALTH_LIVE_PATH, HEALTH_READY_PATH,
};
use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

fn healthy_snapshot() -> RuntimeHealthSnapshot {
    RuntimeHealthSnapshot::new(
        true,
        BacklogHealth::WithinBounds,
        DataIntegrityHealth::Verified,
        vec![CapabilityHealth::new("scoring", CapabilityState::Available, true).unwrap()],
    )
    .unwrap()
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
fn bound_listener_serves_live_and_ready_probes_over_tcp() {
    let listener = bind_health_http(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0)).unwrap();
    let addr = listener.local_addr().unwrap();
    let snapshot = healthy_snapshot();
    let server = thread::spawn(move || {
        accept_one_health_http(&listener, &snapshot).unwrap();
        accept_one_health_http(&listener, &snapshot).unwrap();
        accept_one_health_http(&listener, &snapshot).unwrap();
    });

    let live = exchange(
        addr,
        &format!("GET {HEALTH_LIVE_PATH} HTTP/1.1\r\nHost: localhost\r\n\r\n"),
    );
    assert!(live.starts_with("HTTP/1.1 200 OK\r\n"));
    assert!(live.contains("Content-Type: application/json\r\n"));
    assert!(live.contains("Connection: close\r\n"));
    assert!(live.contains("\"live\":true"));

    let ready = exchange(
        addr,
        &format!("GET {HEALTH_READY_PATH}?capability=scoring HTTP/1.1\r\nHost: localhost\r\n\r\n"),
    );
    assert!(ready.starts_with("HTTP/1.1 200 OK\r\n"));
    assert!(ready.contains("\"ready\":true"));

    let unready = exchange(
        addr,
        &format!(
            "GET {HEALTH_READY_PATH}?capability=unregistered_capability HTTP/1.1\r\nHost: localhost\r\n\r\n"
        ),
    );
    assert!(unready.starts_with("HTTP/1.1 503 Service Unavailable\r\n"));
    assert!(unready.contains("Content-Type: application/json\r\n"));
    assert!(unready.contains("\"ready\":false"));

    server.join().unwrap();
}

#[test]
fn bound_listener_returns_problem_details_for_unsupported_methods() {
    let listener = bind_health_http(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0)).unwrap();
    let addr = listener.local_addr().unwrap();
    let snapshot = healthy_snapshot();
    let server = thread::spawn(move || {
        accept_one_health_http(&listener, &snapshot).unwrap();
    });

    let response = exchange(
        addr,
        &format!("POST {HEALTH_LIVE_PATH} HTTP/1.1\r\nHost: localhost\r\n\r\n"),
    );
    assert!(response.starts_with("HTTP/1.1 405 Method Not Allowed\r\n"));
    assert!(response.contains("Content-Type: application/problem+json\r\n"));
    assert!(response.contains("Allow: GET\r\n"));
    assert!(response.contains("Cache-Control: no-store\r\n"));
    assert!(response.contains("\"title\":\"Method Not Allowed\""));
    assert!(!response.contains("postgres"));

    server.join().unwrap();
}

#[test]
fn bound_listener_fails_closed_for_unknown_paths_and_truncated_requests() {
    let listener = bind_health_http(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0)).unwrap();
    let addr = listener.local_addr().unwrap();
    let snapshot = healthy_snapshot();
    let server = thread::spawn(move || {
        accept_one_health_http(&listener, &snapshot).unwrap();
        accept_one_health_http(&listener, &snapshot).unwrap();
    });

    let missing = exchange(addr, "GET /v1/sessions HTTP/1.1\r\nHost: localhost\r\n\r\n");
    assert!(missing.starts_with("HTTP/1.1 404 Not Found\r\n"));
    assert!(missing.contains("Content-Type: application/problem+json\r\n"));
    assert!(!missing.contains("/v1/instruments"));

    let truncated = exchange(addr, "GET /live");
    assert!(truncated.starts_with("HTTP/1.1 400 Bad Request\r\n"));
    assert!(truncated.contains("Content-Type: application/problem+json\r\n"));
    assert!(!truncated.contains("GET /live HTTP"));

    server.join().unwrap();
}

#[test]
fn bound_listener_rejects_an_oversized_request_without_echoing_it() {
    let listener = bind_health_http(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0)).unwrap();
    let addr = listener.local_addr().unwrap();
    let snapshot = healthy_snapshot();
    let server = thread::spawn(move || {
        accept_one_health_http(&listener, &snapshot).unwrap();
    });

    let oversized = format!(
        "GET /live HTTP/1.1\r\nHost: localhost\r\nX-Pad: {}\r\n\r\n",
        "A".repeat(9_000)
    );
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(2)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let _ = stream.write_all(oversized.as_bytes());
    let _ = stream.shutdown(std::net::Shutdown::Write);
    let mut response = String::new();
    match stream.read_to_string(&mut response) {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::ConnectionReset => {}
        Err(error) => panic!("unexpected oversized-request read error: {error}"),
    }
    assert!(
        response.starts_with("HTTP/1.1 400 Bad Request\r\n"),
        "{response}"
    );
    assert!(!response.contains(&"A".repeat(32)));

    server.join().unwrap();
}

#[test]
fn bound_listener_fails_closed_when_the_client_never_finishes_the_request() {
    let listener = bind_health_http(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0)).unwrap();
    let addr = listener.local_addr().unwrap();
    let snapshot = healthy_snapshot();
    let server = thread::spawn(move || {
        accept_one_health_http(&listener, &snapshot).unwrap();
    });

    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(2)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(4)))
        .unwrap();
    stream.write_all(b"GET /live HTTP/1.1\r\nHost: ").unwrap();
    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .expect("the probe listener must answer an incomplete request instead of hanging");
    assert!(
        response.starts_with("HTTP/1.1 400 Bad Request\r\n"),
        "{response}"
    );
    assert!(response.contains("Cache-Control: no-store\r\n"));
    assert!(!response.contains("GET /live HTTP"));

    server.join().unwrap();
}

#[test]
fn serve_loop_answers_successive_probes_until_accept_fails() {
    let listener = bind_health_http(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0)).unwrap();
    let addr = listener.local_addr().unwrap();
    let snapshot = healthy_snapshot();
    let stop = listener.try_clone().unwrap();
    let (done_tx, done_rx) = mpsc::channel();
    let server = thread::spawn(move || {
        let result = serve_health_http(&listener, &snapshot);
        let _ = done_tx.send(result);
    });

    let live = exchange(
        addr,
        &format!("GET {HEALTH_LIVE_PATH} HTTP/1.1\r\nHost: localhost\r\n\r\n"),
    );
    assert!(live.starts_with("HTTP/1.1 200 OK\r\n"));
    assert!(live.contains("\"live\":true"));

    let ready = exchange(
        addr,
        &format!("GET {HEALTH_READY_PATH}?capability=scoring HTTP/1.1\r\nHost: localhost\r\n\r\n"),
    );
    assert!(ready.starts_with("HTTP/1.1 200 OK\r\n"));
    assert!(ready.contains("\"ready\":true"));

    stop.set_nonblocking(true)
        .expect("the test must be able to stop the shared listener");
    let _ = TcpStream::connect_timeout(&addr, Duration::from_millis(200));
    let stopped = done_rx
        .recv_timeout(Duration::from_secs(3))
        .expect("serve_health_http must return after accept can no longer block");
    assert!(
        stopped.is_err(),
        "serve_health_http must surface the accept failure instead of hanging"
    );
    server.join().unwrap();
}
