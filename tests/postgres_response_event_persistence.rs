//! Real `PostgreSQL` contract for durable mid-session response events.

use postgres::{Client, IsolationLevel, NoTls};
use psychometrics_commons_runtime::postgres_response_event::{
    apply_response_event_migration, load_response_event_receipts, load_response_ledger,
    persist_response_event, ResponseEventPersistenceDisposition, ResponseEventPersistenceError,
    ResponseEventReceipt,
};
#[path = "response_support/mod.rs"]
mod response_support;

use response_support::{active_session, completed_session};

use psychometrics_commons_runtime::response::{
    ResponseEvent, ResponseLedger, ResponseWrite, WriteError,
};
use psychometrics_commons_runtime::scoring::{ScoringRequest, ScoringRequestInput};
use std::sync::{Mutex, MutexGuard};

const DIGEST_N1: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const DIGEST_N2: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const OBSERVED_AT_MS: u64 = 1_700_000_000_000;
const RECEIVED_AT_MS: u64 = 1_700_000_000_250;

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
    let session = active_session(session_ref);
    let mut ledger = ResponseLedger::from_session(&session).unwrap();
    let event = ledger.record(&session, request).unwrap();
    (ledger, event)
}

fn persist_ok(
    client: &mut Client,
    session_ref: &str,
    event: &ResponseEvent,
) -> ResponseEventPersistenceDisposition {
    persist_ok_at(client, session_ref, event, OBSERVED_AT_MS, RECEIVED_AT_MS)
}

fn persist_ok_at(
    client: &mut Client,
    session_ref: &str,
    event: &ResponseEvent,
    observed_at_unix_ms: u64,
    received_at_unix_ms: u64,
) -> ResponseEventPersistenceDisposition {
    let mut transaction = client.transaction().unwrap();
    let disposition = persist_response_event(
        &mut transaction,
        session_ref,
        event,
        observed_at_unix_ms,
        received_at_unix_ms,
    )
    .unwrap();
    transaction.commit().unwrap();
    disposition
}

fn persist_err(
    client: &mut Client,
    session_ref: &str,
    event: &ResponseEvent,
) -> ResponseEventPersistenceError {
    persist_err_at(client, session_ref, event, OBSERVED_AT_MS, RECEIVED_AT_MS)
}

fn persist_err_at(
    client: &mut Client,
    session_ref: &str,
    event: &ResponseEvent,
    observed_at_unix_ms: u64,
    received_at_unix_ms: u64,
) -> ResponseEventPersistenceError {
    let mut transaction = client.transaction().unwrap();
    let error = persist_response_event(
        &mut transaction,
        session_ref,
        event,
        observed_at_unix_ms,
        received_at_unix_ms,
    )
    .unwrap_err();
    transaction.rollback().unwrap();
    error
}

fn load_ok(client: &mut Client, session_ref: &str) -> ResponseLedger {
    let mut transaction = client.transaction().unwrap();
    let ledger = load_response_ledger(&mut transaction, session_ref).unwrap();
    transaction.commit().unwrap();
    ledger
}

fn load_receipts_ok(client: &mut Client, session_ref: &str) -> Vec<ResponseEventReceipt> {
    let mut transaction = client.transaction().unwrap();
    let receipts = load_response_event_receipts(&mut transaction, session_ref).unwrap();
    transaction.commit().unwrap();
    receipts
}

fn load_err(client: &mut Client, session_ref: &str) -> ResponseEventPersistenceError {
    let mut transaction = client.transaction().unwrap();
    let error = load_response_ledger(&mut transaction, session_ref).unwrap_err();
    transaction.rollback().unwrap();
    error
}

fn load_receipts_err(client: &mut Client, session_ref: &str) -> ResponseEventPersistenceError {
    let mut transaction = client.transaction().unwrap();
    let error = load_response_event_receipts(&mut transaction, session_ref).unwrap_err();
    transaction.rollback().unwrap();
    error
}

