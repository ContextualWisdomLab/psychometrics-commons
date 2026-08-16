//! Real `PostgreSQL` contract for scoring-worker terminal commits with a stable event identity.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::integration::IntegrationEvent;
use psychometrics_commons_runtime::postgres_integration::{
    apply_integration_migration, PersistenceDisposition,
};
use psychometrics_commons_runtime::postgres_scoring_completion::ScoringCompletionOutboxError;
use psychometrics_commons_runtime::postgres_scoring_failure::ScoringFailureOutboxError;
use psychometrics_commons_runtime::postgres_scoring_job::{
    apply_scoring_job_migration, claim_scoring_job, persist_scoring_job,
    ScoringJobCompletionDisposition, ScoringJobFailureDisposition, ScoringJobPersistenceError,
};
use psychometrics_commons_runtime::postgres_scoring_worker::{
    commit_scoring_worker_outcome, run_scoring_worker_attempt, ScoringWorkerCommitError,
    ScoringWorkerOutcome, ScoringWorkerPersistence,
};
use psychometrics_commons_runtime::scoring_job::ScoringJob;
use psychometrics_commons_runtime::scoring_worker::{
    scoring_terminal_event_ref, ScoringTerminalIdentity, ScoringWorkerEngine,
    ScoringWorkerEngineOutcome, ScoringWorkerEnvelope, ScoringWorkerError,
};
use std::error::Error;
use std::sync::{Mutex, MutexGuard};

const DIGEST: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

static WORKER_TEST_LOCK: Mutex<()> = Mutex::new(());

fn worker_test_guard() -> MutexGuard<'static, ()> {
    WORKER_TEST_LOCK
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
            "CREATE SCHEMA IF NOT EXISTS scoring_worker_terminal_test;\
             SET search_path TO scoring_worker_terminal_test;",
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
             DROP TABLE IF EXISTS scoring_job_state;",
        )
        .unwrap();
    apply_integration_migration(client).unwrap();
    apply_scoring_job_migration(client).unwrap();
}

fn persist_and_claim(client: &mut Client, job_ref: &str, request_ref: &str) -> u64 {
    let job = ScoringJob::new(job_ref, request_ref, 3).unwrap();
    let mut transaction = client.transaction().unwrap();
    persist_scoring_job(&mut transaction, &job).unwrap();
    transaction.commit().unwrap();

    let mut transaction = client.transaction().unwrap();
    let lease = claim_scoring_job(
        &mut transaction,
        job_ref,
        "worker_terminal_alpha",
        "lease_terminal_alpha",
        10_000,
        30_000,
    )
    .unwrap();
    let fencing_token = lease.fencing_token();
    transaction.commit().unwrap();
    fencing_token
}

fn terminal_event(event_ref: &str, event_type: &str, job_ref: &str) -> IntegrationEvent {
    IntegrationEvent::new(
        event_ref,
        event_type,
        "v1",
        "psychometrics_commons",
        "tenant_worker_terminal",
        job_ref,
        20_000,
        "correlation_worker_terminal",
        Some("scoring_request_worker_terminal"),
        DIGEST,
    )
    .unwrap()
}

