//! Real `PostgreSQL` bounds for durable item-delivery rows.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::postgres_item_delivery::apply_item_delivery_migration;
use std::sync::{Mutex, MutexGuard};

const ITEM_DELIVERY_SCHEMA_DATABASE_LOCK_KEY: i64 = 0x4954_454D_5343_484D;
static ITEM_DELIVERY_SCHEMA_LOCK: Mutex<()> = Mutex::new(());

fn schema_test_guard() -> MutexGuard<'static, ()> {
    ITEM_DELIVERY_SCHEMA_LOCK
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
            "CREATE SCHEMA IF NOT EXISTS item_delivery_schema_test;\
             SET search_path TO item_delivery_schema_test;",
        )
        .unwrap();
    client
}

fn reset_schema(client: &mut Client) {
    client
        .batch_execute(
            "DROP TABLE IF EXISTS item_delivery_schema_test.item_delivery_event;\
             DROP TABLE IF EXISTS item_delivery_schema_test.item_delivery_ledger;",
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

#[test]
fn fixture_lock_is_visible_across_database_sessions() {
    let _guard = schema_test_guard();
    let _owner = test_client();
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let mut contender = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    let acquired: bool = contender
        .query_one(
            "SELECT pg_try_advisory_lock($1)",
            &[&ITEM_DELIVERY_SCHEMA_DATABASE_LOCK_KEY],
        )
        .unwrap()
        .get(0);

    if acquired {
        contender
            .query_one(
                "SELECT pg_advisory_unlock($1)",
                &[&ITEM_DELIVERY_SCHEMA_DATABASE_LOCK_KEY],
            )
            .unwrap();
    }

    assert!(
        !acquired,
        "fixture serialization must be enforced by PostgreSQL, not only by a process-local mutex"
    );
}

#[test]
fn schema_rejects_numeric_identities_empty_item_sets_and_nonpositive_sequences() {
    let _guard = schema_test_guard();
    let mut client = test_client();
    reset_schema(&mut client);
    apply_item_delivery_migration(&mut client).unwrap();

    let numeric_session = client
        .execute(
            "INSERT INTO item_delivery_ledger (\
             tenant_ref, session_ref, instrument_release_ref, release_content_digest, locale, \
             allowed_item_version_refs\
         ) VALUES ('tenant_item_delivery', '12', 'release_big_five_ko_v1', \
             'sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef', \
             'ko-KR', ARRAY['item_version_001'])",
            &[],
        )
        .unwrap_err();
    assert_eq!(
        constraint_name(&numeric_session),
        "item_delivery_ledger_session_ref_format_check"
    );

    let empty_items = client
        .execute(
            "INSERT INTO item_delivery_ledger (\
             tenant_ref, session_ref, instrument_release_ref, release_content_digest, locale, \
             allowed_item_version_refs\
         ) VALUES ('tenant_item_delivery', 'session_schema_empty', 'release_big_five_ko_v1', \
             'sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef', \
             'ko-KR', ARRAY[]::TEXT[])",
            &[],
        )
        .unwrap_err();
    assert_eq!(
        constraint_name(&empty_items),
        "item_delivery_ledger_allowed_items_not_empty_check"
    );

    let bad_digest = client
        .execute(
            "INSERT INTO item_delivery_ledger (\
             tenant_ref, session_ref, instrument_release_ref, release_content_digest, locale, \
             allowed_item_version_refs\
         ) VALUES ('tenant_item_delivery', 'session_schema_digest', 'release_big_five_ko_v1', \
             'not-a-digest', 'ko-KR', ARRAY['item_version_001'])",
            &[],
        )
        .unwrap_err();
    assert_eq!(
        constraint_name(&bad_digest),
        "item_delivery_ledger_digest_format_check"
    );

    client
        .execute(
            "INSERT INTO item_delivery_ledger (\
                 tenant_ref, session_ref, instrument_release_ref, release_content_digest, locale, \
                 allowed_item_version_refs\
             ) VALUES ('tenant_item_delivery', 'session_schema_valid', 'release_big_five_ko_v1', \
                 'sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef', \
                 'ko-KR', ARRAY['item_version_001'])",
            &[],
        )
        .unwrap();

    let zero_sequence = client
        .execute(
            "INSERT INTO item_delivery_event (\
             tenant_ref, session_ref, delivery_event_ref, item_version_ref, presentation_context_ref, \
             delivery_sequence\
         ) VALUES ('tenant_item_delivery', 'session_schema_valid', 'delivery_event_001', \
             'item_version_001', 'presentation_standard_v1', 0)",
            &[],
        )
        .unwrap_err();
    assert_eq!(
        constraint_name(&zero_sequence),
        "item_delivery_event_sequence_positive_check"
    );

    let numeric_delivery = client
        .execute(
            "INSERT INTO item_delivery_event (\
             tenant_ref, session_ref, delivery_event_ref, item_version_ref, presentation_context_ref, \
             delivery_sequence\
         ) VALUES ('tenant_item_delivery', 'session_schema_valid', '99', 'item_version_001', \
             'presentation_standard_v1', 1)",
            &[],
        )
        .unwrap_err();
    assert_eq!(
        constraint_name(&numeric_delivery),
        "item_delivery_event_delivery_ref_format_check"
    );
}
