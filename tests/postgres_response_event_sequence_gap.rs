//! Regression contract: response-event persistence rejects a non-contiguous server sequence.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::postgres_response_event::{
    apply_response_event_migration, persist_response_event, ResponseEventPersistenceError,
};
use psychometrics_commons_runtime::response::ResponseEvent;
use std::sync::{Mutex, MutexGuard};

const OBSERVED_AT_MS: u64 = 1_700_000_000_000;
const RECEIVED_AT_MS: u64 = 1_700_000_000_250;
const OBSERVED_AT_MS_FLOAT: f64 = 1_700_000_000_000.0;
const RECEIVED_AT_MS_FLOAT: f64 = 1_700_000_000_250.0;

static TEST_LOCK: Mutex<()> = Mutex::new(());

fn test_guard() -> MutexGuard<'static, ()> {
    TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

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
    let _guard = test_guard();
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
    assert!(error.to_string().contains("gapped"), "{error}");
    transaction.rollback().unwrap();
}

#[test]
fn persist_rejects_extension_of_a_preexisting_corrupt_prefix() {
    let _guard = test_guard();
    let mut client = test_client();
    client
        .execute(
            "INSERT INTO response_event (\
                 response_event_ref, session_ref, client_event_ref, item_version_ref, \
                 payload_digest, server_sequence, observed_at, received_at\
             ) VALUES ($1, $2, $3, $4, $5, 2, to_timestamp($6::double precision / 1000.0), \
                       to_timestamp($7::double precision / 1000.0))",
            &[
                &"server_event_corrupt_02",
                &"session_ipip_ko_corrupt_prefix",
                &"client_event_corrupt_02",
                &"item_version_n2_ko",
                &"sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                &OBSERVED_AT_MS_FLOAT,
                &RECEIVED_AT_MS_FLOAT,
            ],
        )
        .unwrap();

    let next = ResponseEvent::from_persisted(
        "server_event_item_03",
        "client_event_item_03",
        "item_version_n3_ko",
        "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
        3,
    )
    .unwrap();
    let mut transaction = client.transaction().unwrap();
    let error = persist_response_event(
        &mut transaction,
        "session_ipip_ko_corrupt_prefix",
        &next,
        OBSERVED_AT_MS + 1_000,
        RECEIVED_AT_MS + 1_000,
    )
    .unwrap_err();

    assert!(matches!(
        error,
        ResponseEventPersistenceError::InvalidSequence
    ));
    let persisted_rows: i64 = transaction
        .query_one(
            "SELECT COUNT(*) FROM response_event WHERE session_ref = $1",
            &[&"session_ipip_ko_corrupt_prefix"],
        )
        .unwrap()
        .get(0);
    assert_eq!(
        persisted_rows, 1,
        "a corrupt stored prefix must not be extended"
    );
    transaction.rollback().unwrap();
}
