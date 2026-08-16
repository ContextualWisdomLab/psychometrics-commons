//! Regression contract: response-event persistence rejects a non-contiguous server sequence.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::postgres_response_event::{
    apply_response_event_migration, persist_response_event, ResponseEventPersistenceError,
};
use psychometrics_commons_runtime::response::ResponseEvent;

const OBSERVED_AT_MS: u64 = 1_700_000_000_000;
const RECEIVED_AT_MS: u64 = 1_700_000_000_250;

fn test_client() -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let mut client = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    client
        .batch_execute(
            "CREATE SCHEMA IF NOT EXISTS response_event_sequence_gap_test;\
             SET search_path TO response_event_sequence_gap_test;\
             DROP TABLE IF EXISTS response_event;",
        )
        .unwrap();
    apply_response_event_migration(&mut client).unwrap();
    client
}

#[test]
fn persist_rejects_a_server_sequence_gap_before_commit() {
    let mut client = test_client();
    let gapped = ResponseEvent::from_persisted(
        "server_event_item_02",
        "client_event_item_02",
        "item_version_n2_ko",
        "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        2,
    )
    .unwrap();

    let mut transaction = client.transaction().unwrap();
    let error = persist_response_event(
        &mut transaction,
        "session_ipip_ko_sequence_gap",
        &gapped,
        OBSERVED_AT_MS,
        RECEIVED_AT_MS,
    )
    .unwrap_err();

    assert!(matches!(
        error,
        ResponseEventPersistenceError::InvalidSequence
    ));
    let persisted_rows: i64 = transaction
        .query_one("SELECT COUNT(*) FROM response_event", &[])
        .unwrap()
        .get(0);
    assert_eq!(
        persisted_rows, 0,
        "a rejected gap must leave no durable row"
    );
    transaction.rollback().unwrap();
}
