//! Exhaustive Unicode-class parity for durable assessment-session references.
//!
//! An opaque reference is a product-issued identifier whose exact accepted spelling is identity;
//! callers must not reinterpret visually similar spellings as the same resource. Durable lifecycle
//! provenance is the persisted session and command evidence needed to reconstruct that identity
//! after restart or recovery. A Unicode scalar value is a valid Unicode code point representable by
//! Rust `char` (surrogate code points are excluded). Default-ignorable characters are code points
//! that Unicode normally hides during rendering; the product rejects them so visually identical
//! spellings cannot become different database identities.
//!
//! Here, parity means PostgreSQL's persisted-reference validator makes the same accept/reject
//! decision as the Rust 1.97 boundary for the character class under test. PostgreSQL 18 classifies
//! controls through `pg_unicode_fast` and numeric/default-ignorable characters through explicit
//! Unicode 17 multiranges. These tests compare those decisions across every Unicode scalar value
//! representable by PostgreSQL text.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::postgres_assessment_session::apply_assessment_session_migration;
use std::sync::atomic::{AtomicUsize, Ordering};

const PARITY_BATCH_SIZE: usize = 4096;
static NEXT_SCHEMA_ID: AtomicUsize = AtomicUsize::new(0);

fn test_client() -> (Client, String) {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let mut client = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    let schema_id = NEXT_SCHEMA_ID.fetch_add(1, Ordering::Relaxed);
    let schema = format!(
        "assessment_session_unicode_parity_test_{}_{}",
        std::process::id(),
        schema_id
    );
    client
        .batch_execute(&format!(
            "DROP SCHEMA IF EXISTS {schema} CASCADE;\
             CREATE SCHEMA {schema};\
             SET search_path TO {schema};"
        ))
        .expect("isolated parity schema must be created");
    apply_assessment_session_migration(&mut client).unwrap();
    (client, schema)
}

const fn is_default_ignorable(character: char) -> bool {
    matches!(
        character,
        '\u{00AD}'
            | '\u{034F}'
            | '\u{061C}'
            | '\u{115F}'..='\u{1160}'
            | '\u{17B4}'..='\u{17B5}'
            | '\u{180B}'..='\u{180F}'
            | '\u{200B}'..='\u{200F}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2060}'..='\u{206F}'
            | '\u{3164}'
            | '\u{FE00}'..='\u{FE0F}'
            | '\u{FEFF}'
            | '\u{FFA0}'
            | '\u{FFF0}'..='\u{FFF8}'
            | '\u{1BCA0}'..='\u{1BCA3}'
            | '\u{1D173}'..='\u{1D17A}'
            | '\u{E0000}'..='\u{E0FFF}'
    )
}

fn assert_validity_batch(client: &mut Client, samples: &[String], expected: &[bool]) {
    assert_eq!(samples.len(), expected.len());
    let all_match: bool = client
        .query_one(
            "SELECT bool_and(assessment_session_reference_is_valid(sample_text) = expected_valid) \
             FROM unnest($1::text[], $2::bool[]) AS sample(sample_text, expected_valid)",
            &[&samples, &expected],
        )
        .expect("PostgreSQL reference-parity query must execute")
        .get(0);
    assert!(
        all_match,
        "database reference classification must match the Rust character boundary"
    );
}

#[test]
fn postgres_control_class_matches_rust_is_control_in_both_directions() {
    let (mut client, schema) = test_client();
    let mut samples = Vec::with_capacity(PARITY_BATCH_SIZE);
    let mut expected = Vec::with_capacity(PARITY_BATCH_SIZE);

    for scalar in (1u32..=0x0010_FFFF).filter_map(char::from_u32) {
        if is_default_ignorable(scalar) {
            continue;
        }

        samples.push(format!("a{scalar}b"));
        expected.push(!scalar.is_control());

        if samples.len() == PARITY_BATCH_SIZE {
            assert_validity_batch(&mut client, &samples, &expected);
            samples.clear();
            expected.clear();
        }
    }

    if !samples.is_empty() {
        assert_validity_batch(&mut client, &samples, &expected);
    }

    client
        .batch_execute(&format!("DROP SCHEMA {schema} CASCADE;"))
        .expect("isolated parity schema must be removable");
}

#[test]
fn postgres_numeric_multirange_matches_rust_is_numeric_in_both_directions() {
    let (mut client, schema) = test_client();
    let mut samples = Vec::with_capacity(PARITY_BATCH_SIZE);
    let mut expected = Vec::with_capacity(PARITY_BATCH_SIZE);

    for scalar in (1u32..=0x0010_FFFF).filter_map(char::from_u32) {
        if scalar.is_control() || scalar.is_whitespace() || is_default_ignorable(scalar) {
            continue;
        }

        samples.push(scalar.to_string());
        expected.push(!scalar.is_numeric());

        if samples.len() == PARITY_BATCH_SIZE {
            assert_validity_batch(&mut client, &samples, &expected);
            samples.clear();
            expected.clear();
        }
    }

    if !samples.is_empty() {
        assert_validity_batch(&mut client, &samples, &expected);
    }

    client
        .batch_execute(&format!("DROP SCHEMA {schema} CASCADE;"))
        .expect("isolated parity schema must be removable");
}

#[test]
fn postgres_default_ignorable_multirange_matches_rust_boundary_in_both_directions() {
    let (mut client, schema) = test_client();
    let mut samples = Vec::with_capacity(PARITY_BATCH_SIZE);
    let mut expected = Vec::with_capacity(PARITY_BATCH_SIZE);

    // Embed each scalar between ordinary letters. This keeps numeric and edge-whitespace rules
    // from deciding the outcome, so every non-control default-ignorable omission or extra range is
    // observable as a PostgreSQL/Rust parity mismatch. U+0000 is excluded because PostgreSQL text
    // cannot contain it.
    for scalar in (1u32..=0x0010_FFFF).filter_map(char::from_u32) {
        samples.push(format!("a{scalar}b"));
        expected.push(!scalar.is_control() && !is_default_ignorable(scalar));

        if samples.len() == PARITY_BATCH_SIZE {
            assert_validity_batch(&mut client, &samples, &expected);
            samples.clear();
            expected.clear();
        }
    }

    if !samples.is_empty() {
        assert_validity_batch(&mut client, &samples, &expected);
    }

    client
        .batch_execute(&format!("DROP SCHEMA {schema} CASCADE;"))
        .expect("isolated parity schema must be removable");
}
