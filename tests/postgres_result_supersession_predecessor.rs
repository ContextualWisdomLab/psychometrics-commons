//! Real `PostgreSQL` contract for result-snapshot supersession predecessor integrity.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::postgres_result_snapshot::{
    apply_result_snapshot_migration, persist_result_snapshot,
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
    assert!(persist_result_snapshot(&mut transaction, &successor).is_err());
    let stored: i64 = transaction
        .query_one("SELECT COUNT(*)::bigint FROM result_snapshot", &[])
        .unwrap()
        .get(0);
    assert_eq!(stored, 0);
    transaction.rollback().unwrap();
}
