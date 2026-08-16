//! Stable operator-facing error contracts for `PostgreSQL` item-delivery persistence.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::postgres_item_delivery::ItemDeliveryPersistenceError;

fn test_client() -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    Client::connect(&connection, NoTls).expect("isolated CI PostgreSQL database must be reachable")
}

#[test]
fn persistence_errors_expose_stable_messages_and_database_sources() {
    for (error, expected_message) in [
        (
            ItemDeliveryPersistenceError::InvalidReference,
            "item delivery persistence references must be opaque values",
        ),
        (
            ItemDeliveryPersistenceError::ConflictingReplay,
            "item delivery identity was replayed with conflicting evidence",
        ),
        (
            ItemDeliveryPersistenceError::DuplicateItemDelivery,
            "item version was already delivered in this persisted session",
        ),
        (
            ItemDeliveryPersistenceError::SequenceConflict,
            "item delivery sequence was reused by a different delivery identity",
        ),
        (
            ItemDeliveryPersistenceError::UnsupportedIsolationLevel,
            "item delivery persistence requires read committed isolation",
        ),
    ] {
        assert_eq!(error.to_string(), expected_message);
        assert!(std::error::Error::source(&error).is_none());
    }

    let mut client = test_client();
    let database_error = client
        .query_one(
            "SELECT * FROM item_delivery_error_contract_missing_relation",
            &[],
        )
        .unwrap_err();
    let error = ItemDeliveryPersistenceError::from(database_error);
    assert_eq!(
        error.to_string(),
        "PostgreSQL item-delivery persistence failed"
    );
    assert!(std::error::Error::source(&error).is_some());
}
