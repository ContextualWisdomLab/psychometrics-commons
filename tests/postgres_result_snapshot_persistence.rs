//! Real `PostgreSQL` contract for durable immutable result snapshots.

use postgres::{Client, IsolationLevel, NoTls};
use psychometrics_commons_runtime::postgres_result_snapshot::{
    apply_result_snapshot_migration, persist_result_snapshot, ResultSnapshotPersistenceDisposition,
    ResultSnapshotPersistenceError,
};
use psychometrics_commons_runtime::response::{ResponseLedger, ResponseWrite};
use psychometrics_commons_runtime::result::{ResultSnapshot, ResultSnapshotInput};
use psychometrics_commons_runtime::scoring::{
    ObservationDisposition, ScoreObservation, ScoringRequest, ScoringRequestInput, ScoringResult,
};
use psychometrics_commons_runtime::session::SessionState;
use std::sync::{Mutex, MutexGuard};

const ENGINE_DIGEST: &str =
    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const OTHER_DIGEST: &str =
    "sha256:fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210";

static RESULT_SNAPSHOT_TEST_LOCK: Mutex<()> = Mutex::new(());

fn result_snapshot_test_guard() -> MutexGuard<'static, ()> {
    RESULT_SNAPSHOT_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn test_client() -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let mut client = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    client
        .batch_execute(
            "CREATE SCHEMA IF NOT EXISTS result_snapshot_persistence_test;\
             SET search_path TO result_snapshot_persistence_test;",
        )
        .unwrap();
    client
}

fn reset_result_snapshot_tables(client: &mut Client) {
    client
        .batch_execute(
            "DROP TABLE IF EXISTS result_snapshot_persistence_test.result_snapshot_observation;\
             DROP TABLE IF EXISTS result_snapshot_persistence_test.result_snapshot;",
        )
        .unwrap();
}

fn cleanup_result_snapshot_fault_injection(client: &mut Client) {
    client
        .batch_execute(
            "DROP TRIGGER IF EXISTS result_snapshot_redirect_after_insert \
                 ON result_snapshot_persistence_test.result_snapshot;\
             DROP FUNCTION IF EXISTS result_snapshot_persistence_test.result_snapshot_redirect_after_insert();\
             DROP SCHEMA IF EXISTS result_snapshot_select_failure_sink CASCADE;\
             DROP TRIGGER IF EXISTS result_snapshot_reject_observation \
                 ON result_snapshot_persistence_test.result_snapshot_observation;\
             DROP FUNCTION IF EXISTS result_snapshot_persistence_test.result_snapshot_reject_observation();",
        )
        .unwrap();
}

fn snapshot_named(
    session_ref: &str,
    result_snapshot_ref: &str,
    engine_digest: &str,
    norm_version_ref: Option<&str>,
    supersedes_ref: Option<&str>,
    observations: Vec<ScoreObservation>,
) -> ResultSnapshot {
    let mut ledger = ResponseLedger::new(session_ref).unwrap();
    ledger
        .record(
            SessionState::Active,
            ResponseWrite {
                server_event_ref: "server_event_result_one",
                client_event_ref: "client_event_result_one",
                item_version_ref: "item_version_001",
                payload_digest: ENGINE_DIGEST,
            },
        )
        .unwrap();
    let response = ledger
        .freeze_as(SessionState::Completed, "response_snapshot_result_one")
        .unwrap();
    let request = ScoringRequest::from_snapshot(
        &response,
        ScoringRequestInput {
            scoring_request_ref: "scoring_request_result_one",
            response_snapshot_ref: "response_snapshot_result_one",
            assessment_spec_ref: "assessment_spec_big_five_v1",
            instrument_version_ref: "instrument_version_big_five_ko_v1",
            scoring_version_ref: "scoring_version_big_five_v1",
            calibration_reference: "calibration_big_five_ko_v1",
            norm_version_ref,
            requested_output_schema_version: 1,
        },
    )
    .unwrap();
    let scoring_result = ScoringResult::new(
        "scoring_result_result_one",
        &request,
        engine_digest,
        observations,
    )
    .unwrap();
    ResultSnapshot::new(
        &request,
        &scoring_result,
        ResultSnapshotInput {
            result_snapshot_ref,
            participant_ref: "participant_result_one",
            narrative_version_ref: "narrative_version_big_five_v1",
            consent_snapshot_refs: &["consent_snapshot_service_v1"],
            created_at_unix_ms: 70_000,
            supersedes_ref,
        },
    )
    .unwrap()
}

