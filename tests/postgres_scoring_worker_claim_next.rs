//! Real `PostgreSQL` contract: claim a due job and drive the request-bound snapshot worker.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::postgres_integration::apply_integration_migration;
use psychometrics_commons_runtime::postgres_integration::PersistenceDisposition;
use psychometrics_commons_runtime::postgres_result_snapshot::apply_result_snapshot_migration;
use psychometrics_commons_runtime::postgres_result_snapshot::ResultSnapshotPersistenceDisposition;
use psychometrics_commons_runtime::postgres_scoring_job::{
    apply_scoring_job_migration, persist_scoring_job, ScoringJobCompletionDisposition,
    ScoringJobPersistenceError,
};
use psychometrics_commons_runtime::postgres_scoring_request::{
    apply_scoring_request_migration, persist_scoring_request,
};
use psychometrics_commons_runtime::postgres_scoring_worker::{
    claim_and_run_scoring_worker_attempt_with_result_snapshot, ScoringWorkerCommitError,
    ScoringWorkerPersistence,
};
use psychometrics_commons_runtime::result::ResultSnapshotInput;
use psychometrics_commons_runtime::scoring::{
    ScoreObservation, ScoringRequest, ScoringRequestInput, ScoringResult,
};
use psychometrics_commons_runtime::scoring_job::ScoringJob;
use psychometrics_commons_runtime::scoring_worker::{
    ScoringWorkerEnvelope, ScoringWorkerError, ScoringWorkerResultEngine,
    ScoringWorkerResultOutcome,
};
use std::cell::Cell;
use std::sync::{Mutex, MutexGuard};

const ENGINE_DIGEST: &str =
    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const PAYLOAD_DIGEST: &str =
    "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

static CLAIM_NEXT_LOCK: Mutex<()> = Mutex::new(());

fn claim_next_guard() -> MutexGuard<'static, ()> {
    CLAIM_NEXT_LOCK
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
            "CREATE SCHEMA IF NOT EXISTS scoring_worker_claim_next_test;\
             SET search_path TO scoring_worker_claim_next_test;",
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
        "session_worker_claim_next",
        ScoringRequestInput {
            scoring_request_ref: "scoring_request_worker_claim_next",
            response_snapshot_ref: "response_snapshot_worker_claim_next",
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

fn persist_queued_job(client: &mut Client, job_ref: &str) {
    let request = loaded_request();
    let job = ScoringJob::new(job_ref, request.scoring_request_ref(), 3).unwrap();
    let mut transaction = client.transaction().unwrap();
    persist_scoring_request(&mut transaction, &request).unwrap();
    persist_scoring_job(&mut transaction, &job).unwrap();
    transaction.commit().unwrap();
}

fn snapshot_input<'a>() -> ResultSnapshotInput<'a> {
    ResultSnapshotInput {
        result_snapshot_ref: "result_worker_claim_next",
        participant_ref: "participant_worker_claim_next",
        narrative_version_ref: "narrative_version_big_five_v1",
        consent_snapshot_refs: &["consent_snapshot_service_v1"],
        created_at_unix_ms: 20_000,
        supersedes_ref: None,
    }
}

fn worker_envelope() -> ScoringWorkerEnvelope<'static> {
    ScoringWorkerEnvelope {
        event_type: "scoring.result.completed",
        schema_version: "v1",
        source: "psychometrics_commons",
        tenant_ref: "tenant_worker_claim_next",
        occurred_at_unix_ms: 20_000,
        correlation_ref: "correlation_worker_claim_next",
        causation_ref: Some("scoring_request_worker_claim_next"),
        payload_digest: PAYLOAD_DIGEST,
    }
}

struct CountingResultEngine {
    outcome: ScoringWorkerResultOutcome,
    calls: Cell<u32>,
}

impl ScoringWorkerResultEngine for CountingResultEngine {
    fn score_claimed_request(
        &self,
        _scoring_job_ref: &str,
        _request: &ScoringRequest,
    ) -> Result<ScoringWorkerResultOutcome, ScoringWorkerError> {
        self.calls.set(self.calls.get() + 1);
        Ok(self.outcome.clone())
    }
}

