//! Real `PostgreSQL` contract for durable mid-session response events.

use postgres::{Client, IsolationLevel, NoTls};
use psychometrics_commons_runtime::postgres_response_event::{
    apply_response_event_migration, load_response_ledger, persist_response_event,
    ResponseEventPersistenceDisposition, ResponseEventPersistenceError,
};
use psychometrics_commons_runtime::response::{ResponseEvent, ResponseLedger, ResponseWrite};
use psychometrics_commons_runtime::session::SessionState;
use std::sync::{Mutex, MutexGuard};

const DIGEST_N1: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const DIGEST_N2: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

static RESPONSE_EVENT_TEST_LOCK: Mutex<()> = Mutex::new(());

fn response_event_test_guard() -> MutexGuard<'static, ()> {
    RESPONSE_EVENT_TEST_LOCK
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

fn reset_response_event_table(client: &mut Client) {
    client
        .batch_execute("DROP TABLE IF EXISTS response_event_persistence_test.response_event;")
        .unwrap();
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

fn recorded_event(
    session_ref: &str,
    request: ResponseWrite<'_>,
) -> (ResponseLedger, ResponseEvent) {
    let mut ledger = ResponseLedger::new(session_ref).unwrap();
    let event = ledger.record(SessionState::Active, request).unwrap();
    (ledger, event)
}

fn persist_ok(
    client: &mut Client,
    session_ref: &str,
    event: &ResponseEvent,
) -> ResponseEventPersistenceDisposition {
    let mut transaction = client.transaction().unwrap();
    let disposition = persist_response_event(&mut transaction, session_ref, event).unwrap();
    transaction.commit().unwrap();
    disposition
}

fn persist_err(
    client: &mut Client,
    session_ref: &str,
    event: &ResponseEvent,
) -> ResponseEventPersistenceError {
    let mut transaction = client.transaction().unwrap();
    let error = persist_response_event(&mut transaction, session_ref, event).unwrap_err();
    transaction.rollback().unwrap();
    error
}

fn load_ok(client: &mut Client, session_ref: &str) -> ResponseLedger {
    let mut transaction = client.transaction().unwrap();
    let ledger = load_response_ledger(&mut transaction, session_ref).unwrap();
    transaction.commit().unwrap();
    ledger
}

fn load_err(client: &mut Client, session_ref: &str) -> ResponseEventPersistenceError {
    let mut transaction = client.transaction().unwrap();
    let error = load_response_ledger(&mut transaction, session_ref).unwrap_err();
    transaction.rollback().unwrap();
    error
}

fn rebound_event(
    client_event_ref: &str,
    item_version_ref: &str,
    payload_digest: &str,
    sequence: usize,
) -> ResponseEvent {
    ResponseEvent::from_persisted(
        "server_event_item_01",
        client_event_ref,
        item_version_ref,
        payload_digest,
        sequence,
    )
    .unwrap()
}

#[test]
fn two_item_korean_path_survives_restart_and_exact_replay() {
    let _guard = response_event_test_guard();
    let mut client = test_client();
    reset_response_event_table(&mut client);
    apply_response_event_migration(&mut client).unwrap();

    let mut live = ResponseLedger::new("session_ipip_ko_quick").unwrap();
    let first = live
        .record(
            SessionState::Active,
            write(
                "server_event_item_01",
                "client_event_item_01",
                "item_version_n1_ko",
                DIGEST_N1,
            ),
        )
        .unwrap();
    assert_eq!(
        persist_ok(&mut client, "session_ipip_ko_quick", &first),
        ResponseEventPersistenceDisposition::Inserted
    );
    assert_eq!(
        persist_ok(&mut client, "session_ipip_ko_quick", &first),
        ResponseEventPersistenceDisposition::Duplicate
    );

    let after_first = load_ok(&mut client, "session_ipip_ko_quick");
    assert_eq!(after_first.events(), std::slice::from_ref(&first));

    let second = live
        .record(
            SessionState::Active,
            write(
                "server_event_item_02",
                "client_event_item_02",
                "item_version_n2_ko",
                DIGEST_N2,
            ),
        )
        .unwrap();
    assert_eq!(
        persist_ok(&mut client, "session_ipip_ko_quick", &second),
        ResponseEventPersistenceDisposition::Inserted
    );

    let rebuilt = load_ok(&mut client, "session_ipip_ko_quick");
    assert_eq!(rebuilt, live);
    let snapshot = rebuilt
        .freeze_as(SessionState::Completed, "response_snapshot_ipip_ko_quick")
        .unwrap();
    assert_eq!(snapshot.event_count(), 2);
    assert_eq!(snapshot.last_sequence(), Some(2));
}

#[test]
fn empty_session_reload_is_an_empty_ledger() {
    let _guard = response_event_test_guard();
    let mut client = test_client();
    reset_response_event_table(&mut client);
    apply_response_event_migration(&mut client).unwrap();

    let rebuilt = load_ok(&mut client, "session_ipip_ko_empty");
    assert!(rebuilt.is_empty());
    assert_eq!(rebuilt.session_ref(), "session_ipip_ko_empty");
}

#[test]
fn event_identity_rebinding_and_sequence_reuse_fail_closed() {
    let _guard = response_event_test_guard();
    let mut client = test_client();
    reset_response_event_table(&mut client);
    apply_response_event_migration(&mut client).unwrap();

    let (_, first) = recorded_event(
        "session_ipip_ko_conflict",
        write(
            "server_event_item_01",
            "client_event_item_01",
            "item_version_n1_ko",
            DIGEST_N1,
        ),
    );
    persist_ok(&mut client, "session_ipip_ko_conflict", &first);

    assert!(matches!(
        persist_err(&mut client, "session_ipip_ko_other", &first),
        ResponseEventPersistenceError::ConflictingReplay
    ));
    for rebound in [
        rebound_event("client_event_item_99", "item_version_n1_ko", DIGEST_N1, 1),
        rebound_event("client_event_item_01", "item_version_n9_ko", DIGEST_N1, 1),
        rebound_event("client_event_item_01", "item_version_n1_ko", DIGEST_N2, 1),
        rebound_event("client_event_item_01", "item_version_n1_ko", DIGEST_N1, 2),
    ] {
        assert!(matches!(
            persist_err(&mut client, "session_ipip_ko_conflict", &rebound),
            ResponseEventPersistenceError::ConflictingReplay
        ));
    }

    let (_, other_server) = recorded_event(
        "session_ipip_ko_conflict",
        write(
            "server_event_item_99",
            "client_event_item_01",
            "item_version_n1_ko",
            DIGEST_N1,
        ),
    );
    assert!(matches!(
        persist_err(&mut client, "session_ipip_ko_conflict", &other_server),
        ResponseEventPersistenceError::ConflictingReplay
    ));
}

#[test]
fn reused_server_sequence_by_another_event_fails_closed() {
    let _guard = response_event_test_guard();
    let mut client = test_client();
    reset_response_event_table(&mut client);
    apply_response_event_migration(&mut client).unwrap();

    let (_, first) = recorded_event(
        "session_ipip_ko_sequence",
        write(
            "server_event_item_01",
            "client_event_item_01",
            "item_version_n1_ko",
            DIGEST_N1,
        ),
    );
    persist_ok(&mut client, "session_ipip_ko_sequence", &first);
    client
        .execute(
            "UPDATE response_event SET server_sequence = 2 \
             WHERE response_event_ref = 'server_event_item_01'",
            &[],
        )
        .unwrap();
    let (_, other_sequence) = recorded_event(
        "session_ipip_ko_sequence",
        write(
            "server_event_item_02",
            "client_event_item_02",
            "item_version_n2_ko",
            DIGEST_N2,
        ),
    );
    assert!(matches!(
        persist_err(&mut client, "session_ipip_ko_sequence", &other_sequence),
        ResponseEventPersistenceError::SequenceConflict
    ));
}

#[test]
fn persist_and_load_require_read_committed_and_opaque_session() {
    let _guard = response_event_test_guard();
    let mut client = test_client();
    reset_response_event_table(&mut client);
    apply_response_event_migration(&mut client).unwrap();

    let (_, event) = recorded_event(
        "session_ipip_ko_isolation",
        write(
            "server_event_item_01",
            "client_event_item_01",
            "item_version_n1_ko",
            DIGEST_N1,
        ),
    );
    let mut serializable = client
        .build_transaction()
        .isolation_level(IsolationLevel::Serializable)
        .start()
        .unwrap();
    assert!(matches!(
        persist_response_event(&mut serializable, "session_ipip_ko_isolation", &event),
        Err(ResponseEventPersistenceError::UnsupportedIsolationLevel)
    ));
    assert!(matches!(
        load_response_ledger(&mut serializable, "session_ipip_ko_isolation"),
        Err(ResponseEventPersistenceError::UnsupportedIsolationLevel)
    ));
    serializable.rollback().unwrap();

    assert!(matches!(
        persist_err(&mut client, " ", &event),
        ResponseEventPersistenceError::InvalidReference
    ));
    assert!(matches!(
        load_err(&mut client, "12"),
        ResponseEventPersistenceError::InvalidReference
    ));
}

#[test]
fn missing_relation_and_gapped_history_fail_closed() {
    let _guard = response_event_test_guard();
    let mut client = test_client();
    reset_response_event_table(&mut client);

    let (_, event) = recorded_event(
        "session_ipip_ko_missing",
        write(
            "server_event_item_01",
            "client_event_item_01",
            "item_version_n1_ko",
            DIGEST_N1,
        ),
    );
    assert!(matches!(
        persist_err(&mut client, "session_ipip_ko_missing", &event),
        ResponseEventPersistenceError::Database(_)
    ));

    apply_response_event_migration(&mut client).unwrap();
    persist_ok(&mut client, "session_ipip_ko_gap", &event);
    client
        .execute(
            "INSERT INTO response_event (\
                 response_event_ref, session_ref, client_event_ref, item_version_ref, \
                 payload_digest, server_sequence\
             ) VALUES (\
                 'server_event_item_03', 'session_ipip_ko_gap', 'client_event_item_03', \
                 'item_version_n3_ko', $1, 3\
             )",
            &[&DIGEST_N2],
        )
        .unwrap();
    assert!(matches!(
        load_err(&mut client, "session_ipip_ko_gap"),
        ResponseEventPersistenceError::InvalidSequence
    ));
}

#[test]
fn stored_noncanonical_digest_fails_closed_on_reload() {
    let _guard = response_event_test_guard();
    let mut client = test_client();
    reset_response_event_table(&mut client);
    apply_response_event_migration(&mut client).unwrap();

    client
        .batch_execute(
            "ALTER TABLE response_event DROP CONSTRAINT response_event_payload_digest_format_check;",
        )
        .unwrap();
    client
        .execute(
            "INSERT INTO response_event (\
                 response_event_ref, session_ref, client_event_ref, item_version_ref, \
                 payload_digest, server_sequence\
             ) VALUES (\
                 'server_event_item_01', 'session_ipip_ko_digest', 'client_event_item_01', \
                 'item_version_n1_ko', 'not-a-digest', 1\
             )",
            &[],
        )
        .unwrap();
    assert!(matches!(
        load_err(&mut client, "session_ipip_ko_digest"),
        ResponseEventPersistenceError::ConflictingReplay
    ));
}

#[test]
fn unexpected_unique_constraint_and_negative_sequence_fail_closed() {
    let _guard = response_event_test_guard();
    let mut client = test_client();
    reset_response_event_table(&mut client);
    apply_response_event_migration(&mut client).unwrap();

    let (_, first) = recorded_event(
        "session_ipip_ko_extra",
        write(
            "server_event_item_01",
            "client_event_item_01",
            "item_version_n1_ko",
            DIGEST_N1,
        ),
    );
    persist_ok(&mut client, "session_ipip_ko_extra", &first);
    client
        .batch_execute(
            "CREATE UNIQUE INDEX response_event_session_only_unique ON response_event (session_ref);",
        )
        .unwrap();
    let (_, second) = recorded_event(
        "session_ipip_ko_extra",
        write(
            "server_event_item_02",
            "client_event_item_02",
            "item_version_n2_ko",
            DIGEST_N2,
        ),
    );
    assert!(matches!(
        persist_err(&mut client, "session_ipip_ko_extra", &second),
        ResponseEventPersistenceError::Database(_)
    ));

    client
        .batch_execute(
            "DROP INDEX response_event_session_only_unique;\
             ALTER TABLE response_event DROP CONSTRAINT response_event_server_sequence_positive_check;",
        )
        .unwrap();
    client
        .execute(
            "INSERT INTO response_event (\
                 response_event_ref, session_ref, client_event_ref, item_version_ref, \
                 payload_digest, server_sequence\
             ) VALUES (\
                 'server_event_item_neg', 'session_ipip_ko_negative', 'client_event_item_neg', \
                 'item_version_n_neg', $1, -1\
             )",
            &[&DIGEST_N1],
        )
        .unwrap();
    assert!(matches!(
        load_err(&mut client, "session_ipip_ko_negative"),
        ResponseEventPersistenceError::InvalidSequence
    ));
}
