//! PostgreSQL parity regression for invisible longitudinal reference aliases.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::postgres_longitudinal_observation::apply_longitudinal_observation_migration;

fn client() -> Client {
    let url = std::env::var("TEST_DATABASE_URL").expect("TEST_DATABASE_URL is required");
    let mut client = Client::connect(&url, NoTls).expect("CI PostgreSQL must be reachable");
    client
        .batch_execute(
            "DROP SCHEMA IF EXISTS longitudinal_default_ignorable_parity CASCADE; \
             CREATE SCHEMA longitudinal_default_ignorable_parity; \
             SET search_path TO longitudinal_default_ignorable_parity;",
        )
        .unwrap();
    client
}

#[test]
fn default_ignorable_references_match_the_rust_fail_closed_boundary() {
    let mut client = client();
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
}
