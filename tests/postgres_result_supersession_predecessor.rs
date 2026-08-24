//! Real `PostgreSQL` contract for result-snapshot supersession predecessor integrity.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::postgres_result_snapshot::{
    apply_result_snapshot_migration, persist_result_snapshot, ResultSnapshotPersistenceDisposition,
    ResultSnapshotPersistenceError,
};
use psychometrics_commons_runtime::response::{ResponseLedger, ResponseWrite};
use psychometrics_commons_runtime::result::{ResultSnapshot, ResultSnapshotInput};
use psychometrics_commons_runtime::scoring::{
    ScoreObservation, ScoringRequest, ScoringRequestInput, ScoringResult,
};
use psychometrics_commons_runtime::session::SessionState;
use std::sync::{Mutex, MutexGuard};

const ENGINE_DIGEST: &str =
    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const DATABASE_TEST_LOCK_KEY: i64 = 0x5253_5355_5052_4544;

static RESULT_SUPERSESSION_TEST_LOCK: Mutex<()> = Mutex::new(());

fn test_guard() -> MutexGuard<'static, ()> {
    RESULT_SUPERSESSION_TEST_LOCK
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
            "DROP SCHEMA IF EXISTS result_supersession_predecessor_test CASCADE;\
             CREATE SCHEMA result_supersession_predecessor_test;\
             SET search_path TO result_supersession_predecessor_test;",
        )
        .unwrap();
    client
}

fn snapshot(result_snapshot_ref: &str, supersedes_ref: Option<&str>) -> ResultSnapshot {
    let mut ledger = ResponseLedger::new("session_result_supersession").unwrap();
    ledger
        .record(
            SessionState::Active,
            ResponseWrite {
                server_event_ref: "server_event_result_supersession",
                client_event_ref: "client_event_result_supersession",
                item_version_ref: "item_version_001",
                payload_digest: ENGINE_DIGEST,
            },
        )
        .unwrap();
    let response = ledger
        .freeze_as(
            SessionState::Completed,
            "response_snapshot_result_supersession",
        )
        .unwrap();
    let request = ScoringRequest::from_snapshot(
        &response,
        ScoringRequestInput {
            scoring_request_ref: "scoring_request_result_supersession",
            response_snapshot_ref: "response_snapshot_result_supersession",
            assessment_spec_ref: "assessment_spec_big_five_v1",
            instrument_version_ref: "instrument_version_big_five_ko_v1",
            scoring_version_ref: "scoring_version_big_five_v1",
            calibration_reference: "calibration_big_five_ko_v1",
            norm_version_ref: Some("norm_version_big_five_ko_v1"),
            requested_output_schema_version: 1,
        },
    )
    .unwrap();
    let scoring_result = ScoringResult::new(
        "scoring_result_result_supersession",
        &request,
        ENGINE_DIGEST,
        vec![ScoreObservation::scored("construct_big_five", 0.25, Some(0.05)).unwrap()],
    )
    .unwrap();
    ResultSnapshot::new(
        &request,
        &scoring_result,
        ResultSnapshotInput {
            result_snapshot_ref,
            participant_ref: "participant_result_supersession",
            narrative_version_ref: "narrative_version_big_five_v1",
            consent_snapshot_refs: &["consent_snapshot_service_v1"],
            created_at_unix_ms: 70_000,
            supersedes_ref,
        },
    )
    .unwrap()
}

fn insert_raw_snapshot(
    client: &mut Client,
    result_snapshot_ref: &str,
    supersedes_ref: Option<&str>,
) -> Result<u64, postgres::Error> {
    client.execute(
        "INSERT INTO result_snapshot (\
             result_snapshot_ref, participant_ref, scoring_result_ref, session_ref, \
             response_snapshot_ref, assessment_spec_ref, instrument_version_ref, \
             scoring_version_ref, calibration_reference, norm_version_ref, \
             requested_output_schema_version, narrative_version_ref, consent_snapshot_refs, \
             engine_artifact_digest, created_at_unix_ms, supersedes_ref\
         ) VALUES (\
             $1, 'participant_result_supersession', 'scoring_result_result_supersession', \
             'session_result_supersession', 'response_snapshot_result_supersession', \
             'assessment_spec_big_five_v1', 'instrument_version_big_five_ko_v1', \
             'scoring_version_big_five_v1', 'calibration_big_five_ko_v1', \
             'norm_version_big_five_ko_v1', 1, 'narrative_version_big_five_v1', \
             ARRAY['consent_snapshot_service_v1'], $2, 70000, $3\
         )",
        &[&result_snapshot_ref, &ENGINE_DIGEST, &supersedes_ref],
    )
}

#[test]
fn fixture_serialization_is_visible_across_postgres_sessions() {
    let _guard = test_guard();
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let mut contender = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    let acquired: bool = contender
        .query_one("SELECT pg_try_advisory_lock($1)", &[&DATABASE_TEST_LOCK_KEY])
        .expect("contender PostgreSQL session should query the fixture lock")
        .get(0);
    if acquired {
        contender
            .query_one("SELECT pg_advisory_unlock($1)", &[&DATABASE_TEST_LOCK_KEY])
            .expect("contender lock cleanup should succeed");
    }
    assert!(
        !acquired,
        "fixture serialization must be visible to independent PostgreSQL sessions"
    );
}

