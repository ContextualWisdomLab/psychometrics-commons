//! Real PostgreSQL regression coverage for response-event uniqueness conflicts.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::postgres_response::{
    apply_response_event_migration, persist_response_ledger, ResponsePersistenceError,
};
use psychometrics_commons_runtime::response::{ResponseLedger, ResponseWrite};
use psychometrics_commons_runtime::session::SessionState;
use std::sync::{Mutex, MutexGuard};

const DIGEST: &str =
    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
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
            "CREATE SCHEMA IF NOT EXISTS response_event_unique_collision_test;\
             SET search_path TO response_event_unique_collision_test;\
             DROP TABLE IF EXISTS response_event;\
             DROP TABLE IF EXISTS response_event_ledger;",
        )
        .unwrap();
    apply_response_event_migration(&mut client).unwrap();
    client
}

fn one_event_ledger(
    session_ref: &str,
    server_event_ref: &str,
    client_event_ref: &str,
) -> ResponseLedger {
    let mut ledger = ResponseLedger::new(session_ref).unwrap();
    ledger
        .record(
            SessionState::Active,
            ResponseWrite {
                server_event_ref,
                client_event_ref,
                item_version_ref: "item_version_new",
                payload_digest: DIGEST,
            },
        )
        .unwrap();
    ledger
}

fn insert_existing_event(
    client: &mut Client,
    session_ref: &str,
    server_event_ref: &str,
    client_event_ref: &str,
    server_sequence: i64,
) {
    client
        .execute(
            "INSERT INTO response_event_ledger (session_ref) VALUES ($1)",
            &[&session_ref],
        )
        .unwrap();
    client
        .execute(
            "INSERT INTO response_event (\
                 session_ref, server_event_ref, client_event_ref, item_version_ref, \
                 payload_digest, server_sequence\
             ) VALUES ($1, $2, $3, 'item_version_existing', $4, $5)",
            &[
                &session_ref,
                &server_event_ref,
                &client_event_ref,
                &DIGEST,
                &server_sequence,
            ],
        )
        .unwrap();
}

fn assert_conflict_rolls_back(client: &mut Client, ledger: &ResponseLedger, session_ref: &str) {
    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        persist_response_ledger(&mut transaction, ledger),
        Err(ResponsePersistenceError::ConflictingReplay)
    ));
    transaction.rollback().unwrap();

    let event_count: i64 = client
        .query_one(
            "SELECT COUNT(*) FROM response_event WHERE session_ref = $1",
            &[&session_ref],
        )
        .unwrap()
        .get(0);
    assert_eq!(event_count, 1);
}

#[test]
fn distinct_server_event_reusing_client_identity_is_conflicting_replay() {
    let _guard = test_guard();
    let mut client = test_client();
    let session_ref = "session_client_collision";
    insert_existing_event(
        &mut client,
        session_ref,
        "server_event_existing",
        "client_event_shared",
        99,
    );

    let replay = one_event_ledger(
        session_ref,
        "server_event_new",
        "client_event_shared",
    );
    assert_conflict_rolls_back(&mut client, &replay, session_ref);
}

#[test]
fn distinct_server_event_reusing_server_sequence_is_conflicting_replay() {
    let _guard = test_guard();
    let mut client = test_client();
    let session_ref = "session_sequence_collision";
    insert_existing_event(
        &mut client,
        session_ref,
        "server_event_existing",
        "client_event_existing",
        1,
    );

    let replay = one_event_ledger(session_ref, "server_event_new", "client_event_new");
    assert_conflict_rolls_back(&mut client, &replay, session_ref);
}
