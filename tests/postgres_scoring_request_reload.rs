//! Real `PostgreSQL` contract: a version-pinned scoring request survives restart.
//!
//! After a buyer completes a two-item path and dispatch is persisted, a worker
//! that still has `scoring_request_ref` must recover the exact `AssessmentSpec`,
//! instrument, scoring, calibration, and optional norm pins. A missing request
//! is absent. Stronger isolation, blank aliases, unsupported stored schema, and
//! missing relations fail closed.

use postgres::{Client, IsolationLevel, NoTls};
use psychometrics_commons_runtime::postgres_scoring_request::{
    apply_scoring_request_migration, load_scoring_request, persist_scoring_request,
    ScoringRequestPersistenceDisposition, ScoringRequestPersistenceError,
};
use psychometrics_commons_runtime::response::{ResponseLedger, ResponseWrite};
use psychometrics_commons_runtime::scoring::{
    ScoreObservation, ScoringRequest, ScoringRequestInput, ScoringResult,
};
use psychometrics_commons_runtime::session::SessionState;
use std::sync::{Mutex, MutexGuard};

const PAYLOAD_DIGEST: &str =
    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const ENGINE_DIGEST: &str =
    "sha256:fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210";

static SCORING_REQUEST_RELOAD_LOCK: Mutex<()> = Mutex::new(());

fn reload_guard() -> MutexGuard<'static, ()> {
    SCORING_REQUEST_RELOAD_LOCK
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
            "CREATE SCHEMA IF NOT EXISTS scoring_request_reload_test;\
             SET search_path TO scoring_request_reload_test;",
        )
        .unwrap();
    client
}

fn reset_tables(client: &mut Client) {
    client
        .batch_execute("DROP TABLE IF EXISTS scoring_request_reload_test.scoring_request;")
        .unwrap();
}

fn request_named(
    session_ref: &str,
    scoring_request_ref: &str,
    snapshot_ref: &str,
    norm_version_ref: Option<&str>,
) -> ScoringRequest {
    let mut ledger = ResponseLedger::new(session_ref).unwrap();
    ledger
        .record(
            SessionState::Active,
            ResponseWrite {
                server_event_ref: "server_event_zzz_first",
                client_event_ref: "client_event_001",
                item_version_ref: "item_version_001",
                payload_digest: PAYLOAD_DIGEST,
            },
        )
        .unwrap();
    ledger
        .record(
            SessionState::Active,
            ResponseWrite {
                server_event_ref: "server_event_aaa_second",
                client_event_ref: "client_event_002",
                item_version_ref: "item_version_002",
                payload_digest: ENGINE_DIGEST,
            },
        )
        .unwrap();
    let snapshot = ledger
        .freeze_as(SessionState::Completed, snapshot_ref)
        .unwrap();
    ScoringRequest::from_snapshot(
        &snapshot,
        ScoringRequestInput {
            scoring_request_ref,
            response_snapshot_ref: snapshot_ref,
            assessment_spec_ref: "assessment_spec_big_five_v1",
            instrument_version_ref: "instrument_version_big_five_ko_v1",
            scoring_version_ref: "scoring_version_big_five_v1",
            calibration_reference: "calibration_big_five_ko_v1",
            norm_version_ref,
            requested_output_schema_version: 1,
        },
    )
    .unwrap()
}

fn persist_ok(
    client: &mut Client,
    request: &ScoringRequest,
) -> ScoringRequestPersistenceDisposition {
    let mut transaction = client.transaction().unwrap();
    let disposition = persist_scoring_request(&mut transaction, request).unwrap();
    transaction.commit().unwrap();
    disposition
}

fn load_ok(client: &mut Client, scoring_request_ref: &str) -> Option<ScoringRequest> {
    let mut transaction = client.transaction().unwrap();
    let loaded = load_scoring_request(&mut transaction, scoring_request_ref).unwrap();
    transaction.commit().unwrap();
    loaded
}

#[test]
fn unknown_scoring_request_reload_is_absent() {
    let _guard = reload_guard();
    let mut client = test_client();
    reset_tables(&mut client);
    apply_scoring_request_migration(&mut client).unwrap();

    assert!(
        load_ok(&mut client, "scoring_request_reload_unknown").is_none(),
        "a scoring request that was never persisted must not appear after restart"
    );
}

#[test]
fn two_item_dispatch_pins_reload_and_remain_result_bindable() {
    let _guard = reload_guard();
    let mut client = test_client();
    reset_tables(&mut client);
    apply_scoring_request_migration(&mut client).unwrap();

    let request = request_named(
        "session_reload_score",
        "scoring_request_reload_score",
        "response_snapshot_reload_score",
        Some("norm_version_big_five_ko_v1"),
    );
    persist_ok(&mut client, &request);

    let loaded = load_ok(&mut client, "scoring_request_reload_score")
        .expect("a persisted scoring request must reload after restart");
    assert_eq!(loaded, request);
    assert_eq!(
        loaded.instrument_version_ref(),
        "instrument_version_big_five_ko_v1"
    );
    assert_eq!(
        loaded.norm_version_ref(),
        Some("norm_version_big_five_ko_v1")
    );

    let result = ScoringResult::new(
        "scoring_result_reload_score",
        &loaded,
        ENGINE_DIGEST,
        vec![ScoreObservation::scored("big_five_openness", 1.2, Some(0.15)).unwrap()],
    )
    .expect("reloaded version pins must still accept a typed result");
    assert_eq!(result.scoring_request_ref(), "scoring_request_reload_score");
}

