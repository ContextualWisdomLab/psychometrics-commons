//! Exhaustive real-PostgreSQL parity for Rust numeric-only scoring references.
//!
//! The SQL validator carries a generated Unicode numeric multirange so direct SQL cannot admit a
//! numeric-only identity that Rust `char::is_numeric` rejects at the product boundary. These tests
//! check both directions against the toolchain's complete scalar set instead of relying on
//! representative examples.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::postgres_scoring_request::apply_scoring_request_migration;
use std::sync::{Mutex, MutexGuard};

const SCHEMA_LOCK_KEY: i64 = 0x5343_4F52_4E55_4D50;
static NUMERIC_PARITY_TEST_LOCK: Mutex<()> = Mutex::new(());

fn numeric_parity_test_guard() -> MutexGuard<'static, ()> {
    NUMERIC_PARITY_TEST_LOCK
        .lock()
        .expect("scoring numeric-parity test lock must not be poisoned")
}

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

fn is_numeric_syntax_separator(character: char) -> bool {
    matches!(
        character,
        '+' | '-' | '.' | ',' | 'e' | 'E' | '\u{066B}' | '\u{066C}' | '\u{FF0E}' | '\u{FF0C}'
    )
}

fn is_default_ignorable(character: char) -> bool {
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

#[test]
fn sql_rejects_every_single_scalar_rust_classifies_as_numeric() {
    let _guard = numeric_parity_test_guard();
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

#[test]
fn sql_does_not_overclassify_visible_rust_nonnumeric_scalars() {
    let _guard = numeric_parity_test_guard();
    let mut client = client();
    let rust_visible_nonnumeric: Vec<i32> = (1u32..=0x0010_FFFF)
        .filter_map(char::from_u32)
        .filter(|character| {
            !character.is_numeric()
                && !character.is_control()
                && !character.is_whitespace()
                && !is_default_ignorable(*character)
                && !is_numeric_syntax_separator(*character)
        })
        .map(|character| {
            i32::try_from(u32::from(character)).expect("Unicode scalar values always fit in i32")
        })
        .collect();

    assert!(
        !rust_visible_nonnumeric.is_empty(),
        "the pinned Rust toolchain must expose visible nonnumeric Unicode scalars"
    );

    let rejected: Vec<i32> = client
        .query(
            "SELECT codepoint \
             FROM unnest($1::int4[]) AS codepoint \
             WHERE NOT scoring_request_reference_is_valid('1' || chr(codepoint)) \
             LIMIT 100",
            &[&rust_visible_nonnumeric],
        )
        .expect("PostgreSQL must not overclassify visible Rust-nonnumeric scalars")
        .into_iter()
        .map(|row| row.get(0))
        .collect();

    assert!(
        rejected.is_empty(),
        "SQL overclassified visible Rust-nonnumeric scalars in numeric-shaped references: {rejected:?}"
    );
}
