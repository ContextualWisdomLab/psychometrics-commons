//! Real `PostgreSQL` contract: worker attempts persist the result snapshot atomically.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::postgres_integration::apply_integration_migration;
use psychometrics_commons_runtime::postgres_integration::PersistenceDisposition;
use psychometrics_commons_runtime::postgres_result_snapshot::apply_result_snapshot_migration;
use psychometrics_commons_runtime::postgres_result_snapshot::ResultSnapshotPersistenceDisposition;
use psychometrics_commons_runtime::postgres_scoring_job::{
    apply_scoring_job_migration, claim_scoring_job, persist_scoring_job,
    ScoringJobCompletionDisposition,
};
use psychometrics_commons_runtime::postgres_scoring_request::{
    apply_scoring_request_migration, persist_scoring_request,
};
use psychometrics_commons_runtime::postgres_scoring_worker::{
    run_scoring_worker_attempt_with_result_snapshot, ScoringWorkerCommitError,
    ScoringWorkerPersistence,
};
use psychometrics_commons_runtime::result::ResultSnapshotInput;
use psychometrics_commons_runtime::scoring::{
    ScoreObservation, ScoringRequest, ScoringRequestInput, ScoringResult,
};
use psychometrics_commons_runtime::scoring_job::ScoringJob;
use psychometrics_commons_runtime::scoring_worker::{
    ScoringWorkerEnvelope, ScoringWorkerResultEngine, ScoringWorkerResultOutcome,
};
use std::sync::{Mutex, MutexGuard};

const ENGINE_DIGEST: &str =
    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const PAYLOAD_DIGEST: &str =
    "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

static WORKER_SNAPSHOT_LOCK: Mutex<()> = Mutex::new(());

fn worker_snapshot_guard() -> MutexGuard<'static, ()> {
    WORKER_SNAPSHOT_LOCK
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
            "CREATE SCHEMA IF NOT EXISTS scoring_worker_result_snapshot_test;\
             SET search_path TO scoring_worker_result_snapshot_test;",
        )
        .unwrap();
    client
}

fn reset_and_migrate(client: &mut Client) {
    client
        .batch_execute(
            "DROP TABLE IF EXISTS result_snapshot_observation;\
             DROP TABLE IF EXISTS result_snapshot;\
             DROP TABLE IF EXISTS integration_delivery_attempt;\
             DROP TABLE IF EXISTS integration_inbox;\
             DROP TABLE IF EXISTS integration_outbox;\
             DROP TABLE IF EXISTS scoring_job_state;\
             DROP TABLE IF EXISTS scoring_request;",
        )
        .unwrap();
    apply_integration_migration(client).unwrap();
    apply_scoring_job_migration(client).unwrap();
    apply_scoring_request_migration(client).unwrap();
    apply_result_snapshot_migration(client).unwrap();
}

fn loaded_request() -> ScoringRequest {
    ScoringRequest::from_persisted(
        "session_worker_snapshot",
        ScoringRequestInput {
            scoring_request_ref: "scoring_request_worker_snapshot",
            response_snapshot_ref: "response_snapshot_worker_snapshot",
            assessment_spec_ref: "assessment_spec_big_five_v1",
            instrument_version_ref: "instrument_version_big_five_ko_v1",
            scoring_version_ref: "scoring_version_big_five_v1",
            calibration_reference: "calibration_big_five_ko_v1",
            norm_version_ref: Some("norm_version_big_five_ko_v1"),
            requested_output_schema_version: 1,
        },
    )
    .unwrap()
}

fn persist_request_and_claim(client: &mut Client, job_ref: &str) -> u64 {
    let request = loaded_request();
    let job = ScoringJob::new(job_ref, request.scoring_request_ref(), 3).unwrap();
    let mut transaction = client.transaction().unwrap();
    persist_scoring_request(&mut transaction, &request).unwrap();
    persist_scoring_job(&mut transaction, &job).unwrap();
    transaction.commit().unwrap();

    let mut transaction = client.transaction().unwrap();
    let lease = claim_scoring_job(
        &mut transaction,
        job_ref,
        "worker_snapshot_alpha",
        "lease_snapshot_alpha",
        10_000,
        80_000,
    )
    .unwrap();
    let fencing_token = lease.fencing_token();
    transaction.commit().unwrap();
    fencing_token
}

fn snapshot_input<'a>() -> ResultSnapshotInput<'a> {
    ResultSnapshotInput {
        result_snapshot_ref: "result_worker_snapshot",
        participant_ref: "participant_worker_snapshot",
        narrative_version_ref: "narrative_version_big_five_v1",
        consent_snapshot_refs: &["consent_snapshot_service_v1"],
        created_at_unix_ms: 70_000,
        supersedes_ref: None,
    }
}

