//! Stable operator-facing error contracts for scoring-request persistence.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::postgres_scoring_request::ScoringRequestPersistenceError;

fn test_client() -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    Client::connect(&connection, NoTls).expect("isolated CI PostgreSQL database must be reachable")
}

#[test]
fn persistence_errors_expose_stable_messages_and_database_sources() {
    for (error, expected_message) in [
        (
            ScoringRequestPersistenceError::InvalidReference,
            "scoring request persistence references must be opaque values",
        ),
        (
            ScoringRequestPersistenceError::ConflictingReplay,
            "scoring request identity was replayed with conflicting evidence",
        ),
        (
            ScoringRequestPersistenceError::InvalidSchemaVersion,
            "scoring request schema version exceeds the PostgreSQL integer range",
        ),
        (
            ScoringRequestPersistenceError::UnsupportedIsolationLevel,
            "scoring request persistence requires read committed isolation",
        ),
        (
            ScoringRequestPersistenceError::CorruptHistory,
            "stored scoring request rows cannot reconstruct a valid version-pinned request",
        ),
        (
            ScoringRequestPersistenceError::UnsupportedStoredSchema,
            "stored scoring request schema version is not supported by this runtime",
        ),
    ] {
        assert_eq!(error.to_string(), expected_message);
        assert!(std::error::Error::source(&error).is_none());
    }

    let mut client = test_client();
    let database_error = client
        .query_one(
            "SELECT * FROM scoring_request_error_contract_missing_relation",
            &[],
        )
        .unwrap_err();
    let error = ScoringRequestPersistenceError::from(database_error);
    assert_eq!(
        error.to_string(),
        "PostgreSQL scoring-request persistence failed"
    );
    assert!(std::error::Error::source(&error).is_some());
}