fn completed_engine() -> CountingResultEngine {
    let request = loaded_request();
    CountingResultEngine {
        outcome: ScoringWorkerResultOutcome::Completed {
            result: Box::new(
                ScoringResult::new(
                    "result_worker_claim_next",
                    &request,
                    ENGINE_DIGEST,
                    vec![ScoreObservation::scored("big_five_openness", 1.2, Some(0.15)).unwrap()],
                )
                .unwrap(),
            ),
        },
        calls: Cell::new(0),
    }
}

fn retryable_engine() -> CountingResultEngine {
    CountingResultEngine {
        outcome: ScoringWorkerResultOutcome::Retryable {
            cause_code: "engine_unavailable".to_owned(),
        },
        calls: Cell::new(0),
    }
}

fn job_state(client: &mut Client, job_ref: &str) -> (String, Option<String>, Option<String>) {
    let row = client
        .query_one(
            "SELECT scoring_state, result_ref, last_failure_code \
             FROM scoring_job_state WHERE scoring_job_ref = $1",
            &[&job_ref],
        )
        .unwrap();
    (row.get(0), row.get(1), row.get(2))
}

fn snapshot_count(client: &mut Client) -> i64 {
    client
        .query_one("SELECT count(*) FROM result_snapshot", &[])
        .unwrap()
        .get(0)
}

fn outbox_count(client: &mut Client) -> i64 {
    client
        .query_one("SELECT count(*) FROM integration_outbox", &[])
        .unwrap()
        .get(0)
}

