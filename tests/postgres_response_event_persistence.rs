//! Real `PostgreSQL` contract for in-progress response-event persistence.

use postgres::{Client, IsolationLevel, NoTls};
use psychometrics_commons_runtime::postgres_response_event::{
    apply_response_event_migration, load_response_event_times, load_response_ledger,
    persist_response_ledger, ResponseEventPersistenceDisposition, ResponseEventPersistenceError,
};
use psychometrics_commons_runtime::response::{ResponseEvent, ResponseLedger, ResponseWrite};
use psychometrics_commons_runtime::session::SessionState;
use std::sync::{Mutex, MutexGuard};

const DIGEST_A: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const DIGEST_B: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const DIGEST_C: &str = "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const OBSERVED_OPENNESS_MS: u64 = 1_700_000_000_000;
const RECEIVED_OPENNESS_MS: u64 = 1_700_000_000_250;
const OBSERVED_CONSCIENTIOUSNESS_MS: u64 = 1_700_000_030_000;
const RECEIVED_CONSCIENTIOUSNESS_MS: u64 = 1_700_000_030_400;
const OBSERVED_EXTRAVERSION_MS: u64 = 1_700_000_060_000;
const RECEIVED_EXTRAVERSION_MS: u64 = 1_700_000_060_350;

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

fn two_item_korean_ledger() -> ResponseLedger {
    let mut ledger = ResponseLedger::new("session_big_five_ko").unwrap();
    ledger
        .record(
            SessionState::Active,
            write(
                "response_event_openness",
                "client_openness",
                "item_version_openness_ko",
                DIGEST_A,
            ),
        )
        .unwrap();
    ledger
        .record(
            SessionState::Active,
            write(
                "response_event_conscientiousness",
                "client_conscientiousness",
                "item_version_conscientiousness_ko",
                DIGEST_B,
            ),
        )
        .unwrap();
    ledger
}

fn event_times() -> [(u64, u64); 2] {
    [
        (OBSERVED_OPENNESS_MS, RECEIVED_OPENNESS_MS),
        (OBSERVED_CONSCIENTIOUSNESS_MS, RECEIVED_CONSCIENTIOUSNESS_MS),
    ]
}

fn persist_ok(
    client: &mut Client,
    ledger: &ResponseLedger,
    event_times: &[(u64, u64)],
) -> ResponseEventPersistenceDisposition {
    let mut transaction = client.transaction().unwrap();
    let disposition = persist_response_ledger(&mut transaction, ledger, event_times).unwrap();
    transaction.commit().unwrap();
    disposition
}

fn persist_err(
    client: &mut Client,
    ledger: &ResponseLedger,
    event_times: &[(u64, u64)],
) -> ResponseEventPersistenceError {
    let mut transaction = client.transaction().unwrap();
    let error = persist_response_ledger(&mut transaction, ledger, event_times).unwrap_err();
    transaction.rollback().unwrap();
    error
}

fn load_ok(client: &mut Client, session_ref: &str) -> Option<ResponseLedger> {
    let mut transaction = client.transaction().unwrap();
    let ledger = load_response_ledger(&mut transaction, session_ref).unwrap();
    transaction.commit().unwrap();
    ledger
}

#[test]
fn two_item_korean_path_survives_restart_with_exact_replay() {
    let _guard = response_event_test_guard();
    let mut client = test_client();
    reset_response_event_table(&mut client);
    apply_response_event_migration(&mut client).unwrap();

    let live = two_item_korean_ledger();
    assert_eq!(
        persist_ok(&mut client, &live, &event_times()),
        ResponseEventPersistenceDisposition::Inserted
    );
    assert_eq!(
        persist_ok(&mut client, &live, &event_times()),
        ResponseEventPersistenceDisposition::Duplicate
    );

    let reloaded = load_ok(&mut client, "session_big_five_ko").unwrap();
    assert_eq!(reloaded, live);
    assert_eq!(
        reloaded.events()[1].item_version_ref(),
        "item_version_conscientiousness_ko"
    );
    assert!(load_ok(&mut client, "session_missing_ko").is_none());
    let mut times_transaction = client.transaction().unwrap();
    assert_eq!(
        load_response_event_times(&mut times_transaction, "session_big_five_ko").unwrap(),
        Some(event_times().to_vec())
    );
    assert!(
        load_response_event_times(&mut times_transaction, "session_missing_ko")
            .unwrap()
            .is_none()
    );
    times_transaction.commit().unwrap();
}

