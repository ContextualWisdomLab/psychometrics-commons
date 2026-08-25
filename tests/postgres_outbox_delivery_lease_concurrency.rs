//! Concurrency acceptance for exclusive outbox delivery claims and fencing tokens.
//!
//! This fixture holds the winning claim transaction open long enough to prove that the
//! competing worker is actually blocked by `PostgreSQL`. Only after the winner commits may the
//! loser classify the row as non-claimable. Recovery must then issue a strictly newer fence.

use postgres::{error::SqlState, Client, NoTls};
use psychometrics_commons_runtime::integration::IntegrationEvent;
use psychometrics_commons_runtime::postgres_integration::{
    apply_integration_migration, claim_outbox_delivery, enqueue_outbox_event,
    expire_outbox_delivery_lease, OutboxPersistenceIdentity, PersistenceDisposition,
    PersistenceError,
};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

const DIGEST: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const SCHEMA: &str = "outbox_delivery_lease_concurrency_test";
const DATABASE_TEST_LOCK_KEY: i64 = 0x4F55_5442_4F58_434E;

fn connect_client() -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    Client::connect(&connection, NoTls).expect("isolated CI PostgreSQL database must be reachable")
}

fn acquire_database_lock(
    client: &mut Client,
    lock_key: i64,
    lock_timeout: &str,
) -> Result<(), postgres::Error> {
    client.query_one(
        "SELECT set_config('lock_timeout', $1, false)",
        &[&lock_timeout],
    )?;
    client.query_one("SELECT pg_advisory_lock($1)", &[&lock_key])?;
    Ok(())
}

fn database_test_guard() -> Client {
    let mut client = connect_client();
    acquire_database_lock(&mut client, DATABASE_TEST_LOCK_KEY, "60s")
        .expect("shared PostgreSQL concurrency test advisory lock should be acquired within sixty seconds");
    client
}

fn event() -> IntegrationEvent {
    IntegrationEvent::new(
        "event_concurrent_claim",
        "assessment.session.completed",
        "v1",
        "psychometrics_commons",
        "tenant_alpha",
        "session_alpha",
        10_000,
        "correlation_alpha",
        None,
        DIGEST,
    )
    .expect("concurrency fixture should satisfy the integration-event contract")
}

fn identity() -> OutboxPersistenceIdentity<'static> {
    OutboxPersistenceIdentity::new(
        "psychometrics_commons",
        "tenant_alpha",
        "event_concurrent_claim",
    )
}

fn reset_concurrency_schema(client: &mut Client) {
    client
        .batch_execute(&format!(
            "DROP SCHEMA IF EXISTS {SCHEMA} CASCADE;
             CREATE SCHEMA {SCHEMA};
             SET search_path TO {SCHEMA};"
        ))
        .expect("isolated concurrency schema should be reset");
    apply_integration_migration(client)
        .expect("integration migration should install the outbox schema");
}

fn spawn_competing_claim() -> (thread::JoinHandle<bool>, mpsc::Receiver<i32>) {
    let (pid_sender, pid_receiver) = mpsc::channel();
    let second_worker = thread::spawn(move || {
        let mut second = connect_client();
        second
            .batch_execute(&format!("SET search_path TO {SCHEMA};"))
            .expect("second worker should select the isolated test schema");
        let backend_pid: i32 = second
            .query_one("SELECT pg_backend_pid()", &[])
            .expect("second worker backend PID should be observable")
            .get(0);
        let mut transaction = second
            .transaction()
            .expect("competing claim transaction should begin");
        pid_sender
            .send(backend_pid)
            .expect("competing backend PID should reach the observer");

        let result = claim_outbox_delivery(
            &mut transaction,
            identity(),
            "worker_concurrent_beta",
            "outbox_lease_concurrent_beta",
            10_000,
            11_000,
        );
        transaction
            .rollback()
            .expect("competing claim transaction should roll back cleanly");
        match result {
            Err(PersistenceError::NotLeaseable) => true,
            Ok(_) => false,
            Err(error) => panic!("competing claim failed unexpectedly: {error:?}"),
        }
    });
    (second_worker, pid_receiver)
}

fn wait_until_blocked(observer: &mut Client, backend_pid: i32) -> bool {
    for _ in 0..200 {
        let blocked: bool = observer
            .query_one(
                "SELECT cardinality(pg_blocking_pids($1)) > 0",
                &[&backend_pid],
            )
            .expect("observer should inspect the competing PostgreSQL backend")
            .get(0);
        if blocked {
            return true;
        }
        thread::sleep(Duration::from_millis(10));
    }
    false
}