fn default_snapshot(result_snapshot_ref: &str) -> ResultSnapshot {
    snapshot_named(
        "session_result_one",
        result_snapshot_ref,
        ENGINE_DIGEST,
        Some("norm_version_big_five_ko_v1"),
        None,
        vec![ScoreObservation::scored("construct_big_five", 0.25, Some(0.05)).unwrap()],
    )
}

fn persist_ok(
    client: &mut Client,
    snapshot: &ResultSnapshot,
) -> ResultSnapshotPersistenceDisposition {
    let mut transaction = client.transaction().unwrap();
    let disposition = persist_result_snapshot(&mut transaction, snapshot).unwrap();
    transaction.commit().unwrap();
    disposition
}

fn persist_err(client: &mut Client, snapshot: &ResultSnapshot) -> ResultSnapshotPersistenceError {
    let mut transaction = client.transaction().unwrap();
    let error = persist_result_snapshot(&mut transaction, snapshot).unwrap_err();
    transaction.rollback().unwrap();
    error
}

#[test]
fn result_snapshot_persist_is_exactly_idempotent_and_digest_rebinding_fails_closed() {
    let _guard = result_snapshot_test_guard();
    let mut client = test_client();
    reset_result_snapshot_tables(&mut client);
    apply_result_snapshot_migration(&mut client).unwrap();

    let snapshot = default_snapshot("result_snapshot_ko_v1");
    assert_eq!(
        persist_ok(&mut client, &snapshot),
        ResultSnapshotPersistenceDisposition::Inserted
    );
    assert_eq!(
        persist_ok(&mut client, &snapshot),
        ResultSnapshotPersistenceDisposition::Duplicate
    );

    let rebound = snapshot_named(
        "session_result_one",
        "result_snapshot_ko_v1",
        OTHER_DIGEST,
        Some("norm_version_big_five_ko_v1"),
        None,
        vec![ScoreObservation::scored("construct_big_five", 0.25, Some(0.05)).unwrap()],
    );
    assert!(matches!(
        persist_err(&mut client, &rebound),
        ResultSnapshotPersistenceError::ConflictingReplay
    ));
}

#[test]
fn observation_dispositions_and_supersession_persist_without_mutating_predecessor() {
    let _guard = result_snapshot_test_guard();
    let mut client = test_client();
    reset_result_snapshot_tables(&mut client);
    apply_result_snapshot_migration(&mut client).unwrap();

    let predecessor = default_snapshot("result_snapshot_predecessor");
    persist_ok(&mut client, &predecessor);

    let successor = snapshot_named(
        "session_result_two",
        "result_snapshot_successor",
        ENGINE_DIGEST,
        None,
        Some("result_snapshot_predecessor"),
        vec![
            ScoreObservation::scored("construct_extraversion", 0.5, None).unwrap(),
            ScoreObservation::without_score(
                "construct_openness",
                ObservationDisposition::Abstained,
            )
            .unwrap(),
            ScoreObservation::without_score(
                "construct_agreeableness",
                ObservationDisposition::Failed,
            )
            .unwrap(),
            ScoreObservation::without_score(
                "construct_conscientiousness",
                ObservationDisposition::Excluded,
            )
            .unwrap(),
        ],
    );
    assert_eq!(
        persist_ok(&mut client, &successor),
        ResultSnapshotPersistenceDisposition::Inserted
    );

    let predecessor_state: String = client
        .query_one(
            "SELECT engine_artifact_digest FROM result_snapshot WHERE result_snapshot_ref = $1",
            &[&"result_snapshot_predecessor"],
        )
        .unwrap()
        .get(0);
    assert_eq!(predecessor_state, ENGINE_DIGEST);

    let stored_norm: Option<String> = client
        .query_one(
            "SELECT norm_version_ref FROM result_snapshot WHERE result_snapshot_ref = $1",
            &[&"result_snapshot_successor"],
        )
        .unwrap()
        .get(0);
    assert!(stored_norm.is_none());

    let dispositions: Vec<String> = client
        .query(
            "SELECT observation_disposition FROM result_snapshot_observation \
             WHERE result_snapshot_ref = $1 ORDER BY observation_order",
            &[&"result_snapshot_successor"],
        )
        .unwrap()
        .into_iter()
        .map(|row| row.get(0))
        .collect();
    assert_eq!(
        dispositions,
        vec!["scored", "abstained", "failed", "excluded"]
    );
}

