//! Concurrency contract for idempotent append-only audit persistence.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::audit::{AuditEvidence, AuditEvidenceInput, AuditOutcome};
use psychometrics_commons_runtime::postgres_audit::{
    apply_audit_evidence_migration, persist_audit_evidence, AuditPersistenceDisposition,
};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

const DIGEST: &str = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn evidence() -> AuditEvidence {
    AuditEvidence::new(AuditEvidenceInput {
        audit_event_ref: "audit_event_concurrent_replay_01",
        tenant_ref: "tenant_research_alpha",
        actor_ref: "actor_publisher_alpha",
        purpose_code: "instrument_publication",
        action_code: "publish_instrument_release",
        resource_ref: "instrument_release_big_five_ko_v1",
        outcome: AuditOutcome::Succeeded,
        evidence_digest: DIGEST,
        occurred_at_unix_ms: 1_785_000_000_000,
    })
    .unwrap()
}

#[test]
fn concurrent_exact_replay_observes_committed_winner_under_read_committed() {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let mut first_client = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    first_client
        .batch_execute(
            "DROP SCHEMA IF EXISTS audit_concurrency_test CASCADE;\
             CREATE SCHEMA audit_concurrency_test;\
             SET search_path TO audit_concurrency_test;",
        )
        .unwrap();
    apply_audit_evidence_migration(&mut first_client).unwrap();

    let first = evidence();
    let mut first_transaction = first_client.transaction().unwrap();
    assert_eq!(
        persist_audit_evidence(&mut first_transaction, &first).unwrap(),
        AuditPersistenceDisposition::Inserted
    );

    let replay = first.clone();
    let mut replay_config: postgres::Config = connection
        .parse()
        .expect("TEST_DATABASE_URL must parse as a PostgreSQL URL or libpq keyword/value string");
    replay_config.application_name("audit_concurrency_replay");
    let (started_sender, started_receiver) = mpsc::channel();
    let replay_thread = thread::spawn(move || {
        let mut replay_client = replay_config.connect(NoTls).unwrap();
        replay_client
            .batch_execute("SET search_path TO audit_concurrency_test;")
            .unwrap();
        let mut replay_transaction = replay_client.transaction().unwrap();
        started_sender.send(()).unwrap();
        let result = persist_audit_evidence(&mut replay_transaction, &replay)
            .map_err(|error| error.to_string());
        if result.is_ok() {
            replay_transaction.commit().unwrap();
        } else {
            replay_transaction.rollback().unwrap();
        }
        result
    });
    started_receiver.recv().unwrap();

    let mut observer = Client::connect(&connection, NoTls).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut replay_waited_for_winner = false;
    while Instant::now() < deadline {
        let waiting: bool = observer
            .query_one(
                "SELECT count(*) > 0 \
                 FROM pg_stat_activity \
                 WHERE application_name = 'audit_concurrency_replay' \
                   AND state = 'active' \
                   AND wait_event_type = 'Lock'",
                &[],
            )
            .unwrap()
            .get(0);
        if waiting {
            replay_waited_for_winner = true;
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }
    assert!(
        replay_waited_for_winner,
        "replay must reach the unique-key wait before the winning transaction commits"
    );

    first_transaction.commit().unwrap();
    assert_eq!(
        replay_thread.join().unwrap().unwrap(),
        AuditPersistenceDisposition::Duplicate,
        "a committed concurrent exact replay must remain idempotent"
    );
}
