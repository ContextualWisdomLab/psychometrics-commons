//! Exhaustive real-PostgreSQL parity for Rust numeric-only item-delivery references.
//!
//! Item-delivery SQL carries a generated Unicode numeric table so direct SQL cannot admit a
//! numeric-only identity that Rust `char::is_numeric` rejects. This contract enumerates the full
//! scalar set exposed by the pinned Rust toolchain rather than relying on representative examples.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::postgres_item_delivery::apply_item_delivery_migration;

const SCHEMA_LOCK_KEY: i64 = 0x4954_444E_554D_5041;

fn client() -> Client {
    let url = std::env::var("TEST_DATABASE_URL").expect("TEST_DATABASE_URL is required");
    let mut client = Client::connect(&url, NoTls).expect("CI PostgreSQL must be reachable");
    client
        .query_one(
            "SELECT set_config('lock_timeout', '60s', false), pg_advisory_lock($1)",
            &[&SCHEMA_LOCK_KEY],
        )
        .expect("item-delivery numeric-parity fixture lock must be acquired within sixty seconds");
    client
        .batch_execute(
            "DROP SCHEMA IF EXISTS item_delivery_numeric_parity_test CASCADE; \
             CREATE SCHEMA item_delivery_numeric_parity_test; \
             SET search_path TO item_delivery_numeric_parity_test;",
        )
        .expect("numeric-parity schema must initialize");
    apply_item_delivery_migration(&mut client).expect("item-delivery migration must apply");
    client
}

#[test]
fn sql_rejects_every_single_scalar_rust_classifies_as_numeric() {
    let mut client = client();
    let rust_numeric: Vec<String> = (1u32..=0x0010_FFFF)
        .filter_map(char::from_u32)
        .filter(|character| character.is_numeric())
        .map(|character| character.to_string())
        .collect();

    assert!(
        !rust_numeric.is_empty(),
        "the pinned Rust toolchain must expose numeric Unicode scalars"
    );

    let accepted: Vec<String> = client
        .query(
            "SELECT candidate \
             FROM unnest($1::text[]) AS candidate \
             WHERE item_delivery_reference_is_valid(candidate)",
            &[&rust_numeric],
        )
        .expect("PostgreSQL must classify the complete Rust numeric scalar set")
        .into_iter()
        .map(|row| row.get(0))
        .collect();

    assert!(
        accepted.is_empty(),
        "SQL accepted Rust-numeric-only item-delivery references: {accepted:?}"
    );
}
