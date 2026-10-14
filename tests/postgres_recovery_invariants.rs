//! Real `PostgreSQL` recovery acceptance for the currently persisted product state.
//!
//! This is deliberately narrower than a production backup-service claim. It rebuilds a clean
//! schema from the repository migration chain, streams recovery-critical rows through `PostgreSQL`
//! `COPY ... FORMAT BINARY`, restores them into that clean schema, and then proves that immutable
//! provenance, tenant-scoped deduplication, and in-flight fencing evidence still behave correctly.

use postgres::{error::SqlState, Client, NoTls};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const SOURCE_SCHEMA: &str = "recovery_backup_source_test";
const RESTORED_SCHEMA: &str = "recovery_backup_restored_test";
const DATABASE_TEST_LOCK_KEY: i64 = 0x5245_434F_5645_5259;
const DIGEST_A: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const DIGEST_B: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

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
        "recovery acceptance requires at least one physical migration"
    );
    files
}

fn apply_migration_chain(client: &mut Client, schema: &str, files: &[PathBuf]) {
    client
        .batch_execute(&format!(
            "DROP SCHEMA IF EXISTS {schema} CASCADE; CREATE SCHEMA {schema}; SET search_path TO {schema};"
        ))
        .expect("clean recovery schema should be created");

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

fn seed_recovery_critical_state(client: &mut Client) {
    client
        .batch_execute(&format!(
            "INSERT INTO {SOURCE_SCHEMA}.integration_outbox (
                event_ref, event_type, schema_version, source_ref, tenant_ref, subject_ref,
                occurred_at_unix_ms, correlation_ref, causation_ref, payload_digest, max_attempts,
                current_state, latest_event_at_unix_ms
             ) VALUES (
                'event_recovery_alpha', 'assessment.result.available', 'v1',
                'psychometrics_commons', 'tenant_recovery_alpha', 'result_recovery_alpha',
                10000, 'correlation_recovery_alpha', NULL, '{DIGEST_A}', 5, 'pending', 10000
             );
             INSERT INTO {SOURCE_SCHEMA}.integration_inbox (
                consumer_ref, source_ref, tenant_ref, source_event_ref, event_type,
                schema_version, subject_ref, payload_digest, received_at_unix_ms
             ) VALUES (
                'consumer_recovery_alpha', 'dependency_recovery_alpha', 'tenant_recovery_alpha',
                'event_dependency_alpha', 'dependency.effect.requested', 'v1',
                'participant_recovery_alpha', '{DIGEST_B}', 11000
             );
             INSERT INTO {SOURCE_SCHEMA}.integration_consumption (
                consumer_ref, source_ref, tenant_ref, source_event_ref, consumption_ref,
                side_effect_ref, consumption_state, fencing_token, latest_event_at_unix_ms,
                claim_expires_at_unix_ms, claim_deadline_at, completion_evidence_ref, cause_code
             ) VALUES (
                'consumer_recovery_alpha', 'dependency_recovery_alpha', 'tenant_recovery_alpha',
                'event_dependency_alpha', 'consumption_recovery_alpha', 'effect_recovery_alpha',
                'processing', 7, 12000, 13000, clock_timestamp() + INTERVAL '1 hour', NULL, NULL
             );
             INSERT INTO {SOURCE_SCHEMA}.response_snapshot (
                snapshot_ref, session_ref, event_count, last_sequence
             ) VALUES ('snapshot_recovery_alpha', 'session_recovery_alpha', 1, 1);
             INSERT INTO {SOURCE_SCHEMA}.response_snapshot_entry (
                snapshot_ref, snapshot_sequence, event_ref, item_version_ref, payload_digest
             ) VALUES (
                'snapshot_recovery_alpha', 1, 'response_recovery_alpha',
                'item_version_recovery_alpha', '{DIGEST_A}'
             );
             INSERT INTO {SOURCE_SCHEMA}.response_event (
                response_event_ref, session_ref, client_event_ref, item_version_ref,
                payload_digest, server_sequence, observed_at, received_at
             ) VALUES (
                'response_event_recovery_alpha', 'session_recovery_alpha',
                'client_event_recovery_alpha', 'item_version_recovery_alpha',
                '{DIGEST_A}', 1,
                TIMESTAMPTZ '2023-11-14 22:13:20+00',
                TIMESTAMPTZ '2023-11-14 22:13:20.250+00'
             );"
        ))
        .expect("recovery fixture should satisfy all protected-main persistence constraints");
}

