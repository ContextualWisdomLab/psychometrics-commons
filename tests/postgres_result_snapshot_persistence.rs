//! PostgreSQL persistence contract for immutable product result snapshots.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::postgres_result::{
    apply_result_snapshot_migration, persist_result_snapshot, ResultSnapshotPersistenceDisposition,
    ResultSnapshotPersistenceError,
};
use psychometrics_commons_runtime::response::{ResponseLedger, ResponseWrite};
use psychometrics_commons_runtime::result::{ResultSnapshot, ResultSnapshotInput};
use psychometrics_commons_runtime::scoring::{
    ObservationDisposition, ScoreObservation, ScoringRequest, ScoringRequestInput, ScoringResult,
};
use psychometrics_commons_runtime::session::SessionState;

fn test_client(schema_prefix: &str) -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let mut client = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    let schema = format!("{schema_prefix}_{}", std::process::id());
    client
        .batch_execute(&format!(
            "CREATE SCHEMA IF NOT EXISTS {schema}; SET search_path TO {schema};"
        ))
        .unwrap();
    apply_result_snapshot_migration(&mut client).unwrap();
    client
}

fn completed_snapshot() -> psychometrics_commons_runtime::response::ResponseSnapshot {
    let mut ledger = ResponseLedger::new("result_session_ref").unwrap();
    ledger
        .record(
            SessionState::Active,
            ResponseWrite {
                server_event_ref: "result_response_event_ref",
                client_event_ref: "result_client_event_ref",
                item_version_ref: "result_item_version_ref",
                payload_digest: "sha256:result-response",
            },
        )
        .unwrap();
    ledger
        .freeze_as(SessionState::Completed, "result_response_snapshot_ref")
        .unwrap()
}

fn result_snapshot(result_snapshot_ref: &str, narrative_version_ref: &str) -> ResultSnapshot {
    let request = ScoringRequest::from_snapshot(
        &completed_snapshot(),
        ScoringRequestInput {
            scoring_request_ref: "result_scoring_request_ref",
            response_snapshot_ref: "result_response_snapshot_ref",
            assessment_spec_ref: "result_assessment_spec_ref",
            instrument_version_ref: "result_instrument_version_ref",
            scoring_version_ref: "result_scoring_version_ref",
            calibration_reference: "sha256:result-calibration",
            norm_version_ref: Some("result_norm_version_ref"),
            requested_output_schema_version: 1,
        },
    )
    .unwrap();
    let result = ScoringResult::new(
        "result_scoring_result_ref",
        &request,
        "sha256:result-engine",
        vec![
            ScoreObservation::scored("big_five_openness", 0.0, Some(0.25)).unwrap(),
            ScoreObservation::without_score(
                "big_five_neuroticism",
                ObservationDisposition::Failed,
            )
            .unwrap(),
        ],
    )
    .unwrap();
    ResultSnapshot::new(
        &request,
        &result,
        ResultSnapshotInput {
            result_snapshot_ref,
            participant_ref: "result_participant_ref",
            narrative_version_ref,
            consent_snapshot_refs: &["service_consent_ref", "research_consent_ref"],
            created_at_unix_ms: 1_786_240_000_000,
            supersedes_ref: Some("prior_result_snapshot_ref"),
        },
    )
    .unwrap()
}

#[test]
fn exact_replay_is_idempotent_and_conflicting_reuse_fails_closed() {
    let mut client = test_client("result_snapshot_replay_test");
    let original = result_snapshot("result_snapshot_persisted_ref", "narrative_version_ref");
    let conflicting = result_snapshot(
        "result_snapshot_persisted_ref",
        "different_narrative_version_ref",
    );

    let mut transaction = client.transaction().unwrap();
    assert_eq!(
        persist_result_snapshot(&mut transaction, &original).unwrap(),
        ResultSnapshotPersistenceDisposition::Inserted
    );
    transaction.commit().unwrap();

    let mut transaction = client.transaction().unwrap();
    assert_eq!(
        persist_result_snapshot(&mut transaction, &original).unwrap(),
        ResultSnapshotPersistenceDisposition::Duplicate
    );
    transaction.commit().unwrap();

    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        persist_result_snapshot(&mut transaction, &conflicting),
        Err(ResultSnapshotPersistenceError::ConflictingReplay)
    ));
    transaction.rollback().unwrap();
}
