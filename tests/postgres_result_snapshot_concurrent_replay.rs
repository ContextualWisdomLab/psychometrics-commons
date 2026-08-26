//! Deterministic real-`PostgreSQL` coverage for the exact-replay insert race.
//!
//! A writer can observe no existing result, pause inside its `INSERT`, and then lose the
//! unique-key race to another transaction. Under `READ COMMITTED`, the losing writer must
//! classify the now-committed row as an exact duplicate instead of treating the zero-row
//! `ON CONFLICT DO NOTHING` result as an insertion failure.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::postgres_result_snapshot::{
    apply_result_snapshot_migration, persist_result_snapshot, ResultSnapshotPersistenceDisposition,
};
#[path = "response_support/mod.rs"]
mod response_support;

use psychometrics_commons_runtime::response::ResponseWrite;
use psychometrics_commons_runtime::result::{ResultSnapshot, ResultSnapshotInput};
use psychometrics_commons_runtime::scoring::{
    ScoreObservation, ScoringRequest, ScoringRequestInput, ScoringResult,
};
use response_support::frozen_snapshot;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

const ENGINE_DIGEST: &str =
    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const DATABASE_TEST_LOCK_KEY: i64 = 0x5253_5241_4345_3031;
const INSERT_PAUSE_LOCK_KEY: i64 = 0x5253_5241_4345_3032;
const TEST_SCHEMA: &str = "result_snapshot_concurrent_replay_test";

fn connect_client() -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    Client::connect(&connection, NoTls).expect("isolated CI PostgreSQL database must be reachable")
}

fn connect_in_test_schema() -> Client {
    let mut client = connect_client();
    client
        .batch_execute(&format!("SET search_path TO {TEST_SCHEMA};"))
        .expect("concurrent replay test schema must already exist");
    client
}

fn snapshot() -> ResultSnapshot {
    let response = frozen_snapshot(
        "session_result_concurrent_replay",
        "response_snapshot_result_concurrent_replay",
        &[ResponseWrite {
            server_event_ref: "server_event_result_concurrent_replay",
            client_event_ref: "client_event_result_concurrent_replay",
            item_version_ref: "item_version_001",
            payload_digest: ENGINE_DIGEST,
        }],
    );
    let request = ScoringRequest::from_snapshot(
        &response,
        ScoringRequestInput {
            scoring_request_ref: "scoring_request_result_concurrent_replay",
            response_snapshot_ref: "response_snapshot_result_concurrent_replay",
            assessment_spec_ref: "assessment_spec_big_five_v1",
            instrument_version_ref: "instrument_version_big_five_ko_v1",
            scoring_version_ref: "scoring_version_big_five_v1",
            calibration_reference: "calibration_big_five_ko_v1",
            norm_version_ref: Some("norm_version_big_five_ko_v1"),
            requested_output_schema_version: 1,
        },
    )
    .unwrap();
    let scoring_result = ScoringResult::new(
        "scoring_result_result_concurrent_replay",
        &request,
        ENGINE_DIGEST,
        vec![ScoreObservation::scored("construct_big_five", 0.25, Some(0.05)).unwrap()],
    )
    .unwrap();
    ResultSnapshot::new(
        &request,
        &scoring_result,
        ResultSnapshotInput {
            result_snapshot_ref: "result_snapshot_concurrent_replay",
            participant_ref: "participant_result_concurrent_replay",
            narrative_version_ref: "narrative_version_big_five_v1",
            consent_snapshot_refs: &["consent_snapshot_service_v1"],
            created_at_unix_ms: 70_000,
            supersedes_ref: None,
        },
    )
    .unwrap()
}

fn install_insert_pause_trigger(client: &mut Client) {
    client
        .batch_execute(&format!(
            "CREATE OR REPLACE FUNCTION test_pause_result_snapshot_insert()\
             RETURNS trigger\
             LANGUAGE plpgsql\
             AS $$\
             BEGIN\
                 IF current_setting('psychometrics_commons.test_result_insert_pause', true) = 'on' THEN\
                     PERFORM pg_advisory_xact_lock({INSERT_PAUSE_LOCK_KEY});\
                 END IF;\
                 RETURN NEW;\
             END;\
             $$;\
             CREATE TRIGGER result_snapshot_zz_test_pause_insert\
             BEFORE INSERT ON result_snapshot\
             FOR EACH ROW EXECUTE FUNCTION test_pause_result_snapshot_insert();"
        ))
        .expect("test-only result insert pause trigger must install");
}

