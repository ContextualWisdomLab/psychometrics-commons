//! Real `PostgreSQL` contracts for scoring-request reference-policy upgrades.
//!
//! The immutable reference validator is consumed by CHECK constraints. `PostgreSQL` does not
//! automatically revalidate historical rows when such a function is replaced, so the migration
//! must derive its validation marker from the live validator definition. Concurrent migration
//! reapplies must serialize that marker decision without taking an exclusive table lock when the
//! already-installed policy is unchanged.

use postgres::{error::SqlState, Client, NoTls};
use psychometrics_commons_runtime::postgres_scoring_request::apply_scoring_request_migration;
use std::sync::{Mutex, MutexGuard};

const SCHEMA: &str = "scoring_request_migration_policy_test";
const POLICY_LOCK_CLASS_ID: i32 = 1_883_264_113;
static FIXTURE_TEST_LOCK: Mutex<()> = Mutex::new(());

fn fixture_test_guard() -> MutexGuard<'static, ()> {
    FIXTURE_TEST_LOCK
        .lock()
        .expect("scoring-request migration fixture test lock must not be poisoned")
}

fn connect() -> Client {
    let url = std::env::var("TEST_DATABASE_URL").expect("TEST_DATABASE_URL is required");
    Client::connect(&url, NoTls).expect("CI PostgreSQL must be reachable")
}

fn prepare_schema(client: &mut Client) {
    client
        .batch_execute(&format!(
            "DROP SCHEMA IF EXISTS {SCHEMA} CASCADE; \
             CREATE SCHEMA {SCHEMA}; \
             SET search_path TO {SCHEMA};"
        ))
        .unwrap();
    apply_scoring_request_migration(client).unwrap();
}

#[test]
fn migration_checks_utf8_before_installing_unicode_reference_validator() {
    let migration = include_str!("../migrations/0011_scoring_request.sql");
    let encoding_guard = migration
        .find("current_setting('server_encoding') <> 'UTF8'")
        .expect("migration must fail closed when PostgreSQL server encoding is not UTF8");
    let validator = migration
        .find("CREATE OR REPLACE FUNCTION scoring_request_reference_is_valid")
        .expect("migration must install the scoring-request reference validator");

    assert!(
        encoding_guard < validator,
        "UTF8 must be verified before ascii(substr(...)) is used as a Unicode code-point oracle"
    );
    assert!(migration.contains(
        "scoring_request reference parity requires PostgreSQL server_encoding UTF8"
    ));
}

#[test]
fn reference_policy_marker_is_derived_from_live_validator_definition() {
    let _guard = fixture_test_guard();
    let mut client = connect();
    prepare_schema(&mut client);

    let row = client
        .query_one(
            "SELECT \
                 obj_description(\
                     'scoring_request_reference_is_valid(text)'::regprocedure, \
                     'pg_proc'\
                 ), \
                 'psychometrics-commons:scoring-request-reference:' || \
                     md5(pg_get_functiondef(\
                         'scoring_request_reference_is_valid(text)'::regprocedure\
                     ))",
            &[],
        )
        .expect("scoring-request reference policy marker must be inspectable");
    let actual_marker: Option<String> = row.get(0);
    let expected_marker: String = row.get(1);
    assert_eq!(
        actual_marker.as_deref(),
        Some(expected_marker.as_str()),
        "validator edits must automatically invalidate stale CHECK validation evidence"
    );
}

#[test]
fn concurrent_reapply_waits_on_the_schema_policy_lock() {
    let _guard = fixture_test_guard();
    let mut setup = connect();
    prepare_schema(&mut setup);

    let mut holder = connect();
    holder
        .batch_execute(&format!("SET search_path TO {SCHEMA}; BEGIN;"))
        .unwrap();
    holder
        .query_one(
            "SELECT pg_advisory_xact_lock($1, hashtext(current_schema()))",
            &[&POLICY_LOCK_CLASS_ID],
        )
        .expect("the test holder must acquire the schema-scoped policy lock");

    let mut contender = connect();
    contender
        .batch_execute(&format!(
            "SET search_path TO {SCHEMA}; SET statement_timeout = '250ms';"
        ))
        .unwrap();
    let error = apply_scoring_request_migration(&mut contender)
        .expect_err("a concurrent reapply must wait on the schema policy lock");
    assert_eq!(
        error.code(),
        Some(&SqlState::QUERY_CANCELED),
        "the bounded wait must end at the policy serialization boundary"
    );

    holder.batch_execute("ROLLBACK").unwrap();
    contender
        .batch_execute("SET statement_timeout = 0")
        .unwrap();
    apply_scoring_request_migration(&mut contender)
        .expect("migration reapply must succeed after the competing policy transaction releases");
}

#[test]
fn unchanged_reapply_does_not_block_an_active_reader() {
    let _guard = fixture_test_guard();
    let mut setup = connect();
    prepare_schema(&mut setup);

    let mut reader = connect();
    reader
        .batch_execute(&format!("SET search_path TO {SCHEMA}; BEGIN;"))
        .unwrap();
    reader
        .query_one("SELECT count(*) FROM scoring_request", &[])
        .expect("reader must acquire and retain an ordinary table read lock");

    let mut reapplier = connect();
    reapplier
        .batch_execute(&format!(
            "SET search_path TO {SCHEMA}; SET statement_timeout = '500ms';"
        ))
        .unwrap();
    apply_scoring_request_migration(&mut reapplier).expect(
        "an unchanged policy reapply must not require ACCESS EXCLUSIVE while readers are active",
    );

    reader.batch_execute("ROLLBACK").unwrap();
}
