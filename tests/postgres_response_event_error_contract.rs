//! Stable operator-facing error contracts for response-event persistence.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::postgres_response_event::ResponseEventPersistenceError;

fn test_client() -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    Client::connect(&connection, NoTls).expect("isolated CI PostgreSQL database must be reachable")
}

#[test]
fn persistence_errors_expose_stable_messages_and_database_sources() {
    for (error, expected_message) in [
        (
            ResponseEventPersistenceError::InvalidReference,
            "response event persistence references must be opaque durable values",
        ),
        (
            ResponseEventPersistenceError::ConflictingReplay,
            "response event identity was replayed with conflicting evidence",
        ),
        (
            ResponseEventPersistenceError::SequenceConflict,
            "response event sequence was reused by a different event identity",
        ),
        (
            ResponseEventPersistenceError::InvalidSequence,
            "response event sequence is missing, gapped, or outside the PostgreSQL bigint range",
        ),
        (
            ResponseEventPersistenceError::UnsupportedIsolationLevel,
            "response event persistence requires read committed isolation",
        ),
    ] {
        assert_eq!(error.to_string(), expected_message);
        assert!(std::error::Error::source(&error).is_none());
    }

    let mut client = test_client();
    let database_error = client
        .query_one(
            "SELECT * FROM response_event_error_contract_missing_relation",
            &[],
        )
        .unwrap_err();
    let error = ResponseEventPersistenceError::from(database_error);
    assert_eq!(
        error.to_string(),
        "PostgreSQL response-event persistence failed"
    );
    assert!(std::error::Error::source(&error).is_some());
}
