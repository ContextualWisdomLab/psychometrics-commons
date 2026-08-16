//! Real `PostgreSQL` contract for atomic scoring failure and outbox persistence.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::integration::IntegrationEvent;
use psychometrics_commons_runtime::postgres_integration::{
    apply_integration_migration, enqueue_outbox_event, PersistenceDisposition, PersistenceError,
};
use psychometrics_commons_runtime::postgres_scoring_failure::{
    record_permanent_scoring_failure_with_outbox, ScoringFailureOutboxError,
};
use psychometrics_commons_runtime::postgres_scoring_job::{
    apply_scoring_job_migration, claim_scoring_job, persist_scoring_job,
    record_permanent_scoring_failure, ScoringJobFailureDisposition, ScoringJobPersistenceError,
};
use psychometrics_commons_runtime::scoring_job::ScoringJob;
use std::error::Error;
use std::sync::{Mutex, MutexGuard};

const DIGEST_A: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const DIGEST_B: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

static FAILURE_TEST_LOCK: Mutex<()> = Mutex::new(());

fn failure_test_guard() -> MutexGuard<'static, ()> {
    FAILURE_TEST_LOCK
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
            "CREATE SCHEMA IF NOT EXISTS scoring_failure_outbox_test;\
             SET search_path TO scoring_failure_outbox_test;",
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
        "worker_failure_alpha",
        "lease_failure_alpha",
        10_000,
        30_000,
    )
    .unwrap();
    let fencing_token = lease.fencing_token();
    transaction.commit().unwrap();
    fencing_token
}

fn failure_event(event_ref: &str, job_ref: &str, digest: &str) -> IntegrationEvent {
    IntegrationEvent::new(
        event_ref,
        "scoring.result.failed",
        "v1",
        "psychometrics_commons",
        "tenant_failure_alpha",
        job_ref,
        20_000,
        "correlation_failure_alpha",
        Some("scoring_request_failure_alpha"),
        digest,
    )
    .unwrap()
}

