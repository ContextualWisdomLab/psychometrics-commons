//! Durable outbox lease identities must match the Rust opaque-reference boundary.

use postgres::{error::SqlState, Client, NoTls};
use psychometrics_commons_runtime::postgres_integration::apply_integration_migration;
use std::sync::{Mutex, MutexGuard};

const DIGEST: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
static TEST_LOCK: Mutex<()> = Mutex::new(());

fn guard() -> MutexGuard<'static, ()> {
    TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn client() -> Client {
    let url = std::env::var("TEST_DATABASE_URL").expect("TEST_DATABASE_URL is required");
    let mut client = Client::connect(&url, NoTls).expect("CI PostgreSQL must be reachable");
    client
        .batch_execute(
            "DROP SCHEMA IF EXISTS outbox_lease_reference_parity_test CASCADE; \
             CREATE SCHEMA outbox_lease_reference_parity_test; \
             SET search_path TO outbox_lease_reference_parity_test;",
        )
        .unwrap();
    apply_integration_migration(&mut client).unwrap();
    client
}

fn assert_check(error: &postgres::Error, constraint: &str) {
    let database_error = error
        .as_db_error()
        .expect("reference rejection must be a PostgreSQL CHECK violation");
    assert_eq!(database_error.code(), &SqlState::CHECK_VIOLATION);
    assert_eq!(database_error.constraint(), Some(constraint));
}

fn insert_outbox(client: &mut Client, suffix: &str) {
    client
        .execute(
            "INSERT INTO integration_outbox (\
                 event_ref, event_type, schema_version, source_ref, tenant_ref, subject_ref,\
                 occurred_at_unix_ms, correlation_ref, payload_digest, max_attempts,\
                 current_state, latest_event_at_unix_ms\
             ) VALUES ($1,'assessment.session.completed','v1',$2,$3,$4,10000,$5,$6,3,'pending',10000)",
            &[
                &format!("event_{suffix}"),
                &format!("source_{suffix}"),
                &format!("tenant_{suffix}"),
                &format!("subject_{suffix}"),
                &format!("correlation_{suffix}"),
                &DIGEST,
            ],
        )
        .unwrap();
}

fn set_lease(
    client: &mut Client,
    suffix: &str,
    worker_ref: &str,
    lease_ref: &str,
) -> Result<u64, postgres::Error> {
    client.execute(
        "UPDATE integration_outbox SET \
             lease_worker_ref=$1, lease_ref=$2, lease_fencing_token=1, \
             lease_expires_at_unix_ms=20000, delivery_lease_generation=1 \
         WHERE source_ref=$3 AND tenant_ref=$4 AND event_ref=$5",
        &[
            &worker_ref,
            &lease_ref,
            &format!("source_{suffix}"),
            &format!("tenant_{suffix}"),
            &format!("event_{suffix}"),
        ],
    )
}

#[test]
fn worker_and_lease_references_reject_unicode_numeric_whitespace_and_control_aliases() {
    let _guard = guard();
    let mut client = client();

    for (index, invalid_ref) in ["½", "²", "Ⅳ", "\u{00a0}worker_alpha", "worker_\u{0001}_alpha"]
        .into_iter()
        .enumerate()
    {
        let suffix = format!("worker_{index}");
        insert_outbox(&mut client, &suffix);
        let error = set_lease(&mut client, &suffix, invalid_ref, "lease_valid")
            .expect_err("worker reference must match normalized_reference");
        assert_check(&error, "integration_outbox_lease_worker_ref_format_check");
    }

    for (index, invalid_ref) in ["½", "²", "Ⅳ", "\u{00a0}lease_alpha", "lease_\u{0001}_alpha"]
        .into_iter()
        .enumerate()
    {
        let suffix = format!("lease_{index}");
        insert_outbox(&mut client, &suffix);
        let error = set_lease(&mut client, &suffix, "worker_valid", invalid_ref)
            .expect_err("lease reference must match normalized_reference");
        assert_check(&error, "integration_outbox_lease_ref_format_check");
    }
}

#[test]
fn migration_reapplication_repairs_weakened_lease_reference_constraints() {
    let _guard = guard();
    let mut client = client();

    client
        .batch_execute(
            "ALTER TABLE integration_outbox \
                 DROP CONSTRAINT integration_outbox_lease_worker_ref_format_check; \
             ALTER TABLE integration_outbox \
                 ADD CONSTRAINT integration_outbox_lease_worker_ref_format_check CHECK (\
                     lease_worker_ref IS NULL OR (lease_worker_ref=btrim(lease_worker_ref) AND lease_worker_ref<>'')\
                 );",
        )
        .unwrap();
    apply_integration_migration(&mut client).unwrap();

    insert_outbox(&mut client, "upgrade_guard");
    let error = set_lease(&mut client, "upgrade_guard", "½", "lease_upgrade_guard")
        .expect_err("reapplication must restore the exact worker-reference predicate");
    assert_check(&error, "integration_outbox_lease_worker_ref_format_check");
}

#[test]
fn migration_reapplication_fails_closed_on_historical_invalid_lease_identity() {
    let _guard = guard();
    let mut client = client();

    client
        .batch_execute(
            "ALTER TABLE integration_outbox \
                 DROP CONSTRAINT integration_outbox_lease_ref_format_check; \
             ALTER TABLE integration_outbox \
                 ADD CONSTRAINT integration_outbox_lease_ref_format_check CHECK (\
                     lease_ref IS NULL OR (lease_ref=btrim(lease_ref) AND lease_ref<>'')\
                 );",
        )
        .unwrap();
    insert_outbox(&mut client, "historical");
    set_lease(&mut client, "historical", "worker_historical", "½")
        .expect("weakened historical predicate must admit the regression fixture");

    let error = apply_integration_migration(&mut client)
        .expect_err("upgrade must fail closed instead of blessing invalid durable lease identity");
    assert_check(&error, "integration_outbox_lease_ref_format_check");
}
