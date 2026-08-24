//! Real `PostgreSQL` bounds for durable result-snapshot rows.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::postgres_result_snapshot::apply_result_snapshot_migration;
use std::sync::{Mutex, MutexGuard};

const DATABASE_TEST_LOCK_KEY: i64 = 0x5253_534E_4150_434B;
static RESULT_SNAPSHOT_SCHEMA_LOCK: Mutex<()> = Mutex::new(());

fn schema_test_guard() -> MutexGuard<'static, ()> {
    RESULT_SNAPSHOT_SCHEMA_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
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
fn fixed_schema_serialization_must_be_visible_to_other_database_sessions() {
    let _guard = schema_test_guard();
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let mut contender = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    let acquired: bool = contender
        .query_one("SELECT pg_try_advisory_lock($1)", &[&DATABASE_TEST_LOCK_KEY])
        .expect("cross-process fixture lock should be observable from PostgreSQL")
        .get(0);
    if acquired {
        contender
            .query_one("SELECT pg_advisory_unlock($1)", &[&DATABASE_TEST_LOCK_KEY])
            .expect("RED fixture lock should be released after probing");
    }
    assert!(
        !acquired,
        "a process-local mutex cannot serialize a fixed PostgreSQL schema across CI processes"
    );
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
