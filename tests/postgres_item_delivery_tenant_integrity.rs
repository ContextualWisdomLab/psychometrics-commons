//! Real `PostgreSQL` tenant and identity integrity for item-delivery persistence.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::postgres_item_delivery::apply_item_delivery_migration;
use std::sync::{Mutex, MutexGuard};

static ITEM_DELIVERY_TENANT_LOCK: Mutex<()> = Mutex::new(());

fn tenant_test_guard() -> MutexGuard<'static, ()> {
    ITEM_DELIVERY_TENANT_LOCK
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
