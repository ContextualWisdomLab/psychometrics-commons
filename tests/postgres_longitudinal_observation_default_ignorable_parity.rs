//! `PostgreSQL` parity regression for invisible longitudinal reference aliases.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::postgres_longitudinal_observation::apply_longitudinal_observation_migration;

/// Connects to the isolated CI database and creates a per-process parity-test schema.
fn client(schema: &str) -> Client {
    let url = std::env::var("TEST_DATABASE_URL").expect("TEST_DATABASE_URL is required");
    let mut client = Client::connect(&url, NoTls).expect("CI PostgreSQL must be reachable");
    client
        .batch_execute(&format!(
            "DROP SCHEMA IF EXISTS {schema} CASCADE; \
             CREATE SCHEMA {schema}; \
             SET search_path TO {schema};"
        ))
        .unwrap();
    client
}

/// Verifies invisible aliases fail closed while visible multilingual references remain valid.
#[test]
fn default_ignorable_references_match_the_rust_fail_closed_boundary() {
    let schema = format!(
        "longitudinal_default_ignorable_parity_{}",
        std::process::id()
    );
    let mut client = client(&schema);
    apply_longitudinal_observation_migration(&mut client).unwrap();

    for reference in [
        "tenant\u{200b}_clinic",
        "tenant\u{200d}_clinic",
        "tenant\u{202e}_clinic",
        "tenant\u{2060}_clinic",
        "tenant\u{fe0f}_clinic",
        "tenant\u{e0001}_clinic",
    ] {
        let is_valid: bool = client
            .query_one("SELECT longitudinal_reference_is_valid($1)", &[&reference])
            .unwrap()
            .get(0);
        assert!(
            !is_valid,
            "PostgreSQL accepted an invisible alias rejected by Rust: {reference:?}"
        );
    }

    let visible_multilingual_reference = "tenant_가나다_東京_éclair";
    let is_valid: bool = client
        .query_one(
            "SELECT longitudinal_reference_is_valid($1)",
            &[&visible_multilingual_reference],
        )
        .unwrap()
        .get(0);
    assert!(
        is_valid,
        "visible multilingual identity material must remain valid"
    );

    client
        .batch_execute(&format!(
            "SET search_path TO public; DROP SCHEMA {schema} CASCADE;"
        ))
        .expect("isolated default-ignorable parity schema must be removable");
    let schema_was_removed: bool = client
        .query_one("SELECT to_regnamespace($1) IS NULL", &[&schema])
        .expect("schema cleanup must be observable")
        .get(0);
    assert!(
        schema_was_removed,
        "default-ignorable parity test left residual schema {schema}"
    );
}
