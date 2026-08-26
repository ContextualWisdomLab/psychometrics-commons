//! `PostgreSQL` consent references must enforce the same opaque-reference boundary as Rust.
//!
//! The domain rejects surrounding Unicode whitespace, embedded control characters, and values
//! whose complete spelling is numeric-like under Rust `char::is_numeric`. Direct SQL writes must
//! not be able to persist evidence that the Rust domain would reject or later classify as corrupt.

use postgres::{error::SqlState, Client, NoTls};
use psychometrics_commons_runtime::postgres_consent::apply_consent_migration;
use std::sync::{Mutex, MutexGuard};

static TEST_LOCK: Mutex<()> = Mutex::new(());
const MIGRATION: &str = include_str!("../migrations/0005_consent_lifecycle.sql");
const REFERENCE_SOURCE: &str = include_str!("../src/reference.rs");

fn guard() -> MutexGuard<'static, ()> {
    TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn client() -> Client {
    let url = std::env::var("TEST_DATABASE_URL").expect("TEST_DATABASE_URL is required");
    let mut client = Client::connect(&url, NoTls).expect("CI PostgreSQL must be reachable");
    client
        .batch_execute(
            "DROP SCHEMA IF EXISTS consent_reference_parity_test CASCADE; \
             CREATE SCHEMA consent_reference_parity_test; \
             SET search_path TO consent_reference_parity_test;",
        )
        .unwrap();
    apply_consent_migration(&mut client).unwrap();
    client
}

fn assert_check(error: &postgres::Error, constraint: &str) {
    let database_error = error
        .as_db_error()
        .expect("reference rejection must come from a PostgreSQL CHECK constraint");
    assert_eq!(database_error.code(), &SqlState::CHECK_VIOLATION);
    assert_eq!(database_error.constraint(), Some(constraint));
}

fn insert_event(
    client: &mut Client,
    event_ref: &str,
    consent_form_version_ref: &str,
    purpose: &str,
    research_scope_ref: Option<&str>,
) -> Result<u64, postgres::Error> {
    client.execute(
        "INSERT INTO consent_event (\
             participant_ref, event_ref, consent_purpose, consent_decision, \
             consent_form_version_ref, research_scope_ref, occurred_at_unix_ms\
         ) VALUES ($1,$2,$3,'granted',$4,$5,1000)",
        &[
            &"participant_reference_parity",
            &event_ref,
            &purpose,
            &consent_form_version_ref,
            &research_scope_ref,
        ],
    )
}

fn migration_numeric_ranges() -> Vec<(u32, u32)> {
    const RANGE_PREFIX: &str = "ascii(character_text) <@ '";
    const RANGE_SUFFIX: &str = "'::int4multirange";

    let after_prefix = MIGRATION
        .split_once(RANGE_PREFIX)
        .expect("consent migration must declare the Rust numeric multirange")
        .1;
    let literal = after_prefix
        .split_once(RANGE_SUFFIX)
        .expect("consent migration numeric multirange must use int4multirange")
        .0;
    let body = literal
        .strip_prefix('{')
        .and_then(|value| value.strip_suffix('}'))
        .expect("consent migration numeric multirange must use canonical braces");

    body.split("),")
        .map(|range| {
            let range = range
                .strip_prefix('[')
                .expect("numeric multirange entries must be inclusive-exclusive ranges");
            let range = range.strip_suffix(')').unwrap_or(range);
            let (start, end) = range
                .split_once(',')
                .expect("numeric multirange entries must have start and end bounds");
            (
                start.parse().expect("numeric range start must be u32"),
                end.parse().expect("numeric range end must be u32"),
            )
        })
        .collect()
}

