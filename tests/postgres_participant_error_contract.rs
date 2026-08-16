//! Stable operator-facing error contracts for assessment-participant persistence.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::postgres_participant::ParticipantPersistenceError;

fn test_client() -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    Client::connect(&connection, NoTls).expect("isolated CI PostgreSQL database must be reachable")
}

#[test]
fn persistence_errors_expose_stable_messages_and_database_sources() {
    for (error, expected_message) in [
        (
            ParticipantPersistenceError::InvalidReference,
            "assessment participant persistence references must be opaque values",
        ),
        (
            ParticipantPersistenceError::ConflictingReplay,
            "assessment participant identity was replayed with conflicting evidence",
        ),
        (
            ParticipantPersistenceError::InvalidTimestamp,
            "assessment participant timestamp exceeds the PostgreSQL bigint range",
        ),
        (
            ParticipantPersistenceError::UnsupportedIsolationLevel,
            "assessment participant persistence requires read committed isolation",
        ),
        (
            ParticipantPersistenceError::IdentityLinkOutOfScope,
            "assessment participant persistence stores anonymous identity only",
        ),
        (
            ParticipantPersistenceError::NotFound,
            "assessment participant was not found for the requested tenant",
        ),
    ] {
        assert_eq!(error.to_string(), expected_message);
        assert!(std::error::Error::source(&error).is_none());
    }

    let mut client = test_client();
    let database_error = client
        .query_one(
            "SELECT * FROM assessment_participant_error_contract_missing_relation",
            &[],
        )
        .unwrap_err();
    let error = ParticipantPersistenceError::from(database_error);
    assert_eq!(
        error.to_string(),
        "PostgreSQL assessment-participant persistence failed"
    );
    assert!(std::error::Error::source(&error).is_some());
}
