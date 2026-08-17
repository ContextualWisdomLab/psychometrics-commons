//! Stable operator-facing error contracts for result-snapshot persistence.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::postgres_result_snapshot::ResultSnapshotPersistenceError;

fn test_client() -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    Client::connect(&connection, NoTls).expect("isolated CI PostgreSQL database must be reachable")
}

#[test]
fn persistence_errors_expose_stable_messages_and_database_sources() {
    for (error, expected_message) in [
        (
            ResultSnapshotPersistenceError::InvalidReference,
            "result snapshot persistence references must be opaque values",
        ),
        (
            ResultSnapshotPersistenceError::ConflictingReplay,
            "result snapshot identity was replayed with conflicting evidence",
        ),
        (
            ResultSnapshotPersistenceError::InvalidTimestamp,
            "result snapshot timestamp exceeds the PostgreSQL bigint range",
        ),
        (
            ResultSnapshotPersistenceError::UnsupportedIsolationLevel,
            "result snapshot persistence requires read committed isolation",
        ),
        (
            ResultSnapshotPersistenceError::InconsistentEvidence,
            "durable result evidence cannot reconstruct the published snapshot",
        ),
    ] {
        assert_eq!(error.to_string(), expected_message);
        assert!(std::error::Error::source(&error).is_none());
    }

    let mut client = test_client();
    let database_error = client
        .query_one(
            "SELECT * FROM result_snapshot_error_contract_missing_relation",
            &[],
        )
        .unwrap_err();
    let error = ResultSnapshotPersistenceError::from(database_error);
    assert_eq!(
        error.to_string(),
        "PostgreSQL result-snapshot persistence failed"
    );
    assert!(std::error::Error::source(&error).is_some());
}
