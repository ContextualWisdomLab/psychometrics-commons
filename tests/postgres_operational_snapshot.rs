//! Compose PostgreSQL probes into one operation-scoped runtime health snapshot.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::health::{
    BacklogHealth, CapabilityState, DataIntegrityHealth,
};
use psychometrics_commons_runtime::postgres_health::{
    observe_postgres_operational_snapshot, POSTGRES_OPERATIONAL_STORE_CAPABILITY_REF,
};

fn test_client() -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    Client::connect(&connection, NoTls).expect("isolated CI PostgreSQL database must be reachable")
}

#[test]
fn writable_store_and_verified_relations_are_ready_for_the_postgres_capability() {
    let mut client = test_client();
    let snapshot = observe_postgres_operational_snapshot(
        &mut client,
        &["pg_catalog.pg_class"],
        BacklogHealth::WithinBounds,
    );

    assert!(snapshot.is_live());
    assert_eq!(snapshot.backlog_health(), BacklogHealth::WithinBounds);
    assert_eq!(
        snapshot.data_integrity_health(),
        DataIntegrityHealth::Verified
    );
    assert!(snapshot.is_ready_for(&[POSTGRES_OPERATIONAL_STORE_CAPABILITY_REF]));
    let capability = snapshot
        .capability(POSTGRES_OPERATIONAL_STORE_CAPABILITY_REF)
        .unwrap();
    assert_eq!(capability.state(), CapabilityState::Available);
    assert!(capability.accepts_new_work());
}

#[test]
fn missing_required_relation_fails_readiness_closed() {
    let mut client = test_client();
    let snapshot = observe_postgres_operational_snapshot(
        &mut client,
        &["psychometrics_commons_missing_relation"],
        BacklogHealth::WithinBounds,
    );

    assert!(snapshot.is_live());
    assert_eq!(
        snapshot.data_integrity_health(),
        DataIntegrityHealth::Incompatible
    );
    assert!(!snapshot.is_ready_for(&[POSTGRES_OPERATIONAL_STORE_CAPABILITY_REF]));
}

#[test]
fn probe_failure_maps_to_unknown_unready_evidence_without_exposing_the_driver() {
    let mut client = test_client();
    let _ = client.batch_execute("SELECT pg_terminate_backend(pg_backend_pid())");
    let snapshot =
        observe_postgres_operational_snapshot(&mut client, &["pg_catalog.pg_class"], BacklogHealth::WithinBounds);

    assert!(snapshot.is_live());
    assert_eq!(snapshot.data_integrity_health(), DataIntegrityHealth::Unknown);
    let capability = snapshot
        .capability(POSTGRES_OPERATIONAL_STORE_CAPABILITY_REF)
        .unwrap();
    assert_eq!(capability.state(), CapabilityState::Unknown);
    assert!(!capability.accepts_new_work());
    assert!(!snapshot.is_ready_for(&[POSTGRES_OPERATIONAL_STORE_CAPABILITY_REF]));
}

#[test]
fn caller_supplied_stalled_backlog_fails_readiness_even_when_the_store_is_writable() {
    let mut client = test_client();
    let snapshot = observe_postgres_operational_snapshot(
        &mut client,
        &["pg_catalog.pg_class"],
        BacklogHealth::Stalled,
    );

    assert_eq!(snapshot.backlog_health(), BacklogHealth::Stalled);
    assert!(!snapshot.is_ready_for(&[POSTGRES_OPERATIONAL_STORE_CAPABILITY_REF]));
}

#[test]
fn read_only_transaction_is_live_but_not_ready() {
    let mut client = test_client();
    let mut transaction = client.build_transaction().read_only(true).start().unwrap();
    let snapshot = observe_postgres_operational_snapshot(
        &mut transaction,
        &["pg_catalog.pg_class"],
        BacklogHealth::WithinBounds,
    );

    assert!(snapshot.is_live());
    let capability = snapshot
        .capability(POSTGRES_OPERATIONAL_STORE_CAPABILITY_REF)
        .unwrap();
    assert_eq!(capability.state(), CapabilityState::Unavailable);
    assert!(!snapshot.is_ready_for(&[POSTGRES_OPERATIONAL_STORE_CAPABILITY_REF]));
}
