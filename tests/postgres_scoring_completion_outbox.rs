//! Real `PostgreSQL` contract for atomic scoring completion and outbox persistence.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::integration::IntegrationEvent;
use psychometrics_commons_runtime::postgres_integration::{
    apply_integration_migration, enqueue_outbox_event, PersistenceDisposition, PersistenceError,
};
use psychometrics_commons_runtime::postgres_scoring_completion::{
    record_successful_scoring_completion_with_outbox, ScoringCompletionOutboxError,
};
use psychometrics_commons_runtime::postgres_scoring_job::{
    apply_scoring_job_migration, claim_scoring_job, persist_scoring_job,
    ScoringJobCompletionDisposition, ScoringJobPersistenceError,
};
use psychometrics_commons_runtime::scoring_job::ScoringJob;
use std::error::Error;
use std::sync::{Mutex, MutexGuard};

const DIGEST_A: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const DIGEST_B: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

static COMPLETION_TEST_LOCK: Mutex<()> = Mutex::new(());

fn completion_test_guard() -> MutexGuard<'static, ()> {
    COMPLETION_TEST_LOCK
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
            "CREATE SCHEMA IF NOT EXISTS scoring_completion_outbox_test;\
             SET search_path TO scoring_completion_outbox_test;",
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
        "worker_completion_alpha",
        "lease_completion_alpha",
        10_000,
        30_000,
    )
    .unwrap();
    let fencing_token = lease.fencing_token();
    transaction.commit().unwrap();
    fencing_token
}

fn completion_event(event_ref: &str, job_ref: &str, digest: &str) -> IntegrationEvent {
    IntegrationEvent::new(
        event_ref,
        "scoring.result.completed",
        "v1",
        "psychometrics_commons",
        "tenant_completion_alpha",
        job_ref,
        20_000,
        "correlation_completion_alpha",
        Some("scoring_request_completion_alpha"),
        digest,
    )
    .unwrap()
}

#[test]
fn completion_and_outbox_commit_and_replay_together() {
    let _guard = completion_test_guard();
    let mut client = test_client();
    reset_and_migrate(&mut client);
    let job_ref = "scoring_job_completion_alpha";
    let result_ref = "scoring_result_completion_alpha";
    let fencing_token = persist_and_claim(
        &mut client,
        job_ref,
        "scoring_request_completion_alpha",
    );
    let event = completion_event("event_completion_alpha", job_ref, DIGEST_A);

    let mut transaction = client.transaction().unwrap();
    let inserted = record_successful_scoring_completion_with_outbox(
        &mut transaction,
        job_ref,
        fencing_token,
        result_ref,
        20_000,
        &event,
        3,
    )
    .unwrap();
    assert_eq!(
        inserted.completion(),
        ScoringJobCompletionDisposition::Completed
    );
    assert_eq!(inserted.outbox(), PersistenceDisposition::Inserted);
    transaction.commit().unwrap();

    let mut transaction = client.transaction().unwrap();
    let duplicate = record_successful_scoring_completion_with_outbox(
        &mut transaction,
        job_ref,
        fencing_token,
        result_ref,
        20_000,
        &event,
        3,
    )
    .unwrap();
    assert_eq!(
        duplicate.completion(),
        ScoringJobCompletionDisposition::Duplicate
    );
    assert_eq!(duplicate.outbox(), PersistenceDisposition::Duplicate);
    transaction.commit().unwrap();

    let row = client
        .query_one(
            "SELECT scoring_state, result_ref FROM scoring_job_state WHERE scoring_job_ref = $1",
            &[&job_ref],
        )
        .unwrap();
    let state: String = row.get(0);
    let stored_result_ref: Option<String> = row.get(1);
    assert_eq!(state, "completed");
    assert_eq!(stored_result_ref.as_deref(), Some(result_ref));
    let outbox_count: i64 = client
        .query_one(
            "SELECT count(*) FROM integration_outbox WHERE event_ref = $1",
            &[&event.event_ref()],
        )
        .unwrap()
        .get(0);
    assert_eq!(outbox_count, 1);
}

#[test]
fn late_outbox_conflict_rolls_back_the_completion_transition() {
    let _guard = completion_test_guard();
    let mut client = test_client();
    reset_and_migrate(&mut client);
    let job_ref = "scoring_job_completion_conflict";
    let fencing_token = persist_and_claim(
        &mut client,
        job_ref,
        "scoring_request_completion_conflict",
    );
    let existing_event = completion_event("event_completion_conflict", job_ref, DIGEST_A);
    assert_eq!(
        enqueue_outbox_event(&mut client, &existing_event, 3).unwrap(),
        PersistenceDisposition::Inserted
    );
    let conflicting_event = completion_event("event_completion_conflict", job_ref, DIGEST_B);

    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        record_successful_scoring_completion_with_outbox(
            &mut transaction,
            job_ref,
            fencing_token,
            "scoring_result_completion_conflict",
            20_000,
            &conflicting_event,
            3,
        ),
        Err(ScoringCompletionOutboxError::Outbox(
            PersistenceError::ConflictingReplay
        ))
    ));
    transaction.rollback().unwrap();

    let row = client
        .query_one(
            "SELECT scoring_state, result_ref, active_fencing_token \
             FROM scoring_job_state WHERE scoring_job_ref = $1",
            &[&job_ref],
        )
        .unwrap();
    let state: String = row.get(0);
    let stored_result_ref: Option<String> = row.get(1);
    let stored_fencing_token: Option<i64> = row.get(2);
    assert_eq!(state, "leased");
    assert_eq!(stored_result_ref, None);
    assert_eq!(stored_fencing_token, Some(fencing_token as i64));
}

#[test]
fn completion_failure_does_not_enqueue_an_outbox_event() {
    let _guard = completion_test_guard();
    let mut client = test_client();
    reset_and_migrate(&mut client);
    let event = completion_event(
        "event_completion_missing_job",
        "scoring_job_completion_missing",
        DIGEST_A,
    );

    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        record_successful_scoring_completion_with_outbox(
            &mut transaction,
            "scoring_job_completion_missing",
            1,
            "scoring_result_completion_missing",
            20_000,
            &event,
            3,
        ),
        Err(ScoringCompletionOutboxError::Completion(
            ScoringJobPersistenceError::JobNotFound
        ))
    ));
    transaction.rollback().unwrap();

    let outbox_count: i64 = client
        .query_one("SELECT count(*) FROM integration_outbox", &[])
        .unwrap()
        .get(0);
    assert_eq!(outbox_count, 0);
}

#[test]
fn completion_outbox_errors_retain_typed_sources() {
    let errors = [
        ScoringCompletionOutboxError::Completion(ScoringJobPersistenceError::InvalidReference),
        ScoringCompletionOutboxError::Outbox(PersistenceError::InvalidReference),
    ];
    for error in errors {
        assert!(!error.to_string().is_empty());
        assert!(error.source().is_some());
    }
}
