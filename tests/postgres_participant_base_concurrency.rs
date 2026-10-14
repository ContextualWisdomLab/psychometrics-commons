//! Deterministic `PostgreSQL` evidence for anonymous participant replay classification.
//!
//! The adapter inserts with `ON CONFLICT DO NOTHING` and then rereads the winner under
//! `READ COMMITTED`. These cases prove two complementary facts without a fixed sleep:
//! an uncommitted winner makes a conflicting persist wait (`lock_timeout` SQLSTATE `55P03`),
//! and a waiting persist classifies the committed winner as `Duplicate` or `ConflictingReplay`
//! rather than inventing corrupt stored identity.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::participant::ParticipantRecord;
use psychometrics_commons_runtime::postgres_participant::{
    apply_participant_base_migration, persist_anonymous_participant_base,
    ParticipantBasePersistenceDisposition, ParticipantBasePersistenceError,
};
use std::sync::{mpsc, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

const TEST_SCHEMA: &str = "participant_base_concurrency_test";
static PARTICIPANT_CONCURRENCY_TEST_LOCK: Mutex<()> = Mutex::new(());

fn participant_concurrency_test_guard() -> MutexGuard<'static, ()> {
    PARTICIPANT_CONCURRENCY_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn test_client() -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let mut client = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    client
        .batch_execute(&format!(
            "CREATE SCHEMA IF NOT EXISTS {TEST_SCHEMA}; SET search_path TO {TEST_SCHEMA};"
        ))
        .unwrap();
    client
}

fn prepare_schema(client: &mut Client) {
    client
        .batch_execute(
            "DROP TABLE IF EXISTS assessment_participant;\
             DROP FUNCTION IF EXISTS reject_assessment_participant_mutation() CASCADE;\
             DROP FUNCTION IF EXISTS opaque_reference_numeric_like(TEXT);",
        )
        .unwrap();
    apply_participant_base_migration(client).unwrap();
}

fn assert_unique_key_is_held(observer: &mut Client, participant_ref: &str) {
    observer
        .batch_execute("SET lock_timeout TO '100ms';")
        .unwrap();
    let error = observer
        .execute(
            "INSERT INTO assessment_participant \
             (participant_ref, tenant_ref, created_at_unix_ms) \
             VALUES ($1, 'tenant_lock_probe', 1)",
            &[&participant_ref],
        )
        .expect_err("an uncommitted winner must block a conflicting unique insert immediately");
    assert_eq!(
        error.code().map(postgres::error::SqlState::code),
        Some("55P03"),
        "unique-key contention must surface as lock_timeout, not a later classification: {error}"
    );
    observer
        .batch_execute("ROLLBACK; SET lock_timeout TO DEFAULT;")
        .unwrap();
}

fn wait_until_blocked(observer: &mut Client, backend_pid: i32) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let blocked: bool = observer
            .query_one(
                "SELECT cardinality(pg_blocking_pids($1)) > 0",
                &[&backend_pid],
            )
            .unwrap()
            .get(0);
        if blocked {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "contender backend {backend_pid} did not become blocked on the uncommitted winner"
        );
        thread::sleep(Duration::from_millis(10));
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ReplayOutcome {
    Duplicate,
    ConflictingReplay,
}

