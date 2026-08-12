//! Stable operator-facing error contracts for PostgreSQL scoring-job persistence.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::postgres_scoring_job::ScoringJobPersistenceError;

fn test_client() -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    Client::connect(&connection, NoTls).expect("isolated CI PostgreSQL database must be reachable")
}

#[test]
fn persistence_errors_expose_stable_messages_and_database_sources() {
    for (error, expected_message) in [
        (
            ScoringJobPersistenceError::InvalidReference,
            "scoring persistence references must be opaque values",
        ),
        (
            ScoringJobPersistenceError::InvalidTimestamp,
            "scoring persistence timestamps must be greater than zero",
        ),
        (
            ScoringJobPersistenceError::ValueOutOfRange,
            "scoring persistence value exceeds the PostgreSQL range",
        ),
        (
            ScoringJobPersistenceError::InvalidLeaseWindow,
            "scoring lease expiry must be later than claim time",
        ),
        (
            ScoringJobPersistenceError::UnsupportedInitialState,
            "only a fresh queued scoring job may be inserted",
        ),
        (
            ScoringJobPersistenceError::ConflictingReplay,
            "scoring job identity was replayed with conflicting evidence",
        ),
        (
            ScoringJobPersistenceError::UnsupportedIsolationLevel,
            "scoring job enqueue replay requires read committed isolation",
        ),
        (
            ScoringJobPersistenceError::JobNotFound,
            "scoring job does not exist",
        ),
        (
            ScoringJobPersistenceError::NotLeaseable,
            "scoring job is not currently leaseable",
        ),
    ] {
        assert_eq!(error.to_string(), expected_message);
        assert!(std::error::Error::source(&error).is_none());
    }

    let mut client = test_client();
    let database_error = client
        .query_one(
            "SELECT * FROM scoring_job_error_contract_missing_relation",
            &[],
        )
        .unwrap_err();
    let error = ScoringJobPersistenceError::from(database_error);
    assert_eq!(error.to_string(), "PostgreSQL scoring-job persistence failed");
    assert!(std::error::Error::source(&error).is_some());
}
