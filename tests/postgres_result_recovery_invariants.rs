//! Real `PostgreSQL` recovery acceptance for immutable result evidence.
//!
//! This test rebuilds clean schemas from the repository migration chain, streams one released
//! result snapshot and its observations through `PostgreSQL` binary `COPY`, restores them, and
//! proves that scientific provenance, score disposition, uncertainty, and immutability guards
//! survive the round trip. It is recovery evidence, not a production backup-service claim.

use postgres::{error::SqlState, Client, NoTls};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

const SOURCE_SCHEMA: &str = "result_recovery_source_test";
const RESTORED_SCHEMA: &str = "result_recovery_restored_test";
const DATABASE_TEST_LOCK_KEY: i64 = 0x5253_4C54_5245_4356;
const ENGINE_DIGEST: &str =
    "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn connect_client() -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    Client::connect(&connection, NoTls).expect("isolated CI PostgreSQL database must be reachable")
}

fn migration_files() -> Vec<PathBuf> {
    let migration_directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations");
    let mut files: Vec<PathBuf> = fs::read_dir(migration_directory)
        .expect("repository migrations directory must be readable")
        .map(|entry| {
            entry
                .expect("migration directory entry must be readable")
                .path()
        })
        .filter(|path| path.extension().is_some_and(|extension| extension == "sql"))
        .collect();
    files.sort();
    assert!(
        !files.is_empty(),
        "result recovery acceptance requires physical migrations"
    );
    files
}

fn apply_migration_chain(client: &mut Client, schema: &str, files: &[PathBuf]) {
    client
        .batch_execute(&format!(
            "DROP SCHEMA IF EXISTS {schema} CASCADE; CREATE SCHEMA {schema}; SET search_path TO {schema};"
        ))
        .expect("clean result recovery schema should be created");

    for path in files {
        let sql = fs::read_to_string(path).unwrap_or_else(|error| {
            panic!(
                "migration {} must be readable as UTF-8: {error}",
                path.display()
            )
        });
        client
            .batch_execute(&sql)
            .unwrap_or_else(|error| panic!("migration {} must apply: {error}", path.display()));
    }
}

fn seed_result_snapshot(client: &mut Client) {
    client
        .batch_execute(&format!(
            "INSERT INTO {SOURCE_SCHEMA}.result_snapshot (
                result_snapshot_ref,
                participant_ref,
                scoring_result_ref,
                session_ref,
                response_snapshot_ref,
                assessment_spec_ref,
                instrument_version_ref,
                scoring_version_ref,
                calibration_reference,
                norm_version_ref,
                requested_output_schema_version,
                narrative_version_ref,
                consent_snapshot_refs,
                engine_artifact_digest,
                created_at_unix_ms,
                supersedes_ref
             ) VALUES (
                'result_snapshot_recovery_alpha',
                'participant_recovery_alpha',
                'scoring_result_recovery_alpha',
                'session_recovery_alpha',
                'response_snapshot_recovery_alpha',
                'assessment_spec_recovery_alpha',
                'instrument_version_recovery_alpha',
                'scoring_version_recovery_alpha',
                'calibration_recovery_alpha',
                'norm_version_recovery_alpha',
                1,
                'narrative_version_recovery_alpha',
                ARRAY['consent_snapshot_recovery_alpha'],
                '{ENGINE_DIGEST}',
                51000,
                NULL
             );
             INSERT INTO {SOURCE_SCHEMA}.result_snapshot_observation (
                result_snapshot_ref,
                observation_order,
                construct_ref,
                observation_disposition,
                score,
                standard_error
             ) VALUES
             (
                'result_snapshot_recovery_alpha', 0, 'construct_extraversion',
                'scored', 0.625, 0.125
             ),
             (
                'result_snapshot_recovery_alpha', 1, 'construct_openness',
                'abstained', NULL, NULL
             );"
        ))
        .expect("result recovery fixture should satisfy protected-main persistence constraints");
}