fn outbox_count(client: &mut Client, event_ref: &str) -> i64 {
    client
        .query_one(
            "SELECT count(*) FROM integration_outbox WHERE event_ref = $1",
            &[&event_ref],
        )
        .unwrap()
        .get(0)
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

#[test]
fn worker_commits_and_replays_completion_with_the_stable_event_ref() {
    let _guard = worker_test_guard();
    let mut client = test_client();
    reset_and_migrate(&mut client);
    let job_ref = "scoring_job_worker_completed";
    let fencing_token = persist_and_claim(&mut client, job_ref, "scoring_request_worker_completed");
    let identity = ScoringTerminalIdentity::Result("result_worker_completed");
    let event_ref = scoring_terminal_event_ref(job_ref, identity).unwrap();
    let event = terminal_event(&event_ref, "scoring.result.completed", job_ref);

    let mut transaction = client.transaction().unwrap();
    let inserted = commit_scoring_worker_outcome(
        &mut transaction,
        job_ref,
        fencing_token,
        ScoringWorkerOutcome::Completed {
            result_ref: "result_worker_completed",
        },
        20_000,
        &event,
        3,
    )
    .unwrap();
    assert!(matches!(
        inserted,
        ScoringWorkerPersistence::Completed(persistence)
            if persistence.completion() == ScoringJobCompletionDisposition::Completed
                && persistence.outbox() == PersistenceDisposition::Inserted
    ));
    transaction.commit().unwrap();

    let mut transaction = client.transaction().unwrap();
    let duplicate = commit_scoring_worker_outcome(
        &mut transaction,
        job_ref,
        fencing_token,
        ScoringWorkerOutcome::Completed {
            result_ref: "result_worker_completed",
        },
        20_000,
        &event,
        3,
    )
    .unwrap();
    assert!(matches!(
        duplicate,
        ScoringWorkerPersistence::Completed(persistence)
            if persistence.completion() == ScoringJobCompletionDisposition::Duplicate
                && persistence.outbox() == PersistenceDisposition::Duplicate
    ));
    transaction.commit().unwrap();

    let (state, result_ref, cause) = job_state(&mut client, job_ref);
    assert_eq!(state, "completed");
    assert_eq!(result_ref.as_deref(), Some("result_worker_completed"));
    assert_eq!(cause, None);
    assert_eq!(outbox_count(&mut client, &event_ref), 1);
}

#[test]
fn worker_commits_and_replays_failure_with_the_stable_event_ref() {
    let _guard = worker_test_guard();
    let mut client = test_client();
    reset_and_migrate(&mut client);
    let job_ref = "scoring_job_worker_failed";
    let fencing_token = persist_and_claim(&mut client, job_ref, "scoring_request_worker_failed");
    let identity = ScoringTerminalIdentity::Cause("invalid_scientific_evidence");
    let event_ref = scoring_terminal_event_ref(job_ref, identity).unwrap();
    let event = terminal_event(&event_ref, "scoring.result.failed", job_ref);

    let mut transaction = client.transaction().unwrap();
    let inserted = commit_scoring_worker_outcome(
        &mut transaction,
        job_ref,
        fencing_token,
        ScoringWorkerOutcome::Failed {
            cause_code: "invalid_scientific_evidence",
        },
        20_000,
        &event,
        3,
    )
    .unwrap();
    assert!(matches!(
        inserted,
        ScoringWorkerPersistence::Failed(persistence)
            if persistence.failure() == ScoringJobFailureDisposition::Quarantined
                && persistence.outbox() == PersistenceDisposition::Inserted
    ));
    transaction.commit().unwrap();

    let mut transaction = client.transaction().unwrap();
    let duplicate = commit_scoring_worker_outcome(
        &mut transaction,
        job_ref,
        fencing_token,
        ScoringWorkerOutcome::Failed {
            cause_code: "invalid_scientific_evidence",
        },
        20_000,
        &event,
        3,
    )
    .unwrap();
    assert!(matches!(
        duplicate,
        ScoringWorkerPersistence::Failed(persistence)
            if persistence.failure() == ScoringJobFailureDisposition::Duplicate
                && persistence.outbox() == PersistenceDisposition::Duplicate
    ));
    transaction.commit().unwrap();

    let (state, result_ref, cause) = job_state(&mut client, job_ref);
    assert_eq!(state, "quarantined");
    assert_eq!(result_ref, None);
    assert_eq!(cause.as_deref(), Some("invalid_scientific_evidence"));
    assert_eq!(outbox_count(&mut client, &event_ref), 1);
}

#[test]
fn minted_event_ref_is_rejected_before_the_first_terminal_write() {
    let _guard = worker_test_guard();
    let mut client = test_client();
    reset_and_migrate(&mut client);
    let job_ref = "scoring_job_worker_minted_first";
    let fencing_token =
        persist_and_claim(&mut client, job_ref, "scoring_request_worker_minted_first");
    let event = terminal_event(
        "event_scoring_worker_minted_first",
        "scoring.result.completed",
        job_ref,
    );

    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        commit_scoring_worker_outcome(
            &mut transaction,
            job_ref,
            fencing_token,
            ScoringWorkerOutcome::Completed {
                result_ref: "result_worker_minted_first",
            },
            20_000,
            &event,
            3,
        ),
        Err(ScoringWorkerCommitError::Identity(
            ScoringWorkerError::UnstableEventRef
        ))
    ));
    transaction.rollback().unwrap();

    let (state, result_ref, cause) = job_state(&mut client, job_ref);
    assert_eq!(state, "leased");
    assert_eq!(result_ref, None);
    assert_eq!(cause, None);
    let total_outbox: i64 = client
        .query_one("SELECT count(*) FROM integration_outbox", &[])
        .unwrap()
        .get(0);
    assert_eq!(total_outbox, 0);
}

