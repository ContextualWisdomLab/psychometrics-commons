//! Real `PostgreSQL` contract for one fenced scoring-worker attempt.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::integration::IntegrationEvent;
use psychometrics_commons_runtime::postgres_integration::apply_integration_migration;
use psychometrics_commons_runtime::postgres_scoring_completion::ScoringCompletionOutboxError;
use psychometrics_commons_runtime::postgres_scoring_failure::ScoringFailureOutboxError;
use psychometrics_commons_runtime::postgres_scoring_job::ScoringJobPersistenceError;
use psychometrics_commons_runtime::postgres_scoring_job::{
    apply_scoring_job_migration, claim_scoring_job, persist_scoring_job,
    ScoringJobCompletionDisposition, ScoringJobFailureDisposition,
};
use psychometrics_commons_runtime::postgres_scoring_worker::{
    run_scoring_worker_attempt, ScoringWorkerAttemptError, ScoringWorkerAttemptPersistence,
    ScoringWorkerPersistence,
};
use psychometrics_commons_runtime::scoring_job::ScoringJobState;
use psychometrics_commons_runtime::scoring_worker::ScoringWorkerError;
use psychometrics_commons_runtime::scoring_worker::{
    scoring_terminal_event_ref, ScoringEngineAttempt, ScoringTerminalIdentity,
    ScriptedScoringEngine,
};
use std::error::Error;
use std::sync::{Mutex, MutexGuard};

const DIGEST: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

static ATTEMPT_TEST_LOCK: Mutex<()> = Mutex::new(());

fn attempt_test_guard() -> MutexGuard<'static, ()> {
    ATTEMPT_TEST_LOCK
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
            "CREATE SCHEMA IF NOT EXISTS scoring_worker_attempt_test;\
             SET search_path TO scoring_worker_attempt_test;",
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
    let job = psychometrics_commons_runtime::scoring_job::ScoringJob::new(job_ref, request_ref, 3)
        .unwrap();
    let mut transaction = client.transaction().unwrap();
    persist_scoring_job(&mut transaction, &job).unwrap();
    transaction.commit().unwrap();

    let mut transaction = client.transaction().unwrap();
    let lease = claim_scoring_job(
        &mut transaction,
        job_ref,
        "worker_attempt_alpha",
        "lease_attempt_alpha",
        10_000,
        30_000,
    )
    .unwrap();
    let fencing_token = lease.fencing_token();
    transaction.commit().unwrap();
    fencing_token
}

