//! Process entrypoint contract for operator health probes.
//!
//! A buyer must be able to start one process from listen/store environment
//! variables, point a load balancer at GET `/live` and GET `/ready`, and keep
//! liveness free of store I/O when `PostgreSQL` is down.

use psychometrics_commons_runtime::health::BacklogHealth;
use psychometrics_commons_runtime::health_http::{HEALTH_LIVE_PATH, HEALTH_READY_PATH};
use psychometrics_commons_runtime::health_process::{
    bind_health_process, parse_health_process_config, run_health_process, serve_health_process,
    HealthProcessConfigError, HealthProcessRunError, HEALTH_BACKLOG_HEALTH_ENV,
    HEALTH_DATABASE_URL_ENV, HEALTH_LISTEN_ADDR_ENV, HEALTH_LISTEN_PORT_ENV,
};
use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

fn env_lookup(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
    let owned: Vec<(String, String)> = pairs
        .iter()
        .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
        .collect();
    move |key| {
        owned
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.clone())
    }
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
fn run_health_process_fails_closed_when_the_listen_address_cannot_bind() {
    let error = run_health_process(env_lookup(&[(HEALTH_LISTEN_ADDR_ENV, "8.8.8.8:80")]))
        .expect_err("binding a foreign address must not start a silent probe process");
    assert!(matches!(error, HealthProcessRunError::Listen(_)));
    assert!(error.to_string().contains("HEALTH_LISTEN_ADDR"));
}

#[test]
fn listen_config_fails_closed_when_no_address_or_port_is_set() {
    let error = parse_health_process_config(|_| None)
        .expect_err("a process without a listen target must not start");
    assert_eq!(error, HealthProcessConfigError::MissingListenAddress);
    assert!(error.to_string().contains("HEALTH_LISTEN_ADDR"));
    assert!(error.to_string().contains("PORT"));
}

#[test]
fn listen_address_rejects_blank_padded_and_unparseable_values() {
    for (env_key, value, expected) in [
        (
            HEALTH_LISTEN_ADDR_ENV,
            "",
            HealthProcessConfigError::InvalidListenAddress,
        ),
        (
            HEALTH_LISTEN_ADDR_ENV,
            " 127.0.0.1:8080",
            HealthProcessConfigError::InvalidListenAddress,
        ),
        (
            HEALTH_LISTEN_ADDR_ENV,
            "not-a-socket",
            HealthProcessConfigError::InvalidListenAddress,
        ),
        (
            HEALTH_LISTEN_PORT_ENV,
            "",
            HealthProcessConfigError::InvalidListenPort,
        ),
        (
            HEALTH_LISTEN_PORT_ENV,
            " 8080",
            HealthProcessConfigError::InvalidListenPort,
        ),
        (
            HEALTH_LISTEN_PORT_ENV,
            "65536",
            HealthProcessConfigError::InvalidListenPort,
        ),
        (
            HEALTH_LISTEN_PORT_ENV,
            "abc",
            HealthProcessConfigError::InvalidListenPort,
        ),
    ] {
        let error = parse_health_process_config(env_lookup(&[(env_key, value)]))
            .expect_err("invalid listen configuration must fail closed");
        assert_eq!(error, expected, "{env_key}={value:?}");
        assert!(!error.to_string().is_empty());
    }
}

#[test]
fn explicit_listen_address_wins_over_platform_port() {
    let config = parse_health_process_config(env_lookup(&[
        (HEALTH_LISTEN_ADDR_ENV, "127.0.0.1:0"),
        (HEALTH_LISTEN_PORT_ENV, "8080"),
    ]))
    .expect("an explicit listen address must start the process");
    assert_eq!(
        config.listen_addr(),
        SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0)
    );
    assert!(config.database_url().is_none());
    assert_eq!(config.backlog_health(), BacklogHealth::Unknown);
}

#[test]
fn platform_port_binds_all_interfaces() {
    let config = parse_health_process_config(env_lookup(&[(HEALTH_LISTEN_PORT_ENV, "8080")]))
        .expect("PORT must be enough for a hosted process");
    assert_eq!(
        config.listen_addr(),
        SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 8080)
    );
    let ephemeral = parse_health_process_config(env_lookup(&[(HEALTH_LISTEN_PORT_ENV, "0")]))
        .expect("PORT=0 must remain a valid ephemeral bind");
    assert_eq!(
        ephemeral.listen_addr(),
        SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0)
    );
}

#[test]
fn database_url_and_backlog_fail_closed_on_unknown_semantics() {
    for (pairs, expected) in [
        (
            &[
                (HEALTH_LISTEN_ADDR_ENV, "127.0.0.1:0"),
                (HEALTH_DATABASE_URL_ENV, ""),
            ] as &[(&str, &str)],
            HealthProcessConfigError::InvalidDatabaseUrl,
        ),
        (
            &[
                (HEALTH_LISTEN_ADDR_ENV, "127.0.0.1:0"),
                (HEALTH_DATABASE_URL_ENV, " postgres://localhost/db"),
            ],
            HealthProcessConfigError::InvalidDatabaseUrl,
        ),
        (
            &[
                (HEALTH_LISTEN_ADDR_ENV, "127.0.0.1:0"),
                (HEALTH_DATABASE_URL_ENV, "https://example.test/db"),
            ],
            HealthProcessConfigError::InvalidDatabaseUrl,
        ),
        (
            &[
                (HEALTH_LISTEN_ADDR_ENV, "127.0.0.1:0"),
                (HEALTH_DATABASE_URL_ENV, "postgres://localhost:65536/db"),
            ],
            HealthProcessConfigError::InvalidDatabaseUrl,
        ),
        (
            &[
                (HEALTH_LISTEN_ADDR_ENV, "127.0.0.1:0"),
                (HEALTH_BACKLOG_HEALTH_ENV, "green"),
            ],
            HealthProcessConfigError::InvalidBacklogHealth,
        ),
        (
            &[
                (HEALTH_LISTEN_ADDR_ENV, "127.0.0.1:0"),
                (HEALTH_BACKLOG_HEALTH_ENV, " within_bounds"),
            ],
            HealthProcessConfigError::InvalidBacklogHealth,
        ),
    ] {
        let owned = pairs.to_vec();
        let error = parse_health_process_config(move |key| {
            owned
                .iter()
                .find(|(name, _)| *name == key)
                .map(|(_, value)| (*value).to_string())
        })
        .expect_err("unknown store or backlog semantics must not start the process");
        assert_eq!(error, expected);
        assert!(!error.to_string().is_empty());
    }
}

