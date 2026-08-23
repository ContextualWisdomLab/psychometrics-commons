//! Database-level immutability coverage for persisted result snapshots and observations.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::postgres_result_snapshot::apply_result_snapshot_migration;
use std::time::{SystemTime, UNIX_EPOCH};

fn legacy_schema_name(prefix: &str, process_id: u32, nonce: u128) -> String {
    format!("{prefix}_{process_id}_{nonce}")
}

fn isolated_client() -> (Client, String) {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let mut client = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock must be after the Unix epoch")
        .as_nanos();
    let schema_name = legacy_schema_name("result_snapshot_immutable", std::process::id(), nonce);
    client
        .batch_execute(&format!(
            "CREATE SCHEMA {schema_name}; SET search_path TO {schema_name};"
        ))
        .unwrap();
    apply_result_snapshot_migration(&mut client).unwrap();
    client
        .batch_execute(
            "INSERT INTO result_snapshot (\
                 result_snapshot_ref, participant_ref, scoring_result_ref, session_ref,\
                 response_snapshot_ref, assessment_spec_ref, instrument_version_ref,\
                 scoring_version_ref, calibration_reference, norm_version_ref,\
                 requested_output_schema_version, narrative_version_ref, consent_snapshot_refs,\
                 engine_artifact_digest, created_at_unix_ms, supersedes_ref\
             ) VALUES (\
                 'result_snapshot_immutable', 'participant_immutable', 'scoring_result_immutable',\
                 'session_immutable', 'response_snapshot_immutable', 'assessment_spec_immutable',\
                 'instrument_version_immutable', 'scoring_version_immutable',\
                 'calibration_reference_immutable', NULL, 1, 'narrative_version_immutable',\
                 ARRAY['consent_snapshot_immutable'],\
                 'sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',\
                 10000, NULL\
             );\
             INSERT INTO result_snapshot_observation (\
                 result_snapshot_ref, observation_order, construct_ref,\
                 observation_disposition, score, standard_error\
             ) VALUES (\
                 'result_snapshot_immutable', 0, 'construct_immutable', 'scored', 0.5, 0.1\
             );",
        )
        .unwrap();
    (client, schema_name)
}

fn assert_immutable_error(error: &postgres::Error) {
    let database_error = error
        .as_db_error()
        .expect("immutable result evidence must fail at the database boundary");
    assert_eq!(database_error.code().code(), "55000");
}

fn expect_rejected_statement(client: &mut Client, statement: &str) {
    let mut transaction = client.transaction().unwrap();
    let error = transaction
        .batch_execute(statement)
        .expect_err("immutable result evidence mutation must be rejected");
    assert_immutable_error(&error);
    transaction.rollback().unwrap();
}

#[test]
fn schema_name_must_not_repeat_after_process_restart() {
    let before_restart = legacy_schema_name("result_snapshot_immutable", 4242, 1_000_000);
    let after_restart = legacy_schema_name("result_snapshot_immutable", 4242, 1_000_000);

    assert_ne!(
        before_restart, after_restart,
        "test schema identity must survive PID reuse and restarted process-local state"
    );
}

#[test]
fn result_evidence_rejects_update_delete_and_truncate() {
    let (mut client, schema_name) = isolated_client();

    let mutations = [
        "UPDATE result_snapshot SET narrative_version_ref = 'narrative_version_tampered' \
         WHERE result_snapshot_ref = 'result_snapshot_immutable'",
        "UPDATE result_snapshot_observation SET score = 0.9 \
         WHERE result_snapshot_ref = 'result_snapshot_immutable'",
        "DELETE FROM result_snapshot_observation \
         WHERE result_snapshot_ref = 'result_snapshot_immutable'",
        "DELETE FROM result_snapshot \
         WHERE result_snapshot_ref = 'result_snapshot_immutable'",
        "TRUNCATE TABLE result_snapshot_observation",
        "TRUNCATE TABLE result_snapshot CASCADE",
    ];
    for statement in mutations {
        expect_rejected_statement(&mut client, statement);
    }

    let counts = client
        .query_one(
            "SELECT\
                 (SELECT count(*) FROM result_snapshot),\
                 (SELECT count(*) FROM result_snapshot_observation)",
            &[],
        )
        .unwrap();
    assert_eq!(counts.get::<_, i64>(0), 1);
    assert_eq!(counts.get::<_, i64>(1), 1);

    client
        .batch_execute(&format!("DROP SCHEMA {schema_name} CASCADE;"))
        .unwrap();
}