#[test]
fn superseding_snapshot_requires_an_existing_predecessor() {
    let _guard = test_guard();
    let mut client = test_client();
    apply_result_snapshot_migration(&mut client).unwrap();

    let successor = snapshot(
        "result_snapshot_successor",
        Some("result_snapshot_missing_predecessor"),
    );
    let mut transaction = client.transaction().unwrap();
    let error = persist_result_snapshot(&mut transaction, &successor).unwrap_err();
    assert!(matches!(
        error,
        ResultSnapshotPersistenceError::InvalidSupersession
    ));
    assert_eq!(
        error.to_string(),
        "result snapshot supersession predecessor must already exist"
    );
    let stored: i64 = transaction
        .query_one("SELECT COUNT(*)::bigint FROM result_snapshot", &[])
        .unwrap()
        .get(0);
    assert_eq!(stored, 0);
    transaction.rollback().unwrap();
}

#[test]
fn predecessor_inserted_earlier_in_the_same_transaction_is_visible() {
    let _guard = test_guard();
    let mut client = test_client();
    apply_result_snapshot_migration(&mut client).unwrap();

    let predecessor = snapshot("result_snapshot_same_transaction_predecessor", None);
    let successor = snapshot(
        "result_snapshot_same_transaction_successor",
        Some("result_snapshot_same_transaction_predecessor"),
    );
    let mut transaction = client.transaction().unwrap();
    assert_eq!(
        persist_result_snapshot(&mut transaction, &predecessor).unwrap(),
        ResultSnapshotPersistenceDisposition::Inserted
    );
    assert_eq!(
        persist_result_snapshot(&mut transaction, &successor).unwrap(),
        ResultSnapshotPersistenceDisposition::Inserted
    );
    transaction.rollback().unwrap();
}

#[test]
fn direct_sql_cannot_bypass_supersession_predecessor_integrity() {
    let _guard = test_guard();
    let mut client = test_client();
    apply_result_snapshot_migration(&mut client).unwrap();

    assert!(insert_raw_snapshot(
        &mut client,
        "result_snapshot_sql_successor",
        Some("result_snapshot_missing_predecessor")
    )
    .is_err());
    let stored: i64 = client
        .query_one("SELECT COUNT(*)::bigint FROM result_snapshot", &[])
        .unwrap()
        .get(0);
    assert_eq!(stored, 0);
}

#[test]
fn migration_reapply_rejects_historical_supersession_cycles_with_check_violation() {
    let _guard = test_guard();
    let mut client = test_client();
    apply_result_snapshot_migration(&mut client).unwrap();

    insert_raw_snapshot(&mut client, "result_snapshot_cycle_a", None).unwrap();
    insert_raw_snapshot(
        &mut client,
        "result_snapshot_cycle_b",
        Some("result_snapshot_cycle_a"),
    )
    .unwrap();
    client
        .batch_execute(
            "ALTER TABLE result_snapshot DISABLE TRIGGER result_snapshot_immutable_guard;\
             UPDATE result_snapshot \
             SET supersedes_ref = 'result_snapshot_cycle_b' \
             WHERE result_snapshot_ref = 'result_snapshot_cycle_a';\
             ALTER TABLE result_snapshot ENABLE TRIGGER result_snapshot_immutable_guard;",
        )
        .unwrap();

    let error = apply_result_snapshot_migration(&mut client).unwrap_err();
    assert_eq!(
        error.code().map(postgres::error::SqlState::code),
        Some("23514")
    );
}

#[test]
fn migration_reapply_rejects_historical_dangling_predecessor_with_foreign_key_violation() {
    let _guard = test_guard();
    let mut client = test_client();
    apply_result_snapshot_migration(&mut client).unwrap();

    insert_raw_snapshot(&mut client, "result_snapshot_dangling", None).unwrap();
    client
        .batch_execute(
            "ALTER TABLE result_snapshot DISABLE TRIGGER result_snapshot_immutable_guard;\
             UPDATE result_snapshot \
             SET supersedes_ref = 'result_snapshot_missing_historical_predecessor' \
             WHERE result_snapshot_ref = 'result_snapshot_dangling';\
             ALTER TABLE result_snapshot ENABLE TRIGGER result_snapshot_immutable_guard;",
        )
        .unwrap();

    let error = apply_result_snapshot_migration(&mut client).unwrap_err();
    assert_eq!(
        error.code().map(postgres::error::SqlState::code),
        Some("23503")
    );
}

#[test]
fn predecessor_lookup_database_failure_stays_typed_as_database_error() {
    let _guard = test_guard();
    let mut client = test_client();
    let successor = snapshot(
        "result_snapshot_missing_relation",
        Some("result_snapshot_predecessor"),
    );
    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        persist_result_snapshot(&mut transaction, &successor),
        Err(ResultSnapshotPersistenceError::Database(_))
    ));
    transaction.rollback().unwrap();
}
