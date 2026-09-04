//! Real `PostgreSQL` acceptance for scoring-engine failure routing.
//!
//! Deterministic scientific failures and request/result provenance mismatches must
//! quarantine the currently leased scoring job immediately. Unclassified provider
//! failures remain retryable and consume the existing bounded retry budget.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::postgres_scoring_job::{
    apply_scoring_job_migration, claim_scoring_job, persist_scoring_job,
};
use psychometrics_commons_runtime::scoring_dispatch::record_scoring_execution_failure;
use psychometrics_commons_runtime::scoring_engine::{
    ScientificScoringFailure, ScoringEngineExecutionError,
};
use psychometrics_commons_runtime::scoring_job::{ScoringJob, ScoringJobState};
use std::error::Error;
use std::fmt::{Display, Formatter};

const SCHEMA: &str = "scoring_failure_routing_test";

#[derive(Debug)]
struct EngineFailure;

impl Display for EngineFailure {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("engine failure details")
    }
}

impl Error for EngineFailure {}

fn test_client() -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let mut client = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    client
        .batch_execute(&format!(
            "DROP SCHEMA IF EXISTS {SCHEMA} CASCADE;\
             CREATE SCHEMA {SCHEMA};\
             SET search_path TO {SCHEMA};"
        ))
        .unwrap();
    apply_scoring_job_migration(&mut client).unwrap();
    client
}

fn persist_and_claim(client: &mut Client, job_ref: &str, max_attempts: u32) {
    let request_ref = format!("request_{job_ref}");
    let job = ScoringJob::new(job_ref, request_ref, max_attempts).unwrap();
    let mut transaction = client.transaction().unwrap();
    persist_scoring_job(&mut transaction, &job).unwrap();
    claim_scoring_job(
        &mut transaction,
        job_ref,
        "worker_scoring_failure_routing",
        &format!("lease_{job_ref}"),
        10_000,
        11_000,
    )
    .unwrap();
    transaction.commit().unwrap();
}

fn stored_state(client: &mut Client, job_ref: &str) -> (String, Option<String>, Option<i64>) {
    let row = client
        .query_one(
            "SELECT scoring_state, last_failure_code, next_attempt_at_unix_ms \
             FROM scoring_job_state WHERE scoring_job_ref = $1",
            &[&job_ref],
        )
        .unwrap();
    (row.get(0), row.get(1), row.get(2))
}

#[test]
fn durable_routing_separates_scientific_integrity_and_provider_failures() {
    let mut client = test_client();

    persist_and_claim(&mut client, "job_scientific_non_identification", 3);
    {
        let error = ScoringEngineExecutionError::Scientific {
            failure: ScientificScoringFailure::NonIdentification,
            source: EngineFailure,
        };
        let mut transaction = client.transaction().unwrap();
        assert_eq!(
            record_scoring_execution_failure(
                &mut transaction,
                "job_scientific_non_identification",
                1,
                &error,
                10_500,
                12_000,
            )
            .unwrap(),
            ScoringJobState::Quarantined
        );
        transaction.commit().unwrap();
    }
    assert_eq!(
        stored_state(&mut client, "job_scientific_non_identification"),
        (
            "quarantined".to_owned(),
            Some("non_identification".to_owned()),
            None,
        )
    );

    persist_and_claim(&mut client, "job_provider_unavailable", 3);
    {
        let error = ScoringEngineExecutionError::Engine(EngineFailure);
        let mut transaction = client.transaction().unwrap();
        assert_eq!(
            record_scoring_execution_failure(
                &mut transaction,
                "job_provider_unavailable",
                1,
                &error,
                10_500,
                12_000,
            )
            .unwrap(),
            ScoringJobState::RetryScheduled
        );
        transaction.commit().unwrap();
    }
    assert_eq!(
        stored_state(&mut client, "job_provider_unavailable"),
        (
            "retry_scheduled".to_owned(),
            Some("scoring_engine_failure".to_owned()),
            Some(12_000),
        )
    );

    persist_and_claim(&mut client, "job_request_mismatch", 3);
    {
        let error: ScoringEngineExecutionError<EngineFailure> =
            ScoringEngineExecutionError::RequestMismatch;
        let mut transaction = client.transaction().unwrap();
        assert_eq!(
            record_scoring_execution_failure(
                &mut transaction,
                "job_request_mismatch",
                1,
                &error,
                10_500,
                12_000,
            )
            .unwrap(),
            ScoringJobState::Quarantined
        );
        transaction.commit().unwrap();
    }
    assert_eq!(
        stored_state(&mut client, "job_request_mismatch"),
        (
            "quarantined".to_owned(),
            Some("scoring_request_mismatch".to_owned()),
            None,
        )
    );
}
