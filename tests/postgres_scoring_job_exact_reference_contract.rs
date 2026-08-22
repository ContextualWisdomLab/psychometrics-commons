//! Exact-spelling contracts for persisted scoring-job commands.
//!
//! Persistence must reject padded aliases before any database lookup so a
//! caller cannot collapse a different spelling onto immutable job, worker,
//! lease, failure, or result evidence.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::postgres_scoring_job::{
    apply_scoring_job_migration, cancel_scoring_job, claim_scoring_job,
    record_permanent_scoring_failure, record_retryable_scoring_failure,
    record_successful_scoring_completion, ScoringJobPersistenceError,
};
use std::mem::discriminant;

fn test_client() -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let mut client = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    client
        .batch_execute(
            "CREATE SCHEMA IF NOT EXISTS scoring_job_exact_reference_test;\
             SET search_path TO scoring_job_exact_reference_test;\
             DROP TABLE IF EXISTS scoring_job_state;",
        )
        .unwrap();
    apply_scoring_job_migration(&mut client).unwrap();
    client
}

fn assert_invalid_reference(error: ScoringJobPersistenceError) {
    assert_eq!(
        discriminant(&error),
        discriminant(&ScoringJobPersistenceError::InvalidReference)
    );
}

#[test]
fn persisted_claim_rejects_padded_job_worker_and_lease_aliases_before_lookup() {
    let mut client = test_client();

    for (job_ref, worker_ref, lease_ref) in [
        (" missing_scoring_job", "worker_alpha", "lease_alpha"),
        ("missing_scoring_job", "worker_alpha ", "lease_alpha"),
        ("missing_scoring_job", "worker_alpha", " lease_alpha"),
    ] {
        let mut transaction = client.transaction().unwrap();
        let error = claim_scoring_job(
            &mut transaction,
            job_ref,
            worker_ref,
            lease_ref,
            10_000,
            20_000,
        )
        .unwrap_err();
        assert_invalid_reference(error);
        transaction.rollback().unwrap();
    }
}

#[test]
fn persisted_terminal_commands_reject_padded_evidence_before_lookup() {
    let mut client = test_client();

    {
        let mut transaction = client.transaction().unwrap();
        assert_invalid_reference(
            cancel_scoring_job(&mut transaction, "missing_scoring_job ").unwrap_err(),
        );
        transaction.rollback().unwrap();
    }

    {
        let mut transaction = client.transaction().unwrap();
        assert_invalid_reference(
            record_retryable_scoring_failure(
                &mut transaction,
                "missing_scoring_job",
                1,
                " provider_timeout",
                10_500,
                12_000,
            )
            .unwrap_err(),
        );
        transaction.rollback().unwrap();
    }

    {
        let mut transaction = client.transaction().unwrap();
        assert_invalid_reference(
            record_permanent_scoring_failure(
                &mut transaction,
                "missing_scoring_job",
                1,
                "invalid_contract ",
                10_500,
            )
            .unwrap_err(),
        );
        transaction.rollback().unwrap();
    }

    let mut transaction = client.transaction().unwrap();
    assert_invalid_reference(
        record_successful_scoring_completion(
            &mut transaction,
            "missing_scoring_job",
            1,
            " scoring_result_alpha",
            10_500,
        )
        .unwrap_err(),
    );
    transaction.rollback().unwrap();
}