fn copy_table_out(client: &mut Client, table: &str) -> Vec<u8> {
    let mut reader = client
        .copy_out(&format!(
            "COPY {SOURCE_SCHEMA}.{table} TO STDOUT (FORMAT BINARY)"
        ))
        .unwrap_or_else(|error| panic!("{table} backup stream must open: {error}"));
    let mut bytes = Vec::new();
    reader
        .read_to_end(&mut bytes)
        .unwrap_or_else(|error| panic!("{table} backup stream must be readable: {error}"));
    assert!(
        !bytes.is_empty(),
        "{table} backup stream must contain PostgreSQL binary COPY data"
    );
    bytes
}

fn copy_table_in(client: &mut Client, table: &str, bytes: &[u8]) {
    let mut writer = client
        .copy_in(&format!(
            "COPY {RESTORED_SCHEMA}.{table} FROM STDIN (FORMAT BINARY)"
        ))
        .unwrap_or_else(|error| panic!("{table} restore stream must open: {error}"));
    writer
        .write_all(bytes)
        .unwrap_or_else(|error| panic!("{table} restore stream must accept data: {error}"));
    writer
        .finish()
        .unwrap_or_else(|error| panic!("{table} restore stream must commit: {error}"));
}

fn assert_restored_result(client: &mut Client) {
    let restored = client
        .query_one(
            &format!(
                "SELECT participant_ref, scoring_result_ref, session_ref, response_snapshot_ref,
                        assessment_spec_ref, instrument_version_ref, scoring_version_ref,
                        calibration_reference, norm_version_ref, requested_output_schema_version,
                        narrative_version_ref, consent_snapshot_refs, engine_artifact_digest,
                        created_at_unix_ms, supersedes_ref
                 FROM {RESTORED_SCHEMA}.result_snapshot
                 WHERE result_snapshot_ref = 'result_snapshot_recovery_alpha'"
            ),
            &[],
        )
        .expect("restored immutable result should remain queryable");

    assert_eq!(restored.get::<_, String>(0), "participant_recovery_alpha");
    assert_eq!(restored.get::<_, String>(1), "scoring_result_recovery_alpha");
    assert_eq!(restored.get::<_, String>(2), "session_recovery_alpha");
    assert_eq!(
        restored.get::<_, String>(3),
        "response_snapshot_recovery_alpha"
    );
    assert_eq!(restored.get::<_, String>(4), "assessment_spec_recovery_alpha");
    assert_eq!(
        restored.get::<_, String>(5),
        "instrument_version_recovery_alpha"
    );
    assert_eq!(restored.get::<_, String>(6), "scoring_version_recovery_alpha");
    assert_eq!(restored.get::<_, String>(7), "calibration_recovery_alpha");
    assert_eq!(
        restored.get::<_, Option<String>>(8).as_deref(),
        Some("norm_version_recovery_alpha")
    );
    assert_eq!(restored.get::<_, i32>(9), 1);
    assert_eq!(
        restored.get::<_, String>(10),
        "narrative_version_recovery_alpha"
    );
    assert_eq!(
        restored.get::<_, Vec<String>>(11),
        vec!["consent_snapshot_recovery_alpha".to_string()]
    );
    assert_eq!(restored.get::<_, String>(12), ENGINE_DIGEST);
    assert_eq!(restored.get::<_, i64>(13), 51000);
    assert_eq!(restored.get::<_, Option<String>>(14), None);

    let observations = client
        .query(
            &format!(
                "SELECT observation_order, construct_ref, observation_disposition, score, standard_error
                 FROM {RESTORED_SCHEMA}.result_snapshot_observation
                 WHERE result_snapshot_ref = 'result_snapshot_recovery_alpha'
                 ORDER BY observation_order"
            ),
            &[],
        )
        .expect("restored result observations should remain queryable");
    assert_eq!(observations.len(), 2);
    assert_eq!(observations[0].get::<_, i32>(0), 0);
    assert_eq!(observations[0].get::<_, String>(1), "construct_extraversion");
    assert_eq!(observations[0].get::<_, String>(2), "scored");
    assert_eq!(observations[0].get::<_, Option<f64>>(3), Some(0.625));
    assert_eq!(observations[0].get::<_, Option<f64>>(4), Some(0.125));
    assert_eq!(observations[1].get::<_, i32>(0), 1);
    assert_eq!(observations[1].get::<_, String>(1), "construct_openness");
    assert_eq!(observations[1].get::<_, String>(2), "abstained");
    assert_eq!(observations[1].get::<_, Option<f64>>(3), None);
    assert_eq!(observations[1].get::<_, Option<f64>>(4), None);

    let timestamps_match: bool = client
        .query_one(
            &format!(
                "SELECT source_row.created_at = restored_row.created_at
                 FROM {SOURCE_SCHEMA}.result_snapshot AS source_row
                 JOIN {RESTORED_SCHEMA}.result_snapshot AS restored_row
                   USING (result_snapshot_ref)
                 WHERE source_row.result_snapshot_ref = 'result_snapshot_recovery_alpha'"
            ),
            &[],
        )
        .expect("restored result creation evidence should remain comparable")
        .get(0);
    assert!(
        timestamps_match,
        "restore must preserve the exact database-authored result creation timestamp"
    );
}

