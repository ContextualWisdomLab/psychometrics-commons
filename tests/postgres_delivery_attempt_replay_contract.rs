//! Focused replay and operator-error contract for durable outbox delivery attempts.

use postgres::{error::SqlState, Client, NoTls};
use psychometrics_commons_runtime::integration::{DeliveryOutcome, IntegrationEvent, OutboxState};
use psychometrics_commons_runtime::postgres_integration::{
    apply_integration_migration, enqueue_outbox_event, record_outbox_delivery_attempt,
    OutboxPersistenceIdentity, PersistenceDisposition, PersistenceError,
};

const DIGEST: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const DATABASE_TEST_LOCK_KEY: i64 = 0x5053_5943_484F_4D4D;

fn test_client() -> Client {
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
    let mut client = test_client();
    acquire_database_lock(&mut client, DATABASE_TEST_LOCK_KEY, "60s")
        .expect("shared PostgreSQL integration-test advisory lock should be acquired within 60 seconds");
    client
}

fn event() -> IntegrationEvent {
    IntegrationEvent::new(
        "event_quarantined_replay",
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
    .unwrap()
}

fn identity() -> OutboxPersistenceIdentity<'static> {
    OutboxPersistenceIdentity::new(
        "psychometrics_commons",
        "tenant_alpha",
        "event_quarantined_replay",
    )
}

#[test]
fn fixture_lock_wait_has_finite_postgresql_budget() {
    let mut guard = database_test_guard();
    let timeout_ms: i64 = guard
        .query_one(
            "SELECT setting::bigint FROM pg_settings WHERE name = 'lock_timeout'",
            &[],
        )
        .expect("fixture lock wait budget should be queryable")
        .get(0);

    assert_eq!(
        timeout_ms, 60_000,
        "delivery-attempt fixture lock acquisition must not wait indefinitely"
    );
}

#[test]
fn fixture_lock_wait_aborts_under_real_contention() {
    let mut holder = test_client();
    let behavior_lock_key: i64 = holder
        .query_one("SELECT pg_backend_pid()::bigint", &[])
        .expect("holder backend identity should be queryable")
        .get(0);
    holder
        .query_one("SELECT pg_advisory_lock($1)", &[&behavior_lock_key])
        .expect("behavior-test holder should acquire its private advisory lock");

    let mut contender = test_client();
    let error = acquire_database_lock(&mut contender, behavior_lock_key, "100ms")
        .expect_err("contended fixture lock acquisition must stop at the configured timeout");
    assert_eq!(error.code(), Some(&SqlState::LOCK_NOT_AVAILABLE));

    let released: bool = holder
        .query_one("SELECT pg_advisory_unlock($1)", &[&behavior_lock_key])
        .expect("behavior-test holder should release its advisory lock")
        .get(0);
    assert!(released, "behavior-test advisory lock should be released");
}

#[test]
fn exact_replay_after_quarantine_remains_idempotent() {
    let _database_guard = database_test_guard();
    let mut client = test_client();
    client
        .batch_execute(
            "DROP TABLE IF EXISTS integration_inbox;\
             DROP TABLE IF EXISTS integration_delivery_attempt;\
             DROP TABLE IF EXISTS integration_outbox;",
        )
        .unwrap();
    apply_integration_migration(&mut client).unwrap();
    enqueue_outbox_event(&mut client, &event(), 1).unwrap();

    let mut first = client.transaction().unwrap();
    let inserted = record_outbox_delivery_attempt(
        &mut first,
        identity(),
        "attempt_retry",
        DeliveryOutcome::RetryableFailure,
        10_001,
        Some("provider_unavailable"),
    )
    .unwrap();
    assert_eq!(inserted.disposition(), PersistenceDisposition::Inserted);
    assert_eq!(inserted.outbox_state(), OutboxState::Quarantined);
    first.commit().unwrap();

    let mut replay = client.transaction().unwrap();
    let duplicate = record_outbox_delivery_attempt(
        &mut replay,
        identity(),
        "attempt_retry",
        DeliveryOutcome::RetryableFailure,
        10_001,
        Some("provider_unavailable"),
    )
    .unwrap();
    assert_eq!(duplicate.disposition(), PersistenceDisposition::Duplicate);
    assert_eq!(duplicate.outbox_state(), OutboxState::Quarantined);
    replay.rollback().unwrap();
}

#[test]
fn delivery_attempt_error_messages_are_stable_for_operator_classification() {
    let expectations = [
        (
            PersistenceError::OutboxNotFound,
            "delivery attempt references an unknown outbox entry",
        ),
        (
            PersistenceError::NonMonotonicTimestamp,
            "delivery attempt timestamp precedes the latest outbox evidence",
        ),
        (
            PersistenceError::TerminalOutboxState,
            "terminal outbox state rejects new delivery attempts",
        ),
        (
            PersistenceError::InvalidStoredState,
            "stored outbox state violates the persistence contract",
        ),
    ];
    for (error, message) in expectations {
        assert_eq!(error.to_string(), message);
        assert!(std::error::Error::source(&error).is_none());
    }
}