#[test]
fn claim_next_persists_the_snapshot_from_a_queued_job() {
    let _guard = claim_next_guard();
    let mut client = test_client();
    reset_and_migrate(&mut client);
    let job_ref = "scoring_job_worker_claim_next_ok";
    persist_queued_job(&mut client, job_ref);
    let engine = completed_engine();

    let mut transaction = client.transaction().unwrap();
    let inserted = claim_and_run_scoring_worker_attempt_with_result_snapshot(
        &mut transaction,
        job_ref,
        "worker_claim_next_alpha",
        "lease_claim_next_alpha",
        10_000,
        30_000,
        &engine,
        snapshot_input(),
        worker_envelope(),
        3,
        25_000,
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
    assert_eq!(engine.calls.get(), 1);
    transaction.commit().unwrap();

    let (state, result_ref, cause) = job_state(&mut client, job_ref);
    assert_eq!(state, "completed");
    assert_eq!(result_ref.as_deref(), Some("result_worker_claim_next"));
    assert_eq!(cause, None);
    assert_eq!(snapshot_count(&mut client), 1);
    assert_eq!(outbox_count(&mut client), 1);
    let score: f64 = client
        .query_one(
            "SELECT score FROM result_snapshot_observation \
             WHERE result_snapshot_ref = $1 AND construct_ref = $2",
            &[&"result_worker_claim_next", &"big_five_openness"],
        )
        .unwrap()
        .get(0);
    assert!((score - 1.2).abs() < f64::EPSILON);
}

#[test]
fn claim_next_recovers_a_due_retryable_outage_into_a_real_snapshot() {
    let _guard = claim_next_guard();
    let mut client = test_client();
    reset_and_migrate(&mut client);
    let job_ref = "scoring_job_worker_claim_next_recover";
    persist_queued_job(&mut client, job_ref);

    let mut transaction = client.transaction().unwrap();
    let scheduled = claim_and_run_scoring_worker_attempt_with_result_snapshot(
        &mut transaction,
        job_ref,
        "worker_claim_next_retry",
        "lease_claim_next_retry",
        10_000,
        30_000,
        &retryable_engine(),
        snapshot_input(),
        worker_envelope(),
        3,
        25_000,
    )
    .unwrap();
    assert_eq!(
        scheduled.terminal(),
        ScoringWorkerPersistence::RetryScheduled
    );
    assert_eq!(scheduled.snapshot(), None);
    transaction.commit().unwrap();

    let (state, result_ref, cause) = job_state(&mut client, job_ref);
    assert_eq!(state, "retry_scheduled");
    assert_eq!(result_ref, None);
    assert_eq!(cause.as_deref(), Some("engine_unavailable"));
    assert_eq!(snapshot_count(&mut client), 0);
    assert_eq!(outbox_count(&mut client), 0);

    let engine = completed_engine();
    let mut transaction = client.transaction().unwrap();
    let recovered = claim_and_run_scoring_worker_attempt_with_result_snapshot(
        &mut transaction,
        job_ref,
        "worker_claim_next_recover",
        "lease_claim_next_recover",
        25_000,
        45_000,
        &engine,
        snapshot_input(),
        worker_envelope(),
        3,
        40_000,
    )
    .unwrap();
    assert!(matches!(
        recovered.terminal(),
        ScoringWorkerPersistence::Completed(persistence)
            if persistence.completion() == ScoringJobCompletionDisposition::Completed
                && persistence.outbox() == PersistenceDisposition::Inserted
    ));
    assert_eq!(
        recovered.snapshot(),
        Some(ResultSnapshotPersistenceDisposition::Inserted)
    );
    assert_eq!(engine.calls.get(), 1);
    transaction.commit().unwrap();

    let (state, result_ref, cause) = job_state(&mut client, job_ref);
    assert_eq!(state, "completed");
    assert_eq!(result_ref.as_deref(), Some("result_worker_claim_next"));
    assert_eq!(cause, None);
    assert_eq!(snapshot_count(&mut client), 1);
    assert_eq!(outbox_count(&mut client), 1);
}

#[test]
fn claim_next_before_the_retry_instant_leaves_the_schedule_untouched() {
    let _guard = claim_next_guard();
    let mut client = test_client();
    reset_and_migrate(&mut client);
    let job_ref = "scoring_job_worker_claim_next_early";
    persist_queued_job(&mut client, job_ref);

    let mut transaction = client.transaction().unwrap();
    claim_and_run_scoring_worker_attempt_with_result_snapshot(
        &mut transaction,
        job_ref,
        "worker_claim_next_early_first",
        "lease_claim_next_early_first",
        10_000,
        30_000,
        &retryable_engine(),
        snapshot_input(),
        worker_envelope(),
        3,
        25_000,
    )
    .unwrap();
    transaction.commit().unwrap();

    let engine = completed_engine();
    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        claim_and_run_scoring_worker_attempt_with_result_snapshot(
            &mut transaction,
            job_ref,
            "worker_claim_next_early_second",
            "lease_claim_next_early_second",
            20_000,
            40_000,
            &engine,
            snapshot_input(),
            worker_envelope(),
            3,
            35_000,
        ),
        Err(ScoringWorkerCommitError::Claim(
            ScoringJobPersistenceError::LeaseNotDue
        ))
    ));
    transaction.rollback().unwrap();

    assert_eq!(engine.calls.get(), 0);
    let (state, result_ref, cause) = job_state(&mut client, job_ref);
    assert_eq!(state, "retry_scheduled");
    assert_eq!(result_ref, None);
    assert_eq!(cause.as_deref(), Some("engine_unavailable"));
    assert_eq!(snapshot_count(&mut client), 0);
    assert_eq!(outbox_count(&mut client), 0);
}

#[test]
fn claim_next_missing_job_does_not_run_the_engine() {
    let _guard = claim_next_guard();
    let mut client = test_client();
    reset_and_migrate(&mut client);
    let request = loaded_request();
    let mut transaction = client.transaction().unwrap();
    persist_scoring_request(&mut transaction, &request).unwrap();
    transaction.commit().unwrap();
    let engine = completed_engine();

    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        claim_and_run_scoring_worker_attempt_with_result_snapshot(
            &mut transaction,
            "scoring_job_worker_claim_next_missing",
            "worker_claim_next_missing",
            "lease_claim_next_missing",
            10_000,
            30_000,
            &engine,
            snapshot_input(),
            worker_envelope(),
            3,
            25_000,
        ),
        Err(ScoringWorkerCommitError::Claim(
            ScoringJobPersistenceError::JobNotFound
        ))
    ));
    transaction.rollback().unwrap();

    assert_eq!(engine.calls.get(), 0);
    assert_eq!(snapshot_count(&mut client), 0);
    assert_eq!(outbox_count(&mut client), 0);
}
