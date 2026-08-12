//! Real `PostgreSQL` contract for durable outbox delivery attempts.

use postgres::{Client, IsolationLevel, NoTls};
use psychometrics_commons_runtime::integration::{DeliveryOutcome, IntegrationEvent, OutboxState};
use psychometrics_commons_runtime::postgres_integration::{
    apply_integration_migration, enqueue_outbox_event, record_outbox_delivery_attempt,
    PersistenceDisposition, PersistenceError,
};

const DIGEST: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn test_client() -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    Client::connect(&connection, NoTls).expect("isolated CI PostgreSQL database must be reachable")
}

fn reset_integration_tables(client: &mut Client) {
    client
        .batch_execute(
            "DROP TABLE IF EXISTS integration_inbox;\
             DROP TABLE IF EXISTS integration_delivery_attempt;\
             DROP TABLE IF EXISTS integration_outbox;",
        )
        .unwrap();
    apply_integration_migration(client).unwrap();
}

fn event(event_ref: &str) -> IntegrationEvent {
    IntegrationEvent::new(
        event_ref,
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

fn enqueue(client: &mut Client, event_ref: &str, max_attempts: usize) {
    assert_eq!(
        enqueue_outbox_event(client, &event(event_ref), max_attempts).unwrap(),
        PersistenceDisposition::Inserted
    );
}

fn persisted_state(client: &mut Client, event_ref: &str) -> String {
    client
        .query_one(
            "SELECT current_state FROM integration_outbox \
             WHERE source_ref = 'psychometrics_commons' \
               AND tenant_ref = 'tenant_alpha' AND event_ref = $1",
            &[&event_ref],
        )
        .unwrap()
        .get(0)
}

#[test]
fn delivery_attempts_transition_outbox_and_replay_exact_evidence() {
    let mut client = test_client();
    reset_integration_tables(&mut client);
    enqueue(&mut client, "event_delivered", 3);

    let mut transaction = client.transaction().unwrap();
    let inserted = record_outbox_delivery_attempt(
        &mut transaction,
        "psychometrics_commons",
        "tenant_alpha",
        "event_delivered",
        "attempt_alpha",
        DeliveryOutcome::Delivered,
        10_001,
        None,
    )
    .unwrap();
    assert_eq!(inserted.disposition(), PersistenceDisposition::Inserted);
    assert_eq!(inserted.outbox_state(), OutboxState::Delivered);
    transaction.commit().unwrap();
    assert_eq!(persisted_state(&mut client, "event_delivered"), "delivered");

    let mut replay = client.transaction().unwrap();
    let duplicate = record_outbox_delivery_attempt(
        &mut replay,
        "psychometrics_commons",
        "tenant_alpha",
        "event_delivered",
        "attempt_alpha",
        DeliveryOutcome::Delivered,
        10_001,
        None,
    )
    .unwrap();
    assert_eq!(duplicate.disposition(), PersistenceDisposition::Duplicate);
    assert_eq!(duplicate.outbox_state(), OutboxState::Delivered);
    replay.rollback().unwrap();

    let mut conflicting = client.transaction().unwrap();
    assert!(matches!(
        record_outbox_delivery_attempt(
            &mut conflicting,
            "psychometrics_commons",
            "tenant_alpha",
            "event_delivered",
            "attempt_alpha",
            DeliveryOutcome::PermanentFailure,
            10_001,
            Some("provider_rejected"),
        ),
        Err(PersistenceError::ConflictingReplay)
    ));
    conflicting.rollback().unwrap();

    let mut terminal = client.transaction().unwrap();
    assert!(matches!(
        record_outbox_delivery_attempt(
            &mut terminal,
            "psychometrics_commons",
            "tenant_alpha",
            "event_delivered",
            "attempt_beta",
            DeliveryOutcome::Delivered,
            10_002,
            None,
        ),
        Err(PersistenceError::TerminalOutboxState)
    ));
    terminal.rollback().unwrap();
}

#[test]
fn retry_budget_and_permanent_failure_quarantine_durably() {
    let mut client = test_client();
    reset_integration_tables(&mut client);
    enqueue(&mut client, "event_retry", 2);
    enqueue(&mut client, "event_permanent", 3);

    let mut first = client.transaction().unwrap();
    let first_retry = record_outbox_delivery_attempt(
        &mut first,
        "psychometrics_commons",
        "tenant_alpha",
        "event_retry",
        "attempt_first",
        DeliveryOutcome::RetryableFailure,
        10_001,
        Some("provider_unavailable"),
    )
    .unwrap();
    assert_eq!(first_retry.outbox_state(), OutboxState::Pending);
    first.commit().unwrap();

    let mut second = client.transaction().unwrap();
    let exhausted = record_outbox_delivery_attempt(
        &mut second,
        "psychometrics_commons",
        "tenant_alpha",
        "event_retry",
        "attempt_second",
        DeliveryOutcome::RetryableFailure,
        10_002,
        Some("provider_unavailable"),
    )
    .unwrap();
    assert_eq!(exhausted.outbox_state(), OutboxState::Quarantined);
    second.commit().unwrap();
    assert_eq!(persisted_state(&mut client, "event_retry"), "quarantined");

    let mut permanent = client.transaction().unwrap();
    let permanent_failure = record_outbox_delivery_attempt(
        &mut permanent,
        "psychometrics_commons",
        "tenant_alpha",
        "event_permanent",
        "attempt_permanent",
        DeliveryOutcome::PermanentFailure,
        10_003,
        Some("schema_rejected"),
    )
    .unwrap();
    assert_eq!(permanent_failure.outbox_state(), OutboxState::Quarantined);
    permanent.commit().unwrap();
}

#[test]
fn delivery_attempt_validation_and_transaction_rollback_fail_closed() {
    let mut client = test_client();
    reset_integration_tables(&mut client);
    enqueue(&mut client, "event_validation", 3);

    let mut invalid_reference = client.transaction().unwrap();
    assert!(matches!(
        record_outbox_delivery_attempt(
            &mut invalid_reference,
            "psychometrics_commons",
            "tenant_alpha",
            "event_validation",
            "123",
            DeliveryOutcome::Delivered,
            10_001,
            None,
        ),
        Err(PersistenceError::InvalidReference)
    ));
    invalid_reference.rollback().unwrap();

    let mut invalid_cause = client.transaction().unwrap();
    assert!(matches!(
        record_outbox_delivery_attempt(
            &mut invalid_cause,
            "psychometrics_commons",
            "tenant_alpha",
            "event_validation",
            "attempt_invalid_cause",
            DeliveryOutcome::RetryableFailure,
            10_001,
            Some("123"),
        ),
        Err(PersistenceError::InvalidReference)
    ));
    invalid_cause.rollback().unwrap();

    let mut invalid_timestamp = client.transaction().unwrap();
    assert!(matches!(
        record_outbox_delivery_attempt(
            &mut invalid_timestamp,
            "psychometrics_commons",
            "tenant_alpha",
            "event_validation",
            "attempt_zero_time",
            DeliveryOutcome::Delivered,
            0,
            None,
        ),
        Err(PersistenceError::InvalidTimestamp)
    ));
    invalid_timestamp.rollback().unwrap();

    let mut backward = client.transaction().unwrap();
    assert!(matches!(
        record_outbox_delivery_attempt(
            &mut backward,
            "psychometrics_commons",
            "tenant_alpha",
            "event_validation",
            "attempt_backwards",
            DeliveryOutcome::Delivered,
            9_999,
            None,
        ),
        Err(PersistenceError::NonMonotonicTimestamp)
    ));
    backward.rollback().unwrap();

    let mut unknown = client.transaction().unwrap();
    assert!(matches!(
        record_outbox_delivery_attempt(
            &mut unknown,
            "psychometrics_commons",
            "tenant_alpha",
            "event_missing",
            "attempt_missing",
            DeliveryOutcome::Delivered,
            10_001,
            None,
        ),
        Err(PersistenceError::OutboxNotFound)
    ));
    unknown.rollback().unwrap();

    let mut rolled_back = client.transaction().unwrap();
    let recorded = record_outbox_delivery_attempt(
        &mut rolled_back,
        "psychometrics_commons",
        "tenant_alpha",
        "event_validation",
        "attempt_rollback",
        DeliveryOutcome::RetryableFailure,
        10_001,
        Some("temporary_failure"),
    )
    .unwrap();
    assert_eq!(recorded.outbox_state(), OutboxState::Pending);
    rolled_back.rollback().unwrap();

    let attempt_count: i64 = client
        .query_one(
            "SELECT count(*) FROM integration_delivery_attempt WHERE event_ref = 'event_validation'",
            &[],
        )
        .unwrap()
        .get(0);
    assert_eq!(attempt_count, 0);
    assert_eq!(persisted_state(&mut client, "event_validation"), "pending");
}

#[test]
fn delivery_attempt_requires_read_committed_and_surfaces_database_failure() {
    let mut client = test_client();
    reset_integration_tables(&mut client);
    enqueue(&mut client, "event_isolation", 3);

    let mut serializable = client
        .build_transaction()
        .isolation_level(IsolationLevel::Serializable)
        .start()
        .unwrap();
    assert!(matches!(
        record_outbox_delivery_attempt(
            &mut serializable,
            "psychometrics_commons",
            "tenant_alpha",
            "event_isolation",
            "attempt_serializable",
            DeliveryOutcome::Delivered,
            10_001,
            None,
        ),
        Err(PersistenceError::UnsupportedIsolationLevel)
    ));
    serializable.rollback().unwrap();

    client
        .batch_execute(
            "DROP TABLE integration_inbox;\
             DROP TABLE integration_delivery_attempt;\
             DROP TABLE integration_outbox;",
        )
        .unwrap();
    let mut transaction = client.transaction().unwrap();
    let database_error = record_outbox_delivery_attempt(
        &mut transaction,
        "psychometrics_commons",
        "tenant_alpha",
        "event_isolation",
        "attempt_database_error",
        DeliveryOutcome::Delivered,
        10_001,
        None,
    )
    .unwrap_err();
    assert!(matches!(database_error, PersistenceError::Database(_)));
    assert!(std::error::Error::source(&database_error).is_some());
    transaction.rollback().unwrap();
}
