//! Guard item-delivery CHECK revalidation when the reference validators change.
//!
//! `PostgreSQL` does not automatically rescan rows behind an immutable CHECK predicate when a
//! predicate function is replaced. The migration therefore stores a policy marker on the scalar
//! validator and rebuilds dependent CHECK constraints only when that marker changes. The scalar
//! validator also uses `ascii(...)` as a Unicode code-point oracle, which is valid only for UTF8
//! databases, so migration application must reject unsupported server encodings before installing
//! either validator.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::postgres_item_delivery::apply_item_delivery_migration;

#[test]
fn migration_checks_utf8_before_installing_unicode_reference_validators() {
    let migration = include_str!("../migrations/0004_item_delivery_evidence.sql");
    let encoding_guard = migration
        .find("current_setting('server_encoding') <> 'UTF8'")
        .expect("migration must fail closed when PostgreSQL server encoding is not UTF8");
    let scalar_validator = migration
        .find("CREATE OR REPLACE FUNCTION item_delivery_reference_is_valid")
        .expect("migration must install the scalar item-delivery reference validator");
    let array_validator = migration
        .find("item_delivery_reference_array_is_valid(reference_values TEXT[])")
        .expect("migration must install the item-delivery reference-array validator");

    assert!(
        encoding_guard < scalar_validator && encoding_guard < array_validator,
        "UTF8 must be verified before ascii(...) is used as a Unicode code-point oracle"
    );
    assert!(migration.contains(
        "item_delivery reference parity requires PostgreSQL server_encoding UTF8"
    ));
}

#[test]
fn reference_policy_marker_is_derived_from_the_live_validator_definition() {
    let url = std::env::var("TEST_DATABASE_URL").expect("TEST_DATABASE_URL is required");
    let mut client = Client::connect(&url, NoTls).expect("CI PostgreSQL must be reachable");
    client
        .batch_execute(
            "DROP SCHEMA IF EXISTS item_delivery_policy_marker_test CASCADE; \
             CREATE SCHEMA item_delivery_policy_marker_test; \
             SET search_path TO item_delivery_policy_marker_test;",
        )
        .unwrap();
    apply_item_delivery_migration(&mut client).unwrap();

    let row = client
        .query_one(
            "SELECT \
                 obj_description(\
                     'item_delivery_reference_is_valid(text)'::regprocedure, \
                     'pg_proc'\
                 ), \
                     'psychometrics-commons:item-delivery-reference:' || \
                     md5(\
                         pg_get_functiondef(\
                             'item_delivery_reference_is_valid(text)'::regprocedure\
                         ) || E'\\n' || \
                         pg_get_functiondef(\
                             'item_delivery_reference_array_is_valid(text[])'::regprocedure\
                         )\
                     )",
            &[],
        )
        .expect("item-delivery reference policy marker must be inspectable");
    let actual_marker: Option<String> = row.get(0);
    let expected_marker: String = row.get(1);
    assert_eq!(
        actual_marker.as_deref(),
        Some(expected_marker.as_str()),
        "validator edits must automatically invalidate stale CHECK validation evidence"
    );
}
