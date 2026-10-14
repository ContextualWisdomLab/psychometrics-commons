//! Exhaustive two-way numeric classification parity for participant references.
//!
//! `PostgreSQL` `text` cannot represent U+0000, so the test covers every other
//! Unicode scalar value that Rust can encode as UTF-8 and compares the database
//! predicate directly with Rust 1.97 `char::is_numeric`.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::postgres_participant::apply_participant_base_migration;

const PARITY_BATCH_SIZE: usize = 4096;

fn test_client() -> (Client, String) {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let mut client = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    let schema = format!("participant_numeric_parity_test_{}", std::process::id());
    client
        .batch_execute(&format!(
            "DROP SCHEMA IF EXISTS {schema} CASCADE;\
             CREATE SCHEMA {schema};\
             SET search_path TO {schema};"
        ))
        .expect("isolated parity schema must be created");
    (client, schema)
}

fn assert_parity_batch(client: &mut Client, samples: &[String], expected: &[bool]) {
    assert_eq!(samples.len(), expected.len());
    let all_match: bool = client
        .query_one(
            "SELECT bool_and(opaque_reference_numeric_like(sample_text) = expected_numeric) \
             FROM unnest($1::text[], $2::bool[]) \
             AS sample(sample_text, expected_numeric)",
            &[&samples, &expected],
        )
        .expect("PostgreSQL numeric-parity query must execute")
        .get(0);
    assert!(
        all_match,
        "database numeric-like classification must exactly match Rust char::is_numeric"
    );
}

#[test]
fn database_numeric_like_predicate_matches_rust_in_both_directions() {
    let (mut client, schema) = test_client();
    apply_participant_base_migration(&mut client).unwrap();

    assert!(
        !'\0'.is_numeric(),
        "the one Unicode scalar PostgreSQL text cannot represent must not be numeric"
    );

    let mut samples = Vec::with_capacity(PARITY_BATCH_SIZE);
    let mut expected = Vec::with_capacity(PARITY_BATCH_SIZE);

    for scalar in (1u32..=0x0010_FFFF).filter_map(char::from_u32) {
        samples.push(scalar.to_string());
        expected.push(scalar.is_numeric());

        if samples.len() == PARITY_BATCH_SIZE {
            assert_parity_batch(&mut client, &samples, &expected);
            samples.clear();
            expected.clear();
        }
    }

    if !samples.is_empty() {
        assert_parity_batch(&mut client, &samples, &expected);
    }

    client
        .batch_execute(&format!("DROP SCHEMA {schema} CASCADE;"))
        .expect("isolated parity schema must be removable");
}