#[test]
fn minted_event_ref_cannot_add_a_second_outbox_row_after_accept() {
    let _guard = worker_test_guard();
    let mut client = test_client();
    reset_and_migrate(&mut client);
    let job_ref = "scoring_job_worker_minted_replay";
    let fencing_token =
        persist_and_claim(&mut client, job_ref, "scoring_request_worker_minted_replay");
    let identity = ScoringTerminalIdentity::Cause("invalid_scientific_evidence");
    let event_ref = scoring_terminal_event_ref(job_ref, identity).unwrap();
    let accepted = terminal_event(&event_ref, "scoring.result.failed", job_ref);

    let mut transaction = client.transaction().unwrap();
    commit_scoring_worker_outcome(
        &mut transaction,
        job_ref,
        fencing_token,
        ScoringWorkerOutcome::Failed {
            cause_code: "invalid_scientific_evidence",
        },
        20_000,
        &accepted,
        3,
    )
    .unwrap();
    transaction.commit().unwrap();

    let minted = terminal_event(
        "event_scoring_worker_minted_replay",
        "scoring.result.failed",
        job_ref,
    );
    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        commit_scoring_worker_outcome(
            &mut transaction,
            job_ref,
            fencing_token,
            ScoringWorkerOutcome::Failed {
                cause_code: "invalid_scientific_evidence",
            },
            20_000,
            &minted,
            3,
        ),
        Err(ScoringWorkerCommitError::Identity(
            ScoringWorkerError::UnstableEventRef
        ))
    ));
    transaction.rollback().unwrap();

    let (state, _, cause) = job_state(&mut client, job_ref);
    assert_eq!(state, "quarantined");
    assert_eq!(cause.as_deref(), Some("invalid_scientific_evidence"));
    assert_eq!(outbox_count(&mut client, &event_ref), 1);
    let minted_count: i64 = client
        .query_one(
            "SELECT count(*) FROM integration_outbox WHERE event_ref = $1",
            &[&"event_scoring_worker_minted_replay"],
        )
        .unwrap()
        .get(0);
    assert_eq!(minted_count, 0);
}

struct ScriptedScoringEngine {
    result: Result<ScoringWorkerEngineOutcome, ScoringWorkerError>,
}

impl ScoringWorkerEngine for ScriptedScoringEngine {
    fn score_claimed_job(
        &self,
        _scoring_job_ref: &str,
        _scoring_request_ref: &str,
    ) -> Result<ScoringWorkerEngineOutcome, ScoringWorkerError> {
        self.result.clone()
    }
}

