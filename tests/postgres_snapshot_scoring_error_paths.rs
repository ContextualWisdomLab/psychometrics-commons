//! Failure-boundary coverage for atomic snapshot and scoring dispatch persistence.

use postgres::{Client, IsolationLevel, NoTls};
use psychometrics_commons_runtime::integration::IntegrationEvent;
use psychometrics_commons_runtime::postgres_integration::apply_integration_migration;
use psychometrics_commons_runtime::postgres_response_snapshot::{
    apply_response_snapshot_migration, ResponseSnapshotPersistenceError,
};
use psychometrics_commons_runtime::postgres_scoring_job::apply_scoring_job_migration;
use psychometrics_commons_runtime::postgres_scoring_orchestration::{
    persist_response_snapshot_and_scoring_dispatch, SnapshotScoringPersistenceError,
};
use psychometrics_commons_runtime::postgres_scoring_request::{
    apply_scoring_request_migration, ScoringDispatchPersistenceError,
};
use psychometrics_commons_runtime::response::{ResponseLedger, ResponseWrite};
use psychometrics_commons_runtime::scoring::{ScoringRequest, ScoringRequestInput};
use psychometrics_commons_runtime::scoring_job::ScoringJob;
use psychometrics_commons_runtime::session::SessionState;
use std::error::Error;
use std::sync::{Mutex, MutexGuard};

const DIGEST: &str = "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
static ERROR_PATH_LOCK: Mutex<()> = Mutex::new(());

fn test_guard() -> MutexGuard<'static, ()> {
    ERROR_PATH_LOCK
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
            "CREATE SCHEMA IF NOT EXISTS snapshot_scoring_error_test;\
             SET search_path TO snapshot_scoring_error_test;\
             DROP TABLE IF EXISTS integration_delivery_attempt;\
             DROP TABLE IF EXISTS integration_inbox;\
             DROP TABLE IF EXISTS integration_outbox;\
             DROP TABLE IF EXISTS scoring_job_state;\
             DROP TABLE IF EXISTS scoring_request;\
             DROP TABLE IF EXISTS response_snapshot_entry;\
             DROP TABLE IF EXISTS response_snapshot;",
        )
        .unwrap();
    apply_integration_migration(&mut client).unwrap();
    apply_scoring_job_migration(&mut client).unwrap();
    apply_response_snapshot_migration(&mut client).unwrap();
    apply_scoring_request_migration(&mut client).unwrap();
    client
}

fn evidence() -> (
    psychometrics_commons_runtime::response::ResponseSnapshot,
    ScoringRequest,
    IntegrationEvent,
) {
    let mut ledger = ResponseLedger::new("session_snapshot_error_alpha").unwrap();
    ledger
        .record(
            SessionState::Active,
            ResponseWrite {
                server_event_ref: "server_event_snapshot_error_alpha",
                client_event_ref: "client_event_snapshot_error_alpha",
                item_version_ref: "item_version_snapshot_error_alpha",
                payload_digest: DIGEST,
            },
        )
        .unwrap();
    let snapshot = ledger
        .freeze_as(SessionState::Completed, "response_snapshot_error_alpha")
        .unwrap();
    let request = ScoringRequest::from_snapshot(
        &snapshot,
        ScoringRequestInput {
            scoring_request_ref: "scoring_request_snapshot_error_alpha",
            response_snapshot_ref: "response_snapshot_error_alpha",
            assessment_spec_ref: "assessment_spec_big_five_v1",
            instrument_version_ref: "instrument_version_big_five_ko_v1",
            scoring_version_ref: "scoring_version_big_five_v1",
            calibration_reference: "calibration_big_five_ko_v1",
            norm_version_ref: None,
            requested_output_schema_version: 1,
        },
    )
    .unwrap();
    let event = IntegrationEvent::new(
        "event_snapshot_error_alpha",
        "scoring.dispatch.requested",
        "v1",
        "psychometrics_commons",
        "tenant_snapshot_error_alpha",
        "scoring_job_snapshot_error_alpha",
        10_000,
        "correlation_snapshot_error_alpha",
        Some("response_snapshot_error_alpha"),
        DIGEST,
    )
    .unwrap();
    (snapshot, request, event)
}