fn worker_envelope() -> ScoringWorkerEnvelope<'static> {
    ScoringWorkerEnvelope {
        event_type: "scoring.result.completed",
        schema_version: "v1",
        source: "psychometrics_commons",
        tenant_ref: "tenant_worker_snapshot",
        occurred_at_unix_ms: 70_000,
        correlation_ref: "correlation_worker_snapshot",
        causation_ref: Some("scoring_request_worker_snapshot"),
        payload_digest: PAYLOAD_DIGEST,
    }
}

struct ScriptedResultEngine {
    result: ScoringResult,
}

impl ScoringWorkerResultEngine for ScriptedResultEngine {
    fn score_claimed_request(
        &self,
        _scoring_job_ref: &str,
        _request: &ScoringRequest,
    ) -> Result<
        ScoringWorkerResultOutcome,
        psychometrics_commons_runtime::scoring_worker::ScoringWorkerError,
    > {
        Ok(ScoringWorkerResultOutcome::Completed {
            result: Box::new(self.result.clone()),
        })
    }
}

struct RetryableResultEngine;

impl ScoringWorkerResultEngine for RetryableResultEngine {
    fn score_claimed_request(
        &self,
        _scoring_job_ref: &str,
        _request: &ScoringRequest,
    ) -> Result<
        ScoringWorkerResultOutcome,
        psychometrics_commons_runtime::scoring_worker::ScoringWorkerError,
    > {
        Ok(ScoringWorkerResultOutcome::Retryable {
            cause_code: "engine_unavailable".to_owned(),
        })
    }
}

#[test]
fn missing_scoring_request_leaves_the_job_and_snapshot_untouched() {
    let _guard = worker_snapshot_guard();
    let mut client = test_client();
    reset_and_migrate(&mut client);
    let job_ref = "scoring_job_worker_snapshot_missing";
    let job = ScoringJob::new(job_ref, "scoring_request_worker_snapshot", 3).unwrap();
    let mut transaction = client.transaction().unwrap();
    persist_scoring_job(&mut transaction, &job).unwrap();
    transaction.commit().unwrap();
    let mut transaction = client.transaction().unwrap();
    let fencing_token = claim_scoring_job(
        &mut transaction,
        job_ref,
        "worker_snapshot_missing",
        "lease_snapshot_missing",
        10_000,
        80_000,
    )
    .unwrap()
    .fencing_token();
    transaction.commit().unwrap();

    let engine = ScriptedResultEngine {
        result: ScoringResult::new(
            "result_worker_snapshot",
            &loaded_request(),
            ENGINE_DIGEST,
            vec![ScoreObservation::scored("big_five_openness", 1.2, Some(0.15)).unwrap()],
        )
        .unwrap(),
    };
    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        run_scoring_worker_attempt_with_result_snapshot(
            &mut transaction,
            job_ref,
            fencing_token,
            "scoring_request_worker_snapshot",
            &engine,
            snapshot_input(),
            worker_envelope(),
            3,
            80_000,
        ),
        Err(ScoringWorkerCommitError::MissingRequest)
    ));
    transaction.rollback().unwrap();

    let state: String = client
        .query_one(
            "SELECT scoring_state FROM scoring_job_state WHERE scoring_job_ref = $1",
            &[&job_ref],
        )
        .unwrap()
        .get(0);
    assert_eq!(state, "leased");
    let snapshots: i64 = client
        .query_one("SELECT count(*) FROM result_snapshot", &[])
        .unwrap()
        .get(0);
    assert_eq!(snapshots, 0);
}

