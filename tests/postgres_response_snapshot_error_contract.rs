//! Stable operator-facing error contracts for response-snapshot persistence.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::postgres_response_snapshot::ResponseSnapshotPersistenceError;

fn test_client() -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    Client::connect(&connection, NoTls).expect("isolated CI PostgreSQL database must be reachable")
}

#[test]
fn persistence_errors_expose_stable_messages_and_database_sources() {
    for (error, expected_message) in [
        (
            ResponseSnapshotPersistenceError::InvalidReference,
            "response snapshot persistence references must be exact safe opaque durable values",
        ),
        (
            ResponseSnapshotPersistenceError::ConflictingReplay,
            "response snapshot identity was replayed with conflicting evidence",
        ),
        (
            ResponseSnapshotPersistenceError::InvalidSequence,
            "response snapshot sequence exceeds the PostgreSQL bigint range",
        ),
        (
            ResponseSnapshotPersistenceError::UnsupportedIsolationLevel,
            "response snapshot persistence requires read committed isolation",
        ),
    ] {
        assert_eq!(error.to_string(), expected_message);
        assert!(std::error::Error::source(&error).is_none());
    }

    let mut client = test_client();
    let database_error = client
        .query_one(
            "SELECT * FROM response_snapshot_error_contract_missing_relation",
            &[],
        )
        .unwrap_err();
    let error = ResponseSnapshotPersistenceError::from(database_error);
    assert_eq!(
        error.to_string(),
        "PostgreSQL response-snapshot persistence failed"
    );
    assert!(std::error::Error::source(&error).is_some());
}
