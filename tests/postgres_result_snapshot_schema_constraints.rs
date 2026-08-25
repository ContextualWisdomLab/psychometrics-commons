//! Real `PostgreSQL` bounds for durable result-snapshot rows.

use postgres::{error::SqlState, Client, NoTls};
use psychometrics_commons_runtime::postgres_result_snapshot::apply_result_snapshot_migration;

const RESULT_SNAPSHOT_SCHEMA_LOCK_KEY: i64 = 0x5253_5343_4845_4D41;

fn acquire_schema_lock(
    client: &mut Client,
    lock_key: i64,
    lock_timeout: &str,
) -> Result<(), postgres::Error> {
    client.query_one(
        "SELECT set_config('lock_timeout', $1, false)",
        &[&lock_timeout],
    )?;
    client.query_one("SELECT pg_advisory_lock($1)", &[&lock_key])?;
    Ok(())
}

fn schema_test_guard() -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let mut guard = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    acquire_schema_lock(&mut guard, RESULT_SNAPSHOT_SCHEMA_LOCK_KEY, "60s")
        .expect("shared result-snapshot schema test lock should be acquired within sixty seconds");
    guard
}

fn test_client() -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let mut client = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    client
        .batch_execute(
            "CREATE SCHEMA IF NOT EXISTS result_snapshot_schema_test;\
             SET search_path TO result_snapshot_schema_test;",
        )
        .unwrap();
    client
}

fn reset_schema(client: &mut Client) {
    client
        .batch_execute(
            "DROP TABLE IF EXISTS result_snapshot_schema_test.result_snapshot_observation;\
             DROP TABLE IF EXISTS result_snapshot_schema_test.result_snapshot;",
        )
        .unwrap();
}

fn constraint_name(error: &postgres::Error) -> String {
    error
        .as_db_error()
        .and_then(postgres::error::DbError::constraint)
        .unwrap_or_default()
        .to_owned()
}

const SNAPSHOT_COLUMNS: &str =
    "result_snapshot_ref, participant_ref, scoring_result_ref, session_ref, \
     response_snapshot_ref, assessment_spec_ref, instrument_version_ref, \
     scoring_version_ref, calibration_reference, norm_version_ref, \
     requested_output_schema_version, narrative_version_ref, \
     consent_snapshot_refs, engine_artifact_digest, created_at_unix_ms";

const VALID_ENGINE_DIGEST: &str =
    "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const VALID_SNAPSHOT_VALUES: &str = "'result_snapshot_valid', 'participant_result_one', \
     'scoring_result_result_one', 'session_result_one', \
     'response_snapshot_result_one', 'assessment_spec_big_five_v1', \
     'instrument_version_big_five_ko_v1', 'scoring_version_big_five_v1', \
     'calibration_big_five_ko_v1', NULL, 1, 'narrative_version_big_five_v1', \
     ARRAY['consent_snapshot_service_v1'], \
     'sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', 70000";

#[test]
fn result_snapshot_schema_guard_is_visible_to_another_postgres_session() {
    let _guard = schema_test_guard();
    let mut contender = test_client();
    let acquired: bool = contender
        .query_one(
            "SELECT pg_try_advisory_lock($1)",
            &[&RESULT_SNAPSHOT_SCHEMA_LOCK_KEY],
        )
        .expect("contender lock probe should succeed")
        .get(0);

    assert!(
        !acquired,
        "fixed-schema result-snapshot fixture guard must serialize across PostgreSQL sessions"
    );
}

#[test]
fn result_snapshot_schema_guard_has_finite_postgresql_wait_budget() {
    let mut guard = schema_test_guard();
    let timeout_ms: i64 = guard
        .query_one(
            "SELECT setting::bigint FROM pg_settings WHERE name = 'lock_timeout'",
            &[],
        )
        .expect("result-snapshot schema lock timeout should be queryable from PostgreSQL")
        .get(0);

    assert_eq!(
        timeout_ms, 60_000,
        "result-snapshot schema fixture must not wait indefinitely for its advisory lock"
    );
}

#[test]
fn result_snapshot_schema_lock_wait_aborts_under_real_contention() {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let mut holder = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    let behavior_lock_key: i64 = holder
        .query_one("SELECT pg_backend_pid()::bigint", &[])
        .expect("holder backend identity should be queryable")
        .get(0);
    holder
        .query_one("SELECT pg_advisory_lock($1)", &[&behavior_lock_key])
        .expect("behavior-test holder should acquire its private advisory lock");

    let mut contender = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    let error = acquire_schema_lock(&mut contender, behavior_lock_key, "100ms")
        .expect_err("contended result-snapshot schema lock must stop at the configured timeout");
    assert_eq!(error.code(), Some(&SqlState::LOCK_NOT_AVAILABLE));

    let released: bool = holder
        .query_one("SELECT pg_advisory_unlock($1)", &[&behavior_lock_key])
        .expect("behavior-test advisory lock should be released")
        .get(0);
    assert!(released, "behavior-test advisory lock should be released");
}

