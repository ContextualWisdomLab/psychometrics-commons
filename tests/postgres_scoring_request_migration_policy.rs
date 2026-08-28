//! Real `PostgreSQL` contracts for scoring-request reference-policy upgrades.
//!
//! The immutable reference validator is consumed by CHECK constraints. `PostgreSQL` does not
//! automatically revalidate historical rows when such a function is replaced, so the migration
//! must derive its validation marker from the live validator and owned CHECK manifest. Concurrent
//! migration reapplies must serialize that marker decision without taking an exclusive table lock
//! when the already-installed policy is unchanged.

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
fn reference_policy_marker_is_derived_from_live_validator_and_constraints() {
    let _guard = fixture_test_guard();
    let mut client = connect();
    prepare_schema(&mut client);

    let row = client
        .query_one(
            "WITH constraint_manifest AS (\
                 SELECT string_agg(\
                     format(\
                         '%s:%s:%s:%s:%s', \
                         constraint_record.conname, \
                         constraint_record.contype, \
                         constraint_record.convalidated, \
                         constraint_record.conenforced, \
                         pg_get_constraintdef(constraint_record.oid)\
                     ), \
                     E'\\n' ORDER BY constraint_record.conname\
                 ) AS manifest \
                 FROM pg_constraint AS constraint_record \
                 WHERE constraint_record.conrelid = 'scoring_request'::regclass \
                   AND constraint_record.conname = ANY (ARRAY[\
                       'scoring_request_scoring_request_ref_format_check', \
                       'scoring_request_session_ref_format_check', \
                       'scoring_request_response_snapshot_ref_format_check', \
                       'scoring_request_assessment_spec_ref_format_check', \
                       'scoring_request_instrument_version_ref_format_check', \
                       'scoring_request_scoring_version_ref_format_check', \
                       'scoring_request_calibration_reference_format_check', \
                       'scoring_request_norm_version_ref_format_check'\
                   ])\
             ) \
             SELECT \
                 obj_description(\
                     'scoring_request_reference_is_valid(text)'::regprocedure, \
                     'pg_proc'\
                 ), \
                 'psychometrics-commons:scoring-request-reference:' || \
                     md5(\
                         pg_get_functiondef(\
                             'scoring_request_reference_is_valid(text)'::regprocedure\
                         ) || E'\\n' || COALESCE(constraint_manifest.manifest, '')\
                     ) \
             FROM constraint_manifest",
            &[],
        )
        .expect("scoring-request reference policy marker must be inspectable");
    let actual_marker: Option<String> = row.get(0);
    let expected_marker: String = row.get(1);
    assert_eq!(
        actual_marker.as_deref(),
        Some(expected_marker.as_str()),
        "validator or owned CHECK edits must invalidate stale validation evidence"
    );
}

#[test]
fn reapply_repairs_weakened_reference_constraint_without_marker_reset() {
    let _guard = fixture_test_guard();
    let mut client = connect();
    prepare_schema(&mut client);

    let marker_before: Option<String> = client
        .query_one(
            "SELECT obj_description(\
                 'scoring_request_reference_is_valid(text)'::regprocedure, \
                 'pg_proc'\
             )",
            &[],
        )
        .expect("installed reference-policy marker must be readable")
        .get(0);
    client
        .batch_execute(
            "ALTER TABLE scoring_request \
                 DROP CONSTRAINT scoring_request_scoring_request_ref_format_check; \
             ALTER TABLE scoring_request \
                 ADD CONSTRAINT scoring_request_scoring_request_ref_format_check \
                 CHECK (char_length(scoring_request_ref) > 0);",
        )
        .expect("fixture must be able to simulate constraint drift without touching the marker");
    let marker_after_drift: Option<String> = client
        .query_one(
            "SELECT obj_description(\
                 'scoring_request_reference_is_valid(text)'::regprocedure, \
                 'pg_proc'\
             )",
            &[],
        )
        .expect("constraint drift must leave the function marker readable")
        .get(0);
    assert_eq!(
        marker_after_drift, marker_before,
        "the regression must preserve the stale marker while weakening only a CHECK constraint"
    );

    apply_scoring_request_migration(&mut client)
        .expect("reapply must repair constraint drift even when the function marker is unchanged");
    let repaired_definition: String = client
        .query_one(
            "SELECT pg_get_constraintdef(oid) \
             FROM pg_constraint \
             WHERE conrelid = 'scoring_request'::regclass \
               AND conname = 'scoring_request_scoring_request_ref_format_check'",
            &[],
        )
        .expect("repaired reference constraint must exist")
        .get(0);
    assert!(
        repaired_definition.contains("scoring_request_reference_is_valid(scoring_request_ref)"),
        "reapply must replace a weakened same-name constraint with the canonical validator"
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