#[test]
fn scoring_request_without_norm_reloads_without_inventing_a_norm() {
    let _guard = reload_guard();
    let mut client = test_client();
    reset_tables(&mut client);
    apply_scoring_request_migration(&mut client).unwrap();

    let request = request_named(
        "session_reload_score_plain",
        "scoring_request_reload_plain",
        "response_snapshot_reload_plain",
        None,
    );
    persist_ok(&mut client, &request);
    let loaded = load_ok(&mut client, "scoring_request_reload_plain")
        .expect("a request without a norm must reload");
    assert_eq!(loaded, request);
    assert_eq!(loaded.norm_version_ref(), None);
}

#[test]
fn stored_unsupported_schema_and_blank_aliases_fail_closed_on_reload() {
    let _guard = reload_guard();
    let mut client = test_client();
    reset_tables(&mut client);
    apply_scoring_request_migration(&mut client).unwrap();

    persist_ok(
        &mut client,
        &request_named(
            "session_reload_score_schema",
            "scoring_request_reload_schema",
            "response_snapshot_reload_schema",
            None,
        ),
    );
    client
        .execute(
            "UPDATE scoring_request SET requested_output_schema_version = 2 \
             WHERE scoring_request_ref = 'scoring_request_reload_schema'",
            &[],
        )
        .unwrap();
    let mut unsupported = client.transaction().unwrap();
    assert!(matches!(
        load_scoring_request(&mut unsupported, "scoring_request_reload_schema"),
        Err(ScoringRequestPersistenceError::CorruptHistory)
    ));
    unsupported.rollback().unwrap();

    let mut transaction = client.transaction().unwrap();
    for invalid_ref in ["", " ", "42"] {
        assert!(matches!(
            load_scoring_request(&mut transaction, invalid_ref),
            Err(ScoringRequestPersistenceError::InvalidReference)
        ));
    }
    transaction.rollback().unwrap();
}

#[test]
fn scoring_request_reload_requires_read_committed() {
    let _guard = reload_guard();
    let mut client = test_client();
    reset_tables(&mut client);
    apply_scoring_request_migration(&mut client).unwrap();

    let mut serializable = client
        .build_transaction()
        .isolation_level(IsolationLevel::Serializable)
        .start()
        .unwrap();
    assert!(matches!(
        load_scoring_request(&mut serializable, "scoring_request_reload_score"),
        Err(ScoringRequestPersistenceError::UnsupportedIsolationLevel)
    ));
    serializable.rollback().unwrap();

    let mut repeatable = client
        .build_transaction()
        .isolation_level(IsolationLevel::RepeatableRead)
        .start()
        .unwrap();
    assert!(matches!(
        load_scoring_request(&mut repeatable, "scoring_request_reload_score"),
        Err(ScoringRequestPersistenceError::UnsupportedIsolationLevel)
    ));
    repeatable.rollback().unwrap();
}

#[test]
fn missing_scoring_request_relation_fails_closed_on_reload() {
    let _guard = reload_guard();
    let mut client = test_client();
    reset_tables(&mut client);
    apply_scoring_request_migration(&mut client).unwrap();
    persist_ok(
        &mut client,
        &request_named(
            "session_reload_score_missing",
            "scoring_request_reload_missing",
            "response_snapshot_reload_missing",
            None,
        ),
    );

    client.batch_execute("DROP TABLE scoring_request;").unwrap();
    let mut missing = client.transaction().unwrap();
    assert!(matches!(
        load_scoring_request(&mut missing, "scoring_request_reload_missing"),
        Err(ScoringRequestPersistenceError::Database(_))
    ));
    missing.rollback().unwrap();
}

#[test]
fn overflow_stored_schema_version_fails_closed_on_reload() {
    let _guard = reload_guard();
    let mut client = test_client();
    reset_tables(&mut client);
    apply_scoring_request_migration(&mut client).unwrap();
    persist_ok(
        &mut client,
        &request_named(
            "session_reload_score_overflow",
            "scoring_request_reload_overflow",
            "response_snapshot_reload_overflow",
            None,
        ),
    );
    client
        .batch_execute(
            "ALTER TABLE scoring_request DROP CONSTRAINT scoring_request_schema_version_positive_check;",
        )
        .unwrap();
    client
        .execute(
            "UPDATE scoring_request SET requested_output_schema_version = -1 \
             WHERE scoring_request_ref = 'scoring_request_reload_overflow'",
            &[],
        )
        .unwrap();
    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        load_scoring_request(&mut transaction, "scoring_request_reload_overflow"),
        Err(ScoringRequestPersistenceError::InvalidSchemaVersion)
    ));
    transaction.rollback().unwrap();
}