fn copy_table_out(client: &mut Client, schema: &str, table: &str) -> Vec<u8> {
    let mut reader = client
        .copy_out(&format!("COPY {schema}.{table} TO STDOUT (FORMAT BINARY)"))
        .unwrap_or_else(|error| panic!("{schema}.{table} backup stream must open: {error}"));
    let mut bytes = Vec::new();
    reader
        .read_to_end(&mut bytes)
        .unwrap_or_else(|error| panic!("{schema}.{table} backup stream must be readable: {error}"));
    assert!(
        !bytes.is_empty(),
        "{schema}.{table} backup stream must contain PostgreSQL binary COPY data"
    );
    bytes
}

fn copy_table_in(client: &mut Client, schema: &str, table: &str, bytes: &[u8]) {
    let mut writer = client
        .copy_in(&format!("COPY {schema}.{table} FROM STDIN (FORMAT BINARY)"))
        .unwrap_or_else(|error| panic!("{schema}.{table} restore stream must open: {error}"));
    writer.write_all(bytes).unwrap_or_else(|error| {
        panic!("{schema}.{table} restore stream must accept data: {error}")
    });
    writer
        .finish()
        .unwrap_or_else(|error| panic!("{schema}.{table} restore stream must commit: {error}"));
}

#[allow(clippy::too_many_lines)]
fn assert_restored_evidence(client: &mut Client) {
    let restored_outbox = client
        .query_one(
            &format!(
                "SELECT tenant_ref, payload_digest, current_state
                 FROM {RESTORED_SCHEMA}.integration_outbox
                 WHERE source_ref = 'psychometrics_commons'
                   AND tenant_ref = 'tenant_recovery_alpha'
                   AND event_ref = 'event_recovery_alpha'"
            ),
            &[],
        )
        .expect("restored outbox evidence should remain queryable");
    assert_eq!(restored_outbox.get::<_, String>(0), "tenant_recovery_alpha");
    assert_eq!(restored_outbox.get::<_, String>(1), DIGEST_A);
    assert_eq!(restored_outbox.get::<_, String>(2), "pending");

    let restored_consumption = client
        .query_one(
            &format!(
                "SELECT consumption_state, fencing_token, claim_expires_at_unix_ms,
                        claim_deadline_at IS NOT NULL
                 FROM {RESTORED_SCHEMA}.integration_consumption
                 WHERE consumption_ref = 'consumption_recovery_alpha'"
            ),
            &[],
        )
        .expect("in-flight inbox consumption evidence should survive restore");
    assert_eq!(restored_consumption.get::<_, String>(0), "processing");
    assert_eq!(restored_consumption.get::<_, i64>(1), 7);
    assert_eq!(restored_consumption.get::<_, Option<i64>>(2), Some(13000));
    assert!(restored_consumption.get::<_, bool>(3));

    let claim_deadline_matches_source: bool = client
        .query_one(
            &format!(
                "SELECT source_row.claim_deadline_at = restored_row.claim_deadline_at
                 FROM {SOURCE_SCHEMA}.integration_consumption AS source_row
                 JOIN {RESTORED_SCHEMA}.integration_consumption AS restored_row
                   ON restored_row.consumer_ref = source_row.consumer_ref
                  AND restored_row.source_ref = source_row.source_ref
                  AND restored_row.tenant_ref = source_row.tenant_ref
                  AND restored_row.source_event_ref = source_row.source_event_ref
                  AND restored_row.consumption_ref = source_row.consumption_ref
                 WHERE source_row.consumption_ref = 'consumption_recovery_alpha'"
            ),
            &[],
        )
        .expect("restored claim-deadline evidence should remain comparable")
        .get(0);
    assert!(
        claim_deadline_matches_source,
        "restore must preserve the exact database-authoritative processing claim deadline"
    );

    let restored_snapshot = client
        .query_one(
            &format!(
                "SELECT response_snapshot.session_ref, response_snapshot.event_count,
                        response_snapshot_entry.event_ref, response_snapshot_entry.payload_digest
                 FROM {RESTORED_SCHEMA}.response_snapshot
                 JOIN {RESTORED_SCHEMA}.response_snapshot_entry USING (snapshot_ref)
                 WHERE response_snapshot.snapshot_ref = 'snapshot_recovery_alpha'"
            ),
            &[],
        )
        .expect("immutable response provenance should survive restore");
    assert_eq!(
        restored_snapshot.get::<_, String>(0),
        "session_recovery_alpha"
    );
    assert_eq!(restored_snapshot.get::<_, i64>(1), 1);
    assert_eq!(
        restored_snapshot.get::<_, String>(2),
        "response_recovery_alpha"
    );
    assert_eq!(restored_snapshot.get::<_, String>(3), DIGEST_A);

    let restored_event = client
        .query_one(
            &format!(
                "SELECT session_ref, client_event_ref, item_version_ref, payload_digest, server_sequence,
                        observed_at, received_at
                 FROM {RESTORED_SCHEMA}.response_event
                 WHERE response_event_ref = 'response_event_recovery_alpha'"
            ),
            &[],
        )
        .expect("accepted mid-session response events should survive restore");
    assert_eq!(restored_event.get::<_, String>(0), "session_recovery_alpha");
    assert_eq!(
        restored_event.get::<_, String>(1),
        "client_event_recovery_alpha"
    );
    assert_eq!(
        restored_event.get::<_, String>(2),
        "item_version_recovery_alpha"
    );
    assert_eq!(restored_event.get::<_, String>(3), DIGEST_A);
    assert_eq!(restored_event.get::<_, i64>(4), 1);
    assert_eq!(
        restored_event.get::<_, SystemTime>(5),
        UNIX_EPOCH + Duration::from_secs(1_700_000_000)
    );
    assert_eq!(
        restored_event.get::<_, SystemTime>(6),
        UNIX_EPOCH + Duration::from_millis(1_700_000_000_250)
    );
}

