//! Real `PostgreSQL` regression for result-snapshot replay error classification.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::postgres_result_snapshot::{
    apply_result_snapshot_migration, persist_result_snapshot, ResultSnapshotPersistenceDisposition,
    ResultSnapshotPersistenceError,
};
#[path = "response_support/mod.rs"]
mod response_support;

use psychometrics_commons_runtime::response::ResponseWrite;
use psychometrics_commons_runtime::result::{ResultSnapshot, ResultSnapshotInput};
use psychometrics_commons_runtime::scoring::{
    ScoreObservation, ScoringRequest, ScoringRequestInput, ScoringResult,
};
use response_support::frozen_snapshot;

const ENGINE_DIGEST: &str =
    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const DATABASE_TEST_LOCK_KEY: i64 = 0x5253_5250_4C59_434C;

fn connect_client() -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    Client::connect(&connection, NoTls).expect("isolated CI PostgreSQL database must be reachable")
}

fn test_guard() -> Client {
    let mut client = connect_client();
    client
        .query_one("SELECT pg_advisory_lock($1)", &[&DATABASE_TEST_LOCK_KEY])
        .expect("shared PostgreSQL replay-classification fixture lock should be acquired");
    client
}

fn test_client() -> Client {
    let mut client = connect_client();
    client
        .batch_execute(
            "DROP SCHEMA IF EXISTS result_supersession_replay_classification_test CASCADE;\
             CREATE SCHEMA result_supersession_replay_classification_test;\
             SET search_path TO result_supersession_replay_classification_test;",
        )
        .unwrap();
    client
}

fn snapshot(result_snapshot_ref: &str, supersedes_ref: Option<&str>) -> ResultSnapshot {
    let response = frozen_snapshot(
        "session_result_supersession_replay",
        "response_snapshot_result_supersession_replay",
        &[ResponseWrite {
            server_event_ref: "server_event_result_supersession_replay",
            client_event_ref: "client_event_result_supersession_replay",
            item_version_ref: "item_version_001",
            payload_digest: ENGINE_DIGEST,
        }],
    );
    let request = ScoringRequest::from_snapshot(
        &response,
        ScoringRequestInput {
            scoring_request_ref: "scoring_request_result_supersession_replay",
            response_snapshot_ref: "response_snapshot_result_supersession_replay",
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
        "scoring_result_result_supersession_replay",
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
            participant_ref: "participant_result_supersession_replay",
            narrative_version_ref: "narrative_version_big_five_v1",
            consent_snapshot_refs: &["consent_snapshot_service_v1"],
            created_at_unix_ms: 70_000,
            supersedes_ref,
        },
    )
    .unwrap()
}

#[test]
fn existing_result_rebinding_is_classified_before_new_predecessor_validation() {
    let _guard = test_guard();
    let mut client = test_client();
    apply_result_snapshot_migration(&mut client).unwrap();

    let original = snapshot("result_snapshot_replay_conflict", None);
    let mut first = client.transaction().unwrap();
    assert_eq!(
        persist_result_snapshot(&mut first, &original).unwrap(),
        ResultSnapshotPersistenceDisposition::Inserted
    );
    first.commit().unwrap();

    let rebound = snapshot(
        "result_snapshot_replay_conflict",
        Some("result_snapshot_missing_predecessor"),
    );
    let mut replay = client.transaction().unwrap();
    let error = persist_result_snapshot(&mut replay, &rebound).unwrap_err();
    assert!(matches!(
        error,
        ResultSnapshotPersistenceError::ConflictingReplay
    ));
    replay.rollback().unwrap();

    let row = client
        .query_one(
            "SELECT supersedes_ref, COUNT(*) OVER ()::bigint \
             FROM result_snapshot WHERE result_snapshot_ref = $1",
            &[&"result_snapshot_replay_conflict"],
        )
        .unwrap();
    let supersedes_ref: Option<String> = row.get(0);
    let row_count: i64 = row.get(1);
    assert_eq!(supersedes_ref, None);
    assert_eq!(row_count, 1);
}