fn assert_restored_result_remains_immutable(client: &mut Client) {
    let update_error = client
        .execute(
            &format!(
                "UPDATE {RESTORED_SCHEMA}.result_snapshot
                 SET narrative_version_ref = 'narrative_version_rebound'
                 WHERE result_snapshot_ref = 'result_snapshot_recovery_alpha'"
            ),
            &[],
        )
        .expect_err("restored result snapshot must remain immutable");
    assert_eq!(
        update_error
            .as_db_error()
            .expect("result mutation must fail at the database boundary")
            .code(),
        &SqlState::OBJECT_NOT_IN_PREREQUISITE_STATE
    );

    let observation_error = client
        .execute(
            &format!(
                "UPDATE {RESTORED_SCHEMA}.result_snapshot_observation
                 SET score = 0.75
                 WHERE result_snapshot_ref = 'result_snapshot_recovery_alpha'
                   AND construct_ref = 'construct_extraversion'"
            ),
            &[],
        )
        .expect_err("restored score observation must remain immutable");
    assert_eq!(
        observation_error
            .as_db_error()
            .expect("score mutation must fail at the database boundary")
            .code(),
        &SqlState::OBJECT_NOT_IN_PREREQUISITE_STATE
    );
}

#[test]
fn clean_restore_preserves_immutable_result_provenance_scores_and_uncertainty() {
    let mut client = connect_client();
    client
        .query_one("SELECT pg_advisory_lock($1)", &[&DATABASE_TEST_LOCK_KEY])
        .expect("result recovery advisory lock should be acquired");

    let files = migration_files();
    apply_migration_chain(&mut client, SOURCE_SCHEMA, &files);
    seed_result_snapshot(&mut client);
    let snapshot_backup = copy_table_out(&mut client, "result_snapshot");
    let observation_backup = copy_table_out(&mut client, "result_snapshot_observation");

    apply_migration_chain(&mut client, RESTORED_SCHEMA, &files);
    copy_table_in(&mut client, "result_snapshot", &snapshot_backup);
    copy_table_in(
        &mut client,
        "result_snapshot_observation",
        &observation_backup,
    );

    assert_restored_result(&mut client);
    assert_restored_result_remains_immutable(&mut client);

    client
        .batch_execute(&format!(
            "DROP SCHEMA IF EXISTS {SOURCE_SCHEMA} CASCADE;
             DROP SCHEMA IF EXISTS {RESTORED_SCHEMA} CASCADE;"
        ))
        .expect("result recovery schemas should be removed");
}