#[test]
fn reloaded_korean_path_records_item_three_and_keeps_the_scoring_prefix() {
    let _guard = response_event_test_guard();
    let mut client = test_client();
    reset_response_event_table(&mut client);
    apply_response_event_migration(&mut client).unwrap();
    persist_ok(&mut client, &two_item_korean_ledger(), &event_times());

    let mut continued = load_ok(&mut client, "session_big_five_ko").unwrap();
    continued
        .record(
            SessionState::Active,
            write(
                "response_event_extraversion",
                "client_extraversion",
                "item_version_extraversion_ko",
                DIGEST_C,
            ),
        )
        .unwrap();
    assert_eq!(
        persist_ok(
            &mut client,
            &continued,
            &[
                (OBSERVED_OPENNESS_MS, RECEIVED_OPENNESS_MS),
                (OBSERVED_CONSCIENTIOUSNESS_MS, RECEIVED_CONSCIENTIOUSNESS_MS),
                (OBSERVED_EXTRAVERSION_MS, RECEIVED_EXTRAVERSION_MS),
            ]
        ),
        ResponseEventPersistenceDisposition::Inserted
    );

    let reloaded = load_ok(&mut client, "session_big_five_ko").unwrap();
    assert_eq!(reloaded, continued);
    assert_eq!(reloaded.events()[2].sequence(), 3);
    assert_eq!(
        reloaded.events()[2].item_version_ref(),
        "item_version_extraversion_ko"
    );
    assert_eq!(
        reloaded
            .freeze_as(SessionState::Completed, "response_snapshot_big_five_ko")
            .unwrap()
            .event_refs(),
        [
            "response_event_openness",
            "response_event_conscientiousness",
            "response_event_extraversion"
        ]
    );
}

#[test]
fn client_rebinding_and_sequence_reuse_fail_closed() {
    let _guard = response_event_test_guard();
    let mut client = test_client();
    reset_response_event_table(&mut client);
    apply_response_event_migration(&mut client).unwrap();
    persist_ok(&mut client, &two_item_korean_ledger(), &event_times());

    let mut rebound_client = ResponseLedger::new("session_big_five_ko").unwrap();
    rebound_client
        .record(
            SessionState::Active,
            write(
                "response_event_openness",
                "client_openness",
                "item_version_openness_ko",
                DIGEST_B,
            ),
        )
        .unwrap();
    assert!(matches!(
        persist_err(
            &mut client,
            &rebound_client,
            &[(OBSERVED_OPENNESS_MS, RECEIVED_OPENNESS_MS)]
        ),
        ResponseEventPersistenceError::ConflictingReplay
    ));

    let mut reused_sequence = ResponseLedger::new("session_big_five_ko").unwrap();
    reused_sequence
        .record(
            SessionState::Active,
            write(
                "response_event_extraversion",
                "client_extraversion",
                "item_version_extraversion_ko",
                DIGEST_A,
            ),
        )
        .unwrap();
    assert!(matches!(
        persist_err(
            &mut client,
            &reused_sequence,
            &[(OBSERVED_OPENNESS_MS, RECEIVED_OPENNESS_MS)]
        ),
        ResponseEventPersistenceError::SequenceConflict
    ));
}

