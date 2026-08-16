//! Wire live `PostgreSQL` operational snapshots into operator HTTP probes.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::health::BacklogHealth;
use psychometrics_commons_runtime::health_http::bind_health_http;
use psychometrics_commons_runtime::health_http::{HEALTH_LIVE_PATH, HEALTH_READY_PATH};
use psychometrics_commons_runtime::postgres_health::POSTGRES_OPERATIONAL_STORE_CAPABILITY_REF;
use psychometrics_commons_runtime::postgres_health_http::{
    accept_one_postgres_health_http, handle_postgres_health_http_request,
};
use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream};
use std::thread;
use std::time::Duration;

fn test_client() -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    Client::connect(&connection, NoTls).expect("isolated CI PostgreSQL database must be reachable")
}

fn request(target: &str) -> String {
    format!("GET {target} HTTP/1.1\r\nHost: localhost\r\n\r\n")
}

#[test]
fn writable_store_answers_live_and_postgres_ready_probes() {
    let mut client = test_client();
    let live = handle_postgres_health_http_request(
        &request(HEALTH_LIVE_PATH),
        &mut client,
        &["pg_catalog.pg_class"],
        BacklogHealth::WithinBounds,
    );
    assert_eq!(live.status(), 200);
    assert!(live.body().contains("\"live\":true"));
    assert!(!live.body().contains("postgres::"));
    assert!(!live.body().contains("sql"));

    let ready = handle_postgres_health_http_request(
        &request(&format!(
            "{HEALTH_READY_PATH}?capability={POSTGRES_OPERATIONAL_STORE_CAPABILITY_REF}"
        )),
        &mut client,
        &["pg_catalog.pg_class"],
        BacklogHealth::WithinBounds,
    );
    assert_eq!(ready.status(), 200);
    assert!(ready.body().contains("\"ready\":true"));
    assert!(ready
        .body()
        .contains(POSTGRES_OPERATIONAL_STORE_CAPABILITY_REF));
}

#[test]
fn missing_required_relation_is_live_but_not_ready() {
    let mut client = test_client();
    let ready = handle_postgres_health_http_request(
        &request(&format!(
            "{HEALTH_READY_PATH}?capability={POSTGRES_OPERATIONAL_STORE_CAPABILITY_REF}"
        )),
        &mut client,
        &["psychometrics_commons_missing_relation"],
        BacklogHealth::WithinBounds,
    );
    assert_eq!(ready.status(), 503);
    assert!(ready.body().contains("\"live\":true"));
    assert!(ready.body().contains("\"ready\":false"));
    assert!(ready
        .body()
        .contains("\"data_integrity_health\":\"incompatible\""));
}

#[test]
fn probe_failure_fails_readiness_closed_without_driver_text() {
    let mut client = test_client();
    let _ = client.batch_execute("SELECT pg_terminate_backend(pg_backend_pid())");
    let ready = handle_postgres_health_http_request(
        &request(&format!(
            "{HEALTH_READY_PATH}?capability={POSTGRES_OPERATIONAL_STORE_CAPABILITY_REF}"
        )),
        &mut client,
        &["pg_catalog.pg_class"],
        BacklogHealth::WithinBounds,
    );
    assert_eq!(ready.status(), 503);
    assert!(ready.body().contains("\"ready\":false"));
    assert!(!ready.body().contains("terminate"));
    assert!(!ready.body().contains("postgres::"));
    assert!(!ready.body().contains("DbError"));
    assert!(!ready.body().contains("sql"));
}

#[test]
fn stalled_backlog_fails_readiness_even_when_the_store_is_writable() {
    let mut client = test_client();
    let ready = handle_postgres_health_http_request(
        &request(HEALTH_READY_PATH),
        &mut client,
        &["pg_catalog.pg_class"],
        BacklogHealth::Stalled,
    );
    assert_eq!(ready.status(), 503);
    assert!(ready.body().contains("\"backlog_health\":\"stalled\""));
}

#[test]
fn bound_listener_serves_a_postgres_ready_probe() {
    let listener = bind_health_http(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0)).unwrap();
    let addr = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let mut client = test_client();
        accept_one_postgres_health_http(
            &listener,
            &mut client,
            &["pg_catalog.pg_class"],
            BacklogHealth::WithinBounds,
        )
        .unwrap();
    });

    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(2)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    stream
        .write_all(
            format!(
                "GET {HEALTH_READY_PATH}?capability={POSTGRES_OPERATIONAL_STORE_CAPABILITY_REF} HTTP/1.1\r\nHost: localhost\r\n\r\n"
            )
            .as_bytes(),
        )
        .unwrap();
    stream.shutdown(std::net::Shutdown::Write).unwrap();
    let mut body = String::new();
    stream.read_to_string(&mut body).unwrap();
    assert!(body.starts_with("HTTP/1.1 200 OK\r\n"));
    assert!(body.contains("\"ready\":true"));
    server.join().unwrap();
}
