//! Real `PostgreSQL` concurrency contract for anonymous participant replay classification.
//!
//! These cases deliberately keep the winning insert uncommitted while a second connection
//! attempts the same `participant_ref`. `PostgreSQL` must make the loser wait for the unique-key
//! conflict to resolve; the following `READ COMMITTED` statement then sees the committed winner
//! and classifies the replay instead of reporting corrupt stored identity.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::participant::ParticipantRecord;
use psychometrics_commons_runtime::postgres_participant::{
    apply_participant_base_migration, persist_anonymous_participant_base,
    ParticipantBasePersistenceDisposition, ParticipantBasePersistenceError,
};
use std::sync::{mpsc, Arc, Barrier};
use std::thread;
use std::time::{Duration, Instant};

const TEST_SCHEMA: &str = "participant_base_concurrency_test";

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
             DROP FUNCTION IF EXISTS reject_assessment_participant_mutation() CASCADE;",
        )
        .unwrap();
    apply_participant_base_migration(client).unwrap();
}

fn wait_for_unique_conflict(observer: &mut Client, contender_pid: i32, winner_pid: i32) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let waiting: bool = observer
            .query_one(
                "SELECT EXISTS (\
                    SELECT 1 \
                    FROM pg_locks AS contender_lock \
                    JOIN pg_locks AS winner_lock \
                      ON winner_lock.locktype = 'transactionid' \
                     AND winner_lock.transactionid = contender_lock.transactionid \
                    WHERE contender_lock.pid = $1 \
                      AND contender_lock.locktype = 'transactionid' \
                      AND contender_lock.granted = FALSE \
                      AND winner_lock.pid = $2 \
                      AND winner_lock.granted = TRUE\
                )",
                &[&contender_pid, &winner_pid],
            )
            .unwrap()
            .get(0);
        if waiting {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "contender backend {contender_pid} did not enter an ungranted transaction-id lock on winner backend {winner_pid}"
        );
        thread::sleep(Duration::from_millis(10));
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ReplayOutcome {
    Duplicate,
    ConflictingReplay,
    Unexpected,
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
    let winner_pid: i32 = winner_transaction
        .query_one("SELECT pg_backend_pid()", &[])
        .unwrap()
        .get(0);

    let barrier = Arc::new(Barrier::new(2));
    let contender_barrier = Arc::clone(&barrier);
    let (pid_sender, pid_receiver) = mpsc::channel();
    let contender = thread::spawn(move || {
        let mut contender_client = test_client();
        let contender_pid: i32 = contender_client
            .query_one("SELECT pg_backend_pid()", &[])
            .unwrap()
            .get(0);
        pid_sender.send(contender_pid).unwrap();
        let contender = ParticipantRecord::new_anonymous(
            participant_ref,
            contender_tenant_ref,
            contender_created_at_unix_ms,
        )
        .unwrap();
        let mut contender_transaction = contender_client.transaction().unwrap();
        contender_barrier.wait();
        let result = persist_anonymous_participant_base(&mut contender_transaction, &contender);
        contender_transaction.rollback().unwrap();
        match result {
            Ok(ParticipantBasePersistenceDisposition::Duplicate) => ReplayOutcome::Duplicate,
            Err(ParticipantBasePersistenceError::ConflictingReplay) => {
                ReplayOutcome::ConflictingReplay
            }
            _ => ReplayOutcome::Unexpected,
        }
    });

    let contender_pid = pid_receiver
        .recv_timeout(Duration::from_secs(2))
        .expect("contender backend must publish its PostgreSQL PID before the race");
    barrier.wait();
    let mut observer = test_client();
    wait_for_unique_conflict(&mut observer, contender_pid, winner_pid);
    winner_transaction.commit().unwrap();

    contender
        .join()
        .expect("concurrent contender must not panic")
}

#[test]
fn concurrent_replay_observes_committed_winner_before_classification() {
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
