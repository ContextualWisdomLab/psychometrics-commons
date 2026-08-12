//! Real `PostgreSQL` contract for durable scoring-job enqueue and lease fencing.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::postgres_scoring_job::{
    apply_scoring_job_migration, claim_scoring_job, persist_scoring_job,
    ScoringJobPersistenceDisposition, ScoringJobPersistenceError,
};
use psychometrics_commons_runtime::scoring_job::ScoringJob;
use std::mem::discriminant;

fn test_client() -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    Client::connect(&connection, NoTls).expect("isolated CI PostgreSQL database must be reachable")
}

fn reset_scoring_job_table(client: &mut Client) {
    client
        .batch_execute("DROP TABLE IF EXISTS scoring_job_state;")
        .unwrap();
}

fn queued_job(job_ref: &str, request_ref: &str, max_attempts: u32) -> ScoringJob {
    ScoringJob::new(job_ref, request_ref, max_attempts).unwrap()
}

#[test]
fn scoring_job_enqueue_is_exactly_idempotent_and_conflicts_fail_closed() {
    let mut client = test_client();
    reset_scoring_job_table(&mut client);
    apply_scoring_job_migration(&mut client).unwrap();

    let job = queued_job("scoring_job_alpha", "scoring_request_alpha", 3);
    {
        let mut transaction = client.transaction().unwrap();
        assert_eq!(
            persist_scoring_job(&mut transaction, &job).unwrap(),
            ScoringJobPersistenceDisposition::Inserted
        );
        transaction.commit().unwrap();
    }
    {
        let mut transaction = client.transaction().unwrap();
        assert_eq!(
            persist_scoring_job(&mut transaction, &job).unwrap(),
            ScoringJobPersistenceDisposition::Duplicate
        );
        transaction.commit().unwrap();
    }

    let conflicting_request = queued_job("scoring_job_alpha", "scoring_request_beta", 3);
    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        persist_scoring_job(&mut transaction, &conflicting_request),
        Err(ScoringJobPersistenceError::ConflictingReplay)
    ));
    transaction.rollback().unwrap();
}

#[test]
fn claim_is_atomic_and_issues_monotonic_fencing_evidence() {
    let mut client = test_client();
    reset_scoring_job_table(&mut client);
    apply_scoring_job_migration(&mut client).unwrap();

    let job = queued_job("scoring_job_claim", "scoring_request_claim", 3);
    {
        let mut transaction = client.transaction().unwrap();
        persist_scoring_job(&mut transaction, &job).unwrap();
        transaction.commit().unwrap();
    }

    let lease = {
        let mut transaction = client.transaction().unwrap();
        let lease = claim_scoring_job(
            &mut transaction,
            "scoring_job_claim",
            "worker_alpha",
            "scoring_lease_alpha",
            10_000,
            11_000,
        )
        .unwrap();
        transaction.commit().unwrap();
        lease
    };
    assert_eq!(lease.worker_ref(), "worker_alpha");
    assert_eq!(lease.lease_ref(), "scoring_lease_alpha");
    assert_eq!(lease.fencing_token(), 1);
    assert_eq!(lease.expires_at_unix_ms(), 11_000);

    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        claim_scoring_job(
            &mut transaction,
            "scoring_job_claim",
            "worker_beta",
            "scoring_lease_beta",
            10_500,
            11_500,
        ),
        Err(ScoringJobPersistenceError::NotLeaseable)
    ));
    transaction.rollback().unwrap();
}

#[test]
fn invalid_claim_evidence_fails_before_persistence_mutation() {
    let mut client = test_client();
    reset_scoring_job_table(&mut client);
    apply_scoring_job_migration(&mut client).unwrap();

    let job = queued_job("scoring_job_invalid", "scoring_request_invalid", 2);
    {
        let mut transaction = client.transaction().unwrap();
        persist_scoring_job(&mut transaction, &job).unwrap();
        transaction.commit().unwrap();
    }

    for (worker_ref, lease_ref, claimed_at, expires_at, expected) in [
        (
            "123",
            "scoring_lease_invalid",
            10_000,
            11_000,
            ScoringJobPersistenceError::InvalidReference,
        ),
        (
            "worker_invalid",
            "scoring_lease_invalid",
            0,
            11_000,
            ScoringJobPersistenceError::InvalidTimestamp,
        ),
        (
            "worker_invalid",
            "scoring_lease_invalid",
            10_000,
            10_000,
            ScoringJobPersistenceError::InvalidLeaseWindow,
        ),
    ] {
        let mut transaction = client.transaction().unwrap();
        let error = claim_scoring_job(
            &mut transaction,
            "scoring_job_invalid",
            worker_ref,
            lease_ref,
            claimed_at,
            expires_at,
        )
        .unwrap_err();
        assert_eq!(discriminant(&error), discriminant(&expected));
        transaction.rollback().unwrap();
    }

    let row = client
        .query_one(
            "SELECT scoring_state, attempt_count, active_lease_ref \
             FROM scoring_job_state WHERE scoring_job_ref = 'scoring_job_invalid'",
            &[],
        )
        .unwrap();
    assert_eq!(row.get::<_, String>(0), "queued");
    assert_eq!(row.get::<_, i32>(1), 0);
    assert_eq!(row.get::<_, Option<String>>(2), None);
}
