//! PostgreSQL/Rust parity for the longitudinal outer-whitespace reference boundary.
//!
//! `normalized_reference` trims with Rust `char::is_whitespace`, so durable validation must reject
//! every Rust-whitespace scalar at either edge without rejecting the same visible whitespace when
//! it is legitimate identity material inside an otherwise opaque reference. The result must not
//! depend on the caller's text collation: a durable identity validator must behave the same under
//! the database default and `PostgreSQL`'s byte-oriented `C` collation.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::postgres_longitudinal_observation::apply_longitudinal_observation_migration;

#[test]
fn postgres_rejects_every_rust_whitespace_scalar_at_reference_edges() {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let mut client = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    let schema = format!("longitudinal_whitespace_parity_test_{}", std::process::id());
    client
        .batch_execute(&format!(
            "DROP SCHEMA IF EXISTS {schema} CASCADE;\
             CREATE SCHEMA {schema};\
             SET search_path TO {schema};"
        ))
        .expect("isolated whitespace-parity schema must be created");
    apply_longitudinal_observation_migration(&mut client).unwrap();

    let whitespace: Vec<char> = (0..=char::MAX as u32)
        .filter_map(char::from_u32)
        .filter(|character| character.is_whitespace())
        .collect();
    assert!(
        !whitespace.is_empty(),
        "the pinned Rust toolchain must classify whitespace"
    );

    for character in whitespace {
        for reference in [
            format!("{character}tenant_alpha"),
            format!("tenant_alpha{character}"),
        ] {
            for query in [
                "SELECT longitudinal_reference_is_valid($1)",
                "SELECT longitudinal_reference_is_valid($1::text COLLATE \"C\")",
            ] {
                let is_valid: bool = client
                    .query_one(query, &[&reference])
                    .expect("PostgreSQL whitespace-parity query must execute")
                    .get(0);
                assert!(
                    !is_valid,
                    "PostgreSQL accepted outer Rust whitespace U+{:04X} in {reference:?} for query {query:?}",
                    character as u32
                );
            }
        }
    }

    for reference in ["tenant alpha", "tenant\u{00a0}alpha", "tenant\u{3000}alpha"] {
        for query in [
            "SELECT longitudinal_reference_is_valid($1)",
            "SELECT longitudinal_reference_is_valid($1::text COLLATE \"C\")",
        ] {
            let is_valid: bool = client
                .query_one(query, &[&reference])
                .expect("PostgreSQL embedded-whitespace parity query must execute")
                .get(0);
            assert!(
                is_valid,
                "visible embedded whitespace must remain valid identity material: {reference:?} for query {query:?}"
            );
        }
    }

    client
        .batch_execute(&format!("DROP SCHEMA {schema} CASCADE;"))
        .expect("isolated whitespace-parity schema must be removable");
}
