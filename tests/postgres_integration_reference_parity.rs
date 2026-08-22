//! `PostgreSQL` integration references must enforce the same opaque-reference boundary as Rust.
//!
//! Integration evidence can be inserted by recovery/operator paths as well as normal Rust code.
//! The physical schema must therefore reject every reference spelling that the domain rejects,
//! including Unicode numeric-only values, surrounding Unicode whitespace, and controls.

use postgres::{error::SqlState, Client, NoTls};
use psychometrics_commons_runtime::postgres_integration::apply_integration_migration;
use std::sync::{Mutex, MutexGuard};

static TEST_LOCK: Mutex<()> = Mutex::new(());
const SCALAR_PARITY_BATCH_SIZE: usize = 32_768;

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
            "DROP SCHEMA IF EXISTS integration_reference_parity_test CASCADE; \
             CREATE SCHEMA integration_reference_parity_test; \
             SET search_path TO integration_reference_parity_test;",
        )
        .unwrap();
    apply_integration_migration(&mut client).unwrap();
    client
}

fn assert_check(error: &postgres::Error) {
    let database_error = error
        .as_db_error()
        .expect("reference rejection must come from a PostgreSQL CHECK constraint");
    assert_eq!(database_error.code(), &SqlState::CHECK_VIOLATION);
}

fn insert_outbox(client: &mut Client, event_ref: &str) -> Result<u64, postgres::Error> {
    client.execute(
        "INSERT INTO integration_outbox (\
             event_ref, event_type, schema_version, source_ref, tenant_ref, subject_ref, \
             occurred_at_unix_ms, correlation_ref, causation_ref, payload_digest, \
             max_attempts, current_state, latest_event_at_unix_ms\
         ) VALUES ($1,'result.released','v1','psychometrics_commons','tenant_alpha',\
                   'result_alpha',1000,'correlation_alpha',NULL,\
                   'sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',\
                   3,'pending',1000)",
        &[&event_ref],
    )
}

fn assert_scalar_parity_batch(client: &mut Client, references: &[String], expected: &[bool]) {
    let mismatches: Vec<String> = client
        .query_one(
            "SELECT COALESCE(array_agg(reference_text), ARRAY[]::text[]) \
             FROM (\
                 SELECT reference_text \
                 FROM unnest($1::text[], $2::boolean[]) \
                      AS candidate(reference_text, expected_valid) \
                 WHERE integration_reference_is_valid(reference_text) \
                       IS DISTINCT FROM expected_valid \
                 LIMIT 8\
             ) AS mismatch",
            &[&references, &expected],
        )
        .expect("the scalar parity probe must execute against the migrated validator")
        .get(0);

    assert!(
        mismatches.is_empty(),
        "PostgreSQL reference classification diverged from Rust for {:?}",
        mismatches
    );
}

#[test]
fn outbox_identity_rejects_unicode_numeric_whitespace_and_control_aliases() {
    let _guard = guard();
    let mut client = client();

    for invalid_ref in ["½", "²", "Ⅳ", "\u{00a0}event_alpha", "event_\u{0001}_alpha"] {
        let error = insert_outbox(&mut client, invalid_ref)
            .expect_err("direct SQL must not bypass the Rust outbox reference boundary");
        assert_check(&error);
    }
}

#[test]
fn database_validator_matches_rust_scalar_classes_exhaustively() {
    let _guard = guard();
    let mut client = client();
    let mut references = Vec::with_capacity(SCALAR_PARITY_BATCH_SIZE);
    let mut expected = Vec::with_capacity(SCALAR_PARITY_BATCH_SIZE);
    let mut checked_scalars = 0usize;

    for character in (0..=char::MAX as u32).filter_map(char::from_u32) {
        if character == '\0' {
            // PostgreSQL text cannot represent U+0000. The validator's embedded-control
            // behavior is exercised with representable control scalars below and in the
            // focused outbox test.
            continue;
        }
        references.push(character.to_string());
        expected.push(
            !character.is_numeric() && !character.is_whitespace() && !character.is_control(),
        );
        checked_scalars += 1;

        if references.len() == SCALAR_PARITY_BATCH_SIZE {
            assert_scalar_parity_batch(&mut client, &references, &expected);
            references.clear();
            expected.clear();
        }
    }

    if !references.is_empty() {
        assert_scalar_parity_batch(&mut client, &references, &expected);
    }
    assert!(checked_scalars > 1_000_000);
}

#[test]
fn delivery_attempt_and_inbox_identity_reject_unicode_numeric_aliases() {
    let _guard = guard();
    let mut client = client();
    insert_outbox(&mut client, "event_reference_parity").unwrap();

    for invalid_ref in ["½", "²", "Ⅳ"] {
        let attempt_error = client
            .execute(
                "INSERT INTO integration_delivery_attempt (\
                     source_ref, tenant_ref, event_ref, attempt_ref, delivery_outcome, \
                     occurred_at_unix_ms, cause_code\
                 ) VALUES ('psychometrics_commons','tenant_alpha','event_reference_parity',$1,\
                           'retryable_failure',1100,NULL)",
                &[&invalid_ref],
            )
            .expect_err("attempt references must preserve the Rust reference boundary");
        assert_check(&attempt_error);

        let inbox_error = client
            .execute(
                "INSERT INTO integration_inbox (\
                     consumer_ref, source_ref, tenant_ref, source_event_ref, event_type, \
                     schema_version, subject_ref, payload_digest, received_at_unix_ms\
                 ) VALUES ($1,'external_source','tenant_alpha','source_event_alpha',\
                           'result.released','v1','result_alpha',\
                           'sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb',\
                           1200)",
                &[&invalid_ref],
            )
            .expect_err("consumer references must preserve the Rust reference boundary");
        assert_check(&inbox_error);
    }
}

#[test]
fn migration_reapplication_repairs_a_weaker_event_reference_check() {
    let _guard = guard();
    let mut client = client();

    client
        .batch_execute(
            "ALTER TABLE integration_outbox DROP CONSTRAINT integration_outbox_event_ref_check; \
             ALTER TABLE integration_outbox ADD CONSTRAINT integration_outbox_event_ref_check \
             CHECK (event_ref = btrim(event_ref) AND event_ref <> '');",
        )
        .unwrap();
    insert_outbox(&mut client, "½").expect("the weakened historical check demonstrates the gap");
    client
        .execute("DELETE FROM integration_outbox WHERE event_ref = '½'", &[])
        .unwrap();

    apply_integration_migration(&mut client).unwrap();
    let error = insert_outbox(&mut client, "½")
        .expect_err("reapplying the product migration must repair the weaker check");
    assert_check(&error);
}
