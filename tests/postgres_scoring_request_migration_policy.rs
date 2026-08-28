//! Real `PostgreSQL` contracts for scoring-request reference-policy upgrades.
//!
//! The immutable reference validator is consumed by CHECK constraints. `PostgreSQL` does not
//! automatically revalidate historical rows when such a function is replaced, so the migration
//! must derive its validation marker from the live validator definition. Concurrent migration
//! reapplies must also serialize the marker decision before either caller rebuilds constraints.

use postgres::{error::SqlState, Client, NoTls};
use psychometrics_commons_runtime::postgres_scoring_request::apply_scoring_request_migration;
use std::sync::{Mutex, MutexGuard};

const SCHEMA: &str = "scoring_request_migration_policy_test";
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
fn concurrent_reapply_waits_on_the_policy_table_lock() {
    let _guard = fixture_test_guard();
    let mut setup = connect();
    prepare_schema(&mut setup);

    let mut holder = connect();
    holder
        .batch_execute(&format!(
            "SET search_path TO {SCHEMA}; \
             BEGIN; \
             LOCK TABLE scoring_request IN ACCESS EXCLUSIVE MODE;"
        ))
        .expect("the test holder must acquire the scoring-request policy table lock");

    let mut contender = connect();
    contender
        .batch_execute(&format!(
            "SET search_path TO {SCHEMA}; SET statement_timeout = '250ms';"
        ))
        .unwrap();
    let error = apply_scoring_request_migration(&mut contender)
        .expect_err("a concurrent reapply must wait on the policy table lock");
    assert_eq!(
        error.code(),
        Some(&SqlState::QUERY_CANCELED),
        "the bounded wait must end at the serialization boundary, not another migration failure"
    );

    holder.batch_execute("ROLLBACK").unwrap();
    contender
        .batch_execute("SET statement_timeout = 0")
        .unwrap();
    apply_scoring_request_migration(&mut contender)
        .expect("migration reapply must succeed after the competing policy transaction releases");
}
