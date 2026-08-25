//! Real `PostgreSQL` recovery acceptance for durable scoring-worker evidence.
//!
//! This is deliberately narrower than a production backup-service claim. It rebuilds two clean
//! schemas from the repository migration chain, streams one in-flight scoring job through
//! `PostgreSQL` binary `COPY`, restores it, and proves that request identity, lease ownership,
//! fencing, attempt budget, and database constraints survive the round trip.

use postgres::{error::SqlState, Client, NoTls};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

const SOURCE_SCHEMA: &str = "scoring_recovery_source_test";
const RESTORED_SCHEMA: &str = "scoring_recovery_restored_test";
const DATABASE_TEST_LOCK_KEY: i64 = 0x5343_4F52_4552_4543;

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
        "scoring recovery acceptance requires physical migrations"
    );
    files
}

fn apply_migration_chain(client: &mut Client, schema: &str, files: &[PathBuf]) {
    client
        .batch_execute(&format!(
            "DROP SCHEMA IF EXISTS {schema} CASCADE; CREATE SCHEMA {schema}; SET search_path TO {schema};"
        ))
        .expect("clean scoring recovery schema should be created");

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

fn seed_in_flight_scoring_job(client: &mut Client) {
    client
        .batch_execute(&format!(
            "INSERT INTO {SOURCE_SCHEMA}.scoring_job_state (
                scoring_job_ref,
                scoring_request_ref,
                scoring_state,
                attempt_count,
                max_attempts,
                next_attempt_at_unix_ms,
                last_failure_code,
                active_worker_ref,
                active_lease_ref,
                active_fencing_token,
                active_lease_expires_at_unix_ms,
                result_ref,
                completed_fencing_token
             ) VALUES (
                'scoring_job_recovery_alpha',
                'scoring_request_recovery_alpha',
                'leased',
                2,
                5,
                NULL,
                'provider_timeout',
                'worker_recovery_alpha',
                'lease_recovery_alpha',
                2,
                41000,
                NULL,
                NULL
             );"
        ))
        .expect("in-flight scoring fixture should satisfy protected-main persistence constraints");
}

fn copy_scoring_job_out(client: &mut Client) -> Vec<u8> {
    let mut reader = client
        .copy_out(&format!(
            "COPY {SOURCE_SCHEMA}.scoring_job_state TO STDOUT (FORMAT BINARY)"
        ))
        .expect("scoring-job backup stream must open");
    let mut bytes = Vec::new();
    reader
        .read_to_end(&mut bytes)
        .expect("scoring-job backup stream must be readable");
    assert!(
        !bytes.is_empty(),
        "scoring-job backup stream must contain PostgreSQL binary COPY data"
    );
    bytes
}

fn copy_scoring_job_in(client: &mut Client, bytes: &[u8]) {
    let mut writer = client
        .copy_in(&format!(
            "COPY {RESTORED_SCHEMA}.scoring_job_state FROM STDIN (FORMAT BINARY)"
        ))
        .expect("scoring-job restore stream must open");
    writer
        .write_all(bytes)
        .expect("scoring-job restore stream must accept data");
    writer
        .finish()
        .expect("scoring-job restore stream must commit");
}