fn worker_envelope(event_type: &'static str) -> ScoringWorkerEnvelope<'static> {
    ScoringWorkerEnvelope {
        event_type,
        schema_version: "v1",
        source: "psychometrics_commons",
        tenant_ref: "tenant_worker_terminal",
        occurred_at_unix_ms: 20_000,
        correlation_ref: "correlation_worker_terminal",
        causation_ref: Some("scoring_request_worker_terminal"),
        payload_digest: DIGEST,
    }
}

#[test]
fn worker_attempt_commits_engine_completion_with_the_stable_event_ref() {
    let _guard = worker_test_guard();
    let mut client = test_client();
    reset_and_migrate(&mut client);
    let job_ref = "scoring_job_worker_attempt_completed";
    let fencing_token = persist_and_claim(
        &mut client,
        job_ref,
        "scoring_request_worker_attempt_completed",
    );
    let engine = ScriptedScoringEngine {
        result: Ok(ScoringWorkerEngineOutcome::Completed {
            result_ref: "result_worker_attempt_completed".to_owned(),
        }),
    };

    let mut transaction = client.transaction().unwrap();
    let inserted = run_scoring_worker_attempt(
        &mut transaction,
        job_ref,
        fencing_token,
        "scoring_request_worker_attempt_completed",
        &engine,
        worker_envelope("scoring.result.completed"),
        3,
    )
    .unwrap();
    assert!(matches!(
        inserted,
        ScoringWorkerPersistence::Completed(persistence)
            if persistence.completion() == ScoringJobCompletionDisposition::Completed
                && persistence.outbox() == PersistenceDisposition::Inserted
    ));
    transaction.commit().unwrap();

    let mut transaction = client.transaction().unwrap();
    let duplicate = run_scoring_worker_attempt(
        &mut transaction,
        job_ref,
        fencing_token,
        "scoring_request_worker_attempt_completed",
        &engine,
        worker_envelope("scoring.result.completed"),
        3,
    )
    .unwrap();
    assert!(matches!(
        duplicate,
        ScoringWorkerPersistence::Completed(persistence)
            if persistence.completion() == ScoringJobCompletionDisposition::Duplicate
                && persistence.outbox() == PersistenceDisposition::Duplicate
    ));
    transaction.commit().unwrap();

    let event_ref = scoring_terminal_event_ref(
        job_ref,
        ScoringTerminalIdentity::Result("result_worker_attempt_completed"),
    )
    .unwrap();
    let (state, result_ref, cause) = job_state(&mut client, job_ref);
    assert_eq!(state, "completed");
    assert_eq!(
        result_ref.as_deref(),
        Some("result_worker_attempt_completed")
    );
    assert_eq!(cause, None);
    assert_eq!(outbox_count(&mut client, &event_ref), 1);
}

#[test]
fn worker_attempt_commits_engine_failure_with_the_stable_event_ref() {
    let _guard = worker_test_guard();
    let mut client = test_client();
    reset_and_migrate(&mut client);
    let job_ref = "scoring_job_worker_attempt_failed";
    let fencing_token = persist_and_claim(
        &mut client,
        job_ref,
        "scoring_request_worker_attempt_failed",
    );
    let engine = ScriptedScoringEngine {
        result: Ok(ScoringWorkerEngineOutcome::Failed {
            cause_code: "invalid_scientific_evidence".to_owned(),
        }),
    };

    let mut transaction = client.transaction().unwrap();
    let inserted = run_scoring_worker_attempt(
        &mut transaction,
        job_ref,
        fencing_token,
        "scoring_request_worker_attempt_failed",
        &engine,
        worker_envelope("scoring.result.failed"),
        3,
    )
    .unwrap();
    assert!(matches!(
        inserted,
        ScoringWorkerPersistence::Failed(persistence)
            if persistence.failure() == ScoringJobFailureDisposition::Quarantined
                && persistence.outbox() == PersistenceDisposition::Inserted
    ));
    transaction.commit().unwrap();

    let event_ref = scoring_terminal_event_ref(
        job_ref,
        ScoringTerminalIdentity::Cause("invalid_scientific_evidence"),
    )
    .unwrap();
    let (state, result_ref, cause) = job_state(&mut client, job_ref);
    assert_eq!(state, "quarantined");
    assert_eq!(result_ref, None);
    assert_eq!(cause.as_deref(), Some("invalid_scientific_evidence"));
    assert_eq!(outbox_count(&mut client, &event_ref), 1);
}

#[test]
fn worker_attempt_replays_engine_failure_with_the_stable_event_ref() {
    let _guard = worker_test_guard();
    let mut client = test_client();
    reset_and_migrate(&mut client);
    let job_ref = "scoring_job_worker_attempt_failed_replay";
    let fencing_token = persist_and_claim(
        &mut client,
        job_ref,
        "scoring_request_worker_attempt_failed_replay",
    );
    let engine = ScriptedScoringEngine {
        result: Ok(ScoringWorkerEngineOutcome::Failed {
            cause_code: "invalid_scientific_evidence".to_owned(),
        }),
    };

    let mut transaction = client.transaction().unwrap();
    run_scoring_worker_attempt(
        &mut transaction,
        job_ref,
        fencing_token,
        "scoring_request_worker_attempt_failed_replay",
        &engine,
        worker_envelope("scoring.result.failed"),
        3,
    )
    .unwrap();
    transaction.commit().unwrap();

    let mut transaction = client.transaction().unwrap();
    let duplicate = run_scoring_worker_attempt(
        &mut transaction,
        job_ref,
        fencing_token,
        "scoring_request_worker_attempt_failed_replay",
        &engine,
        worker_envelope("scoring.result.failed"),
        3,
    )
    .unwrap();
    assert!(matches!(
        duplicate,
        ScoringWorkerPersistence::Failed(persistence)
            if persistence.failure() == ScoringJobFailureDisposition::Duplicate
                && persistence.outbox() == PersistenceDisposition::Duplicate
    ));
    transaction.commit().unwrap();

    let event_ref = scoring_terminal_event_ref(
        job_ref,
        ScoringTerminalIdentity::Cause("invalid_scientific_evidence"),
    )
    .unwrap();
    let (state, result_ref, cause) = job_state(&mut client, job_ref);
    assert_eq!(state, "quarantined");
    assert_eq!(result_ref, None);
    assert_eq!(cause.as_deref(), Some("invalid_scientific_evidence"));
    assert_eq!(outbox_count(&mut client, &event_ref), 1);
}

#[test]
fn engine_error_leaves_the_leased_job_and_outbox_untouched() {
    let _guard = worker_test_guard();
    let mut client = test_client();
    reset_and_migrate(&mut client);
    let job_ref = "scoring_job_worker_engine_error";
    let fencing_token =
        persist_and_claim(&mut client, job_ref, "scoring_request_worker_engine_error");
    let engine = ScriptedScoringEngine {
        result: Err(ScoringWorkerError::InvalidEnvelope),
    };

    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        run_scoring_worker_attempt(
            &mut transaction,
            job_ref,
            fencing_token,
            "scoring_request_worker_engine_error",
            &engine,
            worker_envelope("scoring.result.completed"),
            3,
        ),
        Err(ScoringWorkerCommitError::Planning(
            ScoringWorkerError::InvalidEnvelope
        ))
    ));
    transaction.rollback().unwrap();

    let (state, result_ref, cause) = job_state(&mut client, job_ref);
    assert_eq!(state, "leased");
    assert_eq!(result_ref, None);
    assert_eq!(cause, None);
    let total_outbox: i64 = client
        .query_one("SELECT count(*) FROM integration_outbox", &[])
        .unwrap()
        .get(0);
    assert_eq!(total_outbox, 0);
}

