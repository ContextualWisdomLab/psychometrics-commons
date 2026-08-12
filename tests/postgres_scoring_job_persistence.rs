//! Real `PostgreSQL` contract for durable scoring-job enqueue and lease fencing.

use postgres::{Client, IsolationLevel, NoTls};
use psychometrics_commons_runtime::postgres_scoring_job::{
    apply_scoring_job_migration, claim_scoring_job, persist_scoring_job,
    ScoringJobPersistenceDisposition, ScoringJobPersistenceError,
};
use psychometrics_commons_runtime::scoring_job::ScoringJob;
use std::mem::discriminant;
use std::sync::{Arc, Barrier, Mutex, MutexGuard};
use std::thread;

static SCORING_JOB_TEST_LOCK: Mutex<()> = Mutex::new(());

fn scoring_job_test_guard() -> MutexGuard<'static, ()> {
    SCORING_JOB_TEST_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn test_client() -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let mut client =
        Client::connect(&connection, NoTls).expect("isolated CI PostgreSQL database must be reachable");
    client
        .batch_execute(
            "CREATE SCHEMA IF NOT EXISTS scoring_job_persistence_test;\
             SET search_path TO scoring_job_persistence_test;",
        )
        .unwrap();
    client
}

fn reset_scoring_job_table(client: &mut Client) {
    client
        .batch_execute(
            "DROP TABLE IF EXISTS scoring_job_persistence_test.scoring_job_state;",
        )
        .unwrap();
}

fn queued_job(job_ref: &str, request_ref: &str, max_attempts: u32) -> ScoringJob {
    ScoringJob::new(job_ref, request_ref, max_attempts).unwrap()
}

#[test]
fn scoring_job_enqueue_is_exactly_idempotent_and_conflicts_fail_closed() {
    let _guard = scoring_job_test_guard();
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

    let conflicting_attempt_budget = queued_job("scoring_job_alpha", "scoring_request_alpha", 4);
    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        persist_scoring_job(&mut transaction, &conflicting_attempt_budget),
        Err(ScoringJobPersistenceError::ConflictingReplay)
    ));
    transaction.rollback().unwrap();
}

#[test]
fn enqueue_rejects_nonfresh_jobs_large_attempt_budgets_and_stronger_isolation() {
    let _guard = scoring_job_test_guard();
    let mut client = test_client();
    reset_scoring_job_table(&mut client);
    apply_scoring_job_migration(&mut client).unwrap();

    let mut leased_job = queued_job("scoring_job_nonfresh", "scoring_request_nonfresh", 3);
    leased_job
        .claim("worker_nonfresh", "scoring_lease_nonfresh", 10_000, 11_000)
        .unwrap();
    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        persist_scoring_job(&mut transaction, &leased_job),
        Err(ScoringJobPersistenceError::UnsupportedInitialState)
    ));
    transaction.rollback().unwrap();

    let oversized_job = queued_job(
        "scoring_job_oversized",
        "scoring_request_oversized",
        u32::MAX,
    );
    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        persist_scoring_job(&mut transaction, &oversized_job),
        Err(ScoringJobPersistenceError::ValueOutOfRange)
    ));
    transaction.rollback().unwrap();

    let serializable_job = queued_job(
        "scoring_job_serializable",
        "scoring_request_serializable",
        3,
    );
    let mut transaction = client
        .build_transaction()
        .isolation_level(IsolationLevel::Serializable)
        .start()
        .unwrap();
    assert!(matches!(
        persist_scoring_job(&mut transaction, &serializable_job),
        Err(ScoringJobPersistenceError::UnsupportedIsolationLevel)
    ));
    transaction.rollback().unwrap();
}