fn race_replay(
    winner_client: &mut Client,
    participant_ref: &'static str,
    contender_tenant_ref: &'static str,
    contender_created_at_unix_ms: u64,
) -> ReplayOutcome {
    let winner =
        ParticipantRecord::new_anonymous(participant_ref, "tenant_concurrency_demo", 40_000)
            .unwrap();
    let mut winner_transaction = winner_client.transaction().unwrap();
    assert_eq!(
        persist_anonymous_participant_base(&mut winner_transaction, &winner).unwrap(),
        ParticipantBasePersistenceDisposition::Inserted
    );

    let mut probe = test_client();
    assert_unique_key_is_held(&mut probe, participant_ref);

    let (pid_sender, pid_receiver) = mpsc::channel();
    let (ready_sender, ready_receiver) = mpsc::channel();
    let contender = thread::spawn(move || {
        let mut contender_client = test_client();
        let contender_pid: i32 = contender_client
            .query_one("SELECT pg_backend_pid()", &[])
            .unwrap()
            .get(0);
        pid_sender.send(contender_pid).unwrap();
        ready_receiver.recv().unwrap();
        let contender = ParticipantRecord::new_anonymous(
            participant_ref,
            contender_tenant_ref,
            contender_created_at_unix_ms,
        )
        .unwrap();
        let mut contender_transaction = contender_client.transaction().unwrap();
        let result = persist_anonymous_participant_base(&mut contender_transaction, &contender);
        contender_transaction.rollback().unwrap();
        match result {
            Ok(ParticipantBasePersistenceDisposition::Duplicate) => ReplayOutcome::Duplicate,
            Err(ParticipantBasePersistenceError::ConflictingReplay) => {
                ReplayOutcome::ConflictingReplay
            }
            other => panic!("concurrent persist classified unexpectedly: {other:?}"),
        }
    });

    let contender_pid = pid_receiver
        .recv_timeout(Duration::from_secs(2))
        .expect("contender backend must publish its PostgreSQL PID before persist");
    ready_sender.send(()).unwrap();
    wait_until_blocked(&mut probe, contender_pid);
    winner_transaction.commit().unwrap();

    contender
        .join()
        .expect("concurrent contender must not panic")
}

#[test]
fn uncommitted_winner_makes_conflicting_persist_wait() {
    let _guard = participant_concurrency_test_guard();
    let mut winner_client = test_client();
    prepare_schema(&mut winner_client);
    let winner = ParticipantRecord::new_anonymous(
        "participant_concurrency_lock",
        "tenant_concurrency_demo",
        40_000,
    )
    .unwrap();
    let mut winner_transaction = winner_client.transaction().unwrap();
    assert_eq!(
        persist_anonymous_participant_base(&mut winner_transaction, &winner).unwrap(),
        ParticipantBasePersistenceDisposition::Inserted
    );

    let mut contender_client = test_client();
    contender_client
        .batch_execute("SET lock_timeout TO '100ms';")
        .unwrap();
    let mut contender_transaction = contender_client.transaction().unwrap();
    let error = persist_anonymous_participant_base(&mut contender_transaction, &winner)
        .expect_err("conflicting persist must wait on the uncommitted unique key");
    match error {
        ParticipantBasePersistenceError::Database(database_error) => {
            assert_eq!(
                database_error.code().map(postgres::error::SqlState::code),
                Some("55P03"),
                "waiting persist must fail closed as lock_timeout, not as corrupt identity"
            );
        }
        other => panic!("expected a database lock_timeout, got {other:?}"),
    }
    contender_transaction.rollback().unwrap();
    winner_transaction.rollback().unwrap();
}

#[test]
fn concurrent_replay_observes_committed_winner_before_classification() {
    let _guard = participant_concurrency_test_guard();
    let mut winner_client = test_client();
    prepare_schema(&mut winner_client);

    assert_eq!(
        race_replay(
            &mut winner_client,
            "participant_concurrency_duplicate",
            "tenant_concurrency_demo",
            40_000,
        ),
        ReplayOutcome::Duplicate,
        "an exact concurrent replay must observe the committed winner and remain idempotent"
    );
    assert_eq!(
        race_replay(
            &mut winner_client,
            "participant_concurrency_tenant_conflict",
            "tenant_other_concurrency",
            40_000,
        ),
        ReplayOutcome::ConflictingReplay,
        "a concurrent replay cannot rebind the participant to another tenant"
    );
    assert_eq!(
        race_replay(
            &mut winner_client,
            "participant_concurrency_time_conflict",
            "tenant_concurrency_demo",
            40_001,
        ),
        ReplayOutcome::ConflictingReplay,
        "a concurrent replay cannot rewrite the server-authoritative creation time"
    );
}
