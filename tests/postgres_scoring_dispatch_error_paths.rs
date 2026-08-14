//! Failure-path coverage for atomic scoring-dispatch persistence.

use postgres::{Client, IsolationLevel, NoTls};
use psychometrics_commons_runtime::integration::IntegrationEvent;
use psychometrics_commons_runtime::postgres_integration::{
    apply_integration_migration, PersistenceError,
};
use psychometrics_commons_runtime::postgres_scoring_job::{
    apply_scoring_job_migration, ScoringJobPersistenceError,
};
use psychometrics_commons_runtime::postgres_scoring_request::{
    apply_scoring_request_migration, persist_scoring_dispatch, ScoringDispatchPersistenceError,
    ScoringRequestPersistenceError,
};
use psychometrics_commons_runtime::response::{ResponseLedger, ResponseWrite};
use psychometrics_commons_runtime::scoring::{ScoringRequest, ScoringRequestInput};
use psychometrics_commons_runtime::scoring_job::ScoringJob;
use psychometrics_commons_runtime::session::SessionState;
use std::error::Error;
use std::sync::{Mutex, MutexGuard};

const PAYLOAD_DIGEST: &str =
    "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

static ERROR_PATH_TEST_LOCK: Mutex<()> = Mutex::new(());

fn error_path_guard() -> MutexGuard<'static, ()> {
    ERROR_PATH_TEST_LOCK
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
            "CREATE SCHEMA IF NOT EXISTS scoring_dispatch_error_path_test;\
             SET search_path TO scoring_dispatch_error_path_test;",
        )
        .unwrap();
    client
}

fn reset_and_migrate(client: &mut Client) {
    client
        .batch_execute(
            "DROP TABLE IF EXISTS integration_delivery_attempt;\
             DROP TABLE IF EXISTS integration_inbox;\
             DROP TABLE IF EXISTS integration_outbox;\
             DROP TABLE IF EXISTS scoring_job_state;\
             DROP TABLE IF EXISTS scoring_request;",
        )
        .unwrap();
    apply_integration_migration(client).unwrap();
    apply_scoring_job_migration(client).unwrap();
    apply_scoring_request_migration(client).unwrap();
}

fn request_named(scoring_request_ref: &str) -> ScoringRequest {
    let mut ledger = ResponseLedger::new("session_dispatch_error_path").unwrap();
    ledger
        .record(
            SessionState::Active,
            ResponseWrite {
                server_event_ref: "server_event_dispatch_error_path",
                client_event_ref: "client_event_dispatch_error_path",
                item_version_ref: "item_version_dispatch_error_path",
                payload_digest: PAYLOAD_DIGEST,
            },
        )
        .unwrap();
    let snapshot = ledger
        .freeze_as(
            SessionState::Completed,
            "response_snapshot_dispatch_error_path",
        )
        .unwrap();
    ScoringRequest::from_snapshot(
        &snapshot,
        ScoringRequestInput {
            scoring_request_ref,
            response_snapshot_ref: "response_snapshot_dispatch_error_path",
            assessment_spec_ref: "assessment_spec_big_five_v1",
            instrument_version_ref: "instrument_version_big_five_ko_v1",
            scoring_version_ref: "scoring_version_big_five_v1",
            calibration_reference: "calibration_big_five_ko_v1",
            norm_version_ref: None,
            requested_output_schema_version: 1,
        },
    )
    .unwrap()
}

fn dispatch_event() -> IntegrationEvent {
    IntegrationEvent::new(
        "event_dispatch_error_path",
        "scoring.dispatch.requested",
        "v1",
        "psychometrics_commons",
        "tenant_dispatch_error_path",
        "scoring_job_dispatch_error_path",
        20_000,
        "correlation_dispatch_error_path",
        None,
        PAYLOAD_DIGEST,
    )
    .unwrap()
}

#[test]
fn request_isolation_failure_is_preserved_without_committed_partial_state() {
    let _guard = error_path_guard();
    let mut client = test_client();
    reset_and_migrate(&mut client);
    let request = request_named("scoring_request_dispatch_serializable");
    let job = ScoringJob::new(
        "scoring_job_dispatch_error_path",
        request.scoring_request_ref(),
        3,
    )
    .unwrap();
    let event = dispatch_event();

    let mut transaction = client
        .build_transaction()
        .isolation_level(IsolationLevel::Serializable)
        .start()
        .unwrap();
    assert!(matches!(
        persist_scoring_dispatch(&mut transaction, &request, &job, &event, 3),
        Err(ScoringDispatchPersistenceError::Request(
            ScoringRequestPersistenceError::UnsupportedIsolationLevel
        ))
    ));
    transaction.rollback().unwrap();

    let request_count: i64 = client
        .query_one("SELECT count(*) FROM scoring_request", &[])
        .unwrap()
        .get(0);
    assert_eq!(request_count, 0);
}

#[test]
fn nonfresh_job_failure_rolls_back_request_insert() {
    let _guard = error_path_guard();
    let mut client = test_client();
    reset_and_migrate(&mut client);
    let request = request_named("scoring_request_dispatch_nonfresh");
    let mut job = ScoringJob::new(
        "scoring_job_dispatch_error_path",
        request.scoring_request_ref(),
        3,
    )
    .unwrap();
    job.claim(
        "worker_dispatch_error_path",
        "lease_dispatch_error_path",
        20_000,
        30_000,
    )
    .unwrap();
    let event = dispatch_event();

    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        persist_scoring_dispatch(&mut transaction, &request, &job, &event, 3),
        Err(ScoringDispatchPersistenceError::Job(
            ScoringJobPersistenceError::UnsupportedInitialState
        ))
    ));
    transaction.rollback().unwrap();

    let request_count: i64 = client
        .query_one("SELECT count(*) FROM scoring_request", &[])
        .unwrap()
        .get(0);
    assert_eq!(request_count, 0);
}

#[test]
fn dispatch_error_display_and_sources_are_typed() {
    let cases = [
        ScoringDispatchPersistenceError::MismatchedScoringRequest,
        ScoringDispatchPersistenceError::Request(ScoringRequestPersistenceError::InvalidReference),
        ScoringDispatchPersistenceError::Job(ScoringJobPersistenceError::InvalidReference),
        ScoringDispatchPersistenceError::Outbox(PersistenceError::InvalidReference),
    ];

    for (index, error) in cases.into_iter().enumerate() {
        assert!(!error.to_string().is_empty());
        assert_eq!(error.source().is_some(), index != 0);
    }
}