fn expire_and_reclaim(client: &mut Client) {
    let mut expiry_transaction = client
        .transaction()
        .expect("expiry transaction should begin");
    expire_outbox_delivery_lease(&mut expiry_transaction, identity(), 11_000)
        .expect("the committed winning lease should expire at its boundary");
    expiry_transaction
        .commit()
        .expect("expiry recovery should commit");

    let mut reclaim_transaction = client
        .transaction()
        .expect("reclaim transaction should begin");
    let reclaimed = claim_outbox_delivery(
        &mut reclaim_transaction,
        identity(),
        "worker_concurrent_recovered",
        "outbox_lease_concurrent_recovered",
        11_001,
        12_000,
    )
    .expect("recovered pending event should be claimable");
    assert_eq!(reclaimed.fencing_token(), 2);
    reclaim_transaction
        .commit()
        .expect("recovered claim should commit");
}

#[test]
fn fixture_lock_wait_has_finite_postgresql_budget() {
    let mut guard = database_test_guard();
    let timeout_ms: i64 = guard
        .query_one(
            "SELECT setting::bigint FROM pg_settings WHERE name = 'lock_timeout'",
            &[],
        )
        .expect("outbox concurrency fixture lock timeout should be queryable from PostgreSQL")
        .get(0);

    assert_eq!(
        timeout_ms, 60_000,
        "outbox concurrency fixture must not wait indefinitely for its PostgreSQL advisory lock"
    );
}

#[test]
fn fixture_lock_wait_aborts_under_real_contention() {
    let mut holder = connect_client();
    let behavior_lock_key: i64 = holder
        .query_one("SELECT pg_backend_pid()::bigint", &[])
        .expect("holder backend identity should be queryable")
        .get(0);
    holder
        .query_one("SELECT pg_advisory_lock($1)", &[&behavior_lock_key])
        .expect("behavior-test holder should acquire its private advisory lock");

    let mut contender = connect_client();
    let error = acquire_database_lock(&mut contender, behavior_lock_key, "100ms")
        .expect_err("contended outbox concurrency fixture lock must stop at the configured timeout");
    assert_eq!(error.code(), Some(&SqlState::LOCK_NOT_AVAILABLE));

    let released: bool = holder
        .query_one("SELECT pg_advisory_unlock($1)", &[&behavior_lock_key])
        .expect("behavior-test advisory lock should be released")
        .get(0);
    assert!(released, "behavior-test advisory lock should be released");
}

#[test]
fn concurrent_claim_blocks_then_loses_and_reclaim_advances_the_fence() {
    let mut database_guard = database_test_guard();
    let mut primary = connect_client();
    reset_concurrency_schema(&mut primary);
    assert_eq!(
        enqueue_outbox_event(&mut primary, &event(), 3).expect("pending event should be enqueued"),
        PersistenceDisposition::Inserted,
    );

    let mut first_transaction = primary
        .transaction()
        .expect("winning claim transaction should begin");
    let first_lease = claim_outbox_delivery(
        &mut first_transaction,
        identity(),
        "worker_concurrent_alpha",
        "outbox_lease_concurrent_alpha",
        10_000,
        11_000,
    )
    .expect("first worker should claim the pending event");
    assert_eq!(first_lease.fencing_token(), 1);

    let (second_worker, pid_receiver) = spawn_competing_claim();
    let second_pid = pid_receiver
        .recv_timeout(Duration::from_secs(2))
        .expect("competing worker should reach the claim boundary");
    assert!(
        wait_until_blocked(&mut database_guard, second_pid),
        "the second worker must contend on the first uncommitted claim instead of bypassing exclusivity",
    );

    first_transaction
        .commit()
        .expect("winning delivery claim should commit");
    assert!(
        second_worker
            .join()
            .expect("competing claim worker should not panic"),
        "after the winner commits, the competing worker must observe NotLeaseable",
    );

    expire_and_reclaim(&mut primary);
    primary
        .batch_execute(&format!(
            "SET search_path TO public; DROP SCHEMA IF EXISTS {SCHEMA} CASCADE;"
        ))
        .expect("isolated concurrency schema should be removed");
}
