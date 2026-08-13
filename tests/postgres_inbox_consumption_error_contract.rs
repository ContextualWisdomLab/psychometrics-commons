//! Stable operator-facing error contracts for inbox-consumption persistence.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::postgres_inbox_consumption::InboxConsumptionPersistenceError;

fn test_client() -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    Client::connect(&connection, NoTls).expect("isolated CI PostgreSQL database must be reachable")
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
