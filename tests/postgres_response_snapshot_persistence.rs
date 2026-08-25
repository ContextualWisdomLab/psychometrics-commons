//! Real `PostgreSQL` contract for durable immutable response snapshots.

use postgres::{Client, IsolationLevel, NoTls};
use psychometrics_commons_runtime::postgres_response_snapshot::{
    apply_response_snapshot_migration, persist_response_snapshot,
    ResponseSnapshotPersistenceDisposition, ResponseSnapshotPersistenceError,
};
#[path = "response_support/mod.rs"]
mod response_support;

use psychometrics_commons_runtime::response::{ResponseLedger, ResponseWrite};
use psychometrics_commons_runtime::session::SessionState;
use response_support::{advance_to, active_session};

const RESPONSE_SNAPSHOT_TEST_LOCK_KEY: i64 = 0x5253_5052_5354_4C4B;
const PAYLOAD_DIGEST: &str =
    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const OTHER_DIGEST: &str =
    "sha256:fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210";

fn response_snapshot_test_guard() -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let mut guard = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    guard
        .query_one(
            "SELECT pg_advisory_lock($1)",
            &[&RESPONSE_SNAPSHOT_TEST_LOCK_KEY],
        )
        .expect("shared response-snapshot persistence test lock should be acquired");
    guard
}

fn test_client() -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let mut client = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    client
        .batch_execute(
            "CREATE SCHEMA IF NOT EXISTS response_snapshot_persistence_test;\
             SET search_path TO response_snapshot_persistence_test;",
        )
        .unwrap();
    client
}

fn reset_response_snapshot_tables(client: &mut Client) {
    client
        .batch_execute(
            "DROP TABLE IF EXISTS response_snapshot_persistence_test.response_snapshot_entry;\
             DROP TABLE IF EXISTS response_snapshot_persistence_test.response_snapshot;",
        )
        .unwrap();
}

fn persist_ok(
    client: &mut Client,
    snapshot: &psychometrics_commons_runtime::response::ResponseSnapshot,
) -> ResponseSnapshotPersistenceDisposition {
    let mut transaction = client.transaction().unwrap();
    let disposition = persist_response_snapshot(&mut transaction, snapshot).unwrap();
    transaction.commit().unwrap();
    disposition
}

fn persist_err(
    client: &mut Client,
    snapshot: &psychometrics_commons_runtime::response::ResponseSnapshot,
) -> ResponseSnapshotPersistenceError {
    let mut transaction = client.transaction().unwrap();
    let error = persist_response_snapshot(&mut transaction, snapshot).unwrap_err();
    transaction.rollback().unwrap();
    error
}

