//! Real `PostgreSQL` contract for tenant-scoped durable scoring-job lease fencing.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::postgres_scoring_job::{
    apply_scoring_job_migration, claim_scoring_job, complete_scoring_job, persist_scoring_job,
    ScoringJobPersistenceDisposition, ScoringJobPersistenceError, ScoringJobPersistenceIdentity,
};
use psychometrics_commons_runtime::scoring_job::ScoringJob;

const DATABASE_TEST_LOCK_KEY: i64 = 0x5053_5943_5343_4F52;

fn test_client() -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    Client::connect(&connection, NoTls).expect("isolated CI PostgreSQL database must be reachable")
}

fn database_test_guard() -> Client {
    let mut client = test_client();
    client
        .query_one("SELECT pg_advisory_lock($1)", &[&DATABASE_TEST_LOCK_KEY])
        .expect("shared PostgreSQL integration-test advisory lock should be acquired");
    client
}

fn reset_scoring_tables(client: &mut Client) {
    client
        .batch_execute("DROP TABLE IF EXISTS scoring_job;")
        .unwrap();
    apply_scoring_job_migration(client).unwrap();
}

fn identity<'a>(tenant_ref: &'a str, scoring_job_ref: &'a str) -> ScoringJobPersistenceIdentity<'a> {
    ScoringJobPersistenceIdentity::new(tenant_ref, scoring_job_ref)
}

#[test]
fn scoring_job_persistence_fences_stale_completion_and_replays_exact_evidence() {
    let _database_guard = database_test_guard();
    let mut client = test_client();
    reset_scoring_tables(&mut client);

    let job = ScoringJob::new("scoring_job_alpha", "scoring_request_alpha", 3).unwrap();
    assert_eq!(
        persist_scoring_job(&mut client, "tenant_alpha", &job).unwrap(),
        ScoringJobPersistenceDisposition::Inserted
    );
    assert_eq!(
        persist_scoring_job(&mut client, "tenant_alpha", &job).unwrap(),
        ScoringJobPersistenceDisposition::Duplicate
    );

    let conflicting = ScoringJob::new("scoring_job_alpha", "scoring_request_beta", 3).unwrap();
    assert!(matches!(
        persist_scoring_job(&mut client, "tenant_alpha", &conflicting),
        Err(ScoringJobPersistenceError::ConflictingReplay)
    ));
    assert_eq!(
        persist_scoring_job(&mut client, "tenant_beta", &conflicting).unwrap(),
        ScoringJobPersistenceDisposition::Inserted
    );

    let lease = claim_scoring_job(
        &mut client,
        identity("tenant_alpha", "scoring_job_alpha"),
        "worker_alpha",
        "lease_alpha",
        10_000,
        10_100,
    )
    .unwrap();
    assert_eq!(lease.fencing_token(), 1);
    assert_eq!(lease.worker_ref(), "worker_alpha");
    assert_eq!(lease.lease_ref(), "lease_alpha");
    assert_eq!(lease.expires_at_unix_ms(), 10_100);

    assert!(matches!(
        claim_scoring_job(
            &mut client,
            identity("tenant_alpha", "scoring_job_alpha"),
            "worker_beta",
            "lease_beta",
            10_010,
            10_110,
        ),
        Err(ScoringJobPersistenceError::NotLeaseable)
    ));

    assert!(matches!(
        complete_scoring_job(
            &mut client,
            identity("tenant_alpha", "scoring_job_alpha"),
            "lease_alpha",
            2,
            "scoring_result_alpha",
            10_050,
        ),
        Err(ScoringJobPersistenceError::StaleLease)
    ));

    assert_eq!(
        complete_scoring_job(
            &mut client,
            identity("tenant_alpha", "scoring_job_alpha"),
            "lease_alpha",
            1,
            "scoring_result_alpha",
            10_050,
        )
        .unwrap(),
        ScoringJobPersistenceDisposition::Applied
    );
    assert_eq!(
        complete_scoring_job(
            &mut client,
            identity("tenant_alpha", "scoring_job_alpha"),
            "lease_alpha",
            1,
            "scoring_result_alpha",
            10_050,
        )
        .unwrap(),
        ScoringJobPersistenceDisposition::Duplicate
    );
    assert!(matches!(
        complete_scoring_job(
            &mut client,
            identity("tenant_alpha", "scoring_job_alpha"),
            "lease_alpha",
            1,
            "scoring_result_beta",
            10_050,
        ),
        Err(ScoringJobPersistenceError::ConflictingCompletion)
    ));
}

#[test]
fn scoring_job_persistence_rejects_expired_worker_authority() {
    let _database_guard = database_test_guard();
    let mut client = test_client();
    reset_scoring_tables(&mut client);

    let job = ScoringJob::new("scoring_job_expiry", "scoring_request_expiry", 2).unwrap();
    persist_scoring_job(&mut client, "tenant_alpha", &job).unwrap();
    let lease = claim_scoring_job(
        &mut client,
        identity("tenant_alpha", "scoring_job_expiry"),
        "worker_expiry",
        "lease_expiry",
        20_000,
        20_100,
    )
    .unwrap();

    assert!(matches!(
        complete_scoring_job(
            &mut client,
            identity("tenant_alpha", "scoring_job_expiry"),
            lease.lease_ref(),
            lease.fencing_token(),
            "scoring_result_expiry",
            20_100,
        ),
        Err(ScoringJobPersistenceError::LeaseExpired)
    ));

    let state: String = client
        .query_one(
            "SELECT current_state FROM scoring_job \
             WHERE tenant_ref = 'tenant_alpha' AND scoring_job_ref = 'scoring_job_expiry'",
            &[],
        )
        .unwrap()
        .get(0);
    assert_eq!(state, "leased");
}