#[test]
fn postgres_urls_and_named_backlog_values_are_accepted() {
    let postgres = parse_health_process_config(env_lookup(&[
        (HEALTH_LISTEN_ADDR_ENV, "127.0.0.1:0"),
        (
            HEALTH_DATABASE_URL_ENV,
            "postgres://operator:secret@db/product",
        ),
        (HEALTH_BACKLOG_HEALTH_ENV, "within_bounds"),
    ]))
    .unwrap();
    assert_eq!(
        postgres.database_url(),
        Some("postgres://operator:secret@db/product")
    );
    assert_eq!(postgres.backlog_health(), BacklogHealth::WithinBounds);

    let postgresql = parse_health_process_config(env_lookup(&[
        (HEALTH_LISTEN_ADDR_ENV, "[::1]:9090"),
        (HEALTH_DATABASE_URL_ENV, "postgresql://db/product"),
        (HEALTH_BACKLOG_HEALTH_ENV, "stalled"),
    ]))
    .unwrap();
    assert_eq!(postgresql.database_url(), Some("postgresql://db/product"));
    assert_eq!(postgresql.backlog_health(), BacklogHealth::Stalled);
    assert_eq!(
        postgresql.listen_addr(),
        "[::1]:9090".parse::<SocketAddr>().unwrap()
    );

    let unknown = parse_health_process_config(env_lookup(&[
        (HEALTH_LISTEN_ADDR_ENV, "127.0.0.1:0"),
        (HEALTH_BACKLOG_HEALTH_ENV, "unknown"),
    ]))
    .unwrap();
    assert_eq!(unknown.backlog_health(), BacklogHealth::Unknown);
}

#[test]
fn process_without_database_answers_live_and_fails_ready_closed() {
    let config =
        parse_health_process_config(env_lookup(&[(HEALTH_LISTEN_ADDR_ENV, "127.0.0.1:0")]))
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
    assert!(live.contains("\"live\":true"));
    assert!(!live.contains("postgres"));
    assert!(!live.contains("DATABASE_URL"));

    let ready = exchange(
        addr,
        &format!("GET {HEALTH_READY_PATH} HTTP/1.1\r\nHost: localhost\r\n\r\n"),
    );
    assert!(
        ready.starts_with("HTTP/1.1 503 Service Unavailable\r\n"),
        "a process without store evidence must not advertise readiness: {ready}"
    );
    assert!(ready.contains("\"ready\":false"));
    assert!(!ready.contains("postgres::"));
    assert!(!ready.contains("DbError"));

    stop.set_nonblocking(true)
        .expect("the test must be able to stop the process listener");
    let _ = TcpStream::connect_timeout(&addr, Duration::from_millis(200));
    let stopped = done_rx
        .recv_timeout(Duration::from_secs(3))
        .expect("serve_health_process must return after accept can no longer block");
    assert!(stopped.is_err(), "{stopped:?}");
    server.join().unwrap();
}

#[test]
fn unreachable_database_keeps_liveness_and_hides_driver_errors() {
    let config = parse_health_process_config(env_lookup(&[
        (HEALTH_LISTEN_ADDR_ENV, "127.0.0.1:0"),
        (HEALTH_DATABASE_URL_ENV, "postgres://127.0.0.1:1/missing"),
        (HEALTH_BACKLOG_HEALTH_ENV, "within_bounds"),
    ]))
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
    assert!(
        live.starts_with("HTTP/1.1 200 OK\r\n"),
        "a down store must not restart a live process: {live}"
    );
    assert!(!live.contains("127.0.0.1"));
    assert!(!live.contains("postgres::"));

    let ready = exchange(
        addr,
        &format!("GET {HEALTH_READY_PATH} HTTP/1.1\r\nHost: localhost\r\n\r\n"),
    );
    assert!(
        ready.starts_with("HTTP/1.1 503 Service Unavailable\r\n"),
        "{ready}"
    );
    assert!(!ready.contains("127.0.0.1:1"));
    assert!(!ready.contains("DbError"));
    assert!(!ready.contains("Connection refused"));

    let named = exchange(
        addr,
        &format!("GET {HEALTH_READY_PATH}?capability=scoring HTTP/1.1\r\nHost: localhost\r\n\r\n"),
    );
    assert!(
        named.starts_with("HTTP/1.1 503 Service Unavailable\r\n"),
        "{named}"
    );
    assert!(!named.contains("Connection refused"));

    stop.set_nonblocking(true).unwrap();
    let _ = TcpStream::connect_timeout(&addr, Duration::from_millis(200));
    let _ = done_rx.recv_timeout(Duration::from_secs(3));
    server.join().unwrap();
}