fn overflow_snapshot() -> ResultSnapshot {
    let mut ledger = ResponseLedger::new("session_result_overflow").unwrap();
    ledger
        .record(
            SessionState::Active,
            ResponseWrite {
                server_event_ref: "server_event_overflow",
                client_event_ref: "client_event_overflow",
                item_version_ref: "item_version_001",
                payload_digest: ENGINE_DIGEST,
            },
        )
        .unwrap();
    let response = ledger
        .freeze_as(SessionState::Completed, "response_snapshot_overflow")
        .unwrap();
    let request = ScoringRequest::from_snapshot(
        &response,
        ScoringRequestInput {
            scoring_request_ref: "scoring_request_overflow",
            response_snapshot_ref: "response_snapshot_overflow",
            assessment_spec_ref: "assessment_spec_big_five_v1",
            instrument_version_ref: "instrument_version_big_five_ko_v1",
            scoring_version_ref: "scoring_version_big_five_v1",
            calibration_reference: "calibration_big_five_ko_v1",
            norm_version_ref: None,
            requested_output_schema_version: 1,
        },
    )
    .unwrap();
    let scoring_result = ScoringResult::new(
        "scoring_result_overflow",
        &request,
        ENGINE_DIGEST,
        vec![ScoreObservation::scored("construct_big_five", 0.25, None).unwrap()],
    )
    .unwrap();
    ResultSnapshot::new(
        &request,
        &scoring_result,
        ResultSnapshotInput {
            result_snapshot_ref: "result_snapshot_overflow",
            participant_ref: "participant_result_one",
            narrative_version_ref: "narrative_version_big_five_v1",
            consent_snapshot_refs: &["consent_snapshot_service_v1"],
            created_at_unix_ms: u64::MAX,
            supersedes_ref: None,
        },
    )
    .unwrap()
}

#[test]
fn observation_rebinding_and_overflow_timestamp_fail_closed() {
    let _guard = result_snapshot_test_guard();
    let mut client = test_client();
    reset_result_snapshot_tables(&mut client);
    apply_result_snapshot_migration(&mut client).unwrap();

    persist_ok(
        &mut client,
        &default_snapshot("result_snapshot_observations"),
    );
    let rebound = snapshot_named(
        "session_result_one",
        "result_snapshot_observations",
        ENGINE_DIGEST,
        Some("norm_version_big_five_ko_v1"),
        None,
        vec![ScoreObservation::scored("construct_neuroticism", 0.1, None).unwrap()],
    );
    assert!(matches!(
        persist_err(&mut client, &rebound),
        ResultSnapshotPersistenceError::ConflictingReplay
    ));
    assert!(matches!(
        persist_err(&mut client, &overflow_snapshot()),
        ResultSnapshotPersistenceError::InvalidTimestamp
    ));
}