fn assert_restored_tenant_scoped_deduplication(client: &mut Client) {
    let duplicate = client
        .execute(
            &format!(
                "INSERT INTO {RESTORED_SCHEMA}.integration_outbox (
                    event_ref, event_type, schema_version, source_ref, tenant_ref, subject_ref,
                    occurred_at_unix_ms, correlation_ref, causation_ref, payload_digest, max_attempts,
                    current_state, latest_event_at_unix_ms
                 ) VALUES (
                    'event_recovery_alpha', 'assessment.result.available', 'v1',
                    'psychometrics_commons', 'tenant_recovery_alpha', 'result_recovery_alpha',
                    10000, 'correlation_recovery_alpha', NULL, '{DIGEST_B}', 5, 'pending', 10000
                 )"
            ),
            &[],
        )
        .expect_err(
            "restore must preserve the original scoped outbox identity instead of permitting a conflicting replay",
        );
    let database_error = duplicate
        .as_db_error()
        .expect("conflicting restored outbox replay must fail at the database constraint boundary");
    assert_eq!(database_error.code(), &SqlState::UNIQUE_VIOLATION);
    assert_eq!(database_error.constraint(), Some("integration_outbox_pkey"));

    let independent_tenant = client
        .execute(
            &format!(
                "INSERT INTO {RESTORED_SCHEMA}.integration_outbox (
                    event_ref, event_type, schema_version, source_ref, tenant_ref, subject_ref,
                    occurred_at_unix_ms, correlation_ref, causation_ref, payload_digest, max_attempts,
                    current_state, latest_event_at_unix_ms
                 ) VALUES (
                    'event_recovery_alpha', 'assessment.result.available', 'v1',
                    'psychometrics_commons', 'tenant_recovery_beta', 'result_recovery_beta',
                    14000, 'correlation_recovery_beta', NULL, '{DIGEST_B}', 5, 'pending', 14000
                 )"
            ),
            &[],
        )
        .expect("restored tenant-scoped deduplication must not become globally keyed");
    assert_eq!(independent_tenant, 1);
}

#[test]
fn clean_restore_preserves_provenance_deduplication_and_fencing_state() {
    let mut client = connect_client();
    client
        .query_one("SELECT pg_advisory_lock($1)", &[&DATABASE_TEST_LOCK_KEY])
        .expect("shared PostgreSQL recovery-test advisory lock should be acquired");

    let files = migration_files();
    apply_migration_chain(&mut client, SOURCE_SCHEMA, &files);
    seed_recovery_critical_state(&mut client);

    let tables = [
        "integration_outbox",
        "integration_inbox",
        "integration_consumption",
        "response_snapshot",
        "response_snapshot_entry",
        "response_event",
    ];
    let backups: Vec<(&str, Vec<u8>)> = tables
        .iter()
        .map(|table| (*table, copy_table_out(&mut client, SOURCE_SCHEMA, table)))
        .collect();

    apply_migration_chain(&mut client, RESTORED_SCHEMA, &files);
    for (table, bytes) in &backups {
        copy_table_in(&mut client, RESTORED_SCHEMA, table, bytes);
    }

    assert_restored_evidence(&mut client);
    assert_restored_tenant_scoped_deduplication(&mut client);

    client
        .batch_execute(&format!(
            "DROP SCHEMA IF EXISTS {SOURCE_SCHEMA} CASCADE;
             DROP SCHEMA IF EXISTS {RESTORED_SCHEMA} CASCADE;"
        ))
        .expect("recovery acceptance schemas should be removed");
}
