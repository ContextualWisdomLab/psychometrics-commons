//! Real `PostgreSQL` contract for durable response-event evidence.

use postgres::{Client, IsolationLevel, NoTls};
use psychometrics_commons_runtime::postgres_response::{
    apply_response_event_migration, persist_response_ledger, ResponsePersistenceDisposition,
    ResponsePersistenceError,
};
use psychometrics_commons_runtime::response::{ResponseLedger, ResponseWrite};
use psychometrics_commons_runtime::session::SessionState;
use std::sync::{Mutex, MutexGuard};

const PAYLOAD_DIGEST: &str =
    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const OTHER_DIGEST: &str =
    "sha256:fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210";

static RESPONSE_TEST_LOCK: Mutex<()> = Mutex::new(());

fn response_test_guard() -> MutexGuard<'static, ()> {
    RESPONSE_TEST_LOCK
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
            "CREATE SCHEMA IF NOT EXISTS response_event_persistence_test;\
             SET search_path TO response_event_persistence_test;",
        )
        .unwrap();
    client
}

fn reset_response_tables(client: &mut Client) {
    client
        .batch_execute(
            "DROP TABLE IF EXISTS response_event_persistence_test.response_event;\
             DROP TABLE IF EXISTS response_event_persistence_test.response_event_ledger;",
        )
        .unwrap();
}

fn persist_ok(client: &mut Client, ledger: &ResponseLedger) -> ResponsePersistenceDisposition {
    let mut transaction = client.transaction().unwrap();
    let disposition = persist_response_ledger(&mut transaction, ledger).unwrap();
    transaction.commit().unwrap();
    disposition
}

fn persist_err(client: &mut Client, ledger: &ResponseLedger) -> ResponsePersistenceError {
    let mut transaction = client.transaction().unwrap();
    let error = persist_response_ledger(&mut transaction, ledger).unwrap_err();
    transaction.rollback().unwrap();
    error
}

fn write<'a>(
    server_event_ref: &'a str,
    client_event_ref: &'a str,
    item_version_ref: &'a str,
    payload_digest: &'a str,
) -> ResponseWrite<'a> {
    ResponseWrite {
        server_event_ref,
        client_event_ref,
        item_version_ref,
        payload_digest,
    }
}

