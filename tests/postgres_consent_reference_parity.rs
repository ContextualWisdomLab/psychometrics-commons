//! `PostgreSQL` consent references must enforce the same opaque-reference boundary as Rust.
//!
//! The domain rejects surrounding Unicode whitespace, embedded control characters, and values
//! whose complete spelling is numeric-like under Rust `char::is_numeric`. Direct SQL writes must
//! not be able to persist evidence that the Rust domain would reject or later classify as corrupt.

use postgres::{error::SqlState, Client, NoTls};
use psychometrics_commons_runtime::postgres_consent::apply_consent_migration;
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
