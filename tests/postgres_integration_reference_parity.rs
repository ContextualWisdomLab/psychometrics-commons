//! `PostgreSQL` integration references must enforce the same opaque-reference boundary as Rust.
//!
//! Integration evidence can be inserted by recovery/operator paths as well as normal Rust code.
//! The physical schema must therefore reject every reference spelling that the domain rejects,
//! including Unicode numeric-only values, surrounding Unicode whitespace, controls, and invisible
//! or display-altering Unicode default-ignorable characters.

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

fn assert_check_constraint(error: &postgres::Error, expected_constraint: &str) {
    assert_check(error);
    let database_error = error
        .as_db_error()
        .expect("reference rejection must expose the violated CHECK constraint");
    assert_eq!(
        database_error.constraint(),
        Some(expected_constraint),
        "the invalid reference must fail on its own column constraint"
    );
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

fn insert_inbox(
    client: &mut Client,
    consumer_ref: &str,
    source_ref: &str,
    tenant_ref: &str,
    source_event_ref: &str,
    subject_ref: &str,
) -> Result<u64, postgres::Error> {
    client.execute(
        "INSERT INTO integration_inbox (\
             consumer_ref, source_ref, tenant_ref, source_event_ref, event_type, schema_version, \
             subject_ref, payload_digest, received_at_unix_ms\
         ) VALUES ($1,$2,$3,$4,'result.released','v1',$5,\
                   'sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb',\
                   1200)",
        &[
            &consumer_ref,
            &source_ref,
            &tenant_ref,
            &source_event_ref,
            &subject_ref,
        ],
    )
}

fn reference_is_valid(client: &mut Client, reference: &str) -> bool {
    client
        .query_one("SELECT integration_reference_is_valid($1)", &[&reference])
        .expect("the migrated reference validator must be callable")
        .get(0)
}

fn constraint_oid(client: &mut Client, constraint_name: &str) -> i64 {
    client
        .query_one(
            "SELECT constraint_row.oid::bigint \
             FROM pg_catalog.pg_constraint AS constraint_row \
             JOIN pg_catalog.pg_class AS relation_row \
               ON relation_row.oid = constraint_row.conrelid \
             JOIN pg_catalog.pg_namespace AS namespace_row \
               ON namespace_row.oid = relation_row.relnamespace \
             WHERE namespace_row.nspname = current_schema() \
               AND relation_row.relname = 'integration_outbox' \
               AND constraint_row.conname = $1",
            &[&constraint_name],
        )
        .expect("the migrated outbox constraint must exist")
        .get(0)
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
        "PostgreSQL reference classification diverged from Rust for {mismatches:?}"
    );
}

