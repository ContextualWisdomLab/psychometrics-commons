//! Stable operator-facing error contracts for data-rights persistence.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::postgres_data_rights::DataRightsPersistenceError;
use psychometrics_commons_runtime::postgres_integration::PersistenceError;

fn test_client() -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    Client::connect(&connection, NoTls).expect("isolated CI PostgreSQL database must be reachable")
}

#[test]
fn persistence_errors_expose_stable_messages_and_database_sources() {
    for (error, expected_message) in [
        (
            DataRightsPersistenceError::InvalidReference,
            "data-rights propagation references must be opaque non-numeric values",
        ),
        (
            DataRightsPersistenceError::InvalidRequestState,
            "data-rights durable propagation requires Requested state",
        ),
        (
            DataRightsPersistenceError::EmptyTargetSet,
            "data-rights propagation requires at least one dependent system",
        ),
        (
            DataRightsPersistenceError::DuplicateTarget,
            "data-rights propagation target set contains a duplicate system",
        ),
        (
            DataRightsPersistenceError::DuplicateEventIdentity,
            "data-rights propagation target set reuses an event identity",
        ),
        (
            DataRightsPersistenceError::InvalidPropagationEnvelope,
            "data-rights propagation event does not match the durable request",
        ),
        (
            DataRightsPersistenceError::ConflictingReplay,
            "data-rights request was replayed with conflicting durable evidence",
        ),
        (
            DataRightsPersistenceError::UnsupportedIsolationLevel,
            "data-rights persistence requires read committed isolation",
        ),
        (
            DataRightsPersistenceError::ValueOutOfRange,
            "data-rights persistence value exceeds the PostgreSQL bigint range",
        ),
    ] {
        assert_eq!(error.to_string(), expected_message);
        assert!(std::error::Error::source(&error).is_none());
    }

    let integration = DataRightsPersistenceError::Integration(PersistenceError::InvalidReference);
    assert_eq!(
        integration.to_string(),
        "data-rights outbox evidence failed persistence validation"
    );
    assert!(std::error::Error::source(&integration).is_some());

    let mut client = test_client();
    let database_error = client
        .query_one(
            "SELECT * FROM data_rights_error_contract_missing_relation",
            &[],
        )
        .unwrap_err();
    let error = DataRightsPersistenceError::from(database_error);
    assert_eq!(
        error.to_string(),
        "PostgreSQL data-rights persistence operation failed"
    );
    assert!(std::error::Error::source(&error).is_some());
}