fn minted_envelope(event_type: &str, job_ref: &str) -> IntegrationEvent {
    IntegrationEvent::new(
        "event_scoring_worker_minted",
        event_type,
        "v1",
        "psychometrics_commons",
        "tenant_worker_attempt",
        job_ref,
        20_000,
        "correlation_worker_attempt",
        Some("scoring_request_worker_attempt"),
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

fn minted_outbox_count(client: &mut Client) -> i64 {
    outbox_count(client, "event_scoring_worker_minted")
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
fn attempt_rewrites_a_minted_envelope_and_replays_the_stable_completion() {
    let _guard = attempt_test_guard();
    let mut client = test_client();
    reset_and_migrate(&mut client);
    let job_ref = "scoring_job_attempt_completed";
    let fencing_token =
        persist_and_claim(&mut client, job_ref, "scoring_request_attempt_completed");
    let engine = ScriptedScoringEngine::new(ScoringEngineAttempt::Completed {
        result_ref: "result_attempt_completed",
    });
    let expected = scoring_terminal_event_ref(
        job_ref,
        ScoringTerminalIdentity::Result("result_attempt_completed"),
    )
    .unwrap();

    let mut transaction = client.transaction().unwrap();
    let first = run_scoring_worker_attempt(
        &mut transaction,
        job_ref,
        fencing_token,
        engine.evaluate(),
        &minted_envelope("scoring.result.completed", job_ref),
        20_000,
        40_000,
        3,
    )
    .unwrap();
    transaction.commit().unwrap();

    match first {
        ScoringWorkerAttemptPersistence::Terminal(ScoringWorkerPersistence::Completed(
            persistence,
        )) => {
            assert_eq!(
                persistence.completion(),
                ScoringJobCompletionDisposition::Completed
            );
        }
        ScoringWorkerAttemptPersistence::Terminal(ScoringWorkerPersistence::Failed(_))
        | ScoringWorkerAttemptPersistence::Retryable(_) => {
            panic!("scripted completion must persist a terminal result");
        }
    }
    assert_eq!(
        job_state(&mut client, job_ref),
        (
            "completed".to_owned(),
            Some("result_attempt_completed".to_owned()),
            None
        )
    );
    assert_eq!(outbox_count(&mut client, &expected), 1);
    assert_eq!(minted_outbox_count(&mut client), 0);

    let mut transaction = client.transaction().unwrap();
    let replay = run_scoring_worker_attempt(
        &mut transaction,
        job_ref,
        fencing_token,
        engine.evaluate(),
        &minted_envelope("scoring.result.completed", job_ref),
        20_000,
        40_000,
        3,
    )
    .unwrap();
    transaction.commit().unwrap();
    match replay {
        ScoringWorkerAttemptPersistence::Terminal(ScoringWorkerPersistence::Completed(
            persistence,
        )) => {
            assert_eq!(
                persistence.completion(),
                ScoringJobCompletionDisposition::Duplicate
            );
        }
        ScoringWorkerAttemptPersistence::Terminal(ScoringWorkerPersistence::Failed(_))
        | ScoringWorkerAttemptPersistence::Retryable(_) => {
            panic!("exact replay must stay a completed terminal");
        }
    }
    assert_eq!(outbox_count(&mut client, &expected), 1);
    assert_eq!(minted_outbox_count(&mut client), 0);
}

#[test]
fn attempt_rewrites_a_minted_envelope_and_replays_the_stable_failure() {
    let _guard = attempt_test_guard();
    let mut client = test_client();
    reset_and_migrate(&mut client);
    let job_ref = "scoring_job_attempt_failed";
    let fencing_token = persist_and_claim(&mut client, job_ref, "scoring_request_attempt_failed");
    let engine = ScriptedScoringEngine::new(ScoringEngineAttempt::PermanentFailure {
        cause_code: "invalid_scientific_evidence",
    });
    let expected = scoring_terminal_event_ref(
        job_ref,
        ScoringTerminalIdentity::Cause("invalid_scientific_evidence"),
    )
    .unwrap();

    let mut transaction = client.transaction().unwrap();
    let first = run_scoring_worker_attempt(
        &mut transaction,
        job_ref,
        fencing_token,
        engine.evaluate(),
        &minted_envelope("scoring.result.failed", job_ref),
        20_000,
        40_000,
        3,
    )
    .unwrap();
    transaction.commit().unwrap();
    match first {
        ScoringWorkerAttemptPersistence::Terminal(ScoringWorkerPersistence::Failed(
            persistence,
        )) => {
            assert_eq!(
                persistence.failure(),
                ScoringJobFailureDisposition::Quarantined
            );
        }
        ScoringWorkerAttemptPersistence::Terminal(ScoringWorkerPersistence::Completed(_))
        | ScoringWorkerAttemptPersistence::Retryable(_) => {
            panic!("scripted permanent failure must persist a terminal quarantine");
        }
    }
    assert_eq!(
        job_state(&mut client, job_ref),
        (
            "quarantined".to_owned(),
            None,
            Some("invalid_scientific_evidence".to_owned())
        )
    );
    assert_eq!(outbox_count(&mut client, &expected), 1);
    assert_eq!(minted_outbox_count(&mut client), 0);

    let mut transaction = client.transaction().unwrap();
    let replay = run_scoring_worker_attempt(
        &mut transaction,
        job_ref,
        fencing_token,
        engine.evaluate(),
        &minted_envelope("scoring.result.failed", job_ref),
        20_000,
        40_000,
        3,
    )
    .unwrap();
    transaction.commit().unwrap();
    match replay {
        ScoringWorkerAttemptPersistence::Terminal(ScoringWorkerPersistence::Failed(
            persistence,
        )) => {
            assert_eq!(
                persistence.failure(),
                ScoringJobFailureDisposition::Duplicate
            );
        }
        ScoringWorkerAttemptPersistence::Terminal(ScoringWorkerPersistence::Completed(_))
        | ScoringWorkerAttemptPersistence::Retryable(_) => {
            panic!("exact replay must stay a failed terminal");
        }
    }
    assert_eq!(outbox_count(&mut client, &expected), 1);
}

#[test]
fn retryable_attempt_does_not_write_a_terminal_outbox_row() {
    let _guard = attempt_test_guard();
    let mut client = test_client();
    reset_and_migrate(&mut client);
    let job_ref = "scoring_job_attempt_retryable";
    let fencing_token =
        persist_and_claim(&mut client, job_ref, "scoring_request_attempt_retryable");
    let engine = ScriptedScoringEngine::new(ScoringEngineAttempt::Retryable {
        cause_code: "scoring_engine_unavailable",
    });

    let mut transaction = client.transaction().unwrap();
    let outcome = run_scoring_worker_attempt(
        &mut transaction,
        job_ref,
        fencing_token,
        engine.evaluate(),
        &minted_envelope("scoring.result.completed", job_ref),
        20_000,
        40_000,
        3,
    )
    .unwrap();
    transaction.commit().unwrap();

    assert_eq!(
        outcome,
        ScoringWorkerAttemptPersistence::Retryable(ScoringJobState::RetryScheduled)
    );
    assert_eq!(
        job_state(&mut client, job_ref),
        (
            "retry_scheduled".to_owned(),
            None,
            Some("scoring_engine_unavailable".to_owned())
        )
    );
    assert_eq!(minted_outbox_count(&mut client), 0);
    let terminal_count: i64 = client
        .query_one("SELECT count(*) FROM integration_outbox", &[])
        .unwrap()
        .get(0);
    assert_eq!(terminal_count, 0);
}

#[test]
fn retryable_attempt_rejects_an_invalid_retry_window_without_outbox() {
    let _guard = attempt_test_guard();
    let mut client = test_client();
    reset_and_migrate(&mut client);
    let job_ref = "scoring_job_attempt_retry_window";
    let fencing_token =
        persist_and_claim(&mut client, job_ref, "scoring_request_attempt_retry_window");
    let engine = ScriptedScoringEngine::new(ScoringEngineAttempt::Retryable {
        cause_code: "scoring_engine_unavailable",
    });

    let mut transaction = client.transaction().unwrap();
    let error = run_scoring_worker_attempt(
        &mut transaction,
        job_ref,
        fencing_token,
        engine.evaluate(),
        &minted_envelope("scoring.result.completed", job_ref),
        20_000,
        19_000,
        3,
    )
    .unwrap_err();
    transaction.rollback().unwrap();

    assert!(matches!(
        error,
        ScoringWorkerAttemptError::Retry(ScoringJobPersistenceError::InvalidRetryWindow)
    ));
    assert_eq!(
        job_state(&mut client, job_ref),
        ("leased".to_owned(), None, None)
    );
    assert_eq!(minted_outbox_count(&mut client), 0);
}

#[test]
fn mismatched_completion_time_fails_closed_without_a_minted_row() {
    let _guard = attempt_test_guard();
    let mut client = test_client();
    reset_and_migrate(&mut client);
    let job_ref = "scoring_job_attempt_time_mismatch";
    let fencing_token = persist_and_claim(
        &mut client,
        job_ref,
        "scoring_request_attempt_time_mismatch",
    );
    let engine = ScriptedScoringEngine::new(ScoringEngineAttempt::Completed {
        result_ref: "result_attempt_time_mismatch",
    });

    let mut transaction = client.transaction().unwrap();
    let error = run_scoring_worker_attempt(
        &mut transaction,
        job_ref,
        fencing_token,
        engine.evaluate(),
        &minted_envelope("scoring.result.completed", job_ref),
        21_000,
        40_000,
        3,
    )
    .unwrap_err();
    transaction.rollback().unwrap();

    assert!(matches!(
        error,
        ScoringWorkerAttemptError::Completion(
            ScoringCompletionOutboxError::InvalidCompletionEnvelope
        )
    ));
    assert_eq!(
        job_state(&mut client, job_ref),
        ("leased".to_owned(), None, None)
    );
    assert_eq!(minted_outbox_count(&mut client), 0);
}

#[test]
fn attempt_errors_explain_the_next_safe_action() {
    let identity = ScoringWorkerAttemptError::Identity(ScoringWorkerError::UnstableEventRef);
    assert_eq!(
        identity.to_string(),
        "bind the stable job and outcome event identity before the terminal write"
    );
    assert!(identity.source().is_some());

    let completion = ScoringWorkerAttemptError::Completion(
        ScoringCompletionOutboxError::InvalidCompletionEnvelope,
    );
    assert_eq!(
        completion.to_string(),
        "scoring worker completion persistence failed"
    );
    assert!(completion.source().is_some());

    let failure =
        ScoringWorkerAttemptError::Failure(ScoringFailureOutboxError::InvalidFailureEnvelope);
    assert_eq!(
        failure.to_string(),
        "scoring worker failure persistence failed"
    );
    assert!(failure.source().is_some());

    let retry = ScoringWorkerAttemptError::Retry(ScoringJobPersistenceError::InvalidRetryWindow);
    assert_eq!(
        retry.to_string(),
        "record the retryable engine failure without a terminal outbox row"
    );
    assert!(retry.source().is_some());
}
