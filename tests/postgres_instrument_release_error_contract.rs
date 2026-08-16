//! Stable operator-facing error contracts for instrument-release persistence.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::postgres_instrument_release::InstrumentReleasePersistenceError;

fn test_client() -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    Client::connect(&connection, NoTls).expect("isolated CI PostgreSQL database must be reachable")
}

#[test]
fn persistence_errors_expose_stable_messages_and_database_sources() {
    for (error, expected_message) in [
        (
            InstrumentReleasePersistenceError::InvalidReference,
            "instrument release persistence references must be opaque values",
        ),
        (
            InstrumentReleasePersistenceError::ConflictingReplay,
            "instrument release identity was replayed with conflicting evidence",
        ),
        (
            InstrumentReleasePersistenceError::InvalidTransition,
            "instrument release publication state cannot move to an unreachable lifecycle",
        ),
        (
            InstrumentReleasePersistenceError::InvalidTimestamp,
            "instrument release timestamp exceeds the PostgreSQL bigint range",
        ),
        (
            InstrumentReleasePersistenceError::UnsupportedIsolationLevel,
            "instrument release persistence requires read committed isolation",
        ),
        (
            InstrumentReleasePersistenceError::InconsistentEvidence,
            "durable instrument-release evidence cannot reconstruct the stored snapshot",
        ),
    ] {
        assert_eq!(error.to_string(), expected_message);
        assert!(std::error::Error::source(&error).is_none());
    }

    let mut client = test_client();
    let database_error = client
        .query_one(
            "SELECT * FROM instrument_release_error_contract_missing_relation",
            &[],
        )
        .unwrap_err();
    let error = InstrumentReleasePersistenceError::from(database_error);
    assert_eq!(
        error.to_string(),
        "PostgreSQL instrument-release persistence failed"
    );
    assert!(std::error::Error::source(&error).is_some());
}
