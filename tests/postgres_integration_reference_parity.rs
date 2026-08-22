//! `PostgreSQL` integration references must enforce the same opaque-reference boundary as Rust.
//!
//! Integration evidence can be inserted by recovery/operator paths as well as normal Rust code.
//! The physical schema must therefore reject every reference spelling that the domain rejects,
//! including Unicode numeric-only values, surrounding Unicode whitespace, and controls.

use postgres::{error::SqlState, Client, NoTls};
use psychometrics_commons_runtime::postgres_integration::apply_integration_migration;
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
fn database_numeric_set_matches_rust_is_numeric_for_every_unicode_scalar() {
    let _guard = guard();
    let mut client = client();
    let numeric_references: Vec<String> = (0..=char::MAX as u32)
        .filter_map(char::from_u32)
        .filter(|character| character.is_numeric())
        .map(|character| character.to_string())
        .collect();

    assert!(!numeric_references.is_empty());
    let accepted_numeric_count: i64 = client
        .query_one(
            "SELECT count(*) \
             FROM unnest($1::text[]) AS candidate(reference_text) \
             WHERE integration_reference_is_valid(reference_text)",
            &[&numeric_references],
        )
        .expect("the parity probe must execute against the migrated validator")
        .get(0);

    assert_eq!(
        accepted_numeric_count, 0,
        "every single-character reference Rust classifies as numeric must be rejected by PostgreSQL"
    );
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
