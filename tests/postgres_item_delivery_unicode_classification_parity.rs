//! Exhaustive boundary evidence for PostgreSQL whitespace/control classification.
//!
//! Item-delivery persistence relies on PostgreSQL 18 `pg_unicode_fast` character
//! classes while the product boundary relies on Rust `str::trim` / `char::is_control`.
//! These contracts pin the load-bearing Unicode classes so direct SQL cannot admit
//! opaque references the Rust domain rejects.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::postgres_item_delivery::apply_item_delivery_migration;
use std::sync::{Mutex, MutexGuard};

static TEST_LOCK: Mutex<()> = Mutex::new(());

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
            "DROP SCHEMA IF EXISTS item_delivery_unicode_classification_test CASCADE; \
             CREATE SCHEMA item_delivery_unicode_classification_test; \
             SET search_path TO item_delivery_unicode_classification_test;",
        )
        .unwrap();
    apply_item_delivery_migration(&mut client).unwrap();
    client
}

fn first_accepted(client: &mut Client, aliases: &[String]) -> Option<String> {
    let refs = aliases.iter().map(String::as_str).collect::<Vec<_>>();
    client
        .query_opt(
            "SELECT candidate \
             FROM unnest($1::text[]) AS unsafe(candidate) \
             WHERE item_delivery_reference_is_valid(candidate) \
             LIMIT 1",
            &[&refs],
        )
        .unwrap()
        .map(|row| row.get::<_, String>(0))
}

#[test]
fn sql_predicate_rejects_every_c1_control_rejected_by_rust() {
    let _guard = guard();
    let mut client = client();
    let aliases = (0x80_u32..=0x9f)
        .map(|codepoint| {
            let character = char::from_u32(codepoint).expect("C1 value must be a Unicode scalar");
            assert!(character.is_control(), "fixture must match Rust char::is_control");
            format!("opaque_{character}_alpha")
        })
        .collect::<Vec<_>>();

    assert!(
        first_accepted(&mut client, &aliases).is_none(),
        "PostgreSQL pg_unicode_fast [[:cntrl:]] must reject every C1 control Rust rejects"
    );
}

#[test]
fn sql_predicate_rejects_every_outer_unicode_whitespace_trimmed_by_rust() {
    let _guard = guard();
    let mut client = client();
    let whitespace = [
        '\u{0009}', '\u{000a}', '\u{000b}', '\u{000c}', '\u{000d}', '\u{0020}', '\u{0085}',
        '\u{00a0}', '\u{1680}', '\u{2000}', '\u{2001}', '\u{2002}', '\u{2003}', '\u{2004}',
        '\u{2005}', '\u{2006}', '\u{2007}', '\u{2008}', '\u{2009}', '\u{200a}', '\u{2028}',
        '\u{2029}', '\u{202f}', '\u{205f}', '\u{3000}',
    ];
    let mut aliases = Vec::with_capacity(whitespace.len() * 2);
    for character in whitespace {
        assert!(character.is_whitespace(), "fixture must match Rust Unicode whitespace");
        let leading = format!("{character}opaque_alpha");
        let trailing = format!("opaque_alpha{character}");
        assert_eq!(leading.trim(), "opaque_alpha");
        assert_eq!(trailing.trim(), "opaque_alpha");
        aliases.push(leading);
        aliases.push(trailing);
    }

    assert!(
        first_accepted(&mut client, &aliases).is_none(),
        "PostgreSQL pg_unicode_fast [[:space:]] must reject every outer Unicode whitespace alias Rust trims"
    );
}

#[test]
fn sql_predicate_preserves_visible_mixed_opaque_references() {
    let _guard = guard();
    let mut client = client();
    for reference in ["item_2", "opaque alpha 2", "release_3.1", "v1-2", "측정_버전_2"] {
        let accepted: bool = client
            .query_one(
                "SELECT item_delivery_reference_is_valid($1)",
                &[&reference],
            )
            .unwrap()
            .get(0);
        assert!(accepted, "visible mixed opaque reference must remain valid: {reference:?}");
    }
}