/// Freeze one session-bound completed snapshot through the authoritative ledger API.
fn frozen_snapshot(
    session_ref: &str,
    snapshot_ref: &str,
    writes: &[ResponseWrite<'_>],
) -> psychometrics_commons_runtime::response::ResponseSnapshot {
    let mut session = active_session(session_ref);
    let mut ledger = ResponseLedger::from_session(&session).unwrap();
    for request in writes {
        ledger.record(&session, *request).unwrap();
    }
    advance_to(&mut session, SessionState::Completed);
    ledger.freeze_as(&session, snapshot_ref).unwrap()
}

/// Freeze one session-bound snapshot without pinning a server snapshot reference.
fn unbound_frozen_snapshot(
    session_ref: &str,
    writes: &[ResponseWrite<'_>],
) -> psychometrics_commons_runtime::response::ResponseSnapshot {
    let mut session = active_session(session_ref);
    let mut ledger = ResponseLedger::from_session(&session).unwrap();
    for request in writes {
        ledger.record(&session, *request).unwrap();
    }
    advance_to(&mut session, SessionState::Completed);
    ledger.freeze(&session).unwrap()
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

#[test]
fn response_snapshot_fixture_guard_is_visible_to_another_postgres_session() {
    let _guard = response_snapshot_test_guard();
    let mut contender = test_client();
    let acquired: bool = contender
        .query_one(
            "SELECT pg_try_advisory_lock($1)",
            &[&RESPONSE_SNAPSHOT_TEST_LOCK_KEY],
        )
        .expect("contender lock probe should succeed")
        .get(0);

    assert!(
        !acquired,
        "fixed-schema response-snapshot fixture guard must serialize across PostgreSQL sessions"
    );
}

#[test]
fn empty_completed_snapshot_persist_is_exactly_idempotent() {
    let _guard = response_snapshot_test_guard();
    let mut client = test_client();
    reset_response_snapshot_tables(&mut client);
    apply_response_snapshot_migration(&mut client).unwrap();

    let snapshot = frozen_snapshot("session_snapshot_empty", "response_snapshot_empty", &[]);
    assert_eq!(
        persist_ok(&mut client, &snapshot),
        ResponseSnapshotPersistenceDisposition::Inserted
    );
    assert_eq!(
        persist_ok(&mut client, &snapshot),
        ResponseSnapshotPersistenceDisposition::Duplicate
    );
}

#[test]
fn accepted_snapshot_is_idempotent_and_entry_rebinding_fails_closed() {
    let _guard = response_snapshot_test_guard();
    let mut client = test_client();
    reset_response_snapshot_tables(&mut client);
    apply_response_snapshot_migration(&mut client).unwrap();

    let snapshot = frozen_snapshot(
        "session_snapshot_beta",
        "response_snapshot_beta",
        &[write(
            "server_event_001",
            "client_event_001",
            "item_version_001",
            PAYLOAD_DIGEST,
        )],
    );
    assert_eq!(
        persist_ok(&mut client, &snapshot),
        ResponseSnapshotPersistenceDisposition::Inserted
    );
    assert_eq!(
        persist_ok(&mut client, &snapshot),
        ResponseSnapshotPersistenceDisposition::Duplicate
    );

    client
        .execute(
            "UPDATE response_snapshot_entry SET payload_digest = $1 \
             WHERE snapshot_ref = 'response_snapshot_beta'",
            &[&OTHER_DIGEST],
        )
        .unwrap();
    assert!(matches!(
        persist_err(&mut client, &snapshot),
        ResponseSnapshotPersistenceError::ConflictingReplay
    ));
    client
        .execute(
            "UPDATE response_snapshot_entry SET payload_digest = $1, \
                 item_version_ref = 'item_version_other' \
             WHERE snapshot_ref = 'response_snapshot_beta'",
            &[&PAYLOAD_DIGEST],
        )
        .unwrap();
    assert!(matches!(
        persist_err(&mut client, &snapshot),
        ResponseSnapshotPersistenceError::ConflictingReplay
    ));
    client
        .execute(
            "UPDATE response_snapshot_entry SET item_version_ref = 'item_version_001', \
                 event_ref = 'server_event_other' \
             WHERE snapshot_ref = 'response_snapshot_beta'",
            &[],
        )
        .unwrap();
    assert!(matches!(
        persist_err(&mut client, &snapshot),
        ResponseSnapshotPersistenceError::ConflictingReplay
    ));
}

#[test]
fn snapshot_header_count_and_sequence_rebinding_fails_closed() {
    let _guard = response_snapshot_test_guard();
    let mut client = test_client();
    reset_response_snapshot_tables(&mut client);
    apply_response_snapshot_migration(&mut client).unwrap();

    let snapshot = frozen_snapshot(
        "session_snapshot_header",
        "response_snapshot_header",
        &[write(
            "server_event_001",
            "client_event_001",
            "item_version_001",
            PAYLOAD_DIGEST,
        )],
    );
    persist_ok(&mut client, &snapshot);
    client
        .execute(
            "UPDATE response_snapshot SET event_count = 99 \
             WHERE snapshot_ref = 'response_snapshot_header'",
            &[],
        )
        .unwrap();
    assert!(matches!(
        persist_err(&mut client, &snapshot),
        ResponseSnapshotPersistenceError::ConflictingReplay
    ));
    client
        .execute(
            "UPDATE response_snapshot SET event_count = 1, last_sequence = 99 \
             WHERE snapshot_ref = 'response_snapshot_header'",
            &[],
        )
        .unwrap();
    assert!(matches!(
        persist_err(&mut client, &snapshot),
        ResponseSnapshotPersistenceError::ConflictingReplay
    ));
}

#[test]
fn snapshot_session_rebinding_fails_closed() {
    let _guard = response_snapshot_test_guard();
    let mut client = test_client();
    reset_response_snapshot_tables(&mut client);
    apply_response_snapshot_migration(&mut client).unwrap();

    persist_ok(
        &mut client,
        &frozen_snapshot(
            "session_snapshot_gamma",
            "response_snapshot_gamma",
            &[write(
                "server_event_001",
                "client_event_001",
                "item_version_001",
                PAYLOAD_DIGEST,
            )],
        ),
    );
    let rebound = frozen_snapshot(
        "session_snapshot_other",
        "response_snapshot_gamma",
        &[write(
            "server_event_001",
            "client_event_001",
            "item_version_001",
            PAYLOAD_DIGEST,
        )],
    );
    assert!(matches!(
        persist_err(&mut client, &rebound),
        ResponseSnapshotPersistenceError::ConflictingReplay
    ));
}

#[test]
fn unbound_snapshot_fails_closed_before_insert() {
    let _guard = response_snapshot_test_guard();
    let mut client = test_client();
    reset_response_snapshot_tables(&mut client);
    apply_response_snapshot_migration(&mut client).unwrap();

    let snapshot = unbound_frozen_snapshot(
        "session_snapshot_unbound",
        &[write(
            "server_event_unbound",
            "client_event_unbound",
            "item_version_001",
            PAYLOAD_DIGEST,
        )],
    );
    assert!(matches!(
        persist_err(&mut client, &snapshot),
        ResponseSnapshotPersistenceError::InvalidReference
    ));
}

#[test]
fn response_snapshot_persistence_requires_read_committed() {
    let _guard = response_snapshot_test_guard();
    let mut client = test_client();
    reset_response_snapshot_tables(&mut client);
    apply_response_snapshot_migration(&mut client).unwrap();

    let snapshot = frozen_snapshot(
        "session_snapshot_serializable",
        "response_snapshot_serializable",
        &[],
    );
    let mut transaction = client
        .build_transaction()
        .isolation_level(IsolationLevel::Serializable)
        .start()
        .unwrap();
    assert!(matches!(
        persist_response_snapshot(&mut transaction, &snapshot),
        Err(ResponseSnapshotPersistenceError::UnsupportedIsolationLevel)
    ));
    transaction.rollback().unwrap();
}

#[test]
fn entry_insert_failure_is_a_database_failure() {
    let _guard = response_snapshot_test_guard();
    let mut client = test_client();
    reset_response_snapshot_tables(&mut client);
    apply_response_snapshot_migration(&mut client).unwrap();

    client
        .batch_execute(
            "CREATE OR REPLACE FUNCTION response_snapshot_reject_entry() \
             RETURNS trigger LANGUAGE plpgsql AS $$ \
             BEGIN \
                 RAISE EXCEPTION 'response_snapshot entry sink'; \
             END $$; \
             CREATE TRIGGER response_snapshot_reject_entry \
             BEFORE INSERT ON response_snapshot_entry \
             FOR EACH STATEMENT EXECUTE FUNCTION response_snapshot_reject_entry();",
        )
        .unwrap();

    let snapshot = frozen_snapshot(
        "session_snapshot_hidden_insert",
        "response_snapshot_hidden_insert",
        &[write(
            "server_event_001",
            "client_event_001",
            "item_version_001",
            PAYLOAD_DIGEST,
        )],
    );
    assert!(matches!(
        persist_err(&mut client, &snapshot),
        ResponseSnapshotPersistenceError::Database(_)
    ));
}

#[test]
fn header_replay_select_failure_is_a_database_failure() {
    let _guard = response_snapshot_test_guard();
    let mut client = test_client();
    reset_response_snapshot_tables(&mut client);
    apply_response_snapshot_migration(&mut client).unwrap();

    let snapshot = frozen_snapshot(
        "session_snapshot_hidden_header",
        "response_snapshot_hidden_header",
        &[],
    );
    persist_ok(&mut client, &snapshot);
    client
        .batch_execute(
            "CREATE SCHEMA IF NOT EXISTS response_snapshot_select_failure_sink;\
             CREATE OR REPLACE FUNCTION response_snapshot_redirect_after_insert() \
             RETURNS trigger LANGUAGE plpgsql AS $$ \
             BEGIN \
                 PERFORM set_config('search_path', 'response_snapshot_select_failure_sink', false); \
                 RETURN NULL; \
             END $$; \
             CREATE TRIGGER response_snapshot_redirect_after_insert \
             AFTER INSERT ON response_snapshot \
             FOR EACH STATEMENT EXECUTE FUNCTION response_snapshot_redirect_after_insert();",
        )
        .unwrap();
    assert!(matches!(
        persist_err(&mut client, &snapshot),
        ResponseSnapshotPersistenceError::Database(_)
    ));
}

#[test]
fn missing_response_snapshot_relation_is_a_database_failure() {
    let _guard = response_snapshot_test_guard();
    let mut client = test_client();
    reset_response_snapshot_tables(&mut client);

    let snapshot = frozen_snapshot("session_snapshot_missing", "response_snapshot_missing", &[]);
    assert!(matches!(
        persist_err(&mut client, &snapshot),
        ResponseSnapshotPersistenceError::Database(_)
    ));
}

#[test]
fn entry_replay_select_failure_is_a_database_failure() {
    let _guard = response_snapshot_test_guard();
    let mut client = test_client();
    reset_response_snapshot_tables(&mut client);
    apply_response_snapshot_migration(&mut client).unwrap();

    let snapshot = frozen_snapshot(
        "session_snapshot_hidden_entry",
        "response_snapshot_hidden_entry",
        &[write(
            "server_event_001",
            "client_event_001",
            "item_version_001",
            PAYLOAD_DIGEST,
        )],
    );
    persist_ok(&mut client, &snapshot);
    client
        .batch_execute("DROP TABLE response_snapshot_persistence_test.response_snapshot_entry;")
        .unwrap();
    assert!(matches!(
        persist_err(&mut client, &snapshot),
        ResponseSnapshotPersistenceError::Database(_)
    ));
}
