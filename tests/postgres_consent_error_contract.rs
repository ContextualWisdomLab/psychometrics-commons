//! Stable operator-facing error contracts for `PostgreSQL` consent persistence.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::postgres_consent::ConsentPersistenceError;

fn test_client() -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    Client::connect(&connection, NoTls).expect("isolated CI PostgreSQL database must be reachable")
}

#[test]
fn persistence_errors_expose_stable_messages_and_database_sources() {
    for (error, expected_message) in [
        (
            ConsentPersistenceError::InvalidReference,
            "consent persistence references must be opaque values",
        ),
        (
            ConsentPersistenceError::ConflictingReplay,
            "consent event identity was replayed with conflicting evidence",
        ),
        (
            ConsentPersistenceError::InvalidTimestamp,
            "consent event timestamp exceeds the PostgreSQL bigint range",
        ),
        (
            ConsentPersistenceError::UnsupportedIsolationLevel,
            "consent persistence requires read committed isolation",
        ),
    ] {
        assert_eq!(error.to_string(), expected_message);
        assert!(std::error::Error::source(&error).is_none());
    }

    let mut client = test_client();
    let database_error = client
        .query_one("SELECT * FROM consent_error_contract_missing_relation", &[])
        .unwrap_err();
    let error = ConsentPersistenceError::from(database_error);
    assert_eq!(error.to_string(), "PostgreSQL consent persistence failed");
    assert!(std::error::Error::source(&error).is_some());
}
