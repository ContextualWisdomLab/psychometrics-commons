//! Real `PostgreSQL` integrity contract for allowed item-version evidence.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::postgres_item_delivery::apply_item_delivery_migration;

const DIGEST: &str = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn test_client() -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    Client::connect(&connection, NoTls).expect("isolated CI PostgreSQL database must be reachable")
}

fn constraint_name(error: &postgres::Error) -> &str {
    error
        .as_db_error()
        .and_then(postgres::error::DbError::constraint)
        .unwrap_or_default()
}

fn assert_invalid_allowed_items(
    client: &mut Client,
    session_ref: &str,
    allowed_items: &[Option<String>],
) {
    let error = client
        .execute(
            "INSERT INTO item_delivery_ledger (\
                 tenant_ref, session_ref, instrument_release_ref, instrument_version_ref, \
                 release_content_digest, locale, allowed_item_version_refs\
             ) VALUES ('tenant_item_delivery', $1, 'release_big_five_ko_v1', \
                 'instrument_version_ko_v1', $2, 'ko-KR', $3)",
            &[&session_ref, &DIGEST, &allowed_items],
        )
        .unwrap_err();
    assert_eq!(
        constraint_name(&error),
        "item_delivery_ledger_allowed_items_format_check",
        "unexpected constraint for {session_ref}"
    );
}

#[test]
fn allowed_item_versions_are_non_null_opaque_canonical_and_unique() {
    let mut client = test_client();
    client
        .batch_execute(
            "DROP SCHEMA IF EXISTS item_delivery_allowed_items_test CASCADE;\
             CREATE SCHEMA item_delivery_allowed_items_test;\
             SET search_path TO item_delivery_allowed_items_test;",
        )
        .unwrap();
    apply_item_delivery_migration(&mut client).unwrap();

    for (session_ref, item_values) in [
        (
            "session_duplicate_items",
            vec![Some("item_version_001"), Some("item_version_001")],
        ),
        (
            "session_blank_item",
            vec![Some("item_version_001"), Some("   ")],
        ),
        (
            "session_numeric_item",
            vec![Some("item_version_001"), Some("12345")],
        ),
        (
            "session_noncanonical_item",
            vec![Some("item_version_001"), Some(" item_version_002 ")],
        ),
        ("session_null_item", vec![Some("item_version_001"), None]),
    ] {
        let allowed_items: Vec<Option<String>> = item_values
            .into_iter()
            .map(|value| value.map(str::to_owned))
            .collect();
        assert_invalid_allowed_items(&mut client, session_ref, &allowed_items);
    }

    client
        .batch_execute("DROP SCHEMA item_delivery_allowed_items_test CASCADE;")
        .unwrap();
}
