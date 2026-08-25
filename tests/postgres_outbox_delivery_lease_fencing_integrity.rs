//! Physical integrity contract for durable outbox lease fencing evidence.

use postgres::{error::SqlState, Client, NoTls};
use psychometrics_commons_runtime::postgres_integration::apply_integration_migration;

const DATABASE_TEST_LOCK_KEY: i64 = 0x4F55_5442_4F58_4649;

fn schema_name() -> String {
    format!("outbox_lease_fence_integrity_{}", std::process::id())
}

fn acquire_database_lock(
    client: &mut Client,
    lock_key: i64,
    lock_timeout: &str,
) -> Result<(), postgres::Error> {
    client.query_one(
        "SELECT set_config('lock_timeout', $1, false)",
        &[&lock_timeout],
    )?;
    client.query_one("SELECT pg_advisory_lock($1)", &[&lock_key])?;
    Ok(())
}

fn ready_client() -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let mut client = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    acquire_database_lock(&mut client, DATABASE_TEST_LOCK_KEY, "60s")
        .expect("shared PostgreSQL outbox fencing integrity lock should be acquired within 60 seconds");
    let schema = schema_name();
    client
        .batch_execute(&format!(
            "DROP SCHEMA IF EXISTS {schema} CASCADE;
             CREATE SCHEMA {schema};
             SET search_path TO {schema};"
        ))
        .unwrap();
    apply_integration_migration(&mut client).unwrap();
    client
}

fn cleanup(client: &mut Client) {
    let schema = schema_name();
    client
        .batch_execute(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE;"))
        .unwrap();
}

#[test]
fn fixture_lock_wait_is_bounded_under_real_contention() {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let mut holder = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    let behavior_lock_key: i64 = holder
        .query_one("SELECT pg_backend_pid()::bigint", &[])
        .expect("holder backend identity should be queryable")
        .get(0);
    holder
        .query_one("SELECT pg_advisory_lock($1)", &[&behavior_lock_key])
        .expect("behavior-test holder should acquire its private advisory lock");

    let mut contender = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    let error = acquire_database_lock(&mut contender, behavior_lock_key, "100ms")
        .expect_err("contended advisory-lock acquisition must stop at the configured timeout");

    assert_eq!(error.code(), Some(&SqlState::LOCK_NOT_AVAILABLE));
    let released: bool = holder
        .query_one("SELECT pg_advisory_unlock($1)", &[&behavior_lock_key])
        .expect("behavior-test holder should release its advisory lock")
        .get(0);
    assert!(released, "behavior-test advisory lock should be released");
}

#[test]
fn current_lease_fencing_token_must_equal_persisted_generation() {
    let mut client = ready_client();
    client
        .execute(
            "INSERT INTO integration_outbox (
                 event_ref, event_type, schema_version, source_ref, tenant_ref,
                 subject_ref, occurred_at_unix_ms, correlation_ref, payload_digest,
                 max_attempts, latest_event_at_unix_ms
             ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$7)",
            &[
                &"event_alpha",
                &"assessment.completed",
                &"v1",
                &"psychometrics_commons",
                &"tenant_alpha",
                &"subject_alpha",
                &10_000_i64,
                &"correlation_alpha",
                &"sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                &3_i32,
            ],
        )
        .unwrap();

    let error = client
        .execute(
            "UPDATE integration_outbox
             SET lease_worker_ref = 'worker_alpha',
                 lease_ref = 'lease_alpha',
                 lease_fencing_token = 2,
                 lease_expires_at_unix_ms = 20_000,
                 delivery_lease_generation = 1
             WHERE source_ref = 'psychometrics_commons'
               AND tenant_ref = 'tenant_alpha'
               AND event_ref = 'event_alpha'",
            &[],
        )
        .expect_err("current lease token must be the current delivery lease generation");
    assert_eq!(
        error.code(),
        Some(&postgres::error::SqlState::CHECK_VIOLATION)
    );
    cleanup(&mut client);
}
