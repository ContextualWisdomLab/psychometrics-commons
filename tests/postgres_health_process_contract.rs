//! Process entrypoint contract against a reachable `PostgreSQL` operational store.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::health_http::{HEALTH_LIVE_PATH, HEALTH_READY_PATH};
use psychometrics_commons_runtime::health_process::{
    bind_health_process, parse_health_process_config, serve_health_process,
    HEALTH_BACKLOG_HEALTH_ENV, HEALTH_DATABASE_URL_ENV, HEALTH_LISTEN_ADDR_ENV,
    HEALTH_REQUIRED_RELATIONS_ENV,
};
use psychometrics_commons_runtime::postgres_health::POSTGRES_OPERATIONAL_STORE_CAPABILITY_REF;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

fn test_database_url() -> String {
    std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database")
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
fn process_with_reachable_store_answers_ready_without_exposing_the_url() {
    let url = test_database_url();
    Client::connect(&url, NoTls).expect("isolated CI PostgreSQL database must be reachable");
    let owned_url = url.clone();
    let config = parse_health_process_config(move |key| match key {
        HEALTH_LISTEN_ADDR_ENV => Some("127.0.0.1:0".to_owned()),
        HEALTH_DATABASE_URL_ENV => Some(owned_url.clone()),
        HEALTH_BACKLOG_HEALTH_ENV => Some("within_bounds".to_owned()),
        HEALTH_REQUIRED_RELATIONS_ENV => Some("pg_catalog.pg_class".to_owned()),
        _ => None,
    })
    .unwrap();
    let listener = bind_health_process(&config).unwrap();
    let addr = listener.local_addr().unwrap();
    let stop = listener.try_clone().unwrap();
    let (done_tx, done_rx) = mpsc::channel();
    let server = thread::spawn(move || {
        let result = serve_health_process(&listener, &config);
        let _ = done_tx.send(result);
    });

    let live = exchange(
        addr,
        &format!("GET {HEALTH_LIVE_PATH} HTTP/1.1\r\nHost: localhost\r\n\r\n"),
    );
    assert!(live.starts_with("HTTP/1.1 200 OK\r\n"), "{live}");
    assert!(!live.contains(&url));

    let ready = exchange(
        addr,
        &format!(
            "GET {HEALTH_READY_PATH}?capability={POSTGRES_OPERATIONAL_STORE_CAPABILITY_REF} HTTP/1.1\r\nHost: localhost\r\n\r\n"
        ),
    );
    assert!(
        ready.starts_with("HTTP/1.1 200 OK\r\n"),
        "a reachable store with caller-measured backlog must be ready: {ready}"
    );
    assert!(ready.contains(POSTGRES_OPERATIONAL_STORE_CAPABILITY_REF));
    assert!(!ready.contains(&url));
    assert!(!ready.contains("postgres::"));

    stop.set_nonblocking(true).unwrap();
    let _ = TcpStream::connect_timeout(&addr, Duration::from_millis(200));
    let _ = done_rx.recv_timeout(Duration::from_secs(3));
    server.join().unwrap();
}

#[test]
fn reachable_store_without_declared_relations_stays_unready() {
    let url = test_database_url();
    Client::connect(&url, NoTls).expect("isolated CI PostgreSQL database must be reachable");
    let owned_url = url.clone();
    let config = parse_health_process_config(move |key| match key {
        HEALTH_LISTEN_ADDR_ENV => Some("127.0.0.1:0".to_owned()),
        HEALTH_DATABASE_URL_ENV => Some(owned_url.clone()),
        HEALTH_BACKLOG_HEALTH_ENV => Some("within_bounds".to_owned()),
        _ => None,
    })
    .unwrap();
    let listener = bind_health_process(&config).unwrap();
    let addr = listener.local_addr().unwrap();
    let stop = listener.try_clone().unwrap();
    let (done_tx, done_rx) = mpsc::channel();
    let server = thread::spawn(move || {
        let result = serve_health_process(&listener, &config);
        let _ = done_tx.send(result);
    });

    let ready = exchange(
        addr,
        &format!(
            "GET {HEALTH_READY_PATH}?capability={POSTGRES_OPERATIONAL_STORE_CAPABILITY_REF} HTTP/1.1\r\nHost: localhost\r\n\r\n"
        ),
    );
    assert!(
        ready.starts_with("HTTP/1.1 503 Service Unavailable\r\n"),
        "missing declared relation evidence must fail closed: {ready}"
    );
    assert!(ready.contains("\"data_integrity_health\":\"unknown\""));
    assert!(ready.contains("\"ready\":false"));
    assert!(!ready.contains(&url));

    stop.set_nonblocking(true).unwrap();
    let _ = TcpStream::connect_timeout(&addr, Duration::from_millis(200));
    let _ = done_rx.recv_timeout(Duration::from_secs(3));
    server.join().unwrap();
}
