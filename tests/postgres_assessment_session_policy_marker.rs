//! Guard assessment-session CHECK rebuilding when the validator itself changes.
//!
//! The migration avoids repeatedly rebuilding validated CHECK constraints. That optimization is
//! safe only when the marker stored on each constraint changes whenever the SQL validator changes.
//! This real-PostgreSQL contract derives the expected marker from `PostgreSQL`'s own normalized
//! function definition, so a future validator edit cannot silently keep a stale validation marker.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::postgres_assessment_session::apply_assessment_session_migration;

const REFERENCE_CONSTRAINTS: [&str; 5] = [
    "assessment_session_command_command_ref_format_check",
    "assessment_session_participant_ref_format_check",
    "assessment_session_release_ref_format_check",
    "assessment_session_session_ref_format_check",
    "assessment_session_version_ref_format_check",
];

#[test]
fn constraint_policy_marker_is_derived_from_the_live_validator_definition() {
    let url = std::env::var("TEST_DATABASE_URL").expect("TEST_DATABASE_URL is required");
    let mut client = Client::connect(&url, NoTls).expect("CI PostgreSQL must be reachable");
    client
        .batch_execute(
            "DROP SCHEMA IF EXISTS assessment_session_policy_marker_test CASCADE; \
             CREATE SCHEMA assessment_session_policy_marker_test; \
             SET search_path TO assessment_session_policy_marker_test;",
        )
        .unwrap();
    apply_assessment_session_migration(&mut client).unwrap();

    let constraint_names: Vec<String> = REFERENCE_CONSTRAINTS
        .iter()
        .map(|name| (*name).to_owned())
        .collect();
    let rows = client
        .query(
            "SELECT \
                 conname, \
                 obj_description(oid, 'pg_constraint'), \
                 'psychometrics-commons:assessment-session-reference:' || \
                     md5(pg_get_functiondef(\
                         'assessment_session_reference_is_valid(text)'::regprocedure\
                     )) AS expected_marker \
             FROM pg_constraint \
             WHERE conname = ANY($1::text[]) \
               AND connamespace = current_schema()::regnamespace \
               AND conrelid IN (\
                   'assessment_session'::regclass, \
                   'assessment_session_command'::regclass\
               ) \
             ORDER BY conname",
            &[&constraint_names],
        )
        .expect("assessment-session reference markers must be inspectable");

    assert_eq!(
        rows.len(),
        REFERENCE_CONSTRAINTS.len(),
        "every reference CHECK must carry the validator-derived policy marker"
    );
    for row in rows {
        let constraint_name: String = row.get(0);
        let actual_marker: Option<String> = row.get(1);
        let expected_marker: String = row.get(2);
        assert_eq!(
            actual_marker.as_deref(),
            Some(expected_marker.as_str()),
            "{constraint_name} must be invalidated automatically when the validator definition changes"
        );
    }
}
