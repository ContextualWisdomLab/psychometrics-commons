//! Real PostgreSQL restart-reload contract for immutable product results.
//!
//! The loader returns copied durable result evidence only. Missing rows stay
//! absent; ambiguous or cyclic current-result graphs fail closed.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::postgres_result_snapshot::{
    apply_result_snapshot_migration, load_current_result_snapshot_for_session,
    load_result_snapshot, ResultSnapshotPersistenceError,
};
use std::sync::{Mutex, MutexGuard};

static RESULT_RELOAD_LOCK: Mutex<()> = Mutex::new(());

fn guard() -> MutexGuard<'static, ()> {
    RESULT_RELOAD_LOCK
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
            "DROP SCHEMA IF EXISTS result_reload_reconciliation_test CASCADE;\
             CREATE SCHEMA result_reload_reconciliation_test;\
             SET search_path TO result_reload_reconciliation_test;",
        )
        .unwrap();
    apply_result_snapshot_migration(&mut client).unwrap();
    client
}

fn insert_snapshot(
    client: &mut Client,
    snapshot_ref: &str,
    session_ref: &str,
    supersedes_ref: Option<&str>,
    score: f64,
) {
    client
        .execute(
            "INSERT INTO result_snapshot (\
                 result_snapshot_ref, participant_ref, scoring_result_ref, session_ref, \
                 response_snapshot_ref, assessment_spec_ref, instrument_version_ref, \
                 scoring_version_ref, calibration_reference, norm_version_ref, \
                 requested_output_schema_version, narrative_version_ref, consent_snapshot_refs, \
                 engine_artifact_digest, created_at_unix_ms, supersedes_ref\
             ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, 1, $11, $12, $13, 70000, $14)",
            &[
                &snapshot_ref,
                &"participant_result_reload",
                &format!("scoring_result_{snapshot_ref}"),
                &session_ref,
                &format!("response_snapshot_{snapshot_ref}"),
                &"assessment_spec_big_five_v1",
                &"instrument_version_big_five_ko_v1",
                &"scoring_version_big_five_v1",
                &"calibration_big_five_ko_v1",
                &Some("norm_version_big_five_ko_v1"),
                &"narrative_version_big_five_v1",
                &vec!["consent_snapshot_service_v1".to_owned()],
                &"sha256:1111111111111111111111111111111111111111111111111111111111111111",
                &supersedes_ref,
            ],
        )
        .unwrap();
    client
        .execute(
            "INSERT INTO result_snapshot_observation (\
                 result_snapshot_ref, observation_order, construct_ref, \
                 observation_disposition, score, standard_error\
             ) VALUES ($1, 0, 'construct_extraversion', 'scored', $2, 0.08)",
            &[&snapshot_ref, &score],
        )
        .unwrap();
}

#[test]
fn snapshot_and_unique_session_tip_reload_without_rescoring() {
    let _guard = guard();
    let mut client = test_client();
    insert_snapshot(
        &mut client,
        "result_snapshot_reload_old",
        "session_result_reload",
        None,
        0.21,
    );
    insert_snapshot(
        &mut client,
        "result_snapshot_reload_current",
        "session_result_reload",
        Some("result_snapshot_reload_old"),
        0.42,
    );

    let mut transaction = client.transaction().unwrap();
    let exact = load_result_snapshot(&mut transaction, "result_snapshot_reload_current")
        .unwrap()
        .expect("stored immutable result must reload");
    let current = load_current_result_snapshot_for_session(
        &mut transaction,
        "session_result_reload",
    )
    .unwrap()
    .expect("one non-superseded session tip must reload");
    transaction.commit().unwrap();

    assert_eq!(exact, current);
    assert_eq!(current.result_snapshot_ref(), "result_snapshot_reload_current");
    assert_eq!(current.score_observations()[0].score(), Some(0.42));
}

#[test]
fn missing_and_noncanonical_aliases_fail_closed() {
    let _guard = guard();
    let mut client = test_client();
    let mut transaction = client.transaction().unwrap();

    assert!(load_result_snapshot(&mut transaction, "result_snapshot_absent")
        .unwrap()
        .is_none());
    assert!(load_current_result_snapshot_for_session(&mut transaction, "session_result_absent")
        .unwrap()
        .is_none());
    assert!(matches!(
        load_result_snapshot(&mut transaction, " result_snapshot_absent"),
        Err(ResultSnapshotPersistenceError::InvalidReference)
    ));
    assert!(matches!(
        load_current_result_snapshot_for_session(&mut transaction, " session_result_absent"),
        Err(ResultSnapshotPersistenceError::InvalidReference)
    ));
    transaction.rollback().unwrap();
}

#[test]
fn ambiguous_and_cyclic_session_tips_fail_closed() {
    let _guard = guard();
    let mut client = test_client();

    insert_snapshot(
        &mut client,
        "result_snapshot_tip_alpha",
        "session_result_two_tips",
        None,
        0.21,
    );
    insert_snapshot(
        &mut client,
        "result_snapshot_tip_beta",
        "session_result_two_tips",
        None,
        0.31,
    );
    let mut ambiguous = client.transaction().unwrap();
    assert!(matches!(
        load_current_result_snapshot_for_session(&mut ambiguous, "session_result_two_tips"),
        Err(ResultSnapshotPersistenceError::InconsistentEvidence)
    ));
    ambiguous.rollback().unwrap();

    insert_snapshot(
        &mut client,
        "result_snapshot_cycle_alpha",
        "session_result_cycle",
        Some("result_snapshot_cycle_beta"),
        0.25,
    );
    insert_snapshot(
        &mut client,
        "result_snapshot_cycle_beta",
        "session_result_cycle",
        Some("result_snapshot_cycle_alpha"),
        0.35,
    );
    let mut cycle = client.transaction().unwrap();
    assert!(matches!(
        load_current_result_snapshot_for_session(&mut cycle, "session_result_cycle"),
        Err(ResultSnapshotPersistenceError::InconsistentEvidence)
    ));
    cycle.rollback().unwrap();
}

#[test]
fn gapped_observation_order_fails_closed() {
    let _guard = guard();
    let mut client = test_client();
    insert_snapshot(
        &mut client,
        "result_snapshot_gap",
        "session_result_gap",
        None,
        0.21,
    );
    client
        .batch_execute(
            "ALTER TABLE result_snapshot_observation DISABLE TRIGGER USER;\
             UPDATE result_snapshot_observation \
             SET observation_order = 2 \
             WHERE result_snapshot_ref = 'result_snapshot_gap';\
             ALTER TABLE result_snapshot_observation ENABLE TRIGGER USER;",
        )
        .unwrap();

    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        load_result_snapshot(&mut transaction, "result_snapshot_gap"),
        Err(ResultSnapshotPersistenceError::InconsistentEvidence)
    ));
    transaction.rollback().unwrap();
}
