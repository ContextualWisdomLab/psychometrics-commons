//! Real `PostgreSQL` contracts for scoring-request reference-policy upgrades.
//!
//! The immutable reference validator is consumed by CHECK constraints. `PostgreSQL` does not
//! automatically revalidate historical rows when such a function is replaced, so the migration
//! must derive its validation marker from the live validator definition. Concurrent migration
//! reapplies must also serialize the marker decision before either caller rebuilds constraints.

use postgres::{error::SqlState, Client, NoTls};
use psychometrics_commons_runtime::postgres_scoring_request::apply_scoring_request_migration;

const MIGRATION_LOCK_KEY: i64 = 5_999_726_343_356_564_817;
const SCHEMA: &str = "scoring_request_migration_policy_test";

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
fn concurrent_reapply_waits_on_the_migration_policy_lock() {
    let mut setup = connect();
    prepare_schema(&mut setup);

    let mut holder = connect();
    holder
        .batch_execute(&format!("SET search_path TO {SCHEMA}; BEGIN;"))
        .unwrap();
    holder
        .query_one("SELECT pg_advisory_xact_lock($1)", &[&MIGRATION_LOCK_KEY])
        .expect("the test holder must acquire the scoring-request migration policy lock");

    let mut contender = connect();
    contender
        .batch_execute(&format!(
            "SET search_path TO {SCHEMA}; SET statement_timeout = '250ms';"
        ))
        .unwrap();
    let error = apply_scoring_request_migration(&mut contender)
        .expect_err("a concurrent reapply must wait on the migration policy lock");
    assert_eq!(
        error.code(),
        Some(&SqlState::QUERY_CANCELED),
        "the bounded wait must end at the advisory-lock boundary, not another migration failure"
    );

    holder.batch_execute("ROLLBACK").unwrap();
    contender.batch_execute("SET statement_timeout = 0").unwrap();
    apply_scoring_request_migration(&mut contender)
        .expect("migration reapply must succeed after the competing policy transaction releases");
}
