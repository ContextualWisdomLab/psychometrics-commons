//! Real PostgreSQL concurrency contract for anonymous participant replay classification.
//!
//! These cases deliberately keep the winning insert uncommitted while a second connection
//! attempts the same `participant_ref`. PostgreSQL must make the loser wait for the unique-key
//! conflict to resolve; the following `READ COMMITTED` statement then sees the committed winner
//! and classifies the replay instead of reporting corrupt stored identity.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::participant::ParticipantRecord;
use psychometrics_commons_runtime::postgres_participant::{
    apply_participant_base_migration, persist_anonymous_participant_base,
    ParticipantBasePersistenceDisposition, ParticipantBasePersistenceError,
};
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::Duration;

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
    let winner = ParticipantRecord::new_anonymous(participant_ref, "tenant_concurrency_demo", 40_000)
        .unwrap();
    let mut winner_transaction = winner_client.transaction().unwrap();
    assert_eq!(
        persist_anonymous_participant_base(&mut winner_transaction, &winner).unwrap(),
        ParticipantBasePersistenceDisposition::Inserted
    );

    let barrier = Arc::new(Barrier::new(2));
    let contender_barrier = Arc::clone(&barrier);
    let contender = thread::spawn(move || {
        let mut contender_client = test_client();
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

    barrier.wait();
    // Give the contender a deterministic window to enter INSERT ... ON CONFLICT while the
    // winning unique-key row is still uncommitted. The PostgreSQL conflict itself supplies the
    // synchronization; this sleep only prevents the winner from committing before the attempt.
    thread::sleep(Duration::from_millis(100));
    winner_transaction.commit().unwrap();

    contender.join().expect("concurrent contender must not panic")
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
