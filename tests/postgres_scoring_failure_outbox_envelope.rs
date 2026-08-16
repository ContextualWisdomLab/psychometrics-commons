//! Scoring failure must not commit with outbox evidence bound to another failure.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::integration::IntegrationEvent;
use psychometrics_commons_runtime::postgres_integration::apply_integration_migration;
use psychometrics_commons_runtime::postgres_scoring_failure::{
    record_permanent_scoring_failure_with_outbox, ScoringFailureOutboxError,
};
use psychometrics_commons_runtime::postgres_scoring_job::{
    apply_scoring_job_migration, claim_scoring_job, persist_scoring_job,
};
use psychometrics_commons_runtime::scoring_job::ScoringJob;

const SCHEMA: &str = "scoring_failure_outbox_envelope_test";
const DATABASE_TEST_LOCK_KEY: i64 = 0x5343_4641_494C_454E;
const DIGEST: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn ready_client() -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let mut client = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    client
        .query_one("SELECT pg_advisory_lock($1)", &[&DATABASE_TEST_LOCK_KEY])
        .expect("shared scoring-failure envelope test lock should be acquired");
    client
        .batch_execute(&format!(
            "DROP SCHEMA IF EXISTS {SCHEMA} CASCADE;
             CREATE SCHEMA {SCHEMA};
             SET search_path TO {SCHEMA};"
        ))
        .unwrap();
    apply_integration_migration(&mut client).unwrap();
    apply_scoring_job_migration(&mut client).unwrap();
    client
}

fn persist_and_claim(client: &mut Client, job_ref: &str) -> u64 {
    let job = ScoringJob::new(job_ref, "scoring_request_failure_envelope", 3).unwrap();
    let mut transaction = client.transaction().unwrap();
    persist_scoring_job(&mut transaction, &job).unwrap();
    transaction.commit().unwrap();

    let mut transaction = client.transaction().unwrap();
    let lease = claim_scoring_job(
        &mut transaction,
        job_ref,
        "worker_failure_envelope",
        "lease_failure_envelope",
        10_000,
        30_000,
    )
    .unwrap();
    let fencing_token = lease.fencing_token();
    transaction.commit().unwrap();
    fencing_token
}

fn event(source: &str, subject: &str, occurred_at_unix_ms: u64) -> IntegrationEvent {
    IntegrationEvent::new(
        "event_scoring_failure_envelope",
        "scoring.result.failed",
        "v1",
        source,
        "tenant_failure_envelope",
        subject,
        occurred_at_unix_ms,
        "correlation_failure_envelope",
        Some("scoring_request_failure_envelope"),
        DIGEST,
    )
    .unwrap()
}

#[test]
fn unrelated_source_subject_or_time_is_rejected_before_failure_write() {
    let invalid_envelopes = [
        ("other_source", "scoring_job_failure_envelope_alpha", 20_000),
        (
            "psychometrics_commons",
            "scoring_job_failure_envelope_other",
            20_000,
        ),
        (
            "psychometrics_commons",
            "scoring_job_failure_envelope_alpha",
            20_001,
        ),
    ];

    for (index, (source, subject, occurred_at_unix_ms)) in invalid_envelopes.into_iter().enumerate()
    {
        let mut client = ready_client();
        let job_ref = "scoring_job_failure_envelope_alpha";
        let fencing_token = persist_and_claim(&mut client, job_ref);
        let failure_event = event(source, subject, occurred_at_unix_ms);

        let mut transaction = client.transaction().unwrap();
        assert!(matches!(
            record_permanent_scoring_failure_with_outbox(
                &mut transaction,
                job_ref,
                fencing_token,
                "invalid_scientific_evidence",
                20_000,
                &failure_event,
                3,
            ),
            Err(ScoringFailureOutboxError::InvalidFailureEnvelope)
        ));
        transaction.rollback().unwrap();

        let state: String = client
            .query_one(
                "SELECT scoring_state FROM scoring_job_state WHERE scoring_job_ref = $1",
                &[&job_ref],
            )
            .unwrap()
            .get(0);
        let outbox_count: i64 = client
            .query_one("SELECT count(*) FROM integration_outbox", &[])
            .unwrap()
            .get(0);
        assert_eq!(
            state, "leased",
            "invalid envelope case {index} must not quarantine"
        );
        assert_eq!(
            outbox_count, 0,
            "invalid envelope case {index} must not enqueue"
        );
    }

    let mut client = ready_client();
    client
        .batch_execute(&format!(
            "SET search_path TO public;
             DROP SCHEMA IF EXISTS {SCHEMA} CASCADE;"
        ))
        .unwrap();
}
