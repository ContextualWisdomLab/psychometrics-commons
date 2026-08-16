//! Real `PostgreSQL` contract: claim-next runs only the request-bound snapshot worker.
//!
//! After a buyer finishes an assessment, the worker must pick the oldest due job,
//! reconstruct the stored scoring request, and persist the snapshot plus one
//! terminal outbox row. It must not accept a caller-supplied request pin.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::postgres_integration::apply_integration_migration;
use psychometrics_commons_runtime::postgres_integration::PersistenceDisposition;
use psychometrics_commons_runtime::postgres_result_snapshot::{
    apply_result_snapshot_migration, ResultSnapshotPersistenceDisposition,
};
use psychometrics_commons_runtime::postgres_scoring_job::{
    apply_scoring_job_migration, persist_scoring_job, ScoringJobCompletionDisposition,
    ScoringJobPersistenceError,
};
use psychometrics_commons_runtime::postgres_scoring_request::{
    apply_scoring_request_migration, persist_scoring_request,
};
use psychometrics_commons_runtime::postgres_scoring_worker::{
    run_next_due_scoring_worker_attempt_with_result_snapshot, ScoringWorkerCommitError,
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
use std::sync::{Mutex, MutexGuard};

const ENGINE_DIGEST: &str =
    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const PAYLOAD_DIGEST: &str =
    "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

static CLAIM_NEXT_WORKER_LOCK: Mutex<()> = Mutex::new(());

fn claim_next_worker_guard() -> MutexGuard<'static, ()> {
    CLAIM_NEXT_WORKER_LOCK
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

fn request_for(request_ref: &'static str, session_ref: &'static str) -> ScoringRequest {
    ScoringRequest::from_persisted(
        session_ref,
        ScoringRequestInput {
            scoring_request_ref: request_ref,
            response_snapshot_ref: "response_snapshot_claim_next",
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

fn persist_request_and_job(client: &mut Client, job_ref: &str, request: &ScoringRequest) {
    let job = ScoringJob::new(job_ref, request.scoring_request_ref(), 3).unwrap();
    let mut transaction = client.transaction().unwrap();
    persist_scoring_request(&mut transaction, request).unwrap();
    persist_scoring_job(&mut transaction, &job).unwrap();
    transaction.commit().unwrap();
}

fn snapshot_input(result_ref: &'static str) -> ResultSnapshotInput<'static> {
    ResultSnapshotInput {
        result_snapshot_ref: result_ref,
        participant_ref: "participant_claim_next",
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
        tenant_ref: "tenant_claim_next",
        occurred_at_unix_ms: 20_000,
        correlation_ref: "correlation_claim_next",
        causation_ref: Some("scoring_request_claim_next_alpha"),
        payload_digest: PAYLOAD_DIGEST,
    }
}

struct RequestBoundEngine {
    expected_request_ref: String,
    result: ScoringResult,
}

impl ScoringWorkerResultEngine for RequestBoundEngine {
    fn score_claimed_request(
        &self,
        _scoring_job_ref: &str,
        request: &ScoringRequest,
    ) -> Result<ScoringWorkerResultOutcome, ScoringWorkerError> {
        assert_eq!(
            request.scoring_request_ref(),
            self.expected_request_ref,
            "claim-next must score the stored request, not a caller-supplied pin"
        );
        Ok(ScoringWorkerResultOutcome::Completed {
            result: Box::new(self.result.clone()),
        })
    }
}

fn job_state(client: &mut Client, job_ref: &str) -> (String, Option<String>) {
    let row = client
        .query_one(
            "SELECT scoring_state, result_ref FROM scoring_job_state WHERE scoring_job_ref = $1",
            &[&job_ref],
        )
        .unwrap();
    (row.get(0), row.get(1))
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
fn empty_queue_does_not_invent_a_score() {
    let _guard = claim_next_worker_guard();
    let mut client = test_client();
    reset_and_migrate(&mut client);
    let request = request_for(
        "scoring_request_claim_next_empty",
        "session_claim_next_empty",
    );
    let engine = RequestBoundEngine {
        expected_request_ref: request.scoring_request_ref().to_owned(),
        result: ScoringResult::new(
            "result_claim_next_empty",
            &request,
            ENGINE_DIGEST,
            vec![ScoreObservation::scored("big_five_openness", 1.2, Some(0.15)).unwrap()],
        )
        .unwrap(),
    };

    let mut transaction = client.transaction().unwrap();
    assert!(run_next_due_scoring_worker_attempt_with_result_snapshot(
        &mut transaction,
        "worker_claim_next_empty",
        "lease_claim_next_empty",
        10_000,
        30_000,
        &engine,
        snapshot_input("result_claim_next_empty"),
        worker_envelope(),
        3,
        40_000,
    )
    .unwrap()
    .is_none());
    transaction.commit().unwrap();
    assert_eq!(snapshot_count(&mut client), 0);
    assert_eq!(outbox_count(&mut client), 0);
}

#[test]
fn invalid_claim_window_does_not_invent_a_score() {
    let _guard = claim_next_worker_guard();
    let mut client = test_client();
    reset_and_migrate(&mut client);
    let request = request_for(
        "scoring_request_claim_next_window",
        "session_claim_next_window",
    );
    persist_request_and_job(&mut client, "scoring_job_claim_next_window", &request);
    let engine = RequestBoundEngine {
        expected_request_ref: request.scoring_request_ref().to_owned(),
        result: ScoringResult::new(
            "result_claim_next_window",
            &request,
            ENGINE_DIGEST,
            vec![ScoreObservation::scored("big_five_openness", 1.2, Some(0.15)).unwrap()],
        )
        .unwrap(),
    };

    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        run_next_due_scoring_worker_attempt_with_result_snapshot(
            &mut transaction,
            "worker_claim_next_window",
            "lease_claim_next_window",
            20_000,
            20_000,
            &engine,
            snapshot_input("result_claim_next_window"),
            worker_envelope(),
            3,
            40_000,
        ),
        Err(ScoringWorkerCommitError::Claim(
            ScoringJobPersistenceError::InvalidLeaseWindow
        ))
    ));
    transaction.rollback().unwrap();
    let (state, result_ref) = job_state(&mut client, "scoring_job_claim_next_window");
    assert_eq!(state, "queued");
    assert_eq!(result_ref, None);
    assert_eq!(snapshot_count(&mut client), 0);
    assert_eq!(outbox_count(&mut client), 0);
}

#[test]
fn oldest_due_job_persists_the_snapshot_from_its_stored_request() {
    let _guard = claim_next_worker_guard();
    let mut client = test_client();
    reset_and_migrate(&mut client);
    let older = request_for(
        "scoring_request_claim_next_alpha",
        "session_claim_next_alpha",
    );
    let newer = request_for("scoring_request_claim_next_beta", "session_claim_next_beta");
    persist_request_and_job(&mut client, "scoring_job_claim_next_alpha", &older);
    persist_request_and_job(&mut client, "scoring_job_claim_next_beta", &newer);
    let engine = RequestBoundEngine {
        expected_request_ref: older.scoring_request_ref().to_owned(),
        result: ScoringResult::new(
            "result_claim_next_alpha",
            &older,
            ENGINE_DIGEST,
            vec![ScoreObservation::scored("big_five_openness", 1.2, Some(0.15)).unwrap()],
        )
        .unwrap(),
    };

    let mut transaction = client.transaction().unwrap();
    let attempted = run_next_due_scoring_worker_attempt_with_result_snapshot(
        &mut transaction,
        "worker_claim_next_run",
        "lease_claim_next_run",
        10_000,
        30_000,
        &engine,
        snapshot_input("result_claim_next_alpha"),
        worker_envelope(),
        3,
        40_000,
    )
    .unwrap()
    .expect("oldest due job must be claimed and scored");
    transaction.commit().unwrap();

    assert_eq!(
        attempted.claimed().scoring_job_ref(),
        "scoring_job_claim_next_alpha"
    );
    assert_eq!(
        attempted.claimed().scoring_request_ref(),
        "scoring_request_claim_next_alpha"
    );
    assert!(matches!(
        attempted.persistence().terminal(),
        ScoringWorkerPersistence::Completed(persistence)
            if persistence.completion() == ScoringJobCompletionDisposition::Completed
                && persistence.outbox() == PersistenceDisposition::Inserted
    ));
    assert_eq!(
        attempted.persistence().snapshot(),
        Some(ResultSnapshotPersistenceDisposition::Inserted)
    );

    let (older_state, older_result) = job_state(&mut client, "scoring_job_claim_next_alpha");
    let (newer_state, newer_result) = job_state(&mut client, "scoring_job_claim_next_beta");
    assert_eq!(older_state, "completed");
    assert_eq!(older_result.as_deref(), Some("result_claim_next_alpha"));
    assert_eq!(newer_state, "queued");
    assert_eq!(newer_result, None);
    assert_eq!(snapshot_count(&mut client), 1);
    assert_eq!(outbox_count(&mut client), 1);
    let score: f64 = client
        .query_one(
            "SELECT score FROM result_snapshot_observation \
             WHERE result_snapshot_ref = $1 AND construct_ref = $2",
            &[&"result_claim_next_alpha", &"big_five_openness"],
        )
        .unwrap()
        .get(0);
    assert!((score - 1.2).abs() < f64::EPSILON);
}
