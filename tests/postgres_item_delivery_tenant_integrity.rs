//! Real `PostgreSQL` tenant and identity integrity for item-delivery persistence.

use postgres::{error::SqlState, Client, NoTls};
use psychometrics_commons_runtime::postgres_item_delivery::apply_item_delivery_migration;

const ITEM_DELIVERY_TENANT_LOCK_KEY: i64 = 0x4954_544E_544C_4F43;

fn acquire_tenant_fixture_lock(
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

fn tenant_test_guard() -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let mut guard = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    acquire_tenant_fixture_lock(&mut guard, ITEM_DELIVERY_TENANT_LOCK_KEY, "60s").expect(
        "shared item-delivery tenant integrity test lock should be acquired within sixty seconds",
    );
    guard
}

fn test_client() -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let mut client = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    client
        .batch_execute(
            "CREATE SCHEMA IF NOT EXISTS item_delivery_tenant_test;\
             SET search_path TO item_delivery_tenant_test;",
        )
        .unwrap();
    client
}

fn reset_tables(client: &mut Client) {
    client
        .batch_execute(
            "DROP TABLE IF EXISTS item_delivery_tenant_test.item_delivery_event;\
             DROP TABLE IF EXISTS item_delivery_tenant_test.item_delivery_ledger;",
        )
        .unwrap();
}

fn constraint_name(error: &postgres::Error) -> String {
    error
        .as_db_error()
        .and_then(postgres::error::DbError::constraint)
        .unwrap_or_default()
        .to_owned()
}

fn insert_ledger(client: &mut Client, tenant_ref: &str, session_ref: &str) {
    client
        .execute(
            "INSERT INTO item_delivery_ledger (\
                 tenant_ref, session_ref, instrument_release_ref, instrument_version_ref, \
                 release_content_digest, locale, allowed_item_version_refs\
             ) VALUES ($1, $2, 'release_big_five_ko_v1', 'instrument_version_ko_v1', \
                 'sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef', \
                 'ko-KR', ARRAY['item_version_001'])",
            &[&tenant_ref, &session_ref],
        )
        .unwrap();
}

#[test]
fn item_delivery_tenant_fixture_guard_is_visible_to_another_postgres_session() {
    let _guard = tenant_test_guard();
    let mut contender = test_client();
    let acquired: bool = contender
        .query_one(
            "SELECT pg_try_advisory_lock($1)",
            &[&ITEM_DELIVERY_TENANT_LOCK_KEY],
        )
        .expect("contender lock probe should succeed")
        .get(0);

    assert!(
        !acquired,
        "fixed-schema item-delivery tenant fixture guard must serialize across PostgreSQL sessions"
    );
}

#[test]
fn item_delivery_tenant_fixture_lock_wait_has_finite_postgresql_budget() {
    let mut guard = tenant_test_guard();
    let timeout_ms: i64 = guard
        .query_one(
            "SELECT setting::bigint FROM pg_settings WHERE name = 'lock_timeout'",
            &[],
        )
        .expect("item-delivery tenant fixture lock timeout should be queryable from PostgreSQL")
        .get(0);

    assert_eq!(
        timeout_ms, 60_000,
        "item-delivery tenant fixture must not wait indefinitely for its PostgreSQL advisory lock"
    );
}

#[test]
fn item_delivery_tenant_fixture_lock_wait_aborts_under_real_contention() {
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
    let error = acquire_tenant_fixture_lock(&mut contender, behavior_lock_key, "100ms").expect_err(
        "contended item-delivery tenant fixture lock must stop at the configured timeout",
    );
    assert_eq!(error.code(), Some(&SqlState::LOCK_NOT_AVAILABLE));

    let released: bool = holder
        .query_one("SELECT pg_advisory_unlock($1)", &[&behavior_lock_key])
        .expect("behavior-test advisory lock should be released")
        .get(0);
    assert!(released, "behavior-test advisory lock should be released");
}

#[test]
fn ledger_and_event_rows_require_explicit_tenant_scope() {
    let _guard = tenant_test_guard();
    let mut client = test_client();
    reset_tables(&mut client);
    apply_item_delivery_migration(&mut client).unwrap();

    let invalid_ledger_tenant = client
        .execute(
            "INSERT INTO item_delivery_ledger (\
                 tenant_ref, session_ref, instrument_release_ref, instrument_version_ref, \
                 release_content_digest, locale, allowed_item_version_refs\
             ) VALUES ('12', 'session_tenant_scope', 'release_big_five_ko_v1', \
                 'instrument_version_ko_v1', \
                 'sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef', \
                 'ko-KR', ARRAY['item_version_001'])",
            &[],
        )
        .unwrap_err();
    assert_eq!(
        constraint_name(&invalid_ledger_tenant),
        "item_delivery_ledger_tenant_ref_format_check"
    );

    insert_ledger(&mut client, "tenant_alpha", "session_tenant_scope");
    let invalid_event_tenant = client
        .execute(
            "INSERT INTO item_delivery_event (\
                 tenant_ref, session_ref, delivery_event_ref, item_version_ref, \
                 presentation_context_ref, delivery_sequence\
             ) VALUES ('12', 'session_tenant_scope', 'delivery_tenant_scope', \
                 'item_version_001', 'presentation_standard_v1', 1)",
            &[],
        )
        .unwrap_err();
    assert_eq!(
        constraint_name(&invalid_event_tenant),
        "item_delivery_event_tenant_ref_format_check"
    );
}

#[test]
fn event_cannot_cross_tenant_session_binding() {
    let _guard = tenant_test_guard();
    let mut client = test_client();
    reset_tables(&mut client);
    apply_item_delivery_migration(&mut client).unwrap();
    insert_ledger(&mut client, "tenant_alpha", "session_cross_tenant");

    let error = client
        .execute(
            "INSERT INTO item_delivery_event (\
                 tenant_ref, session_ref, delivery_event_ref, item_version_ref, \
                 presentation_context_ref, delivery_sequence\
             ) VALUES ('tenant_beta', 'session_cross_tenant', 'delivery_cross_tenant', \
                 'item_version_001', 'presentation_standard_v1', 1)",
            &[],
        )
        .unwrap_err();
    assert_eq!(
        constraint_name(&error),
        "item_delivery_event_session_tenant_fk"
    );
}

#[test]
fn delivery_event_reference_is_globally_collision_resistant() {
    let _guard = tenant_test_guard();
    let mut client = test_client();
    reset_tables(&mut client);
    apply_item_delivery_migration(&mut client).unwrap();
    insert_ledger(&mut client, "tenant_alpha", "session_delivery_alpha");
    insert_ledger(&mut client, "tenant_beta", "session_delivery_beta");

    client
        .execute(
            "INSERT INTO item_delivery_event (\
                 tenant_ref, session_ref, delivery_event_ref, item_version_ref, \
                 presentation_context_ref, delivery_sequence\
             ) VALUES ('tenant_alpha', 'session_delivery_alpha', 'delivery_global_identity', \
                 'item_version_001', 'presentation_standard_v1', 1)",
            &[],
        )
        .unwrap();
    let error = client
        .execute(
            "INSERT INTO item_delivery_event (\
                 tenant_ref, session_ref, delivery_event_ref, item_version_ref, \
                 presentation_context_ref, delivery_sequence\
             ) VALUES ('tenant_beta', 'session_delivery_beta', 'delivery_global_identity', \
                 'item_version_001', 'presentation_standard_v1', 1)",
            &[],
        )
        .unwrap_err();
    assert_eq!(
        constraint_name(&error),
        "item_delivery_event_delivery_ref_unique"
    );
}