#[test]
fn schema_rejects_numeric_identity_empty_consent_and_self_supersession() {
    let _guard = schema_test_guard();
    let mut client = test_client();
    reset_schema(&mut client);
    apply_result_snapshot_migration(&mut client).unwrap();

    let numeric = client
        .execute(
            &format!(
                "INSERT INTO result_snapshot ({SNAPSHOT_COLUMNS}) VALUES ({})",
                VALID_SNAPSHOT_VALUES.replace("'result_snapshot_valid'", "'12'")
            ),
            &[],
        )
        .unwrap_err();
    assert_eq!(
        constraint_name(&numeric),
        "result_snapshot_ref_format_check"
    );

    let empty_consent = client
        .execute(
            &format!(
                "INSERT INTO result_snapshot ({SNAPSHOT_COLUMNS}) VALUES ({})",
                VALID_SNAPSHOT_VALUES
                    .replace("ARRAY['consent_snapshot_service_v1']", "ARRAY[]::TEXT[]")
            ),
            &[],
        )
        .unwrap_err();
    assert_eq!(
        constraint_name(&empty_consent),
        "result_snapshot_consent_refs_not_empty_check"
    );

    let self_supersede = client
        .execute(
            &format!(
                "INSERT INTO result_snapshot ({SNAPSHOT_COLUMNS}, supersedes_ref) VALUES ({}, '{}')",
                VALID_SNAPSHOT_VALUES.replace("'result_snapshot_valid'", "'result_snapshot_self'"),
                "result_snapshot_self"
            ),
            &[],
        )
        .unwrap_err();
    assert_eq!(
        constraint_name(&self_supersede),
        "result_snapshot_supersedes_ref_format_check"
    );
}

#[test]
fn schema_rejects_noncanonical_engine_digest_and_nonfinite_score_evidence() {
    let _guard = schema_test_guard();
    let mut client = test_client();
    reset_schema(&mut client);
    apply_result_snapshot_migration(&mut client).unwrap();

    let invalid_digest = client
        .execute(
            &format!(
                "INSERT INTO result_snapshot ({SNAPSHOT_COLUMNS}) VALUES ({})",
                VALID_SNAPSHOT_VALUES.replace(VALID_ENGINE_DIGEST, "sha256:not-a-digest")
            ),
            &[],
        )
        .unwrap_err();
    assert_eq!(
        constraint_name(&invalid_digest),
        "result_snapshot_engine_digest_format_check"
    );

    client
        .execute(
            &format!(
                "INSERT INTO result_snapshot ({SNAPSHOT_COLUMNS}) VALUES ({VALID_SNAPSHOT_VALUES})"
            ),
            &[],
        )
        .unwrap();

    let nan_score = client
        .execute(
            "INSERT INTO result_snapshot_observation (\
                 result_snapshot_ref, observation_order, construct_ref, \
                 observation_disposition, score, standard_error\
             ) VALUES (\
                 'result_snapshot_valid', 0, 'construct_nan_score', 'scored', \
                 'NaN'::double precision, NULL\
             )",
            &[],
        )
        .unwrap_err();
    assert_eq!(
        constraint_name(&nan_score),
        "result_snapshot_observation_score_finite_check"
    );

    let infinite_standard_error = client
        .execute(
            "INSERT INTO result_snapshot_observation (\
                 result_snapshot_ref, observation_order, construct_ref, \
                 observation_disposition, score, standard_error\
             ) VALUES (\
                 'result_snapshot_valid', 1, 'construct_infinite_error', 'scored', \
                 0.5, 'Infinity'::double precision\
             )",
            &[],
        )
        .unwrap_err();
    assert_eq!(
        constraint_name(&infinite_standard_error),
        "result_snapshot_observation_standard_error_shape_check"
    );
}

#[test]
fn schema_rejects_scored_observation_without_score() {
    let _guard = schema_test_guard();
    let mut client = test_client();
    reset_schema(&mut client);
    apply_result_snapshot_migration(&mut client).unwrap();

    client
        .execute(
            &format!(
                "INSERT INTO result_snapshot ({SNAPSHOT_COLUMNS}) VALUES ({VALID_SNAPSHOT_VALUES})"
            ),
            &[],
        )
        .unwrap();
    let scored_without_score = client
        .execute(
            "INSERT INTO result_snapshot_observation (\
                 result_snapshot_ref, observation_order, construct_ref, \
                 observation_disposition, score, standard_error\
             ) VALUES (\
                 'result_snapshot_valid', 0, 'construct_big_five', 'scored', NULL, NULL\
             )",
            &[],
        )
        .unwrap_err();
    assert_eq!(
        constraint_name(&scored_without_score),
        "result_snapshot_observation_score_shape_check"
    );
}
