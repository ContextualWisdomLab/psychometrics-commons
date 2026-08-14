//! Real `PostgreSQL` contract for response-snapshot and scoring-dispatch atomicity.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::integration::IntegrationEvent;
use psychometrics_commons_runtime::postgres_integration::{
    apply_integration_migration, enqueue_outbox_event, PersistenceDisposition,
};
use psychometrics_commons_runtime::postgres_response_snapshot::{
    apply_response_snapshot_migration, ResponseSnapshotPersistenceDisposition,
};
use psychometrics_commons_runtime::postgres_scoring_job::{
    apply_scoring_job_migration, ScoringJobPersistenceDisposition,
};
use psychometrics_commons_runtime::postgres_scoring_orchestration::{
    persist_response_snapshot_and_scoring_dispatch, SnapshotScoringPersistenceError,
};
use psychometrics_commons_runtime::postgres_scoring_request::{
    apply_scoring_request_migration, ScoringRequestPersistenceDisposition,
};
use psychometrics_commons_runtime::response::{ResponseLedger, ResponseSnapshot, ResponseWrite};
use psychometrics_commons_runtime::scoring::{ScoringRequest, ScoringRequestInput};
use psychometrics_commons_runtime::scoring_job::ScoringJob;
use psychometrics_commons_runtime::session::SessionState;
use std::error::Error;
use std::sync::{Mutex, MutexGuard};

const DIGEST_A: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const DIGEST_B: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
static SNAPSHOT_SCORING_TEST_LOCK: Mutex<()> = Mutex::new(());

fn test_guard() -> MutexGuard<'static, ()> {
    SNAPSHOT_SCORING_TEST_LOCK
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
            "CREATE SCHEMA IF NOT EXISTS snapshot_scoring_transaction_test;\
             SET search_path TO snapshot_scoring_transaction_test;",
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
             DROP TABLE IF EXISTS scoring_request;\
             DROP TABLE IF EXISTS response_snapshot_entry;\
             DROP TABLE IF EXISTS response_snapshot;",
        )
        .unwrap();
    apply_integration_migration(client).unwrap();
    apply_scoring_job_migration(client).unwrap();
    apply_response_snapshot_migration(client).unwrap();
    apply_scoring_request_migration(client).unwrap();
}

fn snapshot_and_request(
    session_ref: &str,
    snapshot_ref: &str,
    scoring_request_ref: &str,
) -> (ResponseSnapshot, ScoringRequest) {
    let mut ledger = ResponseLedger::new(session_ref).unwrap();
    ledger
        .record(
            SessionState::Active,
            ResponseWrite {
                server_event_ref: "server_event_snapshot_dispatch_alpha",
                client_event_ref: "client_event_snapshot_dispatch_alpha",
                item_version_ref: "item_version_snapshot_dispatch_alpha",
                payload_digest: DIGEST_A,
            },
        )
        .unwrap();
    let snapshot = ledger
        .freeze_as(SessionState::Completed, snapshot_ref)
        .unwrap();
    let request = ScoringRequest::from_snapshot(
        &snapshot,
        ScoringRequestInput {
            scoring_request_ref,
            response_snapshot_ref: snapshot_ref,
            assessment_spec_ref: "assessment_spec_big_five_v1",
            instrument_version_ref: "instrument_version_big_five_ko_v1",
            scoring_version_ref: "scoring_version_big_five_v1",
            calibration_reference: "calibration_big_five_ko_v1",
            norm_version_ref: Some("norm_version_big_five_ko_v1"),
            requested_output_schema_version: 1,
        },
    )
    .unwrap();
    (snapshot, request)
}

fn dispatch_event(event_ref: &str, job_ref: &str, digest: &str) -> IntegrationEvent {
    IntegrationEvent::new(
        event_ref,
        "scoring.dispatch.requested",
        "v1",
        "psychometrics_commons",
        "tenant_snapshot_dispatch_alpha",
        job_ref,
        10_000,
        "correlation_snapshot_dispatch_alpha",
        Some("response_snapshot_dispatch_alpha"),
        digest,
    )
    .unwrap()
}

