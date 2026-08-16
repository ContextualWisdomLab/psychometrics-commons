//! Real `PostgreSQL` contract: a frozen response prefix survives process restart.
//!
//! After a buyer completes a two-item Korean Big Five path, scoring must still
//! dispatch from the exact stored prefix. Reload keeps server order even when a
//! later event identity sorts first, and it fails closed on header/entry lies,
//! stronger isolation, and missing relations.

use postgres::{Client, IsolationLevel, NoTls};
use psychometrics_commons_runtime::postgres_response_snapshot::{
    apply_response_snapshot_migration, load_response_snapshot, load_response_snapshot_for_session,
    persist_response_snapshot, ResponseSnapshotPersistenceDisposition,
    ResponseSnapshotPersistenceError,
};
use psychometrics_commons_runtime::response::{ResponseLedger, ResponseWrite};
use psychometrics_commons_runtime::scoring::{ScoringRequest, ScoringRequestInput};
use psychometrics_commons_runtime::session::SessionState;
use std::sync::{Mutex, MutexGuard};

const PAYLOAD_DIGEST: &str =
    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const OTHER_DIGEST: &str =
    "sha256:fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210";

static RESPONSE_SNAPSHOT_RELOAD_LOCK: Mutex<()> = Mutex::new(());

fn reload_guard() -> MutexGuard<'static, ()> {
    RESPONSE_SNAPSHOT_RELOAD_LOCK
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
            "CREATE SCHEMA IF NOT EXISTS response_snapshot_reload_test;\
             SET search_path TO response_snapshot_reload_test;",
        )
        .unwrap();
    client
}

