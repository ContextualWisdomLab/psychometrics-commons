//! Stable operator-facing error contracts for inbox-consumption persistence.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::integration::{InboxConsumption, IntegrationEvent};
use psychometrics_commons_runtime::postgres_inbox_consumption::{
    apply_inbox_consumption_migration, begin_inbox_consumption, expire_inbox_consumption,
    persist_inbox_consumption, InboxConsumptionPersistenceError,
};
use psychometrics_commons_runtime::postgres_integration::{
    accept_inbox_event, apply_integration_migration,
};

const DIGEST: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn test_client() -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    Client::connect(&connection, NoTls).expect("isolated CI PostgreSQL database must be reachable")
}

fn assert_database_error(error: &InboxConsumptionPersistenceError) {
    assert!(matches!(
        error,
        InboxConsumptionPersistenceError::Database(_)
    ));
    assert!(std::error::Error::source(error).is_some());
}

#[test]
fn persistence_errors_expose_stable_messages_and_database_sources() {
    for (error, expected_message) in [
        (
            InboxConsumptionPersistenceError::InvalidReference,
            "inbox consumption persistence references must be opaque values",
        ),
        (
            InboxConsumptionPersistenceError::InvalidTimestamp,
            "inbox consumption timestamps must be greater than zero",
        ),
        (
            InboxConsumptionPersistenceError::ValueOutOfRange,
            "inbox consumption value exceeds the supported PostgreSQL range",
        ),
        (
            InboxConsumptionPersistenceError::UnsupportedIsolationLevel,
            "inbox consumption persistence requires read committed isolation",
        ),
        (
            InboxConsumptionPersistenceError::ConflictingReplay,
            "inbox consumption identity was replayed with conflicting evidence",
        ),
        (
            InboxConsumptionPersistenceError::InboxNotFound,
            "inbox consumption references an unknown inbox receipt",
        ),
        (
            InboxConsumptionPersistenceError::ConsumptionNotFound,
            "inbox consumption row does not exist",
        ),
        (
            InboxConsumptionPersistenceError::TerminalConsumptionState,
            "terminal inbox consumption rejects a new processing transition",
        ),
        (
            InboxConsumptionPersistenceError::ConsumptionNotClaimable,
            "inbox consumption can be claimed only from the pending state",
        ),
        (
            InboxConsumptionPersistenceError::StaleConsumptionFence,
            "inbox consumption fencing token does not match the current claim",
        ),
        (
            InboxConsumptionPersistenceError::NonMonotonicTimestamp,
            "inbox consumption timestamp precedes the latest accepted evidence",
        ),
        (
            InboxConsumptionPersistenceError::InvalidStoredState,
            "stored inbox consumption state violates the persistence contract",
        ),
        (
            InboxConsumptionPersistenceError::UnsupportedInitialState,
            "inbox consumption persist accepts only a fresh pending domain state",
        ),
        (
            InboxConsumptionPersistenceError::InvalidConsumptionClaimWindow,
            "inbox consumption claim expiry must be later than claim time",
        ),
        (
            InboxConsumptionPersistenceError::ConsumptionClaimStillActive,
            "inbox consumption processing claim has not expired",
        ),
        (
            InboxConsumptionPersistenceError::ConsumptionNotProcessing,
            "inbox consumption claim expiry requires the processing state",
        ),
    ] {
        assert_eq!(error.to_string(), expected_message);
        assert!(std::error::Error::source(&error).is_none());
    }

    let mut client = test_client();
    let database_error = client
        .query_one(
            "SELECT * FROM inbox_consumption_error_contract_missing_relation",
            &[],
        )
        .unwrap_err();
    let error = InboxConsumptionPersistenceError::from(database_error);
    assert_eq!(
        error.to_string(),
        "PostgreSQL inbox-consumption persistence failed"
    );
    assert!(std::error::Error::source(&error).is_some());
}

#[test]
fn database_failure_paths_are_classified_in_fresh_transactions() {
    let mut client = test_client();
    client
        .batch_execute(
            "DROP SCHEMA IF EXISTS inbox_consumption_error_transaction_test CASCADE;\
             CREATE SCHEMA inbox_consumption_error_transaction_test;\
             SET search_path TO inbox_consumption_error_transaction_test;",
        )
        .unwrap();
    apply_integration_migration(&mut client).unwrap();
    apply_inbox_consumption_migration(&mut client).unwrap();

    let source_event = IntegrationEvent::new(
        "event_transaction_boundary",
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
    .unwrap();
    accept_inbox_event(&mut client, "consumer_alpha", &source_event, 20_000).unwrap();
    let consumption = InboxConsumption::pending(
        "consumer_alpha",
        "psychometrics_commons",
        "tenant_alpha",
        "event_transaction_boundary",
        "consumption_transaction_boundary",
        "side_effect_transaction_boundary",
        20_000,
    )
    .unwrap();

    client
        .batch_execute("DROP TABLE integration_consumption;")
        .unwrap();

    let mut persist_transaction = client.transaction().unwrap();
    let persist_error =
        persist_inbox_consumption(&mut persist_transaction, &consumption).unwrap_err();
    assert_database_error(&persist_error);
    persist_transaction.rollback().unwrap();

    let mut begin_transaction = client.transaction().unwrap();
    let begin_error = begin_inbox_consumption(
        &mut begin_transaction,
        &consumption,
        20_001,
        21_000,
    )
    .unwrap_err();
    assert_database_error(&begin_error);
    begin_transaction.rollback().unwrap();

    let mut expire_transaction = client.transaction().unwrap();
    let expire_error =
        expire_inbox_consumption(&mut expire_transaction, &consumption, 21_000).unwrap_err();
    assert_database_error(&expire_error);
    expire_transaction.rollback().unwrap();

    client
        .batch_execute(
            "RESET search_path;\
             DROP SCHEMA inbox_consumption_error_transaction_test CASCADE;",
        )
        .unwrap();
}