#[test]
fn snapshot_request_job_and_outbox_commit_and_replay_together() {
    let _guard = test_guard();
    let mut client = test_client();
    reset_and_migrate(&mut client);
    let (snapshot, request) = snapshot_and_request(
        "session_snapshot_dispatch_alpha",
        "response_snapshot_dispatch_alpha",
        "scoring_request_snapshot_dispatch_alpha",
    );
    let job = ScoringJob::new(
        "scoring_job_snapshot_dispatch_alpha",
        request.scoring_request_ref(),
        3,
    )
    .unwrap();
    let event = dispatch_event(
        "event_snapshot_dispatch_alpha",
        job.scoring_job_ref(),
        DIGEST_A,
    );

    let mut transaction = client.transaction().unwrap();
    let inserted = persist_response_snapshot_and_scoring_dispatch(
        &mut transaction,
        &snapshot,
        &request,
        &job,
        &event,
        3,
    )
    .unwrap();
    assert_eq!(
        inserted.response_snapshot(),
        ResponseSnapshotPersistenceDisposition::Inserted
    );
    assert_eq!(
        inserted.scoring_request(),
        ScoringRequestPersistenceDisposition::Inserted
    );
    assert_eq!(
        inserted.scoring_job(),
        ScoringJobPersistenceDisposition::Inserted
    );
    assert_eq!(inserted.outbox(), PersistenceDisposition::Inserted);
    transaction.commit().unwrap();

    let mut transaction = client.transaction().unwrap();
    let duplicate = persist_response_snapshot_and_scoring_dispatch(
        &mut transaction,
        &snapshot,
        &request,
        &job,
        &event,
        3,
    )
    .unwrap();
    assert_eq!(
        duplicate.response_snapshot(),
        ResponseSnapshotPersistenceDisposition::Duplicate
    );
    assert_eq!(
        duplicate.scoring_request(),
        ScoringRequestPersistenceDisposition::Duplicate
    );
    assert_eq!(
        duplicate.scoring_job(),
        ScoringJobPersistenceDisposition::Duplicate
    );
    assert_eq!(duplicate.outbox(), PersistenceDisposition::Duplicate);
    transaction.commit().unwrap();

    for table in [
        "response_snapshot",
        "response_snapshot_entry",
        "scoring_request",
        "scoring_job_state",
        "integration_outbox",
    ] {
        let count: i64 = client
            .query_one(&format!("SELECT count(*) FROM {table}"), &[])
            .unwrap()
            .get(0);
        assert_eq!(count, 1, "{table} must contain one immutable row");
    }
}

#[test]
fn mismatched_snapshot_binding_fails_before_any_write() {
    let _guard = test_guard();
    let mut client = test_client();
    reset_and_migrate(&mut client);
    let (snapshot, _) = snapshot_and_request(
        "session_snapshot_dispatch_alpha",
        "response_snapshot_dispatch_alpha",
        "scoring_request_unused",
    );
    let (_, request) = snapshot_and_request(
        "session_snapshot_dispatch_other",
        "response_snapshot_dispatch_other",
        "scoring_request_snapshot_dispatch_other",
    );
    let job = ScoringJob::new(
        "scoring_job_snapshot_dispatch_other",
        request.scoring_request_ref(),
        3,
    )
    .unwrap();
    let event = dispatch_event(
        "event_snapshot_dispatch_mismatch",
        job.scoring_job_ref(),
        DIGEST_A,
    );

    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        persist_response_snapshot_and_scoring_dispatch(
            &mut transaction,
            &snapshot,
            &request,
            &job,
            &event,
            3,
        ),
        Err(SnapshotScoringPersistenceError::MismatchedSnapshotBinding)
    ));
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
fn late_outbox_conflict_rolls_back_snapshot_request_and_job() {
    let _guard = test_guard();
    let mut client = test_client();
    reset_and_migrate(&mut client);
    let (snapshot, request) = snapshot_and_request(
        "session_snapshot_dispatch_alpha",
        "response_snapshot_dispatch_alpha",
        "scoring_request_snapshot_dispatch_alpha",
    );
    let job = ScoringJob::new(
        "scoring_job_snapshot_dispatch_alpha",
        request.scoring_request_ref(),
        3,
    )
    .unwrap();
    let existing_event = dispatch_event(
        "event_snapshot_dispatch_conflict",
        job.scoring_job_ref(),
        DIGEST_A,
    );
    assert_eq!(
        enqueue_outbox_event(&mut client, &existing_event, 3).unwrap(),
        PersistenceDisposition::Inserted
    );
    let conflicting_event = dispatch_event(
        "event_snapshot_dispatch_conflict",
        job.scoring_job_ref(),
        DIGEST_B,
    );

    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        persist_response_snapshot_and_scoring_dispatch(
            &mut transaction,
            &snapshot,
            &request,
            &job,
            &conflicting_event,
            3,
        ),
        Err(SnapshotScoringPersistenceError::Dispatch(_))
    ));
    transaction.rollback().unwrap();

    for table in ["response_snapshot", "scoring_request", "scoring_job_state"] {
        let count: i64 = client
            .query_one(&format!("SELECT count(*) FROM {table}"), &[])
            .unwrap()
            .get(0);
        assert_eq!(count, 0, "{table} must roll back with the failed outbox");
    }
    let outbox_count: i64 = client
        .query_one("SELECT count(*) FROM integration_outbox", &[])
        .unwrap()
        .get(0);
    assert_eq!(outbox_count, 1, "pre-existing event evidence must remain");
}

#[test]
fn snapshot_scoring_errors_expose_typed_sources() {
    let mismatch = SnapshotScoringPersistenceError::MismatchedSnapshotBinding;
    assert!(!mismatch.to_string().is_empty());
    assert!(mismatch.source().is_none());
}
