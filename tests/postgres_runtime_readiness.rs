//! Real `PostgreSQL` contract for product-owned operational-store readiness.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::health::{CapabilityState, DataIntegrityHealth};
use psychometrics_commons_runtime::postgres_health::{
    classify_postgres_runtime, probe_postgres_relation_integrity, probe_postgres_runtime,
    PostgresRuntimeStatus, POSTGRES_OPERATIONAL_STORE_CAPABILITY_REF, SUPPORTED_POSTGRES_MAJOR,
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
    assert_eq!(
        capability.capability_ref(),
        POSTGRES_OPERATIONAL_STORE_CAPABILITY_REF
    );
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
    assert_eq!(
        health.status(),
        PostgresRuntimeStatus::UnsupportedMajorVersion
    );
    assert_eq!(health.capability_state(), CapabilityState::Unavailable);
    assert!(!health.accepts_new_work());

    let capability = health.capability_health().unwrap();
    assert_eq!(capability.state(), CapabilityState::Unavailable);
    assert!(!capability.accepts_new_work());
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

#[test]
fn live_probe_surfaces_a_database_failure() {
    let mut client = test_client();
    let mut transaction = client.transaction().unwrap();
    assert!(transaction.batch_execute("SELECT 1 / 0").is_err());

    assert!(probe_postgres_runtime(&mut transaction).is_err());
}

#[test]
fn live_probe_surfaces_a_closed_connection_failure() {
    let mut client = test_client();
    let _ = client.batch_execute("SELECT pg_terminate_backend(pg_backend_pid())");

    assert!(probe_postgres_runtime(&mut client).is_err());
}

#[test]
fn relation_integrity_probe_verifies_all_required_relations() {
    let mut client = test_client();
    let integrity = probe_postgres_relation_integrity(
        &mut client,
        &["pg_catalog.pg_class", "pg_catalog.pg_namespace"],
    )
    .unwrap();

    assert_eq!(integrity, DataIntegrityHealth::Verified);
}

#[test]
fn relation_integrity_probe_fails_closed_when_a_required_relation_is_missing() {
    let mut client = test_client();
    let integrity = probe_postgres_relation_integrity(
        &mut client,
        &[
            "pg_catalog.pg_class",
            "psychometrics_commons_missing_relation",
        ],
    )
    .unwrap();

    assert_eq!(integrity, DataIntegrityHealth::Incompatible);
}

#[test]
fn empty_relation_requirement_is_vacuously_verified() {
    let mut client = test_client();
    let integrity = probe_postgres_relation_integrity(&mut client, &[]).unwrap();

    assert_eq!(integrity, DataIntegrityHealth::Verified);
}

#[test]
fn relation_integrity_probe_surfaces_a_database_failure() {
    let mut client = test_client();
    let mut transaction = client.transaction().unwrap();
    assert!(transaction.batch_execute("SELECT 1 / 0").is_err());

    assert!(probe_postgres_relation_integrity(&mut transaction, &["pg_catalog.pg_class"]).is_err());
}