fn scoring_input<'a>() -> ScoringRequestInput<'a> {
    ScoringRequestInput {
        scoring_request_ref: "scoring_request_ipip_ko_quick",
        response_snapshot_ref: "response_snapshot_ipip_ko_quick",
        assessment_spec_ref: "assessment_spec_ipip_bf_ko_quick",
        instrument_version_ref: "instrument_version_ipip_bf_ko_quick",
        scoring_version_ref: "scoring_version_ipip_mlsirm_v1",
        calibration_reference: "calibration_ipip_bf_ko_quick",
        norm_version_ref: Some("norm_ipip_bf_ko_quick"),
        requested_output_schema_version: 1,
    }
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

    let session_control = active_session("session_ipip_ko_quick");
    let mut control = ResponseLedger::from_session(&session_control).unwrap();
    let control_first = control
        .record(
            &session_control,
            write(
                "server_event_item_01",
                "client_event_item_01",
                "item_version_n1_ko",
                DIGEST_N1,
            ),
        )
        .unwrap();
    control
        .record(
            &session_control,
            write(
                "server_event_item_02",
                "client_event_item_02",
                "item_version_n2_ko",
                DIGEST_N2,
            ),
        )
        .unwrap();
    let expected_snapshot = control
        .freeze_as(
            &completed_session("session_ipip_ko_quick"),
            "response_snapshot_ipip_ko_quick",
        )
        .unwrap();
    let expected_request =
        ScoringRequest::from_snapshot(&expected_snapshot, scoring_input()).unwrap();

    let session_first_only = active_session("session_ipip_ko_quick");
    let mut first_only = ResponseLedger::from_session(&session_first_only).unwrap();
    let first = first_only
        .record(
            &session_first_only,
            write(
                "server_event_item_01",
                "client_event_item_01",
                "item_version_n1_ko",
                DIGEST_N1,
            ),
        )
        .unwrap();
    assert_eq!(first, control_first);
    assert_eq!(
        persist_ok(&mut client, "session_ipip_ko_quick", &first),
        ResponseEventPersistenceDisposition::Inserted
    );
    assert_eq!(
        persist_ok(&mut client, "session_ipip_ko_quick", &first),
        ResponseEventPersistenceDisposition::Duplicate
    );

    let mut after_restart = load_ok(&mut client, "session_ipip_ko_quick");
    assert_eq!(after_restart.events(), std::slice::from_ref(&first));
    let first_receipts = load_receipts_ok(&mut client, "session_ipip_ko_quick");
    assert_eq!(first_receipts.len(), 1);
    assert_eq!(first_receipts[0].event(), &first);
    assert_eq!(first_receipts[0].observed_at_unix_ms(), OBSERVED_AT_MS);
    assert_eq!(first_receipts[0].received_at_unix_ms(), RECEIVED_AT_MS);

    let second = after_restart
        .record(
            &session_control,
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
    assert_eq!(rebuilt, after_restart);
    assert_eq!(rebuilt, control);
    let snapshot = rebuilt
        .freeze_as(
            &completed_session("session_ipip_ko_quick"),
            "response_snapshot_ipip_ko_quick",
        )
        .unwrap();
    let request = ScoringRequest::from_snapshot(&snapshot, scoring_input()).unwrap();
    assert_eq!(snapshot, expected_snapshot);
    assert_eq!(request, expected_request);
    assert_eq!(snapshot.event_count(), 2);
    assert_eq!(snapshot.last_sequence(), Some(2));
}

#[test]
fn persisted_replay_rejects_padded_references_and_replays_exact_ones() {
    let _guard = response_event_test_guard();
    let mut client = test_client();
    reset_response_event_table(&mut client);
    apply_response_event_migration(&mut client).unwrap();

    let session_ledger = active_session("session_ipip_ko_alias");
    let mut ledger = ResponseLedger::from_session(&session_ledger).unwrap();

    // The exact-reference contract rejects padded aliases outright instead of
    // silently canonicalizing them, so the padded first offer fails closed.
    assert_eq!(
        ledger
            .record(
                &session_ledger,
                write(
                    " server_event_item_01 ",
                    " client_event_item_01 ",
                    " item_version_n1_ko ",
                    DIGEST_N1,
                ),
            )
            .unwrap_err(),
        WriteError::InvalidReference
    );

    // The exact spelling is accepted and persists as the inserted event.
    let event = ledger
        .record(
            &session_ledger,
            write(
                "server_event_item_01",
                "client_event_item_01",
                "item_version_n1_ko",
                DIGEST_N1,
            ),
        )
        .unwrap();
    assert_eq!(event.client_event_ref(), "client_event_item_01");
    assert_eq!(event.item_version_ref(), "item_version_n1_ko");
    assert_eq!(
        persist_ok(&mut client, "session_ipip_ko_alias", &event),
        ResponseEventPersistenceDisposition::Inserted
    );

    // A replay that supplies a different server reference with otherwise exact
    // content stays idempotent and persists as a duplicate.
    let replay = ledger
        .record(
            &session_ledger,
            write(
                "ignored_server_event_ref",
                "client_event_item_01",
                "item_version_n1_ko",
                DIGEST_N1,
            ),
        )
        .unwrap();
    assert_eq!(replay, event);
    assert_eq!(
        persist_ok(&mut client, "session_ipip_ko_alias", &replay),
        ResponseEventPersistenceDisposition::Duplicate
    );
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
fn reload_keeps_neighbor_session_prefixes_isolated() {
    let _guard = response_event_test_guard();
    let mut client = test_client();
    reset_response_event_table(&mut client);
    apply_response_event_migration(&mut client).unwrap();

    let (_, alpha) = recorded_event(
        "session_ipip_ko_alpha",
        write(
            "server_event_alpha_01",
            "client_event_alpha_01",
            "item_version_n1_ko",
            DIGEST_N1,
        ),
    );
    let (_, beta) = recorded_event(
        "session_ipip_ko_beta",
        write(
            "server_event_beta_01",
            "client_event_beta_01",
            "item_version_n1_ko",
            DIGEST_N2,
        ),
    );
    persist_ok(&mut client, "session_ipip_ko_alpha", &alpha);
    persist_ok(&mut client, "session_ipip_ko_beta", &beta);

    let loaded_alpha = load_ok(&mut client, "session_ipip_ko_alpha");
    let loaded_beta = load_ok(&mut client, "session_ipip_ko_beta");
    assert_eq!(loaded_alpha.events(), std::slice::from_ref(&alpha));
    assert_eq!(loaded_beta.events(), std::slice::from_ref(&beta));
    assert_eq!(
        loaded_alpha.events()[0].server_event_ref(),
        "server_event_alpha_01"
    );
    assert_eq!(
        loaded_beta.events()[0].server_event_ref(),
        "server_event_beta_01"
    );
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
    let colliding = ResponseEvent::from_persisted(
        "server_event_item_02",
        "client_event_item_02",
        "item_version_n2_ko",
        DIGEST_N2,
        1,
    )
    .unwrap();
    assert!(matches!(
        persist_err(&mut client, "session_ipip_ko_sequence", &colliding),
        ResponseEventPersistenceError::SequenceConflict
    ));
    let rebuilt = load_ok(&mut client, "session_ipip_ko_sequence");
    assert_eq!(rebuilt.events(), std::slice::from_ref(&first));
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
        persist_response_event(
            &mut serializable,
            "session_ipip_ko_isolation",
            &event,
            OBSERVED_AT_MS,
            RECEIVED_AT_MS,
        ),
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
    assert!(matches!(
        load_receipts_err(&mut client, "session_ipip_ko_missing"),
        ResponseEventPersistenceError::Database(_)
    ));

    apply_response_event_migration(&mut client).unwrap();
    persist_ok(&mut client, "session_ipip_ko_gap", &event);
    client
        .execute(
            "INSERT INTO response_event (\
                 response_event_ref, session_ref, client_event_ref, item_version_ref, \
                 payload_digest, server_sequence, observed_at, received_at\
             ) VALUES (\
                 'server_event_item_03', 'session_ipip_ko_gap', 'client_event_item_03', \
                 'item_version_n3_ko', $1, 3, TIMESTAMPTZ '2023-11-14 22:13:20+00', \
                 TIMESTAMPTZ '2023-11-14 22:13:20.250+00'\
             )",
            &[&DIGEST_N2],
        )
        .unwrap();
    assert!(matches!(
        load_err(&mut client, "session_ipip_ko_gap"),
        ResponseEventPersistenceError::InvalidSequence
    ));
    assert!(matches!(
        load_receipts_err(&mut client, "session_ipip_ko_gap"),
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
                 payload_digest, server_sequence, observed_at, received_at\
             ) VALUES (\
                 'server_event_item_01', 'session_ipip_ko_digest', 'client_event_item_01', \
                 'item_version_n1_ko', 'not-a-digest', 1, \
                 TIMESTAMPTZ '2023-11-14 22:13:20+00', \
                 TIMESTAMPTZ '2023-11-14 22:13:20.250+00'\
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

    let session_ledger = active_session("session_ipip_ko_extra");
    let mut ledger = ResponseLedger::from_session(&session_ledger).unwrap();
    let first = ledger
        .record(
            &session_ledger,
            write(
                "server_event_item_01",
                "client_event_item_01",
                "item_version_n1_ko",
                DIGEST_N1,
            ),
        )
        .unwrap();
    persist_ok(&mut client, "session_ipip_ko_extra", &first);
    client
        .batch_execute(
            "CREATE UNIQUE INDEX response_event_session_only_unique ON response_event (session_ref);",
        )
        .unwrap();
    let second = ledger
        .record(
            &session_ledger,
            write(
                "server_event_item_02",
                "client_event_item_02",
                "item_version_n2_ko",
                DIGEST_N2,
            ),
        )
        .unwrap();
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
                 payload_digest, server_sequence, observed_at, received_at\
             ) VALUES (\
                 'server_event_item_neg', 'session_ipip_ko_negative', 'client_event_item_neg', \
                 'item_version_n_neg', $1, -1, \
                 TIMESTAMPTZ '2023-11-14 22:13:20+00', \
                 TIMESTAMPTZ '2023-11-14 22:13:20.250+00'\
             )",
            &[&DIGEST_N1],
        )
        .unwrap();
    assert!(matches!(
        load_err(&mut client, "session_ipip_ko_negative"),
        ResponseEventPersistenceError::InvalidSequence
    ));
}

#[test]
fn inverted_or_zero_event_times_and_time_rebinding_fail_closed() {
    let _guard = response_event_test_guard();
    let mut client = test_client();
    reset_response_event_table(&mut client);
    apply_response_event_migration(&mut client).unwrap();

    let (_, event) = recorded_event(
        "session_ipip_ko_time",
        write(
            "server_event_item_01",
            "client_event_item_01",
            "item_version_n1_ko",
            DIGEST_N1,
        ),
    );
    assert!(matches!(
        persist_err_at(
            &mut client,
            "session_ipip_ko_time",
            &event,
            0,
            RECEIVED_AT_MS
        ),
        ResponseEventPersistenceError::InvalidTimestamp
    ));
    assert!(matches!(
        persist_err_at(
            &mut client,
            "session_ipip_ko_time",
            &event,
            RECEIVED_AT_MS + 1,
            RECEIVED_AT_MS
        ),
        ResponseEventPersistenceError::InvalidTimestamp
    ));
    persist_ok(&mut client, "session_ipip_ko_time", &event);
    assert!(matches!(
        persist_err_at(
            &mut client,
            "session_ipip_ko_time",
            &event,
            OBSERVED_AT_MS + 1,
            RECEIVED_AT_MS
        ),
        ResponseEventPersistenceError::ConflictingReplay
    ));
    assert!(matches!(
        persist_err_at(
            &mut client,
            "session_ipip_ko_time",
            &event,
            OBSERVED_AT_MS,
            RECEIVED_AT_MS + 1
        ),
        ResponseEventPersistenceError::ConflictingReplay
    ));

    client
        .batch_execute(
            "ALTER TABLE response_event DROP CONSTRAINT response_event_observed_not_after_received_check;",
        )
        .unwrap();
    client
        .execute(
            "INSERT INTO response_event (\
                 response_event_ref, session_ref, client_event_ref, item_version_ref, \
                 payload_digest, server_sequence, observed_at, received_at\
             ) VALUES (\
                 'server_event_item_inverted', 'session_ipip_ko_inverted', \
                 'client_event_item_inverted', 'item_version_n_inverted', $1, 1, \
                 TIMESTAMPTZ '2023-11-14 22:13:21+00', \
                 TIMESTAMPTZ '2023-11-14 22:13:20+00'\
             )",
            &[&DIGEST_N1],
        )
        .unwrap();
    assert!(matches!(
        load_err(&mut client, "session_ipip_ko_inverted"),
        ResponseEventPersistenceError::InvalidTimestamp
    ));
    client
        .execute(
            "INSERT INTO response_event (\
                 response_event_ref, session_ref, client_event_ref, item_version_ref, \
                 payload_digest, server_sequence, observed_at, received_at\
             ) VALUES (\
                 'server_event_item_epoch', 'session_ipip_ko_epoch', \
                 'client_event_item_epoch', 'item_version_n_epoch', $1, 1, \
                 TIMESTAMPTZ '1970-01-01 00:00:00+00', \
                 TIMESTAMPTZ '1970-01-01 00:00:00+00'\
             )",
            &[&DIGEST_N1],
        )
        .unwrap();
    assert!(matches!(
        load_err(&mut client, "session_ipip_ko_epoch"),
        ResponseEventPersistenceError::InvalidTimestamp
    ));
}
