//! Guard item-delivery CHECK revalidation when the scalar reference validator changes.
//!
//! PostgreSQL does not automatically rescan rows behind an immutable CHECK predicate when the
//! predicate function is replaced. The migration therefore stores a policy marker on the scalar
//! validator and rebuilds its dependent CHECK constraints only when that marker changes. This
//! contract derives the expected marker from PostgreSQL's normalized live function definition so
//! a future validator edit cannot silently retain stale validation evidence.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::postgres_item_delivery::apply_item_delivery_migration;

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
                     md5(pg_get_functiondef(\
                         'item_delivery_reference_is_valid(text)'::regprocedure\
                     ))",
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
