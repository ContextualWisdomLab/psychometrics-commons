//! Exact-spelling contracts for response-event persistence aliases.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::postgres_response_event::{
    apply_response_event_migration, persist_response_event, ResponseEventPersistenceError,
};
use psychometrics_commons_runtime::response::ResponseEvent;

fn test_client() -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let mut client = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    client
        .batch_execute(
            "CREATE SCHEMA IF NOT EXISTS response_event_exact_reference_test;\
             SET search_path TO response_event_exact_reference_test;\
             DROP TABLE IF EXISTS response_event;",
        )
        .unwrap();
    apply_response_event_migration(&mut client).unwrap();
    client
}

#[test]
fn persist_rejects_padded_session_alias_before_insert() {
    let mut client = test_client();
    let event = ResponseEvent::from_persisted(
        "server_event_item_01",
        "client_event_item_01",
        "item_version_n1_ko",
        "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        1,
    )
    .unwrap();

    let mut transaction = client.transaction().unwrap();
    let error = persist_response_event(
        &mut transaction,
        " session_ipip_ko_quick ",
        &event,
        1_700_000_000_000,
        1_700_000_000_250,
    )
    .expect_err("padded session aliases must fail instead of collapsing to another spelling");

    assert!(matches!(
        error,
        ResponseEventPersistenceError::InvalidReference
    ));
    let persisted_rows: i64 = transaction
        .query_one("SELECT COUNT(*) FROM response_event", &[])
        .unwrap()
        .get(0);
    assert_eq!(persisted_rows, 0, "a rejected alias must not persist a row");
    transaction.rollback().unwrap();
}