#[test]
fn inverted_time_blank_session_and_repeatable_read_fail_closed() {
    let _guard = response_event_test_guard();
    let mut client = test_client();
    reset_response_event_table(&mut client);
    apply_response_event_migration(&mut client).unwrap();
    let live = two_item_korean_ledger();

    assert!(matches!(
        persist_err(&mut client, &live, &[(1, 2)]),
        ResponseEventPersistenceError::InvalidEventTimeArity
    ));
    assert!(matches!(
        persist_err(
            &mut client,
            &live,
            &[
                (RECEIVED_OPENNESS_MS, OBSERVED_OPENNESS_MS),
                (OBSERVED_CONSCIENTIOUSNESS_MS, RECEIVED_CONSCIENTIOUSNESS_MS)
            ]
        ),
        ResponseEventPersistenceError::InvalidTimestamp
    ));
    assert!(matches!(
        persist_err(
            &mut client,
            &live,
            &[
                (0, RECEIVED_OPENNESS_MS),
                (OBSERVED_CONSCIENTIOUSNESS_MS, RECEIVED_CONSCIENTIOUSNESS_MS)
            ]
        ),
        ResponseEventPersistenceError::InvalidTimestamp
    ));

    let mut transaction = client
        .build_transaction()
        .isolation_level(IsolationLevel::RepeatableRead)
        .start()
        .unwrap();
    assert!(matches!(
        persist_response_ledger(&mut transaction, &live, &event_times()),
        Err(ResponseEventPersistenceError::UnsupportedIsolationLevel)
    ));
    transaction.rollback().unwrap();

    let mut load_transaction = client.transaction().unwrap();
    assert!(matches!(
        load_response_ledger(&mut load_transaction, "12"),
        Err(ResponseEventPersistenceError::InvalidReference)
    ));
    load_transaction.rollback().unwrap();
}

#[test]
fn empty_ledger_persist_is_duplicate_and_gapped_store_fails_closed() {
    let _guard = response_event_test_guard();
    let mut client = test_client();
    reset_response_event_table(&mut client);
    apply_response_event_migration(&mut client).unwrap();

    let empty = ResponseLedger::new("session_empty_ko").unwrap();
    assert_eq!(
        persist_ok(&mut client, &empty, &[]),
        ResponseEventPersistenceDisposition::Duplicate
    );
    assert!(load_ok(&mut client, "session_empty_ko").is_none());

    persist_ok(&mut client, &two_item_korean_ledger(), &event_times());
    client
        .execute(
            "DELETE FROM response_event WHERE response_event_ref = 'response_event_openness'",
            &[],
        )
        .unwrap();
    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        load_response_ledger(&mut transaction, "session_big_five_ko"),
        Err(ResponseEventPersistenceError::InvalidStoredIdentity)
    ));
    transaction.rollback().unwrap();
}

#[test]
fn server_event_rebinding_to_another_session_fails_closed() {
    let _guard = response_event_test_guard();
    let mut client = test_client();
    reset_response_event_table(&mut client);
    apply_response_event_migration(&mut client).unwrap();
    persist_ok(&mut client, &two_item_korean_ledger(), &event_times());

    let mut other_session = ResponseLedger::new("session_big_five_en").unwrap();
    other_session
        .record(
            SessionState::Active,
            write(
                "response_event_openness",
                "client_openness_en",
                "item_version_openness_en",
                DIGEST_A,
            ),
        )
        .unwrap();
    assert!(matches!(
        persist_err(
            &mut client,
            &other_session,
            &[(OBSERVED_OPENNESS_MS, RECEIVED_OPENNESS_MS)]
        ),
        ResponseEventPersistenceError::ConflictingReplay
    ));

    let rebound_server = ResponseLedger::from_persisted(
        "session_big_five_ko",
        vec![ResponseEvent::from_persisted(
            "response_event_other_server",
            "client_openness",
            "item_version_openness_ko",
            DIGEST_A,
            1,
        )
        .unwrap()],
    )
    .unwrap();
    assert!(matches!(
        persist_err(
            &mut client,
            &rebound_server,
            &[(OBSERVED_OPENNESS_MS, RECEIVED_OPENNESS_MS)]
        ),
        ResponseEventPersistenceError::ConflictingReplay
            | ResponseEventPersistenceError::SequenceConflict
    ));
}
