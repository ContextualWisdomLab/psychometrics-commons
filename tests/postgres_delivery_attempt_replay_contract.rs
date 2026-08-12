//! Focused replay and operator-error contract for durable outbox delivery attempts.

use postgres::{Client, NoTls};
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

fn database_test_guard() -> Client {
    let mut client = test_client();
    client
        .query_one(
            "SELECT pg_advisory_lock($1)",
            &[&DATABASE_TEST_LOCK_KEY],
        )
        .expect("shared PostgreSQL integration-test advisory lock should be acquired");
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