fn migration_default_ignorable_ranges() -> Vec<(u32, u32)> {
    const RANGE_PREFIX: &str = "ascii(character_text) <@ '";
    const RANGE_SUFFIX: &str = "'::int4multirange";

    let after_prefix = MIGRATION
        .split(RANGE_PREFIX)
        .nth(2)
        .expect("consent migration must declare the default-ignorable multirange");
    let literal = after_prefix
        .split_once(RANGE_SUFFIX)
        .expect("consent migration default-ignorable multirange must use int4multirange")
        .0;
    let body = literal
        .strip_prefix('{')
        .and_then(|value| value.strip_suffix('}'))
        .expect("consent migration default-ignorable multirange must use canonical braces");

    body.split("),")
        .map(|range| {
            let range = range
                .strip_prefix('[')
                .expect("default-ignorable multirange entries must be inclusive-exclusive ranges");
            let range = range.strip_suffix(')').unwrap_or(range);
            let (start, end) = range
                .split_once(',')
                .expect("default-ignorable multirange entries must have start and end bounds");
            (
                start
                    .parse()
                    .expect("default-ignorable range start must be u32"),
                end.parse()
                    .expect("default-ignorable range end must be u32"),
            )
        })
        .collect()
}

fn rust_numeric_ranges() -> Vec<(u32, u32)> {
    let mut ranges = Vec::new();
    let mut range_start = None;
    let mut previous_numeric = None;

    for codepoint in 0..=0x10_FFFF {
        if char::from_u32(codepoint).is_some_and(char::is_numeric) {
            range_start.get_or_insert(codepoint);
            previous_numeric = Some(codepoint);
        } else if let (Some(start), Some(previous)) = (range_start.take(), previous_numeric.take())
        {
            ranges.push((start, previous + 1));
        }
    }
    if let (Some(start), Some(previous)) = (range_start, previous_numeric) {
        ranges.push((start, previous + 1));
    }
    ranges
}

fn codepoint_from_rust_unicode_literal(literal: &str) -> u32 {
    let prefix = literal
        .find("\\u{")
        .expect("default-ignorable Rust entry must use a Unicode escape")
        + 3;
    let suffix = literal[prefix..]
        .find('}')
        .expect("default-ignorable Rust Unicode escape must close")
        + prefix;
    u32::from_str_radix(&literal[prefix..suffix], 16)
        .expect("default-ignorable Rust Unicode escape must be hexadecimal")
}

fn rust_default_ignorable_ranges() -> Vec<(u32, u32)> {
    let function = REFERENCE_SOURCE
        .split_once("const fn is_default_ignorable_identifier_character")
        .expect("reference boundary must declare the pinned default-ignorable helper")
        .1;
    let patterns = function
        .split_once("#[cfg(test)]")
        .expect("reference boundary helper must precede its unit tests")
        .0;

    patterns
        .lines()
        .filter(|line| line.contains("\\u{"))
        .map(|line| {
            let pattern = line.trim().trim_start_matches('|').trim();
            if let Some((start, end)) = pattern.split_once("..=") {
                (
                    codepoint_from_rust_unicode_literal(start),
                    codepoint_from_rust_unicode_literal(end) + 1,
                )
            } else {
                let codepoint = codepoint_from_rust_unicode_literal(pattern);
                (codepoint, codepoint + 1)
            }
        })
        .collect()
}

#[test]
fn migration_numeric_ranges_exactly_match_pinned_rust_unicode_tables() {
    assert_eq!(migration_numeric_ranges(), rust_numeric_ranges());
}

#[test]
fn migration_default_ignorable_ranges_exactly_match_pinned_rust_boundary() {
    assert_eq!(
        migration_default_ignorable_ranges(),
        rust_default_ignorable_ranges()
    );
}

#[test]
fn migration_declares_and_runs_under_the_required_utf8_database_encoding() {
    let _guard = guard();
    let mut client = client();
    let encoding: String = client
        .query_one("SHOW server_encoding", &[])
        .unwrap()
        .get(0);
    assert_eq!(encoding, "UTF8");

    assert!(MIGRATION.contains("current_setting('server_encoding')"));
    assert!(MIGRATION.contains("consent reference migration requires UTF8 database encoding"));
}

