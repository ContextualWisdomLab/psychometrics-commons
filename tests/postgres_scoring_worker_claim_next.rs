//! Real `PostgreSQL` contract: claim-next runs the worker with the stored request pin.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::postgres_integration::apply_integration_migration;
use psychometrics_commons_runtime::postgres_result_snapshot::apply_result_snapshot_migration;
use psychometrics_commons_runtime::postgres_scoring_job::{
    apply_scoring_job_migration, persist_scoring_job, ScoringJobPersistenceError,
};
use psychometrics_commons_runtime::postgres_scoring_request::{
    apply_scoring_request_migration, persist_scoring_request,
};
use psychometrics_commons_runtime::postgres_scoring_worker::{
    claim_and_run_next_scoring_worker_attempt, ScoringWorkerCommitError, ScoringWorkerPersistence,
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
use std::sync::{Mutex, MutexGuard};

const ENGINE_DIGEST: &str =
    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const PAYLOAD_DIGEST: &str =
    "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

static WORKER_CLAIM_NEXT_LOCK: Mutex<()> = Mutex::new(());

fn worker_claim_next_guard() -> MutexGuard<'static, ()> {
    WORKER_CLAIM_NEXT_LOCK
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

fn request_named(suffix: &str) -> ScoringRequest {
    ScoringRequest::from_persisted(
        &format!("session_claim_next_{suffix}"),
        ScoringRequestInput {
            scoring_request_ref: &format!("scoring_request_claim_next_{suffix}"),
            response_snapshot_ref: &format!("response_snapshot_claim_next_{suffix}"),
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

fn persist_request_and_job(client: &mut Client, suffix: &str) -> ScoringRequest {
    let request = request_named(suffix);
    let job = ScoringJob::new(
        format!("scoring_job_claim_next_{suffix}"),
        request.scoring_request_ref(),
        3,
    )
    .unwrap();
    let mut transaction = client.transaction().unwrap();
    persist_scoring_request(&mut transaction, &request).unwrap();
    persist_scoring_job(&mut transaction, &job).unwrap();
    transaction.commit().unwrap();
    request
}

fn snapshot_input_for(suffix: &str, created_at_unix_ms: u64) -> ResultSnapshotInput<'_> {
    ResultSnapshotInput {
        result_snapshot_ref: match suffix {
            "older" => "result_claim_next_older",
            "newer" => "result_claim_next_newer",
            _ => "result_claim_next_recover",
        },
        participant_ref: "participant_claim_next",
        narrative_version_ref: "narrative_version_big_five_v1",
        consent_snapshot_refs: &["consent_snapshot_service_v1"],
        created_at_unix_ms,
        supersedes_ref: None,
    }
}

fn worker_envelope_at(
    occurred_at_unix_ms: u64,
    causation_ref: &'static str,
) -> ScoringWorkerEnvelope<'static> {
    ScoringWorkerEnvelope {
        event_type: "scoring.result.completed",
        schema_version: "v1",
        source: "psychometrics_commons",
        tenant_ref: "tenant_claim_next",
        occurred_at_unix_ms,
        correlation_ref: "correlation_claim_next",
        causation_ref: Some(causation_ref),
        payload_digest: PAYLOAD_DIGEST,
    }
}

struct RequestBoundEngine;

impl ScoringWorkerResultEngine for RequestBoundEngine {
    fn score_claimed_request(
        &self,
        _scoring_job_ref: &str,
        request: &ScoringRequest,
    ) -> Result<ScoringWorkerResultOutcome, ScoringWorkerError> {
        let result_ref = if request.scoring_request_ref().ends_with("_older") {
            "result_claim_next_older"
        } else if request.scoring_request_ref().ends_with("_newer") {
            "result_claim_next_newer"
        } else {
            "result_claim_next_recover"
        };
        Ok(ScoringWorkerResultOutcome::Completed {
            result: Box::new(
                ScoringResult::new(
                    result_ref,
                    request,
                    ENGINE_DIGEST,
                    vec![ScoreObservation::scored("big_five_openness", 1.2, Some(0.15)).unwrap()],
                )
                .unwrap(),
            ),
        })
    }
}

struct RetryableResultEngine;

impl ScoringWorkerResultEngine for RetryableResultEngine {
    fn score_claimed_request(
        &self,
        _scoring_job_ref: &str,
        _request: &ScoringRequest,
    ) -> Result<ScoringWorkerResultOutcome, ScoringWorkerError> {
        Ok(ScoringWorkerResultOutcome::Retryable {
            cause_code: "engine_unavailable".to_owned(),
        })
    }
}

#[test]
fn claim_next_scores_the_oldest_job_using_its_stored_request_pin() {
    let _guard = worker_claim_next_guard();
    let mut client = test_client();
    reset_and_migrate(&mut client);
    let older = persist_request_and_job(&mut client, "older");
    persist_request_and_job(&mut client, "newer");

    let mut transaction = client.transaction().unwrap();
    let claimed = claim_and_run_next_scoring_worker_attempt(
        &mut transaction,
        "worker_claim_next_older",
        "lease_claim_next_older",
        20_000,
        80_000,
        &RequestBoundEngine,
        snapshot_input_for("older", 70_000),
        worker_envelope_at(70_000, "scoring_request_claim_next_older"),
        3,
        90_000,
    )
    .unwrap();
    transaction.commit().unwrap();

    assert_eq!(claimed.scoring_job_ref(), "scoring_job_claim_next_older");
    assert_eq!(claimed.scoring_request_ref(), older.scoring_request_ref());
    assert_eq!(claimed.fencing_token(), 1);
    assert!(matches!(
        claimed.persistence().terminal(),
        ScoringWorkerPersistence::Completed(_)
    ));
    assert!(claimed.persistence().snapshot().is_some());

    let row = client
        .query_one(
            "SELECT scoring_state, result_ref FROM scoring_job_state \
             WHERE scoring_job_ref = $1",
            &[&"scoring_job_claim_next_older"],
        )
        .unwrap();
    let state: String = row.get(0);
    let result_ref: Option<String> = row.get(1);
    assert_eq!(state, "completed");
    assert_eq!(result_ref.as_deref(), Some("result_claim_next_older"));

    let newer_state: String = client
        .query_one(
            "SELECT scoring_state FROM scoring_job_state WHERE scoring_job_ref = $1",
            &[&"scoring_job_claim_next_newer"],
        )
        .unwrap()
        .get(0);
    assert_eq!(newer_state, "queued");

    let snapshots: i64 = client
        .query_one(
            "SELECT count(*) FROM result_snapshot WHERE result_snapshot_ref = $1",
            &[&"result_claim_next_older"],
        )
        .unwrap()
        .get(0);
    assert_eq!(snapshots, 1);
    let score: f64 = client
        .query_one(
            "SELECT score FROM result_snapshot_observation \
             WHERE result_snapshot_ref = $1 AND construct_ref = $2",
            &[&"result_claim_next_older", &"big_five_openness"],
        )
        .unwrap()
        .get(0);
    assert!((score - 1.2).abs() < f64::EPSILON);
}

#[test]
fn later_due_claim_next_recovers_the_real_snapshot_from_the_stored_pin() {
    let _guard = worker_claim_next_guard();
    let mut client = test_client();
    reset_and_migrate(&mut client);
    persist_request_and_job(&mut client, "recover");

    let mut transaction = client.transaction().unwrap();
    let scheduled = claim_and_run_next_scoring_worker_attempt(
        &mut transaction,
        "worker_claim_next_outage",
        "lease_claim_next_outage",
        20_000,
        30_000,
        &RetryableResultEngine,
        snapshot_input_for("recover", 25_000),
        worker_envelope_at(25_000, "scoring_request_claim_next_recover"),
        3,
        40_000,
    )
    .unwrap();
    assert_eq!(
        scheduled.persistence().terminal(),
        ScoringWorkerPersistence::RetryScheduled
    );
    assert_eq!(scheduled.persistence().snapshot(), None);
    transaction.commit().unwrap();

    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        claim_and_run_next_scoring_worker_attempt(
            &mut transaction,
            "worker_claim_next_early",
            "lease_claim_next_early",
            30_000,
            50_000,
            &RequestBoundEngine,
            snapshot_input_for("recover", 45_000),
            worker_envelope_at(45_000, "scoring_request_claim_next_recover"),
            3,
            60_000,
        ),
        Err(ScoringWorkerCommitError::Claim(
            ScoringJobPersistenceError::NoDueJob
        ))
    ));
    transaction.rollback().unwrap();

    let mut transaction = client.transaction().unwrap();
    let recovered = claim_and_run_next_scoring_worker_attempt(
        &mut transaction,
        "worker_claim_next_recover",
        "lease_claim_next_recover",
        40_000,
        80_000,
        &RequestBoundEngine,
        snapshot_input_for("recover", 70_000),
        worker_envelope_at(70_000, "scoring_request_claim_next_recover"),
        3,
        90_000,
    )
    .unwrap();
    transaction.commit().unwrap();

    assert_eq!(
        recovered.scoring_job_ref(),
        "scoring_job_claim_next_recover"
    );
    assert_eq!(
        recovered.scoring_request_ref(),
        "scoring_request_claim_next_recover"
    );
    assert_eq!(recovered.fencing_token(), 2);
    assert!(recovered.persistence().snapshot().is_some());

    let state: String = client
        .query_one(
            "SELECT scoring_state FROM scoring_job_state WHERE scoring_job_ref = $1",
            &[&"scoring_job_claim_next_recover"],
        )
        .unwrap()
        .get(0);
    assert_eq!(state, "completed");
    let snapshots: i64 = client
        .query_one("SELECT count(*) FROM result_snapshot", &[])
        .unwrap()
        .get(0);
    assert_eq!(snapshots, 1);
    let outbox: i64 = client
        .query_one("SELECT count(*) FROM integration_outbox", &[])
        .unwrap()
        .get(0);
    assert_eq!(outbox, 1);
}

#[test]
fn empty_claim_next_queue_does_not_invent_a_score() {
    let _guard = worker_claim_next_guard();
    let mut client = test_client();
    reset_and_migrate(&mut client);

    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        claim_and_run_next_scoring_worker_attempt(
            &mut transaction,
            "worker_claim_next_empty",
            "lease_claim_next_empty",
            20_000,
            80_000,
            &RequestBoundEngine,
            snapshot_input_for("older", 70_000),
            worker_envelope_at(70_000, "scoring_request_claim_next_older"),
            3,
            90_000,
        ),
        Err(ScoringWorkerCommitError::Claim(
            ScoringJobPersistenceError::NoDueJob
        ))
    ));
    transaction.rollback().unwrap();

    let jobs: i64 = client
        .query_one("SELECT count(*) FROM scoring_job_state", &[])
        .unwrap()
        .get(0);
    assert_eq!(jobs, 0);
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