fn wait_until_worker_is_blocked_on_advisory_lock(controller: &mut Client, worker_pid: i32) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let waiting: bool = controller
            .query_one(
                "SELECT EXISTS (\
                    SELECT 1 FROM pg_locks\
                    WHERE pid = $1 AND locktype = 'advisory' AND NOT granted\
                 )",
                &[&worker_pid],
            )
            .expect("controller must be able to observe its sibling test session's lock wait")
            .get(0);
        if waiting {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "worker never reached the deterministic pre-insert advisory-lock pause"
        );
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn concurrent_exact_insert_winner_is_reclassified_as_duplicate() {
    let mut guard = connect_client();
    guard
        .query_one("SELECT pg_advisory_lock($1)", &[&DATABASE_TEST_LOCK_KEY])
        .expect("shared PostgreSQL concurrent-replay fixture lock should be acquired");

    let mut controller = connect_client();
    controller
        .batch_execute(&format!(
            "DROP SCHEMA IF EXISTS {TEST_SCHEMA} CASCADE;\
             CREATE SCHEMA {TEST_SCHEMA};\
             SET search_path TO {TEST_SCHEMA};"
        ))
        .expect("clean concurrent replay schema should be created");
    apply_result_snapshot_migration(&mut controller).unwrap();
    install_insert_pause_trigger(&mut controller);
    controller
        .query_one("SELECT pg_advisory_lock($1)", &[&INSERT_PAUSE_LOCK_KEY])
        .expect("controller should hold the test-only insert pause lock");

    let (pid_sender, pid_receiver) = mpsc::channel();
    let worker = thread::spawn(move || {
        let mut client = connect_in_test_schema();
        let worker_pid: i32 = client
            .query_one("SELECT pg_backend_pid()", &[])
            .expect("worker backend pid must be observable")
            .get(0);
        let mut transaction = client.transaction().unwrap();
        transaction
            .query_one(
                "SELECT set_config(\
                    'psychometrics_commons.test_result_insert_pause', 'on', true\
                 )",
                &[],
            )
            .expect("worker transaction should enable the test-only insert pause");
        pid_sender
            .send(worker_pid)
            .expect("controller should still be waiting for the worker pid");
        let disposition = persist_result_snapshot(&mut transaction, &snapshot()).unwrap();
        transaction.commit().unwrap();
        disposition
    });

    let worker_pid = pid_receiver
        .recv_timeout(Duration::from_secs(5))
        .expect("worker should report its PostgreSQL backend pid");
    wait_until_worker_is_blocked_on_advisory_lock(&mut controller, worker_pid);

    let winner = snapshot();
    let mut winner_transaction = controller.transaction().unwrap();
    assert_eq!(
        persist_result_snapshot(&mut winner_transaction, &winner).unwrap(),
        ResultSnapshotPersistenceDisposition::Inserted
    );
    winner_transaction.commit().unwrap();

    let unlocked: bool = controller
        .query_one("SELECT pg_advisory_unlock($1)", &[&INSERT_PAUSE_LOCK_KEY])
        .expect("controller should release the test-only insert pause lock")
        .get(0);
    assert!(unlocked);

    assert_eq!(
        worker
            .join()
            .expect("worker should finish after the lock release"),
        ResultSnapshotPersistenceDisposition::Duplicate
    );

    let result_rows: i64 = controller
        .query_one(
            "SELECT COUNT(*)::bigint FROM result_snapshot\
             WHERE result_snapshot_ref = 'result_snapshot_concurrent_replay'",
            &[],
        )
        .unwrap()
        .get(0);
    let observation_rows: i64 = controller
        .query_one(
            "SELECT COUNT(*)::bigint FROM result_snapshot_observation\
             WHERE result_snapshot_ref = 'result_snapshot_concurrent_replay'",
            &[],
        )
        .unwrap()
        .get(0);
    assert_eq!(result_rows, 1);
    assert_eq!(observation_rows, 1);
}
