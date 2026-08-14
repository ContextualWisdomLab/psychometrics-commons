//! Real PostgreSQL contract for product-owned operational-store readiness.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::health::CapabilityState;
use psychometrics_commons_runtime::postgres_health::{
    classify_postgres_runtime, probe_postgres_runtime, PostgresRuntimeStatus,
    POSTGRES_OPERATIONAL_STORE_CAPABILITY_REF, SUPPORTED_POSTGRES_MAJOR,
};

fn test_client() -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    Client::connect(&connection, NoTls).expect("isolated CI PostgreSQL database must be reachable")
}

#[test]
fn supported_writable_postgres_is_ready_for_new_operational_work() {
    let health = classify_postgres_runtime(180_002, false);

    assert_eq!(SUPPORTED_POSTGRES_MAJOR, 18);
    assert_eq!(health.server_major_version(), 18);
    assert_eq!(health.status(), PostgresRuntimeStatus::Ready);
    assert_eq!(health.capability_state(), CapabilityState::Available);
    assert!(health.accepts_new_work());

    let capability = health.capability_health().unwrap();
    assert_eq!(capability.capability_ref(), POSTGRES_OPERATIONAL_STORE_CAPABILITY_REF);
    assert_eq!(capability.state(), CapabilityState::Available);
    assert!(capability.accepts_new_work());
}

#[test]
fn supported_read_only_postgres_fails_state_changing_readiness_closed() {
    let health = classify_postgres_runtime(180_000, true);

    assert_eq!(health.server_major_version(), 18);
    assert_eq!(health.status(), PostgresRuntimeStatus::ReadOnly);
    assert_eq!(health.capability_state(), CapabilityState::Unavailable);
    assert!(!health.accepts_new_work());

    let capability = health.capability_health().unwrap();
    assert_eq!(capability.state(), CapabilityState::Unavailable);
    assert!(!capability.accepts_new_work());
}

#[test]
fn unsupported_postgres_major_fails_readiness_closed_even_when_writable() {
    let health = classify_postgres_runtime(170_009, false);

    assert_eq!(health.server_major_version(), 17);
    assert_eq!(health.status(), PostgresRuntimeStatus::UnsupportedMajorVersion);
    assert_eq!(health.capability_state(), CapabilityState::Unavailable);
    assert!(!health.accepts_new_work());
}

#[test]
fn live_probe_accepts_the_repository_supported_postgres_major() {
    let mut client = test_client();
    let health = probe_postgres_runtime(&mut client).unwrap();

    assert_eq!(health.server_major_version(), SUPPORTED_POSTGRES_MAJOR);
    assert_eq!(health.status(), PostgresRuntimeStatus::Ready);
    assert_eq!(health.capability_state(), CapabilityState::Available);
    assert!(health.accepts_new_work());
}

#[test]
fn live_probe_detects_a_read_only_transaction() {
    let mut client = test_client();
    let mut transaction = client.build_transaction().read_only(true).start().unwrap();
    let health = probe_postgres_runtime(&mut transaction).unwrap();

    assert_eq!(health.server_major_version(), SUPPORTED_POSTGRES_MAJOR);
    assert_eq!(health.status(), PostgresRuntimeStatus::ReadOnly);
    assert_eq!(health.capability_state(), CapabilityState::Unavailable);
    assert!(!health.accepts_new_work());
}