#[test]
fn claim_is_atomic_and_issues_monotonic_fencing_evidence() {
    let _guard = scoring_job_test_guard();
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

    {
        let mut transaction = client.transaction().unwrap();
        assert_eq!(
            persist_scoring_job(&mut transaction, &job).unwrap(),
            ScoringJobPersistenceDisposition::Duplicate
        );
        transaction.commit().unwrap();
    }

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
fn concurrent_claimers_receive_exactly_one_persisted_lease() {
    let _guard = scoring_job_test_guard();
    let mut setup_client = test_client();
    reset_scoring_job_table(&mut setup_client);
    apply_scoring_job_migration(&mut setup_client).unwrap();

    let job = queued_job("scoring_job_concurrent", "scoring_request_concurrent", 3);
    {
        let mut transaction = setup_client.transaction().unwrap();
        persist_scoring_job(&mut transaction, &job).unwrap();
        transaction.commit().unwrap();
    }

    let barrier = Arc::new(Barrier::new(2));
    let handles = [
        ("worker_concurrent_alpha", "scoring_lease_concurrent_alpha"),
        ("worker_concurrent_beta", "scoring_lease_concurrent_beta"),
    ]
    .into_iter()
    .map(|(worker_ref, lease_ref)| {
        let barrier = Arc::clone(&barrier);
        thread::spawn(move || {
            let mut client = test_client();
            barrier.wait();
            let mut transaction = client.transaction().unwrap();
            match claim_scoring_job(
                &mut transaction,
                "scoring_job_concurrent",
                worker_ref,
                lease_ref,
                20_000,
                21_000,
            ) {
                Ok(lease) => {
                    let fencing_token = lease.fencing_token();
                    transaction.commit().unwrap();
                    Some(fencing_token)
                }
                Err(ScoringJobPersistenceError::NotLeaseable) => {
                    transaction.rollback().unwrap();
                    None
                }
                Err(error) => panic!("unexpected concurrent claim error: {error:?}"),
            }
        })
    })
    .collect::<Vec<_>>();

    let mut successful_tokens = handles
        .into_iter()
        .filter_map(|handle| handle.join().unwrap())
        .collect::<Vec<_>>();
    successful_tokens.sort_unstable();
    assert_eq!(successful_tokens, vec![1]);

    let row = setup_client
        .query_one(
            "SELECT scoring_state, attempt_count, active_fencing_token \
             FROM scoring_job_state WHERE scoring_job_ref = 'scoring_job_concurrent'",
            &[],
        )
        .unwrap();
    assert_eq!(row.get::<_, String>(0), "leased");
    assert_eq!(row.get::<_, i32>(1), 1);
    assert_eq!(row.get::<_, i64>(2), 1);
}

#[test]
fn invalid_claim_evidence_fails_before_persistence_mutation() {
    let _guard = scoring_job_test_guard();
    let mut client = test_client();
    reset_scoring_job_table(&mut client);
    apply_scoring_job_migration(&mut client).unwrap();

    let job = queued_job("scoring_job_invalid", "scoring_request_invalid", 2);
    {
        let mut transaction = client.transaction().unwrap();
        persist_scoring_job(&mut transaction, &job).unwrap();
        transaction.commit().unwrap();
    }

    for (job_ref, worker_ref, lease_ref, claimed_at, expires_at, expected) in [
        (
            "scoring_job_invalid",
            "123",
            "scoring_lease_invalid",
            10_000,
            11_000,
            ScoringJobPersistenceError::InvalidReference,
        ),
        (
            "123",
            "worker_invalid",
            "scoring_lease_invalid",
            10_000,
            11_000,
            ScoringJobPersistenceError::InvalidReference,
        ),
        (
            "scoring_job_invalid",
            "worker_invalid",
            "123",
            10_000,
            11_000,
            ScoringJobPersistenceError::InvalidReference,
        ),
        (
            "scoring_job_invalid",
            "worker_invalid",
            "scoring_lease_invalid",
            0,
            11_000,
            ScoringJobPersistenceError::InvalidTimestamp,
        ),
        (
            "scoring_job_invalid",
            "worker_invalid",
            "scoring_lease_invalid",
            u64::MAX,
            u64::MAX,
            ScoringJobPersistenceError::ValueOutOfRange,
        ),
        (
            "scoring_job_invalid",
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
            job_ref,
            worker_ref,
            lease_ref,
            claimed_at,
            expires_at,
        )
        .unwrap_err();
        assert_eq!(discriminant(&error), discriminant(&expected));
        transaction.rollback().unwrap();
    }

    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        claim_scoring_job(
            &mut transaction,
            "scoring_job_missing",
            "worker_missing",
            "scoring_lease_missing",
            10_000,
            11_000,
        ),
        Err(ScoringJobPersistenceError::JobNotFound)
    ));
    transaction.rollback().unwrap();

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

#[test]
fn database_constraints_reject_invalid_lease_identity_and_fencing_shape() {
    let _guard = scoring_job_test_guard();
    let mut client = test_client();
    reset_scoring_job_table(&mut client);
    apply_scoring_job_migration(&mut client).unwrap();

    let invalid_worker = client.execute(
        "INSERT INTO scoring_job_state (\
             scoring_job_ref, scoring_request_ref, scoring_state, attempt_count, max_attempts,\
             active_worker_ref, active_lease_ref, active_fencing_token,\
             active_lease_expires_at_unix_ms\
         ) VALUES (\
             'scoring_job_bad_worker', 'scoring_request_bad_worker', 'leased', 1, 3,\
             '123', 'scoring_lease_bad_worker', 1, 11000\
         )",
        &[],
    );
    assert!(invalid_worker.is_err());

    let invalid_lease = client.execute(
        "INSERT INTO scoring_job_state (\
             scoring_job_ref, scoring_request_ref, scoring_state, attempt_count, max_attempts,\
             active_worker_ref, active_lease_ref, active_fencing_token,\
             active_lease_expires_at_unix_ms\
         ) VALUES (\
             'scoring_job_bad_lease', 'scoring_request_bad_lease', 'leased', 1, 3,\
             'worker_bad_lease', '123', 1, 11000\
         )",
        &[],
    );
    assert!(invalid_lease.is_err());

    let mismatched_fence = client.execute(
        "INSERT INTO scoring_job_state (\
             scoring_job_ref, scoring_request_ref, scoring_state, attempt_count, max_attempts,\
             active_worker_ref, active_lease_ref, active_fencing_token,\
             active_lease_expires_at_unix_ms\
         ) VALUES (\
             'scoring_job_bad_fence', 'scoring_request_bad_fence', 'leased', 1, 3,\
             'worker_bad_fence', 'scoring_lease_bad_fence', 2, 11000\
         )",
        &[],
    );
    assert!(mismatched_fence.is_err());
}