#[test]
fn failure_and_outbox_commit_and_replay_together() {
    let _guard = failure_test_guard();
    let mut client = test_client();
    reset_and_migrate(&mut client);
    let job_ref = "scoring_job_failure_alpha";
    let fencing_token = persist_and_claim(&mut client, job_ref, "scoring_request_failure_alpha");
    let event = failure_event("event_failure_alpha", job_ref, DIGEST_A);

    let mut transaction = client.transaction().unwrap();
    let inserted = record_permanent_scoring_failure_with_outbox(
        &mut transaction,
        job_ref,
        fencing_token,
        "invalid_scientific_evidence",
        20_000,
        &event,
        3,
    )
    .unwrap();
    assert_eq!(
        inserted.failure(),
        ScoringJobFailureDisposition::Quarantined
    );
    assert_eq!(inserted.outbox(), PersistenceDisposition::Inserted);
    transaction.commit().unwrap();

    let mut transaction = client.transaction().unwrap();
    let duplicate = record_permanent_scoring_failure_with_outbox(
        &mut transaction,
        job_ref,
        fencing_token,
        "invalid_scientific_evidence",
        20_000,
        &event,
        3,
    )
    .unwrap();
    assert_eq!(duplicate.failure(), ScoringJobFailureDisposition::Duplicate);
    assert_eq!(duplicate.outbox(), PersistenceDisposition::Duplicate);
    transaction.commit().unwrap();

    let row = client
        .query_one(
            "SELECT scoring_state, last_failure_code, result_ref \
             FROM scoring_job_state WHERE scoring_job_ref = $1",
            &[&job_ref],
        )
        .unwrap();
    let state: String = row.get(0);
    let stored_cause: Option<String> = row.get(1);
    let stored_result_ref: Option<String> = row.get(2);
    assert_eq!(state, "quarantined");
    assert_eq!(stored_cause.as_deref(), Some("invalid_scientific_evidence"));
    assert_eq!(stored_result_ref, None);
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
fn exact_legacy_failure_then_helper_inserts_the_missing_outbox() {
    let _guard = failure_test_guard();
    let mut client = test_client();
    reset_and_migrate(&mut client);
    let job_ref = "scoring_job_failure_legacy_quarantine";
    let fencing_token = persist_and_claim(
        &mut client,
        job_ref,
        "scoring_request_failure_legacy_quarantine",
    );
    let event = failure_event("event_failure_legacy_quarantine", job_ref, DIGEST_A);

    let mut transaction = client.transaction().unwrap();
    assert_eq!(
        record_permanent_scoring_failure(
            &mut transaction,
            job_ref,
            fencing_token,
            "invalid_scientific_evidence",
            20_000,
        )
        .unwrap(),
        ScoringJobFailureDisposition::Quarantined
    );
    transaction.commit().unwrap();

    let mut transaction = client.transaction().unwrap();
    let reconciled = record_permanent_scoring_failure_with_outbox(
        &mut transaction,
        job_ref,
        fencing_token,
        "invalid_scientific_evidence",
        20_000,
        &event,
        3,
    )
    .unwrap();
    assert_eq!(
        reconciled.failure(),
        ScoringJobFailureDisposition::Duplicate
    );
    assert_eq!(reconciled.outbox(), PersistenceDisposition::Inserted);
    transaction.commit().unwrap();

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
fn exact_legacy_outbox_then_helper_quarantines_the_leased_job() {
    let _guard = failure_test_guard();
    let mut client = test_client();
    reset_and_migrate(&mut client);
    let job_ref = "scoring_job_failure_legacy_outbox";
    let fencing_token = persist_and_claim(
        &mut client,
        job_ref,
        "scoring_request_failure_legacy_outbox",
    );
    let event = failure_event("event_failure_legacy_outbox", job_ref, DIGEST_A);
    assert_eq!(
        enqueue_outbox_event(&mut client, &event, 3).unwrap(),
        PersistenceDisposition::Inserted
    );

    let mut transaction = client.transaction().unwrap();
    let reconciled = record_permanent_scoring_failure_with_outbox(
        &mut transaction,
        job_ref,
        fencing_token,
        "invalid_scientific_evidence",
        20_000,
        &event,
        3,
    )
    .unwrap();
    assert_eq!(
        reconciled.failure(),
        ScoringJobFailureDisposition::Quarantined
    );
    assert_eq!(reconciled.outbox(), PersistenceDisposition::Duplicate);
    transaction.commit().unwrap();

    let state: String = client
        .query_one(
            "SELECT scoring_state FROM scoring_job_state WHERE scoring_job_ref = $1",
            &[&job_ref],
        )
        .unwrap()
        .get(0);
    assert_eq!(state, "quarantined");
}

#[test]
fn late_outbox_conflict_rolls_back_the_failure_transition() {
    let _guard = failure_test_guard();
    let mut client = test_client();
    reset_and_migrate(&mut client);
    let job_ref = "scoring_job_failure_conflict";
    let fencing_token = persist_and_claim(&mut client, job_ref, "scoring_request_failure_conflict");
    let existing_event = failure_event("event_failure_conflict", job_ref, DIGEST_A);
    assert_eq!(
        enqueue_outbox_event(&mut client, &existing_event, 3).unwrap(),
        PersistenceDisposition::Inserted
    );
    let conflicting_event = failure_event("event_failure_conflict", job_ref, DIGEST_B);

    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        record_permanent_scoring_failure_with_outbox(
            &mut transaction,
            job_ref,
            fencing_token,
            "invalid_scientific_evidence",
            20_000,
            &conflicting_event,
            3,
        ),
        Err(ScoringFailureOutboxError::Outbox(
            PersistenceError::ConflictingReplay
        ))
    ));
    transaction.rollback().unwrap();

    let row = client
        .query_one(
            "SELECT scoring_state, last_failure_code, active_fencing_token \
             FROM scoring_job_state WHERE scoring_job_ref = $1",
            &[&job_ref],
        )
        .unwrap();
    let state: String = row.get(0);
    let stored_cause: Option<String> = row.get(1);
    let stored_fencing_token: Option<i64> = row.get(2);
    let expected_fencing_token = i64::try_from(fencing_token)
        .expect("persisted scoring fencing token must fit PostgreSQL BIGINT");
    assert_eq!(state, "leased");
    assert_eq!(stored_cause, None);
    assert_eq!(stored_fencing_token, Some(expected_fencing_token));
}

#[test]
fn failure_does_not_enqueue_when_the_job_is_missing() {
    let _guard = failure_test_guard();
    let mut client = test_client();
    reset_and_migrate(&mut client);
    let event = failure_event(
        "event_failure_missing_job",
        "scoring_job_failure_missing",
        DIGEST_A,
    );

    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        record_permanent_scoring_failure_with_outbox(
            &mut transaction,
            "scoring_job_failure_missing",
            1,
            "invalid_scientific_evidence",
            20_000,
            &event,
            3,
        ),
        Err(ScoringFailureOutboxError::Failure(
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
fn failure_outbox_errors_retain_typed_sources() {
    let envelope = ScoringFailureOutboxError::InvalidFailureEnvelope;
    assert_eq!(
        envelope.to_string(),
        "scoring failure outbox must bind the exact job and failure time"
    );
    assert!(envelope.source().is_none());

    let errors = [
        (
            ScoringFailureOutboxError::Failure(ScoringJobPersistenceError::InvalidReference),
            "scoring failure persistence failed",
        ),
        (
            ScoringFailureOutboxError::Outbox(PersistenceError::InvalidReference),
            "scoring failure outbox persistence failed",
        ),
    ];
    for (error, expected) in errors {
        assert_eq!(error.to_string(), expected);
        assert!(error.source().is_some());
    }
}