#[test]
fn legacy_invalid_reference_blocks_upgrade_without_rewriting_consent_evidence() {
    let _guard = guard();
    let mut client = client();

    client
        .batch_execute(
            "ALTER TABLE consent_ledger \
                 DROP CONSTRAINT consent_ledger_participant_ref_format_check; \
             ALTER TABLE consent_ledger \
                 ADD CONSTRAINT consent_ledger_participant_ref_format_check \
                 CHECK (participant_ref <> '' AND participant_ref !~ '^[0-9]+$'); \
             INSERT INTO consent_ledger (participant_ref) VALUES ('½');",
        )
        .unwrap();

    let error = apply_consent_migration(&mut client)
        .expect_err("legacy Rust-invalid consent evidence must block constraint strengthening");
    let database_error = error
        .as_db_error()
        .expect("legacy-data preflight must return a PostgreSQL error");
    assert_eq!(database_error.code(), &SqlState::CHECK_VIOLATION);
    assert_eq!(
        database_error.message(),
        "consent reference migration blocked by legacy references outside the current opaque-reference contract"
    );
    assert!(database_error
        .detail()
        .is_some_and(|detail| detail.contains("ledger_participant_ref=1")));
    assert!(!database_error.message().contains('½'));
    assert!(!database_error.detail().unwrap_or_default().contains('½'));

    let preserved: String = client
        .query_one(
            "SELECT participant_ref FROM consent_ledger WHERE participant_ref = '½'",
            &[],
        )
        .unwrap()
        .get(0);
    assert_eq!(preserved, "½");

    client
        .execute(
            "DELETE FROM consent_ledger WHERE participant_ref = '½'",
            &[],
        )
        .unwrap();
    apply_consent_migration(&mut client).expect(
        "migration must be retryable after operator-adjudicated legacy evidence is removed",
    );

    let error = client
        .execute(
            "INSERT INTO consent_ledger (participant_ref) VALUES ('½')",
            &[],
        )
        .expect_err("strengthened constraint must reject the legacy alias after successful retry");
    assert_check(&error, "consent_ledger_participant_ref_format_check");
}

#[test]
fn participant_reference_rejects_unicode_numeric_whitespace_and_control_aliases() {
    let _guard = guard();
    let mut client = client();

    for invalid_ref in [
        "½",
        "²",
        "Ⅳ",
        "\u{00a0}participant_alpha",
        "participant_\u{0001}_alpha",
    ] {
        let error = client
            .execute(
                "INSERT INTO consent_ledger (participant_ref) VALUES ($1)",
                &[&invalid_ref],
            )
            .expect_err(
                "a direct SQL write must not bypass the Rust participant-reference boundary",
            );
        assert_check(&error, "consent_ledger_participant_ref_format_check");
    }
}

#[test]
fn event_form_and_research_scope_references_share_the_rust_boundary() {
    let _guard = guard();
    let mut client = client();
    client
        .execute(
            "INSERT INTO consent_ledger (participant_ref) VALUES ('participant_reference_parity')",
            &[],
        )
        .unwrap();

    for invalid_ref in ["½", "²", "Ⅳ", "\u{00a0}event_alpha", "event_\u{0001}_alpha"] {
        let error = insert_event(
            &mut client,
            invalid_ref,
            "consent_form_reference_parity",
            "service_operation",
            None,
        )
        .expect_err("event references must use the same opaque-reference boundary as Rust");
        assert_check(&error, "consent_event_event_ref_format_check");
    }

    for invalid_ref in ["½", "²", "Ⅳ", "\u{00a0}form_alpha", "form_\u{0001}_alpha"] {
        let error = insert_event(
            &mut client,
            "consent_event_reference_parity",
            invalid_ref,
            "service_operation",
            None,
        )
        .expect_err("consent-form references must use the same opaque-reference boundary as Rust");
        assert_check(&error, "consent_event_form_ref_format_check");
    }

    for invalid_ref in ["½", "²", "Ⅳ", "\u{00a0}scope_alpha", "scope_\u{0001}_alpha"] {
        let error = insert_event(
            &mut client,
            "research_event_reference_parity",
            "research_form_reference_parity",
            "research_contribution",
            Some(invalid_ref),
        )
        .expect_err(
            "research-scope references must use the same opaque-reference boundary as Rust",
        );
        assert_check(&error, "consent_event_research_scope_format_check");
    }
}