fn recorded_ledger(session_ref: &str, writes: &[ResponseWrite<'_>]) -> ResponseLedger {
    let mut ledger = ResponseLedger::new(session_ref).unwrap();
    for request in writes {
        ledger.record(SessionState::Active, *request).unwrap();
    }
    ledger
}

#[test]
fn empty_response_ledger_persist_is_exactly_idempotent() {
    let _guard = response_test_guard();
    let mut client = test_client();
    reset_response_tables(&mut client);
    apply_response_event_migration(&mut client).unwrap();

    let ledger = ResponseLedger::new("session_response_empty").unwrap();
    assert_eq!(
        persist_ok(&mut client, &ledger),
        ResponsePersistenceDisposition::Inserted
    );
    assert_eq!(
        persist_ok(&mut client, &ledger),
        ResponsePersistenceDisposition::Duplicate
    );
}

#[test]
fn accepted_events_are_idempotent_and_digest_rebinding_fails_closed() {
    let _guard = response_test_guard();
    let mut client = test_client();
    reset_response_tables(&mut client);
    apply_response_event_migration(&mut client).unwrap();

    let ledger = recorded_ledger(
        "session_response_beta",
        &[write(
            "server_event_001",
            "client_event_001",
            "item_version_001",
            PAYLOAD_DIGEST,
        )],
    );
    assert_eq!(
        persist_ok(&mut client, &ledger),
        ResponsePersistenceDisposition::Inserted
    );
    assert_eq!(
        persist_ok(&mut client, &ledger),
        ResponsePersistenceDisposition::Duplicate
    );

    client
        .execute(
            "UPDATE response_event SET payload_digest = $1 \
             WHERE session_ref = 'session_response_beta'",
            &[&OTHER_DIGEST],
        )
        .unwrap();
    assert!(matches!(
        persist_err(&mut client, &ledger),
        ResponsePersistenceError::ConflictingReplay
    ));
    client
        .execute(
            "UPDATE response_event SET item_version_ref = 'item_version_other', \
                 payload_digest = $1 \
             WHERE session_ref = 'session_response_beta'",
            &[&PAYLOAD_DIGEST],
        )
        .unwrap();
    assert!(matches!(
        persist_err(&mut client, &ledger),
        ResponsePersistenceError::ConflictingReplay
    ));
    client
        .execute(
            "UPDATE response_event SET item_version_ref = 'item_version_001', \
                 client_event_ref = 'client_event_other', server_sequence = 1 \
             WHERE session_ref = 'session_response_beta'",
            &[],
        )
        .unwrap();
    assert!(matches!(
        persist_err(&mut client, &ledger),
        ResponsePersistenceError::ConflictingReplay
    ));
    client
        .execute(
            "UPDATE response_event SET client_event_ref = 'client_event_001', \
                 server_sequence = 99 \
             WHERE session_ref = 'session_response_beta'",
            &[],
        )
        .unwrap();
    assert!(matches!(
        persist_err(&mut client, &ledger),
        ResponsePersistenceError::ConflictingReplay
    ));
}

#[test]
fn later_events_append_and_sessions_stay_isolated() {
    let _guard = response_test_guard();
    let mut client = test_client();
    reset_response_tables(&mut client);
    apply_response_event_migration(&mut client).unwrap();

    let first = recorded_ledger(
        "session_response_left",
        &[write(
            "server_event_001",
            "client_event_001",
            "item_version_001",
            PAYLOAD_DIGEST,
        )],
    );
    persist_ok(&mut client, &first);
    let mut later = first;
    later
        .record(
            SessionState::Active,
            write(
                "server_event_002",
                "client_event_002",
                "item_version_002",
                OTHER_DIGEST,
            ),
        )
        .unwrap();
    assert_eq!(
        persist_ok(&mut client, &later),
        ResponsePersistenceDisposition::Inserted
    );
    persist_ok(
        &mut client,
        &ResponseLedger::new("session_response_right").unwrap(),
    );
    let left: i64 = client
        .query_one(
            "SELECT COUNT(*) FROM response_event WHERE session_ref = $1",
            &[&"session_response_left"],
        )
        .unwrap()
        .get(0);
    let right: i64 = client
        .query_one(
            "SELECT COUNT(*) FROM response_event WHERE session_ref = $1",
            &[&"session_response_right"],
        )
        .unwrap()
        .get(0);
    assert_eq!(left, 2);
    assert_eq!(right, 0);
}

#[test]
fn response_persistence_requires_read_committed() {
    let _guard = response_test_guard();
    let mut client = test_client();
    reset_response_tables(&mut client);
    apply_response_event_migration(&mut client).unwrap();

    let ledger = ResponseLedger::new("session_serializable").unwrap();
    let mut transaction = client
        .build_transaction()
        .isolation_level(IsolationLevel::Serializable)
        .start()
        .unwrap();
    assert!(matches!(
        persist_response_ledger(&mut transaction, &ledger),
        Err(ResponsePersistenceError::UnsupportedIsolationLevel)
    ));
    transaction.rollback().unwrap();
}

#[test]
fn missing_response_ledger_is_a_database_failure() {
    let _guard = response_test_guard();
    let mut client = test_client();
    reset_response_tables(&mut client);
    assert!(matches!(
        persist_err(
            &mut client,
            &ResponseLedger::new("session_missing").unwrap()
        ),
        ResponsePersistenceError::Database(_)
    ));
}

#[test]
fn missing_event_relation_after_header_is_a_database_failure() {
    let _guard = response_test_guard();
    let mut client = test_client();
    reset_response_tables(&mut client);
    apply_response_event_migration(&mut client).unwrap();
    persist_ok(
        &mut client,
        &ResponseLedger::new("session_missing_event").unwrap(),
    );
    client.batch_execute("DROP TABLE response_event;").unwrap();
    assert!(matches!(
        persist_err(
            &mut client,
            &recorded_ledger(
                "session_missing_event",
                &[write(
                    "server_event_001",
                    "client_event_001",
                    "item_version_001",
                    PAYLOAD_DIGEST,
                )],
            ),
        ),
        ResponsePersistenceError::Database(_)
    ));
}

#[test]
fn replay_select_failure_is_a_database_failure() {
    let _guard = response_test_guard();
    let mut client = test_client();
    reset_response_tables(&mut client);
    apply_response_event_migration(&mut client).unwrap();

    let ledger = recorded_ledger(
        "session_hidden_select",
        &[write(
            "server_event_001",
            "client_event_001",
            "item_version_001",
            PAYLOAD_DIGEST,
        )],
    );
    persist_ok(&mut client, &ledger);
    client
        .batch_execute(
            "CREATE SCHEMA IF NOT EXISTS response_event_select_failure_sink;\
             CREATE OR REPLACE FUNCTION response_event_redirect_after_insert() \
             RETURNS trigger LANGUAGE plpgsql AS $$ \
             BEGIN \
                 PERFORM set_config('search_path', 'response_event_select_failure_sink', false); \
                 RETURN NULL; \
             END $$; \
             CREATE TRIGGER response_event_redirect_after_insert \
             AFTER INSERT ON response_event \
             FOR EACH STATEMENT EXECUTE FUNCTION response_event_redirect_after_insert();",
        )
        .unwrap();
    assert!(matches!(
        persist_err(&mut client, &ledger),
        ResponsePersistenceError::Database(_)
    ));
}