fn reset_tables(client: &mut Client) {
    client
        .batch_execute(
            "DROP TABLE IF EXISTS response_snapshot_reload_test.response_snapshot_entry;\
             DROP TABLE IF EXISTS response_snapshot_reload_test.response_snapshot;",
        )
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

fn frozen_snapshot(
    session_ref: &str,
    snapshot_ref: &str,
    writes: &[ResponseWrite<'_>],
) -> psychometrics_commons_runtime::response::ResponseSnapshot {
    let mut ledger = ResponseLedger::new(session_ref).unwrap();
    for request in writes {
        ledger.record(SessionState::Active, *request).unwrap();
    }
    ledger
        .freeze_as(SessionState::Completed, snapshot_ref)
        .unwrap()
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

fn load_ok(
    client: &mut Client,
    snapshot_ref: &str,
) -> Option<psychometrics_commons_runtime::response::ResponseSnapshot> {
    let mut transaction = client.transaction().unwrap();
    let loaded = load_response_snapshot(&mut transaction, snapshot_ref).unwrap();
    transaction.commit().unwrap();
    loaded
}

#[test]
fn unknown_snapshot_reload_is_absent() {
    let _guard = reload_guard();
    let mut client = test_client();
    reset_tables(&mut client);
    apply_response_snapshot_migration(&mut client).unwrap();

    assert!(
        load_ok(&mut client, "response_snapshot_reload_unknown").is_none(),
        "a snapshot that was never persisted must not appear after restart"
    );
    let mut transaction = client.transaction().unwrap();
    assert!(
        load_response_snapshot_for_session(&mut transaction, "session_reload_unknown")
            .unwrap()
            .is_none(),
        "a session that never froze a snapshot must not appear after restart"
    );
    transaction.commit().unwrap();
}

#[test]
fn empty_completed_snapshot_reloads_without_inventing_events() {
    let _guard = reload_guard();
    let mut client = test_client();
    reset_tables(&mut client);
    apply_response_snapshot_migration(&mut client).unwrap();

    let empty = frozen_snapshot(
        "session_reload_empty",
        "response_snapshot_reload_empty",
        &[],
    );
    persist_ok(&mut client, &empty);
    let loaded = load_ok(&mut client, "response_snapshot_reload_empty")
        .expect("an empty completed snapshot must reload");
    assert_eq!(loaded, empty);
    assert_eq!(loaded.event_count(), 0);
}

#[test]
fn two_item_completed_prefix_reloads_in_server_order_and_stays_scoreable() {
    let _guard = reload_guard();
    let mut client = test_client();
    reset_tables(&mut client);
    apply_response_snapshot_migration(&mut client).unwrap();

    let snapshot = frozen_snapshot(
        "session_reload_alpha",
        "response_snapshot_reload_alpha",
        &[
            write(
                "server_event_zzz_first",
                "client_event_001",
                "item_version_001",
                PAYLOAD_DIGEST,
            ),
            write(
                "server_event_aaa_second",
                "client_event_002",
                "item_version_002",
                OTHER_DIGEST,
            ),
        ],
    );
    persist_ok(&mut client, &snapshot);

    let loaded = load_ok(&mut client, "response_snapshot_reload_alpha")
        .expect("a completed two-item snapshot must reload after restart");
    assert_eq!(loaded, snapshot);
    assert_eq!(
        loaded.event_refs(),
        ["server_event_zzz_first", "server_event_aaa_second"]
    );

    let mut transaction = client.transaction().unwrap();
    let by_session = load_response_snapshot_for_session(&mut transaction, "session_reload_alpha")
        .unwrap()
        .expect("session lookup must find the unique frozen snapshot");
    transaction.commit().unwrap();
    assert_eq!(by_session, loaded);

    let request = ScoringRequest::from_snapshot(
        &loaded,
        ScoringRequestInput {
            scoring_request_ref: "scoring_request_reload_alpha",
            response_snapshot_ref: "response_snapshot_reload_alpha",
            assessment_spec_ref: "assessment_spec_reload_alpha",
            instrument_version_ref: "instrument_version_reload_alpha",
            scoring_version_ref: "scoring_version_reload_alpha",
            calibration_reference: "calibration_reload_alpha",
            norm_version_ref: Some("norm_version_reload_alpha"),
            requested_output_schema_version: 1,
        },
    )
    .expect("reloaded completed evidence must remain the scoring input");
    assert_eq!(
        request.response_snapshot_ref(),
        "response_snapshot_reload_alpha"
    );
}

#[test]
fn header_entry_mismatch_and_gapped_sequences_fail_closed() {
    let _guard = reload_guard();
    let mut client = test_client();
    reset_tables(&mut client);
    apply_response_snapshot_migration(&mut client).unwrap();

    persist_ok(
        &mut client,
        &frozen_snapshot(
            "session_reload_corrupt",
            "response_snapshot_reload_corrupt",
            &[write(
                "server_event_001",
                "client_event_001",
                "item_version_001",
                PAYLOAD_DIGEST,
            )],
        ),
    );
    client
        .execute(
            "UPDATE response_snapshot SET event_count = 99 \
             WHERE snapshot_ref = 'response_snapshot_reload_corrupt'",
            &[],
        )
        .unwrap();
    let mut mismatched = client.transaction().unwrap();
    assert!(matches!(
        load_response_snapshot(&mut mismatched, "response_snapshot_reload_corrupt"),
        Err(ResponseSnapshotPersistenceError::CorruptHistory)
    ));
    mismatched.rollback().unwrap();

    client
        .execute(
            "UPDATE response_snapshot SET event_count = 1, last_sequence = 99 \
             WHERE snapshot_ref = 'response_snapshot_reload_corrupt'",
            &[],
        )
        .unwrap();
    let mut sequence_lie = client.transaction().unwrap();
    assert!(matches!(
        load_response_snapshot(&mut sequence_lie, "response_snapshot_reload_corrupt"),
        Err(ResponseSnapshotPersistenceError::CorruptHistory)
    ));
    sequence_lie.rollback().unwrap();

    client
        .execute(
            "UPDATE response_snapshot SET last_sequence = 1 \
             WHERE snapshot_ref = 'response_snapshot_reload_corrupt'",
            &[],
        )
        .unwrap();
    client
        .execute(
            "UPDATE response_snapshot_entry SET snapshot_sequence = 3 \
             WHERE snapshot_ref = 'response_snapshot_reload_corrupt'",
            &[],
        )
        .unwrap();
    let mut gapped = client.transaction().unwrap();
    assert!(matches!(
        load_response_snapshot(&mut gapped, "response_snapshot_reload_corrupt"),
        Err(ResponseSnapshotPersistenceError::CorruptHistory)
    ));
    gapped.rollback().unwrap();
}

#[test]
fn stored_noncanonical_digest_fails_closed_on_reload() {
    let _guard = reload_guard();
    let mut client = test_client();
    reset_tables(&mut client);
    apply_response_snapshot_migration(&mut client).unwrap();

    persist_ok(
        &mut client,
        &frozen_snapshot(
            "session_reload_digest",
            "response_snapshot_reload_digest",
            &[write(
                "server_event_001",
                "client_event_001",
                "item_version_001",
                PAYLOAD_DIGEST,
            )],
        ),
    );
    client
        .execute(
            "UPDATE response_snapshot_entry SET payload_digest = 'sha256:not-a-digest' \
             WHERE snapshot_ref = 'response_snapshot_reload_digest'",
            &[],
        )
        .unwrap();
    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        load_response_snapshot(&mut transaction, "response_snapshot_reload_digest"),
        Err(ResponseSnapshotPersistenceError::CorruptHistory)
    ));
    transaction.rollback().unwrap();
}

#[test]
fn response_snapshot_reload_requires_read_committed_and_rejects_blank_aliases() {
    let _guard = reload_guard();
    let mut client = test_client();
    reset_tables(&mut client);
    apply_response_snapshot_migration(&mut client).unwrap();

    let mut serializable = client
        .build_transaction()
        .isolation_level(IsolationLevel::Serializable)
        .start()
        .unwrap();
    assert!(matches!(
        load_response_snapshot(&mut serializable, "response_snapshot_reload_alpha"),
        Err(ResponseSnapshotPersistenceError::UnsupportedIsolationLevel)
    ));
    serializable.rollback().unwrap();

    let mut repeatable = client
        .build_transaction()
        .isolation_level(IsolationLevel::RepeatableRead)
        .start()
        .unwrap();
    assert!(matches!(
        load_response_snapshot_for_session(&mut repeatable, "session_reload_alpha"),
        Err(ResponseSnapshotPersistenceError::UnsupportedIsolationLevel)
    ));
    repeatable.rollback().unwrap();

    let mut transaction = client.transaction().unwrap();
    for invalid_ref in ["", " ", "42"] {
        assert!(matches!(
            load_response_snapshot(&mut transaction, invalid_ref),
            Err(ResponseSnapshotPersistenceError::InvalidReference)
        ));
        assert!(matches!(
            load_response_snapshot_for_session(&mut transaction, invalid_ref),
            Err(ResponseSnapshotPersistenceError::InvalidReference)
        ));
    }
    transaction.rollback().unwrap();
}

#[test]
fn missing_response_snapshot_relations_fail_closed_on_reload() {
    let _guard = reload_guard();
    let mut client = test_client();
    reset_tables(&mut client);
    apply_response_snapshot_migration(&mut client).unwrap();
    persist_ok(
        &mut client,
        &frozen_snapshot(
            "session_reload_missing",
            "response_snapshot_reload_missing",
            &[],
        ),
    );

    client
        .batch_execute("DROP TABLE response_snapshot_entry;")
        .unwrap();
    let mut missing_entries = client.transaction().unwrap();
    assert!(matches!(
        load_response_snapshot(&mut missing_entries, "response_snapshot_reload_missing"),
        Err(ResponseSnapshotPersistenceError::Database(_))
    ));
    missing_entries.rollback().unwrap();

    client
        .batch_execute("DROP TABLE response_snapshot;")
        .unwrap();
    let mut missing_header = client.transaction().unwrap();
    assert!(matches!(
        load_response_snapshot(&mut missing_header, "response_snapshot_reload_missing"),
        Err(ResponseSnapshotPersistenceError::Database(_))
    ));
    missing_header.rollback().unwrap();
}

#[test]
fn negative_stored_sequence_fails_closed_on_reload() {
    let _guard = reload_guard();
    let mut client = test_client();
    reset_tables(&mut client);
    apply_response_snapshot_migration(&mut client).unwrap();
    persist_ok(
        &mut client,
        &frozen_snapshot(
            "session_reload_negative",
            "response_snapshot_reload_negative",
            &[write(
                "server_event_001",
                "client_event_001",
                "item_version_001",
                PAYLOAD_DIGEST,
            )],
        ),
    );
    client
        .batch_execute(
            "ALTER TABLE response_snapshot DROP CONSTRAINT response_snapshot_last_sequence_positive_check;",
        )
        .unwrap();
    client
        .execute(
            "UPDATE response_snapshot SET last_sequence = -1 \
             WHERE snapshot_ref = 'response_snapshot_reload_negative'",
            &[],
        )
        .unwrap();
    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        load_response_snapshot(&mut transaction, "response_snapshot_reload_negative"),
        Err(ResponseSnapshotPersistenceError::InvalidSequence)
    ));
    transaction.rollback().unwrap();
}
