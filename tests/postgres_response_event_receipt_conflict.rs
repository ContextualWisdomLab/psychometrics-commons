//! Real PostgreSQL regression for conflicting stored receipt identity history.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::postgres_response_event::{
    apply_response_event_migration, load_response_event_receipts, ResponseEventPersistenceError,
};

const DIGEST_ONE: &str =
    "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const DIGEST_TWO: &str =
    "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn test_client() -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let mut client = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    client
        .batch_execute(
            "CREATE SCHEMA IF NOT EXISTS response_event_receipt_conflict_test;\
             SET search_path TO response_event_receipt_conflict_test;\
             DROP TABLE IF EXISTS response_event;",
        )
        .unwrap();
    client
}

#[test]
fn receipt_reload_rejects_contiguous_history_with_duplicate_client_identity() {
    let mut client = test_client();
    apply_response_event_migration(&mut client).unwrap();
    client
        .batch_execute(
            "ALTER TABLE response_event DROP CONSTRAINT response_event_session_client_unique;",
        )
        .unwrap();
    client
        .execute(
            "INSERT INTO response_event (\
                 response_event_ref, session_ref, client_event_ref, item_version_ref, \
                 payload_digest, server_sequence, observed_at, received_at\
             ) VALUES (\
                 'server_event_item_one', 'session_ipip_ko_conflict', 'client_event_reused', \
                 'item_version_n_one_ko', $1, 1, TIMESTAMPTZ '2023-11-14 22:13:20+00', \
                 TIMESTAMPTZ '2023-11-14 22:13:20.250+00'\
             ), (\
                 'server_event_item_two', 'session_ipip_ko_conflict', 'client_event_reused', \
                 'item_version_n_two_ko', $2, 2, TIMESTAMPTZ '2023-11-14 22:13:21+00', \
                 TIMESTAMPTZ '2023-11-14 22:13:21.250+00'\
             )",
            &[&DIGEST_ONE, &DIGEST_TWO],
        )
        .unwrap();

    let mut transaction = client.transaction().unwrap();
    let error = load_response_event_receipts(&mut transaction, "session_ipip_ko_conflict")
        .expect_err("conflicting stored client identity must fail closed before receipts return");
    transaction.rollback().unwrap();

    assert!(matches!(
        error,
        ResponseEventPersistenceError::ConflictingReplay
    ));
}
