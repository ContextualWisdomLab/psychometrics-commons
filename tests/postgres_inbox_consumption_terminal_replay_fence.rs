//! Regression for terminal replay fencing evidence in durable inbox consumption.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::integration::{InboxConsumption, IntegrationEvent};
use psychometrics_commons_runtime::postgres_inbox_consumption::{
    apply_inbox_consumption_migration, begin_inbox_consumption, complete_inbox_consumption,
    persist_inbox_consumption, InboxConsumptionPersistenceError,
};
use psychometrics_commons_runtime::postgres_integration::{
    accept_inbox_event, apply_integration_migration,
};

const DIGEST: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn test_client() -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let mut client = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    client
        .batch_execute(
            "DROP SCHEMA IF EXISTS inbox_consumption_replay_fence_test CASCADE;\
             CREATE SCHEMA inbox_consumption_replay_fence_test;\
             SET search_path TO inbox_consumption_replay_fence_test;",
        )
        .unwrap();
    apply_integration_migration(&mut client).unwrap();
    apply_inbox_consumption_migration(&mut client).unwrap();
    client
}

fn source_event() -> IntegrationEvent {
    IntegrationEvent::new(
        "event_terminal_replay_fence",
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

#[test]
fn completed_replay_rejects_same_evidence_and_time_with_a_different_fence() {
    let mut client = test_client();
    accept_inbox_event(&mut client, "consumer_alpha", &source_event(), 20_000).unwrap();
    let consumption = InboxConsumption::pending(
        "consumer_alpha",
        "psychometrics_commons",
        "tenant_alpha",
        "event_terminal_replay_fence",
        "consumption_terminal_replay_fence",
        "side_effect_terminal_replay_fence",
        20_000,
    )
    .unwrap();

    {
        let mut transaction = client.transaction().unwrap();
        persist_inbox_consumption(&mut transaction, &consumption).unwrap();
        transaction.commit().unwrap();
    }

    let mut transaction = client.transaction().unwrap();
    assert_eq!(
        begin_inbox_consumption(&mut transaction, &consumption, 20_001, 21_000).unwrap(),
        1
    );
    complete_inbox_consumption(
        &mut transaction,
        &consumption,
        20_002,
        "completion_terminal_replay_fence",
        1,
    )
    .unwrap();

    assert!(matches!(
        complete_inbox_consumption(
            &mut transaction,
            &consumption,
            20_002,
            "completion_terminal_replay_fence",
            0,
        ),
        Err(InboxConsumptionPersistenceError::ConflictingReplay)
    ));
    transaction.rollback().unwrap();
}