fn is_default_ignorable_identifier_character(character: char) -> bool {
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
fn outbox_identity_rejects_unicode_numeric_whitespace_control_and_default_ignorable_aliases() {
    let _guard = guard();
    let mut client = client();

    for invalid_ref in [
        "½",
        "²",
        "Ⅳ",
        "\u{00a0}event_alpha",
        "event_\u{0001}_alpha",
        "event_\u{00ad}_alpha",
        "event_\u{200b}_alpha",
        "event_\u{202e}_alpha",
        "event_\u{2060}_alpha",
        "event_\u{fe0f}_alpha",
        "event_\u{e0001}_alpha",
    ] {
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
            !character.is_numeric()
                && !character.is_whitespace()
                && !character.is_control()
                && !is_default_ignorable_identifier_character(character),
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
fn database_validator_rejects_numeric_literal_spellings_but_preserves_mixed_references() {
    let _guard = guard();
    let mut client = client();

    for invalid_ref in ["-1.5", "1e5", "1\u{066b}2", "１．５"] {
        assert!(
            !reference_is_valid(&mut client, invalid_ref),
            "numeric-like reference {invalid_ref:?} must be rejected"
        );
    }

    for valid_ref in ["e", "event-1", "v1.2"] {
        assert!(
            reference_is_valid(&mut client, valid_ref),
            "mixed opaque reference {valid_ref:?} must remain valid"
        );
    }
}

#[test]
fn nullable_outbox_causation_and_delivery_cause_fail_closed_only_when_present_and_invalid() {
    let _guard = guard();
    let mut client = client();

    insert_outbox(&mut client, "event_null_causation")
        .expect("NULL causation_ref must remain valid for an otherwise valid outbox row");

    let invalid_causation = client
        .execute(
            "INSERT INTO integration_outbox (\
                 event_ref, event_type, schema_version, source_ref, tenant_ref, subject_ref, \
                 occurred_at_unix_ms, correlation_ref, causation_ref, payload_digest, \
                 max_attempts, current_state, latest_event_at_unix_ms\
             ) VALUES ('event_invalid_causation','result.released','v1','psychometrics_commons',\
                       'tenant_alpha','result_alpha',1000,'correlation_alpha',$1,\
                       'sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',\
                       3,'pending',1000)",
            &[&"1e5"],
        )
        .expect_err("a present invalid causation_ref must fail closed");
    assert_check(&invalid_causation);

    insert_outbox(&mut client, "event_delivery_cause").unwrap();
    client
        .execute(
            "INSERT INTO integration_delivery_attempt (\
                 source_ref, tenant_ref, event_ref, attempt_ref, delivery_outcome, \
                 occurred_at_unix_ms, cause_code\
             ) VALUES ('psychometrics_commons','tenant_alpha','event_delivery_cause',\
                       'attempt_null_cause','retryable_failure',1100,NULL)",
            &[],
        )
        .expect("NULL cause_code must remain valid for an otherwise valid delivery attempt");

    let invalid_cause = client
        .execute(
            "INSERT INTO integration_delivery_attempt (\
                 source_ref, tenant_ref, event_ref, attempt_ref, delivery_outcome, \
                 occurred_at_unix_ms, cause_code\
             ) VALUES ('psychometrics_commons','tenant_alpha','event_delivery_cause',\
                       'attempt_invalid_cause','retryable_failure',1101,$1)",
            &[&"１．５"],
        )
        .expect_err("a present invalid cause_code must fail closed");
    assert_check(&invalid_cause);
}

#[test]
fn every_inbox_reference_column_enforces_the_shared_reference_boundary() {
    let _guard = guard();
    let mut client = client();

    let cases = [
        (
            "integration_inbox_consumer_ref_check",
            "1e5",
            "source_alpha",
            "tenant_alpha",
            "event_alpha",
            "subject_alpha",
        ),
        (
            "integration_inbox_source_ref_check",
            "consumer_alpha",
            "1e5",
            "tenant_alpha",
            "event_alpha",
            "subject_alpha",
        ),
        (
            "integration_inbox_tenant_ref_check",
            "consumer_alpha",
            "source_alpha",
            "1e5",
            "event_alpha",
            "subject_alpha",
        ),
        (
            "integration_inbox_source_event_ref_check",
            "consumer_alpha",
            "source_alpha",
            "tenant_alpha",
            "1e5",
            "subject_alpha",
        ),
        (
            "integration_inbox_subject_ref_check",
            "consumer_alpha",
            "source_alpha",
            "tenant_alpha",
            "event_alpha",
            "1e5",
        ),
    ];

    for (
        expected_constraint,
        consumer_ref,
        source_ref,
        tenant_ref,
        source_event_ref,
        subject_ref,
    ) in cases
    {
        let error = insert_inbox(
            &mut client,
            consumer_ref,
            source_ref,
            tenant_ref,
            source_event_ref,
            subject_ref,
        )
        .expect_err("every inbox identity/reference column must reject numeric-like aliases");
        assert_check_constraint(&error, expected_constraint);
    }
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

        let inbox_error = insert_inbox(
            &mut client,
            invalid_ref,
            "external_source",
            "tenant_alpha",
            "source_event_alpha",
            "result_alpha",
        )
        .expect_err("consumer references must preserve the Rust reference boundary");
        assert_check(&inbox_error);
    }
}

#[test]
fn migration_reapplication_preserves_canonical_reference_constraints() {
    let _guard = guard();
    let mut client = client();
    let constraint_name = "integration_outbox_event_ref_check";
    let first_oid = constraint_oid(&mut client, constraint_name);

    apply_integration_migration(&mut client).unwrap();

    let second_oid = constraint_oid(&mut client, constraint_name);
    assert_eq!(
        second_oid, first_oid,
        "an unchanged migration must not drop and revalidate a canonical CHECK constraint"
    );
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

#[test]
fn migration_reapplication_fails_closed_on_historical_default_ignorable_identity() {
    let _guard = guard();
    let mut client = client();

    client
        .batch_execute(
            "ALTER TABLE integration_outbox DROP CONSTRAINT integration_outbox_event_ref_check; \
             ALTER TABLE integration_outbox ADD CONSTRAINT integration_outbox_event_ref_check \
             CHECK (event_ref = btrim(event_ref) AND event_ref <> '');",
        )
        .unwrap();
    insert_outbox(&mut client, "event_\u{200b}_historical")
        .expect("the weakened historical check demonstrates the invisible-identity gap");

    let error = apply_integration_migration(&mut client)
        .expect_err("migration reapplication must block until unsafe historical identity is remediated");
    assert_eq!(
        error
            .as_db_error()
            .expect("migration failure must come from PostgreSQL constraint revalidation")
            .code(),
        &SqlState::CHECK_VIOLATION
    );
}