#[test]
fn result_snapshot_persistence_requires_read_committed() {
    let _guard = result_snapshot_test_guard();
    let mut client = test_client();
    reset_result_snapshot_tables(&mut client);
    apply_result_snapshot_migration(&mut client).unwrap();

    let snapshot = default_snapshot("result_snapshot_serializable");
    let mut transaction = client
        .build_transaction()
        .isolation_level(IsolationLevel::Serializable)
        .start()
        .unwrap();
    assert!(matches!(
        persist_result_snapshot(&mut transaction, &snapshot),
        Err(ResultSnapshotPersistenceError::UnsupportedIsolationLevel)
    ));
    transaction.rollback().unwrap();
}

#[test]
fn missing_result_snapshot_relation_is_a_database_failure() {
    let _guard = result_snapshot_test_guard();
    let mut client = test_client();
    reset_result_snapshot_tables(&mut client);

    let snapshot = default_snapshot("result_snapshot_missing");
    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        persist_result_snapshot(&mut transaction, &snapshot),
        Err(ResultSnapshotPersistenceError::Database(_))
    ));
    transaction.rollback().unwrap();
}

#[test]
fn replay_select_failure_is_a_database_failure() {
    let _guard = result_snapshot_test_guard();
    let mut client = test_client();
    reset_result_snapshot_tables(&mut client);
    apply_result_snapshot_migration(&mut client).unwrap();

    let snapshot = default_snapshot("result_snapshot_hidden_select");
    persist_ok(&mut client, &snapshot);

    client
        .batch_execute(
            "DROP SCHEMA IF EXISTS result_snapshot_select_failure_sink CASCADE;\
             CREATE SCHEMA result_snapshot_select_failure_sink;\
             CREATE OR REPLACE FUNCTION result_snapshot_redirect_after_insert() \
             RETURNS trigger LANGUAGE plpgsql AS $$ \
             BEGIN \
                 PERFORM set_config('search_path', 'result_snapshot_select_failure_sink', false); \
                 RETURN NULL; \
             END $$; \
             CREATE TRIGGER result_snapshot_redirect_after_insert \
             AFTER INSERT ON result_snapshot \
             FOR EACH STATEMENT EXECUTE FUNCTION result_snapshot_redirect_after_insert();",
        )
        .unwrap();

    assert!(matches!(
        persist_err(&mut client, &snapshot),
        ResultSnapshotPersistenceError::Database(_)
    ));
    cleanup_result_snapshot_fault_injection(&mut client);
}

#[test]
fn observation_replay_select_failure_is_a_database_failure() {
    let _guard = result_snapshot_test_guard();
    let mut client = test_client();
    reset_result_snapshot_tables(&mut client);
    apply_result_snapshot_migration(&mut client).unwrap();

    let snapshot = default_snapshot("result_snapshot_hidden_observation_select");
    persist_ok(&mut client, &snapshot);
    client
        .batch_execute("DROP TABLE result_snapshot_persistence_test.result_snapshot_observation;")
        .unwrap();

    assert!(matches!(
        persist_err(&mut client, &snapshot),
        ResultSnapshotPersistenceError::Database(_)
    ));
}

#[test]
fn observation_insert_failure_is_a_database_failure() {
    let _guard = result_snapshot_test_guard();
    let mut client = test_client();
    reset_result_snapshot_tables(&mut client);
    apply_result_snapshot_migration(&mut client).unwrap();

    client
        .batch_execute(
            "CREATE OR REPLACE FUNCTION result_snapshot_reject_observation() \
             RETURNS trigger LANGUAGE plpgsql AS $$ \
             BEGIN \
                 RAISE EXCEPTION 'result_snapshot observation sink'; \
             END $$; \
             CREATE TRIGGER result_snapshot_reject_observation \
             BEFORE INSERT ON result_snapshot_observation \
             FOR EACH STATEMENT EXECUTE FUNCTION result_snapshot_reject_observation();",
        )
        .unwrap();

    let snapshot = default_snapshot("result_snapshot_hidden_observation");
    assert!(matches!(
        persist_err(&mut client, &snapshot),
        ResultSnapshotPersistenceError::Database(_)
    ));
    cleanup_result_snapshot_fault_injection(&mut client);
}