#[test]
fn invalid_caller_envelope_leaves_the_leased_job_untouched() {
    let _guard = worker_test_guard();
    let mut client = test_client();
    reset_and_migrate(&mut client);
    let job_ref = "scoring_job_worker_invalid_envelope";
    let fencing_token = persist_and_claim(
        &mut client,
        job_ref,
        "scoring_request_worker_invalid_envelope",
    );
    let engine = ScriptedScoringEngine {
        result: Ok(ScoringWorkerEngineOutcome::Completed {
            result_ref: "result_worker_invalid_envelope".to_owned(),
        }),
    };
    let mut envelope = worker_envelope("scoring.result.completed");
    envelope.payload_digest = "not-a-digest";

    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        run_scoring_worker_attempt(
            &mut transaction,
            job_ref,
            fencing_token,
            "scoring_request_worker_invalid_envelope",
            &engine,
            envelope,
            3,
        ),
        Err(ScoringWorkerCommitError::Planning(
            ScoringWorkerError::InvalidEnvelope
        ))
    ));
    transaction.rollback().unwrap();

    let (state, result_ref, cause) = job_state(&mut client, job_ref);
    assert_eq!(state, "leased");
    assert_eq!(result_ref, None);
    assert_eq!(cause, None);
}

#[test]
fn stale_completion_fence_fails_closed_after_stable_identity_check() {
    let _guard = worker_test_guard();
    let mut client = test_client();
    reset_and_migrate(&mut client);
    let job_ref = "scoring_job_worker_stale_completion";
    let fencing_token = persist_and_claim(
        &mut client,
        job_ref,
        "scoring_request_worker_stale_completion",
    );
    let identity = ScoringTerminalIdentity::Result("result_worker_stale_completion");
    let event_ref = scoring_terminal_event_ref(job_ref, identity).unwrap();
    let event = terminal_event(&event_ref, "scoring.result.completed", job_ref);

    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        commit_scoring_worker_outcome(
            &mut transaction,
            job_ref,
            fencing_token.saturating_add(1),
            ScoringWorkerOutcome::Completed {
                result_ref: "result_worker_stale_completion",
            },
            20_000,
            &event,
            3,
        ),
        Err(ScoringWorkerCommitError::Completion(
            ScoringCompletionOutboxError::Completion(ScoringJobPersistenceError::StaleLease)
        ))
    ));
    transaction.rollback().unwrap();

    let (state, result_ref, cause) = job_state(&mut client, job_ref);
    assert_eq!(state, "leased");
    assert_eq!(result_ref, None);
    assert_eq!(cause, None);
    assert_eq!(outbox_count(&mut client, &event_ref), 0);
}

