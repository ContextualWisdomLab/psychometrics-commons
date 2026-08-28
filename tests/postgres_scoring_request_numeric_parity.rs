//! Exhaustive real-PostgreSQL parity for Rust numeric-only scoring references.
//!
//! The SQL validator carries a generated Unicode numeric multirange so direct SQL cannot admit a
//! numeric-only identity that Rust `char::is_numeric` rejects at the product boundary. This test
//! enumerates the toolchain's complete scalar set instead of relying on representative examples.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::postgres_scoring_request::apply_scoring_request_migration;

const SCHEMA_LOCK_KEY: i64 = 0x5343_4F52_4E55_4D50;

fn client() -> Client {
    let url = std::env::var("TEST_DATABASE_URL").expect("TEST_DATABASE_URL is required");
    let mut client = Client::connect(&url, NoTls).expect("CI PostgreSQL must be reachable");
    client
        .query_one(
            "SELECT set_config('lock_timeout', '60s', false), pg_advisory_lock($1)",
            &[&SCHEMA_LOCK_KEY],
        )
        .expect("scoring numeric-parity fixture lock must be acquired within sixty seconds");
    client
        .batch_execute(
            "DROP SCHEMA IF EXISTS scoring_request_numeric_parity_test CASCADE; \
             CREATE SCHEMA scoring_request_numeric_parity_test; \
             SET search_path TO scoring_request_numeric_parity_test;",
        )
        .expect("numeric-parity schema must initialize");
    apply_scoring_request_migration(&mut client).expect("scoring-request migration must apply");
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
             WHERE scoring_request_reference_is_valid(candidate)",
            &[&rust_numeric],
        )
        .expect("PostgreSQL must classify the complete Rust numeric scalar set")
        .into_iter()
        .map(|row| row.get(0))
        .collect();

    assert!(
        accepted.is_empty(),
        "SQL accepted Rust-numeric-only reference scalars: {accepted:?}"
    );
}