#[test]
fn snapshot_isolation_failure_is_preserved_before_any_write() {
    let _guard = test_guard();
    let mut client = test_client();
    let (snapshot, request, event) = evidence();
    let job = ScoringJob::new(
        "scoring_job_snapshot_error_alpha",
        request.scoring_request_ref(),
        3,
    )
    .unwrap();
    let mut transaction = client
        .build_transaction()
        .isolation_level(IsolationLevel::Serializable)
        .start()
        .unwrap();
    let error = persist_response_snapshot_and_scoring_dispatch(
        &mut transaction,
        &snapshot,
        &request,
        &job,
        &event,
        3,
    )
    .unwrap_err();
    assert!(matches!(
        error,
        SnapshotScoringPersistenceError::Snapshot(
            ResponseSnapshotPersistenceError::UnsupportedIsolationLevel
        )
    ));
    assert!(error.source().is_some());
    transaction.rollback().unwrap();
}

#[test]
fn same_snapshot_reference_with_different_session_fails_before_any_write() {
    let _guard = test_guard();
    let mut client = test_client();
    let (snapshot, _, event) = evidence();

    let mut other_ledger = ResponseLedger::new("session_snapshot_error_other").unwrap();
    other_ledger
        .record(
            SessionState::Active,
            ResponseWrite {
                server_event_ref: "server_event_snapshot_error_other",
                client_event_ref: "client_event_snapshot_error_other",
                item_version_ref: "item_version_snapshot_error_other",
                payload_digest: DIGEST,
            },
        )
        .unwrap();
    let other_snapshot = other_ledger
        .freeze_as(SessionState::Completed, "response_snapshot_error_alpha")
        .unwrap();
    let other_request = ScoringRequest::from_snapshot(
        &other_snapshot,
        ScoringRequestInput {
            scoring_request_ref: "scoring_request_snapshot_error_other",
            response_snapshot_ref: "response_snapshot_error_alpha",
            assessment_spec_ref: "assessment_spec_big_five_v1",
            instrument_version_ref: "instrument_version_big_five_ko_v1",
            scoring_version_ref: "scoring_version_big_five_v1",
            calibration_reference: "calibration_big_five_ko_v1",
            norm_version_ref: None,
            requested_output_schema_version: 1,
        },
    )
    .unwrap();
    let job = ScoringJob::new(
        "scoring_job_snapshot_error_other",
        other_request.scoring_request_ref(),
        3,
    )
    .unwrap();

    let mut transaction = client.transaction().unwrap();
    let error = persist_response_snapshot_and_scoring_dispatch(
        &mut transaction,
        &snapshot,
        &other_request,
        &job,
        &event,
        3,
    )
    .unwrap_err();
    assert!(matches!(
        error,
        SnapshotScoringPersistenceError::MismatchedSnapshotBinding
    ));
    assert!(error.source().is_none());
    transaction.rollback().unwrap();

    for table in [
        "response_snapshot",
        "scoring_request",
        "scoring_job_state",
        "integration_outbox",
    ] {
        let count: i64 = client
            .query_one(&format!("SELECT count(*) FROM {table}"), &[])
            .unwrap()
            .get(0);
        assert_eq!(count, 0, "{table} must remain empty");
    }
}

#[test]
fn dispatch_binding_failure_rolls_back_the_new_snapshot() {
    let _guard = test_guard();
    let mut client = test_client();
    let (snapshot, request, event) = evidence();
    let mismatched_job = ScoringJob::new(
        "scoring_job_snapshot_error_alpha",
        "scoring_request_other",
        3,
    )
    .unwrap();
    let mut transaction = client.transaction().unwrap();
    let error = persist_response_snapshot_and_scoring_dispatch(
        &mut transaction,
        &snapshot,
        &request,
        &mismatched_job,
        &event,
        3,
    )
    .unwrap_err();
    assert!(matches!(
        error,
        SnapshotScoringPersistenceError::Dispatch(
            ScoringDispatchPersistenceError::MismatchedScoringRequest
        )
    ));
    assert!(error.source().is_some());
    transaction.rollback().unwrap();

    let snapshot_count: i64 = client
        .query_one("SELECT count(*) FROM response_snapshot", &[])
        .unwrap()
        .get(0);
    assert_eq!(snapshot_count, 0);
}

#[test]
fn every_error_family_has_stable_operator_facing_display() {
    let errors = [
        SnapshotScoringPersistenceError::MismatchedSnapshotBinding,
        SnapshotScoringPersistenceError::Snapshot(
            ResponseSnapshotPersistenceError::InvalidReference,
        ),
        SnapshotScoringPersistenceError::Dispatch(
            ScoringDispatchPersistenceError::MismatchedScoringRequest,
        ),
    ];
    for error in errors {
        assert!(!error.to_string().is_empty());
    }
}