#[test]
fn stale_failure_fence_fails_closed_after_stable_identity_check() {
    let _guard = worker_test_guard();
    let mut client = test_client();
    reset_and_migrate(&mut client);
    let job_ref = "scoring_job_worker_stale_failure";
    let fencing_token =
        persist_and_claim(&mut client, job_ref, "scoring_request_worker_stale_failure");
    let identity = ScoringTerminalIdentity::Cause("invalid_scientific_evidence");
    let event_ref = scoring_terminal_event_ref(job_ref, identity).unwrap();
    let event = terminal_event(&event_ref, "scoring.result.failed", job_ref);

    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        commit_scoring_worker_outcome(
            &mut transaction,
            job_ref,
            fencing_token.saturating_add(1),
            ScoringWorkerOutcome::Failed {
                cause_code: "invalid_scientific_evidence",
            },
            20_000,
            &event,
            3,
        ),
        Err(ScoringWorkerCommitError::Failure(
            ScoringFailureOutboxError::Failure(ScoringJobPersistenceError::StaleLease)
        ))
    ));
    transaction.rollback().unwrap();

    let (state, result_ref, cause) = job_state(&mut client, job_ref);
    assert_eq!(state, "leased");
    assert_eq!(result_ref, None);
    assert_eq!(cause, None);
    assert_eq!(outbox_count(&mut client, &event_ref), 0);
}

#[test]
fn worker_commit_errors_retain_typed_sources() {
    let identity = ScoringWorkerCommitError::Identity(ScoringWorkerError::UnstableEventRef);
    assert_eq!(
        identity.to_string(),
        "scoring worker must reuse the stable job and outcome event identity"
    );
    assert!(identity.source().is_some());

    let planning = ScoringWorkerCommitError::Planning(ScoringWorkerError::InvalidEnvelope);
    assert_eq!(
        planning.to_string(),
        "scoring worker could not plan a terminal attempt; keep the job leased and retry after a typed engine outcome"
    );
    assert!(planning.source().is_some());

    let completion = ScoringWorkerCommitError::Completion(
        ScoringCompletionOutboxError::InvalidCompletionEnvelope,
    );
    assert_eq!(
        completion.to_string(),
        "scoring worker completion persistence failed"
    );
    assert!(completion.source().is_some());

    let failure =
        ScoringWorkerCommitError::Failure(ScoringFailureOutboxError::InvalidFailureEnvelope);
    assert_eq!(
        failure.to_string(),
        "scoring worker failure persistence failed"
    );
    assert!(failure.source().is_some());
}