#[test]
fn worker_persists_the_result_snapshot_with_the_terminal_job() {
    let _guard = worker_snapshot_guard();
    let mut client = test_client();
    reset_and_migrate(&mut client);
    let job_ref = "scoring_job_worker_snapshot_ok";
    let fencing_token = persist_request_and_claim(&mut client, job_ref);
    let request = loaded_request();
    let engine = ScriptedResultEngine {
        result: ScoringResult::new(
            "result_worker_snapshot",
            &request,
            ENGINE_DIGEST,
            vec![ScoreObservation::scored("big_five_openness", 1.2, Some(0.15)).unwrap()],
        )
        .unwrap(),
    };

    let mut transaction = client.transaction().unwrap();
    let inserted = run_scoring_worker_attempt_with_result_snapshot(
        &mut transaction,
        job_ref,
        fencing_token,
        request.scoring_request_ref(),
        &engine,
        snapshot_input(),
        worker_envelope(),
        3,
        80_000,
    )
    .unwrap();
    assert!(matches!(
        inserted.terminal(),
        ScoringWorkerPersistence::Completed(persistence)
            if persistence.completion() == ScoringJobCompletionDisposition::Completed
                && persistence.outbox() == PersistenceDisposition::Inserted
    ));
    assert_eq!(
        inserted.snapshot(),
        Some(ResultSnapshotPersistenceDisposition::Inserted)
    );
    transaction.commit().unwrap();

    let mut transaction = client.transaction().unwrap();
    let replayed = run_scoring_worker_attempt_with_result_snapshot(
        &mut transaction,
        job_ref,
        fencing_token,
        request.scoring_request_ref(),
        &engine,
        snapshot_input(),
        worker_envelope(),
        3,
        80_000,
    )
    .unwrap();
    assert!(matches!(
        replayed.terminal(),
        ScoringWorkerPersistence::Completed(persistence)
            if persistence.completion() == ScoringJobCompletionDisposition::Duplicate
                && persistence.outbox() == PersistenceDisposition::Duplicate
    ));
    assert_eq!(
        replayed.snapshot(),
        Some(ResultSnapshotPersistenceDisposition::Duplicate)
    );
    transaction.commit().unwrap();

    let row = client
        .query_one(
            "SELECT scoring_state, result_ref FROM scoring_job_state WHERE scoring_job_ref = $1",
            &[&job_ref],
        )
        .unwrap();
    let state: String = row.get(0);
    let result_ref: Option<String> = row.get(1);
    assert_eq!(state, "completed");
    assert_eq!(result_ref.as_deref(), Some("result_worker_snapshot"));
    let snapshots: i64 = client
        .query_one(
            "SELECT count(*) FROM result_snapshot WHERE result_snapshot_ref = $1",
            &[&"result_worker_snapshot"],
        )
        .unwrap()
        .get(0);
    assert_eq!(snapshots, 1);
    let score: f64 = client
        .query_one(
            "SELECT score FROM result_snapshot_observation \
             WHERE result_snapshot_ref = $1 AND construct_ref = $2",
            &[&"result_worker_snapshot", &"big_five_openness"],
        )
        .unwrap()
        .get(0);
    assert!((score - 1.2).abs() < f64::EPSILON);
}

#[test]
fn retryable_engine_outage_schedules_retry_without_a_terminal_event_or_score() {
    let _guard = worker_snapshot_guard();
    let mut client = test_client();
    reset_and_migrate(&mut client);
    let job_ref = "scoring_job_worker_snapshot_retry";
    let fencing_token = persist_request_and_claim(&mut client, job_ref);

    let mut transaction = client.transaction().unwrap();
    let scheduled = run_scoring_worker_attempt_with_result_snapshot(
        &mut transaction,
        job_ref,
        fencing_token,
        "scoring_request_worker_snapshot",
        &RetryableResultEngine,
        snapshot_input(),
        worker_envelope(),
        3,
        80_000,
    )
    .unwrap();
    assert_eq!(
        scheduled.terminal(),
        ScoringWorkerPersistence::RetryScheduled
    );
    assert_eq!(scheduled.snapshot(), None);
    transaction.commit().unwrap();

    let row = client
        .query_one(
            "SELECT scoring_state, result_ref, last_failure_code, next_attempt_at_unix_ms, \
                    active_worker_ref, active_lease_ref \
             FROM scoring_job_state WHERE scoring_job_ref = $1",
            &[&job_ref],
        )
        .unwrap();
    let state: String = row.get(0);
    let result_ref: Option<String> = row.get(1);
    let cause: Option<String> = row.get(2);
    let retry_at: Option<i64> = row.get(3);
    let worker: Option<String> = row.get(4);
    let lease: Option<String> = row.get(5);
    assert_eq!(state, "retry_scheduled");
    assert_eq!(result_ref, None);
    assert_eq!(cause.as_deref(), Some("engine_unavailable"));
    assert_eq!(retry_at, Some(80_000));
    assert_eq!(worker, None);
    assert_eq!(lease, None);
    let snapshots: i64 = client
        .query_one("SELECT count(*) FROM result_snapshot", &[])
        .unwrap()
        .get(0);
    assert_eq!(snapshots, 0);
    let outbox: i64 = client
        .query_one("SELECT count(*) FROM integration_outbox", &[])
        .unwrap()
        .get(0);
    assert_eq!(outbox, 0);
}