fn assert_restored_scoring_evidence(client: &mut Client) {
    let restored = client
        .query_one(
            &format!(
                "SELECT scoring_request_ref, scoring_state, attempt_count, max_attempts,
                        last_failure_code, active_worker_ref, active_lease_ref,
                        active_fencing_token, active_lease_expires_at_unix_ms,
                        result_ref, completed_fencing_token
                 FROM {RESTORED_SCHEMA}.scoring_job_state
                 WHERE scoring_job_ref = 'scoring_job_recovery_alpha'"
            ),
            &[],
        )
        .expect("restored in-flight scoring job should remain queryable");

    assert_eq!(
        restored.get::<_, String>(0),
        "scoring_request_recovery_alpha"
    );
    assert_eq!(restored.get::<_, String>(1), "leased");
    assert_eq!(restored.get::<_, i32>(2), 2);
    assert_eq!(restored.get::<_, i32>(3), 5);
    assert_eq!(
        restored.get::<_, Option<String>>(4).as_deref(),
        Some("provider_timeout")
    );
    assert_eq!(
        restored.get::<_, Option<String>>(5).as_deref(),
        Some("worker_recovery_alpha")
    );
    assert_eq!(
        restored.get::<_, Option<String>>(6).as_deref(),
        Some("lease_recovery_alpha")
    );
    assert_eq!(restored.get::<_, Option<i64>>(7), Some(2));
    assert_eq!(restored.get::<_, Option<i64>>(8), Some(41000));
    assert_eq!(restored.get::<_, Option<String>>(9), None);
    assert_eq!(restored.get::<_, Option<i64>>(10), None);

    let timestamps_match: bool = client
        .query_one(
            &format!(
                "SELECT source_row.created_at = restored_row.created_at
                        AND source_row.updated_at = restored_row.updated_at
                 FROM {SOURCE_SCHEMA}.scoring_job_state AS source_row
                 JOIN {RESTORED_SCHEMA}.scoring_job_state AS restored_row
                   USING (scoring_job_ref)
                 WHERE source_row.scoring_job_ref = 'scoring_job_recovery_alpha'"
            ),
            &[],
        )
        .expect("restored scoring timestamps should remain comparable")
        .get(0);
    assert!(
        timestamps_match,
        "restore must preserve database-authored scoring lifecycle timestamps exactly"
    );
}

fn assert_restored_scoring_constraints(client: &mut Client) {
    let duplicate = client
        .execute(
            &format!(
                "INSERT INTO {RESTORED_SCHEMA}.scoring_job_state (
                    scoring_job_ref, scoring_request_ref, scoring_state, attempt_count, max_attempts
                 ) VALUES (
                    'scoring_job_recovery_alpha', 'scoring_request_conflict_alpha',
                    'queued', 0, 5
                 )"
            ),
            &[],
        )
        .expect_err("restore must preserve scoring-job identity uniqueness");
    let database_error = duplicate
        .as_db_error()
        .expect("duplicate scoring job must fail at the database constraint boundary");
    assert_eq!(database_error.code(), &SqlState::UNIQUE_VIOLATION);
    assert_eq!(database_error.constraint(), Some("scoring_job_state_pkey"));

    let impossible_lease = client
        .execute(
            &format!(
                "INSERT INTO {RESTORED_SCHEMA}.scoring_job_state (
                    scoring_job_ref, scoring_request_ref, scoring_state, attempt_count, max_attempts,
                    active_worker_ref, active_lease_ref, active_fencing_token,
                    active_lease_expires_at_unix_ms
                 ) VALUES (
                    'scoring_job_recovery_bad', 'scoring_request_recovery_bad',
                    'leased', 2, 5, 'worker_recovery_bad', 'lease_recovery_bad', 1, 42000
                 )"
            ),
            &[],
        )
        .expect_err("restored schema must reject a fencing token that does not match the attempt");
    let database_error = impossible_lease
        .as_db_error()
        .expect("impossible scoring lease must fail at the database constraint boundary");
    assert_eq!(database_error.code(), &SqlState::CHECK_VIOLATION);
    assert_eq!(
        database_error.constraint(),
        Some("scoring_fencing_attempt_match_check")
    );
}

#[test]
fn clean_restore_preserves_in_flight_scoring_identity_fencing_and_constraints() {
    let mut client = connect_client();
    client
        .query_one("SELECT pg_advisory_lock($1)", &[&DATABASE_TEST_LOCK_KEY])
        .expect("scoring recovery advisory lock should be acquired");

    let files = migration_files();
    apply_migration_chain(&mut client, SOURCE_SCHEMA, &files);
    seed_in_flight_scoring_job(&mut client);
    let backup = copy_scoring_job_out(&mut client);

    apply_migration_chain(&mut client, RESTORED_SCHEMA, &files);
    copy_scoring_job_in(&mut client, &backup);

    assert_restored_scoring_evidence(&mut client);
    assert_restored_scoring_constraints(&mut client);

    client
        .batch_execute(&format!(
            "DROP SCHEMA IF EXISTS {SOURCE_SCHEMA} CASCADE;
             DROP SCHEMA IF EXISTS {RESTORED_SCHEMA} CASCADE;"
        ))
        .expect("scoring recovery schemas should be removed");
}
